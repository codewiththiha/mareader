//! Styled blocks set by parley, cut into pages, drawn by vello_cpu (L1).
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

mod anim;
mod block;
mod flow;
mod fonts;
mod hyphen;
mod label;
mod meter;
mod page_text;
mod picture;
mod raster;
mod raster_fx;
mod svg;

pub use anim::{
    AnimationSample, AnimationSpec, Easing, HoverDelta, InlineSpace, Interaction, InteractiveItem,
    ItemPixmap, KeyframeStop, MultiCol, Repeat, Transform2d, TransitionSpec,
};
pub use block::{
    Align, Annotation, Area, Backdrop, BackdropSize, Block, Boxed, CHAR_BYTES, Case, Cell, Columns,
    Content, Dir, Edge, Emphasis, FEATURES, Features, Finish, Flex, Floated, Frame, Frame9,
    FrameExtra, Gradient, GradientShape, GridRow, Inline, Justify, Keep, LAYOUT_BYTES, LastLine,
    Layer, Length, Link, Look, Orient, Outline, Passage, PassageKind, Rise, Role, Row, Script,
    Shadow, Sheet, Span, Spot, Table, Text, TextShadow, Tile, VAlign, clip,
};
pub use flow::{
    Anchor, Band, COLUMN_SCALE, Design, Flow, MAX_LAID, MAX_PAGES, PAD, PAGE_HEIGHT, PAGE_WIDTH,
    PageLink, Typography,
};
pub use fonts::{
    BOOK_FACE, BookFace, FACES, Face, Faces, Family, Fonts, Kind, MONO, SANS, SERIF, has_family,
    inter, literata,
};
pub use label::{Fallback, Lines, fallbacks, label, label_lines, measure_lines};
pub use meter::{Held, held};
pub use page_text::{PageChar, PageLine};
pub use picture::{MAX_PICTURE_PIXELS, MAX_PICTURE_SIDE, Picture, PictureError};
pub use raster::{MAX_SIDE, Pixels, Region};
pub use svg::{Links, raster_svg};
pub use thin_vec::{ThinVec, thin_vec};
