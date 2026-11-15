//! Blocks set by parley at their frame's width, then cut into pages.
use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, Mutex, PoisonError};

use hypher::Lang;
use parley::{
    Affinity, Alignment, AlignmentOptions, BaseDirection, Cursor as Caret, FontFamily,
    FontFamilyName, FontFeatures, FontStyle, FontWeight, GenericFamily, IndentOptions, InlineBox,
    InlineBoxKind, Layout, LineHeight, OverflowWrap, PositionedLayoutItem, RangedBuilder,
    Selection, StyleProperty, VerticalAlign,
};
use thin_vec::ThinVec;

use crate::block::clip;
use crate::block::{
    Align, Area, Backdrop, Block, Boxed, Case, Cell, Content, Dir, Edge, Emphasis, Features,
    Finish, Floated, Frame, GridRow, Inline, Justify, LastLine, Length, Link, Look, Orient, Rise,
    Role, Script, Sheet, Spot, Table, Text, VAlign,
};
use crate::fonts::{BOOK_FACE, Faces, Family, Fonts, Ink, Kind, SANS, Tone};
use crate::hyphen::{self, SHY};
use crate::meter;
use crate::picture::{Decoded, Picture};

/// mareader's page: an A4 card at 96 dpi, in layout units.
pub const PAGE_WIDTH: f32 = 794.0;
pub const PAGE_HEIGHT: f32 = 1123.0;
/// The card's inner margin on every side.
pub const PAD: f32 = 72.0;

/// Book layout's inner margin, toward the spine, and its outer one.
const GUTTER: f32 = 92.0;
const EDGE: f32 = 56.0;

/// A pane-sized page's least height and most margin.
const MIN_SHEET: f32 = 160.0;
const MAX_MARGIN: f32 = 240.0;

/// A page's frame: its size, the pads around its column and where it sits.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Card {
    size: (f32, f32),
    /// Above and below the column.
    pads: (f32, f32),
    /// The column's left edge on even pages, then odd ones.
    lefts: (f32, f32),
    column: f32,
}

impl Card {
    /// The A4 card around a column `width` wide, or the pane-sized page.
    fn of(typo: &Typography, width: f32) -> Self {
        let sheet = typo.sheet.filter(|(w, h)| w.is_finite() && h.is_finite());
        let Some((w, h)) = sheet else {
            let (lefts, margins) = match typo.book_layout {
                true => ((EDGE, GUTTER), GUTTER + EDGE),
                false => ((PAD, PAD), 2.0 * PAD),
            };
            let size = (margins + width, PAGE_HEIGHT);
            let pads = (PAD, PAD);
            return Self {
                size,
                pads,
                lefts,
                column: width,
            };
        };
        let fit = |m: f32| {
            if m.is_finite() {
                m.clamp(0.0, MAX_MARGIN)
            } else {
                0.0
            }
        };
        let (my, mx) = (fit(typo.margins.0), fit(typo.margins.1));
        let size = (w.max(MIN_COLUMN + 2.0 * mx), h.max(MIN_SHEET + 2.0 * my));
        let line = match typo.max_line.is_finite() {
            true => typo.max_line.max(MIN_COLUMN),
            false => f32::INFINITY,
        };
        let column = (size.0 - 2.0 * mx).min(line).max(MIN_COLUMN);
        let slack = (size.0 - column) / 2.0;
        // Book layout leans each column toward the spine.
        let lean = match typo.book_layout {
            true => (slack - mx).clamp(0.0, mx) / 2.0,
            false => 0.0,
        };
        let lefts = (slack + lean, slack - lean);
        Self {
            size,
            pads: (my, my),
            lefts,
            column,
        }
    }

    /// The column's width and height.
    fn room(&self) -> (f32, f32) {
        let height = self.size.1 - self.pads.0 - self.pads.1;
        (self.column, height.max(1.0))
    }
}

/// More than this many pages means a broken or hostile book.
pub const MAX_PAGES: usize = 100_000;

/// When a book's own design shows (`docs/epub-layout.md`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Design {
    /// The book's type everywhere; its layout in the chapters it designs.
    #[default]
    Auto,
    On,
    Off,
}

/// How a book is set: the reader's typography plus the text's language.
#[derive(Debug, Clone, PartialEq)]
pub struct Typography {
    /// Body size in layout units (CSS px).
    pub font_size: f32,
    /// A multiple of the font size.
    pub line_height: f32,
    /// Space between blocks, in body ems.
    pub block_gap: f32,
    /// Body weight, 100 to 900; headings keep their own.
    pub weight: f32,
    /// Extra space between words, in layout units.
    pub word_spacing: f32,
    /// Extra space between letters, in ems.
    pub letter_spacing: f32,
    /// A paragraph's first-line indent, in ems.
    pub indent: f32,
    pub justify: bool,
    pub hyphenate: bool,
    /// Each column moves toward the spine: right on even pages.
    pub book_layout: bool,
    /// The column's width as a multiple of the page's own; the page follows.
    pub column_scale: f32,
    /// A BCP 47 tag; it picks the hyphenation patterns.
    pub language: String,
    pub faces: Faces,
    pub design: Design,
    /// A page the pane's size, in layout units; none is the A4 card.
    pub sheet: Option<(f32, f32)>,
    /// On a pane-sized page: margins above and below, and beside.
    pub margins: (f32, f32),
    /// On a pane-sized page: the longest a line runs.
    pub max_line: f32,
    /// The smallest a book's text sets, in layout units.
    pub min_size: f32,
    /// The reader's faces for all text, and body copy at the reader's size.
    pub override_font: bool,
    /// The reader's spacing, indent and justification over the book's.
    pub override_layout: bool,
}

impl Default for Typography {
    fn default() -> Self {
        Self {
            font_size: 17.0,
            line_height: 1.7,
            block_gap: 1.0,
            weight: 400.0,
            word_spacing: 0.0,
            letter_spacing: 0.0,
            indent: 0.0,
            justify: false,
            hyphenate: false,
            book_layout: false,
            column_scale: 1.0,
            language: "en".to_owned(),
            faces: Faces::default(),
            design: Design::Auto,
            sheet: None,
            margins: (44.0, 32.0),
            max_line: 720.0,
            min_size: 8.0,
            override_font: false,
            override_layout: false,
        }
    }
}

const HEADING_LINE_HEIGHT: f32 = 1.3;
/// Inter's heaviest bundled weight stands in for mareader's 700.
const HEADING_WEIGHT: f32 = 600.0;
const HEAD_CELL_WEIGHT: f32 = 600.0;

// Sizes in ems, after mareader's text stylesheet.
const PRE_SCALE: f32 = 0.85;
const PRE_LINE_HEIGHT: f32 = 1.55;
const PRE_PAD: (f32, f32) = (1.1, 0.9);
const CODE_SCALE: f32 = 0.88;
const CELL_SCALE: f32 = 0.95;
const CELL_PAD: (f32, f32) = (0.7, 0.35);
/// Between list items, instead of the paragraph gap.
const TIGHT_GAP: f32 = 0.3;
/// Text sizes a book may ask for, in layout units.
const MIN_SIZE: f32 = 2.0;
const MAX_SIZE: f32 = 400.0;
/// The tightest line height a book may ask for.
const MIN_LINE: f32 = 0.5;
/// Reader defaults (size, line, gap, weight); book type moves off them too.
const READER: (f32, f32, f32, f32) = (17.0, 1.7, 1.0, 400.0);
const RULE_HEIGHT: f32 = 0.5;
/// From a list marker's right edge to the text.
pub(crate) const MARKER_GAP: f32 = 0.5;
/// A capital's height in ems, near enough for most faces.
const CAP_HEIGHT: f32 = 0.7;
/// Lines a drop cap may span, and its gap to the text in ems.
const MAX_DROP: usize = 6;
const DROP_GAP: f32 = 0.2;
/// A drop cap's letter in the text: there, but too small to see.
const HIDDEN_SIZE: f32 = 0.01;
const FIRST_LINE_PASSES: usize = 3;
/// Px of a marker picture per em of its text.
const MARKER_PX: f32 = 16.0;
/// Holds a picture-only marker's line open; draws nothing.
const WORD_JOINER: &str = "\u{2060}";

/// A heading's size in body ems, as mareader's stylesheet has it.
fn heading_scale(level: u8) -> f32 {
    match level {
        1 => 1.7,
        2 => 1.45,
        3 => 1.22,
        4 => 1.08,
        5 => 1.0,
        _ => 0.92,
    }
}

/// CSS `bolder`: what bold text weighs inside text of `weight`.
fn bolder(weight: f32) -> f32 {
    if weight < 350.0 {
        400.0
    } else if weight < 550.0 {
        700.0
    } else {
        900.0
    }
}

type Stack = Vec<FontFamilyName<'static>>;

/// Each stack as parley takes it, with Inter last for symbols.
struct Stacks {
    body: Stack,
    heading: Stack,
    mono: Stack,
}

impl Stacks {
    fn new(faces: &Faces) -> Self {
        Self {
            body: stack(&faces.body),
            heading: stack(&faces.heading),
            mono: stack(&faces.mono),
        }
    }
}

fn stack(families: &[Family]) -> Stack {
    let mut names: Stack = families
        .iter()
        .map(|f| match f {
            Family::Named(name) => FontFamilyName::Named(Cow::Owned(name.clone())),
            Family::Generic(kind) => FontFamilyName::Generic(match kind {
                Kind::Serif => GenericFamily::Serif,
                Kind::Sans => GenericFamily::SansSerif,
                Kind::Mono => GenericFamily::Monospace,
            }),
        })
        .collect();
    let symbols = FontFamilyName::named(SANS);
    if !names.contains(&symbols) {
        names.push(symbols);
    }
    names
}

/// How one text layout is set.
#[derive(Clone, Copy)]
struct Style<'a> {
    faces: &'a [FontFamilyName<'static>],
    /// Inline code's stack.
    mono: &'a [FontFamilyName<'static>],
    size: f32,
    line_height: f32,
    weight: f32,
    tone: Tone,
    align: Align,
    /// `align` is a side, not a start or end.
    sided: bool,
    /// Preformatted: long lines wrap anywhere, code spans aren't resized.
    pre: bool,
    justify: bool,
    hyphens: Option<Lang>,
    /// The first line's indent, in layout units.
    indent: f32,
    /// Extra advance per letter and per word, in layout units.
    letter: f32,
    word: f32,
    /// Runs keep the sizes the book gives them.
    sized: bool,
    /// The smallest a sized run sets.
    floor: f32,
    /// How the last line sits; none follows the rest.
    last: Option<LastLine>,
    dir: Dir,
    /// Stacks for runs' own families, by [`Emphasis::face`].
    runs: &'a [Stack],
    /// Vertical: laid as lines that are drawn turned a quarter.
    turned: bool,
    orient: Orient,
}

/// A text layout, its soft-hyphen lines, and line starts in source bytes.
pub(crate) struct Para {
    pub(crate) layout: Layout<Ink>,
    /// The text as set, soft hyphens included.
    pub(crate) text: String,
    pub(crate) hyphens: Vec<usize>,
    /// Where soft hyphens were put in, ascending; see `Hyphenated`.
    pub(crate) marks: Vec<usize>,
    starts: Vec<usize>,
    /// Linked runs, in bytes of the text as set.
    links: Vec<Link>,
    /// Pictures in its lines, by their boxes' ids.
    pub(crate) pictures: Vec<Arc<Picture>>,
    /// Each picture's link, and whether it is hidden.
    pub(crate) picture_links: Vec<Option<u32>>,
    pub(crate) veiled: Vec<bool>,
    /// Ruby notes over its runs, bases in bytes of the text as set.
    pub(crate) ruby: Vec<Note>,
    /// Vertical: drawn turned, its pictures standing again.
    pub(crate) turned: bool,
    /// Vertical: runs standing upright, and runs set across, in bytes.
    pub(crate) upright: Vec<Range<usize>>,
    pub(crate) combine: Vec<Range<usize>>,
}

/// A ruby note: its base in the text as set, then the note laid small.
pub(crate) struct Note {
    pub(crate) range: Range<usize>,
    text: String,
    pub(crate) para: Option<Box<Para>>,
}

/// What a set block draws; positions are relative to the block.
pub(crate) enum Draw {
    Text {
        /// Inner padding left of the text, for shaded blocks.
        pad: f32,
        shaded: bool,
    },
    Rule,
    Image {
        picture: Arc<Picture>,
        rect: [f32; 4],
        /// Laid on its side in vertical text, drawn standing.
        turned: bool,
        links: ThinVec<Area>,
    },
    Table(Grid),
    /// Room only: a designed book's rule draws as its boxes.
    Space,
}

/// A book's box placed in the column, in layout units (`docs/epub-layout.md`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Placed {
    /// The same for every block in the box.
    key: usize,
    /// The border box across.
    x: f32,
    width: f32,
    border: [f32; 4],
    edges: [Edge; 4],
    colours: [Option<[u8; 3]>; 4],
    /// From its top border to its first block, its last block to the bottom.
    top: f32,
    bottom: f32,
    centre: Option<f32>,
    /// Its top border and padding, and the least height of its content.
    inside_top: f32,
    height: Option<f32>,
    fill: Option<f32>,
    backdrop: Option<Arc<Backdrop>>,
    /// The body em its backdrop's lengths are in.
    em: f32,
    /// Its corners' radius in layout units, before fitting the box.
    radius: f32,
    finish: Option<Arc<Finish>>,
    /// Kept on one page when it fits.
    whole: bool,
}

/// A float placed in the column: its key, the boxes holding it, its place.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FloatAt {
    key: usize,
    depth: usize,
    x: f32,
    width: f32,
}

/// A float beside a text: its far edge in the column, its reach down.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Exclusion {
    right: bool,
    edge: f32,
    height: f32,
}

/// Room a float takes from a text's lines, from its own edges.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Beside {
    left: f32,
    right: f32,
    height: f32,
}

/// A float's wrap going on: its edge, reach, and what may wrap.
#[derive(Debug, Clone)]
struct Wrap {
    exclusion: Exclusion,
    height: f32,
    within: Holders,
}

/// A grid item's place in its row's room, as sized: left, width, line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CellPlace {
    left: f32,
    width: f32,
    /// A wrapped flex row's line.
    line: u16,
}

/// A grid item's least and most content widths, or its set width.
#[derive(Debug, Clone, Copy)]
struct Extent {
    row: usize,
    item: u16,
    least: f32,
    most: f32,
    set: bool,
}

/// A block's place in a grid row, in layout units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CellAt {
    /// The same for every block in the row.
    key: usize,
    column: usize,
    /// How many of the block's boxes hold the grid.
    depth: usize,
    valign: VAlign,
    /// The space above the row.
    before: f32,
    /// Its grid, and the rows its cell spans down.
    grid: usize,
    down: u16,
}

/// A box's borders on one page; a page's cut leaves that side open.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Deco {
    /// Left, top, width and height in column units.
    pub(crate) rect: [f32; 4],
    pub(crate) border: [f32; 4],
    pub(crate) edges: [Edge; 4],
    pub(crate) colours: [Option<[u8; 3]>; 4],
    pub(crate) fill: Option<f32>,
    pub(crate) backdrop: Option<Arc<Backdrop>>,
    pub(crate) em: f32,
    pub(crate) radius: f32,
    pub(crate) finish: Option<Arc<Finish>>,
    /// Cut at the page's top and bottom: no frame picture there.
    pub(crate) continued: [bool; 2],
}

/// A table set in equal columns; row `i` spans `units[i]`.
pub(crate) struct Grid {
    pub(crate) column: f32,
    /// Cell padding: across, down.
    pub(crate) pad: [f32; 2],
    /// Which rows are head rows.
    pub(crate) heads: ThinVec<bool>,
    /// The book's colour for the grid, if it sets one.
    pub(crate) rule: Option<[u8; 3]>,
}

/// A drop cap: its letter laid large, sitting on this line's baseline.
pub(crate) struct Initial {
    pub(crate) para: Para,
    pub(crate) line: usize,
}

/// A block's text layouts: the bulk of a set book, so laid on demand.
pub(crate) enum Laid {
    Text {
        para: Box<Para>,
        /// A list marker, hung left of the first line.
        marker: Option<Box<Para>>,
        initial: Option<Box<Initial>>,
    },
    /// Each row's cells.
    Table(Vec<Vec<Para>>),
    None,
}

/// The blocks laid most recently, up to [`MAX_LAID`] estimated bytes.
#[derive(Default)]
struct Recent {
    /// Each block's layouts, when last used, and estimated bytes.
    laid: HashMap<usize, (Arc<Laid>, u64, usize)>,
    tick: u64,
    bytes: usize,
}

impl Recent {
    fn get(&mut self, block: usize) -> Option<Arc<Laid>> {
        self.tick += 1;
        let entry = self.laid.get_mut(&block)?;
        entry.1 = self.tick;
        Some(Arc::clone(&entry.0))
    }

    fn put(&mut self, block: usize, laid: Arc<Laid>, bytes: usize) {
        self.tick += 1;
        if let Some(old) = self.laid.insert(block, (laid, self.tick, bytes)) {
            self.bytes -= old.2;
            meter::take(&meter::LAID, old.2);
        }
        self.bytes += bytes;
        meter::add(&meter::LAID, bytes);
        // Over: drop the oldest down to half, so eviction is rare.
        if self.bytes > MAX_LAID {
            self.shrink(MAX_LAID / 2, Some(block));
        }
    }

    /// Drops the oldest layouts down to `to` bytes, stopping at `keep`.
    fn shrink(&mut self, to: usize, keep: Option<usize>) {
        let mut ages: Vec<(u64, usize)> = self.laid.iter().map(|(&b, e)| (e.1, b)).collect();
        ages.sort_unstable();
        for (_, old) in ages {
            if self.bytes <= to || Some(old) == keep {
                break;
            }
            if let Some(gone) = self.laid.remove(&old) {
                self.bytes -= gone.2;
                meter::take(&meter::LAID, gone.2);
            }
        }
    }
}

impl Drop for Recent {
    fn drop(&mut self) {
        meter::take(&meter::LAID, self.bytes);
    }
}

/// Estimated bytes of layouts a book keeps; the rest are laid again.
pub const MAX_LAID: usize = 32 << 20;
/// What a resting book keeps: its newest layouts, the pages in view.
const REST_LAID: usize = 4 << 20;

/// One block set: `x` and `width` within the column.
pub(crate) struct Set {
    /// The block's index in the book, blank ones counted.
    pub(crate) source: usize,
    pub(crate) x: f32,
    pub(crate) width: f32,
    /// Quote bars' left edges within the column.
    pub(crate) bars: ThinVec<f32>,
    /// Right to left: its marker hangs right of its text.
    pub(crate) rtl: bool,
    pub(crate) draw: Draw,
    /// Each unit's top and bottom: lines, rows or the whole image.
    pub(crate) units: Vec<(f32, f32)>,
    /// Each unit's place in the block: a text offset or a row.
    starts: Vec<usize>,
    keep_with_next: bool,
    break_before: bool,
    tight: bool,
    /// A look's space above, in layout units, instead of the gap.
    before: Option<f32>,
    /// Text: avoid lone lines at a split.
    lines: bool,
    /// Lines kept together at a split's foot and head.
    orphans: u8,
    widows: u8,
    /// Kept on one page when it fits.
    whole: bool,
    /// A table's leading head rows, never left alone at a page's foot.
    heads: u16,
    /// Designed: the book's boxes around it, outermost first.
    boxes: ThinVec<Placed>,
    /// Designed: its place in a grid row.
    cell: Option<Box<CellAt>>,
    /// Designed: the float it is in.
    float: Option<Box<FloatAt>>,
    /// Designed: a float beside its first lines.
    beside: Option<Exclusion>,
    /// It starts below the float before it.
    clear: bool,
    /// Designed, in a grid: its item's place, as sized.
    place: Option<Box<CellPlace>>,
    /// Designed: a turn in degrees, drawn about its middle.
    pub(crate) rotate: f32,
    /// Takes its room but draws nothing and leads nowhere.
    pub(crate) hidden: bool,
    /// Written vertically: laid on its side, its page drawn turned.
    pub(crate) turned: bool,
    /// On a fixed page: that page, and the positioned box it is in.
    pub(crate) sheet: Option<Arc<Sheet>>,
    spot: Option<Box<(usize, Spot)>>,
}

impl Set {
    /// Its place in a grid row, if designed in one.
    fn cell(&self) -> Option<CellAt> {
        self.cell.as_deref().copied()
    }

    /// The float it is in, if designed in one.
    fn float(&self) -> Option<FloatAt> {
        self.float.as_deref().copied()
    }
}

/// A paginated fixed page: its viewport, and its layers bottom up.
pub(crate) struct Fixed {
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) layers: Vec<(Range<usize>, Range<usize>)>,
}

/// Each unit's top and bottom within its block.
type Units = Vec<(f32, f32)>;

/// A run of one block's units on a page; `dy` maps block to page.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Piece {
    pub(crate) block: usize,
    pub(crate) units: Range<usize>,
    pub(crate) dy: f32,
}

/// A place in the book that survives re-setting it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Anchor {
    /// The block's index in the book.
    pub block: usize,
    /// A byte offset into its text, or a table row.
    pub offset: usize,
}

/// A link's box on one line of a page, in page units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageLink {
    pub target: u32,
    /// Left, top, width, height.
    pub rect: [f32; 4],
}

/// A page's part in a stream: its column, cut to text and gap above.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Band {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// A book paginated, keeping line boxes; `Send + Sync` for the workers.
pub struct Flow {
    pub(crate) blocks: Vec<Set>,
    pub(crate) pages: Vec<Vec<Piece>>,
    /// Each page's box borders, drawn under its pieces.
    pub(crate) decos: Vec<Vec<Deco>>,
    pub(crate) bands: Vec<Band>,
    /// Each page's fixed layout, if it is a fixed page.
    pub(crate) fixed: Vec<Option<Fixed>>,
    pub(crate) book_layout: bool,
    /// Pages before its first, which decide each page's spread side.
    shift: usize,
    /// The text column's width.
    width: f32,
    card: Card,
    pub(crate) decoded: Arc<Decoded>,
    /// Boxed: fonts and type are kilobytes, and books move.
    relay: Box<Relay>,
}

/// The blocks, type and fonts a book was set in, to lay it again.
struct Relay {
    source: Arc<Vec<Block>>,
    typo: Typography,
    /// The page's height as set, so blocks are laid again alike.
    height: f32,
    stacks: Stacks,
    fonts: Mutex<Fonts>,
    recent: Mutex<Recent>,
}

impl Drop for Relay {
    fn drop(&mut self) {
        meter::take(&meter::FLOWS, 1);
    }
}

impl std::fmt::Debug for Flow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Flow({} pages)", self.pages.len())
    }
}

impl Flow {
    /// Sets `blocks` on its page, A4 or the pane's; at least one page.
    pub fn new(blocks: Arc<Vec<Block>>, typo: &Typography, fonts: Fonts) -> Self {
        let room = Card::of(typo, column_width(typo)).room();
        Self::set(blocks, typo, fonts, room, room.1)
    }

    /// Sets `blocks` `width` wide, cut every `height`; tall never cuts.
    pub fn sized(
        blocks: Arc<Vec<Block>>,
        typo: &Typography,
        fonts: Fonts,
        (width, height): (f32, f32),
        tall: bool,
    ) -> Self {
        let typo = Typography {
            book_layout: false,
            sheet: None,
            ..typo.clone()
        };
        let room = (width.max(MIN_COLUMN), height.max(1.0));
        let cut = if tall { f32::INFINITY } else { room.1 };
        Self::set(blocks, &typo, fonts, room, cut)
    }

    fn set(
        blocks: Arc<Vec<Block>>,
        typo: &Typography,
        mut fonts: Fonts,
        (width, height): (f32, f32),
        cut: f32,
    ) -> Self {
        let stacks = Stacks::new(&typo.faces);
        // Laid to measure, then let go; endless streams never turn.
        let turnable = cut.is_finite();
        let set: Vec<Set> = {
            let mut setter = Setter::new(typo, &stacks, &mut fonts);
            let items: Vec<(usize, &Block)> = blocks
                .iter()
                .enumerate()
                .filter(|(_, b)| !b.is_blank())
                .collect();
            let mut out = Vec::with_capacity(items.len());
            // Vertical lines run down; fixed pages keep their viewport.
            let part = |b: &Block| (b.frame.turned, sheet_key(&b.frame));
            for group in items.chunk_by(|a, b| part(a.1) == part(b.1)) {
                let first = group.first().map(|(_, b)| &b.frame);
                let sheet = first.and_then(|f| f.sheet.as_deref());
                let turned = turnable && sheet.is_none() && first.is_some_and(|f| f.turned);
                (setter.turned, setter.fixed) = (turned, sheet.is_some());
                (setter.column, setter.height) = match sheet {
                    Some(s) => (s.width, s.height),
                    None if turned => (height, width),
                    None => (width, height),
                };
                out.extend(setter.set_all(group));
            }
            out
        };
        let (pages, decos, fixed) = paginate_parts(&set, (cut, width), typo);
        meter::add(&meter::FLOWS, 1);
        let mut flow = Self {
            blocks: set,
            pages,
            decos,
            bands: Vec::new(),
            fixed,
            book_layout: typo.book_layout,
            shift: 0,
            width,
            card: Card::of(typo, width),
            decoded: Arc::default(),
            relay: Box::new(Relay {
                source: blocks,
                typo: typo.clone(),
                height,
                stacks,
                fonts: Mutex::new(fonts),
                recent: Mutex::default(),
            }),
        };
        flow.bands = (0..flow.pages.len()).map(|n| flow.band(n, typo)).collect();
        flow
    }

    /// `blocks` in this type for `replaced` pages, ending a spread evenly.
    pub fn region(
        &self,
        blocks: Arc<Vec<Block>>,
        language: Option<&str>,
        fonts: Fonts,
        (first, replaced): (usize, usize),
    ) -> Self {
        let relay = &*self.relay;
        let mut typo = relay.typo.clone();
        if let Some(tag) = language {
            tag.clone_into(&mut typo.language);
        }
        let room = (self.width, relay.height);
        let mut flow = Self::set(blocks, &typo, fonts, room, room.1);
        flow.shift = first % 2;
        // An odd change in count would swap every later page's side.
        if flow.book_layout && (flow.pages.len() + replaced) % 2 == 1 {
            flow.pages.push(Vec::new());
            flow.decos.push(Vec::new());
            flow.fixed.push(None);
        }
        flow.bands = (0..flow.pages.len()).map(|n| flow.band(n, &typo)).collect();
        flow.keep_pictures(self)
    }

    /// Set block `block`'s layouts, laid again if not kept.
    pub(crate) fn laid(&self, block: usize) -> Arc<Laid> {
        let relay = &*self.relay;
        let recent = || relay.recent.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(laid) = recent().get(block) {
            return laid;
        }
        let set = self.blocks.get(block);
        let set = set.filter(|s| matches!(s.draw, Draw::Text { .. } | Draw::Table(_)));
        let Some((set, source)) = set.and_then(|s| Some((s, relay.source.get(s.source)?))) else {
            return Arc::new(Laid::None);
        };
        let laid = {
            let mut fonts = relay.fonts.lock().unwrap_or_else(PoisonError::into_inner);
            let mut setter = Setter::new(&relay.typo, &relay.stacks, &mut fonts);
            // The column it was measured in, not the typography's own.
            (setter.column, setter.height) = match set.sheet.as_deref() {
                Some(s) => (s.width, s.height),
                None if set.turned => (relay.height, self.width),
                None => (self.width, relay.height),
            };
            (setter.turned, setter.fixed) = (set.turned, set.sheet.is_some());
            // Laid again as first set: beside its float, or as one.
            setter.float_box = set.float().map(|f| (f.x, f.width));
            setter.exclusion = set.beside;
            setter.cell_place = set.place.as_deref().copied();
            Arc::new(setter.block(set.source, source).1)
        };
        recent().put(block, Arc::clone(&laid), source.layout_cost());
        laid
    }

    /// Page `page`'s column: its left edge and width.
    pub fn column(&self, page: usize) -> (f32, f32) {
        if self.fixed(page).is_some() {
            return (0.0, self.size(page).0);
        }
        // Spreads pair pages from the first: even ones sit left.
        let x = match (page + self.shift) % 2 {
            0 => self.card.lefts.0,
            _ => self.card.lefts.1,
        };
        (x, self.width)
    }

    /// Whether page `page` is written vertically, drawn turned a quarter.
    pub fn turned(&self, page: usize) -> bool {
        let first = self.pages.get(page).and_then(|p| p.first());
        first
            .and_then(|p| self.blocks.get(p.block))
            .is_some_and(|s| s.turned)
    }

    /// Whether page `page` is a fixed-layout page.
    pub fn is_fixed(&self, page: usize) -> bool {
        self.fixed(page).is_some()
    }

    /// Page `page`'s fixed layout, if it is a fixed page.
    pub(crate) fn fixed(&self, page: usize) -> Option<&Fixed> {
        self.fixed.get(page).and_then(Option::as_ref)
    }

    /// How much page `page` is scaled from the space it was laid in.
    pub(crate) fn fit(&self, page: usize) -> f32 {
        let Some(f) = self.fixed(page) else {
            return 1.0;
        };
        let (width, height) = self.page_size();
        (width / f.width.max(1.0)).min(height / f.height.max(1.0))
    }

    /// Page `page`'s size: a fixed page keeps its shape, fitted in.
    pub fn size(&self, page: usize) -> (f32, f32) {
        match self.fixed(page) {
            Some(f) => {
                let fit = self.fit(page);
                (f.width * fit, f.height * fit)
            }
            None => self.page_size(),
        }
    }

    /// Where page `page`'s pieces start, in the space they were laid in.
    pub(crate) fn origin(&self, page: usize) -> (f32, f32) {
        if self.fixed(page).is_some() {
            return (0.0, 0.0);
        }
        let (x, width) = self.column(page);
        match self.turned(page) {
            // On its side, the right margin is the top one.
            true => (self.card.pads.0, self.page_size().0 - x - width),
            false => (x, self.card.pads.0),
        }
    }

    /// A box in page `page`'s laid space, as drawn on the page.
    pub(crate) fn shown_rect(&self, page: usize, [x, y, w, h]: [f32; 4]) -> [f32; 4] {
        if self.fixed(page).is_some() {
            return [x, y, w, h].map(|v| v * self.fit(page));
        }
        match self.turned(page) {
            true => [self.page_size().0 - y - h, x, h, w],
            false => [x, y, w, h],
        }
    }

    /// Every page's band, for a stream that reads like one long page.
    pub fn bands(&self) -> &[Band] {
        &self.bands
    }

    fn band(&self, page: usize, typo: &Typography) -> Band {
        if self.fixed(page).is_some() {
            let (width, height) = self.size(page);
            return Band {
                x: 0.0,
                y: 0.0,
                width,
                height,
            };
        }
        if self.turned(page) {
            let (x, width) = self.column(page);
            let height = self.card.size.1;
            return Band {
                x,
                y: 0.0,
                width,
                height,
            };
        }
        let pieces = self.pages.get(page).map_or(&[][..], Vec::as_slice);
        let used = pieces
            .iter()
            .filter_map(|p| {
                let set = self.blocks.get(p.block)?;
                let last = set.units.get(p.units.end.checked_sub(1)?)?;
                Some(p.dy + last.1)
            })
            .fold(0.0, f32::max);
        // The gap a page cut swallowed; a chapter gets a pad's worth.
        let (top, bottom) = self.card.pads;
        let lead = match pieces.first() {
            _ if page == 0 => top,
            Some(p) if p.units.start == 0 => self
                .blocks
                .get(p.block)
                .map_or(0.0, |b| if b.break_before { top } else { gap(b, typo) }),
            _ => 0.0,
        }
        .min(top);
        let foot = if page + 1 == self.pages.len() {
            bottom
        } else {
            0.0
        };
        let (x, width) = self.column(page);
        let y = top - lead;
        Band {
            x,
            y,
            width,
            height: (lead + used + foot).min(self.card.size.1 - y),
        }
    }

    /// At rest: only the newest layouts and decodes stay, for pages in view.
    pub fn rest(&self) {
        let mut recent = self
            .relay
            .recent
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        recent.shrink(REST_LAID, None);
        drop(recent);
        self.decoded.rest();
    }

    /// Shares `other`'s decoded pictures: a re-set book shows the same.
    pub fn keep_pictures(mut self, other: &Flow) -> Self {
        self.decoded = Arc::clone(&other.decoded);
        self
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// How far down page `page`'s column its last line reaches.
    pub fn depth(&self, page: usize) -> f32 {
        if let Some(f) = self.fixed(page) {
            return f.height;
        }
        if self.turned(page) {
            return self.relay.height;
        }
        let pieces = self.pages.get(page).map_or(&[][..], Vec::as_slice);
        let bottom = |p: &Piece| {
            let set = self.blocks.get(p.block)?;
            let last = set.units.get(p.units.end.checked_sub(1)?)?;
            Some(p.dy + last.1)
        };
        pieces.iter().filter_map(bottom).fold(0.0, f32::max)
    }

    /// The blocks as given, blank ones counted.
    pub fn source(&self) -> &[Block] {
        &self.relay.source
    }

    /// Page `page`'s blocks and their bytes or rows; `usize::MAX` ends one.
    pub fn excerpts(&self, page: usize) -> Vec<(usize, Range<usize>)> {
        let pieces = self.pages.get(page).map_or(&[][..], Vec::as_slice);
        let excerpt = |p: &Piece| {
            let set = self.blocks.get(p.block)?;
            let from = set.starts.get(p.units.start).copied().unwrap_or(0);
            let to = set.starts.get(p.units.end).copied().unwrap_or(usize::MAX);
            Some((set.source, from..to))
        };
        pieces.iter().filter_map(excerpt).collect()
    }

    /// Every page is the same size: width, height in layout units.
    pub fn page_size(&self) -> (f32, f32) {
        self.card.size
    }

    /// Where page `page`'s text may run: left, top, width and height.
    pub fn text_area(&self, page: usize) -> [f32; 4] {
        let (x, width) = self.column(page);
        let (_, height) = self.card.room();
        [x, self.card.pads.0, width, height]
    }

    /// Where page `page` starts.
    pub fn anchor(&self, page: usize) -> Option<Anchor> {
        let piece = self.pages.get(page)?.first()?;
        let set = self.blocks.get(piece.block)?;
        Some(Anchor {
            block: set.source,
            offset: set.starts.get(piece.units.start).copied().unwrap_or(0),
        })
    }

    /// Page `page`'s links: a box for each line of each.
    pub fn links(&self, page: usize) -> Vec<PageLink> {
        let mut out = Vec::new();
        let (left, top) = self.origin(page);
        for piece in self.pages.get(page).map_or(&[][..], Vec::as_slice) {
            let Some(set) = self.blocks.get(piece.block).filter(|s| !s.hidden) else {
                continue;
            };
            let x = left + set.x;
            let y = top + piece.dy;
            match (&set.draw, &*self.laid(piece.block)) {
                (Draw::Text { pad, .. }, Laid::Text { para, .. }) => {
                    link_boxes(para, piece.units.clone(), (x + pad, y), &mut out);
                }
                (Draw::Table(grid), Laid::Table(rows)) => {
                    let [pad_x, pad_y] = grid.pad;
                    for r in piece.units.clone() {
                        let (Some(&(top, _)), Some(cells)) = (set.units.get(r), rows.get(r)) else {
                            continue;
                        };
                        for (c, cell) in cells.iter().enumerate() {
                            let at = (x + c as f32 * grid.column + pad_x, y + top + pad_y);
                            link_boxes(cell, 0..cell.layout.len(), at, &mut out);
                        }
                    }
                }
                (Draw::Image { rect, links, .. }, _) => {
                    let [ix, iy, w, h] = *rect;
                    out.extend(links.iter().map(|a| PageLink {
                        target: a.target,
                        rect: [
                            x + ix + a.rect[0] * w,
                            y + iy + a.rect[1] * h,
                            a.rect[2] * w,
                            a.rect[3] * h,
                        ],
                    }));
                }
                _ => {}
            }
        }
        for link in &mut out {
            link.rect = self.shown_rect(page, link.rect);
        }
        out
    }

    /// The page showing `anchor`, or the first after it.
    pub fn locate(&self, anchor: Anchor) -> usize {
        let at = (anchor.block, anchor.offset);
        let reaches_past = |page: &Vec<Piece>| {
            page.iter().any(|piece| {
                self.blocks.get(piece.block).is_some_and(|set| {
                    // Where the block goes on after this piece.
                    let next = set.starts.get(piece.units.end).copied();
                    (set.source, next.unwrap_or(usize::MAX)) > at
                })
            })
        };
        // Pages run in book order, so the first that does is found by halving.
        let n = self.pages.partition_point(|page| !reaches_past(page));
        n.min(self.pages.len().saturating_sub(1))
    }
}

/// One job's view of the book's typography and fonts.
struct Setter<'a> {
    typo: &'a Typography,
    stacks: &'a Stacks,
    lang: Lang,
    fonts: &'a mut Fonts,
    column: f32,
    height: f32,
    /// A float's place, given its blocks are set.
    float_box: Option<(f32, f32)>,
    /// A float beside the next block's first lines.
    exclusion: Option<Exclusion>,
    /// The next block's grid item place, overriding the sized ones.
    cell_place: Option<CellPlace>,
    /// The grid being set: its items' places, as sized.
    places: HashMap<(usize, u16), CellPlace>,
    /// Setting vertical blocks: the column runs down the page.
    turned: bool,
    /// Setting a fixed page: the book's own type in CSS px, as it is.
    fixed: bool,
    /// The card's measure, which sizes the design type at any page size.
    measure: f32,
}

mod setter;

/// A book's length in layout units; percentages are of `width`.
pub(crate) fn resolve(length: Length, width: f32, em: f32) -> f32 {
    match length {
        Length::Em(e) => e * em,
        Length::Percent(p) => width * p / 100.0,
        Length::Mix { em: e, percent: p } => e * em + width * p / 100.0,
    }
}

/// Box `b` in the room at `x`, `width` across: margins, width, centring.
fn place(b: &Arc<Boxed>, (x, width): (f32, f32), em: f32) -> Placed {
    let inside = (b.border[1] + b.border[3] + b.padding[1] + b.padding[3]) * em;
    let side = |m: Option<Length>| m.map(|l| resolve(l, width, em));
    let (left, right) = (side(b.margin[0]), side(b.margin[1]));
    let content = match b.width {
        // A set width wider than the room shrinks to it.
        Some(l) => resolve(l, width, em).min(width - inside),
        None => width - left.unwrap_or(0.0) - right.unwrap_or(0.0) - inside,
    };
    let outer = content.max(0.0) + inside;
    let left = match (left, right, b.width) {
        (l, _, None) => l.unwrap_or(0.0),
        (None, None, Some(_)) => ((width - outer) / 2.0).max(0.0),
        (None, Some(r), Some(_)) => (width - outer - r).max(0.0),
        (Some(l), _, Some(_)) => l,
    };
    Placed {
        key: Arc::as_ptr(b) as usize,
        x: x + left,
        width: outer,
        border: b.border.map(|w| w * em),
        edges: b.edges,
        colours: b.colours,
        top: b.top * em,
        bottom: b.bottom * em,
        centre: b.centre,
        inside_top: (b.border[0] + b.padding[0]) * em,
        height: b.height.map(|h| h * em),
        fill: b.fill,
        backdrop: b.backdrop.clone(),
        em,
        radius: b.radius.map_or(0.0, |r| resolve(r, outer, em).max(0.0)),
        finish: b.finish.clone(),
        whole: b.whole,
    }
}

/// The float block `frame` is in, by its address.
fn float_key(frame: &Frame) -> Option<usize> {
    frame.float.as_ref().map(|f| Arc::as_ptr(f) as usize)
}

/// What holds a float, by address: its outer boxes and its cell.
#[derive(Debug, Clone, Default, PartialEq)]
struct Holders {
    boxes: Vec<usize>,
    cell: Option<(usize, u16)>,
}

impl Holders {
    /// Block `frame`'s first `depth` boxes and its cell.
    fn of(frame: &Frame, depth: usize) -> Self {
        let boxes = frame.boxes.iter().take(depth);
        Self {
            boxes: boxes.map(|b| Arc::as_ptr(b) as usize).collect(),
            cell: cell_key(frame),
        }
    }

    /// Whether block `frame` sits in the same holders.
    fn hold(&self, frame: &Frame) -> bool {
        let mine = frame.boxes.iter().map(|b| Arc::as_ptr(b) as usize);
        let inside = frame.boxes.len() >= self.boxes.len();
        inside && self.boxes.iter().zip(mine).all(|(a, b)| *a == b) && self.cell == cell_key(frame)
    }
}

/// Block `frame`'s grid row, by address, and column.
fn cell_key(frame: &Frame) -> Option<(usize, u16)> {
    frame
        .cell
        .as_ref()
        .map(|c| (Arc::as_ptr(&c.row) as usize, c.column))
}

/// How wide a set block's content is, as a float shrinks to it.
fn natural(set: &Set, laid: &Laid) -> f32 {
    match (laid, &set.draw) {
        (Laid::Text { para, .. }, Draw::Text { pad, .. }) => para.layout.width() + 2.0 * pad,
        (_, Draw::Image { rect, .. }) => rect[2],
        _ => set.width,
    }
}

/// Narrows `x`, `width` to cell `c`'s column of its row.
fn enter(c: &Cell, (x, width): (&mut f32, &mut f32), em: f32, at: Option<CellPlace>) -> CellAt {
    let column = usize::from(c.column);
    let (left, span) = match at {
        Some(p) => (p.left, p.width),
        None => cell_span(&c.row, column, *width, em),
    };
    *x += left;
    *width = span.max(1.0);
    let line = at.map_or(0, |p| p.line);
    let gap = c.row.columns.flex.map_or(0.0, |f| f.gap.max(0.0));
    CellAt {
        // A wrapped line is its own row, keyed inside the row's allocation.
        key: Arc::as_ptr(&c.row) as usize + usize::from(line),
        column,
        depth: usize::from(c.depth),
        valign: c.row.valign.get(column).copied().unwrap_or_default(),
        before: if line == 0 { c.row.before } else { gap } * em,
        grid: Arc::as_ptr(&c.row.columns) as usize,
        // A wrapped flex line is a row of its own: nothing spans it.
        down: c.row.down.get(column).copied().unwrap_or(1).max(1),
    }
}

/// The grid block `frame` is in: its table's or flex row's columns.
fn grid_key(frame: &Frame) -> Option<usize> {
    frame
        .cell
        .as_ref()
        .map(|c| Arc::as_ptr(&c.row.columns) as usize)
}

/// Block `frame`'s grid item: its row, by address, and its index.
fn item_key(frame: &Frame) -> Option<(usize, u16)> {
    frame
        .cell
        .as_ref()
        .map(|c| (Arc::as_ptr(&c.row) as usize, c.column))
}

/// A table's columns from its items' extents, shared by its rows.
fn table_places<'a>(
    extents: &[Extent],
    rows: impl Iterator<Item = &'a Cell>,
    (room, fill, centre): (f32, bool, bool),
    places: &mut HashMap<(usize, u16), CellPlace>,
) {
    // Each item's columns: its first and how many.
    let mut spans: HashMap<(usize, u16), (usize, usize)> = HashMap::new();
    for c in rows {
        let item = usize::from(c.column);
        let start = c.row.starts.get(item).map_or(item, |s| usize::from(*s));
        let span = c.row.spans.get(item).map_or(1, |s| usize::from(*s)).max(1);
        let key = (Arc::as_ptr(&c.row) as usize, c.column);
        spans.insert(key, (start.min(MAX_COLUMNS - 1), span));
    }
    let n = spans
        .values()
        .map(|(s, w)| s + w)
        .max()
        .unwrap_or(1)
        .clamp(1, MAX_COLUMNS);
    let (mut least, mut most, mut set) = (vec![0.0f32; n], vec![0.0f32; n], vec![false; n]);
    let mut wide = Vec::new();
    for e in extents {
        let (start, span) = spans.get(&(e.row, e.item)).copied().unwrap_or((0, 1));
        if span > 1 {
            wide.push((start, span.min(n - start.min(n - 1)), e));
            continue;
        }
        if let (Some(l), Some(m), Some(s)) = (
            least.get_mut(start),
            most.get_mut(start),
            set.get_mut(start),
        ) {
            (*l, *m) = (l.max(e.least), m.max(e.most));
            *s |= e.set;
        }
    }
    // A spanning item's excess spreads over its columns evenly.
    for (start, span, e) in wide {
        let cols = start..(start + span).min(n);
        let count = cols.len().max(1) as f32;
        let have: (f32, f32) = cols.clone().fold((0.0, 0.0), |(l, m), c| {
            (
                l + least.get(c).unwrap_or(&0.0),
                m + most.get(c).unwrap_or(&0.0),
            )
        });
        for c in cols {
            if let (Some(l), Some(m)) = (least.get_mut(c), most.get_mut(c)) {
                *l += (e.least - have.0).max(0.0) / count;
                *m += (e.most - have.1).max(0.0) / count;
            }
        }
    }
    let widths = distribute(&least, &most, room, fill);
    let total: f32 = widths.iter().sum();
    let offset = if centre {
        ((room - total) / 2.0).max(0.0)
    } else {
        0.0
    };
    for (key, (start, span)) in spans {
        let left = offset + widths.iter().take(start).sum::<f32>();
        let width = widths.iter().skip(start).take(span).sum();
        places.insert(
            key,
            CellPlace {
                left,
                width,
                line: 0,
            },
        );
    }
}

/// Column widths in `room`: their most, else between least and most.
fn distribute(least: &[f32], most: &[f32], room: f32, fill: bool) -> Vec<f32> {
    let (low, high): (f32, f32) = (least.iter().sum(), most.iter().sum());
    let n = most.len().max(1) as f32;
    if high <= room {
        if fill && high > 0.0 {
            return most.iter().map(|m| m * room / high).collect();
        }
        if fill {
            return vec![room / n; most.len()];
        }
        return most.to_vec();
    }
    if low < room && high > low {
        let share = (room - low) / (high - low);
        return least
            .iter()
            .zip(most)
            .map(|(l, m)| l + (m - l) * share)
            .collect();
    }
    if low > 0.0 {
        return least.iter().map(|l| l * room / low).collect();
    }
    vec![room / n; most.len()]
}

/// A flex row's items placed: wrapped, grown or shrunk, then justified.
fn flex_places(
    items: &[&Extent],
    (room, gap): (f32, f32),
    (flex, grow): (crate::block::Flex, &[f32]),
    places: &mut HashMap<(usize, u16), CellPlace>,
) {
    let basis = |e: &Extent| e.most.clamp(e.least.min(room), room);
    let mut lines: Vec<Vec<&Extent>> = vec![Vec::new()];
    let mut used = 0.0;
    for e in items {
        let w = basis(e);
        let full = lines
            .last()
            .is_some_and(|l| !l.is_empty() && used + gap + w > room);
        if flex.wrap && full && lines.len() < MAX_FLEX_LINES {
            lines.push(Vec::new());
            used = 0.0;
        }
        used += if lines.last().is_some_and(Vec::is_empty) {
            w
        } else {
            gap + w
        };
        if let Some(line) = lines.last_mut() {
            line.push(e);
        }
    }
    for (n, line) in lines.iter().enumerate() {
        let k = line.len().max(1) as f32;
        let mut widths: Vec<f32> = line.iter().map(|e| basis(e)).collect();
        let gaps = gap * (k - 1.0);
        let mut free = room - widths.iter().sum::<f32>() - gaps;
        let grows: Vec<f32> = line
            .iter()
            .map(|e| {
                grow.get(usize::from(e.item))
                    .copied()
                    .unwrap_or(0.0)
                    .max(0.0)
            })
            .collect();
        let total_grow: f32 = grows.iter().sum();
        if free > 0.0 && total_grow > 0.0 {
            for (w, g) in widths.iter_mut().zip(&grows) {
                *w += free * g / total_grow;
            }
            free = 0.0;
        } else if free < 0.0 {
            let sum: f32 = widths.iter().sum::<f32>().max(1.0);
            let short = -free;
            for (w, e) in widths.iter_mut().zip(line) {
                *w = (*w - short * *w / sum).max(e.least.min(room / k));
            }
            free = 0.0;
        }
        let (start, extra) = match flex.justify {
            Justify::Start => (0.0, 0.0),
            Justify::Center => (free / 2.0, 0.0),
            Justify::End => (free, 0.0),
            Justify::Between if k > 1.0 => (0.0, free / (k - 1.0)),
            Justify::Between => (0.0, 0.0),
            Justify::Around => (free / (2.0 * k), free / k),
            Justify::Evenly => (free / (k + 1.0), free / (k + 1.0)),
        };
        let mut left = start.max(0.0);
        for (e, w) in line.iter().zip(widths) {
            let line = u16::try_from(n).unwrap_or(0);
            places.insert(
                (e.row, e.item),
                CellPlace {
                    left,
                    width: w,
                    line,
                },
            );
            left += w + gap + extra;
        }
    }
}

/// A column's left and width; set widths shrink, unset ones share the rest.
fn cell_span(row: &GridRow, column: usize, width: f32, em: f32) -> (f32, f32) {
    let n = row.widths.len().max(column + 1);
    let asked: Vec<Option<f32>> = (0..n)
        .map(|i| row.widths.get(i).copied().flatten())
        .map(|l| l.map(|l| resolve(l, width, em).max(0.0)))
        .collect();
    let fixed: f32 = asked.iter().flatten().sum();
    let free = asked.iter().filter(|a| a.is_none()).count();
    let scale = if fixed > width && fixed > 0.0 {
        width / fixed
    } else {
        1.0
    };
    let share = match free {
        0 => 0.0,
        free => (width - fixed * scale).max(0.0) / free as f32,
    };
    let widths = asked.iter().map(|a| a.map_or(share, |w| w * scale));
    let left = widths.clone().take(column).sum();
    (left, widths.clone().nth(column).unwrap_or(0.0))
}

/// Whether a block reads right to left: as set, or by its first strong letter.
fn right_to_left(block: &Block) -> bool {
    match (block.frame.dir, &block.content) {
        (Dir::Rtl, _) => true,
        (Dir::Auto, Content::Text(t)) => {
            let strong = t.text.chars().find(|&c| rtl_letter(c) || c.is_alphabetic());
            strong.is_some_and(rtl_letter)
        }
        _ => false,
    }
}

/// Hebrew, Arabic, Syriac, Thaana, N'Ko and their presentation forms.
fn rtl_letter(c: char) -> bool {
    matches!(
        u32::from(c),
        0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF | 0x10800..=0x10FFF | 0x1E800..=0x1EFFF
    )
}

/// A look's family names, then `rest`.
fn look_stack(names: &[&String], rest: &[FontFamilyName<'static>]) -> Stack {
    let named = names
        .iter()
        .map(|n| FontFamilyName::Named(Cow::Owned((*n).clone())));
    named.chain(rest.iter().cloned()).collect()
}

/// Vertical runs that stand upright: East Asian letters, unless told.
fn upright_runs(text: &str, orient: Orient) -> Vec<Range<usize>> {
    match orient {
        Orient::Sideways => return Vec::new(),
        Orient::Upright => return std::iter::once(0..text.len()).collect(),
        Orient::Mixed => {}
    }
    let mut out: Vec<Range<usize>> = Vec::new();
    for (at, c) in text.char_indices().filter(|&(_, c)| stands(c)) {
        let end = at + c.len_utf8();
        match out.last_mut() {
            Some(last) if last.end == at => last.end = end,
            _ => out.push(at..end),
        }
    }
    out
}

/// Letters set upright in vertical text, as Unicode's orientation data has.
fn stands(c: char) -> bool {
    matches!(
        u32::from(c),
        0x1100..=0x11FF
            | 0x2E80..=0x2FFF
            | 0x3000..=0x33FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xA000..=0xA4CF
            | 0xA960..=0xA97F
            | 0xAC00..=0xD7FF
            | 0xF900..=0xFAFF
            | 0xFE10..=0xFE1F
            | 0xFE30..=0xFE6F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE7
            | 0x1F000..=0x1FAFF
            | 0x20000..=0x3FFFF
    )
}

/// Vertical forms upright, half widths combined, beside the book's.
fn turned_features(
    b: &mut RangedBuilder<'_, Ink>,
    text: &Text,
    (upright, combine): (&[Range<usize>], &[Range<usize>]),
) {
    let featured: Vec<(&Range<usize>, Features)> = (text.spans.iter())
        .filter(|s| !s.emphasis.features.is_empty())
        .map(|s| (&s.range, s.emphasis.features))
        .collect();
    let mut cuts: Vec<usize> = (featured.iter().map(|f| f.0))
        .chain(upright)
        .chain(combine)
        .flat_map(|r| [r.start, r.end])
        .collect();
    cuts.sort_unstable();
    cuts.dedup();
    let inside = |ranges: &[Range<usize>], at: usize| ranges.iter().any(|r| r.contains(&at));
    for pair in cuts.windows(2) {
        let [from, to] = *pair else {
            continue;
        };
        let mut parts = Vec::new();
        let own = featured.iter().rev().find(|f| f.0.contains(&from));
        if let Some((_, features)) = own {
            parts.push(features.source());
        }
        if inside(combine, from) {
            parts.push("\"hwid\" 1".to_owned());
        } else if inside(upright, from) {
            // Full widths give standing Latin letters an em each.
            parts.push("\"vert\" 1, \"fwid\" 1".to_owned());
        }
        if !parts.is_empty() {
            let source = FontFeatures::Source(Cow::Owned(parts.join(", ")));
            b.push(StyleProperty::FontFeatures(source), from..to);
        }
    }
}

fn alignment(style: &Style<'_>) -> Alignment {
    if style.justify {
        return Alignment::Justify;
    }
    match (style.align, style.sided) {
        (Align::Start, false) => Alignment::Start,
        (Align::Start, true) => Alignment::Left,
        (Align::Center, _) => Alignment::Center,
        (Align::End, false) => Alignment::End,
        (Align::End, true) => Alignment::Right,
    }
}

mod place;
use place::{layout_text, place_row};

/// A list marker's picture at `at`, at most a line of text tall.
fn marker_inline(picture: &Arc<Picture>, at: usize) -> Inline {
    let ems = |px: u32| px.max(1) as f32 / MARKER_PX;
    let (width, height) = (ems(picture.width), ems(picture.height));
    let scale = (1.0 / height).min(1.0);
    Inline {
        at,
        picture: Arc::clone(picture),
        width: width * scale,
        height: height * scale,
        rise: Rise::Baseline,
        link: None,
        hidden: false,
    }
}

/// `text` with `extra` added over its first `end` bytes.
fn over_first_line(text: &Text, end: usize, extra: Emphasis) -> Text {
    let len = text.text.len();
    let mut cuts: Vec<usize> = (text.spans.iter())
        .flat_map(|s| [s.range.start, s.range.end])
        .chain([0, end, len])
        .filter(|&c| c <= len && text.text.is_char_boundary(c))
        .collect();
    cuts.sort_unstable();
    cuts.dedup();
    let mut out = Text {
        text: String::with_capacity(len),
        spans: ThinVec::new(),
        ..text.clone()
    };
    for pair in cuts.windows(2) {
        let (Some(&a), Some(&b)) = (pair.first(), pair.get(1)) else {
            continue;
        };
        let own = (text.spans.iter())
            .find(|s| s.range.start <= a && a < s.range.end)
            .map(|s| s.emphasis)
            .unwrap_or_default();
        let e = match b <= end {
            true => Emphasis {
                italic: own.italic || extra.italic,
                bold: own.bold || extra.bold,
                light: own.light && !extra.bold,
                size: if own.size == 0 { extra.size } else { own.size },
                case: if extra.case == Case::Normal {
                    own.case
                } else {
                    extra.case
                },
                underline: own.underline || extra.underline,
                strike: own.strike || extra.strike,
                colour: own.colour.or(extra.colour),
                shadow: own.shadow.or(extra.shadow),
                ..own
            },
            false => own,
        };
        out.push(text.text.get(a..b).unwrap_or_default(), e);
    }
    out
}

/// Breaks `layout`'s lines in `width`, those beside a float narrower.
fn break_beside(layout: &mut Layout<Ink>, width: f32, beside: Beside) {
    let mut lines = layout.break_lines();
    lines.state_mut().set_layout_max_advance(width);
    loop {
        let top = lines.state().line_y() as f32;
        let (x, room) = match top < beside.height {
            true => (beside.left, width - beside.left - beside.right),
            false => (0.0, width),
        };
        lines.state_mut().set_line_x(x);
        lines
            .state_mut()
            .set_line_max_advance(room.clamp(1.0, width));
        if lines.break_next().is_none() {
            break;
        }
    }
    lines.finish();
}

/// `text` in its shown cases, offsets kept; small capitals' runs.
fn cased(text: &Text) -> (Cow<'_, str>, Vec<(Range<usize>, f32)>) {
    let mut small: Vec<(Range<usize>, f32)> = Vec::new();
    if text.spans.iter().all(|s| s.emphasis.case == Case::Normal) {
        return (Cow::Borrowed(&text.text), small);
    }
    // A letter swapped only for one as long, keeping every offset.
    let swap = |c: char, upper: bool| {
        let mut to: Vec<char> = match upper {
            true => c.to_uppercase().collect(),
            false => c.to_lowercase().collect(),
        };
        let one = to.len() == 1;
        to.pop().filter(|t| one && t.len_utf8() == c.len_utf8())
    };
    let mut shown = String::with_capacity(text.text.len());
    let mut spans = text.spans.iter().peekable();
    for (i, c) in text.text.char_indices() {
        while spans.next_if(|s| s.range.end <= i).is_some() {}
        let span = spans.peek().filter(|s| s.range.start <= i);
        let case = span.map_or(Case::Normal, |s| s.emphasis.case);
        let out = match case {
            Case::Normal => None,
            Case::Upper => swap(c, true),
            Case::Lower => swap(c, false),
            Case::Small => swap(c, true).filter(|_| c.is_lowercase()),
            Case::AllSmall if c.is_uppercase() => Some(c),
            Case::AllSmall => swap(c, true).filter(|_| c.is_lowercase()),
        };
        shown.push(out.unwrap_or(c));
        if matches!(case, Case::Small | Case::AllSmall) && out.is_some() {
            let size = span.map_or(0, |s| s.emphasis.size);
            let scale = if size == 0 {
                1.0
            } else {
                f32::from(size) / 100.0
            };
            let end = i + c.len_utf8();
            match small.last_mut() {
                Some((r, s)) if r.end == i && *s == scale => r.end = end,
                _ => small.push((i..end, scale)),
            }
        }
    }
    (Cow::Owned(shown), small)
}

/// Boxes of `para`'s links on lines `lines`, its origin at `x, y`.
fn link_boxes(para: &Para, lines: Range<usize>, (x, y): (f32, f32), out: &mut Vec<PageLink>) {
    let layout = &para.layout;
    // Pictures in a line take no text, so their boxes are links of their own.
    if para.picture_links.iter().any(Option::is_some) {
        for line in lines.clone().filter_map(|l| layout.get(l)) {
            for item in line.items() {
                let PositionedLayoutItem::InlineBox(b) = item else {
                    continue;
                };
                let i = usize::try_from(b.id).ok();
                let link = i.and_then(|i| para.picture_links.get(i).copied().flatten());
                if let Some(target) = link {
                    let rect = [x + b.x, y + b.y, b.width, b.height];
                    out.push(PageLink { target, rect });
                }
            }
        }
    }
    for link in &para.links {
        let caret = |i, affinity| Caret::from_byte_index(layout, i, affinity);
        let start = caret(link.range.start, Affinity::Downstream);
        let end = caret(link.range.end, Affinity::Upstream);
        Selection::new(start, end).geometry_with(layout, |b, line| {
            if lines.contains(&line) {
                let (w, h) = ((b.x1 - b.x0) as f32, (b.y1 - b.y0) as f32);
                out.push(PageLink {
                    target: link.target,
                    rect: [x + b.x0 as f32, y + b.y0 as f32, w, h],
                });
            }
        });
    }
}

fn line_units(layout: &Layout<Ink>) -> Vec<(f32, f32)> {
    layout
        .lines()
        .map(|l| (l.metrics().block_min_coord, l.metrics().block_max_coord))
        .collect()
}

/// The aspect of a picture laid on its side: width over height.
fn sideways(aspect: f32) -> f32 {
    if aspect > 0.0 && aspect.is_finite() {
        1.0 / aspect
    } else {
        1.0
    }
}

/// `wanted` wide where the column allows, shrunk to fit the page.
fn image_rect(
    aspect: f32,
    align: Align,
    (wanted, width): (f32, f32),
    height: f32,
) -> ([f32; 4], (f32, f32)) {
    let mut w = wanted.max(1.0).min(width);
    let mut h = w * aspect;
    if h > height {
        h = height;
        w = h / aspect;
    }
    let x = match align {
        Align::Start => 0.0,
        Align::Center => (width - w) / 2.0,
        Align::End => width - w,
    };
    ([x, 0.0, w, h], (0.0, h))
}

/// A cover fills the page box, centred, scaled up if need be.
fn cover_rect(aspect: f32, width: f32, height: f32) -> ([f32; 4], (f32, f32)) {
    let w = width.min(height / aspect);
    let h = w * aspect;
    let rect = [(width - w) / 2.0, (height - h) / 2.0, w, h];
    (rect, (0.0, height))
}

/// The page being filled: its pieces, borders and how far down it is.
#[derive(Default)]
struct Cursor {
    pages: Vec<Vec<Piece>>,
    page: Vec<Piece>,
    decos: Vec<Vec<Deco>>,
    deco: Vec<Deco>,
    /// The book's boxes open here, outermost first.
    open: Vec<OpenBox>,
    y: f32,
    /// The page's height, which centring boxes fill.
    height: f32,
    /// Where the last float on the page ends.
    float_bottom: f32,
    /// Cells spanning rows down, waiting for those rows.
    hangs: Vec<Hang>,
}

/// A cell spanning rows down: placed with its first, aligned past its last.
struct Hang {
    /// Its grid, and its rows still to come.
    grid: usize,
    rows: u16,
    /// Where its content ends, and how it sits in the rows it spans.
    bottom: f32,
    valign: VAlign,
    /// Its pieces on the page, and its borders.
    pieces: Vec<usize>,
    decos: Range<usize>,
}

/// A book's box open on the page being filled.
struct OpenBox {
    placed: Placed,
    top: f32,
    /// Begun on an earlier page: no top border here.
    continued: bool,
    /// Centring, begun on a fresh page: the pieces and borders before it.
    centre: Option<(usize, usize)>,
}

impl OpenBox {
    /// Its borders down to `bottom`; open there unless `closed`.
    fn deco(&self, bottom: f32, closed: bool) -> Option<Deco> {
        let (mut border, mut edges) = (self.placed.border, self.placed.edges);
        if self.continued {
            (border[0], edges[0]) = (0.0, Edge::None);
        }
        if !closed {
            (border[2], edges[2]) = (0.0, Edge::None);
        }
        let edged = border
            .iter()
            .zip(&edges)
            .any(|(w, e)| *w > 0.0 && e.draws());
        let p = &self.placed;
        let finished = p.finish.as_ref().is_some_and(|f| f.shows());
        let drawn = edged || p.fill.is_some() || p.backdrop.is_some() || finished;
        let rect = [p.x, self.top, p.width, bottom - self.top];
        drawn.then(|| Deco {
            rect: rect.map(|v| if v.is_finite() { v } else { 0.0 }),
            border,
            edges,
            colours: p.colours,
            fill: p.fill,
            backdrop: p.backdrop.clone(),
            em: p.em,
            radius: p.radius,
            finish: p.finish.clone(),
            continued: [self.continued, !closed],
        })
    }
}

/// The text column's width; A4's own at a scale of 1.
fn column_width(typo: &Typography) -> f32 {
    let page = if typo.book_layout {
        PAGE_WIDTH - GUTTER - EDGE
    } else {
        PAGE_WIDTH - 2.0 * PAD
    };
    let (lo, hi) = COLUMN_SCALE;
    let scale = if typo.column_scale.is_finite() {
        typo.column_scale.clamp(lo, hi)
    } else {
        1.0
    };
    (page * scale).max(MIN_COLUMN)
}

/// The column scale's range, as mareader's Column Width dial.
pub const COLUMN_SCALE: (f32, f32) = (0.6, 1.4);
/// The narrowest column, as mareader's.
const MIN_COLUMN: f32 = 160.0;
/// The most cells a grid row lays side by side.
const MAX_COLUMNS: usize = 64;
/// A ruby note's size per base size, and the line room it adds, in ems.
const RUBY_SCALE: f32 = 0.5;
const RUBY_ROOM: f32 = 0.6;
/// A quote bar's width and air, mirrored right to left.
const BAR_ROOM: f32 = 3.0;
/// The most lines `widows` and `orphans` keep together.
const MAX_KEPT_LINES: u8 = 8;
/// Pictures set in one block's lines, at most.
const MAX_INLINES: usize = 256;
/// Grid blocks measured for content widths; the rest share their sizes.
const MAX_MEASURED: usize = 512;
/// A grid item's most width is measured in this many rooms.
const MEASURE_ROOM: f32 = 8.0;
/// Lines a wrapping flex row makes, at most.
const MAX_FLEX_LINES: usize = 32;
/// A designed page is laid as if this many book ems wide, as a phone's.
const DESIGN_EMS: f32 = 26.0;
/// A fixed page's body em in its CSS px, and CSS's normal line height.
const FIXED_EM: f32 = 16.0;
const FIXED_LINE: f32 = 1.2;
/// The most designed type grows past the reader's size.
const MAX_DESIGN_SCALE: f32 = 1.6;
/// Space between a float and the text past it, in ems.
const FLOAT_GAP: f32 = 0.4;
/// Small capitals' size against capitals.
const SMALL_CAPS: f32 = 0.8;
/// Raised and lowered text's size in reading type.
const SCRIPT_SCALE: f32 = 0.75;
/// How much lighter normal words in bold text are: bold to regular.
const LIGHTER: f32 = 300.0;

fn gap(block: &Set, typo: &Typography) -> f32 {
    if let Some(before) = block.before {
        return before.max(0.0);
    }
    let ems = if block.tight {
        TIGHT_GAP
    } else {
        typo.block_gap
    };
    ems * typo.font_size
}

/// Pages and their borders, with each page's fixed layout if it has one.
type Paged = (Vec<Vec<Piece>>, Vec<Vec<Deco>>, Vec<Option<Fixed>>);

/// Pages for runs of vertical, level and fixed blocks, each cut its way.
fn paginate_parts(blocks: &[Set], (cut, width): (f32, f32), typo: &Typography) -> Paged {
    let none = |n: usize| (0..n).map(|_| None).collect::<Vec<_>>();
    if !blocks.iter().any(|b| b.turned || b.sheet.is_some()) {
        let (pages, decos) = paginate(blocks, cut, typo);
        let fixed = none(pages.len());
        return (pages, decos, fixed);
    }
    let (mut pages, mut decos, mut fixed) = (Vec::new(), Vec::new(), Vec::new());
    let mut start = 0;
    let part = |b: &Set| (b.turned, b.sheet.as_ref().map(|s| Arc::as_ptr(s) as usize));
    for group in blocks.chunk_by(|a, b| part(a) == part(b)) {
        let first = group.first();
        if let Some(sheet) = first.and_then(|b| b.sheet.as_deref()) {
            let (mut own, more, layers) = paginate_sheet(group, sheet, typo);
            for piece in &mut own {
                piece.block += start;
            }
            pages.push(own);
            decos.push(more);
            fixed.push(Some(Fixed {
                width: sheet.width,
                height: sheet.height,
                layers,
            }));
            start += group.len();
            continue;
        }
        let turned = first.is_some_and(|b| b.turned);
        let (mut own, more) = paginate(group, if turned { width } else { cut }, typo);
        for piece in own.iter_mut().flatten() {
            piece.block += start;
        }
        fixed.extend(none(own.len()));
        pages.extend(own);
        decos.extend(more);
        start += group.len();
    }
    (pages, decos, fixed)
}

/// Layers bottom up, each some borders then some pieces, as drawn.
type Layers = Vec<(Range<usize>, Range<usize>)>;

/// A fixed page: the flow, then positioned boxes in `z-index` order.
fn paginate_sheet(
    blocks: &[Set],
    sheet: &Sheet,
    typo: &Typography,
) -> (Vec<Piece>, Vec<Deco>, Layers) {
    // A run of blocks in one box: its paint order, pieces and borders.
    type Run = ((i32, bool, usize), Vec<Piece>, Vec<Deco>);
    let mut runs: Vec<Run> = Vec::new();
    let (mut flow_y, mut start) = (0.0, 0);
    // Where each positioned box goes on, past a box inside it.
    let mut tops: HashMap<usize, f32> = HashMap::new();
    for (order, group) in blocks.chunk_by(|a, b| spot_of(a) == spot_of(b)).enumerate() {
        let (pages, decos) = paginate(group, f32::INFINITY, typo);
        let mut pieces: Vec<Piece> = pages.into_iter().flatten().collect();
        let mut decos: Vec<Deco> = decos.into_iter().flatten().collect();
        let bottom = |p: &Piece| {
            let set = group.get(p.block)?;
            Some(p.dy + set.units.get(p.units.end.checked_sub(1)?)?.1)
        };
        let lines = pieces.iter().filter_map(bottom).fold(0.0, f32::max);
        let boxes = decos.iter().map(|d| d.rect[1] + d.rect[3]);
        let height = boxes.fold(lines, f32::max);
        let spot = group.first().and_then(|b| b.spot.as_deref().copied());
        let (top, key) = match spot {
            // Goes on below its lines: a box's least height would skip.
            None => {
                let top = flow_y;
                flow_y += lines;
                (top, (0, false, order))
            }
            Some((id, s)) => {
                let top = match (tops.get(&id), s.bottom) {
                    (Some(y), _) => *y,
                    (None, Some(line)) => line - height,
                    (None, None) => s.y,
                };
                tops.insert(id, top + height);
                (top, (s.z, true, order))
            }
        };
        for p in &mut pieces {
            p.block += start;
            p.dy += top;
        }
        for d in &mut decos {
            d.rect[1] += top;
        }
        runs.push((key, pieces, decos));
        start += group.len();
    }
    runs.sort_by_key(|r| r.0);
    let (mut pieces, mut decos, mut layers) = (Vec::new(), Vec::new(), Vec::new());
    if let Some(ground) = &sheet.ground {
        let open = OpenBox {
            placed: place(ground, (0.0, sheet.width), FIXED_EM),
            top: 0.0,
            continued: false,
            centre: None,
        };
        decos.extend(open.deco(sheet.height, true));
        layers.push((0..decos.len(), 0..0));
    }
    for (_, own, more) in runs {
        let (d, p) = (decos.len(), pieces.len());
        decos.extend(more);
        pieces.extend(own);
        layers.push((d..decos.len(), p..pieces.len()));
    }
    (pieces, decos, layers)
}

/// The positioned box a fixed page's block is in, by address.
fn spot_of(set: &Set) -> Option<usize> {
    set.spot.as_deref().map(|&(id, _)| id)
}

/// The fixed page a block is on, by address.
fn sheet_key(frame: &Frame) -> Option<usize> {
    frame.sheet.as_ref().map(|s| Arc::as_ptr(s) as usize)
}

/// Fills pages unit by unit; headings never end one, no lone lines.
fn paginate(blocks: &[Set], height: f32, typo: &Typography) -> (Vec<Vec<Piece>>, Vec<Vec<Deco>>) {
    let mut c = Cursor {
        height,
        ..Cursor::default()
    };
    let row = |i: usize| blocks.get(i).and_then(|b| b.cell()).map(|c| c.key);
    let float = |i: usize| blocks.get(i).and_then(|b| b.float()).map(|f| f.key);
    let mut i = 0;
    while i < blocks.len() && c.pages.len() < MAX_PAGES {
        if let (None, Some(key)) = (row(i), float(i)) {
            let end = (i..blocks.len())
                .find(|&j| float(j) != Some(key))
                .unwrap_or(blocks.len())
                .max(i + 1);
            place_float(&mut c, blocks, i..end, (height, typo));
            i = end;
            continue;
        }
        let Some(key) = row(i) else {
            place_block(&mut c, blocks, i, (height, typo));
            i += 1;
            continue;
        };
        let end = (i..blocks.len())
            .find(|&j| row(j) != Some(key))
            .unwrap_or(blocks.len());
        // A row taller than a page is read down the page instead.
        if !place_row(&mut c, blocks, i..end, (height, typo)) {
            for k in i..end {
                place_block(&mut c, blocks, k, (height, typo));
            }
        }
        i = end;
    }
    c.settle();
    c.close(0);
    c.break_page();
    if c.pages.is_empty() {
        c.pages.push(Vec::new());
        c.decos.push(Vec::new());
    }
    (c.pages, c.decos)
}

/// Places a float; what follows starts at its top, unless it ran over.
fn place_float(c: &mut Cursor, blocks: &[Set], float: Range<usize>, room: (f32, &Typography)) {
    c.settle();
    let (pages, top) = (c.pages.len(), c.y);
    for k in float.clone() {
        place_block(c, blocks, k, room);
    }
    let depth = blocks
        .get(float.start)
        .and_then(|b| b.float())
        .map_or(0, |f| f.depth);
    c.close(depth.min(c.open.len()));
    c.float_bottom = c.y;
    if c.pages.len() == pages {
        c.y = top;
    } else if c.page.first().is_some_and(|p| p.block == float.start) {
        // The whole float went overleaf: text starts beside it there.
        c.y = 0.0;
    }
}

/// Places block `i`, split between pages where it must be.
fn place_block(c: &mut Cursor, blocks: &[Set], i: usize, (height, typo): (f32, &Typography)) {
    let Some(block) = blocks.get(i) else {
        return;
    };
    let total = block.units.len();
    if total == 0 {
        return;
    }
    c.settle();
    c.close(c.common(&block.boxes));
    if block.break_before {
        c.break_page();
    }
    if block.clear {
        c.y = c.y.max(c.float_bottom);
    }
    if !c.page.is_empty() {
        c.y += gap(block, typo);
    } else if let (true, Some(before)) = (block.break_before, block.before) {
        // A designed chapter's opening keeps its space at the top.
        c.y += before.clamp(0.0, height / 2.0);
    }
    if block.keep_with_next && !c.page.is_empty() {
        // Room for the heading and the first two units after it.
        let next = blocks.get(i + 1).map_or(0.0, |n| {
            gap(n, typo) + span_height(n, 0..n.units.len().min(2))
        });
        if c.y + span_height(block, 0..total) + next > height {
            c.break_page();
        }
    }
    if !c.page.is_empty() {
        // An unbroken box or block that fits a page starts one.
        let tall = whole_height(c, blocks, i, typo).filter(|&h| h <= height);
        if tall.is_some_and(|h| c.y + h > height) {
            c.break_page();
        }
    }
    let mut start = 0;
    while start < total {
        let fit = fitting_units(block, start, height - c.y);
        let mut fit = if block.lines {
            let kept = (usize::from(block.orphans), usize::from(block.widows));
            balance(fit, (start, total), c.page.is_empty(), kept)
        } else if start + fit <= usize::from(block.heads) && fit < total && !c.page.is_empty() {
            // Head rows go on with the first body row.
            0
        } else {
            fit
        };
        if fit == 0 && !c.page.is_empty() {
            c.break_page();
            continue;
        }
        // A unit taller than a whole page overflows it.
        fit = fit.max(1);
        if start == 0 {
            c.open(&block.boxes);
        }
        c.place(block, i, start..start + fit);
        start += fit;
        if start < total {
            c.break_page();
        }
    }
}

/// The borders of a cell's own boxes, its blocks at their tops.
fn cell_decos<'a>(stack: impl Iterator<Item = (&'a Set, f32)>, depth: usize, out: &mut Vec<Deco>) {
    let close = |open: &mut Vec<OpenBox>, keep: usize, bottom: f32, out: &mut Vec<Deco>| {
        while open.len() > keep {
            if let Some(o) = open.pop() {
                out.extend(o.deco(bottom + o.placed.bottom, true));
            }
        }
    };
    let mut open: Vec<OpenBox> = Vec::new();
    let mut bottom = 0.0;
    for (b, y) in stack {
        let inner = b.boxes.get(depth..).unwrap_or_default();
        let same = |(o, p): &(&OpenBox, &Placed)| o.placed.key == p.key;
        let keep = open.iter().zip(inner).take_while(same).count();
        close(&mut open, keep, bottom, out);
        for p in inner.iter().skip(keep) {
            open.push(OpenBox {
                placed: p.clone(),
                top: y - p.top,
                continued: false,
                centre: None,
            });
        }
        bottom = y + span_height(b, 0..b.units.len());
    }
    close(&mut open, 0, bottom, out);
}

pub(crate) fn span_height(block: &Set, units: Range<usize>) -> f32 {
    let top = block.units.get(units.start).map_or(0.0, |u| u.0);
    let bottom = units
        .end
        .checked_sub(1)
        .and_then(|u| block.units.get(u))
        .map_or(top, |u| u.1);
    bottom - top
}

/// How many units from `start` fit in `room`.
fn fitting_units(block: &Set, start: usize, room: f32) -> usize {
    let Some(&(top, _)) = block.units.get(start) else {
        return 0;
    };
    block
        .units
        .iter()
        .skip(start)
        .take_while(|(_, bottom)| bottom - top <= room + 0.01)
        .count()
}

/// Trims a fit to keep lines together at a split; zero moves the block on.
fn balance(
    fit: usize,
    (start, total): (usize, usize),
    page_empty: bool,
    (orphans, widows): (usize, usize),
) -> usize {
    let rest = total - start;
    if fit >= rest {
        return rest;
    }
    // Too few lines atop the next page: take more along.
    let trimmed = if rest - fit < widows {
        rest.saturating_sub(widows)
    } else {
        fit
    };
    if trimmed >= orphans.max(1) {
        return trimmed;
    }
    // Too few at a page's foot, or no good split: move on, unless atop one.
    if page_empty { fit } else { 0 }
}

/// Lines a split keeps together; the book's count when designed.
fn lines_kept(count: u8, designed: bool) -> u8 {
    match count {
        0 => 2,
        _ if !designed => 2,
        n => n.min(MAX_KEPT_LINES),
    }
}

/// The height block `i` needs on one page: its own, or its unbroken box's.
fn whole_height(c: &Cursor, blocks: &[Set], i: usize, typo: &Typography) -> Option<f32> {
    let block = blocks.get(i)?;
    let depth = c.open.len();
    let boxed = block.boxes.get(depth..).unwrap_or_default();
    let Some((at, outer)) = boxed.iter().enumerate().find(|(_, b)| b.whole) else {
        return block
            .whole
            .then(|| span_height(block, 0..block.units.len()));
    };
    let level = depth + at;
    let inside = |b: &Set| b.boxes.get(level).is_some_and(|p| p.key == outer.key);
    let mut total = outer.top + outer.bottom;
    let mut seen: Vec<usize> = Vec::new();
    for (k, b) in blocks
        .iter()
        .enumerate()
        .skip(i)
        .take_while(|(_, b)| inside(b))
    {
        if k > i {
            total += gap(b, typo);
        }
        total += span_height(b, 0..b.units.len());
        for nested in b.boxes.iter().skip(level + 1) {
            if !seen.contains(&nested.key) {
                seen.push(nested.key);
                total += nested.top + nested.bottom;
            }
        }
    }
    Some(total)
}

/// A line picture's place as parley aligns boxes; `size` is the text's.
fn rise(rise: Rise, size: f32) -> VerticalAlign {
    match rise {
        Rise::Baseline => VerticalAlign::BASELINE,
        Rise::Middle => VerticalAlign::MIDDLE,
        Rise::TextTop => VerticalAlign::TEXT_TOP,
        Rise::TextBottom => VerticalAlign::TEXT_BOTTOM,
        Rise::Top => VerticalAlign::TOP,
        Rise::Bottom => VerticalAlign::BOTTOM,
        Rise::Sub => VerticalAlign::SUB,
        Rise::Super => VerticalAlign::SUPER,
        Rise::By(em) if em.is_finite() => {
            VerticalAlign::length((em * size).clamp(-size * 8.0, size * 8.0))
        }
        Rise::By(_) => VerticalAlign::BASELINE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    mod boxes;
    mod floats;
    mod layout;
    mod pages;
    mod text;

    use crate::block::{Columns, Flex, Inline, Row};
    use crate::picture::tests::png;

    fn para(words: usize) -> Block {
        Block::text(Role::Paragraph, &vec!["word"; words].join(" "))
    }

    fn heading(text: &str) -> Block {
        Block::text(Role::Heading(1), text)
    }

    fn picture(w: u32, h: u32) -> Arc<Picture> {
        let p = Picture::probe(png(w, h)).map(Arc::new);
        assert!(p.is_ok(), "probe");
        p.unwrap_or_else(|_| Arc::new(Picture::empty()))
    }

    fn image(w: u32, h: u32, cover: bool) -> Block {
        let picture = picture(w, h);
        Block::new(Content::Image {
            picture,
            cover,
            width: None,
            links: Vec::new(),
        })
    }

    fn flow(blocks: &[Block]) -> Flow {
        let blocks = Arc::new(blocks.to_vec());
        Flow::new(blocks, &Typography::default(), Fonts::bundled())
    }

    fn room() -> f32 {
        PAGE_HEIGHT - 2.0 * PAD
    }

    /// `block` in a designed chapter, in `boxes`, at `cell` if any.
    fn designed(mut block: Block, boxes: &[Arc<Boxed>], cell: Option<Cell>) -> Block {
        let look = Look {
            designed: true,
            ..Look::default()
        };
        block.frame.look = Some(Arc::new(look));
        block.frame.boxes = boxes.iter().cloned().collect();
        block.frame.cell = cell;
        block
    }

    /// The body em designed text in `f` is set at.
    fn design_em(f: &Flow) -> f32 {
        let em = Typography::default().font_size;
        (f.width / DESIGN_EMS).clamp(em, em * MAX_DESIGN_SCALE)
    }

    /// `block` floated at the left, or right if `right`.
    fn floated(block: Block, right: bool) -> Block {
        let mut block = designed(block, &[], None);
        block.frame.float = Some(Arc::new(Floated { right, depth: 0 }));
        block
    }

    /// Where block `block`'s first glyph run starts in its first line.
    fn first_glyph(f: &Flow, block: usize) -> f32 {
        let Laid::Text { para, .. } = &*f.laid(block) else {
            return -1.0;
        };
        let line = para.layout.lines().next();
        let runs = line.into_iter().flat_map(|l| l.items());
        let mut offsets = runs.filter_map(|item| match item {
            PositionedLayoutItem::GlyphRun(run) => Some(run.offset()),
            PositionedLayoutItem::InlineBox(_) => None,
        });
        offsets.next().unwrap_or(-1.0)
    }

    /// Each line's left edge in block `block`'s text.
    fn line_lefts(f: &Flow, block: usize) -> Vec<f32> {
        match &*f.laid(block) {
            Laid::Text { para, .. } => {
                let lines = para.layout.lines();
                lines.map(|l| l.metrics().inline_min_coord).collect()
            }
            _ => Vec::new(),
        }
    }

    /// Where block `block`'s first piece starts and ends on page `page`.
    fn span(f: &Flow, page: usize, block: usize) -> (f32, f32) {
        let piece = f
            .pages
            .get(page)
            .and_then(|p| p.iter().find(|p| p.block == block));
        let set = f.blocks.get(block);
        let (Some(piece), Some(set)) = (piece, set) else {
            return (f32::NAN, f32::NAN);
        };
        let top = set.units.get(piece.units.start).map_or(0.0, |u| u.0) + piece.dy;
        (top, top + span_height(set, piece.units.clone()))
    }

    #[test]
    fn a_pane_sized_page_centres_a_column_no_longer_than_its_line() {
        let typo = |book_layout| Typography {
            sheet: Some((900.0, 700.0)),
            margins: (40.0, 30.0),
            max_line: 500.0,
            book_layout,
            ..Typography::default()
        };
        let blocks = Arc::new((0..40).map(|_| para(12)).collect::<Vec<_>>());
        let f = Flow::new(Arc::clone(&blocks), &typo(false), Fonts::bundled());
        assert_eq!(f.page_size(), (900.0, 700.0));
        assert_eq!(f.text_area(0), [200.0, 40.0, 500.0, 620.0]);
        assert_eq!(f.column(1), (200.0, 500.0));
        assert!(f.page_count() > 1);
        // Each page's last line stays inside its bottom margin.
        assert!((0..f.page_count()).all(|p| f.depth(p) <= 620.5));
        // Book layout leans each column toward the spine.
        let book = Flow::new(blocks, &typo(true), Fonts::bundled());
        assert_eq!((book.column(0).0, book.column(1).0), (215.0, 185.0));
        // A narrow pane gives the column all but its margins.
        let narrow = Typography {
            sheet: Some((400.0, 700.0)),
            ..typo(false)
        };
        let f = Flow::new(Arc::new(vec![para(3)]), &narrow, Fonts::bundled());
        assert_eq!(f.column(0), (30.0, 340.0));
    }

    #[test]
    fn styled_prose_keeps_its_boxes_at_the_readers_size() {
        let framed = Arc::new(Boxed {
            border: [0.1; 4],
            edges: [Edge::Solid; 4],
            padding: [0.0, 1.0, 0.0, 1.0],
            ..Boxed::default()
        });
        // Margins show too: the book's CSS lays out prose as well.
        let moved = Arc::new(Boxed {
            margin: [Some(Length::Em(6.0)), Some(Length::Em(6.0))],
            ..Boxed::default()
        });
        let prose = |boxes: &[Arc<Boxed>]| {
            let mut b = para(3);
            b.frame.look = Some(Arc::new(Look::default()));
            b.frame.boxes = boxes.iter().cloned().collect();
            b
        };
        let f = flow(&[prose(&[Arc::clone(&moved), framed]), prose(&[moved])]);
        let em = Typography::default().font_size;
        let set = |i: usize| f.blocks.get(i).map(|s| (s.x, s.width, s.boxes.len()));
        let inset = 7.1 * em;
        let framed_at = set(0).unwrap_or_default();
        assert!((framed_at.0 - inset).abs() < 0.5, "{framed_at:?}");
        let inner = f.width - 12.0 * em - 2.2 * em;
        assert!((framed_at.1 - inner).abs() < 0.5, "{framed_at:?}");
        assert_eq!(framed_at.2, 2);
        let moved_at = set(1).unwrap_or_default();
        assert!((moved_at.0 - 6.0 * em).abs() < 0.5, "{moved_at:?}");
        assert!((moved_at.1 - (f.width - 12.0 * em)).abs() < 0.5);
        let decos = f.decos.first().map_or(0, Vec::len);
        assert_eq!(decos, 1);
        // Read as the reader's own, nothing of the book's shows.
        let off = Flow::new(
            Arc::new(vec![prose(&[Arc::new(Boxed {
                border: [0.1; 4],
                edges: [Edge::Solid; 4],
                ..Boxed::default()
            })])]),
            &Typography {
                design: Design::Off,
                ..Typography::default()
            },
            Fonts::bundled(),
        );
        assert_eq!(off.decos.first().map_or(0, Vec::len), 0);
    }

    /// A grid row sharing `columns`, with cells' widths, places and growth.
    fn row_of(
        columns: &Arc<Columns>,
        widths: Vec<Option<Length>>,
        starts: Vec<u16>,
    ) -> Arc<GridRow> {
        let n = widths.len();
        let spans = (0..n)
            .map(|i| {
                starts
                    .get(i + 1)
                    .map_or(1, |s| s - starts.get(i).copied().unwrap_or(0))
            })
            .collect();
        Arc::new(GridRow {
            widths,
            valign: vec![VAlign::Top; n],
            starts,
            spans,
            columns: Arc::clone(columns),
            ..GridRow::default()
        })
    }

    fn at(row: &Arc<GridRow>, column: u16) -> Option<Cell> {
        let row = Arc::clone(row);
        Some(Cell {
            row,
            column,
            depth: 0,
        })
    }

    fn words(text: &str, cell: Option<Cell>) -> Block {
        designed(Block::text(Role::Paragraph, text), &[], cell)
    }

    /// Block `i`'s left edge and width.
    fn across(f: &Flow, i: usize) -> (f32, f32) {
        f.blocks
            .get(i)
            .map_or((f32::NAN, f32::NAN), |s| (s.x, s.width))
    }

    #[test]
    fn overrides_put_the_readers_type_and_spacing_over_the_books() {
        // Small print, tight lines, an indent and no gap: the book's own.
        let styled = || {
            let mut b = para(1);
            b.frame.look = Some(Arc::new(Look {
                size: 0.5,
                line_height: Some(1.0),
                indent: 2.0,
                justify: true,
                ..Look::default()
            }));
            b
        };
        let typo = |override_font, override_layout| Typography {
            block_gap: 2.0,
            override_font,
            override_layout,
            ..Typography::default()
        };
        let em = Typography::default().font_size;
        let set = |t: &Typography| {
            let f = Flow::new(Arc::new(vec![styled(), styled()]), t, Fonts::bundled());
            let second = f.blocks.get(1);
            let tall = second.map_or(0.0, |s| span_height(s, 0..s.units.len()));
            let left = first_glyph(&f, 1);
            (tall, second.and_then(|s| s.before), left)
        };
        // As the book sets it; the reader's wider gap adds to its none.
        let (tall, before, left) = set(&typo(false, false));
        assert!((tall - 0.5 * em).abs() < 1.0, "{tall}");
        assert_eq!((before, left), (Some(em), 2.0 * em));
        // The reader's font: body copy at the reader's size.
        let (tall, _, _) = set(&typo(true, false));
        assert!((tall - em).abs() < 1.0, "{tall}");
        // The reader's layout: its lines, gap and indent outright.
        let (tall, before, left) = set(&typo(true, true));
        assert!((tall - em * 1.7).abs() < 1.0, "{tall}");
        assert_eq!((before, left), (Some(2.0 * em), 0.0));
        // Small print stops at the minimum size.
        let floor = Typography {
            min_size: 12.0,
            ..typo(false, false)
        };
        let (tall, _, _) = set(&floor);
        assert!((tall - 12.0).abs() < 1.0, "{tall}");
    }

    #[test]
    fn long_paragraphs_split_between_lines() {
        let f = flow(&[para(4000)]);
        assert!(f.page_count() > 3, "{}", f.page_count());
        // Every line placed once, in order.
        let lines: Vec<usize> = f
            .pages
            .iter()
            .flatten()
            .flat_map(|p| p.units.clone())
            .collect();
        let total = f.blocks.first().map_or(0, |b| b.units.len());
        assert_eq!(lines, (0..total).collect::<Vec<_>>());
    }

    #[test]
    fn a_heading_never_ends_a_page() {
        let mut blocks = Vec::new();
        for i in 0..40 {
            blocks.push(heading(&format!("Chapter {i}")));
            blocks.push(para(90 + i * 13));
        }
        let f = flow(&blocks);
        for page in &f.pages {
            let last = page.last().and_then(|p| f.blocks.get(p.block));
            assert!(last.is_some_and(|s| !s.keep_with_next));
        }
    }

    /// Each block's top on page one.
    fn tops(f: &Flow) -> Vec<f32> {
        let page = f.pages.first().cloned().unwrap_or_default();
        page.iter()
            .map(|p| {
                let top = f.blocks.get(p.block).and_then(|s| s.units.first());
                top.map_or(0.0, |u| u.0) + p.dy
            })
            .collect()
    }

    #[test]
    fn images_shrink_to_the_column_and_the_page_never_split() {
        let f = flow(&[
            image(100, 50, false),
            image(4000, 1000, false),
            image(500, 9000, false),
        ]);
        let rect = |i: usize| match f.blocks.get(i).map(|s| &s.draw) {
            Some(Draw::Image { rect, .. }) => *rect,
            _ => [0.0; 4],
        };
        // Small images keep their size; wide ones fit the column.
        assert_eq!(rect(0), [0.0, 0.0, 100.0, 50.0]);
        let column = PAGE_WIDTH - 2.0 * PAD;
        assert!((rect(1)[2] - column).abs() < 0.01, "{:?}", rect(1));
        // Tall ones fit the page, alone on it.
        assert!((rect(2)[3] - room()).abs() < 0.01, "{:?}", rect(2));
        assert_eq!(f.page_count(), 2);
    }

    #[test]
    fn tables_split_between_rows() {
        let cell = |s: &str| {
            let mut t = Text::new(Role::Paragraph);
            t.push(s, Emphasis::default());
            t
        };
        let rows = (0..120)
            .map(|i| Row {
                cells: vec![cell(&format!("row {i}")), cell("a longer second cell")],
                head: i == 0,
            })
            .collect();
        let f = flow(&[Block::new(Content::Table(Table { rows, rule: None }))]);
        assert!(f.page_count() >= 2, "{}", f.page_count());
        let rows: Vec<usize> = f
            .pages
            .iter()
            .flatten()
            .flat_map(|p| p.units.clone())
            .collect();
        assert_eq!(rows, (0..120).collect::<Vec<_>>());
    }

    fn with(blocks: &[Block], typo: Typography) -> Flow {
        Flow::new(Arc::new(blocks.to_vec()), &typo, Fonts::bundled())
    }

    fn book() -> Vec<Block> {
        let mut blocks: Vec<Block> = (0..60).map(|i| para(40 + i * 9)).collect();
        blocks.insert(20, heading("Middle"));
        blocks
    }

    /// `see` of the first text block's layout.
    fn first_para<R>(f: &Flow, see: impl FnOnce(&Para) -> R) -> Option<R> {
        let n = f
            .blocks
            .iter()
            .position(|s| matches!(s.draw, Draw::Text { .. }))?;
        match &*f.laid(n) {
            Laid::Text { para, .. } => Some(see(para)),
            _ => None,
        }
    }

    /// Each line's right edge: the end of its last glyph run.
    fn right_edges(p: &Para) -> Vec<f32> {
        p.layout
            .lines()
            .map(|l| {
                l.items()
                    .filter_map(|i| match i {
                        parley::PositionedLayoutItem::GlyphRun(g) => Some(g.offset() + g.advance()),
                        parley::PositionedLayoutItem::InlineBox(_) => None,
                    })
                    .fold(0.0, f32::max)
            })
            .collect()
    }

    /// The leftmost column with ink in it, in pixels.
    fn left_ink(px: &crate::Pixels) -> Option<u32> {
        let width = px.width.max(1) as usize;
        px.rgba
            .iter()
            .skip(3)
            .step_by(4)
            .enumerate()
            .filter(|(_, a)| **a > 128)
            .map(|(i, _)| (i % width) as u32)
            .min()
    }

    #[test]
    fn the_column_scale_widens_the_column_and_page_within_its_range() {
        let blocks = book();
        let scaled = |column_scale| {
            let typo = Typography {
                column_scale,
                ..Typography::default()
            };
            with(&blocks, typo)
        };
        let (plain, wide, narrow) = (scaled(1.0), scaled(1.4), scaled(0.6));
        let width = PAGE_WIDTH - 2.0 * PAD;
        assert_eq!(plain.page_size(), (PAGE_WIDTH, PAGE_HEIGHT));
        assert_eq!(wide.column(0), (PAD, width * 1.4));
        assert_eq!(wide.page_size().0, 2.0 * PAD + width * 1.4);
        assert!(wide.page_count() < plain.page_count());
        assert!(narrow.page_count() > plain.page_count());
        // Past the range, or not a number, it clamps or falls back.
        assert_eq!(scaled(9.0).column(0), wide.column(0));
        assert_eq!(scaled(f32::NAN).column(0), plain.column(0));
    }

    fn plain_count(blocks: &Arc<Vec<Block>>) -> usize {
        let typo = Typography::default();
        Flow::new(Arc::clone(blocks), &typo, Fonts::bundled()).page_count()
    }

    #[test]
    fn book_layout_moves_each_column_toward_the_spine() {
        let blocks = book();
        let plain = with(&blocks, Typography::default());
        let typo = Typography {
            book_layout: true,
            ..Typography::default()
        };
        let bound = with(&blocks, typo);
        assert_eq!(plain.column(0), (PAD, PAGE_WIDTH - 2.0 * PAD));
        assert_eq!(plain.column(1), plain.column(0));
        let width = PAGE_WIDTH - GUTTER - EDGE;
        // Spreads pair pages from the first: the left one's spine is right.
        assert_eq!(bound.column(0), (EDGE, width));
        assert_eq!(bound.column(1), (GUTTER, width));
        let region = [0, 0, PAGE_WIDTH as u32, PAGE_HEIGHT as u32];
        let ink = |f: &Flow, page| f.render(page, 1.0, region).as_ref().and_then(left_ink);
        let (even, odd) = (ink(&bound, 2), ink(&bound, 3));
        assert!(
            even.is_some_and(|x| x.abs_diff(EDGE as u32) < 4),
            "{even:?}"
        );
        assert!(
            odd.is_some_and(|x| x.abs_diff(GUTTER as u32) < 4),
            "{odd:?}"
        );
        assert!(ink(&plain, 3).is_some_and(|x| x.abs_diff(PAD as u32) < 4));
    }

    #[test]
    fn bands_hold_each_page_s_text_and_the_gap_its_cut_swallowed() {
        let f = with(&book(), Typography::default());
        let bands = f.bands();
        assert_eq!(bands.len(), f.page_count());
        let first = bands.first().copied();
        assert!(first.is_some_and(|b| b.y == 0.0), "the first keeps its pad");
        let last = bands.last().copied();
        let foot = last.map(|b| b.y + b.height);
        assert!(foot.is_some_and(|y| y <= PAGE_HEIGHT), "{last:?}");
        for (n, band) in bands.iter().enumerate() {
            assert_eq!((band.x, band.width), f.column(n));
            assert!(band.height > 0.0 && band.y + band.height <= PAGE_HEIGHT);
            let Some(piece) = f.pages.get(n).and_then(|p| p.first()) else {
                continue;
            };
            // Mid-paragraph cuts butt; cuts between blocks keep the gap.
            let lead = PAD - band.y;
            match (n, piece.units.start) {
                (0, _) => assert_eq!(lead, PAD),
                (_, 0) => assert!((lead - 17.0).abs() < 0.01, "{n}: {lead}"),
                _ => assert_eq!(lead, 0.0, "{n}"),
            }
        }
    }

    #[test]
    fn bigger_type_takes_more_pages_and_anchors_keep_the_place() {
        let blocks = book();
        let small = with(&blocks, Typography::default());
        let big = with(
            &blocks,
            Typography {
                font_size: 24.0,
                ..Typography::default()
            },
        );
        assert!(big.page_count() > small.page_count());
        for page in [1, 3, small.page_count() / 2, small.page_count() - 1] {
            let Some(anchor) = small.anchor(page) else {
                continue;
            };
            let found = big.locate(anchor);
            // The page found starts at or before the place...
            assert!(big.anchor(found).is_some_and(|a| a <= anchor), "{page}");
            // ...and the next one after it.
            let next = big.anchor(found + 1);
            assert!(next.is_none_or(|a| a > anchor), "{page}: {next:?}");
        }
        assert_eq!(
            small.locate(Anchor {
                block: 9999,
                offset: 0
            }),
            small.page_count() - 1
        );
    }

    #[test]
    fn justified_lines_reach_the_column_edge_but_the_last() {
        let column = PAGE_WIDTH - 2.0 * PAD;
        let blocks = [para(120)];
        let ragged = with(&blocks, Typography::default());
        let justified = with(
            &blocks,
            Typography {
                justify: true,
                ..Typography::default()
            },
        );
        let edges = first_para(&justified, right_edges).unwrap_or_default();
        let (last, full) = edges.split_last().unwrap_or((&0.0, &[]));
        assert!(full.len() > 2, "{edges:?}");
        // A trailing space may hang past the edge; never short of it.
        let reach = |e: &f32| *e > column - 0.5 && *e < column + 17.0 * 0.5;
        assert!(full.iter().all(reach), "{edges:?}");
        assert!(*last < column - 1.0, "{edges:?}");
        let ragged = first_para(&ragged, right_edges).unwrap_or_default();
        assert!(ragged.iter().any(|e| *e < column - 1.0), "{ragged:?}");
    }

    #[test]
    fn hyphenation_breaks_long_words_and_marks_those_lines() {
        let long = "Incomprehensibilities characteristically internationalisation ";
        let blocks = [Block::text(Role::Paragraph, &long.repeat(12))];
        let marks = |hyphenate| {
            let f = with(
                &blocks,
                Typography {
                    hyphenate,
                    ..Typography::default()
                },
            );
            first_para(&f, |p| p.hyphens.len()).unwrap_or(0)
        };
        assert_eq!(marks(false), 0);
        assert!(marks(true) > 0);
        // Anchors stay in the unhyphenated text's bytes.
        let f = with(
            &blocks,
            Typography {
                hyphenate: true,
                ..Typography::default()
            },
        );
        let starts = f
            .blocks
            .first()
            .map(|s| s.starts.clone())
            .unwrap_or_default();
        let text = long.repeat(12);
        assert!(
            starts
                .iter()
                .all(|&s| text.is_char_boundary(s) && s < text.len())
        );
    }

    #[test]
    fn spacing_indent_and_weight_take_effect() {
        let blocks = [para(200)];
        let lines = |typo: Typography| {
            let f = with(&blocks, typo);
            first_para(&f, |p| p.layout.len()).unwrap_or(0)
        };
        let base = lines(Typography::default());
        let letters = lines(Typography {
            letter_spacing: 0.15,
            ..Typography::default()
        });
        let words = lines(Typography {
            word_spacing: 8.0,
            ..Typography::default()
        });
        assert!(letters > base && words > base, "{base} {letters} {words}");
        let f = with(
            &blocks,
            Typography {
                indent: 2.0,
                ..Typography::default()
            },
        );
        let first = first_para(&f, |p| {
            p.layout.get(0)?.items().find_map(|i| match i {
                parley::PositionedLayoutItem::GlyphRun(g) => Some(g.offset()),
                parley::PositionedLayoutItem::InlineBox(_) => None,
            })
        })
        .flatten();
        assert!(first.is_some_and(|x| (x - 34.0).abs() < 0.5), "{first:?}");
        assert_eq!(bolder(400.0), 700.0);
        assert_eq!(bolder(600.0), 900.0);
        assert_eq!(bolder(200.0), 400.0);
    }

    fn linked(before: &str, link: &str, target: u32) -> Block {
        let mut t = Text::new(Role::Paragraph);
        t.push(before, Emphasis::default());
        t.push_link(link, Emphasis::default(), target);
        t.push(" after.", Emphasis::default());
        Block::new(Content::Text(t))
    }

    #[test]
    fn links_box_their_words_on_every_line_they_cross() {
        let words = vec!["linked"; 60].join(" ");
        let mut second = linked("Next ", "chapter", 9);
        second.break_before = true;
        let f = flow(&[linked("Before the link ", &words, 7), second]);
        let links = f.links(0);
        assert!(links.len() > 2, "{links:?}");
        assert!(links.iter().all(|l| l.target == 7));
        let right = PAGE_WIDTH - PAD + 1.0;
        for l in &links {
            let [x, y, w, h] = l.rect;
            assert!(x >= PAD - 1.0 && x + w <= right && w > 0.0, "{l:?}");
            assert!(y >= PAD - 1.0 && h > 0.0, "{l:?}");
        }
        // The first box starts after the unlinked words, on line one.
        let first = links.first().map(|l| l.rect);
        assert!(first.is_some_and(|[x, ..]| x > PAD + 40.0), "{first:?}");
        let tops: Vec<f32> = links.iter().map(|l| l.rect[1]).collect();
        assert!(tops.windows(2).all(|w| w.first() < w.get(1)), "{tops:?}");
        let next = f.links(1);
        assert_eq!(next.iter().map(|l| l.target).collect::<Vec<_>>(), [9]);
    }

    fn kept(f: &Flow) -> Vec<usize> {
        let recent = f
            .relay
            .recent
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut kept: Vec<usize> = recent.laid.keys().copied().collect();
        kept.sort_unstable();
        kept
    }

    #[test]
    fn layouts_are_laid_for_the_pages_drawn_and_no_others() {
        let f = flow(&book());
        assert!(kept(&f).is_empty(), "nothing is kept at open");
        let region = [0, 0, PAGE_WIDTH as u32, PAGE_HEIGHT as u32];
        assert!(f.render(2, 1.0, region).is_some());
        let on_page: Vec<usize> = f
            .pages
            .get(2)
            .into_iter()
            .flatten()
            .map(|p| p.block)
            .collect();
        assert!(!on_page.is_empty());
        assert_eq!(kept(&f), on_page);
    }

    #[test]
    fn a_page_laid_again_draws_and_reads_the_same() {
        let f = flow(&book());
        let region = [0, 0, PAGE_WIDTH as u32, PAGE_HEIGHT as u32];
        let see = |f: &Flow| (f.render(1, 1.0, region), f.text(1), f.links(1));
        let first = see(&f);
        *f.relay
            .recent
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Recent::default();
        assert_eq!(see(&f), first);
        assert!(first.1.len() > 10);
    }

    #[test]
    fn kept_layouts_stay_under_the_cap_and_the_used_stay() {
        let mut recent = Recent::default();
        let laid = || Arc::new(Laid::None);
        let quarter = MAX_LAID / 4;
        for block in 0..4 {
            recent.put(block, laid(), quarter);
        }
        assert!(recent.get(0).is_some());
        // Past the cap, the oldest go until half is left: 1 and 2.
        recent.put(4, laid(), quarter);
        assert!(recent.bytes <= MAX_LAID / 2);
        let mut kept: Vec<usize> = recent.laid.keys().copied().collect();
        kept.sort_unstable();
        assert_eq!(kept, [0, 4]);
        // One block past the cap alone is still kept while used.
        recent.put(5, laid(), 2 * MAX_LAID);
        assert!(recent.get(5).is_some());
    }

    #[test]
    fn a_missing_face_falls_back_down_its_stack() {
        let blocks = book();
        let mut faces = Faces::default();
        faces
            .body
            .insert(0, Family::Named("No Such Face 7f3a".into()));
        let typo = Typography {
            faces,
            ..Typography::default()
        };
        let missing = Flow::new(Arc::new(blocks.clone()), &typo, Fonts::system());
        let plain = with(&blocks, Typography::default());
        assert_eq!(missing.page_count(), plain.page_count());
        let pages = |f: &Flow| f.pages.clone();
        assert_eq!(pages(&missing), pages(&plain));
    }

    /// A paragraph of `text` that reads `dir`.
    fn directed(text: &str, dir: Dir) -> Block {
        let mut b = Block::text(Role::Paragraph, text);
        b.frame.dir = dir;
        b
    }

    fn turned(text: &str) -> Block {
        let mut b = Block::text(Role::Paragraph, text);
        b.frame.turned = true;
        b
    }
}
