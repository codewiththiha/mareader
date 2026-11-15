//! Caps checked before rbook reads a book, and the trees it would nest.
use std::collections::HashMap;
use std::io::{Cursor, Read, Seek};
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use std::fs::File;
#[cfg(not(target_arch = "wasm32"))]
use std::io::BufReader;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use rbook::Epub;
use zip::ZipArchive;

use crate::OpenError;

/// Files in one book; real books hold hundreds.
pub const MAX_ENTRIES: usize = 10_000;
/// One unpacked file, generous for a large picture.
pub const MAX_ENTRY: u64 = 128 << 20;
/// Everything unpacked together.
pub const MAX_TOTAL: u64 = 512 << 20;
/// Contents nesting rbook may build; deeper, the headings stand in.
pub const MAX_TOC_DEPTH: usize = 64;
/// Links in a metadata refinement chain; longer, metadata is skipped.
pub const MAX_REFINES: usize = 8;

/// The file's bytes, shared with the session that read them.
struct Shared(Arc<Vec<u8>>);

impl AsRef<[u8]> for Shared {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

/// Opens `bytes` with rbook, skipping what it would nest too deep.
pub(crate) fn open(bytes: &Arc<Vec<u8>>) -> Result<Epub, OpenError> {
    let checked = {
        let mut zip =
            ZipArchive::new(Cursor::new(bytes.as_slice())).map_err(|_| OpenError::Invalid)?;
        check_zip(&mut zip)?;
        inspect_zip(&mut zip)
    };
    let read = |toc: bool, metadata: bool| {
        let mut options = Epub::options();
        options.skip_toc(!toc).skip_metadata(!metadata);
        let book = Cursor::new(Shared(Arc::clone(bytes)));
        options.read(book).map_err(|_| OpenError::Invalid)
    };
    if let Some((toc, metadata)) = checked {
        // Contents or metadata rbook rejects needn't cost the book.
        return read(toc, metadata)
            .or_else(|_| read(false, metadata))
            .or_else(|_| read(false, false));
    }
    // rbook frees its contents and resolves refinements recursively.
    let bare = read(false, false)?;
    let package = bare.read_resource_bytes(bare.package().location());
    let package = Package::scan(&package.unwrap_or_default());
    let toc = package.toc_fits(&bare);
    let metadata = package.chains_fit;
    read(toc, metadata)
        .or_else(|_| read(false, metadata))
        .or(Ok(bare))
}

/// Opens an already-checked archive for manifest resource reads only.
pub(crate) fn open_manifest(bytes: &Arc<Vec<u8>>) -> Result<Epub, OpenError> {
    let mut options = Epub::options();
    options.skip_toc(true).skip_metadata(true).skip_spine(true);
    let book = Cursor::new(Shared(Arc::clone(bytes)));
    options.read(book).map_err(|_| OpenError::Invalid)
}

/// Streams a book's package metadata from `path` without loading it whole.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn open_file_metadata(path: &Path) -> Result<Epub, OpenError> {
    let chains = check_file_chains(path)?;
    let read = |metadata: bool, manifest: bool| {
        let mut options = Epub::options();
        options
            .skip_toc(true)
            .skip_spine(true)
            .skip_manifest(!manifest)
            .skip_metadata(!metadata);
        options.open(path).map_err(|_| OpenError::Invalid)
    };
    if let Some(metadata) = chains {
        return read(metadata, false).or_else(|_| read(false, false));
    }
    let bare = read(false, true)?;
    let package = bare.read_resource_bytes(bare.package().location());
    let metadata = Package::scan(&package.unwrap_or_default()).chains_fit;
    if metadata {
        read(true, false).or(Ok(bare))
    } else {
        Ok(bare)
    }
}

/// Streams a book from `path` for cover lookup without loading it whole.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn open_file_cover(path: &Path) -> Result<Epub, OpenError> {
    let chains = check_file_chains(path)?;
    let read = |metadata: bool| {
        let mut options = Epub::options();
        options.skip_toc(true).skip_metadata(!metadata);
        options.open(path).map_err(|_| OpenError::Invalid)
    };
    if let Some(metadata) = chains {
        return read(metadata).or_else(|_| read(false));
    }
    let bare = read(false)?;
    let package = bare.read_resource_bytes(bare.package().location());
    if Package::scan(&package.unwrap_or_default()).chains_fit {
        read(true).or(Ok(bare))
    } else {
        Ok(bare)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn check_file_chains(path: &Path) -> Result<Option<bool>, OpenError> {
    let file = File::open(path).map_err(|_| OpenError::Invalid)?;
    let mut zip = ZipArchive::new(BufReader::new(file)).map_err(|_| OpenError::Invalid)?;
    check_zip(&mut zip)?;
    Ok(scan_zip(&mut zip).map(|(_, pkg)| pkg.chains_fit))
}

fn check_zip<R: Read + Seek>(zip: &mut ZipArchive<R>) -> Result<(), OpenError> {
    if zip.len() > MAX_ENTRIES {
        return Err(OpenError::TooLarge);
    }
    let mut total = 0u64;
    for i in 0..zip.len() {
        let size = zip.by_index_raw(i).map_err(|_| OpenError::Invalid)?.size();
        total = total.saturating_add(size);
        if size > MAX_ENTRY || total > MAX_TOTAL {
            return Err(OpenError::TooLarge);
        }
    }
    Ok(())
}

fn read_entry<R: Read + Seek>(zip: &mut ZipArchive<R>, path: &str) -> Option<Vec<u8>> {
    let entry = zip.by_name(path.trim_start_matches('/')).ok()?;
    let mut out = Vec::new();
    entry.take(MAX_ENTRY + 1).read_to_end(&mut out).ok()?;
    (out.len() as u64 <= MAX_ENTRY).then_some(out)
}

fn rootfile(xml: &[u8]) -> Option<String> {
    let mut reader = Reader::from_reader(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) if e.local_name().as_ref() == "rootfile" => {
                if let Some(path) = value(&e, "full-path") {
                    return Some(path);
                }
            }
            Ok(Event::Eof) | Err(_) => return None,
            Ok(_) => {}
        }
    }
}

fn scan_zip<R: Read + Seek>(zip: &mut ZipArchive<R>) -> Option<(String, Package)> {
    let container = read_entry(zip, "META-INF/container.xml")?;
    let opf = rootfile(&container)?;
    let xml = read_entry(zip, &opf)?;
    Some((opf, Package::scan(&xml)))
}

fn inspect_zip<R: Read + Seek>(zip: &mut ZipArchive<R>) -> Option<(bool, bool)> {
    let (opf, pkg) = scan_zip(zip)?;
    let toc = pkg.toc_fits_zip(zip, &opf)?;
    Some((toc, pkg.chains_fit))
}

/// What the package file says, read as rbook reads it.
struct Package {
    /// The spine's NCX, by manifest id.
    ncx: Option<String>,
    nav_hrefs: Vec<String>,
    items: HashMap<String, String>,
    chains_fit: bool,
}

impl Package {
    fn scan(xml: &[u8]) -> Self {
        let mut reader = Reader::from_reader(xml);
        let mut ncx = None;
        let mut nav_hrefs = Vec::new();
        let mut items = HashMap::new();
        // Each refining element's id and the id it refines.
        let mut refines = HashMap::new();
        loop {
            match reader.read_event() {
                Ok(Event::Start(e) | Event::Empty(e)) => {
                    let (id, target) = (value(&e, "id"), value(&e, "refines"));
                    match e.local_name().as_ref() {
                        "spine" => ncx = value(&e, "toc"),
                        "item" => {
                            if let Some(href) = value(&e, "href") {
                                let is_nav = value(&e, "properties").is_some_and(|p| {
                                    p.split_ascii_whitespace().any(|w| w == "nav")
                                });
                                if is_nav {
                                    nav_hrefs.push(href.clone());
                                }
                                if let Some(id) = id.as_ref() {
                                    items.insert(id.clone(), href);
                                }
                            }
                        }
                        _ => {}
                    }
                    if let (Some(id), Some(target)) = (id, target) {
                        let target = target.strip_prefix('#').unwrap_or(&target);
                        refines.insert(id, target.to_owned());
                    }
                }
                Ok(Event::Eof) | Err(_) => break,
                Ok(_) => {}
            }
        }
        let chain = |id: &String| {
            let mut at = id;
            let mut links = 0;
            while let Some(next) = refines.get(at) {
                links += 1;
                if links > MAX_REFINES {
                    break;
                }
                at = next;
            }
            links
        };
        Self {
            ncx,
            nav_hrefs,
            items,
            chains_fit: refines.keys().all(|id| chain(id) <= MAX_REFINES),
        }
    }

    fn toc_fits_zip<R: Read + Seek>(&self, zip: &mut ZipArchive<R>, opf: &str) -> Option<bool> {
        let base = format!("/{}", opf.trim_start_matches('/'));
        let ncx = self.ncx.as_deref().and_then(|id| self.items.get(id));
        for href in self.nav_hrefs.iter().chain(ncx) {
            let path = crate::href::resolve(&base, href)?;
            let xml = read_entry(zip, &path)?;
            if toc_depth(&xml) > MAX_TOC_DEPTH {
                return Some(false);
            }
        }
        Some(true)
    }

    /// Whether every contents file rbook would read nests shallowly.
    fn toc_fits(&self, bare: &Epub) -> bool {
        let manifest = bare.manifest();
        let navs = manifest
            .iter()
            .filter(|e| e.properties().has_property("nav"));
        let ncx = self.ncx.as_deref().and_then(|id| manifest.by_id(id));
        let mut files = navs.chain(ncx);
        // One that won't read can't be measured, so it's skipped.
        files.all(|e| {
            e.read_bytes()
                .is_ok_and(|xml| toc_depth(&xml) <= MAX_TOC_DEPTH)
        })
    }
}

/// The deepest rbook's contents stack gets, counted as it counts.
fn toc_depth(xml: &[u8]) -> usize {
    let nests = |e: &str| {
        let names = ["nav", "li", "navMap", "pageList", "navPoint", "pageTarget"];
        names.contains(&e)
    };
    let mut reader = Reader::from_reader(xml);
    let (mut depth, mut deepest) = (0usize, 0);
    while deepest <= MAX_TOC_DEPTH {
        match reader.read_event() {
            // A self-closed entry is pushed and never popped.
            Ok(Event::Start(e) | Event::Empty(e)) if nests(e.local_name().as_ref()) => {
                depth += 1;
                deepest = deepest.max(depth);
            }
            Ok(Event::End(e)) if nests(e.local_name().as_ref()) => {
                depth = depth.saturating_sub(1);
            }
            Ok(Event::Eof) | Err(_) => break,
            Ok(_) => {}
        }
    }
    deepest
}

/// Attribute `key`'s last value, unescaped if it can be, as rbook reads it.
fn value(e: &BytesStart, key: &str) -> Option<String> {
    let mut attributes = e.attributes();
    attributes.with_checks(false);
    let raw = attributes
        .flatten()
        .filter(|a| a.key.as_ref() == key)
        .last()?
        .value;
    let unescaped = quick_xml::escape::unescape(&raw).map(|v| v.into_owned());
    Some(unescaped.unwrap_or_else(|_| raw.into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contents_depth_counts_what_rbook_stacks() {
        let nav = "<nav><ol><li><a/><ol><li/><li/></ol></li></ol></nav>";
        // nav, li, then two self-closed items never popped.
        assert_eq!(toc_depth(nav.as_bytes()), 4);
        let ncx = "<ncx><navMap><navPoint><navPoint/></navPoint></navMap></ncx>";
        assert_eq!(toc_depth(ncx.as_bytes()), 3);
        // A mismatched close ends the read, as it ends rbook's.
        let stray = "<nav><li></x><li><li></nav>";
        assert_eq!(toc_depth(stray.as_bytes()), 2);
    }

    #[test]
    fn refinement_chains_are_followed_and_cycles_end() {
        let meta = |id: usize, of: usize| format!("<meta id=\"m{id}\" refines=\"#m{of}\"/>");
        let chain = |n: usize| (1..=n).map(|i| meta(i, i - 1)).collect::<String>();
        let opf =
            |m: String| format!("<package><metadata>{m}</metadata><spine toc=\"ncx\"/></package>");
        let fits = Package::scan(opf(chain(MAX_REFINES)).as_bytes());
        assert!(fits.chains_fit);
        assert_eq!(fits.ncx.as_deref(), Some("ncx"));
        assert!(!Package::scan(opf(chain(MAX_REFINES + 1)).as_bytes()).chains_fit);
        let cycle = format!("{}{}", meta(1, 2), meta(2, 1));
        assert!(!Package::scan(opf(cycle).as_bytes()).chains_fit);
        // Escaped ids link as rbook links them, closing this cycle.
        let escaped = "<meta id=\"a&#x31;\" refines=\"#b\"/><meta id=\"b\" refines=\"#a1\"/>";
        assert!(!Package::scan(opf(escaped.to_owned()).as_bytes()).chains_fit);
    }
}
