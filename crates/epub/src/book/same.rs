//! Cross-snapshot page identity and fallback mapping for [`Book`].

use std::sync::Arc;

use mareader_reflow::{Anchor, Block, BookFace, Content};

use super::{Book, SpinePlace};
use crate::splice::At;
use crate::xhtml::BOOK_FACE;

#[derive(Debug, Clone, PartialEq, Eq)]
enum BlockKey {
    Cover,
    Spine {
        href: String,
        item: usize,
        block: usize,
    },
}

impl Book {
    /// The page of `other` that looks just like page `page`, if any.
    pub fn same_page(&self, page: usize, other: &Self) -> Option<usize> {
        if Arc::ptr_eq(&self.flow, &other.flow) {
            return self.same_spliced_page(page, other);
        }
        let mapped = self.mapped_base_page(page, other)?;
        if !self.page_faces_match(page, other) {
            return None;
        }
        let normal = |book: &Self, page| {
            let excerpts = book.excerpts(page);
            if excerpts.is_empty() {
                return None;
            }
            excerpts
                .into_iter()
                .map(|(block, range)| Some((book.block_key(block)?, range)))
                .collect::<Option<Vec<_>>>()
        };
        let same = normal(self, page)
            .zip(normal(other, mapped))
            .is_some_and(|(a, b)| a == b)
            && self.size(page) == other.size(mapped)
            && self.column(page) == other.column(mapped)
            && self.fixed(page) == other.fixed(mapped)
            && self.turned(page) == other.turned(mapped);
        same.then_some(mapped)
    }

    /// Best-effort same-typography target when `same_page` rejects `page`.
    ///
    /// Keeps the prior raster as a stale fallback until the new page renders.
    pub fn fallback_page(&self, page: usize, other: &Self) -> Option<usize> {
        if Arc::ptr_eq(&self.flow, &other.flow) {
            return None;
        }
        self.mapped_base_page(page, other)
    }

    fn same_spliced_page(&self, page: usize, other: &Self) -> Option<usize> {
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

    fn mapped_base_page(&self, page: usize, other: &Self) -> Option<usize> {
        if !Arc::ptr_eq(&self.identity, &other.identity)
            || self.typo != other.typo
            || self.language != other.language
            || self.rtl != other.rtl
        {
            return None;
        }
        if let Some(s) = &self.spliced
            && !matches!(s.at(page), Some(At::Base(_)))
        {
            return None;
        }
        let anchor = self.anchor(page)?;
        let key = self.block_key(anchor.block)?;
        let mapped = other.locate_key(&key, anchor.offset)?;
        if let Some(s) = &other.spliced
            && !matches!(s.at(mapped), Some(At::Base(_)))
        {
            return None;
        }
        Some(mapped)
    }

    fn has_synthetic_cover(&self) -> bool {
        let shifted = self.spine.first().is_none_or(|s| s.blocks.start == 1);
        shifted
            && matches!(
                self.blocks.first().map(|b| &b.content),
                Some(Content::Image { cover: true, .. })
            )
    }

    fn block_key(&self, block: usize) -> Option<BlockKey> {
        if let Some(place) = self.spine_place(Anchor { block, offset: 0 }) {
            return Some(BlockKey::Spine {
                href: place.href,
                item: place.item,
                block: place.block,
            });
        }
        (block == 0 && self.has_synthetic_cover()).then_some(BlockKey::Cover)
    }

    fn locate_key(&self, key: &BlockKey, offset: usize) -> Option<usize> {
        match key {
            BlockKey::Cover => self
                .has_synthetic_cover()
                .then(|| self.locate(Anchor { block: 0, offset })),
            BlockKey::Spine { href, item, block } => self.locate_spine(&SpinePlace {
                href: href.clone(),
                item: *item,
                block: *block,
                offset,
            }),
        }
    }

    fn page_faces_match(&self, page: usize, other: &Self) -> bool {
        let all_same = self.faces.len() == other.faces.len()
            && self
                .faces
                .iter()
                .zip(other.faces.iter())
                .all(|(a, b)| a.family == b.family && face_signature(a) == face_signature(b));
        if all_same {
            return true;
        }
        for (block_idx, _) in self.excerpts(page) {
            let Some(block) = self.blocks.get(block_idx) else {
                continue;
            };
            if !block_faces_match(block, &self.faces, &other.faces) {
                return false;
            }
        }
        true
    }
}

fn face_signature(face: &BookFace) -> (u32, bool, usize) {
    (face.weight.to_bits(), face.italic, face.len())
}

fn family_faces_match(a: &[BookFace], b: &[BookFace], family: &str) -> bool {
    let mut left = a
        .iter()
        .filter(|face| face.family == family)
        .map(face_signature);
    let mut right = b
        .iter()
        .filter(|face| face.family == family)
        .map(face_signature);
    loop {
        match (left.next(), right.next()) {
            (Some(l), Some(r)) if l == r => {}
            (None, None) => return true,
            _ => return false,
        }
    }
}

fn list_faces_match(list: &[String], a: &[BookFace], b: &[BookFace]) -> bool {
    list.iter()
        .filter(|family| family.starts_with(BOOK_FACE))
        .all(|family| family_faces_match(a, b, family))
}

fn text_faces_match(text: &mareader_reflow::Text, a: &[BookFace], b: &[BookFace]) -> bool {
    text.faces.iter().all(|list| list_faces_match(list, a, b))
}

fn block_faces_match(block: &Block, a: &[BookFace], b: &[BookFace]) -> bool {
    if let Some(look) = block.frame.look.as_deref()
        && !list_faces_match(&look.faces, a, b)
    {
        return false;
    }
    match &block.content {
        Content::Text(t) => text_faces_match(t, a, b),
        Content::Table(t) => t
            .rows
            .iter()
            .flat_map(|row| &row.cells)
            .all(|cell| text_faces_match(cell, a, b)),
        Content::Rule | Content::Image { .. } | Content::Space(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use mareader_reflow::Typography;

    use crate::xhtml::BOOK_FACE;
    use crate::{Book, OpenBook, OpenStep, OpenTarget};

    fn zip_epub(chapters: &[(&str, &str)], cover: bool) -> Arc<Vec<u8>> {
        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut cursor);
            let stored = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            let deflated = zip::write::SimpleFileOptions::default();
            let _ = zip.start_file("mimetype", stored);
            let _ = zip.write_all(b"application/epub+zip");
            let _ = zip.start_file("META-INF/container.xml", deflated);
            let _ = zip.write_all(
                br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#,
            );
            let mut manifest = String::new();
            let mut spine = String::new();
            if cover {
                manifest.push_str(
                    r#"<item id="cover-img" href="cover.png" media-type="image/png" properties="cover-image"/>"#,
                );
            }
            for (idx, (href, _)) in chapters.iter().enumerate() {
                manifest.push_str(&format!(
                    r#"<item id="c{idx}" href="{href}" media-type="application/xhtml+xml"/>"#
                ));
                spine.push_str(&format!(r#"<itemref idref="c{idx}"/>"#));
            }
            let opf = format!(
                r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="id">urn:uuid:same-page-test</dc:identifier>
    <dc:title>Progressive</dc:title>
    <dc:language>en</dc:language>
  </metadata>
  <manifest>{manifest}</manifest>
  <spine>{spine}</spine>
</package>"#
            );
            let _ = zip.start_file("OEBPS/content.opf", deflated);
            let _ = zip.write_all(opf.as_bytes());
            if cover {
                let mut png = std::io::Cursor::new(Vec::new());
                let img = image::RgbImage::from_pixel(4, 4, image::Rgb([180, 180, 180]));
                let _ = img.write_to(&mut png, image::ImageFormat::Png);
                let _ = zip.start_file("OEBPS/cover.png", deflated);
                let _ = zip.write_all(&png.into_inner());
            }
            for (href, body) in chapters {
                let _ = zip.start_file(format!("OEBPS/{href}"), deflated);
                let _ = zip.write_all(body.as_bytes());
            }
            let _ = zip.finish();
        }
        Arc::new(cursor.into_inner())
    }

    fn xhtml(title: &str, paras: usize, style: &str) -> String {
        let mut body = format!("<h1>{title}</h1>");
        for i in 0..paras {
            body.push_str(&format!(
                "<p>Chapter {title} paragraph {i} with enough words to fill lines cleanly across the page.</p>"
            ));
        }
        format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>{title}</title>{style}</head><body>{body}</body></html>"#
        )
    }

    fn preview_and_complete(bytes: Arc<Vec<u8>>, target: OpenTarget) -> Option<(Book, Book)> {
        let typo = Typography::default();
        let mut open = OpenBook::new(bytes, &typo, Some(target)).ok()?;
        let OpenStep::Preview(preview) = open.step().ok()? else {
            return None;
        };
        let complete = loop {
            match open.step().ok()? {
                OpenStep::Continue => {}
                OpenStep::Complete(book) => break book,
                OpenStep::Preview(_) => return None,
            }
        };
        Some((preview, complete))
    }

    #[test]
    fn cover_page_zero_and_last_preview_page_adopt_across_complete() {
        let ch: Vec<(String, String)> = (0..8)
            .map(|i| (format!("c{i}.xhtml"), xhtml(&format!("Ch {i}"), 4, "")))
            .collect();
        let refs: Vec<(&str, &str)> = ch.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        let bytes = zip_epub(&refs, true);
        let pair = preview_and_complete(bytes, OpenTarget::default());
        assert!(pair.is_some(), "expected preview and complete");
        let Some((preview, complete)) = pair else {
            return;
        };
        assert_eq!(preview.same_page(0, &complete), Some(0));
        let last = preview.page_count().saturating_sub(1);
        assert_eq!(preview.same_page(last, &complete), Some(last));
    }

    #[test]
    fn mid_book_preview_adopts_even_when_lead_differs() {
        let ch: Vec<(String, String)> = (0..10)
            .map(|i| (format!("c{i}.xhtml"), xhtml(&format!("Ch {i}"), 4, "")))
            .collect();
        let refs: Vec<(&str, &str)> = ch.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        let bytes = zip_epub(&refs, true);
        let full = Book::open(Arc::clone(&bytes), &Typography::default()).ok();
        assert!(full.is_some());
        let Some(full) = full else {
            return;
        };
        let mid_page = full.page_count() / 2;
        let spine = full.anchor(mid_page).and_then(|a| full.spine_place(a));
        let target = OpenTarget {
            page: mid_page,
            place: full.anchor(mid_page),
            spine,
            pages: full.page_count(),
        };
        let pair = preview_and_complete(bytes, target);
        assert!(pair.is_some());
        let Some((preview, mut complete)) = pair else {
            return;
        };
        complete.lead = 1;
        assert_ne!(preview.lead(), complete.lead());
        for page in 0..preview.page_count() {
            assert!(
                preview.same_page(page, &complete).is_some(),
                "mid-book preview page {page} failed to adopt"
            );
        }
    }

    #[test]
    fn extra_unrelated_font_face_still_adopts_while_referenced_face_falls_back() {
        let ch: Vec<(String, String)> = (0..8)
            .map(|i| {
                let style = if i == 0 {
                    "<style>@font-face { font-family: ChapterOne; src: url(c1.ttf); } \
                     p { font-family: ChapterOne, serif; }</style>"
                } else {
                    ""
                };
                (format!("c{i}.xhtml"), xhtml(&format!("Ch {i}"), 4, style))
            })
            .collect();
        let refs: Vec<(&str, &str)> = ch.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        let bytes = zip_epub(&refs, false);
        let pair = preview_and_complete(bytes, OpenTarget::default());
        assert!(pair.is_some());
        let Some((preview, mut complete)) = pair else {
            return;
        };
        let unrelated = mareader_reflow::BookFace::new(
            format!("{BOOK_FACE}laterchapter"),
            vec![1, 2, 3, 4],
            400.0,
            false,
        );
        complete.faces = Arc::new(vec![unrelated]);
        assert_eq!(preview.same_page(0, &complete), Some(0));

        let used = mareader_reflow::BookFace::new(
            format!("{BOOK_FACE}chapterone"),
            vec![1, 2, 3, 4],
            400.0,
            false,
        );
        complete.faces = Arc::new(vec![used]);
        assert_eq!(preview.same_page(0, &complete), None);
        assert_eq!(preview.fallback_page(0, &complete), Some(0));
    }

    #[test]
    fn cancellable_open_pauses_and_resumes_with_updated_typography() {
        let ch: Vec<(String, String)> = (0..12)
            .map(|i| (format!("c{i}.xhtml"), xhtml(&format!("Ch {i}"), 12, "")))
            .collect();
        let refs: Vec<(&str, &str)> = ch.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        let bytes = zip_epub(&refs, false);
        let base = Typography::default();
        let open = OpenBook::new(bytes, &base, Some(OpenTarget::default())).ok();
        assert!(open.is_some());
        let Some(mut open) = open else {
            return;
        };
        let step = open.step().ok();
        assert!(matches!(step, Some(OpenStep::Preview(_))));
        let Some(OpenStep::Preview(preview)) = step else {
            return;
        };
        let larger = Typography {
            font_size: base.font_size + 4.0,
            ..base.clone()
        };
        let no_cancel = AtomicBool::new(false);
        let fast = preview.reset_cancellable(&larger, None, &no_cancel);
        assert!(
            fast.as_ref()
                .is_some_and(|b| !b.is_complete() && b.has_typography(&larger))
        );
        let cancel = AtomicBool::new(true);
        assert!(matches!(
            open.step_cancellable(&cancel),
            Ok(OpenStep::Continue)
        ));
        open.set_typography(&larger);
        cancel.store(false, Ordering::Relaxed);
        let done = loop {
            match open.step_cancellable(&cancel) {
                Ok(OpenStep::Continue) => {}
                Ok(OpenStep::Complete(book)) => break Some(book),
                _ => break None,
            }
        };
        assert!(done.is_some());
        let Some(done) = done else {
            return;
        };
        assert!(done.is_complete());
        assert!(done.has_typography(&larger));
        assert!(done.can_preview_reset(&base));
        let windowed =
            done.reset_preview(&base, None, mareader_reflow::Anchor::default(), &no_cancel);
        assert!(
            windowed
                .as_ref()
                .is_some_and(|b| !b.is_complete() && b.is_windowed())
        );
    }
}
