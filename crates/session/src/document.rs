//! Either format behind one set of calls, so views never ask which.
use std::path::PathBuf;
use std::sync::Arc;

use mareader_epub::{Book, Destination, Format, Inserts, OpenTarget, SpinePlace};
use mareader_pdf::{Bounds, Link, LinkTarget, MAX_DEPTH, Tiling, web_address};
use mareader_pdf::{OutlineEntry, PageSize, PageText, Paper, Raster, RenderError};
use mareader_pdf::{Passage, PassageRun, TextLayer};
use mareader_reflow::{Anchor, InteractiveItem, ItemPixmap, PassageKind, Picture, Typography};

use crate::OpenError;
use crate::cover::read_picture;

/// A text file converted to an EPUB before it opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TextFile {
    pub format: Format,
    /// Its name, which titles a plain text file.
    pub name: String,
    /// The folder its Markdown pictures are read from.
    pub folder: Option<PathBuf>,
}

/// An open document. An EPUB's layout unit shows as one point.
pub enum Document {
    Pdf(mareader_pdf::Document),
    Epub {
        book: Box<Book>,
        sizes: Vec<PageSize>,
        /// The contents on this setting's pages.
        outline: Vec<OutlineEntry>,
        /// The preview's first page in the saved book's page numbering.
        page_offset: usize,
        /// The last known full-book page count, or the preview extent.
        page_total: usize,
    },
}

impl std::fmt::Debug for Document {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pdf(d) => d.fmt(f),
            Self::Epub { book, .. } => book.fmt(f),
        }
    }
}

impl Document {
    /// Opens by content, not name: every EPUB is a zip, set in `typo`.
    pub fn open(bytes: Arc<Vec<u8>>, typo: &Typography) -> Result<Self, OpenError> {
        if !mareader_epub::sniff(&bytes) {
            return Ok(Self::Pdf(mareader_pdf::Document::open(bytes)?));
        }
        Ok(Self::from_book(Book::open(bytes, typo)?))
    }

    /// Converts Markdown or plain text to an EPUB, then opens that.
    pub(crate) fn convert(
        bytes: &[u8],
        file: &TextFile,
        typo: &Typography,
    ) -> Result<Self, OpenError> {
        let folder = file.folder.as_deref();
        let read = |path: &str| folder.and_then(|f| read_picture(f, path));
        let epub = mareader_epub::convert(bytes, file.format, Some(&file.name), &read)?;
        Ok(Self::from_book(Book::open(Arc::new(epub), typo)?))
    }

    pub(crate) fn from_book(book: Book) -> Self {
        Self::from_book_at(book, 0, 0)
    }

    pub(crate) fn from_preview(book: Book, target: Option<&OpenTarget>) -> Self {
        let mut document = Self::from_book(book);
        if let Some(target) = target {
            document.set_preview_target(target);
        }
        document
    }

    fn from_book_at(book: Book, page_offset: usize, page_total: usize) -> Self {
        // Fixed pages keep their own shape; reflowed ones share one.
        let size = |page| {
            let (width, height) = book.size(page);
            PageSize { width, height }
        };
        let sizes = (0..book.page_count()).map(size).collect();
        let entry = |e: &mareader_epub::TocEntry| OutlineEntry {
            title: e.title.clone(),
            page: book.locate(e.at),
            depth: u8::try_from(e.depth).unwrap_or(u8::MAX).min(MAX_DEPTH),
        };
        let outline = book.toc().iter().map(entry).collect();
        Self::Epub {
            book: Box::new(book),
            sizes,
            outline,
            page_offset,
            page_total,
        }
    }

    pub(crate) fn set_preview_target(&mut self, target: &OpenTarget) {
        let Self::Epub {
            book,
            page_offset,
            page_total,
            ..
        } = self
        else {
            return;
        };
        if book.is_complete() {
            return;
        }
        let local = target
            .spine
            .as_ref()
            .and_then(|place| book.locate_spine(place))
            .or_else(|| {
                target
                    .spine
                    .is_none()
                    .then_some(target.place)
                    .flatten()
                    .map(|place| book.locate(place))
            })
            .map(|page| book.to_base(page))
            .unwrap_or(0);
        *page_offset = target.page.saturating_sub(local);
        *page_total = target
            .pages
            .max((*page_offset).saturating_add(book.base_count()));
    }

    /// A book's sound or video at `path`, read to hand to a player.
    pub fn media(&self, path: &str) -> Option<mareader_epub::Media> {
        match self {
            Self::Pdf(_) => None,
            Self::Epub { book, .. } => book.media(path),
        }
    }

    /// The subtitle files of a book's clip at `path`, for its player.
    pub fn subtitles(&self, path: &str) -> Vec<mareader_epub::Subtitle> {
        match self {
            Self::Pdf(_) => Vec::new(),
            Self::Epub { book, .. } => book.subtitles(path),
        }
    }

    /// What a book's trigger link `link` sets off.
    pub fn trigger(&self, link: usize) -> Vec<mareader_epub::Triggered> {
        match self {
            Self::Pdf(_) => Vec::new(),
            Self::Epub { book, .. } => book.trigger(link),
        }
    }

    /// A book's scripted chapters, for a script engine to run.
    pub fn scripts(&self) -> Vec<mareader_epub::Scripted> {
        match self {
            Self::Pdf(_) => Vec::new(),
            Self::Epub { book, .. } => book.scripts(),
        }
    }

    /// A book's narration, if its chapters have media overlays.
    pub fn narration(&self) -> Option<Arc<mareader_epub::Narration>> {
        match self {
            Self::Pdf(_) => None,
            Self::Epub { book, .. } => {
                let n = book.narration();
                (!n.phrases.is_empty()).then(|| Arc::clone(n))
            }
        }
    }

    /// A book's files, to read its narration's sound from.
    pub fn files(&self) -> Option<mareader_epub::Files> {
        match self {
            Self::Pdf(_) => None,
            Self::Epub { book, .. } => book.files(),
        }
    }

    /// At rest: a book lets go of layouts and decodes out of view.
    pub fn rest(&self) {
        if let Self::Epub { book, .. } = self {
            book.rest();
        }
    }

    /// The same book in other type; none for a PDF or on failure.
    pub fn retypeset(&self, typo: &Typography) -> Option<Self> {
        self.reset(typo, None)
    }

    /// The book in `typo` with `inserts` laid in; none keeps its own.
    pub fn reset(&self, typo: &Typography, inserts: Option<&Arc<Inserts>>) -> Option<Self> {
        self.wrap_reset(|book| book.reset(typo, inserts))
    }

    /// The results a book was asked to show; zero for none, or a PDF.
    pub fn inserts_key(&self) -> u64 {
        match self {
            Self::Pdf(_) => 0,
            Self::Epub { book, .. } => book.inserts_key(),
        }
    }

    /// Names a book's own layout, which results laid in keep.
    pub fn layout(&self) -> Option<u64> {
        match self {
            Self::Pdf(_) => None,
            Self::Epub { book, .. } => Some(book.layout()),
        }
    }

    /// Whether result `id` is laid in a book's pages.
    pub fn shows(&self, id: u128) -> bool {
        matches!(self, Self::Epub { book, .. } if book.shows(id))
    }

    /// The own page page `page` stands for; a PDF's pages are its own.
    pub fn to_base(&self, page: usize) -> usize {
        match self {
            Self::Pdf(_) => page,
            Self::Epub { book, .. } => book.to_base(page),
        }
    }

    /// The page showing own page `page`.
    pub fn from_base(&self, page: usize) -> usize {
        match self {
            Self::Pdf(_) => page,
            Self::Epub { book, .. } => book.from_base(page),
        }
    }

    /// The own pages page `page` stands for; a result's later ones, none.
    pub fn base_pages(&self, page: usize) -> std::ops::Range<usize> {
        match self {
            Self::Pdf(_) => page..page + 1,
            Self::Epub { book, .. } => book.base_pages(page),
        }
    }

    /// How many own pages: a book's results aside.
    pub fn base_count(&self) -> usize {
        match self {
            Self::Pdf(d) => d.page_count(),
            Self::Epub { book, .. } => book.base_count(),
        }
    }

    /// The saved-book page represented by a shown page.
    pub fn global_page(&self, page: usize) -> usize {
        self.page_offset().saturating_add(self.to_base(page))
    }

    /// The saved or estimated full-book page count, excluding inserts.
    pub fn global_page_count(&self) -> usize {
        match self {
            Self::Pdf(doc) => doc.page_count(),
            Self::Epub {
                book,
                page_offset,
                page_total,
                ..
            } => (*page_total).max((*page_offset).saturating_add(book.base_count())),
        }
    }

    /// The page number shown to readers while a bounded preview is open.
    pub fn display_page(&self, page: usize) -> usize {
        if self.is_complete() {
            page
        } else {
            self.global_page(page)
        }
    }

    /// The page total shown while a preview has not found the spine's end.
    pub fn display_page_count(&self) -> usize {
        if self.is_complete() {
            self.page_count()
        } else {
            self.global_page_count()
        }
    }

    /// The shown page for a saved global page, if it is in this window.
    pub fn local_page(&self, page: usize) -> Option<usize> {
        let base = page.checked_sub(self.page_offset())?;
        (base < self.base_count()).then(|| self.from_base(base))
    }

    pub fn page_offset(&self) -> usize {
        match self {
            Self::Pdf(_) => 0,
            Self::Epub { page_offset, .. } => *page_offset,
        }
    }

    /// The same book, perhaps set in other type; a PDF only as itself.
    pub fn same_book(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Epub { book, .. }, Self::Epub { book: o, .. }) => book.same_book(o),
            _ => std::ptr::eq(self, other),
        }
    }

    /// The page of `other` drawn just like page `page`, if any.
    pub fn same_page(&self, page: usize, other: &Self) -> Option<usize> {
        match (self, other) {
            (Self::Epub { book, .. }, Self::Epub { book: o, .. }) => book.same_page(page, o),
            _ => None,
        }
    }

    /// Where a reflowable page starts, to find again in other type.
    pub fn anchor(&self, page: usize) -> Option<Anchor> {
        match self {
            Self::Pdf(_) => None,
            Self::Epub { book, .. } => book.anchor(page),
        }
    }

    /// The page showing `anchor`; a PDF has none.
    pub fn locate(&self, anchor: Anchor) -> Option<usize> {
        match self {
            Self::Pdf(_) => None,
            Self::Epub { book, .. } => Some(book.locate(anchor)),
        }
    }

    /// A stable spine location for an EPUB anchor; none for PDFs.
    pub fn spine_place(&self, anchor: Anchor) -> Option<SpinePlace> {
        match self {
            Self::Pdf(_) => None,
            Self::Epub { book, .. } => book.spine_place(anchor),
        }
    }

    /// The anchor at a stable spine location, when it is in this book.
    pub fn anchor_spine(&self, place: &SpinePlace) -> Option<Anchor> {
        match self {
            Self::Pdf(_) => None,
            Self::Epub { book, .. } => book.anchor_spine(place),
        }
    }

    /// The local page showing a stable spine location, if available.
    pub fn locate_spine(&self, place: &SpinePlace) -> Option<usize> {
        match self {
            Self::Pdf(_) => None,
            Self::Epub { book, .. } => book.locate_spine(place),
        }
    }

    /// The shown page for an open target in this document.
    pub fn target_page(&self, target: &OpenTarget) -> Option<usize> {
        if let Some(place) = target.spine.as_ref() {
            return self.locate_spine(place).or_else(|| {
                self.is_complete()
                    .then_some(target.page)
                    .and_then(|page| self.local_page(page))
            });
        }
        target
            .place
            .and_then(|place| self.locate(place))
            .or_else(|| {
                self.is_complete()
                    .then_some(target.page)
                    .and_then(|page| self.local_page(page))
            })
            .or_else(|| self.local_page(target.page))
    }

    /// The anchor from an open target, preferring its stable locator.
    pub fn target_anchor(&self, target: &OpenTarget) -> Option<Anchor> {
        target
            .spine
            .as_ref()
            .and_then(|place| self.anchor_spine(place))
            .or_else(|| target.spine.is_none().then_some(target.place).flatten())
    }

    /// What of each page a continuous stream shows; a PDF streams whole
    /// pages.
    pub fn bands(&self) -> Option<&[mareader_reflow::Band]> {
        match self {
            Self::Pdf(_) => None,
            Self::Epub { book, .. } => Some(book.bands()),
        }
    }

    /// A PDF that opened without its end: pages may be missing.
    pub fn cut_short(&self) -> bool {
        matches!(self, Self::Pdf(d) if d.cut_short())
    }

    /// Whether its pages turn right to left, as a book may ask.
    pub fn right_to_left(&self) -> bool {
        matches!(self, Self::Epub { book, .. } if book.right_to_left())
    }

    /// How many pages show alone before spreads pair them, as a book asks.
    pub fn lead(&self) -> usize {
        match self {
            Self::Epub { book, .. } => book.lead(),
            Self::Pdf(_) => 0,
        }
    }

    /// Whether the text reflows: the typography settings apply.
    pub fn is_reflowable(&self) -> bool {
        matches!(self, Self::Epub { .. })
    }

    /// Whether `page` is a fixed-layout EPUB page drawn in native colour.
    pub fn is_fixed_page(&self, page: usize) -> bool {
        matches!(self, Self::Epub { book, .. } if book.fixed(page))
    }

    /// Interactive or animated elements on `page`.
    pub fn page_interactive(&self, page: usize) -> &[InteractiveItem] {
        match self {
            Self::Pdf(_) => &[],
            Self::Epub { book, .. } => book.interactive(page),
        }
    }

    /// Renders a single interactive item on `page` at `scale` pixels per point.
    pub fn render_interactive_item(
        &self,
        page: usize,
        item: usize,
        scale: f32,
        hovered: bool,
    ) -> Option<ItemPixmap> {
        let Self::Epub { book, .. } = self else {
            return None;
        };
        let size = self.size(page).ok()?;
        let scale = size.fit_scale(scale);
        book.render_interactive_item(page, item, scale, hovered)
    }

    /// Whether all spine items have been parsed and laid out.
    pub fn is_complete(&self) -> bool {
        match self {
            Self::Pdf(_) => true,
            Self::Epub { book, .. } => book.is_complete(),
        }
    }

    pub(crate) fn has_typography(&self, typo: &Typography) -> bool {
        match self {
            Self::Pdf(_) => true,
            Self::Epub { book, .. } => book.has_typography(typo),
        }
    }

    pub fn page_count(&self) -> usize {
        self.page_sizes().len()
    }

    pub fn page_sizes(&self) -> &[PageSize] {
        match self {
            Self::Pdf(d) => d.page_sizes(),
            Self::Epub { sizes, .. } => sizes,
        }
    }

    /// Chapters in reading order; empty when the file has none.
    pub fn outline(&self) -> &[OutlineEntry] {
        match self {
            Self::Pdf(d) => d.outline(),
            Self::Epub { outline, .. } => outline,
        }
    }

    /// The title as written; see `display_name` for what to show.
    pub fn raw_title(&self) -> Option<&str> {
        match self {
            Self::Pdf(d) => d.raw_title(),
            Self::Epub { book, .. } => book.title(),
        }
    }

    /// The scale `render` really uses: `scale`, lowered to fit the caps.
    pub fn fitted_scale(&self, page: usize, scale: f32) -> Option<f32> {
        Some(self.page_sizes().get(page)?.fit_scale(scale))
    }

    /// Draws page `page` (0-based) at `scale` pixels per point.
    pub fn render(&self, page: usize, scale: f32) -> Result<Raster, RenderError> {
        let book = match self {
            Self::Pdf(d) => return d.render(page, scale),
            Self::Epub { book, .. } => book,
        };
        let size = self.size(page)?;
        let scale = size.fit_scale(scale);
        // Truncated, as a PDF page's raster is.
        let w = ((size.width * scale) as u32).max(1);
        let h = ((size.height * scale) as u32).max(1);
        draw(book, page, scale, [0, 0, w, h])
    }

    /// Tile `col`, `row` of page `page` split at `scale`; see [`Tiling`].
    pub fn render_tile(
        &self,
        page: usize,
        scale: f32,
        col: u32,
        row: u32,
    ) -> Result<Raster, RenderError> {
        let book = match self {
            Self::Pdf(d) => return d.render_tile(page, scale, col, row),
            Self::Epub { book, .. } => book,
        };
        let tiling = Tiling::new(self.size(page)?, scale);
        let rect = tiling.rect(col, row).ok_or(RenderError::Failed)?;
        draw(book, page, tiling.scale, rect)
    }

    /// Page `page`'s characters and links.
    pub fn text_layer(&self, page: usize) -> Result<TextLayer, RenderError> {
        let book = match self {
            Self::Pdf(d) => return d.text_layer(page),
            Self::Epub { book, .. } => book,
        };
        self.size(page)?;
        let links = book.links(page).into_iter().filter_map(|l| {
            let target = match l.to {
                Destination::Page(page) => LinkTarget::Page(page),
                Destination::Note { page, text } => LinkTarget::Note { page, text },
                // The same schemes a PDF's links may use.
                Destination::Outside(a) => LinkTarget::Web(web_address(a.as_bytes())?),
                Destination::Media { path, video } => LinkTarget::Media { path, video },
                Destination::Transcript { card, track } => LinkTarget::Transcript { card, track },
                Destination::Trigger(link) => LinkTarget::Trigger {
                    link,
                    what: mareader_epub::Triggered::label(&book.trigger(link)),
                },
                Destination::Script { chapter, node } => LinkTarget::Script { chapter, node },
            };
            Some(Link {
                bounds: bounds(l.rect),
                target,
            })
        });
        let lines = book.text(page);
        let narrow =
            |(b, i): (usize, usize)| Some((u32::try_from(b).ok()?, u32::try_from(i).ok()?));
        let mut places: Vec<_> = lines
            .iter()
            .flat_map(|l| l.chars.iter().map(|c| c.at.and_then(narrow)))
            .collect();
        let tables = table_runs(&lines);
        let passages = passage_runs(&lines);
        let lines = lines.into_iter().map(|l| {
            let chars = l.chars.into_iter().map(|c| (c.ch, bounds(c.rect)));
            (l.wraps, chars.collect())
        });
        let mut text = PageText::from_lines(lines);
        if book.turned(page) {
            text.set_vertical();
        }
        places.truncate(text.len());
        Ok(TextLayer {
            text,
            links: links.collect(),
            places,
            figures: Vec::new(),
            tables,
            passages,
        })
    }

    /// Page `page`'s pictures, spared the ink dial; a PDF lists none.
    pub fn pictures(&self, page: usize) -> Vec<Bounds> {
        match self {
            Self::Pdf(_) => Vec::new(),
            Self::Epub { book, .. } => book.pictures(page).into_iter().map(bounds).collect(),
        }
    }

    /// Box `at` of a PDF's page drawn large; a book's pictures are its own.
    pub fn render_region(&self, page: usize, at: Bounds, side: u32) -> Result<Raster, RenderError> {
        match self {
            Self::Pdf(d) => d.render_region(page, at, side),
            Self::Epub { .. } => Err(RenderError::Failed),
        }
    }

    /// A book's picture at a point of page `page`; a PDF has none.
    pub fn picture_at(&self, page: usize, (x, y): (f32, f32)) -> Option<Arc<Picture>> {
        match self {
            Self::Pdf(_) => None,
            Self::Epub { book, .. } => book.picture_at(page, x, y),
        }
    }

    /// Page `page`'s colour glyphs, emoji mostly; a PDF lists none.
    pub fn emojis(&self, page: usize) -> Vec<Bounds> {
        match self {
            Self::Pdf(_) => Vec::new(),
            Self::Epub { book, .. } => book.emojis(page).into_iter().map(bounds).collect(),
        }
    }

    pub(crate) fn as_pdf(&self) -> Option<&mareader_pdf::Document> {
        match self {
            Self::Pdf(d) => Some(d),
            Self::Epub { .. } => None,
        }
    }

    /// Page `page`'s paper colour; a book's is the theme's, so it has none.
    pub fn paper(&self, page: usize) -> Result<Paper, RenderError> {
        if let Self::Pdf(d) = self {
            return d.paper(page);
        }
        self.size(page)?;
        Ok(Paper::default())
    }

    fn size(&self, page: usize) -> Result<PageSize, RenderError> {
        self.page_sizes()
            .get(page)
            .copied()
            .ok_or(RenderError::NoPage(page))
    }
}

/// Left, top, width, height as a box.
fn bounds([x, y, w, h]: [f32; 4]) -> Bounds {
    Bounds {
        x0: x,
        y0: y,
        x1: x + w,
        y1: y + h,
    }
}

fn draw(book: &Book, page: usize, scale: f32, region: [u32; 4]) -> Result<Raster, RenderError> {
    let px = book
        .render(page, scale, region)
        .ok_or(RenderError::Failed)?;
    Ok(Raster {
        width: px.width,
        height: px.height,
        pixels: px.rgba,
    })
}

/// The runs of `lines` in lists, figures and asides, alike ones joined.
fn passage_runs(lines: &[mareader_reflow::PageLine]) -> Vec<PassageRun> {
    let mut runs: Vec<PassageRun> = Vec::new();
    let mut at = 0;
    for line in lines {
        let end = at + line.chars.len();
        let kind = |k| match k {
            PassageKind::List => Passage::List,
            PassageKind::Figure => Passage::Figure,
            PassageKind::Aside => Passage::Aside,
        };
        let these: Vec<(Passage, usize)> =
            line.passages.iter().map(|&(k, n)| (kind(k), n)).collect();
        match runs.last_mut() {
            _ if these.is_empty() => {}
            Some((run, last)) if *last == these && run.end == at => run.end = end,
            _ => runs.push((at..end, these)),
        }
        at = end;
    }
    runs
}

/// Runs of characters in table cells, by table block, in text order.
fn table_runs(lines: &[mareader_reflow::PageLine]) -> Vec<(std::ops::Range<usize>, u32)> {
    let mut runs: Vec<(std::ops::Range<usize>, u32)> = Vec::new();
    let mut at = 0;
    for line in lines {
        let end = at + line.chars.len();
        let block = line.table.and_then(|b| u32::try_from(b).ok());
        match (block, runs.last_mut()) {
            (Some(b), Some((run, last))) if *last == b && run.end == at => run.end = end,
            (Some(b), _) => runs.push((at..end, b)),
            (None, _) => {}
        }
        at = end;
    }
    runs
}
