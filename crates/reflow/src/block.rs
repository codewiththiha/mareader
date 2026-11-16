//! What a book says, format-free: framed blocks of text, pictures, tables.
use std::ops::Range;
use std::sync::Arc;

use thin_vec::ThinVec;

use crate::anim::{InlineSpace, Interaction, MultiCol, Repeat, Transform2d};
use crate::picture::Picture;

/// A text block's part; it picks the face, size and spacing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Level 1 to 6, as in HTML.
    Heading(u8),
    Paragraph,
    /// Code and verse: spaces and line breaks kept, set in mono.
    Preformatted,
}

impl Role {
    pub fn is_heading(self) -> bool {
        matches!(self, Self::Heading(_))
    }
}

/// How a run of text differs from the block around it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Emphasis {
    pub italic: bool,
    pub bold: bool,
    /// Normal weight inside bolder text, as a heading's plain words.
    pub light: bool,
    /// Inline code, set in the mono face.
    pub code: bool,
    /// Percent of the block's text size; 0 keeps it. Designed text only.
    pub size: u16,
    pub script: Script,
    /// Letter case shown; the text itself keeps its own.
    pub case: Case,
    pub underline: bool,
    pub strike: bool,
    /// The book's colour on the page's greys; black is the theme's ink.
    pub colour: Option<[u8; 3]>,
    /// A soft copy of the glyphs behind them, as `text-shadow`.
    pub shadow: Option<TextShadow>,
    /// Takes its room but isn't drawn, as `visibility: hidden`.
    pub hidden: bool,
    /// Type features the book turns on or off for it.
    pub features: Features,
    /// The book's own family for it, as 1 + an index into [`Text::faces`].
    pub face: u8,
}

/// OpenType features turned on or off, as bits of [`FEATURES`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Features {
    pub on: u32,
    pub off: u32,
}

/// The features CSS asks for by name, each its bit in [`Features`].
pub const FEATURES: [&str; 32] = [
    "liga", "clig", "dlig", "hlig", "calt", "kern", "onum", "lnum", "tnum", "pnum", "frac", "afrc",
    "ordn", "zero", "smcp", "c2sc", "pcap", "c2pc", "unic", "titl", "swsh", "salt", "ss01", "ss02",
    "ss03", "ss04", "ss05", "ss06", "ss07", "ss08", "subs", "sups",
];

impl Features {
    /// Feature `tag` turned on or off; none for a tag not in [`FEATURES`].
    pub fn of(tag: &str, on: bool) -> Option<Self> {
        let bit = FEATURES.iter().position(|t| *t == tag)?;
        let bit = 1u32.checked_shl(u32::try_from(bit).ok()?)?;
        Some(if on {
            Self { on: bit, off: 0 }
        } else {
            Self { on: 0, off: bit }
        })
    }

    /// These with `later` set over them.
    pub fn then(self, later: Self) -> Self {
        Self {
            on: (self.on & !later.off) | later.on,
            off: (self.off & !later.on) | later.off,
        }
    }

    pub fn is_empty(self) -> bool {
        self.on == 0 && self.off == 0
    }

    /// As CSS `font-feature-settings`, which the shaper reads.
    pub fn source(self) -> String {
        let mut out = Vec::new();
        for (bit, tag) in FEATURES.iter().enumerate() {
            let mask = 1u32 << bit;
            if self.on & mask != 0 {
                out.push(format!("\"{tag}\" 1"));
            } else if self.off & mask != 0 {
                out.push(format!("\"{tag}\" 0"));
            }
        }
        out.join(", ")
    }
}

/// A text shadow: offset and blur in hundredths of the text size.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct TextShadow {
    pub at: [i8; 2],
    pub blur: u8,
    /// Of the theme's ink, out of 255.
    pub strength: u8,
    /// True RGBA colour on fixed-layout pages; `None` uses theme ink.
    pub colour: Option<[u8; 4]>,
}

/// Raised or lowered text, as `<sup>` and `<sub>`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Script {
    #[default]
    Normal,
    Super,
    Sub,
}

/// A case shown over the text's own, as CSS transforms and small caps.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Case {
    #[default]
    Normal,
    Upper,
    Lower,
    /// Small letters shown as smaller capitals.
    Small,
    /// Every letter shown as a smaller capital.
    AllSmall,
}

/// `range` is in bytes of the block's text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub range: Range<usize>,
    pub emphasis: Emphasis,
}

/// A run that leads elsewhere; the format knows what `target` means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub range: Range<usize>,
    pub target: u32,
}

/// Styled text; spans cover only emphasised runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Text {
    pub role: Role,
    pub text: String,
    pub spans: ThinVec<Span>,
    /// Linked runs, in order and apart.
    pub links: ThinVec<Link>,
    /// Pictures set in its lines, in order.
    pub inlines: ThinVec<Inline>,
    /// Bytes of its drop cap, drawn large beside the first lines; 0 if none.
    pub initial: usize,
    /// What its first line adds, as `::first-line` does.
    pub first_line: Option<Box<Emphasis>>,
    /// Ruby: small notes set over runs of it, in order and apart.
    pub ruby: ThinVec<Annotation>,
    /// Families its runs ask for, as [`Look::faces`]; see [`Emphasis::face`].
    pub faces: ThinVec<Vec<String>>,
    /// Vertical text: runs set across the column, as `text-combine-upright`.
    pub combine: ThinVec<Range<usize>>,
    /// Horizontal gaps inside lines (inline margins and padding), in ems.
    pub spacers: ThinVec<InlineSpace>,
}

/// A ruby note, as `<rt>`, over the base text `range`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Annotation {
    pub range: Range<usize>,
    pub text: String,
}

/// A picture in a line of text, sized in ems of the text around it.
#[derive(Debug, Clone)]
pub struct Inline {
    /// The byte it sits before.
    pub at: usize,
    pub picture: Arc<Picture>,
    pub width: f32,
    pub height: f32,
    pub rise: Rise,
    /// The link it leads to, by [`Link::target`].
    pub link: Option<u32>,
    /// Takes its room but isn't drawn, as `visibility: hidden`.
    pub hidden: bool,
}

/// Where a picture in a line sits against its text, as `vertical-align`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum Rise {
    /// Its bottom on the text's baseline.
    #[default]
    Baseline,
    Middle,
    /// Against the text's ascent or descent.
    TextTop,
    TextBottom,
    /// Against the whole line's top or bottom.
    Top,
    Bottom,
    Sub,
    Super,
    /// Raised this many ems of the text; below zero lowers it.
    By(f32),
}

impl PartialEq for Inline {
    fn eq(&self, other: &Self) -> bool {
        let size = |i: &Self| [i.width, i.height].map(f32::to_bits);
        let rise = |i: &Self| match i.rise {
            Rise::By(em) => Some(em.to_bits()),
            _ => None,
        };
        self.at == other.at
            && Arc::ptr_eq(&self.picture, &other.picture)
            && size(self) == size(other)
            && std::mem::discriminant(&self.rise) == std::mem::discriminant(&other.rise)
            && rise(self) == rise(other)
            && self.link == other.link
            && self.hidden == other.hidden
    }
}

impl Eq for Inline {}

impl Text {
    pub fn new(role: Role) -> Self {
        Self {
            role,
            text: String::new(),
            spans: ThinVec::new(),
            links: ThinVec::new(),
            inlines: ThinVec::new(),
            initial: 0,
            first_line: None,
            ruby: ThinVec::new(),
            faces: ThinVec::new(),
            combine: ThinVec::new(),
            spacers: ThinVec::new(),
        }
    }

    /// Appends `text` as part of link `target`; a leading space isn't.
    pub fn push_link(&mut self, text: &str, emphasis: Emphasis, target: u32) {
        let at = self.text.len();
        let start = at + (text.len() - text.trim_start_matches(' ').len());
        self.push(text, emphasis);
        let end = self.text.len();
        if start >= end {
            return;
        }
        match self.links.last_mut() {
            // The same link going on, past a space between words.
            Some(last) if last.range.end == at && last.target == target => {
                last.range.end = end;
            }
            _ => self.links.push(Link {
                range: start..end,
                target,
            }),
        }
    }

    /// Appends `text`, merging with the last span when they match.
    pub fn push(&mut self, text: &str, emphasis: Emphasis) {
        let start = self.text.len();
        self.text.push_str(text);
        if emphasis == Emphasis::default() || text.is_empty() {
            return;
        }
        let end = self.text.len();
        match self.spans.last_mut() {
            Some(last) if last.range.end == start && last.emphasis == emphasis => {
                last.range.end = end;
            }
            _ => self.spans.push(Span {
                range: start..end,
                emphasis,
            }),
        }
    }

    /// Drops trailing whitespace, clipping spans to the text.
    pub fn trim_end(&mut self) {
        let len = self.text.trim_end().len();
        self.text.truncate(len);
        for span in &mut self.spans {
            span.range.end = span.range.end.min(len);
        }
        self.spans.retain(|s| !s.range.is_empty());
        for link in &mut self.links {
            link.range.end = link.range.end.min(len);
        }
        self.links.retain(|l| !l.range.is_empty());
        for inline in &mut self.inlines {
            inline.at = inline.at.min(len);
        }
        for spacer in &mut self.spacers {
            spacer.at = spacer.at.min(len);
        }
        self.spacers.retain(|s| s.width > 0.0);
    }

    pub fn is_blank(&self) -> bool {
        self.text.trim().is_empty() && self.inlines.is_empty()
    }
}

/// One table row; a head row is set bold on a shaded ground.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub cells: Vec<Text>,
    pub head: bool,
}

/// Rows of cells in equal columns; rows never split across pages.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Table {
    pub rows: Vec<Row>,
    /// The grid's colour, as [`Emphasis::colour`]; `None` is the faint rule.
    pub rule: Option<[u8; 3]>,
}

/// What a block holds.
#[derive(Debug, Clone, PartialEq)]
pub enum Content {
    Text(Text),
    /// A thematic break, drawn as a hairline.
    Rule,
    /// Never split; `cover` fills the page box instead of the column.
    Image {
        picture: Arc<Picture>,
        cover: bool,
        /// The width the book asked for, if any.
        width: Option<Length>,
        /// Its parts that lead elsewhere, as a link or image map.
        links: Vec<Area>,
    },
    Table(Table),
    /// Blank height in body ems; only its boxes draw, as a book's rules.
    Space(f32),
}

/// A part of a picture that leads to link `target`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    /// Left, top, width, height as shares of the picture, 0 to 1.
    pub rect: [f32; 4],
    pub target: u32,
}

impl Area {
    /// The whole picture, leading to `target`.
    pub fn whole(target: u32) -> Self {
        Self {
            rect: [0.0, 0.0, 1.0, 1.0],
            target,
        }
    }
}

/// A picture's width as a book sets it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Length {
    /// Ems of the reader's text size.
    Em(f32),
    /// A share of the column, 0 to 100.
    Percent(f32),
    /// Ems plus a share of the column, as `calc()` sums them.
    Mix { em: f32, percent: f32 },
}

/// Line alignment within the block's column.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
}

/// How a designed chapter's book styles a block (`docs/epub-layout.md`).
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    /// Its chapter is designed: in Auto, the book's look applies.
    pub designed: bool,
    /// Text size in body ems.
    pub size: f32,
    /// A multiple of `size`; none keeps the reader's.
    pub line_height: Option<f32>,
    /// Extra advance per letter and per word, in body ems.
    pub letter: f32,
    pub word: f32,
    /// The first line's indent, in body ems.
    pub indent: f32,
    pub weight: f32,
    /// Families tried before the reader's, the book's own included.
    pub faces: Vec<String>,
    pub justify: bool,
    /// How a paragraph's last line sits; none follows the rest.
    pub last: Option<LastLine>,
}

/// A paragraph's last line, as `text-align-last`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LastLine {
    Start,
    Center,
    End,
    Justify,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            designed: false,
            size: 1.0,
            line_height: None,
            letter: 0.0,
            word: 0.0,
            indent: 0.0,
            weight: 400.0,
            faces: Vec::new(),
            justify: false,
            last: None,
        }
    }
}

/// A border's line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Edge {
    #[default]
    None,
    Solid,
    Double,
    Dashed,
    Dotted,
    /// Two-tone lines that look carved in or raised.
    Groove,
    Ridge,
    Inset,
    Outset,
    /// Takes its width but draws nothing, as a transparent border.
    Clear,
}

impl Edge {
    /// Whether the side draws a line.
    pub fn draws(self) -> bool {
        !matches!(self, Self::None | Self::Clear)
    }
}

/// A book's box around blocks, in body ems; sides go top, right, bottom, left.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Boxed {
    /// Left and right margins; none is auto, which centres a set width.
    pub margin: [Option<Length>; 2],
    pub border: [f32; 4],
    pub edges: [Edge; 4],
    pub padding: [f32; 4],
    /// The content's width; none fills what holds it.
    pub width: Option<Length>,
    /// From the top border to its first block, and its last to the bottom.
    pub top: f32,
    pub bottom: f32,
    /// A share of the page's height its content is centred in.
    pub centre: Option<f32>,
    /// Its background as a strength of the theme's ink, 0 to 1.
    pub fill: Option<f32>,
    /// Each border side's colour, as [`Emphasis::colour`]; `None` is ink.
    pub colours: [Option<[u8; 3]>; 4],
    /// The least height of its content, in body ems.
    pub height: Option<f32>,
    pub backdrop: Option<Arc<Backdrop>>,
    /// Its corners' radius; a percentage is of its width.
    pub radius: Option<Length>,
    /// Shadows, gradients, a border picture, an outline and opacity.
    pub finish: Option<Arc<Finish>>,
    /// Stands for a rule: read as prose, it draws a plain one.
    pub rule: bool,
    /// Kept on one page when it fits, as `break-inside: avoid`.
    pub whole: bool,
    /// True RGBA background on fixed pages or translucent overlays.
    pub fill_rgba: Option<[u8; 4]>,
    /// `box-sizing: border-box`: width and height include padding and borders.
    pub border_box: bool,
    /// `overflow: hidden | auto | scroll`: clips children to this box.
    pub clip: bool,
    /// CSS multi-column layout (`column-count`, `width`, `rule`).
    pub multicol: Option<Arc<MultiCol>>,
    /// 2D CSS transform (`scale`, `translate`, `rotate`, `skew`).
    pub transform: Option<Transform2d>,
    /// Hover transitions and `@keyframes` animations bound to this box.
    pub interaction: Option<Arc<Interaction>>,
}

/// What a box draws beyond its ground, picture and borders.
#[derive(Debug, Clone, PartialEq)]
pub struct Finish {
    /// The first listed is drawn on top.
    pub shadows: Vec<Shadow>,
    /// Background layers over its ground; the first listed is on top.
    pub layers: Vec<Layer>,
    /// A picture cut in nine drawn in place of its borders.
    pub frame: Option<Frame9>,
    pub outline: Option<Outline>,
    /// How much of the box shows, 0 to 1.
    pub opacity: f32,
}

impl Default for Finish {
    fn default() -> Self {
        Self {
            shadows: Vec::new(),
            layers: Vec::new(),
            frame: None,
            outline: None,
            opacity: 1.0,
        }
    }
}

impl Finish {
    /// Whether it draws anything of its own.
    pub fn shows(&self) -> bool {
        !self.shadows.is_empty()
            || !self.layers.is_empty()
            || self.frame.is_some()
            || self.outline.is_some()
    }
}

/// A box's shadow in body ems; its strength is of the theme's ink.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Shadow {
    pub at: [f32; 2],
    pub blur: f32,
    pub spread: f32,
    pub strength: f32,
    /// Cast inside the box, under its content.
    pub inset: bool,
    /// True RGBA shadow colour on fixed-layout pages; `None` uses theme ink.
    pub colour: Option<[u8; 4]>,
}

/// One background layer: a picture or a gradient.
#[derive(Debug, Clone, PartialEq)]
pub enum Layer {
    Picture(Arc<Backdrop>),
    Gradient(Gradient),
}

/// A gradient between strengths of the theme's ink.
#[derive(Debug, Clone, PartialEq)]
pub struct Gradient {
    pub shape: GradientShape,
    /// Where each stop sits, none spread evenly; its strength, 0 to 1.
    pub stops: Vec<(Option<Length>, f32)>,
    /// Repeats past its last stop.
    pub repeating: bool,
}

/// A gradient's line or ring.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GradientShape {
    /// Its heading in degrees: 0 runs up, 90 right, 180 down.
    Linear(f32),
    /// Its centre, 0 to 1 across and down; reaching the nearest side.
    Radial {
        at: [f32; 2],
        circle: bool,
        near: bool,
    },
}

/// How a border picture's edges fill their sides.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Tile {
    #[default]
    Stretch,
    Repeat,
    /// Repeats a whole number of times, scaled to fit.
    Round,
}

/// A border picture cut in nine, as `border-image`.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame9 {
    pub picture: Arc<Picture>,
    /// Each cut as a share of the picture's height or width.
    pub slice: [f32; 4],
    /// Its middle is drawn too.
    pub fill: bool,
    /// Each side's breadth, and how far it reaches out, in body ems.
    pub width: [f32; 4],
    pub outset: [f32; 4],
    /// Across and down.
    pub tile: [Tile; 2],
}

/// A line around a box's border, as `outline`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Outline {
    /// In body ems.
    pub width: f32,
    pub offset: f32,
    pub edge: Edge,
    pub colour: Option<[u8; 3]>,
}

/// A box's background picture.
#[derive(Debug, Clone, PartialEq)]
pub struct Backdrop {
    pub picture: Arc<Picture>,
    pub size: BackdropSize,
    /// Where it sits: 0 at the left or top, 1 at the right or bottom.
    pub at: [f32; 2],
    /// Tiled across the box rather than drawn once.
    pub repeat: bool,
    /// Axis-specific tiling (`repeat-x`, `repeat-y`); overrides `repeat`.
    pub repeat_axis: Option<Repeat>,
}

/// How large a background picture is drawn.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum BackdropSize {
    /// Its own size, a pixel to a book pixel.
    #[default]
    Auto,
    Cover,
    Contain,
    /// Width and height; none keeps its proportions.
    Set([Option<Length>; 2]),
}

/// A floated element: its blocks sit at one side, text wrapping past them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Floated {
    pub right: bool,
    /// How many of its blocks' boxes hold it; the rest are its own.
    pub depth: u8,
}

/// What a passage read aloud may pass over whole is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassageKind {
    List,
    Figure,
    Aside,
}

/// One list, figure or aside; blocks of one share it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Passage {
    pub kind: PassageKind,
}

/// Where a cell's content sits in a taller row.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum VAlign {
    Top,
    #[default]
    Middle,
    Bottom,
}

/// A row of a book's layout grid: its cells' widths and alignment.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GridRow {
    /// Each cell's width as set; none sizes it to its content.
    pub widths: Vec<Option<Length>>,
    pub valign: Vec<VAlign>,
    /// The space above the row, in body ems.
    pub before: f32,
    /// Each cell's first column and columns spanned; empty is one each.
    pub starts: Vec<u16>,
    pub spans: Vec<u16>,
    /// A flex row's share of free space per item; empty is none.
    pub grow: Vec<f32>,
    /// Rows each cell spans down, its own included; empty is one each.
    pub down: Vec<u16>,
    /// Shared by a table's rows, whose columns line up.
    pub columns: Arc<Columns>,
}

/// What a grid's rows share: a table's columns, or a flex row's spacing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Columns {
    pub flex: Option<Flex>,
    /// A table with a set width: its columns stretch to fill it.
    pub fill: bool,
    /// A table centred by auto margins.
    pub centre: bool,
}

/// A flex row's spacing.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Flex {
    pub justify: Justify,
    /// Between items and between wrapped lines, in body ems.
    pub gap: f32,
    pub wrap: bool,
}

/// Where a flex row's free space goes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Justify {
    #[default]
    Start,
    Center,
    End,
    Between,
    Around,
    Evenly,
}

/// A block's place in a grid row; the row's blocks sit side by side.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub row: Arc<GridRow>,
    pub column: u16,
    /// How many of the frame's boxes hold the grid; the rest are the cell's.
    pub depth: u8,
}

/// Where a block sits: nesting from lists and quotes, and alignment.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Frame {
    /// Left indent in body ems.
    pub inset: f32,
    /// Quote bars, each at this many body ems from the column's left.
    pub bars: ThinVec<f32>,
    /// A list marker hung left of the first line, like "•" or "3.".
    pub marker: Option<Box<str>>,
    /// The marker's colour, as [`Emphasis::colour`].
    pub marker_colour: Option<[u8; 3]>,
    /// A picture shown as the marker, after its text.
    pub marker_picture: Option<Arc<Picture>>,
    pub align: Align,
    /// The alignment names a side, `left` or `right`, kept in any direction.
    pub sided: bool,
    /// A small gap above, as between list items.
    pub tight: bool,
    /// Quoted text, drawn in a softer ink.
    pub soft: bool,
    /// The book's look, used as the Book design setting says.
    pub look: Option<Arc<Look>>,
    /// With a look: the space above, in body ems, margins collapsed.
    pub before: f32,
    /// The same without paragraphs' own margins, which a reader may replace.
    pub outer: f32,
    /// With a look: the boxes around it, outermost first.
    pub boxes: ThinVec<Arc<Boxed>>,
    /// With a look: its place in a grid row.
    pub cell: Option<Cell>,
    /// With a look: the float it is in; blocks of one float share it.
    pub float: Option<Arc<Floated>>,
    /// With a look: a turn in degrees, clockwise; drawn, not laid out.
    pub rotate: f32,
    /// With a look: how page breaks treat it.
    pub keep: Keep,
    /// Its text's direction; lists and quotes mirror for right to left.
    pub dir: Dir,
    /// Its chapter is written vertically: columns read right to left.
    pub turned: bool,
    /// How vertical text stands, as `text-orientation`.
    pub orient: Orient,
    /// Its chapter's fixed page; blocks of one page share it.
    pub sheet: Option<Arc<Sheet>>,
    /// On a fixed page: the positioned box it is in; blocks of one share it.
    pub spot: Option<Arc<Spot>>,
    /// Takes its room but draws nothing, as `visibility: hidden`.
    pub hidden: bool,
    /// The outermost list, figure and aside it is in, outer first.
    pub passages: ThinVec<Arc<Passage>>,
    /// Rare CSS extras kept out of line so plain paragraphs stay small.
    pub extra: Option<Box<FrameExtra>>,
}

/// Rare CSS layout and visual extras attached to a [`Frame`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FrameExtra {
    /// 2D CSS transform on the block itself (images, transformed elements).
    pub transform: Option<Transform2d>,
    /// Hover transitions and `@keyframes` animations on this block.
    pub interaction: Option<Arc<Interaction>>,
    /// `position: relative` visual offset `[dx, dy]` in body ems.
    pub rel_offset: [f32; 2],
    /// `column-span: all`: spans across all columns in a multicol box.
    pub column_span: bool,
    /// Explicit `[height, max_height]` on an image block.
    pub picture_fit: Option<[Option<Length>; 2]>,
}

impl Frame {
    /// Allocates and returns mutable access to rare CSS extras.
    pub fn extra_mut(&mut self) -> &mut FrameExtra {
        self.extra.get_or_insert_with(Default::default)
    }

    pub fn rel_offset(&self) -> [f32; 2] {
        self.extra.as_deref().map_or([0.0; 2], |e| e.rel_offset)
    }

    pub fn column_span(&self) -> bool {
        self.extra.as_deref().is_some_and(|e| e.column_span)
    }

    pub fn transform(&self) -> Option<Transform2d> {
        self.extra.as_deref().and_then(|e| e.transform)
    }

    pub fn interaction(&self) -> Option<&Arc<Interaction>> {
        self.extra.as_deref().and_then(|e| e.interaction.as_ref())
    }

    pub fn picture_fit(&self) -> Option<[Option<Length>; 2]> {
        self.extra.as_deref().and_then(|e| e.picture_fit)
    }
}

/// A fixed-layout page: its blocks laid at the book's own viewport.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sheet {
    /// The viewport in CSS px.
    pub width: f32,
    pub height: f32,
    /// What the page's root paints under everything.
    pub ground: Option<Arc<Boxed>>,
}

/// A box `position` sets on a fixed page, in the page's CSS px.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Spot {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    /// A set height: pictures inside fit it.
    pub height: Option<f32>,
    /// Hung from this line instead of `y`, as `bottom` without `top`.
    pub bottom: Option<f32>,
    /// `z-index`: higher ones paint later.
    pub z: i32,
    /// How many of the frame's boxes hold it; the rest are its own.
    pub depth: u8,
}

/// How vertical text stands, as `text-orientation`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Orient {
    /// East Asian letters upright, the rest turned on their side.
    #[default]
    Mixed,
    Upright,
    Sideways,
}

/// A block's direction, as `dir` and CSS `direction`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Dir {
    /// From its first strong letter.
    #[default]
    Auto,
    Ltr,
    Rtl,
}

/// How page breaks treat a block, as `widows`, `orphans` and `break-*`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Keep {
    /// Lines kept together at its start and end; 0 is the usual 2.
    pub orphans: u8,
    pub widows: u8,
    /// Kept on one page when it fits.
    pub whole: bool,
    /// Kept on the page of the block after it.
    pub with_next: bool,
    /// Kept on the page of the block before it, as `break-before: avoid`.
    pub with_last: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub content: Content,
    pub frame: Frame,
    /// Starts a new page, as each chapter of a book does.
    pub break_before: bool,
}

/// `range` of `text`, held to its length and to character edges.
pub fn clip(text: &str, range: Range<usize>) -> Range<usize> {
    let edge = |mut i: usize| {
        i = i.min(text.len());
        while !text.is_char_boundary(i) {
            i -= 1;
        }
        i
    };
    let (start, end) = (edge(range.start), edge(range.end));
    start..end.max(start)
}

impl Text {
    /// Bytes `range` of the text, its styles and links kept.
    pub fn cut(&self, range: Range<usize>) -> Self {
        let range = clip(&self.text, range);
        if range == (0..self.text.len()) {
            return self.clone();
        }
        let shift = |r: &Range<usize>| {
            let start = r.start.max(range.start);
            let end = r.end.min(range.end);
            (start < end).then(|| start - range.start..end - range.start)
        };
        let mut out = Self::new(self.role);
        out.text = self.text.get(range.clone()).unwrap_or_default().to_owned();
        out.spans = (self.spans.iter())
            .filter_map(|s| {
                Some(Span {
                    range: shift(&s.range)?,
                    emphasis: s.emphasis,
                })
            })
            .collect();
        out.links = (self.links.iter())
            .filter_map(|l| {
                Some(Link {
                    range: shift(&l.range)?,
                    target: l.target,
                })
            })
            .collect();
        out.inlines = (self.inlines.iter())
            .filter(|i| (range.start..range.end).contains(&i.at))
            .map(|i| Inline {
                at: i.at - range.start,
                ..i.clone()
            })
            .collect();
        out.spacers = (self.spacers.iter())
            .filter(|s| (range.start..=range.end).contains(&s.at))
            .map(|s| InlineSpace {
                at: s.at - range.start,
                width: s.width,
            })
            .collect();
        out
    }
}

impl Block {
    /// Bytes of its text or rows of its table; anything else whole.
    pub fn cut(&self, range: Range<usize>) -> Self {
        let content = match &self.content {
            Content::Text(t) => Content::Text(t.cut(range)),
            Content::Table(t) => {
                let rows = t.rows.get(range).map(<[_]>::to_vec).unwrap_or_default();
                Content::Table(Table { rows, rule: t.rule })
            }
            other => other.clone(),
        };
        Self {
            content,
            frame: self.frame.clone(),
            break_before: false,
        }
    }

    pub fn new(content: Content) -> Self {
        Self {
            content,
            frame: Frame::default(),
            break_before: false,
        }
    }

    /// A text block with an unframed role, for simple callers.
    pub fn text(role: Role, text: &str) -> Self {
        let mut t = Text::new(role);
        t.push(text, Emphasis::default());
        Self::new(Content::Text(t))
    }

    /// Blank text, empty tables: nothing to set.
    pub fn is_blank(&self) -> bool {
        match &self.content {
            Content::Text(t) => t.is_blank(),
            Content::Table(t) => t.rows.iter().all(|r| r.cells.iter().all(Text::is_blank)),
            Content::Rule | Content::Image { .. } | Content::Space(_) => false,
        }
    }

    pub fn is_heading(&self) -> bool {
        matches!(&self.content, Content::Text(t) if t.role.is_heading())
    }

    /// Bytes its layouts are likely to hold once set (`docs/memory.md`).
    pub fn layout_cost(&self) -> usize {
        let cost = |t: &Text| LAYOUT_BYTES.saturating_add(t.text.len().saturating_mul(CHAR_BYTES));
        match &self.content {
            Content::Text(t) => cost(t),
            Content::Table(t) => t.rows.iter().flat_map(|r| &r.cells).map(cost).sum(),
            Content::Rule | Content::Image { .. } | Content::Space(_) => 0,
        }
    }
}

/// What one set paragraph or cell holds before its text, measured.
pub const LAYOUT_BYTES: usize = 3000;
/// What each byte of set text adds, measured.
pub const CHAR_BYTES: usize = 48;

#[cfg(test)]
#[path = "block_tests.rs"]
mod tests;
