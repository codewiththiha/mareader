//! Chapter XHTML to blocks: html5ever's tokenizer, our element stack, no DOM.
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;

use html5ever::tendril::StrTendril;
use html5ever::tokenizer::states::RawKind;
use html5ever::tokenizer::{
    BufferQueue, Tag, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
};
use mareader_reflow::{
    Align, Anchor, Annotation, Area, Backdrop, BackdropSize, Block, Boxed, Case, Cell, Columns,
    Content, Dir, Edge, Emphasis, Features, Finish, Flex, Floated, Frame, Frame9, GradientShape,
    GridRow, Inline, Justify, Keep, Length, Links, Look, Outline, Passage, PassageKind, Picture,
    Rise, Role, Row, Script, Sheet, Spot, Table, Text, TextShadow, ThinVec, VAlign,
};

use crate::calc::Vars;
use crate::css::{
    self, BorderStyle, Computed, Ctx, Decl, Display, El, Float, FontFace, Len, Line, MAX_SHEET,
    PictureSize, Placing, PseudoEl, ROOT_PX, Stylesheet, TextAlign,
};
use crate::decor::{self, Span, Tint};
use crate::generated::{self, Content as Generated, Item, ListStyle};
use crate::href;
use crate::math;
use crate::media::{self, Clip};
use crate::query;
use crate::script;
use crate::subtitles;
use crate::svg::{self, Capture};
use crate::trigger;

/// Open elements tracked at once; deeper nesting is read as flat.
pub const MAX_DEPTH: usize = 256;
const MAX_COLUMNS: usize = 32;
/// The most rows a one-column table may have and still read as a box.
const MAX_BOX_ROWS: usize = 8;
const MAX_BARS: usize = 8;
/// The longest ruby note kept, in bytes.
const MAX_NOTE: usize = 256;
/// Runs set across vertical lines, per block.
const MAX_COMBINED: usize = 256;
/// Elements that set as one text block, kept whole without a box.
const TEXT_BLOCKS: [&str; 12] = [
    "p",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "li",
    "dt",
    "pre",
    "caption",
    "figcaption",
];
const MAX_CLASSES: usize = 16;
const MAX_ATTRS: usize = 16;
/// Earlier siblings kept per element, for `+` and `~` selectors.
const MAX_SIBLINGS: usize = 64;
/// Counters in scope past this are not made.
const MAX_COUNTERS: usize = 256;
/// What a book face's family is registered as: never a machine's own.
pub(crate) const BOOK_FACE: &str = mareader_reflow::BOOK_FACE;
/// Generic families: the reader's own faces stand in.
const GENERIC: &[&str] = &[
    "serif",
    "sans-serif",
    "monospace",
    "cursive",
    "fantasy",
    "system-ui",
    "ui-serif",
    "ui-sans-serif",
    "ui-monospace",
    "math",
    "emoji",
    "fangsong",
];
/// Stylesheets one chapter applies; later ones are ignored.
pub const MAX_SHEETS: usize = 16;
/// Sheets importing sheets, this deep.
const MAX_IMPORT_DEPTH: usize = 4;
/// Indents in body ems, as mareader's stylesheet has them.
const LIST_INSET: f32 = 1.6;
const QUOTE_INSET: f32 = 1.0;
/// Backgrounds fainter than this against the page are left out.
const MIN_FILL: f32 = 0.08;
/// A fixed page naming no viewport: the reflowed page box, in CSS px.
const DEFAULT_SHEET: (f32, f32) = (794.0, 1123.0);
/// The largest viewport side a fixed page may ask for.
const MAX_SHEET_SIDE: f32 = 10_000.0;
/// Rows a cell spans down, at most.
const MAX_SPAN_DOWN: u16 = 64;
/// Pictures set in one block's lines, at most.
const MAX_INLINES: usize = 256;
/// A picture this many text ems tall or wide, at most, sits in a line.
const INLINE_TALL: f32 = 3.0;
const INLINE_WIDE: f32 = 8.0;
/// The most ink a background under text takes, keeping it readable.
const MAX_TEXT_FILL: f32 = 0.16;
const TAB: &str = "    ";
/// Ids and links kept per book; past these, the rest are ignored.
pub const MAX_PLACES: usize = 1 << 20;
pub const MAX_LINKS: usize = 1 << 20;

/// Where a link leads, as its chapter wrote it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum Href {
    /// `path` or `path#id` in the archive, both decoded.
    Inside(String),
    /// Anything with a scheme, checked only when followed.
    Outside(String),
    /// Sound or video in the archive, played outside the reader.
    Media { path: String, video: bool },
    /// Shows or hides card `card`'s transcript; a track picks its language.
    Transcript { card: u32, track: Option<u32> },
    /// What a click sets off: its entry in [`Refs::triggers`].
    Trigger(u32),
}

/// The places a book's ids mark and its links' destinations.
#[derive(Debug, Default, Clone)]
pub(crate) struct Refs {
    /// `path` and `path#id` to where each starts.
    pub(crate) places: HashMap<String, Anchor>,
    /// Indexed by the target a link's text carries.
    pub(crate) hrefs: Vec<Href>,
    /// `path#id` to just past where each element ends.
    pub(crate) ends: HashMap<String, Anchor>,
    /// `path#id` of block elements to the blocks they hold.
    pub(crate) extents: HashMap<String, Extent>,
    /// Links marked as leading to a note.
    pub(crate) noterefs: HashSet<u32>,
    /// The book's fonts that designed text uses.
    pub(crate) faces: Vec<FontFace>,
    /// Clips with subtitle tracks, by the card that plays them.
    pub(crate) cards: Vec<subtitles::Card>,
    /// Each trigger link's actions, and the `path#id`s they act on.
    pub(crate) triggers: Vec<Vec<(trigger::Action, String)>>,
    /// Elements triggers show or hide, and their index by `path#id`.
    pub(crate) targets: Vec<trigger::Target>,
    pub(crate) target_keys: HashMap<String, usize>,
    /// Chapters with targets, to read again as triggers change them.
    pub(crate) chapters: Vec<trigger::Laid>,
    /// Clips by `path#id`: the file, if it plays, and whether a video.
    pub(crate) clips: HashMap<String, (Option<String>, bool)>,
}

/// The blocks an element with an id holds, and whether it is a note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Extent {
    pub(crate) blocks: Range<usize>,
    pub(crate) note: bool,
}

impl Refs {
    /// The place `key` marks, else the start of its file.
    pub(crate) fn place(&self, key: &str) -> Option<Anchor> {
        let file = key.split('#').next().unwrap_or_default();
        self.places
            .get(key)
            .or_else(|| self.places.get(file))
            .copied()
    }
}

/// Where a chapter's images and stylesheets come from.
pub(crate) trait Library {
    /// `path` is absolute in the archive; `None` if missing or unusable.
    fn picture(&mut self, path: &str) -> Option<Arc<Picture>>;
    fn stylesheet(&mut self, path: &str) -> Option<Arc<Stylesheet>>;
    /// A file an SVG links, as it is; `None` if missing or too large.
    fn bytes(&mut self, path: &str) -> Option<Arc<Vec<u8>>>;
    /// The type the manifest declares for `path`, if it lists it.
    fn media_type(&mut self, path: &str) -> Option<String>;
}

/// The images SVG at `base` links by `hrefs`, read from the book.
pub(crate) fn svg_links(library: &mut dyn Library, base: &str, hrefs: Vec<String>) -> Links {
    let take = hrefs.into_iter().take(svg::MAX_LINKS);
    take.filter_map(|h| {
        let path = href::resolve(base, &h)?;
        Some((h, library.bytes(&path)?))
    })
    .collect()
}

/// What an inline drawing is read into.
enum Caught {
    Svg(Box<Capture>),
    Math(Box<math::Capture>),
}

/// An inline `svg` or `math` being read: its markup and how it is set.
struct Drawing {
    capture: Caught,
    name: String,
    computed: Computed,
    shown: bool,
    alt: Option<String>,
}

/// What open elements pass down to the text inside them.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Style {
    code: bool,
    pre: bool,
    align: Align,
    sided: bool,
    /// Not shown at all: the head, scripts, `display: none`.
    hidden: bool,
    /// Inside SVG or MathML: no text, though SVG images still show.
    graphic: bool,
    /// Inside a link, by its index in [`Refs::hrefs`].
    link: Option<u32>,
    /// List items show no marker; inherited, as in CSS.
    unmarked: bool,
    /// Inside `<rt>`: a ruby note, kept apart from the text.
    note: bool,
}

struct Open {
    name: String,
    style: Style,
    computed: Computed,
    /// Its element, and its children so far: the latest, and a count.
    el: El,
    kids: Vec<El>,
    count: usize,
    /// Children by tag, for `:nth-of-type`.
    types: Vec<(String, usize)>,
    /// An ordered list's last item number.
    counter: u32,
    /// Its `path#id`, first block and whether it is a note.
    extent: Option<(String, usize, bool)>,
    /// It opened a box in [`State::boxes`].
    boxed: bool,
    /// Its part in the layout grid being read.
    part: Part,
    /// It is the float in [`State::float`].
    floated: bool,
    /// It looks laid out: a strong sign of a designed chapter.
    signals: bool,
    /// Its `::after` box, shown as it closes.
    after: Option<Box<Decl>>,
    /// The list, figure or aside it starts, if none of its kind is open.
    passage: Option<Arc<Passage>>,
}

/// The generated boxes rules give an element.
#[derive(Default)]
struct Pseudos {
    before: Option<Decl>,
    after: Option<Decl>,
    letter: Option<Decl>,
    line: Option<Decl>,
    marker: Option<Decl>,
}

/// What a counter property does to its counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Reset,
    Add,
    Set,
}

/// Whether `d` makes a `::before` or `::after` box.
fn generates(d: &Decl) -> bool {
    matches!(d.content, Some(Generated::Items(_))) && d.display != Some(Display::None)
}

/// A start tag the reader makes for a generated box.
fn synthetic(name: &str) -> Tag {
    Tag {
        kind: TagKind::StartTag,
        name: name.into(),
        self_closing: false,
        attrs: Vec::new(),
        had_duplicate_attributes: false,
    }
}

/// The bytes of a text's first letter, with punctuation before it.
fn first_letter(s: &str) -> usize {
    let mark = |c: char| {
        matches!(c as u32, 0x300..=0x36f | 0x1ab0..=0x1aff | 0x20d0..=0x20ff | 0xfe20..=0xfe2f)
            || matches!(c as u32, 0x102b..=0x103e | 0x1056..=0x1059 | 0x1062..=0x1064)
    };
    let (mut end, mut letter) = (0, false);
    for (i, c) in s.char_indices() {
        let next = i + c.len_utf8();
        match (letter, c) {
            (false, c) if c.is_alphanumeric() => (end, letter) = (next, true),
            (false, c) if !c.is_whitespace() && !c.is_alphanumeric() => end = next,
            (true, c) if mark(c) => end = next,
            _ => break,
        }
    }
    if letter { end } else { 0 }
}

/// An element's part in a layout grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    None,
    Table,
    Row,
    Cell,
    /// A flex container laying its items in a row, and one of its items.
    FlexBox,
    FlexItem,
    /// A flex container stacking its items, and one laid as a row of one.
    Stack,
    StackItem,
}

/// A book's box open around what is read (`docs/epub-layout.md`).
struct BoxOpen {
    spec: Boxed,
    /// Made at its first block; every block in it shares it.
    arc: Option<Arc<Boxed>>,
    /// The space waiting above its top border.
    gap: Gap,
    /// Its first block in [`State::blocks`].
    first: usize,
    /// Its background's full strength, drawn only if it holds no text.
    contrast: Option<f32>,
    /// Its finish at full strength, likewise.
    finish: Option<Arc<Finish>>,
}

/// The outermost layout table being read; nested ones read flat.
struct GridBuild {
    /// Its place in [`State::stack`].
    depth: usize,
    row: Option<RowBuild>,
    /// What its rows share.
    columns: Arc<Columns>,
    /// A flex row's items' default vertical place.
    flex: Option<VAlign>,
    /// Rows each column is still taken for by a cell spanning down.
    taken: Vec<u16>,
    /// A column stack's item, sure to stay a row of one.
    single: bool,
}

/// A grid row being read; its blocks hold a placeholder till it ends.
struct RowBuild {
    widths: Vec<Option<Length>>,
    valign: Vec<VAlign>,
    before: f32,
    placeholder: Arc<GridRow>,
    /// Its first block in [`State::blocks`].
    first: usize,
    /// The cell being read, and how many boxes hold the grid.
    column: Option<u16>,
    boxes: u8,
    starts: Vec<u16>,
    spans: Vec<u16>,
    grow: Vec<f32>,
    /// Rows each cell spans down.
    down: Vec<u16>,
    /// The column after the last cell.
    next: u16,
}

fn is_void(name: &str) -> bool {
    matches!(
        name,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

/// Elements that end the text before them and start anew.
fn is_block(name: &str) -> bool {
    heading(name).is_some()
        || matches!(
            name,
            "p" | "div"
                | "li"
                | "ul"
                | "ol"
                | "dl"
                | "dt"
                | "dd"
                | "blockquote"
                | "pre"
                | "section"
                | "article"
                | "header"
                | "footer"
                | "aside"
                | "nav"
                | "figure"
                | "figcaption"
                | "body"
                | "main"
                | "address"
                | "center"
                | "table"
                | "caption"
                | "thead"
                | "tbody"
                | "tfoot"
                | "tr"
                | "td"
                | "th"
                | "hr"
        )
}

fn heading(name: &str) -> Option<u8> {
    match name {
        "h1" => Some(1),
        "h2" => Some(2),
        "h3" => Some(3),
        "h4" => Some(4),
        "h5" => Some(5),
        "h6" => Some(6),
        _ => None,
    }
}

/// Elements whose content the tokenizer must read as raw text.
fn is_raw(name: &str) -> bool {
    matches!(
        name,
        "style" | "script" | "title" | "template" | "textarea" | "xmp"
    )
}

/// HTML's collapsible spaces; U+00A0 and friends stay as written.
fn is_html_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}')
}

fn attr<'t>(tag: &'t Tag, name: &str) -> Option<&'t str> {
    tag.attrs
        .iter()
        .find(|a| &*a.name.local == name)
        .map(|a| &*a.value)
}

/// Whether attribute `name` lists one of `words`, as `epub:type` does.
fn typed(tag: &Tag, name: &str, words: &[&str]) -> bool {
    let listed = |w: &str| words.iter().any(|x| w.eq_ignore_ascii_case(x));
    attr(tag, name).is_some_and(|v| v.split_whitespace().any(listed))
}

/// The passage an element starts, by its name or semantics.
fn passage_kind(name: &str, tag: &Tag) -> Option<PassageKind> {
    match name {
        "ul" | "ol" | "dl" => Some(PassageKind::List),
        "figure" => Some(PassageKind::Figure),
        "aside" => Some(PassageKind::Aside),
        _ if typed(tag, "epub:type", &["sidebar"]) => Some(PassageKind::Aside),
        _ => match attr(tag, "role")?.trim() {
            "list" => Some(PassageKind::List),
            "figure" => Some(PassageKind::Figure),
            "complementary" => Some(PassageKind::Aside),
            _ => None,
        },
    }
}

/// An element holding a note, by EPUB or ARIA semantics.
fn is_note(tag: &Tag) -> bool {
    typed(
        tag,
        "epub:type",
        &["footnote", "endnote", "rearnote", "note"],
    ) || typed(tag, "role", &["doc-footnote", "doc-endnote"])
}

/// A link to a note, by EPUB or ARIA semantics.
fn is_noteref(tag: &Tag) -> bool {
    typed(tag, "epub:type", &["noteref"]) || typed(tag, "role", &["doc-noteref"])
}

/// The table being read; only the outermost one builds rows.
struct TableBuild {
    table: Table,
    frame: Frame,
    caption: Text,
    /// Tables inside this one, read as text in its cells.
    nested: usize,
    in_cell: bool,
    in_caption: bool,
    in_head: bool,
}

impl TableBuild {
    fn cell(&mut self) -> Option<&mut Text> {
        if self.in_caption {
            return Some(&mut self.caption);
        }
        if !self.in_cell {
            return None;
        }
        self.table.rows.last_mut()?.cells.last_mut()
    }
}

struct State<'a> {
    library: &'a mut dyn Library,
    refs: &'a mut Refs,
    /// The book's index of this chapter's first block.
    first: usize,
    /// Ids waiting for the next block, which they mark.
    pending: Vec<String>,
    /// The chapter's own path, for resolving its links.
    base: &'a str,
    sheets: Vec<Arc<Stylesheet>>,
    /// An inline `<style>` being read.
    style_text: String,
    /// In `<ruby>`: where the base starts, and where it ended at `<rt>`.
    ruby: Option<(usize, Option<usize>)>,
    /// The ruby note being read.
    note: String,
    /// The chapter is written vertically.
    turned: bool,
    /// A run set across vertical lines: its element's depth and start.
    combining: Option<(usize, usize)>,
    /// Whether that `<style>`'s `media` applies.
    style_applies: bool,
    /// `html`, styled before its head's sheets; its custom values wait.
    root: Option<El>,
    blocks: Vec<Block>,
    current: Option<Block>,
    table: Option<TableBuild>,
    /// By each `<table>` in order: whether it only lays things out.
    layouts: Vec<bool>,
    /// The chapter's image maps by name, read ahead.
    maps: HashMap<String, Vec<media::MapArea>>,
    /// Links the next picture block carries, as its map's areas.
    areas: Vec<Area>,
    /// A `<video>` or `<audio>` being read, shown as a card at its end.
    media: Option<media::Pending>,
    /// Open `epub:switch`es: their depth, and whether a case showed.
    switches: Vec<(usize, bool)>,
    /// An open `<picture>`: its depth, and the source it picked.
    sources: Option<(usize, Option<String>)>,
    /// How many `<table>` tags have started.
    tables: usize,
    /// Layout tables open around the text being read.
    laying: usize,
    stack: Vec<Open>,
    /// A collapsed space waits for the next visible character.
    space: bool,
    /// The next block opens a chapter: it starts a page.
    chapter: bool,
    /// The float open, by its place in the stack; inner ones read in line.
    float: Option<(usize, Arc<Floated>)>,
    /// The book's page colour, against which backgrounds show.
    page: css::Rgb,
    /// The book's own text colour, which the theme's ink stands for.
    book_ink: css::Rgb,
    /// A list item's marker, waiting for its first block.
    marker: Option<String>,
    marker_colour: Option<css::Rgb>,
    /// The first block of an outermost list keeps the full gap above.
    list_opened: bool,
    /// Inside `<pre>`, where one leading newline is dropped.
    pre_opened: bool,
    /// The root's children, as `Open::kids` keeps an element's.
    top: Vec<El>,
    top_count: usize,
    /// Margins and padding waiting above the next block.
    gap: Gap,
    /// The open block's line height, in px.
    block_line: f32,
    /// Looks made in this chapter; the next equal one is shared.
    looks: Vec<Arc<Look>>,
    /// The chapter has a style of its own: a sheet or a `style` attribute.
    styled: bool,
    signals: Signals,
    /// The book's boxes open around what is read, outermost first.
    boxes: Vec<BoxOpen>,
    grid: Option<GridBuild>,
    /// An inline drawing read to its end before it is shown.
    drawing: Option<Drawing>,
    /// The cascade the next start tag takes: a generated box's.
    forced: Option<Decl>,
    /// The next box opened stands for an `<hr>`.
    rule_next: bool,
    /// Counters in scope: name, value, and their element's depth.
    counters: Vec<(String, i32, usize)>,
    /// Quotes open, which pick the next marks.
    quotes: usize,
    /// `::first-letter` and `::first-line` waiting for text, by their depth.
    initial: Option<(usize, Decl)>,
    first_line: Option<(usize, Decl)>,
    /// A floated inline starting a block, by depth: a drop cap.
    drop: Option<usize>,
    /// A list item's marker picture, waiting with its marker.
    marker_picture: Option<Arc<Picture>>,
    /// A fixed-layout chapter: its viewport, once read, and its page.
    fixed: bool,
    viewport: Option<(f32, f32)>,
    sheet: Option<Arc<Sheet>>,
    /// The root's ground was the body's, so the body box leaves it.
    grounded: bool,
    /// Containing blocks open on a fixed page, innermost last.
    places: Vec<Place>,
    /// The chapter's triggers, read ahead.
    triggers: trigger::Chapter,
    /// Targets by id that triggers have shown or hidden.
    overrides: HashMap<String, bool>,
    /// What clicking the picture being read sets off.
    inline_link: Option<u32>,
    /// A scripted chapter's place in [`Refs::chapters`].
    script_at: Option<usize>,
}

/// A fixed page's containing block: its depth, rect and positioned box.
#[derive(Clone)]
struct Place {
    depth: usize,
    rect: [f32; 4],
    spot: Option<Arc<Spot>>,
}

/// Margins and fixed space, in px: margins collapse.
#[derive(Debug, Default, Clone, Copy)]
struct Space {
    positive: f32,
    negative: f32,
    fixed: f32,
}

impl Space {
    fn margin(&mut self, px: f32) {
        if px >= 0.0 {
            self.positive = self.positive.max(px);
        } else {
            self.negative = self.negative.min(px);
        }
    }

    fn fixed(&mut self, px: f32) {
        self.fixed += self.positive + self.negative + px;
        (self.positive, self.negative) = (0.0, 0.0);
    }

    fn total(&self) -> f32 {
        self.fixed + self.positive + self.negative
    }
}

/// Vertical space waiting for the next block, in px.
#[derive(Debug, Default, Clone, Copy)]
struct Gap {
    all: Space,
    /// The same without paragraphs' own margins: what a reader's gap meets.
    outer: Space,
}

impl Gap {
    /// A margin; `own` is a paragraph's, which a reader's gap may replace.
    fn margin(&mut self, px: f32, own: bool) {
        self.all.margin(px);
        if !own {
            self.outer.margin(px);
        }
    }

    /// Space margins don't collapse through, like padding or a blank line.
    fn fixed(&mut self, px: f32) {
        self.all.fixed(px);
        self.outer.fixed(px);
    }

    fn take(&mut self) -> f32 {
        self.take_both().0
    }

    /// The whole space, and the space without paragraphs' own margins.
    fn take_both(&mut self) -> (f32, f32) {
        let both = (self.all.total(), self.outer.total());
        *self = Self::default();
        both
    }

    fn total(&self) -> f32 {
        self.all.total()
    }
}

/// What tells a designed chapter from prose (`docs/epub-layout.md`).
#[derive(Debug, Default)]
struct Signals {
    /// Layout tables, flex, borders, block backgrounds, widths, `vh`.
    strong: bool,
    faces: bool,
    letters: bool,
    /// Paragraph sizes seen, in hundredths of an em.
    sizes: Vec<u32>,
    /// Characters of text, and those inside what signalled.
    text: usize,
    boxed: usize,
}

impl Signals {
    fn designed(&self) -> bool {
        let light = [self.faces, self.letters, self.sizes.len() >= 3];
        // Prose with a framed figure or note stays prose.
        let laid_out = self.text <= SHORT_CHAPTER || self.boxed * 10 >= self.text * 3;
        (self.strong && laid_out) || light.iter().filter(|s| **s).count() >= 2
    }
}

/// A chapter this many characters long, at most, reads as a laid-out page.
const SHORT_CHAPTER: usize = 800;

mod boxes;
mod closing;
mod elements;
mod images;
mod layout;
mod links;
mod lists;
mod media_elements;
mod placement;
mod style;
mod tables;
mod text;
mod text_style;

/// A colour's background, the book's text colour and its page colour.
type Grounds = (css::Rgb, css::Rgb, css::Rgb);

/// How far a book's text colour may fade toward the paper, staying legible.
const MAX_FADE: f32 = 0.55;
/// Borders may stay fainter: a book's light rules are meant quiet.
const MAX_BORDER_FADE: f32 = 0.85;
/// Below this gap between the book's text and page, its colours mean little.
const MIN_SPAN: f32 = 0.3;

/// `ink` on the page's greys, faded by how far its contrast falls short.
fn drawn(ink: css::Rgb, (ground, base, page): Grounds, most: f32) -> Option<css::Rgb> {
    let unit = |c: css::Rgb| c.map(|v| f32::from(v) / 255.0);
    let luma = |[r, g, b]: [f32; 3]| 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let span = (luma(unit(base)) - luma(unit(page))).abs();
    if span < MIN_SPAN {
        return None;
    }
    let colour = unit(ink);
    let grey = luma(colour);
    let contrast = (grey - luma(unit(ground))).abs() / span;
    let off = colour.map(|v| v - grey);
    // Never darker than the hue needs: black holds none.
    let need = off.iter().fold(0.0f32, |n, &o| n.max(-o));
    let fade = (1.0 - contrast).max(need).clamp(0.0, most);
    if fade < 0.03 && off.iter().all(|o| o.abs() < 0.04) {
        return None;
    }
    // The most of the hue that fits around this grey.
    let room = off.iter().fold(1.0f32, |k, &o| match o {
        o if o > 1e-4 => k.min((1.0 - fade) / o),
        o if o < -1e-4 => k.min(fade / -o),
        _ => k,
    });
    Some(off.map(|o| ((fade + o * room) * 255.0).round().clamp(0.0, 255.0) as u8))
}

/// The box `name` draws or sits in; a cell's column sets its width.
fn boxed(name: &str, c: &Computed, d: &Decl, cell: bool) -> Option<Boxed> {
    if matches!(name, "tr" | "thead" | "tbody" | "tfoot" | "caption" | "hr") {
        return None;
    }
    let mut edges = c.border_style.map(edge);
    for (e, clear) in edges.iter_mut().zip(c.border_clear) {
        if clear && *e != Edge::None {
            *e = Edge::Clear;
        }
    }
    let mut border = c.border.map(|w| w.max(0.0) / ROOT_PX);
    for (w, e) in border.iter_mut().zip(&edges) {
        if *e == Edge::None {
            *w = 0.0;
        }
    }
    let side = |i: usize| match c.margin.get(i).copied().unwrap_or(Len::Auto) {
        Len::Auto => None,
        Len::Percent(p) | Len::Vw(p) => Some(Length::Percent(p)),
        Len::Mix { px, pct, .. } => Some(Length::Mix {
            em: px / ROOT_PX,
            percent: pct,
        }),
        _ => Some(Length::Em(c.margin_px(i) / ROOT_PX)),
    };
    let zero = Some(Length::Em(0.0));
    let margin = if cell { [zero; 2] } else { [side(3), side(1)] };
    let width = match c.width {
        _ if cell => None,
        Len::Px(px) if px > 0.0 => Some(Length::Em(px / ROOT_PX)),
        Len::Percent(p) | Len::Vw(p) if p > 0.0 => Some(Length::Percent(p)),
        Len::Mix { px, pct, .. } => mixed(px, pct),
        _ => None,
    };
    let flex = matches!(c.display, Display::Flex | Display::Grid);
    let centre = match d.height {
        Some(Len::Vh(v)) if flex && v > 0.0 => Some((v / 100.0).min(1.0)),
        _ => None,
    };
    let padding = c.padding.map(|p| p.max(0.0) / ROOT_PX);
    let flat = margin.iter().all(|m| m.is_none() || *m == zero);
    let drawn = border.iter().any(|w| *w > 0.0);
    // A share of the screen is the page's, not a length to scale.
    let height = match c.height {
        _ if matches!(d.height, Some(Len::Vh(_))) => None,
        Len::Px(px) if px > 0.0 && !cell => Some(px / ROOT_PX),
        _ => None,
    };
    let apart = padding[1] > 0.0 || padding[3] > 0.0 || width.is_some() || centre.is_some();
    (drawn || apart || !flat).then_some(Boxed {
        margin,
        border,
        edges,
        padding,
        width,
        centre,
        height,
        radius: radius(c),
        ..Boxed::default()
    })
}

/// A border style as the page draws it.
fn edge(s: BorderStyle) -> Edge {
    match s {
        BorderStyle::None => Edge::None,
        BorderStyle::Solid => Edge::Solid,
        BorderStyle::Double => Edge::Double,
        BorderStyle::Dashed => Edge::Dashed,
        BorderStyle::Dotted => Edge::Dotted,
        BorderStyle::Groove => Edge::Groove,
        BorderStyle::Ridge => Edge::Ridge,
        BorderStyle::Inset => Edge::Inset,
        BorderStyle::Outset => Edge::Outset,
    }
}

/// A finish with its gradients no stronger than text can sit on.
fn capped(f: &Finish) -> Finish {
    let mut f = f.clone();
    for layer in &mut f.layers {
        if let mareader_reflow::Layer::Gradient(g) = layer {
            for stop in &mut g.stops {
                stop.1 = stop.1.min(MAX_TEXT_FILL);
            }
        }
    }
    f
}

/// Whether the book draws an `<hr>` itself: borders, a ground, a picture.
fn styled_rule(d: &Decl) -> bool {
    let bordered = (d.border_style.iter().zip(&d.border_width)).any(|(s, w)| {
        let thin = matches!(w, Some(Len::Px(px)) if *px <= 0.0);
        matches!(s, Some(s) if *s != BorderStyle::None) && !thin
    });
    let shadowed = d.shadows.as_ref().is_some_and(|s| !s.is_empty());
    let layered = d.layers.as_ref().is_some_and(|l| !l.is_empty());
    bordered
        || shadowed
        || layered
        || d.background == Some(true)
        || d.background_image == Some(true)
        || d.frame.as_ref().is_some_and(Option::is_some)
}

/// A box's corner radius as the book sets it.
fn radius(c: &Computed) -> Option<Length> {
    match c.radius {
        Len::Px(px) if px > 0.0 => Some(Length::Em(px / ROOT_PX)),
        Len::Percent(p) if p > 0.0 => Some(Length::Percent(p.min(50.0))),
        Len::Mix { px, pct, .. } => mixed(px, pct.min(50.0)),
        _ => None,
    }
}

/// A box drawn only for its background: its width, padding and height.
fn bare(c: &Computed) -> Boxed {
    let width = match c.width {
        Len::Px(px) if px > 0.0 => Some(Length::Em(px / ROOT_PX)),
        Len::Percent(p) | Len::Vw(p) if p > 0.0 => Some(Length::Percent(p)),
        Len::Mix { px, pct, .. } => mixed(px, pct),
        _ => None,
    };
    let height = match c.height {
        Len::Px(px) if px > 0.0 => Some(px / ROOT_PX),
        _ => None,
    };
    Boxed {
        padding: c.padding.map(|p| p.max(0.0) / ROOT_PX),
        width,
        height,
        radius: radius(c),
        ..Boxed::default()
    }
}

/// Whether a small picture in `parent` is set in a line of text.
fn in_line(parent: &str) -> bool {
    let heading = parent.len() == 2 && parent.starts_with('h') && parent != "hr";
    heading
        || matches!(
            parent,
            "p" | "span"
                | "a"
                | "li"
                | "dt"
                | "dd"
                | "em"
                | "strong"
                | "b"
                | "i"
                | "u"
                | "sup"
                | "sub"
                | "small"
                | "big"
                | "q"
                | "cite"
                | "label"
                | "ruby"
        )
}

/// A small picture's size in ems of its text, to sit in a line.
fn inline_size(picture: &Picture, c: &Computed) -> Option<(f32, f32)> {
    if matches!(c.width, Len::Percent(_) | Len::Vw(_) | Len::Mix { .. }) {
        return None;
    }
    let (pw, ph) = (picture.width as f32, picture.height as f32);
    let size = c.size.max(1.0);
    let set = |l: Len| match l {
        Len::Px(px) if px > 0.0 => Some(px),
        _ => None,
    };
    if pw <= 0.0 || ph <= 0.0 {
        return None;
    }
    let (w, h) = match (set(c.width), set(c.height)) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) => (w, w * ph / pw),
        (None, Some(h)) => (h * pw / ph, h),
        (None, None) => (pw, ph),
    };
    let fits = h <= INLINE_TALL * size && w <= INLINE_WIDE * size;
    fits.then(|| (w / size, h / size))
}

/// A grid cell's width as the book sets it.
fn cell_width(c: &Computed) -> Option<Length> {
    match c.width {
        Len::Px(px) if px > 0.0 => Some(Length::Em(px / ROOT_PX)),
        Len::Percent(p) | Len::Vw(p) if p > 0.0 => Some(Length::Percent(p)),
        Len::Mix { px, pct, .. } => mixed(px, pct),
        _ => None,
    }
}

impl RowBuild {
    fn new(first: usize) -> Self {
        Self {
            widths: Vec::new(),
            valign: Vec::new(),
            before: 0.0,
            placeholder: Arc::new(GridRow::default()),
            first,
            column: None,
            boxes: 0,
            starts: Vec::new(),
            spans: Vec::new(),
            grow: Vec::new(),
            down: Vec::new(),
            next: 0,
        }
    }
}

/// A `width` attribute in pixels, as ems of the root.
fn attr_width(tag: &Tag) -> Option<Length> {
    let px: f32 = attr(tag, "width")?
        .trim()
        .trim_end_matches("px")
        .parse()
        .ok()?;
    (px > 0.0 && px.is_finite()).then(|| Length::Em((px / ROOT_PX).min(200.0)))
}

/// An image's width as the book sets it, in reader ems or percent.
fn picture_width(c: &Computed) -> Option<Length> {
    match c.width {
        Len::Px(px) if px > 0.0 => Some(Length::Em((px / ROOT_PX).min(200.0))),
        Len::Percent(p) | Len::Vw(p) if p > 0.0 => Some(Length::Percent(p.min(100.0))),
        Len::Mix { px, pct, .. } => mixed(px, pct.min(100.0)),
        _ => None,
    }
}

/// A computed sum as ems and a column share, if it comes out wide.
fn mixed(px: f32, pct: f32) -> Option<Length> {
    let rough = px + pct * css::BASE_WIDTH / 100.0;
    (rough > 0.0).then_some(Length::Mix {
        em: px / ROOT_PX,
        percent: pct,
    })
}

/// What HTML attributes say about style, beneath every sheet.
fn hints(tag: &Tag, name: &str) -> Decl {
    let mut d = Decl::default();
    if let Some(a) = attr(tag, "align") {
        d = Decl::parse(&format!("text-align: {a}"));
    }
    let sized = matches!(
        name,
        "img" | "table" | "td" | "th" | "col" | "image" | "svg"
    );
    if let Some(w) = attr(tag, "width").filter(|_| sized) {
        let w = w.trim();
        let unit = if w.ends_with('%') { "" } else { "px" };
        d.width = Decl::parse(&format!("width: {w}{unit}")).width;
    }
    let ink = match name {
        "font" => attr(tag, "color"),
        "body" => attr(tag, "text"),
        _ => None,
    };
    if let Some(c) = ink {
        d.ink = css::colour(&c.trim().to_ascii_lowercase()).flatten();
    }
    if let Some(dir) = attr(tag, "dir") {
        d.dir = match dir.trim().to_ascii_lowercase().as_str() {
            "rtl" => Some(Dir::Rtl),
            "ltr" => Some(Dir::Ltr),
            "auto" => Some(Dir::Auto),
            _ => None,
        };
    }
    if let Some(c) = attr(tag, "bgcolor") {
        d.background = Some(true);
        d.fill = css::colour(&c.trim().to_ascii_lowercase());
    }
    if let Some(t) = attr(tag, "type").filter(|_| matches!(name, "ol" | "ul" | "li")) {
        // The letters' case matters here, unlike in CSS.
        let word = match t.trim() {
            "1" => "decimal",
            "a" => "lower-alpha",
            "A" => "upper-alpha",
            "i" => "lower-roman",
            "I" => "upper-roman",
            other => other,
        };
        d.list_style = generated::list_style(word).or(d.list_style);
    }
    if let Some(v) = attr(tag, "valign").filter(|_| matches!(name, "td" | "th" | "tr")) {
        d.valign = Decl::parse(&format!("vertical-align: {}", v.trim())).valign;
    }
    d
}

/// `e` with family `names`, registered with `t`; too many keep the block's.
fn faced(t: &mut Text, names: Option<Vec<String>>, e: Emphasis) -> Emphasis {
    let Some(names) = names else {
        return e;
    };
    let at = match t.faces.iter().position(|f| *f == names) {
        Some(at) => at,
        None if t.faces.len() < usize::from(u8::MAX) => {
            t.faces.push(names);
            t.faces.len() - 1
        }
        None => return e,
    };
    let face = u8::try_from(at + 1).unwrap_or(0);
    Emphasis { face, ..e }
}

/// Drops columns blank in every row, such as spacers.
fn drop_blank_columns(table: &mut Table) {
    let columns = table.rows.iter().map(|r| r.cells.len()).max().unwrap_or(0);
    let filled = |i: usize| {
        let cell = |r: &Row| r.cells.get(i).is_some_and(|c| !c.is_blank());
        table.rows.iter().any(cell)
    };
    let keep: Vec<bool> = (0..columns).map(filled).collect();
    if !keep.contains(&true) {
        return;
    }
    for row in &mut table.rows {
        let mut kept = keep.iter();
        row.cells.retain(|_| kept.next().copied().unwrap_or(true));
    }
}

/// One table as the scan sees it.
struct Scanned {
    /// Its place among the chapter's tables.
    at: usize,
    pictured: bool,
    /// Head cells or a caption: a table of data.
    data: bool,
    /// Paragraphs or divisions inside its cells.
    blocks: bool,
    /// Each row's cells: whether each holds anything.
    rows: Vec<Vec<bool>>,
    in_cell: bool,
}

impl Scanned {
    /// Pictures; or, with no heads, block cells beside a spacer column.
    fn layout(&self) -> bool {
        let columns = self.rows.iter().map(Vec::len).max().unwrap_or(0);
        let filled = |i: usize| self.rows.iter().any(|r| r.get(i) == Some(&true));
        let full = (0..columns).filter(|&i| filled(i)).count();
        let spacer = full >= 2 && full < columns;
        // A single column of blocks is a box, as a framed note.
        let boxed = columns == 1 && self.rows.len() <= MAX_BOX_ROWS;
        self.pictured || (!self.data && self.blocks && (spacer || boxed))
    }

    fn fill(&mut self) {
        if !self.in_cell {
            return;
        }
        if let Some(cell) = self.rows.last_mut().and_then(|r| r.last_mut()) {
            *cell = true;
        }
    }
}

/// Reads each `<table>`'s shape, in order, before the chapter is read.
struct Scan(RefCell<(Vec<bool>, Vec<Scanned>)>);

impl TokenSink for Scan {
    type Handle = ();

    fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
        let (flags, open) = &mut *self.0.borrow_mut();
        let tag = match token {
            Token::TagToken(tag) => tag,
            Token::CharacterTokens(text) => {
                if !text.trim().is_empty() {
                    open.iter_mut().for_each(Scanned::fill);
                }
                return TokenSinkResult::Continue;
            }
            Token::EOFToken => {
                while let Some(t) = open.pop() {
                    close_scanned(flags, &t);
                }
                return TokenSinkResult::Continue;
            }
            _ => return TokenSinkResult::Continue,
        };
        let name: &str = &tag.name;
        let start = tag.kind == TagKind::StartTag;
        match (start, name) {
            (true, "table") => {
                if !tag.self_closing {
                    open.push(Scanned {
                        at: flags.len(),
                        pictured: false,
                        data: false,
                        blocks: false,
                        rows: Vec::new(),
                        in_cell: false,
                    });
                }
                flags.push(false);
            }
            (false, "table") => {
                if let Some(t) = open.pop() {
                    close_scanned(flags, &t);
                }
            }
            (true, "img" | "image" | "svg") => {
                for t in open.iter_mut() {
                    t.pictured = true;
                    t.fill();
                }
            }
            (true, "tr") => {
                if let Some(t) = open.last_mut() {
                    t.rows.push(Vec::new());
                }
            }
            (true, "td" | "th") => {
                if let Some(t) = open.last_mut() {
                    if t.rows.is_empty() {
                        t.rows.push(Vec::new());
                    }
                    if let Some(row) = t.rows.last_mut().filter(|r| r.len() < MAX_COLUMNS) {
                        row.push(false);
                    }
                    t.in_cell = true;
                    t.data |= name == "th";
                }
            }
            (false, "td" | "th") => {
                if let Some(t) = open.last_mut() {
                    t.in_cell = false;
                }
            }
            (true, "thead" | "caption") => {
                if let Some(t) = open.last_mut() {
                    t.data = true;
                }
            }
            (true, raw) if is_raw(raw) && !tag.self_closing => {
                return TokenSinkResult::RawData(RawKind::Rawtext);
            }
            (true, block) if is_block(block) => {
                if let Some(t) = open.last_mut().filter(|t| t.in_cell) {
                    t.blocks = true;
                }
            }
            _ => {}
        }
        TokenSinkResult::Continue
    }
}

fn close_scanned(flags: &mut [bool], t: &Scanned) {
    if let Some(f) = flags.get_mut(t.at) {
        *f = t.layout();
    }
}

/// Which of the chapter's tables only lay things out; none without one.
fn layout_tables(xhtml: &str) -> Vec<bool> {
    if !xhtml.contains("<table") && !xhtml.contains("<TABLE") {
        return Vec::new();
    }
    let sink = Scan(RefCell::new((Vec::new(), Vec::new())));
    let tokenizer = Tokenizer::new(sink, TokenizerOpts::default());
    let input = BufferQueue::default();
    input.push_back(StrTendril::from_slice(xhtml));
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    tokenizer.sink.0.into_inner().0
}

fn put(text: &mut Text, s: &str, emphasis: Emphasis, link: Option<u32>) {
    // Hidden text keeps its room but leads nowhere.
    match link.filter(|_| !emphasis.hidden) {
        Some(target) => text.push_link(s, emphasis, target),
        None => text.push(s, emphasis),
    }
}

struct Sink<'a>(RefCell<State<'a>>);

impl TokenSink for Sink<'_> {
    type Handle = ();

    fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
        let mut s = self.0.borrow_mut();
        if s.drawing.is_some() {
            s.draw(&token);
            return TokenSinkResult::Continue;
        }
        match token {
            Token::TagToken(tag) => match tag.kind {
                TagKind::StartTag => {
                    if s.start(&tag) {
                        return TokenSinkResult::RawData(RawKind::Rawtext);
                    }
                    s.begin_drawing(&tag);
                }
                TagKind::EndTag => s.end(&tag.name),
            },
            Token::CharacterTokens(text) => s.characters(&text),
            Token::EOFToken => s.eof(),
            _ => {}
        }
        TokenSinkResult::Continue
    }
}

/// Appends a spine picture filling its page, its own size when `fixed`.
pub(crate) fn read_svg_page(
    path: &str,
    library: &mut dyn Library,
    (out, refs): (&mut Vec<Block>, &mut Refs),
    fixed: bool,
) {
    let Some(picture) = library.picture(path) else {
        return;
    };
    let start = Anchor {
        block: out.len(),
        offset: 0,
    };
    refs.places.entry(path.to_owned()).or_insert(start);
    let side = |n: u32| (n as f32).clamp(1.0, MAX_SHEET_SIDE);
    let sheet = fixed.then(|| Sheet {
        width: side(picture.width),
        height: side(picture.height),
        ground: None,
    });
    let mut block = Block::new(Content::Image {
        picture,
        cover: true,
        width: None,
        links: Vec::new(),
    });
    block.break_before = true;
    block.frame.sheet = sheet.map(Arc::new);
    out.push(block);
}

/// Appends the blocks of `xhtml`, found at `path`; the first starts a page.
pub(crate) fn read_chapter(
    xhtml: &str,
    path: &str,
    library: &mut dyn Library,
    (out, refs): (&mut Vec<Block>, &mut Refs),
) {
    read(
        xhtml,
        (path, false, None),
        HashMap::new(),
        library,
        (out, refs),
    );
}

/// Appends a fixed-layout page: its blocks laid at its own viewport.
pub(crate) fn read_fixed_chapter(
    xhtml: &str,
    path: &str,
    library: &mut dyn Library,
    (out, refs): (&mut Vec<Block>, &mut Refs),
) {
    read(
        xhtml,
        (path, true, None),
        HashMap::new(),
        library,
        (out, refs),
    );
}

/// Reads a chapter again with targets shown or hidden as `overrides` says.
pub(crate) fn reread_chapter(
    xhtml: &str,
    laid: &trigger::Laid,
    overrides: HashMap<String, bool>,
    library: &mut dyn Library,
    (out, refs): (&mut Vec<Block>, &mut Refs),
) {
    let (path, at) = (laid.path.as_str(), Some(laid.at));
    read(
        xhtml,
        (path, laid.fixed, at),
        overrides,
        library,
        (out, refs),
    );
}

fn read(
    xhtml: &str,
    (path, fixed, at): (&str, bool, Option<usize>),
    overrides: HashMap<String, bool>,
    library: &mut dyn Library,
    (out, refs): (&mut Vec<Block>, &mut Refs),
) {
    let first = out.len();
    let found = script::find(xhtml, path);
    let laid_at = at.unwrap_or(refs.chapters.len());
    // Read again, a page may have dropped its own scripts.
    let script_at = (at.is_some() || !found.code.is_empty()).then_some(laid_at);
    let links = refs.hrefs.len();
    let start = Anchor {
        block: first,
        offset: 0,
    };
    refs.places.entry(path.to_owned()).or_insert(start);
    let sink = Sink(RefCell::new(State {
        library,
        refs,
        first,
        pending: Vec::new(),
        base: path,
        sheets: Vec::new(),
        style_text: String::new(),
        ruby: None,
        note: String::new(),
        turned: false,
        combining: None,
        style_applies: true,
        root: None,
        blocks: Vec::new(),
        current: None,
        table: None,
        layouts: layout_tables(xhtml),
        maps: media::image_maps(xhtml),
        areas: Vec::new(),
        media: None,
        switches: Vec::new(),
        sources: None,
        tables: 0,
        laying: 0,
        stack: Vec::new(),
        space: false,
        chapter: true,
        float: None,
        page: [255; 3],
        book_ink: [0; 3],
        marker: None,
        marker_colour: None,
        list_opened: false,
        pre_opened: false,
        top: Vec::new(),
        top_count: 0,
        gap: Gap::default(),
        block_line: ROOT_PX * 1.2,
        looks: Vec::new(),
        styled: false,
        signals: Signals::default(),
        boxes: Vec::new(),
        grid: None,
        drawing: None,
        forced: None,
        rule_next: false,
        counters: Vec::new(),
        quotes: 0,
        initial: None,
        first_line: None,
        drop: None,
        marker_picture: None,
        fixed,
        viewport: None,
        sheet: None,
        grounded: false,
        places: Vec::new(),
        triggers: trigger::read(xhtml),
        overrides,
        inline_link: None,
        script_at,
    }));
    let tokenizer = Tokenizer::new(sink, TokenizerOpts::default());
    let input = BufferQueue::default();
    input.push_back(StrTendril::from_slice(xhtml));
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    let state = tokenizer.sink.0.into_inner();
    let mut blocks = state.blocks;
    if fixed {
        // One page: every block on it, and a page even when it is blank.
        let (width, height) = state.viewport.unwrap_or(DEFAULT_SHEET);
        let blank = || {
            Arc::new(Sheet {
                width,
                height,
                ground: None,
            })
        };
        let sheet = state.sheet.unwrap_or_else(blank);
        let mut end = Block::new(Content::Space(0.0));
        end.break_before = blocks.is_empty();
        blocks.push(end);
        for block in &mut blocks {
            block.frame.sheet = Some(Arc::clone(&sheet));
        }
    }
    if !state.triggers.targets.is_empty() || script_at.is_some() {
        let blocks = first..first + blocks.len();
        let (path, links) = (path.to_owned(), links..state.refs.hrefs.len());
        state.refs.chapters.push(trigger::Laid {
            path,
            blocks,
            links,
            fixed,
            at: laid_at,
            scripts: found,
        });
    }
    out.extend(blocks);
}

/// A viewport `<meta>`'s size in CSS px, as `width=1200, height=1600`.
fn viewport(content: &str) -> Option<(f32, f32)> {
    let mut size = (None, None);
    for item in content.split([',', ';']) {
        let Some((key, value)) = item.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_end_matches("px").parse::<f32>().ok();
        let value = value
            .filter(|v| v.is_finite() && *v >= 1.0)
            .map(|v| v.min(MAX_SHEET_SIDE));
        match key.trim().to_ascii_lowercase().as_str() {
            "width" => size.0 = value,
            "height" => size.1 = value,
            _ => {}
        }
    }
    size.0.zip(size.1)
}

/// A length on a fixed page in its CSS px: viewport units are the page's.
fn placed_px(l: Len, base: f32, (width, height): (f32, f32)) -> Option<f32> {
    match l {
        Len::Vw(n) => Some(n * width / 100.0),
        Len::Vh(n) => Some(n * height / 100.0),
        other => other.px(ROOT_PX, base),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    mod blocks;
    mod clips;
    mod design;
    mod images;
    mod inline;
    mod layout;
    mod lists;
    mod sheets;
    mod sources;

    use mareader_reflow::Rise;

    /// In-memory images and stylesheets, keyed by absolute path.
    #[derive(Default)]
    struct Shelf {
        pictures: HashMap<String, Arc<Picture>>,
        sheets: HashMap<String, Arc<Stylesheet>>,
        files: HashMap<String, Arc<Vec<u8>>>,
    }

    impl Library for Shelf {
        fn picture(&mut self, path: &str) -> Option<Arc<Picture>> {
            self.pictures.get(path).cloned()
        }

        fn stylesheet(&mut self, path: &str) -> Option<Arc<Stylesheet>> {
            self.sheets.get(path).cloned()
        }

        fn bytes(&mut self, path: &str) -> Option<Arc<Vec<u8>>> {
            self.files.get(path).cloned()
        }

        /// Clips are typed by name; everything else as unlisted.
        fn media_type(&mut self, path: &str) -> Option<String> {
            let t = match path.rsplit('.').next()? {
                "mp4" => "video/mp4",
                "webm" => "video/webm",
                "mp3" => "audio/mpeg",
                "ogg" => "audio/ogg",
                "bin" => "application/octet-stream",
                _ => return None,
            };
            Some(t.to_owned())
        }
    }

    const BASE: &str = "/OEBPS/text/c1.xhtml";

    fn read_refs(xhtml: &str) -> (Vec<Block>, Refs) {
        let (mut out, mut refs) = (Vec::new(), Refs::default());
        read_chapter(xhtml, BASE, &mut Shelf::default(), (&mut out, &mut refs));
        (out, refs)
    }

    fn read_with(shelf: &mut Shelf, xhtml: &str) -> Vec<Block> {
        let mut out = Vec::new();
        read_chapter(xhtml, BASE, shelf, (&mut out, &mut Refs::default()));
        out
    }

    fn read(xhtml: &str) -> Vec<Block> {
        read_with(&mut Shelf::default(), xhtml)
    }

    fn text(b: &Block) -> Option<&Text> {
        match &b.content {
            Content::Text(t) => Some(t),
            _ => None,
        }
    }

    fn texts(blocks: &[Block]) -> Vec<&str> {
        blocks
            .iter()
            .filter_map(text)
            .map(|t| t.text.as_str())
            .collect()
    }

    fn role(b: Option<&Block>) -> Option<Role> {
        b.and_then(text).map(|t| t.role)
    }

    fn spans(b: Option<&Block>) -> Vec<(&str, Emphasis)> {
        let Some(t) = b.and_then(text) else {
            return Vec::new();
        };
        t.spans
            .iter()
            .filter_map(|s| Some((t.text.get(s.range.clone())?, s.emphasis)))
            .collect()
    }

    fn png(w: u32, h: u32) -> Option<Arc<Picture>> {
        let img = image::RgbImage::new(w, h);
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).ok()?;
        Picture::probe(out.into_inner()).ok().map(Arc::new)
    }

    #[test]
    fn avoiding_a_break_before_holds_the_block_before() {
        let b = read(
            "<p>a</p><p>quote</p>\
             <p style='page-break-before: avoid; text-align: right'>source</p><p>c</p>",
        );
        let held: Vec<bool> = b.iter().map(|b| b.frame.keep.with_next).collect();
        assert_eq!(held, [false, true, false, false]);
    }

    #[test]
    fn lists_indent_mark_and_tighten_their_items() {
        let b = read(
            "<p>Before</p><ul><li>one</li><li>two<ul><li>deep</li></ul></li></ul>\
             <ol start='4'><li>four<li>five</ol><p>After</p>",
        );
        assert_eq!(
            texts(&b),
            ["Before", "one", "two", "deep", "four", "five", "After"]
        );
        let frames: Vec<(f32, Option<&str>, bool)> = b
            .iter()
            .map(|b| (b.frame.inset, b.frame.marker.as_deref(), b.frame.tight))
            .collect();
        assert_eq!(
            frames,
            [
                (0.0, None, false),
                (1.6, Some("•"), false),
                (1.6, Some("•"), true),
                (3.2, Some("◦"), true),
                (1.6, Some("4."), false),
                (1.6, Some("5."), true),
                (0.0, None, false),
            ]
        );
    }

    #[test]
    fn tables_read_rows_cells_and_heads() {
        let b = read(
            "<table><caption>Prices</caption><thead><tr><th>Item<th>Cost</thead>\
             <tbody><tr><td>Tea<td><p>3</p><p>each</p></tr>\
             <tr><td>Rice<table><tr><td>in</td><td>sacks</td></tr></table></td></tr>\
             </tbody></table><p>after</p>",
        );
        assert_eq!(texts(&b), ["Prices", "after"]);
        assert_eq!(b.first().map(|b| b.frame.align), Some(Align::Center));
        let table = b.get(1).and_then(|b| match &b.content {
            Content::Table(t) => Some(t),
            _ => None,
        });
        let rows: Vec<(Vec<&str>, bool)> = table
            .map(|t| t.rows.iter())
            .into_iter()
            .flatten()
            .map(|r| (r.cells.iter().map(|c| c.text.as_str()).collect(), r.head))
            .collect();
        assert_eq!(
            rows,
            [
                (vec!["Item", "Cost"], true),
                (vec!["Tea", "3\neach"], false),
                (vec!["Rice in sacks"], false),
            ]
        );
    }

    #[test]
    fn rules_and_images_are_blocks_and_missing_images_show_alt() {
        let mut shelf = Shelf::default();
        if let Some(p) = png(4, 2) {
            shelf.pictures.insert("/OEBPS/images/a.png".into(), p);
        }
        let b = read_with(
            &mut shelf,
            "<p>one</p><hr/><div class='c' style='text-align:center'>\
             <img src='../images/a.png' alt='A'/></div>\
             <img src='gone.png' alt='Gone'/><img src='https://x/y.png'/>\
             <svg><image xlink:href='../images/a.png'/></svg>",
        );
        let kinds: Vec<&str> = b
            .iter()
            .map(|b| match &b.content {
                Content::Text(_) => "text",
                Content::Rule => "rule",
                Content::Space(_) => "space",
                Content::Image { .. } => "image",
                Content::Table(_) => "table",
            })
            .collect();
        assert_eq!(kinds, ["text", "rule", "image", "text", "image"]);
        assert_eq!(b.get(2).map(|b| b.frame.align), Some(Align::Center));
        let italic = Emphasis {
            italic: true,
            ..Emphasis::default()
        };
        assert_eq!(spans(b.get(3)), [("Gone", italic)]);
    }

    /// Block `i`'s picture, if it is one.
    fn picture_of(b: &[Block], i: usize) -> Option<Arc<Picture>> {
        match &b.get(i)?.content {
            Content::Image { picture, .. } => Some(Arc::clone(picture)),
            _ => None,
        }
    }

    fn kinds(b: &[Block]) -> Vec<&'static str> {
        b.iter()
            .map(|b| match &b.content {
                Content::Text(_) => "text",
                Content::Rule => "rule",
                Content::Space(_) => "space",
                Content::Image { .. } => "image",
                Content::Table(_) => "table",
            })
            .collect()
    }

    /// Block `i`'s grid row and item.
    fn cell_of(b: &[Block], i: usize) -> Option<(Arc<GridRow>, u16)> {
        b.get(i)
            .and_then(|b| b.frame.cell.clone())
            .map(|c| (c.row, c.column))
    }

    #[test]
    fn stylesheets_style_by_tag_and_class() {
        let mut shelf = Shelf::default();
        let linked = Stylesheet::parse(
            ".it { font-style: italic } .gone { display: none }",
            "/OEBPS/css/book.css",
        );
        shelf
            .sheets
            .insert("/OEBPS/css/book.css".into(), Arc::new(linked));
        let b = read_with(
            &mut shelf,
            "<head><link rel='stylesheet' href='../css/book.css'/>\
             <style>p.c { text-align: center } span.b { font-weight: bold }</style></head>\
             <p class='c'>mid <span class='b it'>x</span></p>\
             <p class='gone'>hidden</p><p style='font-style:italic'>y</p>",
        );
        assert_eq!(texts(&b), ["mid x", "y"]);
        assert_eq!(b.first().map(|b| b.frame.align), Some(Align::Center));
        let e = |italic, bold| Emphasis {
            italic,
            bold,
            ..Emphasis::default()
        };
        assert_eq!(spans(b.first()), [(" x", e(true, true))]);
        assert_eq!(spans(b.get(1)), [("y", e(true, false))]);
    }

    #[test]
    fn ids_mark_their_block_or_their_place_in_its_text() {
        let (_, refs) = read_refs(
            "<h1 id='top'>Title</h1><p>ab <span id='mid'>c</span></p>\
             <div id='empty'></div><p><a name='old'></a>d</p>\
             <table><tr><td id='cell'>e</td></tr></table><p id='end'></p>",
        );
        let at = |key: &str| {
            let a = refs.place(&format!("{BASE}{key}"));
            a.map(|a| (a.block, a.offset))
        };
        assert_eq!(at(""), Some((0, 0)));
        assert_eq!(at("#top"), Some((0, 0)));
        assert_eq!(at("#mid"), Some((1, 2)));
        assert_eq!(at("#empty"), Some((2, 0)));
        assert_eq!(at("#old"), Some((2, 0)));
        assert_eq!(at("#cell"), Some((3, 0)));
        // Past the last block: where the next chapter will start.
        assert_eq!(at("#end"), Some((4, 0)));
        // An unknown id falls back to its file's start.
        assert_eq!(at("#nowhere"), Some((0, 0)));
    }

    #[test]
    fn links_mark_their_words_and_keep_where_they_lead() {
        let (blocks, refs) = read_refs(
            "<p>See <a href='c2.xhtml#n%201'>the <i>first</i> note</a> \
             or <a href='https://example.org/'>a site</a>.</p>\
             <p><a href='#top'>Back</a> <a href='javascript:x()'>run</a>\
             <a href=''>none</a></p>",
        );
        let links = |b: Option<&Block>| {
            let t = b.and_then(text);
            let words = |l: &mareader_reflow::Link| {
                let s = t.and_then(|t| t.text.get(l.range.clone()));
                (s.unwrap_or_default().to_owned(), l.target)
            };
            t.map(|t| t.links.iter().map(words).collect::<Vec<_>>())
        };
        let first = links(blocks.first()).unwrap_or_default();
        assert_eq!(first, [("the first note".into(), 0), ("a site".into(), 1)]);
        let second = links(blocks.get(1)).unwrap_or_default();
        assert_eq!(second, [("Back".into(), 2), ("run".into(), 3)]);
        let inside = |s: &str| Href::Inside(s.to_owned());
        assert_eq!(
            refs.hrefs,
            [
                inside("/OEBPS/text/c2.xhtml#n 1"),
                Href::Outside("https://example.org/".into()),
                inside(&format!("{BASE}#top")),
                Href::Outside("javascript:x()".into()),
            ]
        );
    }

    fn look(b: Option<&Block>) -> Option<&Look> {
        b.and_then(|b| b.frame.look.as_deref())
    }

    fn read_all(shelf: &mut Shelf, xhtml: &str) -> (Vec<Block>, Refs) {
        let (mut out, mut refs) = (Vec::new(), Refs::default());
        read_chapter(xhtml, BASE, shelf, (&mut out, &mut refs));
        (out, refs)
    }

    /// Each picture block's links: where they lead, over which share.
    fn picture_links<'r>(blocks: &[Block], refs: &'r Refs) -> Vec<Vec<(&'r Href, [f32; 4])>> {
        let lead = |a: &Area| Some((refs.hrefs.get(usize::try_from(a.target).ok()?)?, a.rect));
        (blocks.iter())
            .filter_map(|b| match &b.content {
                Content::Image { links, .. } => Some(links.iter().filter_map(lead).collect()),
                _ => None,
            })
            .collect()
    }

    fn pictures_of(blocks: &[Block]) -> Vec<Arc<Picture>> {
        (blocks.iter())
            .filter_map(|b| match &b.content {
                Content::Image { picture, .. } => Some(Arc::clone(picture)),
                _ => None,
            })
            .collect()
    }
}
