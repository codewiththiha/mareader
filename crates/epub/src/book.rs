//! One EPUB: its spine parsed and laid out in resumable steps.
use std::collections::{BTreeMap, HashMap};
use std::ops::Range;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, OnceLock};

#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

use mareader_reflow::{
    Anchor, Block, BookFace, Content, Flow, Fonts, InteractiveItem, ItemPixmap, PageLine, Picture,
    Pixels, Region, Role, Typography,
};
use rbook::Epub;
use rbook::ebook::element::Properties;
use rbook::epub::manifest::EpubManifestEntry;

use crate::OpenError;
use crate::css::{FontFace, MAX_SHEET, Stylesheet};
use crate::fontfile;
use crate::guard::{self, manifest_by_path, read_resource};
use crate::media::{self, Media};
use crate::overlay::{self, Narration};
use crate::script::{self, Scripted};
use crate::splice::{At, Insert, Inserts, Spliced, next_layout};
use crate::subtitles::{self, Subtitle, Track, Wanted};
use crate::trigger::{self, Action, Triggered};
use crate::xhtml::{self, BOOK_FACE, Href, Library, Refs};

mod retype;

/// Transcripts' insert ids: the top bit, then the card.
const TRANSCRIPTS: u128 = 1 << 127;

/// All chapters' markup together; past it the book is refused.
pub const MAX_MARKUP: usize = 64 << 20;
/// Blocks in one book; each keeps its line boxes while open.
pub const MAX_BLOCKS: usize = 1 << 20;
/// Encoded image bytes kept for one book; past it, alt text shows.
pub const MAX_PICTURE_BYTES: usize = 256 << 20;

/// Table of contents entries kept; a longer one is cut.
pub const MAX_TOC: usize = 20_000;
/// An entry's title is cut to this many characters.
const MAX_TITLE: usize = 200;
/// Bytes read of one chapter's overlay file.
const MAX_OVERLAY: usize = 16 << 20;

/// A table of contents entry, or a heading when the book lists none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TocEntry {
    pub title: String,
    /// 0 for the top level.
    pub depth: usize,
    pub at: Anchor,
}

/// Where a link leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destination {
    Page(usize),
    /// A footnote: shown where its mark is, its page to go to.
    Note {
        page: usize,
        text: String,
    },
    /// An address outside the book, its scheme not yet checked.
    Outside(String),
    /// Sound or video in the book, by its path, for [`Book::media`].
    Media {
        path: String,
        video: bool,
    },
    /// Shows or hides clip card `card`'s transcript, in `track` if given.
    Transcript {
        card: usize,
        track: Option<usize>,
    },
    /// What a click sets off, for [`Book::trigger`].
    Trigger(usize),
    /// Element `node` of scripted chapter `chapter`, for its scripts.
    Script {
        chapter: usize,
        node: u32,
    },
}

/// A link's box on one line of a page, in page units.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    /// Left, top, width, height.
    pub rect: [f32; 4],
    pub to: Destination,
}

/// The archive's images and stylesheets, each read once per path.
struct Shelf<'e> {
    epub: &'e Epub,
    read: &'e mut Read,
}

/// What a shelf has read, kept to read chapters again as triggers change.
#[derive(Debug, Default, Clone)]
struct Read {
    pictures: HashMap<String, Option<Arc<Picture>>>,
    sheets: HashMap<String, Option<Arc<Stylesheet>>>,
    files: HashMap<String, Option<Arc<Vec<u8>>>>,
    budget: usize,
}

impl<'e> Shelf<'e> {
    fn on(epub: &'e Epub, read: &'e mut Read) -> Self {
        Self { epub, read }
    }
}

impl Shelf<'_> {
    /// The picture at `path` itself, its fallbacks aside.
    fn decode(&mut self, path: &str) -> Option<Arc<Picture>> {
        let bytes = read_resource(self.epub, path);
        let bytes = bytes.filter(|b| b.len() <= self.read.budget);
        bytes.and_then(|b| {
            self.read.budget -= b.len();
            let picture = if Picture::is_svg(&b) {
                let links = xhtml::svg_links(self, path, Picture::svg_links(&b));
                Picture::svg(b, links)
            } else {
                Picture::probe(b)
            };
            picture.ok().map(Arc::new)
        })
    }
}

impl Library for Shelf<'_> {
    /// Pictures that won't decode give way to their manifest fallbacks.
    fn picture(&mut self, path: &str) -> Option<Arc<Picture>> {
        if let Some(known) = self.read.pictures.get(path) {
            return known.clone();
        }
        let mut picture = self.decode(path);
        if picture.is_none() {
            let paths: Vec<String> = match manifest_by_path(self.epub, path) {
                Some(e) => (e.fallbacks().take(media::MAX_FALLBACKS))
                    .map(|f| f.href().decode().into_owned())
                    .collect(),
                None => Vec::new(),
            };
            picture = paths.iter().find_map(|p| self.decode(p));
        }
        self.read.pictures.insert(path.to_owned(), picture.clone());
        picture
    }

    fn media_type(&mut self, path: &str) -> Option<String> {
        let entry = manifest_by_path(self.epub, path)?;
        Some(entry.media_type().to_owned())
    }

    fn bytes(&mut self, path: &str) -> Option<Arc<Vec<u8>>> {
        if let Some(known) = self.read.files.get(path) {
            return known.clone();
        }
        let bytes = read_resource(self.epub, path);
        let bytes = bytes.filter(|b| b.len() <= self.read.budget).map(|b| {
            self.read.budget -= b.len();
            Arc::new(b)
        });
        self.read.files.insert(path.to_owned(), bytes.clone());
        bytes
    }

    fn stylesheet(&mut self, path: &str) -> Option<Arc<Stylesheet>> {
        if let Some(known) = self.read.sheets.get(path) {
            return known.clone();
        }
        let bytes = read_resource(self.epub, path);
        let sheet = bytes.filter(|b| b.len() <= MAX_SHEET).map(|b| {
            let text = String::from_utf8_lossy(&b);
            Arc::new(Stylesheet::parse(&text, path))
        });
        self.read.sheets.insert(path.to_owned(), sheet.clone());
        sheet
    }
}

/// A book's file, shared, to read its sound and video from.
#[derive(Clone)]
pub struct Files {
    archive: Arc<Vec<u8>>,
    epub: Arc<OnceLock<Option<Epub>>>,
}

impl std::fmt::Debug for Files {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Files({} bytes)", self.archive.len())
    }
}

impl Files {
    fn epub(&self) -> Option<&Epub> {
        self.epub
            .get_or_init(|| guard::open_manifest(&self.archive).ok())
            .as_ref()
    }

    /// Whether both come from one opening of a book, whatever its typesetting.
    pub fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.archive, &other.archive)
    }

    /// The sound or video at `path`, if the manifest names its type.
    pub fn media(&self, path: &str) -> Option<Media> {
        let read = || {
            let entry = manifest_by_path(self.epub()?, path)?;
            let extension = media::extension(entry.media_type())?;
            let data = entry.read_bytes().ok()?;
            Some(Media { data, extension })
        };
        catch_unwind(AssertUnwindSafe(read)).ok().flatten()
    }

    /// Any file the manifest lists at `path`, if not too large.
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        let read = || {
            let data = manifest_by_path(self.epub()?, path)?.read_bytes().ok()?;
            (data.len() <= subtitles::MAX_TRACK).then_some(data)
        };
        catch_unwind(AssertUnwindSafe(read)).ok().flatten()
    }
}

/// A stable EPUB location, relative to one spine item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpinePlace {
    pub href: String,
    pub item: usize,
    pub block: usize,
    pub offset: usize,
}

/// An open book. `Send + Sync`: workers share one through an `Arc`.
pub struct Book {
    identity: Arc<()>,
    blocks: Arc<Vec<Block>>,
    flow: Arc<Flow>,
    typo: Typography,
    layout: u64,
    inserts: Arc<Inserts>,
    spliced: Option<Arc<Spliced>>,
    refs: Arc<Refs>,
    spine: Arc<Vec<SpineRange>>,
    faces: Arc<Vec<BookFace>>,
    notes: Arc<HashMap<u32, String>>,
    toc: Arc<Vec<TocEntry>>,
    title: Option<String>,
    author: Option<String>,
    language: Option<String>,
    rtl: bool,
    lead: usize,
    archive: Option<Arc<Vec<u8>>>,
    narration: Arc<Narration>,
    read: Option<Arc<Read>>,
    complete: bool,
    windowed: bool,
}

const MAX_SPINE_HREF: usize = 4096;
const MAX_SPINE_ITEMS: usize = 1 << 16;
const MAX_PREVIEW_MARKUP: usize = 8 << 20;
const MAX_PREVIEW_BLOCKS: usize = 1 << 16;
const PREVIEW_NEIGHBORS: usize = 2;
const PREVIEW_EXTRA_NEIGHBORS: usize = 1;
const OPEN_BATCH_ITEMS: usize = 16;
/// A saved page or stable location to reach before opening the whole spine.
#[derive(Debug, Clone, Default)]
pub struct OpenTarget {
    pub page: usize,
    pub place: Option<Anchor>,
    pub spine: Option<SpinePlace>,
    pub pages: usize,
}

/// The result of one bounded parsing pass.
#[derive(Debug)]
pub enum OpenStep {
    Continue,
    Preview(Book),
    Complete(Book),
}

#[derive(Debug, Clone)]
struct SpineRecord {
    item: usize,
    id: Option<String>,
    href: Option<String>,
    overlay: Option<String>,
    fixed: bool,
}

#[derive(Debug, Clone)]
struct SpineRange {
    item: usize,
    href: String,
    blocks: Range<usize>,
}

struct ParsedItem {
    markup: usize,
    opening: Option<Range<usize>>,
    spine: Option<SpineRange>,
    pars: Vec<overlay::Par>,
}

struct ItemPass<'a> {
    epub: &'a Epub,
    record: &'a SpineRecord,
    markup_before: usize,
    markup_limit: usize,
    phrase_count: usize,
}

struct BookAssembly<'a> {
    epub: &'a Epub,
    bytes: &'a Arc<Vec<u8>>,
    identity: &'a Arc<()>,
    typo: &'a Typography,
    read: &'a mut Read,
    title: Option<String>,
    author: Option<String>,
    language: Option<String>,
    blocks: Vec<Block>,
    refs: Refs,
    spine: Vec<SpineRange>,
    opening: Option<Range<usize>>,
    pars: Vec<overlay::Par>,
    complete: bool,
    include_cover: bool,
}

impl ParsedItem {
    fn empty() -> Self {
        Self {
            markup: 0,
            opening: None,
            spine: None,
            pars: Vec::new(),
        }
    }
}

/// A bounded preview followed by one complete parse and layout.
pub struct OpenBook {
    bytes: Arc<Vec<u8>>,
    identity: Arc<()>,
    epub: Epub,
    typo: Typography,
    target: Option<OpenTarget>,
    title: Option<String>,
    author: Option<String>,
    language: Option<String>,
    read: Read,
    blocks: Vec<Block>,
    refs: Refs,
    spine: Vec<SpineRange>,
    records: Vec<SpineRecord>,
    markup: usize,
    opening: Option<Range<usize>>,
    pars: Vec<overlay::Par>,
    escapes: u32,
    next: usize,
    preview: Option<Book>,
    finished: bool,
}

impl OpenBook {
    /// Starts opening an EPUB and prepares a small saved-place window.
    pub fn new(
        bytes: Arc<Vec<u8>>,
        typo: &Typography,
        target: Option<OpenTarget>,
    ) -> Result<Self, OpenError> {
        let open = || {
            let epub = guard::open(&bytes)?;
            let (title, author) = names(&epub);
            let language = epub
                .metadata()
                .language()
                .map(|l| l.value().trim().to_owned())
                .filter(|l| !l.is_empty());
            let book_fixed = meta(&epub, "rendition:layout").as_deref() == Some(PRE_PAGINATED);
            if epub.spine().iter().len() > MAX_SPINE_ITEMS {
                return Err(OpenError::TooLarge);
            }
            let records = spine_records(&epub, book_fixed);
            let mut state = Self {
                bytes,
                identity: Arc::new(()),
                epub,
                typo: typo.clone(),
                target,
                title,
                author,
                language,
                read: Read {
                    budget: MAX_PICTURE_BYTES,
                    ..Read::default()
                },
                blocks: Vec::new(),
                refs: Refs::default(),
                spine: Vec::new(),
                records,
                markup: 0,
                opening: None,
                pars: Vec::new(),
                escapes: 0,
                next: 0,
                preview: None,
                finished: false,
            };
            if let Some(target) = state.target.clone() {
                state.preview = state.preview(&target)?;
            }
            Ok(state)
        };
        catch_unwind(AssertUnwindSafe(open)).unwrap_or(Err(OpenError::Invalid))
    }

    /// Whether the first step can show a bounded preview.
    pub fn is_progressive(&self) -> bool {
        self.preview.is_some()
    }

    /// Uses the latest typography for the remaining full-book layout.
    pub fn set_typography(&mut self, typo: &Typography) {
        self.typo = typo.clone();
    }

    /// The target, retained for the document wrapper's page offset.
    pub fn target(&self) -> Option<&OpenTarget> {
        self.target.as_ref()
    }

    /// Publishes the preview, then batches the remaining spine.
    pub fn step(&mut self) -> Result<OpenStep, OpenError> {
        let step = || {
            if let Some(book) = self.preview.take() {
                return Ok(OpenStep::Preview(book));
            }
            if self.finished {
                return Err(OpenError::Invalid);
            }
            for _ in 0..OPEN_BATCH_ITEMS {
                if self.next >= self.records.len() {
                    return self.complete();
                }
                self.advance()?;
            }
            if self.next >= self.records.len() {
                self.complete()
            } else {
                Ok(OpenStep::Continue)
            }
        };
        catch_unwind(AssertUnwindSafe(step)).unwrap_or(Err(OpenError::Invalid))
    }

    fn complete(&mut self) -> Result<OpenStep, OpenError> {
        self.finished = true;
        let book = self.snapshot(true)?;
        Ok(OpenStep::Complete(book))
    }

    fn advance(&mut self) -> Result<(), OpenError> {
        let Some(record) = self.records.get(self.next).cloned() else {
            return Ok(());
        };
        self.next = self.next.saturating_add(1);
        let mut shelf = Shelf::on(&self.epub, &mut self.read);
        let parsed = read_item(
            ItemPass {
                epub: &self.epub,
                record: &record,
                markup_before: self.markup,
                markup_limit: MAX_MARKUP,
                phrase_count: self.pars.len(),
            },
            &mut shelf,
            (&mut self.blocks, &mut self.refs),
            &mut self.escapes,
        )?;
        self.markup = self.markup.saturating_add(parsed.markup);
        if self.markup > MAX_MARKUP || self.blocks.len() > MAX_BLOCKS {
            return Err(OpenError::TooLarge);
        }
        if self.opening.is_none() {
            self.opening = parsed.opening;
        }
        if let Some(spine) = parsed.spine {
            self.spine.push(spine);
        }
        self.pars.extend(parsed.pars);
        Ok(())
    }

    fn preview(&mut self, target: &OpenTarget) -> Result<Option<Book>, OpenError> {
        let item = match target.spine.as_ref() {
            Some(place) if place.href.len() <= MAX_SPINE_HREF => self
                .records
                .get(place.item)
                .filter(|record| record.href.as_deref() == Some(place.href.as_str()))
                .map(|record| record.item)
                .or_else(|| {
                    self.records
                        .iter()
                        .find(|record| record.href.as_deref() == Some(place.href.as_str()))
                        .map(|record| record.item)
                }),
            Some(_) => None,
            None if target.page == 0 && target.place.is_none() => Some(0),
            None => None,
        };
        let Some(item) = item else {
            return Ok(None);
        };
        let read = self.read.clone();
        let expanded =
            self.preview_window(target, item, PREVIEW_NEIGHBORS + PREVIEW_EXTRA_NEIGHBORS)?;
        if expanded.is_some() {
            return Ok(expanded);
        }
        self.read = read;
        self.preview_window(target, item, PREVIEW_NEIGHBORS)
    }

    fn preview_window(
        &mut self,
        target: &OpenTarget,
        item: usize,
        neighbors: usize,
    ) -> Result<Option<Book>, OpenError> {
        let first = item.saturating_sub(neighbors);
        let end = item
            .saturating_add(neighbors)
            .saturating_add(1)
            .min(self.records.len());
        let mut blocks = Vec::new();
        let mut refs = Refs::default();
        let mut spine = Vec::new();
        let mut markup = 0usize;
        let mut opening = None;
        let mut pars = Vec::new();
        let mut escapes = 0u32;
        let mut shelf = Shelf::on(&self.epub, &mut self.read);
        for record in self.records.get(first..end).unwrap_or_default() {
            let parsed = match read_item(
                ItemPass {
                    epub: &self.epub,
                    record,
                    markup_before: markup,
                    markup_limit: MAX_PREVIEW_MARKUP,
                    phrase_count: pars.len(),
                },
                &mut shelf,
                (&mut blocks, &mut refs),
                &mut escapes,
            ) {
                Ok(parsed) => parsed,
                Err(OpenError::TooLarge) => return Ok(None),
                Err(error) => return Err(error),
            };
            markup = markup.saturating_add(parsed.markup);
            if markup > MAX_PREVIEW_MARKUP || blocks.len() > MAX_PREVIEW_BLOCKS {
                return Ok(None);
            }
            if opening.is_none() {
                opening = parsed.opening;
            }
            if let Some(range) = parsed.spine {
                spine.push(range);
            }
            pars.extend(parsed.pars);
        }
        if blocks.is_empty() {
            return Ok(None);
        }
        let book = assemble_book(BookAssembly {
            epub: &self.epub,
            bytes: &self.bytes,
            identity: &self.identity,
            typo: &self.typo,
            read: &mut self.read,
            title: self.title.clone(),
            author: self.author.clone(),
            language: self.language.clone(),
            blocks,
            refs,
            spine,
            opening,
            pars,
            complete: false,
            include_cover: first == 0,
        })?;
        if target
            .spine
            .as_ref()
            .is_some_and(|place| book.anchor_spine(place).is_none())
        {
            return Ok(None);
        }
        Ok(Some(book))
    }

    fn snapshot(&mut self, complete: bool) -> Result<Book, OpenError> {
        let blocks = std::mem::take(&mut self.blocks);
        let refs = std::mem::take(&mut self.refs);
        let spine = std::mem::take(&mut self.spine);
        let opening = self.opening.take();
        let pars = std::mem::take(&mut self.pars);
        assemble_book(BookAssembly {
            epub: &self.epub,
            bytes: &self.bytes,
            identity: &self.identity,
            typo: &self.typo,
            read: &mut self.read,
            title: self.title.clone(),
            author: self.author.clone(),
            language: self.language.clone(),
            blocks,
            refs,
            spine,
            opening,
            pars,
            complete,
            include_cover: true,
        })
    }
}

fn spine_records(epub: &Epub, book_fixed: bool) -> Vec<SpineRecord> {
    epub.spine()
        .iter()
        .enumerate()
        .map(|(item, spine)| {
            let listed = spine.manifest_entry();
            let id = listed.as_ref().map(|entry| entry.id().to_owned());
            let href = listed
                .as_ref()
                .map(|entry| entry.href().decode().into_owned());
            let overlay = listed
                .as_ref()
                .and_then(|entry| entry.media_overlay())
                .map(|entry| entry.href().decode().into_owned());
            SpineRecord {
                item,
                id,
                href,
                overlay,
                fixed: fixed_item(spine.properties(), book_fixed),
            }
        })
        .collect()
}

fn read_item(
    pass: ItemPass<'_>,
    shelf: &mut Shelf<'_>,
    (blocks, refs): (&mut Vec<Block>, &mut Refs),
    escapes: &mut u32,
) -> Result<ParsedItem, OpenError> {
    let ItemPass {
        epub,
        record,
        markup_before,
        markup_limit,
        phrase_count,
    } = pass;
    let Some(href) = record.href.as_deref() else {
        return Ok(ParsedItem::empty());
    };
    let listed = record
        .id
        .as_deref()
        .and_then(|id| epub.manifest().by_id(id))
        .or_else(|| manifest_by_path(epub, href));
    let Some(listed) = listed else {
        return Ok(ParsedItem::empty());
    };
    let entry = readable(listed);
    let path = entry.href().decode();
    let start = blocks.len();
    let mut markup = 0usize;
    let mut pars = Vec::new();
    if is_picture(&entry) {
        xhtml::read_svg_page(path.as_ref(), shelf, (blocks, refs), record.fixed);
    } else if let Some(content) = chapter_text(&entry) {
        markup = content.len();
        if markup_before.saturating_add(markup) > markup_limit {
            return Err(OpenError::TooLarge);
        }
        match record.fixed {
            true => xhtml::read_fixed_chapter(&content, path.as_ref(), shelf, (blocks, refs)),
            false => xhtml::read_chapter(&content, path.as_ref(), shelf, (blocks, refs)),
        }
        let anchor = Anchor {
            block: start,
            offset: 0,
        };
        refs.places.entry(href.to_owned()).or_insert(anchor);
        let overlay = record
            .overlay
            .as_deref()
            .and_then(|path| manifest_by_path(epub, path))
            .or_else(|| entry.media_overlay());
        if let Some(smil) = overlay.filter(|_| phrase_count < overlay::MAX_PHRASES) {
            let xml = smil.read_bytes().unwrap_or_default();
            let xml = xml.get(..xml.len().min(MAX_OVERLAY)).unwrap_or_default();
            pars.extend(overlay::read(xml, &smil.href().decode(), escapes));
        }
    }
    let end = blocks.len();
    let range = (end > start).then(|| SpineRange {
        item: record.item,
        href: href.to_owned(),
        blocks: start..end,
    });
    let opening = (end > start).then_some(start..end);
    if end > start {
        refs.places.entry(href.to_owned()).or_insert(Anchor {
            block: start,
            offset: 0,
        });
    }
    Ok(ParsedItem {
        markup,
        opening,
        spine: range,
        pars,
    })
}

fn assemble_book(assembly: BookAssembly<'_>) -> Result<Book, OpenError> {
    let BookAssembly {
        epub,
        bytes,
        identity,
        typo,
        read,
        title,
        author,
        language,
        mut blocks,
        mut refs,
        mut spine,
        opening,
        pars,
        complete,
        include_cover,
    } = assembly;
    let mut shelf = Shelf::on(epub, read);
    if include_cover {
        let cover = epub
            .manifest()
            .cover_image()
            .map(|entry| entry.href().decode());
        let cover = cover.and_then(|path| shelf.picture(&path));
        let drawn = blocks
            .first()
            .is_some_and(|block| block.frame.sheet.is_some());
        if let Some(cover) = cover.filter(|_| !drawn)
            && place_cover(&mut blocks, opening.unwrap_or(0..0), cover)
        {
            for at in refs.places.values_mut().chain(refs.ends.values_mut()) {
                at.block = at.block.saturating_add(1);
            }
            for extent in refs.extents.values_mut() {
                extent.blocks =
                    extent.blocks.start.saturating_add(1)..extent.blocks.end.saturating_add(1);
            }
            for card in &mut refs.cards {
                card.caption = card.caption.map(|block| block.saturating_add(1));
            }
            for chapter in &mut refs.chapters {
                chapter.blocks =
                    chapter.blocks.start.saturating_add(1)..chapter.blocks.end.saturating_add(1);
            }
            for item in &mut spine {
                item.blocks =
                    item.blocks.start.saturating_add(1)..item.blocks.end.saturating_add(1);
            }
        }
    }
    if blocks.is_empty() {
        return Err(OpenError::Empty);
    }
    let rtl = right_to_left(epub, language.as_deref(), &blocks);
    let lead = if include_cover {
        spread_lead(epub, rtl, &blocks)
    } else {
        0
    };
    blocks.shrink_to_fit();
    let blocks = Arc::new(blocks);
    let faces = load_faces(epub, &refs.faces);
    let flow = Arc::new(set(&blocks, typo, language.as_deref(), &faces));
    let toc = contents(epub, &refs, &blocks);
    let notes = crate::notes::find(&blocks, &refs);
    let narration = narration(epub, pars, (&refs, blocks.len()));
    let plays = refs
        .hrefs
        .iter()
        .any(|href| matches!(href, Href::Media { .. }));
    let plays = plays || !narration.phrases.is_empty() || !refs.cards.is_empty();
    let plays = plays || refs.clips.values().any(|(path, _)| path.is_some());
    let scripted = refs
        .chapters
        .iter()
        .any(|chapter| !chapter.scripts.code.is_empty());
    let shows = !refs.targets.is_empty() || scripted;
    let archive = (!complete || plays || shows).then(|| Arc::clone(bytes));
    let book_read = shows.then(|| {
        let copy = complete.then(|| std::mem::take(read));
        Arc::new(copy.unwrap_or_else(|| read.clone()))
    });
    Ok(Book {
        identity: Arc::clone(identity),
        blocks,
        flow,
        typo: typo.clone(),
        layout: next_layout(),
        inserts: Arc::default(),
        spliced: None,
        refs: Arc::new(refs),
        spine: Arc::new(spine),
        faces: Arc::new(faces),
        notes: Arc::new(notes),
        toc: Arc::new(toc),
        title,
        author,
        language,
        rtl,
        lead,
        archive,
        narration: Arc::new(narration),
        read: book_read,
        complete,
        windowed: false,
    })
}

impl std::fmt::Debug for Book {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Book({} pages)", self.page_count())
    }
}

impl Book {
    /// Reads and lays out every chapter in `typo`; never panics.
    pub fn open(bytes: Arc<Vec<u8>>, typo: &Typography) -> Result<Self, OpenError> {
        let parse = || {
            let mut open = OpenBook::new(bytes, typo, None)?;
            loop {
                if let OpenStep::Complete(book) = open.step()? {
                    return Ok(book);
                }
            }
        };
        catch_unwind(AssertUnwindSafe(parse)).unwrap_or(Err(OpenError::Invalid))
    }

    /// Whether every spine item has been parsed and laid out.
    pub fn is_complete(&self) -> bool {
        self.complete
    }

    /// Whether the book already has this typography.
    pub fn has_typography(&self, typo: &Typography) -> bool {
        self.typo == *typo
    }

    /// The same book set in `typo`; its pictures stay decoded.
    pub fn retypeset(&self, typo: &Typography) -> Option<Self> {
        self.reset(typo, None)
    }

    /// The book in `typo` with `inserts` laid in; none keeps its own.
    pub fn reset(&self, typo: &Typography, inserts: Option<&Arc<Inserts>>) -> Option<Self> {
        self.reset_inner(typo, inserts, None)
    }

    /// Its narration; no phrases if it has none.
    pub fn narration(&self) -> &Arc<Narration> {
        &self.narration
    }

    /// Sound or video at `path` to hand to a player; none for other files.
    pub fn media(&self, path: &str) -> Option<Media> {
        self.files()?.media(path)
    }

    /// Clip `clip`'s subtitle files, for a player to load beside it.
    pub fn subtitles(&self, clip: &str) -> Vec<Subtitle> {
        let Some(files) = self.files() else {
            return Vec::new();
        };
        let cards = self.refs.cards.iter();
        let mut tracks: Vec<&Track> = (cards.filter(|c| c.clip.as_deref() == Some(clip)))
            .flat_map(|c| c.tracks.iter())
            .collect();
        let mut seen = Vec::new();
        tracks.retain(|t| {
            let fresh = !seen.contains(&&t.path);
            seen.push(&t.path);
            fresh
        });
        let tracks = tracks.into_iter().take(subtitles::MAX_TRACKS);
        tracks
            .filter_map(|t| {
                let data = files.read(&t.path)?;
                let extension = subtitles::Format::of(&data).extension();
                let language = t.language.clone();
                Some(Subtitle {
                    language,
                    extension,
                    data,
                })
            })
            .collect()
    }

    /// Card `card`'s transcript, to lay where its link was; `track` or a pick.
    pub fn transcript(&self, card: usize, track: Option<usize>, wanted: Wanted) -> Option<Insert> {
        let c = self.refs.cards.get(card)?;
        let at = c.caption?;
        let caption = self.blocks.get(at)?;
        let chosen = track.filter(|t| *t < c.tracks.len());
        let chosen = chosen.or_else(|| subtitles::pick(&c.tracks, wanted.languages))?;
        let picked = c.tracks.get(chosen)?;
        let data = self.files().and_then(|f| f.read(&picked.path));
        let cues = data.map(|d| subtitles::cues(&d)).unwrap_or_default();
        let blocks = subtitles::transcript(caption, c, (chosen, &cues), wanted.name);
        Some(Insert {
            id: TRANSCRIPTS | card as u128,
            from: Anchor {
                block: at,
                offset: 0,
            },
            to: Anchor {
                block: at + 1,
                offset: 0,
            },
            blocks: Arc::new(blocks),
            language: picked.language.clone(),
        })
    }

    /// What clicking trigger link `n` sets off, in the order written.
    pub fn trigger(&self, n: usize) -> Vec<Triggered> {
        let Some(actions) = self.refs.triggers.get(n) else {
            return Vec::new();
        };
        let act = |(action, key): &(Action, String)| match action.shows() {
            true => {
                let target = *self.refs.target_keys.get(key)?;
                Some(Triggered::Show(target, *action == Action::Show))
            }
            false => {
                let (path, video) = self.refs.clips.get(key)?;
                Some(Triggered::Clip {
                    action: *action,
                    path: path.clone()?,
                    video: *video,
                })
            }
        };
        actions.iter().filter_map(act).collect()
    }

    /// The blocks to lay so each target in `shown` shows as it says.
    pub fn triggered(&self, shown: &BTreeMap<usize, bool>) -> Vec<Insert> {
        self.relaid(shown, &BTreeMap::new())
    }

    /// Triggered blocks, with scripted chapters laid from `pages` instead.
    pub fn relaid(
        &self,
        shown: &BTreeMap<usize, bool>,
        pages: &BTreeMap<usize, String>,
    ) -> Vec<Insert> {
        let reread = || self.reread(shown, pages);
        catch_unwind(AssertUnwindSafe(reread)).unwrap_or_default()
    }

    /// Its scripted chapters, with the code and sheets their scripts need.
    pub fn scripts(&self) -> Vec<Scripted> {
        let gather = || {
            let archive = self.archive.as_ref()?;
            let epub = guard::open(archive).ok()?;
            let mut budget = script::MAX_CODE;
            let mut read = |path: &str| {
                let bytes = read_resource(&epub, path)?;
                budget = budget.checked_sub(bytes.len())?;
                Some(String::from_utf8_lossy(&bytes).into_owned())
            };
            let mut out = Vec::new();
            for laid in self.refs.chapters.iter() {
                let found = &laid.scripts;
                if found.code.is_empty() {
                    continue;
                }
                let entry = manifest_by_path(&epub, &laid.path);
                let Some(xhtml) = entry.and_then(|e| chapter_text(&e)) else {
                    continue;
                };
                let code = found.code.iter().filter_map(|c| match c {
                    script::Code::Inline(text) => Some(text.clone()),
                    script::Code::File(path) => read(path),
                });
                let code = code.collect();
                let styles = found.styles.iter().filter_map(|p| read(p)).collect();
                out.push(Scripted {
                    chapter: laid.at,
                    path: laid.path.clone(),
                    xhtml,
                    code,
                    styles,
                });
            }
            Some(out)
        };
        catch_unwind(AssertUnwindSafe(gather))
            .ok()
            .flatten()
            .unwrap_or_default()
    }

    /// Reads chapters with a target in `shown`, or a page in `pages`, again.
    fn reread(
        &self,
        shown: &BTreeMap<usize, bool>,
        pages: &BTreeMap<usize, String>,
    ) -> Vec<Insert> {
        let (Some(read), Some(archive)) = (&self.read, &self.archive) else {
            return Vec::new();
        };
        let Ok(epub) = guard::open(archive) else {
            return Vec::new();
        };
        let mut chapters: BTreeMap<usize, HashMap<String, bool>> = BTreeMap::new();
        for (&n, &visible) in shown {
            let Some(t) = self.refs.targets.get(n) else {
                continue;
            };
            let id = t.key.rsplit_once('#').map_or("", |(_, id)| id);
            let overrides = chapters.entry(t.chapter).or_default();
            overrides.insert(id.to_owned(), visible);
        }
        for &chapter in pages.keys() {
            chapters.entry(chapter).or_default();
        }
        let mut scratch = read.as_ref().clone();
        let mut shelf = Shelf::on(&epub, &mut scratch);
        let mut out = Vec::new();
        for (chapter, overrides) in chapters {
            let Some(laid) = self.refs.chapters.get(chapter) else {
                continue;
            };
            let entry = manifest_by_path(&epub, &laid.path);
            let content = match pages.get(&chapter) {
                Some(page) => Some(page.clone()),
                None => entry.and_then(|e| chapter_text(&e)),
            };
            let (Some(content), Some(base)) = (content, self.blocks.get(laid.blocks.clone()))
            else {
                continue;
            };
            let (mut blocks, mut fresh) = (Vec::new(), Refs::default());
            let into = (&mut blocks, &mut fresh);
            xhtml::reread_chapter(&content, laid, overrides, &mut shelf, into);
            trigger::remap(&mut blocks, &fresh, &self.refs, laid);
            let language = self.language.as_deref();
            out.extend(trigger::changes(base, blocks, laid.blocks.start, language));
        }
        out
    }

    /// Its files, to read sound from after the book itself is gone.
    pub fn files(&self) -> Option<Files> {
        let archive = Arc::clone(self.archive.as_ref()?);
        Some(Files {
            archive,
            epub: Arc::new(OnceLock::new()),
        })
    }

    pub fn page_count(&self) -> usize {
        match &self.spliced {
            Some(s) => s.pages.len(),
            None => self.flow.page_count(),
        }
    }

    /// Page `page`: its flow and page there, and the region it is of.
    fn part(&self, page: usize) -> (&Flow, usize, Option<&crate::splice::Region>) {
        let Some(s) = &self.spliced else {
            return (&self.flow, page, None);
        };
        match s.at(page) {
            Some(At::Base(p)) => (&self.flow, p, None),
            Some(At::Region(r, i)) => match s.region(r) {
                Some(region) => (&region.flow, i, Some(region)),
                None => (&self.flow, usize::MAX, None),
            },
            None => (&self.flow, usize::MAX, None),
        }
    }

    /// Every page's width and height, in layout units (CSS px).
    pub fn page_size(&self) -> (f32, f32) {
        self.flow.page_size()
    }

    /// Page `page`'s width and height: a fixed page keeps its own shape.
    pub fn size(&self, page: usize) -> (f32, f32) {
        let (flow, page, _) = self.part(page);
        flow.size(page)
    }

    /// Whether page `page` is a fixed-layout page, drawn as the book laid it.
    pub fn fixed(&self, page: usize) -> bool {
        let (flow, page, _) = self.part(page);
        flow.is_fixed(page)
    }

    /// Interactive or animated elements laid out on `page`.
    pub fn interactive(&self, page: usize) -> &[InteractiveItem] {
        let (flow, page, _) = self.part(page);
        flow.interactive(page)
    }

    /// Renders one interactive item on `page` at `scale`.
    pub fn render_interactive_item(
        &self,
        page: usize,
        item: usize,
        scale: f32,
        hovered: bool,
    ) -> Option<ItemPixmap> {
        let (flow, page, _) = self.part(page);
        flow.render_item(page, item, scale, hovered)
    }

    /// Pages shown alone before spreads pair; `usize::MAX` pairs none.
    pub fn lead(&self) -> usize {
        self.lead
    }

    /// The fonts it ships that its designed chapters use, unpacked.
    pub fn faces(&self) -> &[BookFace] {
        &self.faces
    }

    /// Whether page `page` is written vertically: its text runs down.
    pub fn turned(&self, page: usize) -> bool {
        let (flow, page, _) = self.part(page);
        flow.turned(page)
    }

    /// Whether its pages turn right to left, as its spine or language asks.
    pub fn right_to_left(&self) -> bool {
        self.rtl
    }

    /// The `dc:title` as written, if any.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// The first `dc:creator` that isn't blank.
    pub fn author(&self) -> Option<&str> {
        self.author.as_deref()
    }

    /// Where page `page` starts; a result's pages, where theirs did.
    pub fn anchor(&self, page: usize) -> Option<Anchor> {
        self.flow.anchor(self.to_base(page))
    }

    /// The page showing `anchor`, or the first after it.
    pub fn locate(&self, anchor: Anchor) -> usize {
        self.from_base(self.flow.locate(anchor))
    }

    /// The stable spine item and block offset for `anchor`.
    pub fn spine_place(&self, anchor: Anchor) -> Option<SpinePlace> {
        let item = self
            .spine
            .iter()
            .find(|item| (item.blocks.start..item.blocks.end).contains(&anchor.block))?;
        if item.href.len() > MAX_SPINE_HREF {
            return None;
        }
        Some(SpinePlace {
            href: item.href.clone(),
            item: item.item,
            block: anchor.block.saturating_sub(item.blocks.start),
            offset: anchor.offset,
        })
    }

    /// The block at a stable location, if its spine item is present.
    pub fn anchor_spine(&self, place: &SpinePlace) -> Option<Anchor> {
        if place.href.len() > MAX_SPINE_HREF {
            return None;
        }
        let fits = |item: &&SpineRange| {
            item.href == place.href
                && place.block < item.blocks.end.saturating_sub(item.blocks.start)
        };
        let item = self
            .spine
            .iter()
            .find(|item| item.item == place.item && fits(item))
            .or_else(|| self.spine.iter().find(fits))?;
        Some(Anchor {
            block: item.blocks.start.saturating_add(place.block),
            offset: place.offset,
        })
    }

    /// The page showing a stable location, if its spine item is present.
    pub fn locate_spine(&self, place: &SpinePlace) -> Option<usize> {
        self.anchor_spine(place).map(|anchor| self.locate(anchor))
    }

    /// The book's table of contents, in reading order.
    pub fn toc(&self) -> &[TocEntry] {
        &self.toc
    }

    /// Page `page`'s links that lead somewhere: a box per line.
    pub fn links(&self, page: usize) -> Vec<Link> {
        let lead = |target: u32| {
            if let Some((chapter, node)) = script::unlink(target) {
                return Some(Destination::Script { chapter, node });
            }
            let href = self.refs.hrefs.get(usize::try_from(target).ok()?)?;
            Some(match href {
                Href::Inside(key) => {
                    let page = self.locate(self.refs.place(key)?);
                    match self.notes.get(&target) {
                        Some(text) => Destination::Note {
                            page,
                            text: text.clone(),
                        },
                        None => Destination::Page(page),
                    }
                }
                Href::Outside(address) => Destination::Outside(address.clone()),
                Href::Media { path, video } => Destination::Media {
                    path: path.clone(),
                    video: *video,
                },
                Href::Transcript { card, track } => Destination::Transcript {
                    card: usize::try_from(*card).ok()?,
                    track: track.and_then(|t| usize::try_from(t).ok()),
                },
                Href::Trigger(n) => Destination::Trigger(usize::try_from(*n).ok()?),
            })
        };
        let (flow, page, _) = self.part(page);
        let links = flow.links(page).into_iter();
        links
            .filter_map(|l| {
                Some(Link {
                    rect: l.rect,
                    to: lead(l.target)?,
                })
            })
            .collect()
    }

    /// Page `page`'s text by line, each character boxed.
    pub fn text(&self, page: usize) -> Vec<PageLine> {
        match self.part(page) {
            (_, page, Some(region)) => region.text(page),
            (flow, page, None) => flow.text(page),
        }
    }

    /// Page `page`'s pictures, which keep their colours under the ink dial.
    pub fn pictures(&self, page: usize) -> Vec<[f32; 4]> {
        let (flow, page, _) = self.part(page);
        flow.pictures(page)
    }

    /// The picture drawn at a point of page `page`, in page units.
    pub fn picture_at(&self, page: usize, x: f32, y: f32) -> Option<Arc<Picture>> {
        let (flow, page, _) = self.part(page);
        flow.picture_at(page, x, y)
    }

    /// Page `page`'s colour glyphs, which keep their colours too.
    pub fn emojis(&self, page: usize) -> Vec<[f32; 4]> {
        let (flow, page, _) = self.part(page);
        flow.emojis(page)
    }

    /// What of each page a stream shows.
    pub fn bands(&self) -> &[mareader_reflow::Band] {
        match &self.spliced {
            Some(s) => &s.bands,
            None => self.flow.bands(),
        }
    }

    /// `region` of page `page` at `scale` pixels per unit.
    pub fn render(&self, page: usize, scale: f32, region: Region) -> Option<Pixels> {
        let (flow, page, _) = self.part(page);
        flow.render(page, scale, region)
    }

    /// The blocks the book is made of, in reading order.
    pub fn blocks(&self) -> &Arc<Vec<Block>> {
        &self.blocks
    }

    /// What page `page` shows of each book block, results aside.
    pub fn excerpts(&self, page: usize) -> Vec<(usize, std::ops::Range<usize>)> {
        match self.part(page) {
            (_, page, Some(region)) => region.excerpts(page),
            (flow, page, None) => flow.excerpts(page),
        }
    }

    /// The book's language tag, if it names one.
    pub fn language(&self) -> Option<&str> {
        self.language.as_deref()
    }

    /// At rest: its flows keep only what the pages in view need.
    pub fn rest(&self) {
        self.flow.rest();
        let regions = self.spliced.iter().flat_map(|s| &s.regions);
        regions.for_each(|r| r.flow.rest());
    }

    /// Shares the book's decoded pictures with `flow`, set from its blocks.
    pub fn with_pictures(&self, flow: Flow) -> Flow {
        flow.keep_pictures(&self.flow)
    }

    /// Page `page`'s column: its left edge and width, in layout units.
    pub fn column(&self, page: usize) -> (f32, f32) {
        let (flow, page, _) = self.part(page);
        flow.column(page)
    }

    /// Where page `page`'s text may run: left, top, width and height.
    pub fn text_area(&self, page: usize) -> [f32; 4] {
        let (flow, page, _) = self.part(page);
        flow.text_area(page)
    }

    /// Names the layout of its own pages; results laid in keep it.
    pub fn layout(&self) -> u64 {
        self.layout
    }

    /// The results asked for, laid or not; zero for none.
    pub fn inserts_key(&self) -> u64 {
        self.inserts.key
    }

    /// Whether result `id` is laid in the pages.
    pub fn shows(&self, id: u128) -> bool {
        self.spliced.as_ref().is_some_and(|s| s.shows(id))
    }

    /// How many pages of its own it has, results aside.
    pub fn base_count(&self) -> usize {
        self.flow.page_count()
    }

    /// The own page showing `anchor`, results aside.
    pub fn base_locate(&self, anchor: Anchor) -> usize {
        self.flow.locate(anchor)
    }

    /// What own page `page` shows of each block, results aside.
    pub fn base_excerpts(&self, page: usize) -> Vec<(usize, std::ops::Range<usize>)> {
        self.flow.excerpts(page)
    }

    /// The own page page `page` stands for: a result's, its first.
    pub fn to_base(&self, page: usize) -> usize {
        let Some(s) = &self.spliced else {
            return page;
        };
        match s.at(page) {
            Some(At::Base(p)) => p,
            Some(At::Region(r, _)) => s.region(r).map_or(page, |r| r.base.start),
            None => page,
        }
    }

    /// The page showing own page `page`; a replaced one, its result's.
    pub fn from_base(&self, page: usize) -> usize {
        let Some(s) = &self.spliced else {
            return page;
        };
        match s.from_base.get(page) {
            Some(p) => *p,
            None => s.pages.len().saturating_sub(1),
        }
    }

    /// The own pages page `page` stands for; a result's later pages, none.
    pub fn base_pages(&self, page: usize) -> std::ops::Range<usize> {
        let Some(s) = &self.spliced else {
            return page..page + 1;
        };
        match s.at(page) {
            Some(At::Base(p)) => p..p + 1,
            Some(At::Region(r, 0)) => s.region(r).map_or(0..0, |r| r.base.clone()),
            Some(At::Region(r, _)) => s.region(r).map_or(0..0, |r| r.base.start..r.base.start),
            None => 0..0,
        }
    }

    /// The same open EPUB in any type or parsing snapshot.
    pub fn same_book(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.identity, &other.identity)
    }

    /// The page of `other` that looks just like page `page`, if any.
    pub fn same_page(&self, page: usize, other: &Self) -> Option<usize> {
        if !Arc::ptr_eq(&self.flow, &other.flow) {
            let same_faces = self.faces.len() == other.faces.len()
                && self.faces.iter().zip(other.faces.iter()).all(|(a, b)| {
                    a.family == b.family
                        && a.weight == b.weight
                        && a.italic == b.italic
                        && a.len() == b.len()
                });
            if !Arc::ptr_eq(&self.identity, &other.identity)
                || self.typo != other.typo
                || self.language != other.language
                || self.rtl != other.rtl
                || self.lead != other.lead
                || !same_faces
                || self.spliced.is_some()
                || other.spliced.is_some()
                || (!self.complete && page.saturating_add(1) == self.page_count())
            {
                return None;
            }
            let place = self.spine_place(self.anchor(page)?)?;
            let mapped = other.locate_spine(&place)?;
            let normal = |book: &Self, page| {
                book.excerpts(page)
                    .into_iter()
                    .map(|(block, range)| {
                        let place = book.spine_place(Anchor { block, offset: 0 })?;
                        Some((place.href, place.item, place.block, range))
                    })
                    .collect::<Option<Vec<_>>>()
            };
            let same = normal(self, page)
                .zip(normal(other, mapped))
                .is_some_and(|(a, b)| a == b)
                && self.size(page) == other.size(mapped)
                && self.column(page) == other.column(mapped)
                && self.fixed(page) == other.fixed(mapped)
                && self.turned(page) == other.turned(mapped);
            return same.then_some(mapped);
        }
        let at = match &self.spliced {
            Some(s) => s.at(page)?,
            None => At::Base(page),
        };
        match (at, &other.spliced) {
            (At::Base(p), None) => (p < other.base_count()).then_some(p),
            (At::Base(p), Some(o)) => {
                let q = *o.from_base.get(p)?;
                (o.at(q) == Some(At::Base(p))).then_some(q)
            }
            (At::Region(r, i), Some(o)) => {
                let region = self.spliced.as_ref()?.region(r)?;
                Some(o.find(region)? + i)
            }
            (At::Region(..), None) => None,
        }
    }
}

/// The `dc:title` and first non-blank `dc:creator`, as written.
fn names(epub: &Epub) -> (Option<String>, Option<String>) {
    let title = epub
        .metadata()
        .title()
        .map(|t| t.value().trim().to_owned())
        .filter(|t| !t.is_empty());
    let author = epub
        .metadata()
        .creators()
        .map(|c| c.value().trim().to_owned())
        .find(|c| !c.is_empty());
    (title, author)
}

/// A book's title and author without laying it out, for the library.
pub fn describe(bytes: &Arc<Vec<u8>>) -> Option<(Option<String>, Option<String>)> {
    let read = || guard::open(bytes).ok().map(|e| names(&e));
    catch_unwind(AssertUnwindSafe(read)).ok().flatten()
}

/// A book's title and author streamed from `path` without loading it whole.
#[cfg(not(target_arch = "wasm32"))]
pub fn describe_file(path: &Path) -> Option<(Option<String>, Option<String>)> {
    let read = || guard::open_file_metadata(path).ok().map(|e| names(&e));
    catch_unwind(AssertUnwindSafe(read)).ok().flatten()
}

/// A picture this near the opening counts as the cover.
const COVER_BLOCKS: usize = 4;

/// The declared cover, else a picture opening the first chapter; sets no type.
pub fn cover(bytes: &Arc<Vec<u8>>) -> Option<Arc<Picture>> {
    let find = || cover_from(&guard::open(bytes).ok()?);
    catch_unwind(AssertUnwindSafe(find)).ok().flatten()
}

/// The cover streamed from `path` without loading the full archive.
#[cfg(not(target_arch = "wasm32"))]
pub fn cover_file(path: &Path) -> Option<Arc<Picture>> {
    let find = || cover_from(&guard::open_file_cover(path).ok()?);
    catch_unwind(AssertUnwindSafe(find)).ok().flatten()
}

fn cover_from(epub: &Epub) -> Option<Arc<Picture>> {
    let mut read = Read {
        budget: MAX_PICTURE_BYTES,
        ..Read::default()
    };
    let mut shelf = Shelf::on(epub, &mut read);
    let declared = epub.manifest().cover_image().map(|e| e.href().decode());
    if let Some(picture) = declared.and_then(|path| shelf.picture(&path)) {
        return Some(picture);
    }
    let (mut blocks, mut refs) = (Vec::new(), Refs::default());
    let spine = epub.spine().iter().filter_map(|s| s.manifest_entry());
    for entry in spine.map(readable) {
        let path = entry.href().decode();
        if is_picture(&entry) {
            xhtml::read_svg_page(&path, &mut shelf, (&mut blocks, &mut refs), false);
        } else if let Some(content) = chapter_text(&entry).filter(|c| c.len() <= MAX_MARKUP) {
            xhtml::read_chapter(&content, &path, &mut shelf, (&mut blocks, &mut refs));
        }
        if !blocks.is_empty() {
            break;
        }
    }
    for block in blocks.iter().take(COVER_BLOCKS) {
        if let Content::Image { picture, .. } = &block.content {
            return Some(Arc::clone(picture));
        }
    }
    None
}

/// The media type of SVG pages in the spine.
const SVG: &str = "image/svg+xml";

/// A chapter's markup; stray bytes that aren't UTF-8 become U+FFFD.
fn chapter_text(entry: &EpubManifestEntry) -> Option<String> {
    let bytes = || entry.read_bytes().ok();
    let lossy = |b: Vec<u8>| String::from_utf8_lossy(&b).into_owned();
    entry.read_str().ok().or_else(|| bytes().map(lossy))
}

/// Each shipped font may be this big, and all of them together this.
const MAX_FACE_BYTES: usize = 32 << 20;
const MAX_FACES_BYTES: usize = 96 << 20;

/// Fonts its designed text uses, unpacked or revealed; unreadable ones skipped.
fn load_faces(epub: &Epub, faces: &[FontFace]) -> Vec<BookFace> {
    let listed = epub.read_resource_bytes("/META-INF/encryption.xml").ok();
    let hidden = listed.map_or_else(HashMap::new, |x| {
        fontfile::hidden(&String::from_utf8_lossy(&x))
    });
    let keys = (!hidden.is_empty()).then(|| book_keys(epub));
    let font = |path: &str| {
        let bytes = read_resource(epub, path)?;
        if bytes.len() > MAX_FACE_BYTES {
            return None;
        }
        let out = match (hidden.get(path), &keys) {
            (Some(how), Some(keys)) => fontfile::reveal(&bytes, *how, keys)?,
            _ => fontfile::sfnt(bytes)?,
        };
        (out.len() <= MAX_FACE_BYTES).then_some(out)
    };
    let mut total = 0;
    let mut out = Vec::new();
    for face in faces {
        let read = |path: &String| font(path);
        let Some(bytes) = face.src.iter().find_map(read) else {
            continue;
        };
        total += bytes.len();
        if total > MAX_FACES_BYTES {
            break;
        }
        let family = format!("{BOOK_FACE}{}", face.family.to_lowercase());
        out.push(BookFace::new(
            family,
            bytes,
            f32::from(face.weight),
            face.italic,
        ));
    }
    out
}

/// Obfuscation keys from the book's identifiers, its unique one first.
fn book_keys(epub: &Epub) -> fontfile::Keys {
    let metadata = epub.metadata();
    let unique = metadata.identifier().map(|i| i.value().to_owned());
    let others = metadata.identifiers().map(|i| i.value().to_owned());
    let ids: Vec<String> = unique.into_iter().chain(others).collect();
    fontfile::Keys::new(ids.iter().map(String::as_str))
}

/// Lays the blocks out; the book's language beats the default.
fn set(
    blocks: &Arc<Vec<Block>>,
    typo: &Typography,
    language: Option<&str>,
    faces: &[BookFace],
) -> Flow {
    let (blocks, fonts) = (Arc::clone(blocks), Fonts::system().with_faces(faces));
    match language {
        Some(tag) if *tag != typo.language => {
            let typo = Typography {
                language: tag.to_owned(),
                ..typo.clone()
            };
            Flow::new(blocks, &typo, fonts)
        }
        _ => Flow::new(blocks, typo, fonts),
    }
}

/// The listed contents whose places are known; else the headings.
fn contents(epub: &Epub, refs: &Refs, blocks: &[Block]) -> Vec<TocEntry> {
    let root = epub.toc().contents();
    let listed: Vec<TocEntry> = root
        .iter()
        .flat_map(|root| root.flatten())
        .take(MAX_TOC)
        .filter_map(|e| {
            Some(TocEntry {
                title: one_line(e.label())?,
                depth: e.depth().saturating_sub(1),
                at: refs.place(&e.href()?.decode())?,
            })
        })
        .collect();
    if !listed.is_empty() {
        return listed;
    }
    let headings = blocks.iter().enumerate().filter_map(|(block, b)| {
        let Content::Text(text) = &b.content else {
            return None;
        };
        let Role::Heading(level) = text.role else {
            return None;
        };
        Some(TocEntry {
            title: one_line(&text.text)?,
            depth: usize::from(level.saturating_sub(1)),
            at: Anchor { block, offset: 0 },
        })
    });
    headings.take(MAX_TOC).collect()
}

/// `s` on one line and cut short; `None` if blank.
fn one_line(s: &str) -> Option<String> {
    let words: Vec<&str> = s.split_whitespace().collect();
    let line: String = words.join(" ").chars().take(MAX_TITLE).collect();
    (!line.is_empty()).then_some(line)
}

/// Makes the cover fill a page; true if it had to be added first.
fn place_cover(
    blocks: &mut Vec<Block>,
    opening: std::ops::Range<usize>,
    cover: Arc<Picture>,
) -> bool {
    let slice = blocks.get_mut(opening).unwrap_or(&mut []);
    let idx = slice.iter().position(
        |b| matches!(&b.content, Content::Image { picture, .. } if Arc::ptr_eq(picture, &cover)),
    );
    let shown = idx
        .filter(|&i| {
            let prior = slice
                .get(..i)
                .is_some_and(|s| s.iter().any(|b| matches!(b.content, Content::Text(_))));
            let styled = slice.get(i).is_some_and(|b| {
                b.frame.float.is_some()
                    || matches!(b.content, Content::Image { width: Some(_), .. })
            });
            !prior && !styled
        })
        .and_then(|i| slice.get_mut(i));
    if let Some(block) = shown {
        let links = match &mut block.content {
            Content::Image { links, .. } => std::mem::take(links),
            _ => Vec::new(),
        };
        block.content = Content::Image {
            picture: cover,
            cover: true,
            width: None,
            links,
        };
        return false;
    }
    let mut block = Block::new(Content::Image {
        picture: cover,
        cover: true,
        width: None,
        links: Vec::new(),
    });
    block.break_before = true;
    blocks.insert(0, block);
    true
}

/// The spine's page direction; with none, right to left for such languages.
fn right_to_left(epub: &Epub, language: Option<&str>, blocks: &[Block]) -> bool {
    let direction = epub.spine().page_direction();
    if !direction.is_default() {
        return direction.is_rtl();
    }
    // Vertical writing reads its columns, and so its pages, leftward.
    if blocks.iter().any(|b| b.frame.turned) {
        return true;
    }
    let primary = language
        .and_then(|l| l.split(['-', '_']).next())
        .unwrap_or_default();
    RTL_LANGUAGES.contains(&primary.to_ascii_lowercase().as_str())
}

/// `rendition:layout` for pages laid out by the book.
const PRE_PAGINATED: &str = "pre-paginated";

/// The book's narration from `pars`.
fn narration(epub: &Epub, pars: Vec<overlay::Par>, (refs, blocks): (&Refs, usize)) -> Narration {
    let phrases = overlay::phrases(pars, refs, blocks);
    if phrases.is_empty() {
        return Narration::default();
    }
    let duration = meta(epub, "media:duration").and_then(|d| overlay::clock(&d));
    Narration {
        phrases,
        narrator: meta(epub, "media:narrator").filter(|n| !n.is_empty()),
        duration,
    }
}

/// The first value of a package `<meta property>`, trimmed.
fn meta(epub: &Epub, property: &str) -> Option<String> {
    let first = epub.metadata().by_property(property).next();
    first.map(|m| m.value().trim().to_owned())
}

/// Whether a spine item is fixed: its own override, else the book's.
fn fixed_item(properties: &Properties, book: bool) -> bool {
    let has = |p: &str| properties.has_property(p);
    match () {
        _ if has("rendition:layout-pre-paginated") => true,
        _ if has("rendition:layout-reflowable") => false,
        _ => book,
    }
}

/// A spine item as read: itself, or its first readable fallback.
fn readable(entry: EpubManifestEntry) -> EpubManifestEntry {
    let known = |e: &EpubManifestEntry| {
        let t = e.media_type().trim().to_ascii_lowercase();
        is_picture(e) || t == "application/xhtml+xml" || t == "text/html"
    };
    if known(&entry) {
        return entry;
    }
    let found = entry.fallbacks().take(media::MAX_FALLBACKS).find(known);
    found.unwrap_or(entry)
}

/// A spine item that is a picture, shown as a page of its own.
fn is_picture(entry: &EpubManifestEntry) -> bool {
    entry.media_type() == SVG || entry.media_type().starts_with("image/")
}

/// Pages alone before spreads: one if the first wants the far side.
fn spread_lead(epub: &Epub, rtl: bool, blocks: &[Block]) -> usize {
    if blocks.first().is_none_or(|b| b.frame.sheet.is_none()) {
        return 0;
    }
    if meta(epub, "rendition:spread").as_deref() == Some("none") {
        return usize::MAX;
    }
    let far = if rtl {
        "page-spread-left"
    } else {
        "page-spread-right"
    };
    let first = epub.spine().iter().next();
    let props = first.map(|s| s.properties());
    let alone =
        props.is_some_and(|p| p.has_property(far) || p.has_property(&format!("rendition:{far}")));
    usize::from(alone)
}

/// Languages written right to left, by their primary subtag.
const RTL_LANGUAGES: [&str; 10] = ["ar", "he", "iw", "fa", "ur", "yi", "ps", "sd", "ug", "dv"];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample;

    #[test]
    fn the_sample_opens_with_its_title_and_chapters() {
        let book = Book::open(Arc::new(sample::epub(3, "Sample")), &Typography::default());
        let book = book.as_ref().ok();
        assert_eq!(book.and_then(Book::title), Some("Sample"));
        // A cover, then chapters that each start a page and fill more.
        assert!(book.is_some_and(|b| b.page_count() >= 7), "{book:?}");
    }

    #[test]
    fn opening_previews_a_stable_spine_place_then_finishes_once() {
        let bytes = Arc::new(sample::epub(40, "Progress"));
        let typo = Typography::default();
        let full = Book::open(Arc::clone(&bytes), &typo).ok();
        assert!(full.is_some(), "sample opens");
        let Some(full) = full else {
            return;
        };
        let target_page = full.page_count() / 2;
        let anchor = full.anchor(target_page);
        assert!(anchor.is_some(), "middle page has an anchor");
        let Some(anchor) = anchor else {
            return;
        };
        let place = full.spine_place(anchor);
        assert!(place.is_some(), "middle page has a stable spine place");
        let Some(place) = place else {
            return;
        };
        let target = OpenTarget {
            page: target_page,
            place: Some(anchor),
            spine: Some(place.clone()),
            pages: full.page_count(),
        };
        let opening = OpenBook::new(Arc::clone(&bytes), &typo, Some(target));
        assert!(opening.is_ok(), "incremental opening starts");
        let Some(mut opening) = opening.ok() else {
            return;
        };
        let preview = loop {
            match opening.step() {
                Ok(OpenStep::Continue) => {}
                Ok(OpenStep::Preview(book)) => break Some(book),
                Ok(OpenStep::Complete(_)) | Err(_) => break None,
            }
        };
        assert!(
            preview.is_some(),
            "saved place is previewed before completion"
        );
        let Some(preview) = preview else {
            return;
        };
        assert!(!preview.is_complete());
        assert!(preview.page_count() < full.page_count());
        assert!(
            preview.page_count() > 10,
            "extra neighbors widen the preview"
        );
        let target_local = preview.locate_spine(&place);
        assert!(target_local.is_some(), "preview contains the saved place");
        let Some(target_local) = target_local else {
            return;
        };

        let complete = loop {
            match opening.step() {
                Ok(OpenStep::Continue) => {}
                Ok(OpenStep::Preview(_)) => {}
                Ok(OpenStep::Complete(book)) => break Some(book),
                Err(_) => break None,
            }
        };
        assert!(complete.is_some(), "the remaining spine completes");
        let Some(complete) = complete else {
            return;
        };
        assert!(complete.is_complete());
        assert_eq!(complete.locate_spine(&place), Some(target_page));
        assert!(target_local < preview.page_count());
        assert_eq!(
            preview.same_page(target_local, &complete),
            Some(target_page)
        );
    }

    #[test]
    fn junk_is_refused() {
        assert_eq!(
            Book::open(Arc::new(b"not a zip".to_vec()), &Typography::default()).err(),
            Some(OpenError::Invalid)
        );
    }

    #[test]
    fn a_page_draws() {
        let book = Book::open(Arc::new(sample::epub(1, "S")), &Typography::default()).ok();
        let px = book.and_then(|b| b.render(1, 0.25, [0, 0, 198, 280]));
        assert_eq!(px.map(|p| p.rgba.len()), Some(198 * 280 * 4));
    }

    #[test]
    fn the_cover_comes_first_and_draws_in_colour() {
        let book = Book::open(Arc::new(sample::epub(1, "S")), &Typography::default()).ok();
        let px = book.and_then(|b| b.render(0, 0.25, [0, 0, 198, 280]));
        let (pixels, _) = px
            .as_ref()
            .map_or((&[][..], &[][..]), |p| p.rgba.as_chunks::<4>());
        // The sample cover is a dusk sky: plenty of saturated pixels.
        let colourful = pixels
            .iter()
            .filter(|[r, g, b, _]| r.max(g).max(b) - r.min(g).min(b) > 60)
            .count();
        assert!(colourful > 5_000, "{colourful}");
    }

    #[test]
    fn a_retypeset_book_keeps_its_place_in_bigger_type() {
        let book = Book::open(Arc::new(sample::epub(4, "S")), &Typography::default()).ok();
        let big = Typography {
            font_size: 26.0,
            ..Typography::default()
        };
        let bigger = book.as_ref().and_then(|b| b.retypeset(&big));
        assert!(
            book.is_some() && bigger.is_some(),
            "sample opens and re-sets"
        );
        let (Some(book), Some(bigger)) = (book, bigger) else {
            return;
        };
        assert!(bigger.page_count() > book.page_count());
        assert_eq!(bigger.title(), book.title());
        let page = book.page_count() / 2;
        let anchor = book.anchor(page);
        let found = anchor.map(|a| bigger.locate(a));
        let start = found.and_then(|p| bigger.anchor(p));
        assert!(start.is_some() && start <= anchor, "{start:?} {anchor:?}");
    }

    #[test]
    fn a_chapter_showing_the_cover_becomes_the_cover_page() {
        let Some(cover) = sample::cover_png().and_then(|b| Picture::probe(b).ok()) else {
            return assert!(sample::cover_png().is_some(), "cover encodes");
        };
        let cover = Arc::new(cover);
        let mut blocks = vec![
            Block::new(Content::Image {
                picture: cover.clone(),
                cover: false,
                width: None,
                links: Vec::new(),
            }),
            Block::text(mareader_reflow::Role::Paragraph, "text"),
        ];
        place_cover(&mut blocks, 0..2, cover.clone());
        assert_eq!(blocks.len(), 2);
        let first = blocks.first().map(|b| &b.content);
        assert!(matches!(first, Some(Content::Image { cover: true, .. })));
        // Elsewhere, it's added in front on a page of its own.
        let mut blocks = vec![Block::text(mareader_reflow::Role::Paragraph, "text")];
        place_cover(&mut blocks, 0..1, cover);
        assert_eq!(blocks.len(), 2);
        assert!(blocks.first().is_some_and(|b| b.break_before));
    }

    fn open(bytes: Vec<u8>, typo: &Typography) -> Option<Book> {
        Book::open(Arc::new(bytes), typo).ok()
    }

    fn page_of(book: &Book, title: &str) -> Option<usize> {
        let entry = book.toc().iter().find(|e| e.title == title);
        entry.map(|e| book.locate(e.at))
    }

    #[test]
    fn the_contents_list_chapters_and_parts_where_they_are() {
        let book = open(sample::epub(3, "S"), &Typography::default());
        let toc = book.as_ref().map_or(&[][..], Book::toc);
        let listed: Vec<(&str, usize)> = toc.iter().map(|e| (e.title.as_str(), e.depth)).collect();
        assert_eq!(
            listed,
            [
                ("Chapter 1", 0),
                ("Part 1.2", 1),
                ("Chapter 2", 0),
                ("Part 2.2", 1),
                ("Chapter 3", 0),
                ("Part 3.2", 1)
            ]
        );
        // The cover takes page 0; no entry is before the one above it.
        let pages: Vec<usize> = toc
            .iter()
            .filter_map(|e| Some(book.as_ref()?.locate(e.at)))
            .collect();
        assert_eq!(pages.first(), Some(&1));
        assert!(pages.windows(2).all(|w| w.first() <= w.get(1)), "{pages:?}");
        let chapters: Vec<usize> = pages.iter().step_by(2).copied().collect();
        assert!(
            chapters.windows(2).all(|w| w.first() < w.get(1)),
            "{pages:?}"
        );
        // Each part's place is its heading, whatever the type size.
        let big = Typography {
            font_size: 30.0,
            ..Typography::default()
        };
        for typo in [Typography::default(), big] {
            let book = open(sample::epub(3, "S"), &typo);
            for part in ["Part 1.2", "Part 2.2"] {
                let at = book
                    .as_ref()
                    .and_then(|b| b.toc().iter().find(|e| e.title == part));
                let at = at.map(|e| e.at);
                let block = at.and_then(|a| book.as_ref()?.blocks.get(a.block));
                let text = match block.map(|b| &b.content) {
                    Some(Content::Text(t)) => Some(t.text.as_str()),
                    _ => None,
                };
                assert_eq!(text, Some(part));
            }
        }
    }

    #[test]
    fn links_lead_to_their_pages_or_out_of_the_book() {
        let book = open(sample::epub(2, "S"), &Typography::default());
        assert!(book.is_some(), "open");
        let Some(book) = book else {
            return;
        };
        let links: Vec<Link> = (0..book.page_count()).flat_map(|p| book.links(p)).collect();
        let to = |page: Option<usize>| page.map(Destination::Page);
        let leads = |d: Option<Destination>| links.iter().any(|l| Some(&l.to) == d.as_ref());
        assert!(leads(to(page_of(&book, "Chapter 2"))), "{links:?}");
        assert!(leads(to(page_of(&book, "Part 1.2"))), "{links:?}");
        let web = "https://github.com/codewiththiha/mareader2".to_owned();
        assert!(leads(Some(Destination::Outside(web))), "{links:?}");
        // The boxes sit on the page, a line high.
        let (w, h) = book.page_size();
        for l in &links {
            let [x, y, lw, lh] = l.rect;
            assert!(x >= 0.0 && y >= 0.0 && x + lw <= w && y + lh <= h, "{l:?}");
            assert!(lh > 10.0 && lh < 60.0, "{l:?}");
        }
    }

    /// A book of `files`, stored: path, then contents; the package is
    /// `OPS/p.opf`.
    fn zipped(files: &[(&str, &str)]) -> Vec<u8> {
        use std::io::Write;
        let stored = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let container = "<?xml version='1.0'?><container version='1.0' \
            xmlns='urn:oasis:names:tc:opendocument:xmlns:container'><rootfiles>\
            <rootfile full-path='OPS/p.opf' media-type='application/oebps-package+xml'/>\
            </rootfiles></container>";
        let all = [
            ("mimetype", "application/epub+zip"),
            ("META-INF/container.xml", container),
        ];
        for (name, body) in all.iter().chain(files) {
            let _ = zip.start_file(*name, stored);
            let _ = zip.write_all(body.as_bytes());
        }
        zip.finish()
            .map(std::io::Cursor::into_inner)
            .unwrap_or_default()
    }

    #[test]
    fn an_svg_cover_page_in_the_spine_shows_once() {
        let opf = "<?xml version='1.0'?><package xmlns='http://www.idpf.org/2007/opf' \
            version='3.0' unique-identifier='id'><metadata \
            xmlns:dc='http://purl.org/dc/elements/1.1/'><dc:identifier id='id'>x\
            </dc:identifier><dc:title>T</dc:title><dc:language>en</dc:language></metadata>\
            <manifest><item id='c' href='c.svg' media-type='image/svg+xml' \
            properties='cover-image'/><item id='a' href='a.xhtml' \
            media-type='application/xhtml+xml'/><item id='n' href='n.xhtml' \
            media-type='application/xhtml+xml' properties='nav'/></manifest>\
            <spine><itemref idref='c'/><itemref idref='a'/></spine></package>";
        let svg = "<svg xmlns='http://www.w3.org/2000/svg' width='600' height='800'>\
            <rect width='600' height='800' fill='teal'/></svg>";
        let page = "<html xmlns='http://www.w3.org/1999/xhtml'><body><p>Words</p></body></html>";
        let nav = "<html xmlns='http://www.w3.org/1999/xhtml' \
            xmlns:epub='http://www.idpf.org/2007/ops'><body><nav epub:type='toc'>\
            <ol><li><a href='a.xhtml'>A</a></li></ol></nav></body></html>";
        let bytes = zipped(&[
            ("OPS/p.opf", opf),
            ("OPS/c.svg", svg),
            ("OPS/a.xhtml", page),
            ("OPS/n.xhtml", nav),
        ]);
        let book = open(bytes, &Typography::default());
        let blocks = book.as_ref().map(|b| Arc::clone(b.blocks()));
        let kinds: Vec<bool> = (blocks.iter().flat_map(|b| b.iter()))
            .map(|b| matches!(b.content, Content::Image { cover: true, .. }))
            .collect();
        assert_eq!(kinds, [true, false], "{kinds:?}");
    }

    #[test]
    fn a_book_without_contents_lists_its_headings() {
        let body = "<h1>One</h1><p>x</p><h2>Two  <em>parts</em></h2><p>y</p><h4>Deep</h4>";
        let chapter = rbook::epub::EpubChapter::unlisted("a.xhtml").xhtml_body(body);
        let bytes = Epub::builder().title("T").chapter(chapter).write().to_vec();
        let book = bytes.ok().and_then(|b| open(b, &Typography::default()));
        let toc = book.as_ref().map_or(&[][..], Book::toc);
        let listed: Vec<(&str, usize, usize)> = toc
            .iter()
            .map(|e| (e.title.as_str(), e.depth, e.at.block))
            .collect();
        assert_eq!(listed, [("One", 0, 0), ("Two parts", 1, 2), ("Deep", 3, 4)]);
    }
}
