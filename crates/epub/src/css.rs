//! A CSS cascade: selectors, specificity, inheritance, computed lengths.
//! See `docs/epub-layout.md`.
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use mareader_reflow::{
    Align, Case, Dir, Features, Justify, LastLine, Orient, Rise, Script, Tile, VAlign,
};

use crate::calc::{self, Vars};
use crate::decor::{self, Slice, Span};
use crate::generated::{self, Content, ListStyle};
use crate::href;
use crate::query;

pub(crate) mod anim;
pub(crate) mod extra;
pub(crate) mod hover;

pub(crate) use hover::declared_hover;
use hover::initial_value;

/// A stylesheet's text past this is ignored.
pub const MAX_SHEET: usize = 1 << 20;
const MAX_RULES: usize = 10_000;
/// Compounds in one selector; longer ones are skipped.
const MAX_PARTS: usize = 16;
const MAX_FACES: usize = 256;
/// Custom properties one declaration block sets.
const MAX_CUSTOM: usize = 64;
/// Sheets one sheet imports.
const MAX_IMPORTS: usize = 8;
/// What `1rem` and the root's text measure, as browsers default it.
pub(crate) const ROOT_PX: f32 = 16.0;
/// What `vh` and `vw` measure: the page's text box, near enough.
pub(crate) const VIEW_HEIGHT: f32 = 980.0;
pub(crate) const VIEW_WIDTH: f32 = 650.0;
/// Percent widths of padding and margins: a column, near enough.
pub(crate) const BASE_WIDTH: f32 = 600.0;

/// A length as written; `Em` is of the element's own text size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Len {
    Px(f32),
    Em(f32),
    Rem(f32),
    Percent(f32),
    Vh(f32),
    Vw(f32),
    /// A sum, as `calc()` leaves one: px, ems and percent.
    Mix {
        px: f32,
        em: f32,
        pct: f32,
    },
    Auto,
}

impl Len {
    /// In px: `em` is the text size, `base` what 100% is.
    pub(crate) fn px(self, em: f32, base: f32) -> Option<f32> {
        Some(match self {
            Self::Px(n) => n,
            Self::Em(n) => n * em,
            Self::Rem(n) => n * ROOT_PX,
            Self::Percent(n) => n * base / 100.0,
            Self::Vh(n) => n * VIEW_HEIGHT / 100.0,
            Self::Vw(n) => n * VIEW_WIDTH / 100.0,
            Self::Mix { px, em: e, pct } => px + e * em + pct * base / 100.0,
            Self::Auto => return None,
        })
    }

    /// Computed: absolute units become px; percentages and auto stay.
    fn computed(self, em: f32) -> Self {
        match self {
            Self::Percent(_) | Self::Vw(_) | Self::Auto => self,
            Self::Mix { px, em: e, pct } if pct != 0.0 => Self::Mix {
                px: px + e * em,
                em: 0.0,
                pct,
            },
            other => Self::Px(other.px(em, 0.0).unwrap_or(0.0)),
        }
    }
}

/// Any CSS length: absolute units, font units, percent and viewport units.
pub(crate) fn len(v: &str) -> Option<Len> {
    let v = v.trim();
    if v.eq_ignore_ascii_case("auto") {
        return Some(Len::Auto);
    }
    if calc::is_maths(v) {
        return calc::maths(v).and_then(calc::Sum::length);
    }
    let numeric = |c: char| c.is_ascii_digit() || matches!(c, '.' | '-' | '+');
    let split = v.find(|c: char| !numeric(c)).unwrap_or(v.len());
    let (number, unit) = v.split_at(split);
    let n: f32 = number.parse().ok().filter(|n: &f32| n.is_finite())?;
    let n = n.clamp(-10_000.0, 10_000.0);
    Some(match unit.trim().to_ascii_lowercase().as_str() {
        "" if n == 0.0 => Len::Px(0.0),
        "px" => Len::Px(n),
        "pt" => Len::Px(n * 4.0 / 3.0),
        "pc" => Len::Px(n * 16.0),
        "in" => Len::Px(n * 96.0),
        "cm" => Len::Px(n * 96.0 / 2.54),
        "mm" => Len::Px(n * 96.0 / 25.4),
        "q" => Len::Px(n * 96.0 / 101.6),
        "em" => Len::Em(n),
        "ex" | "ch" => Len::Em(n / 2.0),
        "rem" => Len::Rem(n),
        "%" => Len::Percent(n),
        // Pages stand upright: the short side is the width.
        "vh" | "vmax" => Len::Vh(n),
        "vw" | "vmin" => Len::Vw(n),
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Weight {
    Abs(u16),
    Bolder,
    Lighter,
}

/// `font-variant-caps`, as the text shows it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Caps {
    #[default]
    Normal,
    Small,
    AllSmall,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum TextAlign {
    #[default]
    Start,
    Center,
    End,
    Justify,
    /// `left` and `right`: sides, whatever the direction.
    Left,
    Right,
}

impl TextAlign {
    pub(crate) fn align(self) -> Align {
        match self {
            Self::Start | Self::Justify | Self::Left => Align::Start,
            Self::Center => Align::Center,
            Self::End | Self::Right => Align::End,
        }
    }

    /// Whether it names a side rather than a start or end.
    pub(crate) fn sided(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Display {
    #[default]
    Inline,
    Block,
    None,
    Flex,
    Grid,
    Table,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum FontSize {
    Len(Len),
    Larger,
    Smaller,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum LineHeight {
    Normal,
    Factor(f32),
    Len(Len),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum BorderStyle {
    #[default]
    None,
    Solid,
    Double,
    Dashed,
    Dotted,
    Groove,
    Ridge,
    Inset,
    Outset,
}

/// The properties read; `None` keeps what cascades or inherits.
/// Sides go top, right, bottom, left.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Decl {
    pub(crate) italic: Option<bool>,
    pub(crate) weight: Option<Weight>,
    pub(crate) align: Option<TextAlign>,
    pub(crate) display: Option<Display>,
    /// `list-style: none`: list items show no marker.
    pub(crate) unmarked: Option<bool>,
    pub(crate) width: Option<Len>,
    pub(crate) height: Option<Len>,
    pub(crate) size: Option<FontSize>,
    pub(crate) line: Option<LineHeight>,
    pub(crate) letter: Option<Len>,
    pub(crate) word: Option<Len>,
    pub(crate) indent: Option<Len>,
    pub(crate) family: Option<Arc<[String]>>,
    pub(crate) margin: [Option<Len>; 4],
    pub(crate) padding: [Option<Len>; 4],
    pub(crate) border_width: [Option<Len>; 4],
    pub(crate) border_style: [Option<BorderStyle>; 4],
    /// `Some(None)` is the text's own colour, as `currentcolor`.
    pub(crate) border_colour: [Option<Option<Rgb>>; 4],
    pub(crate) background: Option<bool>,
    pub(crate) background_image: Option<bool>,
    pub(crate) float: Option<Float>,
    /// `position`, its `top`, `right`, `bottom` and `left`, and `z-index`.
    pub(crate) placing: Option<Placing>,
    pub(crate) offsets: [Option<Len>; 4],
    pub(crate) z_index: Option<i32>,
    pub(crate) break_before: Option<bool>,
    pub(crate) break_after: Option<bool>,
    pub(crate) valign: Option<VAlign>,
    pub(crate) script: Option<Script>,
    pub(crate) small_caps: Option<Caps>,
    /// `font-variant-*` groups, `font-kerning`, then `font-feature-settings`.
    pub(crate) numeric: Option<Features>,
    pub(crate) ligatures: Option<Features>,
    pub(crate) position: Option<Features>,
    pub(crate) kerning: Option<Features>,
    pub(crate) settings: Option<Features>,
    /// `text-align-last`; `Some(None)` is auto.
    pub(crate) last: Option<Option<LastLine>>,
    pub(crate) orphans: Option<u8>,
    pub(crate) widows: Option<u8>,
    /// `break-inside: avoid`, and `break-after: avoid`.
    pub(crate) whole: Option<bool>,
    pub(crate) keep_next: Option<bool>,
    /// `break-before: avoid`: kept with the block before.
    pub(crate) keep_last: Option<bool>,
    /// `direction`, or the `dir` attribute, which may be auto.
    pub(crate) dir: Option<Dir>,
    /// `writing-mode`: true when vertical.
    pub(crate) writing: Option<bool>,
    /// `text-orientation` and `text-combine-upright`.
    pub(crate) orient: Option<Orient>,
    pub(crate) combine: Option<bool>,
    pub(crate) transform: Option<Case>,
    /// Underline and line-through.
    pub(crate) decoration: Option<(bool, bool)>,
    /// The background colour; `Some(None)` is none.
    pub(crate) fill: Option<Option<Rgb>>,
    /// The text colour, `color`.
    pub(crate) ink: Option<Rgb>,
    /// The background picture, resolved once its sheet's path is known.
    pub(crate) picture: Option<Option<Arc<str>>>,
    pub(crate) picture_size: Option<PictureSize>,
    pub(crate) picture_at: Option<[f32; 2]>,
    pub(crate) picture_repeat: Option<bool>,
    /// A flex container laying its items in a column, not a row.
    pub(crate) flex_column: Option<bool>,
    pub(crate) justify: Option<Justify>,
    pub(crate) align_items: Option<VAlign>,
    pub(crate) align_self: Option<VAlign>,
    /// `align-items` and `align-self` stretch, across a column's width.
    pub(crate) items_stretch: Option<bool>,
    pub(crate) self_stretch: Option<bool>,
    pub(crate) wrap: Option<bool>,
    pub(crate) gap: Option<Len>,
    pub(crate) row_gap: Option<Len>,
    pub(crate) grow: Option<f32>,
    pub(crate) basis: Option<Len>,
    pub(crate) radius: Option<Len>,
    /// A turn in degrees.
    pub(crate) rotate: Option<f32>,
    /// `vertical-align` as a line picture reads it.
    pub(crate) lift: Option<Lift>,
    /// What `::before` and `::after` show.
    pub(crate) content: Option<Content>,
    pub(crate) counter_reset: Option<Counters>,
    pub(crate) counter_increment: Option<Counters>,
    pub(crate) counter_set: Option<Counters>,
    /// Quote marks by depth; empty for none.
    pub(crate) quotes: Option<Quotes>,
    pub(crate) list_style: Option<ListStyle>,
    pub(crate) list_image: Option<Option<Arc<str>>>,
    /// Background layers beyond a lone picture: gradients and lists.
    pub(crate) layers: Option<Arc<[decor::Layer]>>,
    pub(crate) shadows: Option<Arc<[decor::Shadow]>>,
    pub(crate) text_shadow: Option<Option<decor::Shadow>>,
    pub(crate) opacity: Option<f32>,
    pub(crate) outline_width: Option<Len>,
    pub(crate) outline_style: Option<BorderStyle>,
    pub(crate) outline_colour: Option<Option<Rgb>>,
    pub(crate) outline_offset: Option<Len>,
    /// `border-image` and its parts.
    pub(crate) frame: Option<Option<Arc<str>>>,
    pub(crate) frame_slice: Option<Slice>,
    pub(crate) frame_width: Option<[Span; 4]>,
    pub(crate) frame_outset: Option<[Span; 4]>,
    pub(crate) frame_tile: Option<[Tile; 2]>,
    /// Sides whose border colour is transparent: room, no line.
    pub(crate) border_clear: [Option<bool>; 4],
    /// `visibility`: false hides it but keeps its room.
    pub(crate) visible: Option<bool>,
    /// `filter`'s `opacity()` and `drop-shadow()`.
    pub(crate) filter_opacity: Option<f32>,
    pub(crate) drop: Option<Arc<[decor::Shadow]>>,
    /// `--name` declarations as written, in order.
    pub(crate) custom: Vec<(String, String)>,
    /// Declarations naming `var()`, set once the element's values are known.
    pub(crate) deferred: Vec<(String, String)>,
    /// The sheet `deferred` came from, for its `url()`s.
    pub(crate) from: Option<Arc<str>>,
    /// Properties set back to inherit, or to their initial values.
    pub(crate) cleared: Option<Arc<Decl>>,
    /// The element's custom properties, once its cascade is done.
    pub(crate) vars: Option<Vars>,
    /// Extended properties (RGBA fills, box-sizing, multicol, transforms).
    pub(crate) extra: Option<Box<extra::ExtraDecl>>,
}

impl Decl {
    pub(crate) fn extra_mut(&mut self) -> &mut extra::ExtraDecl {
        self.extra
            .get_or_insert_with(|| Box::new(extra::ExtraDecl::default()))
    }
}

/// Counters a declaration names, with their values.
pub(crate) type Counters = Arc<[(String, i32)]>;
/// Open and close marks, outermost first.
pub(crate) type Quotes = Arc<[(String, String)]>;

/// `vertical-align` as written: a keyword's place, or a length.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Lift {
    Keyword(Rise),
    Length(Len),
}

/// A colour's red, green and blue.
pub(crate) type Rgb = [u8; 3];

/// `position`: only fixed pages place boxes by it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Placing {
    #[default]
    Static,
    Relative,
    Absolute,
    Fixed,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Float {
    #[default]
    None,
    Left,
    Right,
}

/// `background-size`; a length's `None` is auto.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) enum PictureSize {
    #[default]
    Auto,
    Cover,
    Contain,
    Set([Option<Len>; 2]),
}

/// Unsets each field `top` sets.
macro_rules! unlayer {
    ($self:ident, $top:ident; $($f:ident),*; $($side:ident),*) => {
        $(if $top.$f.is_some() { $self.$f = None; })*
        $(for (mine, theirs) in $self.$side.iter_mut().zip(&$top.$side) {
            if theirs.is_some() { *mine = None; }
        })*
    };
}

/// Calls macro `$m` with every property field, then the four-sided ones.
macro_rules! each_field {
    ($m:ident, $a:ident, $b:ident) => {
        $m!($a, $b;
            italic, weight, align, display, unmarked, width, height, size, line,
            letter, word, indent, family, background, background_image, float, placing, z_index,
            break_before, break_after, valign, script, small_caps, numeric, ligatures, position,
            kerning, settings, last, orphans, widows, whole, keep_next, keep_last, dir,
            writing, orient, combine,
            transform, decoration,
            fill, ink, picture, picture_size, picture_at, picture_repeat, flex_column, justify,
            align_items, align_self, items_stretch, self_stretch, wrap, gap, row_gap, grow,
            basis, radius, rotate, lift, content, counter_reset, counter_increment,
            counter_set, quotes, list_style, list_image, layers, shadows, text_shadow,
            opacity, outline_width, outline_style, outline_colour, outline_offset, frame,
            frame_slice, frame_width, frame_outset, frame_tile, visible, filter_opacity, drop;
            margin, padding, border_width, border_style, border_colour, border_clear, offsets)
    };
}

/// Sets each field `top` sets.
macro_rules! layer {
    ($self:ident, $top:ident; $($f:ident),*; $($side:ident),*) => {
        $(if $top.$f.is_some() { $self.$f = $top.$f.clone(); })*
        $(for (mine, theirs) in $self.$side.iter_mut().zip(&$top.$side) {
            if theirs.is_some() { *mine = *theirs; }
        })*
    };
}

impl Decl {
    /// A declaration block, `!important` ones folded over the rest.
    pub(crate) fn parse(text: &str) -> Self {
        let (mut normal, important) = Self::parse_both(text);
        normal.layer(&important);
        normal
    }

    /// A declaration block as its normal and `!important` halves.
    pub(crate) fn parse_both(text: &str) -> (Self, Self) {
        let (mut normal, mut important) = (Self::default(), Self::default());
        for item in split_top(text, ';') {
            let Some((name, value)) = item.split_once(':') else {
                continue;
            };
            let raw = name.trim();
            let value = value.trim();
            let lower = value.to_ascii_lowercase();
            let (value, strong) = match lower.find("!important") {
                Some(at) => (value.get(..at).unwrap_or_default().trim(), true),
                None => (value, false),
            };
            let target = if strong { &mut important } else { &mut normal };
            if value.len() > calc::MAX_VALUE {
                continue;
            }
            // Custom property names keep their case.
            if raw.starts_with("--") {
                if target.custom.len() < MAX_CUSTOM {
                    upsert(&mut target.custom, raw, value);
                }
                continue;
            }
            let name = raw.to_ascii_lowercase();
            if calc::has_var(value) {
                upsert(&mut target.deferred, &name, value);
            } else {
                target.deferred.retain(|(n, _)| *n != name);
                target.set(&name, value);
            }
        }
        (normal, important)
    }

    fn set(&mut self, name: &str, value: &str) {
        let name = extra::strip_vendor(name);
        let v = value.to_ascii_lowercase();
        let v = v.as_str();
        if matches!(
            v,
            "inherit" | "initial" | "unset" | "revert" | "revert-layer"
        ) {
            return self.keyword(name, v);
        }
        match name {
            "font-style" => self.italic = italic(v).or(self.italic),
            "font-weight" => self.weight = weight(v).or(self.weight),
            "text-align" => self.align = text_align(v).or(self.align),
            "display" => self.display = Some(display(v)),
            "width" => self.width = len(v).or(self.width),
            "height" => self.height = len(v).or(self.height),
            "font-size" => self.size = font_size(v).or(self.size),
            "line-height" => self.line = line_height(v).or(self.line),
            "letter-spacing" => self.letter = spacing(v).or(self.letter),
            "word-spacing" => self.word = spacing(v).or(self.word),
            "text-indent" => self.indent = len(v).or(self.indent),
            "font-family" => self.family = Some(families(value)),
            "font" => self.font(value),
            "list-style" => self.list_style_shorthand(value),
            "list-style-type" => {
                if let Some(style) = generated::list_style(value) {
                    self.unmarked = Some(style == ListStyle::None);
                    self.list_style = Some(style);
                }
            }
            "list-style-image" => self.list_image = Some(generated::url(value).map(Arc::from)),
            "content" => self.content = generated::content(value).or(self.content.take()),
            "counter-reset" => {
                self.counter_reset = generated::counters(value, 0).or(self.counter_reset.take());
            }
            "counter-increment" => {
                self.counter_increment =
                    generated::counters(value, 1).or(self.counter_increment.take());
            }
            "counter-set" => {
                self.counter_set = generated::counters(value, 0).or(self.counter_set.take());
            }
            "quotes" => self.quotes = generated::quotes(value).or(self.quotes.take()),
            "margin" => sides(&mut self.margin, v, len),
            "padding" => sides(&mut self.padding, v, len),
            "border-width" => sides(&mut self.border_width, v, border_width),
            "border-style" => sides(&mut self.border_style, v, border_style),
            "border-color" => {
                sides(&mut self.border_colour, v, |w| Some(colour(w).flatten()));
                sides(&mut self.border_clear, v, |w| Some(transparent(w)));
            }
            "border" => self.border([true; 4], v),
            "background-color" => {
                self.background = Some(!clear(v));
                self.fill = colour(v).or(self.fill);
                if let Some(rgba) = extra::parse_rgba(v) {
                    self.extra_mut().fill_rgba = Some(rgba);
                }
            }
            "color" => self.ink = colour(v).flatten().or(self.ink),
            "background-image" => self.set_layers(decor::layers(value)),
            "background-size" => self.picture_size = picture_size(v).or(self.picture_size),
            "background-position" => self.picture_at = Some(position(v)),
            "background-repeat" => {
                self.picture_repeat = Some(!v.starts_with("no-repeat"));
                if let Some(rep) = extra::parse_repeat(v) {
                    self.extra_mut().repeat_axis = Some(rep);
                }
            }
            "background" => self.background_shorthand(value),
            "box-shadow" => self.shadows = Some(decor::shadows(v)),
            "text-shadow" => self.text_shadow = Some(decor::shadows(v).first().cloned()),
            "opacity" => self.opacity = opacity(v).or(self.opacity),
            "visibility" => {
                self.visible = match v {
                    "hidden" | "collapse" => Some(false),
                    "visible" => Some(true),
                    _ => self.visible,
                }
            }
            "filter" => self.filter(v),
            "outline" => self.outline(v),
            "outline-width" => self.outline_width = border_width(v).or(self.outline_width),
            "outline-style" => self.outline_style = outline_style(v).or(self.outline_style),
            "outline-color" => self.outline_colour = colour(v).or(self.outline_colour),
            "outline-offset" => self.outline_offset = len(v).or(self.outline_offset),
            "border-image" => self.border_image(value),
            "border-image-source" => self.frame = Some(generated::url(value).map(Arc::from)),
            "border-image-slice" => self.frame_slice = decor::slice(v).or(self.frame_slice),
            "border-image-width" => self.frame_width = decor::spans(v).or(self.frame_width),
            "border-image-outset" => self.frame_outset = decor::spans(v).or(self.frame_outset),
            "border-image-repeat" => self.frame_tile = decor::tiles(v).or(self.frame_tile),
            "position" => {
                self.placing = match v {
                    "relative" | "sticky" => Some(Placing::Relative),
                    "absolute" => Some(Placing::Absolute),
                    "fixed" => Some(Placing::Fixed),
                    "static" => Some(Placing::Static),
                    _ => self.placing,
                }
            }
            "top" => set_side(&mut self.offsets, 0, len(v)),
            "right" => set_side(&mut self.offsets, 1, len(v)),
            "bottom" => set_side(&mut self.offsets, 2, len(v)),
            "left" => set_side(&mut self.offsets, 3, len(v)),
            "inset" => sides(&mut self.offsets, v, len),
            "z-index" => {
                self.z_index = match v {
                    "auto" => Some(0),
                    _ => v.parse::<i32>().ok().or(self.z_index),
                }
            }
            "float" => {
                self.float = match v {
                    "left" | "inline-start" => Some(Float::Left),
                    "right" | "inline-end" => Some(Float::Right),
                    _ => Some(Float::None),
                }
            }
            "vertical-align" => {
                self.valign = match v {
                    "top" | "text-top" | "baseline" => Some(VAlign::Top),
                    "middle" => Some(VAlign::Middle),
                    "bottom" | "text-bottom" => Some(VAlign::Bottom),
                    _ => self.valign,
                };
                self.lift = lift(v).or(self.lift);
                self.script = Some(match v {
                    "super" => Script::Super,
                    "sub" => Script::Sub,
                    _ => Script::Normal,
                });
            }
            "font-variant" => self.font_variant(v),
            "font-variant-caps" => self.small_caps = caps(v).or(self.small_caps),
            "font-variant-numeric" => self.numeric = group(v, numeric_words).or(self.numeric),
            "font-variant-ligatures" => {
                self.ligatures = group(v, ligature_words).or(self.ligatures)
            }
            "font-variant-position" => self.position = group(v, position_words).or(self.position),
            "font-kerning" => self.kerning = kerning(v).or(self.kerning),
            "font-feature-settings" => self.settings = feature_settings(v).or(self.settings),
            "text-align-last" => self.last = last_line(v).or(self.last),
            "direction" => {
                self.dir = match v {
                    "rtl" => Some(Dir::Rtl),
                    "ltr" => Some(Dir::Ltr),
                    _ => self.dir,
                }
            }
            "writing-mode" => self.writing = writing_words(v).or(self.writing),
            "text-orientation" => self.orient = orient_words(v).or(self.orient),
            "text-combine-upright" | "text-combine" | "text-combine-horizontal" => {
                self.combine = combine_words(v).or(self.combine)
            }
            "orphans" => self.orphans = lines_kept(v).or(self.orphans),
            "widows" => self.widows = lines_kept(v).or(self.widows),
            "break-inside" | "page-break-inside" => {
                self.whole = Some(matches!(v, "avoid" | "avoid-page"));
            }
            "text-transform" => {
                self.transform = match v {
                    "uppercase" => Some(Case::Upper),
                    "lowercase" => Some(Case::Lower),
                    "none" => Some(Case::Normal),
                    _ => self.transform,
                }
            }
            "text-decoration" | "text-decoration-line" => {
                let has = |w: &str| v.split_ascii_whitespace().any(|x| x == w);
                self.decoration = Some((has("underline"), has("line-through")));
            }
            "flex-direction" => self.flex_column = Some(v.starts_with("column")),
            "flex-wrap" => self.wrap = Some(v.starts_with("wrap")),
            "flex-flow" => hover::apply_flex_flow(self, v),
            "justify-content" => self.justify = justify(v).or(self.justify),
            "align-items" => {
                self.align_items = flex_align(v).or(self.align_items);
                self.items_stretch = stretch(v).or(self.items_stretch);
            }
            "align-self" => {
                self.align_self = flex_align(v).or(self.align_self);
                self.self_stretch = stretch(v).or(self.self_stretch);
            }
            "gap" | "grid-gap" => {
                let mut words = words(v).into_iter();
                let first = words.next().unwrap_or_default();
                let last = words.last().unwrap_or(first);
                self.row_gap = len(first).or(self.row_gap);
                self.gap = len(last).or(self.gap);
            }
            "column-gap" | "grid-column-gap" => hover::apply_column_gap(self, len(v)),
            "row-gap" | "grid-row-gap" => self.row_gap = len(v).or(self.row_gap),
            "flex-grow" => self.grow = v.parse::<f32>().ok().or(self.grow),
            "flex-basis" => self.basis = len(v).or(self.basis),
            "flex" => self.flex(v),
            "border-radius" => {
                let first = split_top(v, '/').into_iter().next().unwrap_or_default();
                let first = words(first).into_iter().next().unwrap_or_default();
                self.radius = len(first).or(self.radius);
            }
            "transform" => hover::apply_transform_decl(self, value, v),
            "page-break-before" | "break-before" => {
                self.break_before = Some(page_break(v));
                self.keep_last = Some(matches!(v, "avoid" | "avoid-page"));
            }
            "page-break-after" | "break-after" => {
                self.break_after = Some(page_break(v));
                self.keep_next = Some(matches!(v, "avoid" | "avoid-page"));
            }
            _ if !extra::apply_extra(self, name, value, v) => self.side(name, v),
            _ => {}
        }
    }

    /// `margin-top`, `border-left-width` and the like.
    fn side(&mut self, name: &str, v: &str) {
        let (prop, rest) = name.split_once('-').unwrap_or((name, ""));
        let (side, part) = rest.split_once('-').unwrap_or((rest, ""));
        let Some(i) = ["top", "right", "bottom", "left"]
            .iter()
            .position(|s| *s == side)
        else {
            return;
        };
        match (prop, part) {
            ("margin", "") => set_side(&mut self.margin, i, len(v)),
            ("padding", "") => set_side(&mut self.padding, i, len(v)),
            ("border", "") => {
                let mut only = [false; 4];
                if let Some(s) = only.get_mut(i) {
                    *s = true;
                }
                self.border(only, v);
            }
            ("border", "width") => set_side(&mut self.border_width, i, border_width(v)),
            ("border", "style") => set_side(&mut self.border_style, i, border_style(v)),
            ("border", "color") => {
                set_side(&mut self.border_colour, i, Some(colour(v).flatten()));
                set_side(&mut self.border_clear, i, Some(transparent(v)));
            }
            _ => {}
        }
    }

    /// The `background` shorthand; what it leaves unsaid resets.
    fn background_shorthand(&mut self, value: &str) {
        let parts = split_top(value, ',');
        self.set_layers(decor::layers(value));
        // Keywords, not pictures; the colour is in the last layer.
        let plain = |part: Option<&&str>| {
            let part = part.copied().unwrap_or_default().to_ascii_lowercase();
            let kept: Vec<&str> = words(&part)
                .into_iter()
                .filter(|w| !decor::is_image(w))
                .collect();
            kept.join(" ")
        };
        let (rest, last) = (plain(parts.first()), plain(parts.last()));
        let fill = words(&last).iter().find_map(|w| colour(w)).flatten();
        self.fill = Some(fill);
        let named = words(&last).iter().any(|w| is_colour(w) && !clear(w));
        self.background = Some(fill.is_some() || named);
        let (at, size) = rest.split_once('/').unwrap_or((&rest, ""));
        let words: Vec<&str> = at.split_ascii_whitespace().collect();
        self.picture_repeat = Some(!words.contains(&"no-repeat"));
        extra::record_background_shorthand(self, &last, &words);
        let placed: Vec<&str> = words.iter().copied().filter(|w| is_place(w)).collect();
        self.picture_at = Some(position(&placed.join(" ")));
        let size = size
            .split_ascii_whitespace()
            .take(2)
            .collect::<Vec<_>>()
            .join(" ");
        self.picture_size = Some(picture_size(&size).unwrap_or_default());
    }

    /// Background layers: a lone picture, else the list.
    fn set_layers(&mut self, layers: Vec<decor::Layer>) {
        let single = match layers.as_slice() {
            [decor::Layer::Url(url)] => Some(Arc::from(url.as_str())),
            _ => None,
        };
        self.background_image = Some(!layers.is_empty());
        self.layers = Some(match single {
            Some(_) => Arc::from([]),
            None => layers.into(),
        });
        self.picture = Some(single);
    }

    /// `filter`: its opacity and drop shadow; colour filters are the theme's.
    fn filter(&mut self, v: &str) {
        let (mut fade, mut drop) = (None, Vec::new());
        for f in words(v) {
            let Some((name, rest)) = f.split_once('(') else {
                continue;
            };
            let args = rest.trim_end_matches(')');
            match name {
                "opacity" => fade = opacity(args),
                "drop-shadow" => drop.extend(decor::shadows(args).iter().cloned()),
                _ => {}
            }
        }
        self.filter_opacity = Some(fade.unwrap_or(1.0));
        self.drop = Some(drop.into());
    }

    /// The `outline` shorthand; unsaid parts reset.
    fn outline(&mut self, v: &str) {
        let words = words(v);
        self.outline_width = Some(
            words
                .iter()
                .find_map(|w| border_width(w))
                .unwrap_or(Len::Px(3.0)),
        );
        self.outline_style = Some(
            words
                .iter()
                .find_map(|w| outline_style(w))
                .unwrap_or_default(),
        );
        self.outline_colour = Some(words.iter().find_map(|w| colour(w)).flatten());
    }

    /// The `border-image` shorthand; unsaid parts reset.
    fn border_image(&mut self, value: &str) {
        let b = decor::border_image(value);
        self.frame = Some(b.source.map(Arc::from));
        self.frame_slice = Some(b.slice);
        self.frame_width = Some(b.width.unwrap_or([Span::Times(1.0); 4]));
        self.frame_outset = Some(b.outset);
        self.frame_tile = Some(b.tile);
    }

    /// The `flex` shorthand: grow, shrink and basis.
    fn flex(&mut self, v: &str) {
        let (grow, basis) = match v {
            "none" => (0.0, None),
            "auto" => (1.0, None),
            "initial" => (0.0, None),
            v => {
                let words = words(v);
                let grow = words.first().and_then(|w| w.parse::<f32>().ok());
                let basis = words.iter().skip(1).find_map(|w| len(w));
                // `flex: 1` sets a zero basis; growth shares the row.
                let basis = basis.or(grow.map(|_| Len::Px(0.0)));
                (grow.unwrap_or(0.0), basis)
            }
        };
        self.grow = Some(grow);
        self.basis = basis.or(self.basis);
    }

    /// Resolves its pictures against `path`, where it was read.
    pub(crate) fn resolve(&mut self, path: &str) {
        if !self.deferred.is_empty() {
            self.from = Some(Arc::from(path));
        }
        if let Some(Some(raw)) = &self.picture {
            self.picture = Some(generated::resolve(path, raw).map(Arc::from));
        }
        if let Some(Some(raw)) = &self.frame {
            self.frame = Some(generated::resolve(path, raw).map(Arc::from));
        }
        if let Some(layers) = &self.layers {
            let resolved = layers.iter().filter_map(|l| match l {
                decor::Layer::Url(raw) => generated::resolve(path, raw).map(decor::Layer::Url),
                other => Some(other.clone()),
            });
            self.layers = Some(resolved.collect());
        }
        if let Some(content) = &mut self.content {
            content.resolve(path);
        }
        if let Some(Some(raw)) = &self.list_image {
            self.list_image = Some(generated::resolve(path, raw).map(Arc::from));
        }
    }

    /// `list-style: square inside url(dot.png)`; a lone `none` drops both.
    fn list_style_shorthand(&mut self, value: &str) {
        let image = generated::url(value);
        let words = value.split_ascii_whitespace();
        let mut styles = words
            .filter(|w| !w.to_ascii_lowercase().contains("url("))
            .filter_map(generated::list_style);
        let style = styles.find(|s| *s != ListStyle::None || image.is_none());
        self.unmarked = Some(style == Some(ListStyle::None));
        if let Some(style) = style {
            self.list_style = Some(style);
        }
        self.list_image = Some(image.map(Arc::from));
    }

    /// The `border` shorthand on the sides in `which`; unsaid parts reset.
    fn border(&mut self, which: [bool; 4], v: &str) {
        let words = words(v);
        let width = words
            .iter()
            .find_map(|w| border_width(w))
            .unwrap_or(Len::Px(3.0));
        let style = words
            .iter()
            .find_map(|w| border_style(w))
            .unwrap_or_default();
        // Unsaid, the colour is the text's.
        let ink = words.iter().find_map(|w| colour(w)).flatten();
        let see_through = words.iter().any(|w| transparent(w));
        for (i, on) in which.into_iter().enumerate() {
            if on {
                set_side(&mut self.border_width, i, Some(width));
                set_side(&mut self.border_style, i, Some(style));
                set_side(&mut self.border_colour, i, Some(ink));
                set_side(&mut self.border_clear, i, Some(see_through));
            }
        }
    }

    /// `font: italic bold 12px/1.5 serif`: style, weight, size, family.
    fn font(&mut self, value: &str) {
        let lower = value.to_ascii_lowercase();
        let words: Vec<&str> = lower.split_ascii_whitespace().collect();
        let Some(at) = words.iter().position(|w| {
            let size = w.split('/').next().unwrap_or_default();
            font_size(size).is_some()
        }) else {
            return;
        };
        // The shorthand resets the variants and kerning it can't set.
        self.small_caps = Some(Caps::Normal);
        let normal = Some(Features::default());
        (self.numeric, self.ligatures, self.position, self.kerning) =
            (normal, normal, normal, normal);
        for w in words.iter().take(at) {
            self.italic = italic(w).or(self.italic);
            self.weight = weight(w).or(self.weight);
            if *w == "small-caps" {
                self.small_caps = Some(Caps::Small);
            }
        }
        let mut sized = words.get(at).unwrap_or(&"").split('/');
        self.size = sized.next().and_then(font_size).or(self.size);
        self.line = sized.next().and_then(line_height).or(self.line);
        // The family keeps its case: names match faces.
        let rest: Vec<&str> = value.split_ascii_whitespace().skip(at + 1).collect();
        if !rest.is_empty() {
            self.family = Some(families(&rest.join(" ")));
        }
    }

    /// `font-variant`: caps and each group it names; the rest normal.
    fn font_variant(&mut self, v: &str) {
        let mut caps_set = Caps::Normal;
        let mut sets = [Features::default(); 3];
        if v == "none" {
            sets[1] = ligature_words("none").unwrap_or_default();
        } else if v != "normal" {
            for w in v.split_ascii_whitespace() {
                if let Some(c) = caps(w) {
                    caps_set = c;
                }
                for (set, read) in
                    sets.iter_mut()
                        .zip([numeric_words, ligature_words, position_words])
                {
                    if let Some(f) = read(w) {
                        *set = set.then(f);
                    }
                }
            }
        }
        self.small_caps = Some(caps_set);
        let [n, l, p] = sets;
        (self.numeric, self.ligatures, self.position) = (Some(n), Some(l), Some(p));
    }

    /// Sets each property `top` sets, as a later rule does.
    pub(crate) fn layer(&mut self, top: &Self) {
        self.layer_set(top);
        for (name, value) in &top.custom {
            upsert(&mut self.custom, name, value);
        }
        for (name, value) in &top.deferred {
            upsert(&mut self.deferred, name, value);
        }
        if top.from.is_some() {
            self.from.clone_from(&top.from);
        }
    }

    /// What `top` sets and resets, its custom properties aside.
    fn layer_set(&mut self, top: &Self) {
        if let Some(cleared) = &top.cleared {
            self.clear(cleared);
        }
        each_field!(layer, self, top);
        extra::layer_extra(&mut self.extra, &top.extra);
        if top.vars.is_some() {
            self.vars.clone_from(&top.vars);
        }
    }

    /// Unsets each property `probe` sets, and keeps it to unset below.
    fn clear(&mut self, probe: &Self) {
        each_field!(unlayer, self, probe);
        let mut all = self.cleared.as_deref().cloned().unwrap_or_default();
        each_field!(layer, all, probe);
        self.cleared = Some(Arc::new(all));
    }

    /// `inherit`, `initial`, `unset` and `revert` for property `name`.
    fn keyword(&mut self, name: &str, v: &str) {
        let Some((literal, initial, inherited)) = initial_value(name) else {
            return;
        };
        // Reverted, `display` is the default sheet's, which stays set.
        if v.starts_with("revert") && name == "display" {
            return;
        }
        let set = match v {
            "initial" => initial,
            "unset" => initial && !inherited,
            "inherit" if !inherited => return,
            _ => false,
        };
        if set {
            return self.set(name, literal);
        }
        let mut probe = Self::default();
        probe.set(name, literal);
        self.clear(&probe);
    }

    fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

fn set_side<T: Copy>(sides: &mut [Option<T>; 4], i: usize, v: Option<T>) {
    if let (Some(slot), Some(v)) = (sides.get_mut(i), v) {
        *slot = Some(v);
    }
}

/// One to four values, as CSS spreads them over the sides.
fn sides<T: Copy>(out: &mut [Option<T>; 4], v: &str, read: impl Fn(&str) -> Option<T>) {
    let values: Vec<T> = words(v).into_iter().filter_map(read).collect();
    let pick = |order: [usize; 4]| order.map(|i| values.get(i).copied());
    let spread = match values.len() {
        1 => pick([0, 0, 0, 0]),
        2 => pick([0, 1, 0, 1]),
        3 => pick([0, 1, 2, 1]),
        4 => pick([0, 1, 2, 3]),
        _ => return,
    };
    *out = spread;
}

/// Where the bracket open before `text` closes.
fn closing_bracket(text: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in text.char_indices() {
        match c {
            '(' => depth += 1,
            ')' if depth == 0 => return Some(i),
            ')' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// A `font-variant-caps` value; titling and unicase show as normal.
fn caps(v: &str) -> Option<Caps> {
    Some(match v {
        "normal" | "unicase" | "titling-caps" => Caps::Normal,
        "small-caps" | "petite-caps" => Caps::Small,
        "all-small-caps" | "all-petite-caps" => Caps::AllSmall,
        _ => return None,
    })
}

/// Features `tags` all turned on, or all off.
fn tagged(tags: &[&str], on: bool) -> Option<Features> {
    joined(tags.iter().map(|t| Features::of(t, on)))
}

/// A `font-variant-*` group read word by word; one unknown word voids it.
fn group(v: &str, read: fn(&str) -> Option<Features>) -> Option<Features> {
    if v == "normal" {
        return Some(Features::default());
    }
    joined(v.split_ascii_whitespace().map(read))
}

/// Features joined in order, later over earlier; none if one is unknown.
fn joined(each: impl IntoIterator<Item = Option<Features>>) -> Option<Features> {
    each.into_iter()
        .try_fold(Features::default(), |all, f| Some(all.then(f?)))
}

fn numeric_words(w: &str) -> Option<Features> {
    let tag = match w {
        "lining-nums" => "lnum",
        "oldstyle-nums" => "onum",
        "proportional-nums" => "pnum",
        "tabular-nums" => "tnum",
        "diagonal-fractions" => "frac",
        "stacked-fractions" => "afrc",
        "ordinal" => "ordn",
        "slashed-zero" => "zero",
        _ => return None,
    };
    Features::of(tag, true)
}

fn ligature_words(w: &str) -> Option<Features> {
    let (tags, on): (&[&str], bool) = match w {
        "none" => (&["liga", "clig", "dlig", "hlig", "calt"], false),
        "common-ligatures" => (&["liga", "clig"], true),
        "no-common-ligatures" => (&["liga", "clig"], false),
        "discretionary-ligatures" => (&["dlig"], true),
        "no-discretionary-ligatures" => (&["dlig"], false),
        "historical-ligatures" => (&["hlig"], true),
        "no-historical-ligatures" => (&["hlig"], false),
        "contextual" => (&["calt"], true),
        "no-contextual" => (&["calt"], false),
        _ => return None,
    };
    tagged(tags, on)
}

fn position_words(w: &str) -> Option<Features> {
    match w {
        "sub" => Features::of("subs", true),
        "super" => Features::of("sups", true),
        _ => None,
    }
}

fn kerning(v: &str) -> Option<Features> {
    match v {
        "auto" => Some(Features::default()),
        "normal" => Features::of("kern", true),
        "none" => Features::of("kern", false),
        _ => None,
    }
}

/// `font-feature-settings`; tags the shaper isn't asked for are skipped.
fn feature_settings(v: &str) -> Option<Features> {
    if v == "normal" {
        return Some(Features::default());
    }
    let mut out = Features::default();
    for item in v.split(',') {
        let mut parts = item.split_ascii_whitespace();
        let tag = parts.next()?;
        let tag = tag
            .strip_prefix(['"', '\''])
            .and_then(|t| t.strip_suffix(['"', '\'']))?;
        if tag.len() != 4 {
            return None;
        }
        let on = match parts.next() {
            None | Some("on") => true,
            Some("off") => false,
            Some(n) => n.parse::<u32>().ok()? > 0,
        };
        if let Some(f) = Features::of(tag, on) {
            out = out.then(f);
        }
    }
    Some(out)
}

fn last_line(v: &str) -> Option<Option<LastLine>> {
    Some(match v {
        "auto" => None,
        "start" | "left" => Some(LastLine::Start),
        "center" => Some(LastLine::Center),
        "end" | "right" => Some(LastLine::End),
        "justify" => Some(LastLine::Justify),
        _ => return None,
    })
}

/// A `widows` or `orphans` count, at least 1.
fn lines_kept(v: &str) -> Option<u8> {
    v.parse::<u8>().ok().filter(|n| *n > 0)
}

/// Sets `name` in a list of declarations, moving it last.
fn upsert(list: &mut Vec<(String, String)>, name: &str, value: &str) {
    list.retain(|(n, _)| n != name);
    list.push((name.to_owned(), value.to_owned()));
}

/// `writing-mode`, old and new: whether lines run down the page.
fn writing_words(v: &str) -> Option<bool> {
    match v {
        "horizontal-tb" | "lr" | "lr-tb" | "rl" | "rl-tb" => Some(false),
        "vertical-rl" | "vertical-lr" | "tb" | "tb-rl" | "tb-lr" | "sideways-rl"
        | "sideways-lr" => Some(true),
        _ => None,
    }
}

/// `text-orientation`, with the older names.
fn orient_words(v: &str) -> Option<Orient> {
    match v {
        "mixed" | "vertical-right" => Some(Orient::Mixed),
        "upright" => Some(Orient::Upright),
        "sideways" | "sideways-right" | "rotate-right" => Some(Orient::Sideways),
        _ => None,
    }
}

/// `text-combine-upright`, and the older `horizontal`.
fn combine_words(v: &str) -> Option<bool> {
    match v.split_ascii_whitespace().next()? {
        "none" => Some(false),
        "all" | "horizontal" | "digits" => Some(true),
        _ => None,
    }
}

/// Whether `@supports` finds a declaration this cascade reads.
pub(crate) fn understood(name: &str, value: &str) -> bool {
    let name = name.trim();
    if name.starts_with("--") {
        return true;
    }
    let (normal, _) = Decl::parse_both(&format!("{name}: {value}"));
    !normal.is_empty() && normal.display != Some(Display::Other)
}

/// Whether `@supports selector()` finds a selector this cascade reads.
pub(crate) fn selector_understood(text: &str) -> bool {
    parse_selector(text.trim()).is_some_and(|s| s.parts.iter().all(|(c, _)| c.known()))
}

/// A value's words, a bracket's spaces kept, as in `rgb(1, 2, 3)`.
pub(crate) fn words(v: &str) -> Vec<&str> {
    split_top(v, ' ')
        .into_iter()
        .flat_map(|w| split_top(w, '\t'))
        .map(str::trim)
        .filter(|w| !w.is_empty())
        .collect()
}

/// `items` split at `sep`, but not inside brackets or quotes.
pub(crate) fn split_top(text: &str, sep: char) -> Vec<&str> {
    let (mut out, mut depth, mut quote, mut start) = (Vec::new(), 0i32, None, 0);
    for (i, c) in text.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(' | '[') => depth += 1,
            (None, ')' | ']') => depth -= 1,
            (None, c) if c == sep && depth <= 0 => {
                out.push(text.get(start..i).unwrap_or_default());
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push(text.get(start..).unwrap_or_default());
    out
}

fn italic(v: &str) -> Option<bool> {
    match v {
        "italic" | "oblique" => Some(true),
        "normal" => Some(false),
        _ => None,
    }
}

fn weight(v: &str) -> Option<Weight> {
    match v {
        "bold" => Some(Weight::Abs(700)),
        "normal" => Some(Weight::Abs(400)),
        "bolder" => Some(Weight::Bolder),
        "lighter" => Some(Weight::Lighter),
        _ => (v.parse::<u16>().ok())
            .filter(|w| (1..=1000).contains(w))
            .map(Weight::Abs),
    }
}

fn text_align(v: &str) -> Option<TextAlign> {
    match v.trim() {
        "center" | "-webkit-center" => Some(TextAlign::Center),
        "end" => Some(TextAlign::End),
        "start" => Some(TextAlign::Start),
        "right" => Some(TextAlign::Right),
        "left" => Some(TextAlign::Left),
        "justify" => Some(TextAlign::Justify),
        _ => None,
    }
}

fn display(v: &str) -> Display {
    match v.split_ascii_whitespace().last().unwrap_or_default() {
        "none" => Display::None,
        "block" | "list-item" | "flow-root" => Display::Block,
        "inline" | "inline-block" | "contents" => Display::Inline,
        "flex" | "inline-flex" | "-webkit-box" | "-webkit-flex" => Display::Flex,
        "grid" | "inline-grid" => Display::Grid,
        "table" | "inline-table" => Display::Table,
        _ => Display::Other,
    }
}

fn font_size(v: &str) -> Option<FontSize> {
    let px = |n: f32| Some(FontSize::Len(Len::Px(n)));
    match v {
        "xx-small" => px(9.0),
        "x-small" => px(10.0),
        "small" => px(13.0),
        "medium" => px(16.0),
        "large" => px(18.0),
        "x-large" => px(24.0),
        "xx-large" => px(32.0),
        "xxx-large" => px(48.0),
        "larger" => Some(FontSize::Larger),
        "smaller" => Some(FontSize::Smaller),
        _ => len(v)
            .filter(|l| !matches!(l, Len::Auto))
            .map(FontSize::Len),
    }
}

fn line_height(v: &str) -> Option<LineHeight> {
    if v == "normal" {
        return Some(LineHeight::Normal);
    }
    let number = match calc::maths(v) {
        Some(sum) if sum.is_number() => Some(sum.n),
        _ => v.parse::<f32>().ok(),
    };
    if let Some(n) = number {
        return (n.is_finite() && n >= 0.0).then_some(LineHeight::Factor(n.min(10.0)));
    }
    len(v)
        .filter(|l| !matches!(l, Len::Auto))
        .map(LineHeight::Len)
}

fn spacing(v: &str) -> Option<Len> {
    match v {
        "normal" => Some(Len::Px(0.0)),
        _ => len(v).filter(|l| !matches!(l, Len::Auto | Len::Percent(_))),
    }
}

pub(crate) fn border_width(v: &str) -> Option<Len> {
    match v {
        "thin" => Some(Len::Px(1.0)),
        "medium" => Some(Len::Px(3.0)),
        "thick" => Some(Len::Px(5.0)),
        _ => len(v).filter(|l| !matches!(l, Len::Auto | Len::Percent(_))),
    }
}

pub(crate) fn border_style(v: &str) -> Option<BorderStyle> {
    match v {
        "none" | "hidden" => Some(BorderStyle::None),
        "solid" => Some(BorderStyle::Solid),
        "groove" => Some(BorderStyle::Groove),
        "ridge" => Some(BorderStyle::Ridge),
        "inset" => Some(BorderStyle::Inset),
        "outset" => Some(BorderStyle::Outset),
        "double" => Some(BorderStyle::Double),
        "dashed" => Some(BorderStyle::Dashed),
        "dotted" => Some(BorderStyle::Dotted),
        _ => None,
    }
}

/// No paint: `transparent`, `none` or a fully clear colour.
pub(crate) fn clear(v: &str) -> bool {
    matches!(v, "transparent" | "none" | "initial") || v.replace(' ', "").ends_with(",0)")
}

/// A colour that shows nothing: `transparent` or a zero alpha.
fn transparent(w: &str) -> bool {
    w == "transparent"
        || (w.contains('(') && clear(w))
        || decor::tint(w).is_some_and(|t| t.1 < 0.01)
}

/// `opacity`: a number or a percentage, 0 to 1.
fn opacity(v: &str) -> Option<f32> {
    let n = match v.strip_suffix('%') {
        Some(p) => p.trim().parse::<f32>().ok()? / 100.0,
        None => v.trim().parse::<f32>().ok()?,
    };
    n.is_finite().then(|| n.clamp(0.0, 1.0))
}

/// `outline-style`; `auto` draws a plain line.
fn outline_style(v: &str) -> Option<BorderStyle> {
    match v {
        "auto" => Some(BorderStyle::Solid),
        v => border_style(v),
    }
}

/// `justify-content`.
fn justify(v: &str) -> Option<Justify> {
    let v = v.trim_start_matches("safe ").trim_start_matches("unsafe ");
    match v {
        "flex-start" | "start" | "left" | "normal" | "stretch" => Some(Justify::Start),
        "center" => Some(Justify::Center),
        "flex-end" | "end" | "right" => Some(Justify::End),
        "space-between" => Some(Justify::Between),
        "space-around" => Some(Justify::Around),
        "space-evenly" => Some(Justify::Evenly),
        _ => None,
    }
}

/// `vertical-align` for a picture in a line.
fn lift(v: &str) -> Option<Lift> {
    let keyword = match v {
        "baseline" => Rise::Baseline,
        "middle" => Rise::Middle,
        "text-top" => Rise::TextTop,
        "text-bottom" => Rise::TextBottom,
        "top" => Rise::Top,
        "bottom" => Rise::Bottom,
        "sub" => Rise::Sub,
        "super" => Rise::Super,
        _ => return len(v).filter(|l| *l != Len::Auto).map(Lift::Length),
    };
    Some(Lift::Keyword(keyword))
}

/// A lift in ems of text `size` px high; a percentage takes a line.
fn rise(lift: Lift, size: f32) -> Rise {
    let em = match lift {
        Lift::Keyword(r) => return r,
        Lift::Length(Len::Em(n)) => n,
        Lift::Length(Len::Percent(p)) => p / 100.0 * 1.2,
        Lift::Length(l) => match l.px(size, size) {
            Some(px) if size > 0.0 => px / size,
            _ => return Rise::Baseline,
        },
    };
    Rise::By(em.clamp(-8.0, 8.0))
}

/// Whether `align-items` or `align-self` stretches; none if not read.
fn stretch(v: &str) -> Option<bool> {
    match v {
        "stretch" | "normal" => Some(true),
        _ => flex_align(v).map(|_| false),
    }
}

/// `align-items` and `align-self` as a cell's vertical place.
fn flex_align(v: &str) -> Option<VAlign> {
    match v {
        "center" => Some(VAlign::Middle),
        "flex-end" | "end" | "self-end" => Some(VAlign::Bottom),
        "flex-start" | "start" | "self-start" | "stretch" | "normal" | "baseline" => {
            Some(VAlign::Top)
        }
        _ => None,
    }
}

/// The turn a `transform` makes, in degrees, from its `rotate()`.
fn rotation(v: &str) -> Option<f32> {
    let start = v.find("rotate(").or_else(|| v.find("rotatez("))?;
    let inner = v.get(start..)?;
    let inner = inner.get(inner.find('(')? + 1..)?;
    let inner = inner.get(..inner.find(')')?)?.trim();
    let number = |unit: &str| inner.strip_suffix(unit)?.trim().parse::<f32>().ok();
    let degrees = number("deg")
        .or_else(|| number("grad").map(|g| g * 0.9))
        .or_else(|| number("rad").map(f32::to_degrees))
        .or_else(|| number("turn").map(|t| t * 360.0))
        .or_else(|| (inner == "0").then_some(0.0))?;
    degrees.is_finite().then_some(degrees % 360.0)
}

/// Whether a page break is asked for, before or after.
fn page_break(v: &str) -> bool {
    ["always", "page", "left", "right", "recto", "verso"].contains(&v)
}

/// A word of `background-position`.
fn is_place(w: &str) -> bool {
    matches!(w, "left" | "right" | "top" | "bottom" | "center") || len(w).is_some()
}

/// `background-position` as shares across and down; lengths sit at 0.
pub(crate) fn position(v: &str) -> [f32; 2] {
    let mut at = [None, None];
    let mut loose = Vec::new();
    for w in v.split_ascii_whitespace().take(4) {
        match w {
            "left" => at[0] = Some(0.0),
            "right" => at[0] = Some(1.0),
            "top" => at[1] = Some(0.0),
            "bottom" => at[1] = Some(1.0),
            "center" => loose.push(0.5),
            w => loose.push(match len(w) {
                Some(Len::Percent(p)) => p / 100.0,
                _ => 0.0,
            }),
        }
    }
    let mut loose = loose.into_iter();
    for a in &mut at {
        if a.is_none() {
            *a = loose.next();
        }
    }
    [at[0].unwrap_or(0.5), at[1].unwrap_or(0.5)].map(|a| a.clamp(0.0, 1.0))
}

/// `background-size`: cover, contain, auto or one or two lengths.
fn picture_size(v: &str) -> Option<PictureSize> {
    match v.trim() {
        "" | "auto" | "auto auto" => Some(PictureSize::Auto),
        "cover" => Some(PictureSize::Cover),
        "contain" => Some(PictureSize::Contain),
        v => {
            let mut words = v.split_ascii_whitespace();
            let one = |w: Option<&str>| w.filter(|w| *w != "auto").and_then(len);
            let size = [one(words.next()), one(words.next())];
            size.iter()
                .any(Option::is_some)
                .then_some(PictureSize::Set(size))
        }
    }
}

/// A CSS colour; `Some(None)` is none, unknown words `None`.
pub(crate) fn colour(v: &str) -> Option<Option<Rgb>> {
    let v = v.trim();
    if clear(v) {
        return Some(None);
    }
    if let Some(hex) = v.strip_prefix('#') {
        let digit = |i: usize, n: usize| u8::from_str_radix(hex.get(i..i + n)?, 16).ok();
        let twice = |i: usize| digit(i, 1).map(|d| d * 17);
        return match hex.len() {
            3 | 4 => Some([twice(0)?, twice(1)?, twice(2)?]),
            6 | 8 => Some([digit(0, 2)?, digit(2, 2)?, digit(4, 2)?]),
            _ => None,
        }
        .map(Some);
    }
    if let Some(args) = v.strip_prefix("rgba(").or_else(|| v.strip_prefix("rgb(")) {
        let args = args.trim_end_matches(')').replace([',', '/'], " ");
        let mut parts = args
            .split_ascii_whitespace()
            .map(|p| match p.strip_suffix('%') {
                Some(p) => p.parse::<f32>().ok().map(|p| p * 2.55),
                None => p.parse::<f32>().ok(),
            });
        let mut rgb = [0; 3];
        for c in &mut rgb {
            *c = parts.next()??.round().clamp(0.0, 255.0) as u8;
        }
        return Some(Some(rgb));
    }
    if let Some(args) = v.strip_prefix("hsla(").or_else(|| v.strip_prefix("hsl(")) {
        return hsl(args).map(Some);
    }
    let at = NAMED.binary_search_by(|(name, _)| name.cmp(&v)).ok()?;
    let (_, hex) = NAMED.get(at)?;
    let [_, r, g, b] = hex.to_be_bytes();
    let rgb = [r, g, b];
    Some(Some(rgb))
}

/// `hsl()`'s arguments: hue in degrees, saturation and lightness in percent.
fn hsl(args: &str) -> Option<Rgb> {
    let args = args.trim_end_matches(')').replace([',', '/'], " ");
    let mut parts = args.split_ascii_whitespace();
    let hue = parts.next()?;
    let hue = match hue.strip_suffix("turn") {
        Some(t) => t.parse::<f32>().ok()? * 360.0,
        None => hue.trim_end_matches("deg").parse::<f32>().ok()?,
    };
    let mut pct = || -> Option<f32> {
        let p = parts.next()?.trim_end_matches('%').parse::<f32>().ok()?;
        Some((p / 100.0).clamp(0.0, 1.0))
    };
    let (sat, light) = (pct()?, pct()?);
    // The CSS Color 4 conversion.
    let a = sat * light.min(1.0 - light);
    let f = |n: f32| {
        let k = (n + hue.rem_euclid(360.0) / 30.0) % 12.0;
        let c = light - a * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0);
        (c * 255.0).round().clamp(0.0, 255.0) as u8
    };
    Some([f(0.0), f(8.0), f(4.0)])
}

/// CSS's named colours, sorted by name.
const NAMED: &[(&str, u32)] = &[
    ("aliceblue", 0xF0F8FF),
    ("antiquewhite", 0xFAEBD7),
    ("aqua", 0x00FFFF),
    ("aquamarine", 0x7FFFD4),
    ("azure", 0xF0FFFF),
    ("beige", 0xF5F5DC),
    ("bisque", 0xFFE4C4),
    ("black", 0x000000),
    ("blanchedalmond", 0xFFEBCD),
    ("blue", 0x0000FF),
    ("blueviolet", 0x8A2BE2),
    ("brown", 0xA52A2A),
    ("burlywood", 0xDEB887),
    ("cadetblue", 0x5F9EA0),
    ("chartreuse", 0x7FFF00),
    ("chocolate", 0xD2691E),
    ("coral", 0xFF7F50),
    ("cornflowerblue", 0x6495ED),
    ("cornsilk", 0xFFF8DC),
    ("crimson", 0xDC143C),
    ("cyan", 0x00FFFF),
    ("darkblue", 0x00008B),
    ("darkcyan", 0x008B8B),
    ("darkgoldenrod", 0xB8860B),
    ("darkgray", 0xA9A9A9),
    ("darkgreen", 0x006400),
    ("darkgrey", 0xA9A9A9),
    ("darkkhaki", 0xBDB76B),
    ("darkmagenta", 0x8B008B),
    ("darkolivegreen", 0x556B2F),
    ("darkorange", 0xFF8C00),
    ("darkorchid", 0x9932CC),
    ("darkred", 0x8B0000),
    ("darksalmon", 0xE9967A),
    ("darkseagreen", 0x8FBC8F),
    ("darkslateblue", 0x483D8B),
    ("darkslategray", 0x2F4F4F),
    ("darkslategrey", 0x2F4F4F),
    ("darkturquoise", 0x00CED1),
    ("darkviolet", 0x9400D3),
    ("deeppink", 0xFF1493),
    ("deepskyblue", 0x00BFFF),
    ("dimgray", 0x696969),
    ("dimgrey", 0x696969),
    ("dodgerblue", 0x1E90FF),
    ("firebrick", 0xB22222),
    ("floralwhite", 0xFFFAF0),
    ("forestgreen", 0x228B22),
    ("fuchsia", 0xFF00FF),
    ("gainsboro", 0xDCDCDC),
    ("ghostwhite", 0xF8F8FF),
    ("gold", 0xFFD700),
    ("goldenrod", 0xDAA520),
    ("gray", 0x808080),
    ("green", 0x008000),
    ("greenyellow", 0xADFF2F),
    ("grey", 0x808080),
    ("honeydew", 0xF0FFF0),
    ("hotpink", 0xFF69B4),
    ("indianred", 0xCD5C5C),
    ("indigo", 0x4B0082),
    ("ivory", 0xFFFFF0),
    ("khaki", 0xF0E68C),
    ("lavender", 0xE6E6FA),
    ("lavenderblush", 0xFFF0F5),
    ("lawngreen", 0x7CFC00),
    ("lemonchiffon", 0xFFFACD),
    ("lightblue", 0xADD8E6),
    ("lightcoral", 0xF08080),
    ("lightcyan", 0xE0FFFF),
    ("lightgoldenrodyellow", 0xFAFAD2),
    ("lightgray", 0xD3D3D3),
    ("lightgreen", 0x90EE90),
    ("lightgrey", 0xD3D3D3),
    ("lightpink", 0xFFB6C1),
    ("lightsalmon", 0xFFA07A),
    ("lightseagreen", 0x20B2AA),
    ("lightskyblue", 0x87CEFA),
    ("lightslategray", 0x778899),
    ("lightslategrey", 0x778899),
    ("lightsteelblue", 0xB0C4DE),
    ("lightyellow", 0xFFFFE0),
    ("lime", 0x00FF00),
    ("limegreen", 0x32CD32),
    ("linen", 0xFAF0E6),
    ("magenta", 0xFF00FF),
    ("maroon", 0x800000),
    ("mediumaquamarine", 0x66CDAA),
    ("mediumblue", 0x0000CD),
    ("mediumorchid", 0xBA55D3),
    ("mediumpurple", 0x9370DB),
    ("mediumseagreen", 0x3CB371),
    ("mediumslateblue", 0x7B68EE),
    ("mediumspringgreen", 0x00FA9A),
    ("mediumturquoise", 0x48D1CC),
    ("mediumvioletred", 0xC71585),
    ("midnightblue", 0x191970),
    ("mintcream", 0xF5FFFA),
    ("mistyrose", 0xFFE4E1),
    ("moccasin", 0xFFE4B5),
    ("navajowhite", 0xFFDEAD),
    ("navy", 0x000080),
    ("oldlace", 0xFDF5E6),
    ("olive", 0x808000),
    ("olivedrab", 0x6B8E23),
    ("orange", 0xFFA500),
    ("orangered", 0xFF4500),
    ("orchid", 0xDA70D6),
    ("palegoldenrod", 0xEEE8AA),
    ("palegreen", 0x98FB98),
    ("paleturquoise", 0xAFEEEE),
    ("palevioletred", 0xDB7093),
    ("papayawhip", 0xFFEFD5),
    ("peachpuff", 0xFFDAB9),
    ("peru", 0xCD853F),
    ("pink", 0xFFC0CB),
    ("plum", 0xDDA0DD),
    ("powderblue", 0xB0E0E6),
    ("purple", 0x800080),
    ("rebeccapurple", 0x663399),
    ("red", 0xFF0000),
    ("rosybrown", 0xBC8F8F),
    ("royalblue", 0x4169E1),
    ("saddlebrown", 0x8B4513),
    ("salmon", 0xFA8072),
    ("sandybrown", 0xF4A460),
    ("seagreen", 0x2E8B57),
    ("seashell", 0xFFF5EE),
    ("sienna", 0xA0522D),
    ("silver", 0xC0C0C0),
    ("skyblue", 0x87CEEB),
    ("slateblue", 0x6A5ACD),
    ("slategray", 0x708090),
    ("slategrey", 0x708090),
    ("snow", 0xFFFAFA),
    ("springgreen", 0x00FF7F),
    ("steelblue", 0x4682B4),
    ("tan", 0xD2B48C),
    ("teal", 0x008080),
    ("thistle", 0xD8BFD8),
    ("tomato", 0xFF6347),
    ("turquoise", 0x40E0D0),
    ("violet", 0xEE82EE),
    ("wheat", 0xF5DEB3),
    ("white", 0xFFFFFF),
    ("whitesmoke", 0xF5F5F5),
    ("yellow", 0xFFFF00),
    ("yellowgreen", 0x9ACD32),
];

/// A colour's relative luminance, 0 to 1.
pub(crate) fn luminance([r, g, b]: Rgb) -> f32 {
    let lin = |c: u8| {
        let c = f32::from(c) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

/// A colour among a `background` shorthand's words.
fn is_colour(w: &str) -> bool {
    const NOT: &[&str] = &[
        "none",
        "transparent",
        "repeat",
        "no-repeat",
        "repeat-x",
        "repeat-y",
        "space",
        "round",
        "center",
        "top",
        "left",
        "right",
        "bottom",
        "fixed",
        "scroll",
        "local",
        "cover",
        "contain",
        "auto",
        "border-box",
        "padding-box",
        "content-box",
    ];
    let named = w.chars().all(|c| c.is_ascii_alphabetic()) && !NOT.contains(&w);
    w.starts_with('#') || w.starts_with("rgb") || w.starts_with("hsl") || named
}

/// `"A", b, serif` as names, quotes gone, case kept.
fn families(v: &str) -> Arc<[String]> {
    split_top(v, ',')
        .iter()
        .map(|f| f.trim().trim_matches(['"', '\'']).trim().to_owned())
        .filter(|f| !f.is_empty())
        .take(8)
        .collect()
}

/// A font the book ships: its family as the book names it and files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FontFace {
    pub(crate) family: String,
    /// Archive paths, in the order given.
    pub(crate) src: Vec<String>,
    pub(crate) weight: u16,
    pub(crate) italic: bool,
}

/// What a compound's attribute test asks.
#[derive(Debug, Clone, PartialEq, Eq)]
enum AttrOp {
    Exists,
    Equals,
    /// `~=`: one of its words.
    Word,
    /// `|=`: equal, or that and a hyphen.
    Dash,
    Prefix,
    Suffix,
    Contains,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AttrTest {
    name: String,
    op: AttrOp,
    value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Pseudo {
    /// `an+b` among element siblings; of its type with `typed`.
    Nth {
        a: i32,
        b: i32,
        typed: bool,
    },
    Root,
    Link,
    Hover,
    Not(Box<Compound>),
    /// Needs what the stream hasn't read yet, or user action.
    Never,
}

/// A box CSS generates for an element, as `::before`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PseudoEl {
    Before,
    After,
    FirstLetter,
    FirstLine,
    Marker,
}

impl PseudoEl {
    fn named(name: &str, legacy: bool) -> Option<Self> {
        match name {
            "before" => Some(Self::Before),
            "after" => Some(Self::After),
            "first-letter" => Some(Self::FirstLetter),
            "first-line" => Some(Self::FirstLine),
            "marker" if !legacy => Some(Self::Marker),
            _ => None,
        }
    }

    fn bit(self) -> u8 {
        1 << self as u8
    }
}

/// One compound selector: a tag, id, classes, attributes, pseudo-classes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Compound {
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
    attrs: Vec<AttrTest>,
    pseudo: Vec<Pseudo>,
    /// Only on the subject; the rule styles that box.
    element: Option<PseudoEl>,
}

impl Compound {
    /// Whether every pseudo-class is one this cascade can match.
    fn known(&self) -> bool {
        self.pseudo.iter().all(|p| match p {
            Pseudo::Never => false,
            Pseudo::Not(inner) => inner.known() && !inner.has_hover(),
            _ => true,
        })
    }

    fn specificity(&self) -> u32 {
        let not: u32 = (self.pseudo.iter())
            .map(|p| match p {
                Pseudo::Not(c) => c.specificity(),
                _ => 1 << 8,
            })
            .sum();
        let qualifiers = u32::try_from(self.classes.len() + self.attrs.len()).unwrap_or(255);
        (u32::from(self.id.is_some()) << 16)
            + (qualifiers << 8)
            + u32::from(self.tag.is_some())
            + u32::from(self.element.is_some())
            + not
    }

    fn has_hover(&self) -> bool {
        self.pseudo.iter().any(|p| matches!(p, Pseudo::Hover))
    }

    fn matches_mode(&self, el: &El, hover: bool) -> bool {
        self.tag.as_ref().is_none_or(|t| *t == el.tag)
            && self.id.as_ref().is_none_or(|id| el.id.as_ref() == Some(id))
            && self.classes.iter().all(|c| el.classes.contains(c))
            && self.attrs.iter().all(|a| a.matches(el))
            && self.pseudo.iter().all(|p| match p {
                Pseudo::Nth { a, b, typed } => {
                    let i = if *typed { el.type_index } else { el.index };
                    nth(*a, *b, i32::try_from(i).unwrap_or(i32::MAX))
                }
                Pseudo::Root => el.tag == "html",
                Pseudo::Link => el.tag == "a" && el.attr("href").is_some(),
                Pseudo::Hover => hover,
                Pseudo::Not(c) => !c.matches_mode(el, hover),
                Pseudo::Never => false,
            })
    }
}

impl AttrTest {
    fn matches(&self, el: &El) -> bool {
        let Some(v) = el.attr(&self.name) else {
            return false;
        };
        let want = self.value.as_str();
        match self.op {
            AttrOp::Exists => true,
            AttrOp::Equals => v == want,
            AttrOp::Word => v.split_ascii_whitespace().any(|w| w == want),
            AttrOp::Dash => v == want || v.strip_prefix(want).is_some_and(|r| r.starts_with('-')),
            AttrOp::Prefix => !want.is_empty() && v.starts_with(want),
            AttrOp::Suffix => !want.is_empty() && v.ends_with(want),
            AttrOp::Contains => !want.is_empty() && v.contains(want),
        }
    }
}

/// Whether the `i`th sibling (from 1) is `an+b` for some n ≥ 0.
fn nth(a: i32, b: i32, i: i32) -> bool {
    match a {
        0 => i == b,
        a => {
            let d = i - b;
            d % a == 0 && d / a >= 0
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Combinator {
    Descendant,
    Child,
    /// `+`
    Next,
    /// `~`
    Later,
}

/// Compounds from the subject leftward, each with how it joins the next.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Selector {
    parts: Vec<(Compound, Combinator)>,
}

impl Selector {
    fn specificity(&self) -> u32 {
        self.parts.iter().map(|(c, _)| c.specificity()).sum()
    }

    /// The generated box it styles, if any.
    fn element(&self) -> Option<PseudoEl> {
        self.parts.first().and_then(|(c, _)| c.element)
    }
}

/// An element as selectors see it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct El {
    pub(crate) tag: String,
    pub(crate) id: Option<String>,
    pub(crate) classes: Vec<String>,
    /// Attributes, names lowercase.
    pub(crate) attrs: Vec<(String, String)>,
    /// Its place among its parent's elements, and among those of its tag.
    pub(crate) index: usize,
    pub(crate) type_index: usize,
}

impl El {
    fn attr(&self, name: &str) -> Option<&str> {
        (self.attrs.iter())
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// One element of the path to the subject, with its earlier siblings.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Ctx<'a> {
    pub(crate) el: &'a El,
    /// The latest siblings before it, oldest first.
    pub(crate) before: &'a [El],
}

/// A place in the tree: on the path, or the `j`th earlier sibling of one.
#[derive(Clone, Copy)]
enum Pos {
    Path(usize),
    Sibling(usize, usize),
}

fn el_at<'a>(path: &[Ctx<'a>], pos: Pos) -> Option<&'a El> {
    match pos {
        Pos::Path(i) => path.get(i).map(|c| c.el),
        Pos::Sibling(i, j) => path.get(i).and_then(|c| c.before.get(j)),
    }
}

fn parent(pos: Pos) -> Option<Pos> {
    let (Pos::Path(i) | Pos::Sibling(i, _)) = pos;
    i.checked_sub(1).map(Pos::Path)
}

fn previous(path: &[Ctx<'_>], pos: Pos) -> Option<Pos> {
    match pos {
        Pos::Path(i) => {
            let n = path.get(i)?.before.len();
            n.checked_sub(1).map(|j| Pos::Sibling(i, j))
        }
        Pos::Sibling(i, j) => j.checked_sub(1).map(|j| Pos::Sibling(i, j)),
    }
}

fn matches_at(parts: &[(Compound, Combinator)], path: &[Ctx<'_>], pos: Pos, hover: bool) -> bool {
    let Some(((compound, combinator), rest)) = parts.split_first() else {
        return true;
    };
    if !el_at(path, pos).is_some_and(|el| compound.matches_mode(el, hover)) {
        return false;
    }
    if rest.is_empty() {
        return true;
    }
    let step: fn(&[Ctx<'_>], Pos) -> Option<Pos> = match combinator {
        Combinator::Child | Combinator::Descendant => |_, p| parent(p),
        Combinator::Next | Combinator::Later => previous,
    };
    let mut at = step(path, pos);
    while let Some(p) = at {
        if matches_at(rest, path, p, hover) {
            return true;
        }
        if matches!(combinator, Combinator::Child | Combinator::Next) {
            return false;
        }
        at = step(path, p);
    }
    false
}

impl Selector {
    fn has_hover(&self) -> bool {
        self.parts.iter().any(|(c, _)| c.has_hover())
    }

    /// Whether it matches the last element of `path`.
    fn matches(&self, path: &[Ctx<'_>]) -> bool {
        let Some(last) = path.len().checked_sub(1) else {
            return false;
        };
        matches_at(&self.parts, path, Pos::Path(last), false)
    }

    /// Whether it matches `path` when `:hover` is active.
    fn matches_hover(&self, path: &[Ctx<'_>]) -> bool {
        let Some(last) = path.len().checked_sub(1) else {
            return false;
        };
        matches_at(&self.parts, path, Pos::Path(last), true)
    }
}

/// Parses one selector; junk gives `None`.
fn parse_selector(s: &str) -> Option<Selector> {
    let s = s.trim();
    if s.is_empty() || s.contains('\\') {
        return None;
    }
    let mut parts: Vec<(Compound, Combinator)> = Vec::new();
    let mut joiner = Combinator::Descendant;
    let mut rest = s;
    loop {
        let (compound, after) = parse_compound(rest)?;
        parts.push((compound, joiner));
        let trimmed = after.trim_start();
        if trimmed.is_empty() {
            break;
        }
        let spaced = trimmed.len() < after.len();
        joiner = match trimmed.chars().next() {
            Some('>') => Combinator::Child,
            Some('+') => Combinator::Next,
            Some('~') => Combinator::Later,
            _ if spaced => Combinator::Descendant,
            _ => return None,
        };
        rest = match joiner {
            Combinator::Descendant => trimmed,
            _ => trimmed.get(1..)?.trim_start(),
        };
        if parts.len() >= MAX_PARTS {
            return None;
        }
    }
    // Only the subject may name a generated box.
    let inner = parts.len().saturating_sub(1);
    if parts.iter().take(inner).any(|(c, _)| c.element.is_some()) {
        return None;
    }
    // Subject first: each part keeps how it joins the one to its left.
    parts.reverse();
    Some(Selector { parts })
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '-' | '_')
}

/// An identifier at the start of `s`, and what follows.
fn ident(s: &str) -> Option<(&str, &str)> {
    let end = s.find(|c: char| !is_ident(c)).unwrap_or(s.len());
    (end > 0).then(|| s.split_at(end))
}

fn parse_compound(s: &str) -> Option<(Compound, &str)> {
    let mut c = Compound::default();
    let mut rest = s;
    if let Some(after) = rest.strip_prefix('*') {
        rest = after;
    } else if let Some((tag, after)) = ident(rest) {
        // `epub|switch`: namespaces don't matter here.
        let (tag, after) = match after.strip_prefix('|').and_then(ident) {
            Some((local, after)) => (local, after),
            None => (tag, after),
        };
        c.tag = Some(tag.to_ascii_lowercase());
        rest = after;
    }
    let start = rest.len();
    loop {
        if let Some(after) = rest.strip_prefix('#') {
            let (id, after) = ident(after)?;
            c.id = Some(id.to_owned());
            rest = after;
        } else if let Some(after) = rest.strip_prefix('.') {
            let (class, after) = ident(after)?;
            c.classes.push(class.to_owned());
            rest = after;
        } else if let Some(after) = rest.strip_prefix('[') {
            let close = after.find(']')?;
            c.attrs.push(attr_test(after.get(..close)?)?);
            rest = after.get(close + 1..)?;
        } else if let Some(after) = rest.strip_prefix(':') {
            let (legacy, after) = match after.strip_prefix(':') {
                Some(after) => (false, after),
                None => (true, after),
            };
            let (name, after) = ident(after)?;
            let lower = name.to_ascii_lowercase();
            if let Some(element) = PseudoEl::named(&lower, legacy) {
                c.element = Some(element);
                rest = after;
                continue;
            }
            if !legacy || c.element.is_some() {
                return None;
            }
            let (args, after) = match after.strip_prefix('(') {
                Some(inner) => {
                    let close = inner.find(')')?;
                    (Some(inner.get(..close)?), inner.get(close + 1..)?)
                }
                None => (None, after),
            };
            c.pseudo.push(pseudo(&name.to_ascii_lowercase(), args)?);
            rest = after;
        } else {
            break;
        }
    }
    let empty = c.tag.is_none() && rest.len() == start && !s.starts_with('*');
    (!empty).then_some((c, rest))
}

fn attr_test(inner: &str) -> Option<AttrTest> {
    let ops = [
        ("~=", AttrOp::Word),
        ("|=", AttrOp::Dash),
        ("^=", AttrOp::Prefix),
        ("$=", AttrOp::Suffix),
        ("*=", AttrOp::Contains),
        ("=", AttrOp::Equals),
    ];
    let found = ops
        .iter()
        .filter_map(|(sym, op)| inner.find(sym).map(|at| (at, *sym, op.clone())))
        .min_by_key(|(at, _, _)| *at);
    let (name, op, value) = match found {
        Some((at, sym, op)) => {
            let value = inner.get(at + sym.len()..)?.trim();
            let value = value.trim_end_matches([' ', 'i', 'I']).trim();
            (inner.get(..at)?, op, value.trim_matches(['"', '\'']))
        }
        None => (inner, AttrOp::Exists, ""),
    };
    // `epub|type` names the attribute `epub:type`.
    let name = name.trim().replace('|', ":").to_ascii_lowercase();
    let name = name.trim_start_matches(':').to_owned();
    (!name.is_empty()).then(|| AttrTest {
        name,
        op,
        value: value.to_owned(),
    })
}

/// A pseudo-class; pseudo-elements, written with one colon too, give none.
fn pseudo(name: &str, args: Option<&str>) -> Option<Pseudo> {
    let nth_args = |typed| {
        let (a, b) = an_b(args?)?;
        Some(Pseudo::Nth { a, b, typed })
    };
    match name {
        "before" | "after" | "first-line" | "first-letter" | "selection" | "marker" => None,
        "first-child" => Some(Pseudo::Nth {
            a: 0,
            b: 1,
            typed: false,
        }),
        "first-of-type" => Some(Pseudo::Nth {
            a: 0,
            b: 1,
            typed: true,
        }),
        "nth-child" => nth_args(false),
        "nth-of-type" => nth_args(true),
        "root" => Some(Pseudo::Root),
        "link" | "any-link" => Some(Pseudo::Link),
        "hover" | "focus" | "focus-visible" | "active" => Some(Pseudo::Hover),
        "not" => {
            let (inner, rest) = parse_compound(args?.trim())?;
            rest.trim().is_empty().then(|| Pseudo::Not(Box::new(inner)))
        }
        _ => Some(Pseudo::Never),
    }
}

/// `odd`, `even`, `3`, `n`, `-n+3`, `2n+1` as a and b.
fn an_b(s: &str) -> Option<(i32, i32)> {
    let s: String = s
        .split_ascii_whitespace()
        .collect::<String>()
        .to_ascii_lowercase();
    match s.as_str() {
        "odd" => return Some((2, 1)),
        "even" => return Some((2, 0)),
        _ => {}
    }
    let Some((a, b)) = s.split_once('n') else {
        return Some((0, s.parse().ok()?));
    };
    let a = match a {
        "" | "+" => 1,
        "-" => -1,
        a => a.parse().ok()?,
    };
    let b = match b {
        "" => 0,
        b => b.trim_start_matches('+').parse().ok()?,
    };
    Some((a, b))
}

pub(super) struct Rule {
    selector: Selector,
    element: Option<PseudoEl>,
    pub(super) normal: Decl,
    pub(super) important: Decl,
    pub(super) specificity: u32,
    /// Where it last appeared among the sheet's rules.
    pub(super) order: usize,
    /// Its `@layer`'s place, first named first; outside any, [`UNLAYERED`].
    pub(super) rank: u16,
}

/// The rank of rules outside every `@layer`, above them all.
const UNLAYERED: u16 = u16::MAX;
/// Layers one sheet names.
const MAX_LAYERS: usize = 64;

/// A sheet's rules, indexed by their subject's id, class or tag.
#[derive(Default)]
pub struct Stylesheet {
    rules: Vec<Rule>,
    by_id: HashMap<String, Vec<usize>>,
    by_class: HashMap<String, ByClass>,
    by_tag: HashMap<String, Vec<usize>>,
    any: Vec<usize>,
    /// A selector seen before merges into its rule.
    known: HashMap<String, usize>,
    pub(crate) faces: Vec<FontFace>,
    seen: usize,
    /// Each generated box its rules style, by [`PseudoEl::bit`].
    elements: u8,
    /// Sheets `@import` names, in order, to read before this one.
    pub(crate) imports: Vec<String>,
    /// Layer names in the order first seen, nested ones dotted.
    layer_names: Vec<String>,
    /// The `@layer` blocks reading is inside, innermost last.
    layer_path: Vec<String>,
    /// Parsed `@keyframes` rules indexed by name.
    pub(crate) keyframes: HashMap<String, Vec<mareader_reflow::KeyframeStop>>,
    /// True if any rule uses `:hover`, `:focus`, or `:active`.
    pub(super) has_hover: bool,
}

/// A class's rules: any tag, and by tag.
#[derive(Default)]
struct ByClass {
    any: Vec<usize>,
    tags: HashMap<String, Vec<usize>>,
}

impl std::fmt::Debug for Stylesheet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Stylesheet({} rules)", self.rules.len())
    }
}

impl Stylesheet {
    /// A sheet found at `path`, against which its `url()`s resolve.
    pub(crate) fn parse(css: &str, path: &str) -> Self {
        let mut sheet = Self::default();
        let css = strip_comments(css.get(..floor(css, MAX_SHEET)).unwrap_or_default());
        sheet.read(&css, path, 0);
        sheet.settle_ranks();
        sheet
    }

    /// Ranks layers in CSS order: nested ones under their parent's own rules.
    fn settle_ranks(&mut self) {
        let names = &self.layer_names;
        let place = |name: &str| names.iter().position(|n| n == name);
        let key = |name: &String| {
            let ends = name.match_indices('.').map(|(i, _)| i).chain([name.len()]);
            let mut key: Vec<usize> = ends
                .filter_map(|end| place(name.get(..end).unwrap_or_default()))
                .collect();
            key.push(usize::MAX);
            key
        };
        let mut sorted: Vec<(Vec<usize>, usize)> =
            names.iter().enumerate().map(|(i, n)| (key(n), i)).collect();
        sorted.sort();
        let mut ranks = vec![UNLAYERED - 1; names.len()];
        for (rank, (_, i)) in sorted.into_iter().enumerate() {
            if let Some(slot) = ranks.get_mut(i) {
                *slot = u16::try_from(rank).unwrap_or(UNLAYERED - 1);
            }
        }
        for rule in &mut self.rules {
            if let Some(&rank) = ranks.get(usize::from(rule.rank)) {
                rule.rank = rank;
            }
        }
    }

    fn read(&mut self, css: &str, path: &str, depth: usize) {
        const OPEN: char = '\u{7b}';
        let mut rest = css;
        while let Some(open) = rest.find(OPEN) {
            // Statements like `@import x;` end before the selector.
            let mut statements: Vec<&str> =
                rest.get(..open).unwrap_or_default().split(';').collect();
            let prelude = statements.pop().unwrap_or_default().trim();
            for statement in statements {
                self.statement(statement, path, depth);
            }
            let body = rest.get(open + 1..).unwrap_or_default();
            let Some(close) = matching_brace(body) else {
                break;
            };
            let inner = body.get(..close).unwrap_or_default();
            let lower = prelude.to_ascii_lowercase();
            let nested = depth < 4;
            if let Some(query) = lower.strip_prefix("@media") {
                if nested && query::media(query) {
                    self.read(inner, path, depth + 1);
                }
            } else if let Some(test) = lower.strip_prefix("@supports") {
                if nested && query::supports(test) {
                    self.read(inner, path, depth + 1);
                }
            } else if let Some(test) = lower.strip_prefix("@container") {
                if nested && query::container(test) {
                    self.read(inner, path, depth + 1);
                }
            } else if let Some(name) = lower.strip_prefix("@layer") {
                if nested {
                    let full = self.name_layer(name.trim());
                    self.layer_path.push(full);
                    self.read(inner, path, depth + 1);
                    self.layer_path.pop();
                }
            } else if lower.starts_with("@font-face") {
                self.face(inner, path);
            } else if hover::try_keyframes(self, prelude, &lower, inner) {
            } else if !prelude.starts_with('@') {
                let (mut normal, mut important) = Decl::parse_both(inner);
                normal.resolve(path);
                important.resolve(path);
                if !(normal.is_empty() && important.is_empty()) {
                    for selector in split_top(prelude, ',') {
                        self.add(selector, &normal, &important);
                    }
                }
            }
            rest = body.get(close + 1..).unwrap_or_default();
        }
        for statement in rest.split(';') {
            self.statement(statement, path, depth);
        }
    }

    /// `name` within the current layer, ranked if new; unnamed ones fresh.
    fn name_layer(&mut self, name: &str) -> String {
        let name = match name {
            "" => format!("\u{0}{}", self.layer_names.len()),
            named => named.to_owned(),
        };
        let full = match self.layer_path.last() {
            Some(parent) => format!("{parent}.{name}"),
            None => name,
        };
        // `a.b` names `a` too, ranked first.
        let ends = full.match_indices('.').map(|(i, _)| i).chain([full.len()]);
        for end in ends.collect::<Vec<_>>() {
            let prefix = full.get(..end).unwrap_or_default();
            if !self.layer_names.iter().any(|n| n == prefix) && self.layer_names.len() < MAX_LAYERS
            {
                self.layer_names.push(prefix.to_owned());
            }
        }
        full
    }

    /// The current layer's rank, or [`UNLAYERED`].
    fn rank(&self) -> u16 {
        let Some(current) = self.layer_path.last() else {
            return UNLAYERED;
        };
        let at = self.layer_names.iter().position(|n| n == current);
        at.map_or(UNLAYERED - 1, |at| {
            u16::try_from(at).unwrap_or(UNLAYERED - 1)
        })
    }

    /// `@import url(x.css) screen;`: a sheet read first, if its media apply.
    fn statement(&mut self, text: &str, path: &str, depth: usize) {
        let text = text.trim();
        let lower = text.to_ascii_lowercase();
        // `@layer a, b;` fixes the layers' order before their rules come.
        if let Some(names) = lower.strip_prefix("@layer") {
            for name in names.split(',').map(str::trim).filter(|n| !n.is_empty()) {
                self.name_layer(name);
            }
            return;
        }
        if depth > 0 {
            return;
        }
        let head = text.get(..7).unwrap_or_default();
        if !head.eq_ignore_ascii_case("@import") || self.imports.len() >= MAX_IMPORTS {
            return;
        }
        let rest = text.get(7..).unwrap_or_default().trim();
        let (target, after) = if rest
            .get(..4)
            .is_some_and(|h| h.eq_ignore_ascii_case("url("))
        {
            let inner = rest.get(4..).unwrap_or_default();
            let Some(close) = inner.find(')') else {
                return;
            };
            let target = inner.get(..close).unwrap_or_default();
            (
                target.trim().trim_matches(['"', '\'']),
                inner.get(close + 1..),
            )
        } else if let Some(q) = rest.chars().next().filter(|c| matches!(c, '"' | '\'')) {
            let inner = rest.get(1..).unwrap_or_default();
            let Some(close) = inner.find(q) else {
                return;
            };
            (
                inner.get(..close).unwrap_or_default(),
                inner.get(close + 1..),
            )
        } else {
            return;
        };
        let mut condition = after.unwrap_or_default().trim().to_ascii_lowercase();
        // A cascade layer's name comes first; its rules count as unlayered.
        if condition.starts_with("layer") {
            let cut = match condition.strip_prefix("layer(") {
                Some(r) => r.find(')').map_or(condition.len(), |c| c + 7),
                None => 5,
            };
            condition = condition.get(cut..).unwrap_or_default().trim().to_owned();
        }
        if let Some(r) = condition.strip_prefix("supports(") {
            let close = closing_bracket(r).unwrap_or(r.len());
            if !query::supports(&format!("({})", r.get(..close).unwrap_or_default())) {
                return;
            }
            condition = r.get(close + 1..).unwrap_or_default().trim().to_owned();
        }
        if !condition.is_empty() && !query::media(&condition) {
            return;
        }
        if let Some(resolved) = href::resolve(path, target) {
            self.imports.push(resolved);
        }
    }

    fn face(&mut self, body: &str, path: &str) {
        if self.faces.len() >= MAX_FACES {
            return;
        }
        let mut face = FontFace {
            family: String::new(),
            src: Vec::new(),
            weight: 400,
            italic: false,
        };
        for item in split_top(body, ';') {
            let Some((name, value)) = item.split_once(':') else {
                continue;
            };
            let value = value.trim();
            match name.trim().to_ascii_lowercase().as_str() {
                "font-family" => {
                    face.family = families(value).first().cloned().unwrap_or_default();
                }
                "src" => face.src = urls(value, path),
                "font-weight" => {
                    if let Some(Weight::Abs(w)) = weight(&value.to_ascii_lowercase()) {
                        face.weight = w;
                    }
                }
                "font-style" => face.italic = italic(&value.to_ascii_lowercase()) == Some(true),
                _ => {}
            }
        }
        if !face.family.is_empty() && !face.src.is_empty() {
            self.faces.push(face);
        }
    }

    /// A repeated selector merges into its rule, later values winning.
    fn add(&mut self, text: &str, normal: &Decl, important: &Decl) {
        let mut key = text.split_ascii_whitespace().collect::<Vec<_>>().join(" ");
        let plain = key.clone();
        // Layered rules merge only within their own layer.
        let rank = self.rank();
        if rank != UNLAYERED {
            key.insert_str(0, &format!("@{rank}@"));
        }
        let order = self.seen;
        self.seen += 1;
        if let Some(rule) = self.known.get(&key).and_then(|&at| self.rules.get_mut(at)) {
            rule.normal.layer(normal);
            rule.important.layer(important);
            rule.order = order;
            return;
        }
        let Some(selector) = parse_selector(&plain) else {
            return;
        };
        if self.rules.len() >= MAX_RULES {
            return;
        }
        let at = self.rules.len();
        let subject = selector
            .parts
            .first()
            .map(|(c, _)| c.clone())
            .unwrap_or_default();
        if let Some(id) = &subject.id {
            self.by_id.entry(id.clone()).or_default().push(at);
        } else if let Some(class) = subject.classes.first() {
            let by = self.by_class.entry(class.clone()).or_default();
            match &subject.tag {
                Some(tag) => by.tags.entry(tag.clone()).or_default().push(at),
                None => by.any.push(at),
            }
        } else if let Some(tag) = &subject.tag {
            self.by_tag.entry(tag.clone()).or_default().push(at);
        } else {
            self.any.push(at);
        }
        self.known.insert(key, at);
        let element = selector.element();
        self.elements |= element.map_or(0, PseudoEl::bit);
        self.has_hover |= selector.has_hover();
        self.rules.push(Rule {
            specificity: selector.specificity(),
            element,
            selector,
            normal: normal.clone(),
            important: important.clone(),
            order,
            rank,
        });
    }

    /// Calls `found(rule)` for each rule matching the path's last element,
    /// or its generated box `element`.
    fn matching<'s>(
        &'s self,
        (path, element): (&[Ctx<'_>], Option<PseudoEl>),
        found: &mut impl FnMut(&'s Rule),
    ) {
        let Some(el) = path.last().map(|c| c.el) else {
            return;
        };
        if element.is_some_and(|e| self.elements & e.bit() == 0) {
            return;
        }
        let by_id = el.id.as_ref().and_then(|id| self.by_id.get(id));
        let by_class = (el.classes.iter())
            .filter_map(|c| self.by_class.get(c))
            .flat_map(|b| {
                b.any
                    .iter()
                    .chain(b.tags.get(&el.tag).into_iter().flatten())
            });
        let candidates = (by_id.into_iter().flatten())
            .chain(by_class)
            .chain(self.by_tag.get(&el.tag).into_iter().flatten())
            .chain(&self.any);
        for &at in candidates {
            if let Some(rule) = self.rules.get(at)
                && rule.element == element
                && rule.selector.matches(path)
            {
                found(rule);
            }
        }
    }
}

/// The resolved archive paths of a `src` list's `url()`s.
fn urls(value: &str, path: &str) -> Vec<String> {
    split_top(value, ',')
        .iter()
        .filter_map(|item| {
            let start = item.find("url(")? + 4;
            let inner = item.get(start..)?;
            let inner = inner.get(..inner.find(')')?)?;
            href::resolve(path, inner.trim().trim_matches(['"', '\'']))
        })
        .take(4)
        .collect()
}

/// The browser's own defaults, under every book sheet.
fn user_agent() -> &'static Stylesheet {
    static SHEET: OnceLock<Stylesheet> = OnceLock::new();
    SHEET.get_or_init(|| {
        Stylesheet::parse(
            "html, body, div, p, section, article, header, footer, aside, nav, main, \
             figure, figcaption, blockquote, ul, ol, dl, dt, dd, li, h1, h2, h3, h4, h5, h6, \
             pre, address, center, hr, caption { display: block }\n\
             table { display: table }\n\
             head, script, style, title, template { display: none }\n\
             p, blockquote, figure, dl, ul, ol, pre { margin: 1em 0 }\n\
             h1 { font-size: 2em; margin: 0.67em 0; font-weight: bold }\n\
             h2 { font-size: 1.5em; margin: 0.83em 0; font-weight: bold }\n\
             h3 { font-size: 1.17em; margin: 1em 0; font-weight: bold }\n\
             h4 { margin: 1.33em 0; font-weight: bold }\n\
             h5 { font-size: 0.83em; margin: 1.67em 0; font-weight: bold }\n\
             h6 { font-size: 0.67em; margin: 2.33em 0; font-weight: bold }\n\
             b, strong, th { font-weight: bold }\n\
             i, em, cite, var, dfn, address { font-style: italic }\n\
             small, sub, sup { font-size: smaller }\n\
             sub { vertical-align: sub }\n\
             sup { vertical-align: super }\n\
             u, ins { text-decoration: underline }\n\
             s, strike, del { text-decoration: line-through }\n\
             big { font-size: larger }\n\
             center { text-align: center }\n\
             hr { margin: 0.5em 0 }\n\
             td, th { padding: 1px }\n\
             q::before { content: open-quote }\n\
             q::after { content: close-quote }",
            "",
        )
    })
}

/// One element's cascade: user agent, `hint`, `sheets`, `inline`.
/// Important ones win after; `var()`s filled in.
pub(crate) fn declared(
    sheets: &[Arc<Stylesheet>],
    path: &[Ctx<'_>],
    hint: &Decl,
    (inline, base): (Option<&str>, &str),
    inherited: &Vars,
) -> Decl {
    let mut ua = Decl::default();
    user_agent().matching((path, None), &mut |r| ua.layer(&r.normal));
    let mut found: Vec<(u16, u32, usize, usize, &Rule)> = Vec::new();
    for (n, sheet) in sheets.iter().enumerate() {
        sheet.matching((path, None), &mut |r| {
            found.push((r.rank, r.specificity, n, r.order, r));
        });
    }
    found.sort_by_key(|&(rank, spec, sheet, order, _)| (rank, spec, sheet, order));
    let (mut inline, mut inline_important) = inline.map(Decl::parse_both).unwrap_or_default();
    inline.resolve(base);
    inline_important.resolve(base);
    let mut layers: Vec<&Decl> = Vec::with_capacity(2 * found.len() + 4);
    layers.extend([&ua, hint]);
    layers.extend(found.iter().map(|f| &f.4.normal));
    layers.push(&inline);
    // Important rules turn the layers round: a layer's wins.
    let mut strong: Vec<&Rule> = found.iter().map(|f| f.4).collect();
    strong.sort_by_key(|r| std::cmp::Reverse(r.rank));
    layers.extend(strong.iter().map(|r| &r.important));
    layers.push(&inline_important);
    cascade(&layers, inherited)
}

/// The cascade of the last element's generated box `which`, if styled.
pub(crate) fn declared_pseudo(
    sheets: &[Arc<Stylesheet>],
    path: &[Ctx<'_>],
    which: PseudoEl,
    vars: &Vars,
) -> Option<Decl> {
    let mut found: Vec<(u32, usize, usize, &Rule)> = Vec::new();
    user_agent().matching((path, Some(which)), &mut |r| found.push((0, 0, r.order, r)));
    for (n, sheet) in sheets.iter().enumerate() {
        sheet.matching((path, Some(which)), &mut |r| {
            found.push((r.specificity, n + 1, r.order, r));
        });
    }
    if found.is_empty() {
        return None;
    }
    found.sort_by_key(|&(spec, sheet, order, _)| (sheet.min(1), spec, sheet, order));
    let mut layers: Vec<&Decl> = found.iter().map(|f| &f.3.normal).collect();
    layers.extend(found.iter().map(|f| &f.3.important));
    Some(cascade(&layers, vars))
}

/// `layers` in cascade order; each one's `var()`s, filled in, set with it.
fn cascade(layers: &[&Decl], inherited: &Vars) -> Decl {
    let own: Vec<&[(String, String)]> = layers.iter().map(|l| l.custom.as_slice()).collect();
    let vars = calc::resolve(inherited, &own);
    let mut d = Decl::default();
    for layer in layers {
        d.layer_set(layer);
        if layer.deferred.is_empty() {
            continue;
        }
        let mut late = Decl::default();
        for (name, raw) in &layer.deferred {
            // A name with no value leaves the property unset.
            match calc::substitute(raw, &vars) {
                Some(value) => late.set(name, &value),
                None => late.set(name, "unset"),
            }
        }
        late.resolve(layer.from.as_deref().unwrap_or_default());
        d.layer_set(&late);
    }
    d.cleared = None;
    d.vars = Some(vars);
    d
}

/// A line's height as it inherits: a factor, or computed px.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Line {
    Normal,
    Factor(f32),
    Px(f32),
}

/// An element's computed style, in CSS px; sides go top, right, bottom, left.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Computed {
    pub(crate) size: f32,
    pub(crate) line: Line,
    pub(crate) weight: u16,
    pub(crate) italic: bool,
    pub(crate) family: Option<Arc<[String]>>,
    pub(crate) letter: f32,
    pub(crate) word: f32,
    /// Px, or a percentage of the column.
    pub(crate) indent: Len,
    pub(crate) align: TextAlign,
    pub(crate) unmarked: bool,
    /// Unset, lists show their own default markers.
    pub(crate) list_style: Option<ListStyle>,
    pub(crate) list_image: Option<Arc<str>>,
    pub(crate) quotes: Option<Quotes>,
    // Not inherited.
    pub(crate) display: Display,
    /// Px, percentages of the column, or auto.
    pub(crate) margin: [Len; 4],
    pub(crate) padding: [f32; 4],
    /// Zero where no border is drawn.
    pub(crate) border: [f32; 4],
    pub(crate) border_style: [BorderStyle; 4],
    /// Each side's colour; `None` is the book's text colour.
    pub(crate) border_colour: [Option<Rgb>; 4],
    pub(crate) width: Len,
    pub(crate) height: Len,
    pub(crate) background: bool,
    pub(crate) float: Float,
    pub(crate) placing: Placing,
    /// Top, right, bottom, left: px, percentages or auto.
    pub(crate) offsets: [Len; 4],
    pub(crate) z_index: i32,
    pub(crate) break_before: bool,
    pub(crate) break_after: bool,
    /// A cell's `vertical-align`, if set.
    pub(crate) valign: Option<VAlign>,
    pub(crate) script: Script,
    /// Underline and line-through, the element's own.
    pub(crate) decoration: (bool, bool),
    pub(crate) fill: Option<Rgb>,
    /// The text colour, inherited; `None` is the book's default.
    pub(crate) ink: Option<Rgb>,
    pub(crate) picture: Option<Arc<str>>,
    pub(crate) picture_size: PictureSize,
    pub(crate) picture_at: [f32; 2],
    pub(crate) picture_repeat: bool,
    pub(crate) flex_column: bool,
    pub(crate) justify: Justify,
    pub(crate) align_items: Option<VAlign>,
    pub(crate) align_self: Option<VAlign>,
    /// Items fill a column's width, as `align-items` sets by default.
    pub(crate) items_stretch: bool,
    pub(crate) self_stretch: Option<bool>,
    pub(crate) wrap: bool,
    /// Px, between items and between rows or a column's items.
    pub(crate) gap: f32,
    pub(crate) row_gap: f32,
    pub(crate) grow: f32,
    pub(crate) basis: Len,
    /// Px, or a percentage of the box's width.
    pub(crate) radius: Len,
    pub(crate) rotate: f32,
    /// A line picture's place against its text.
    pub(crate) rise: Rise,
    pub(crate) layers: Arc<[decor::Layer]>,
    pub(crate) shadows: Arc<[decor::Shadow]>,
    /// Px; zero where no outline is drawn.
    pub(crate) outline: f32,
    pub(crate) outline_style: BorderStyle,
    pub(crate) outline_colour: Option<Rgb>,
    pub(crate) outline_offset: f32,
    pub(crate) frame: Option<Arc<str>>,
    pub(crate) frame_slice: Slice,
    pub(crate) frame_width: [Span; 4],
    pub(crate) frame_outset: [Span; 4],
    pub(crate) frame_tile: [Tile; 2],
    pub(crate) border_clear: [bool; 4],
    /// Its own opacity times every ancestor's.
    pub(crate) opacity: f32,
    /// Hidden by `visibility`, inherited.
    pub(crate) invisible: bool,
    // Inherited.
    pub(crate) small_caps: Caps,
    pub(crate) features: [Features; 5],
    pub(crate) last: Option<LastLine>,
    /// 0 is the usual 2.
    pub(crate) orphans: u8,
    pub(crate) widows: u8,
    pub(crate) whole: bool,
    pub(crate) keep_next: bool,
    pub(crate) keep_last: bool,
    pub(crate) dir: Dir,
    /// Vertical writing, how its letters stand, and runs set across.
    pub(crate) writing: bool,
    pub(crate) orient: Orient,
    pub(crate) combine: bool,
    pub(crate) transform: Case,
    pub(crate) text_shadow: Option<decor::Shadow>,
    /// Custom properties, inherited.
    pub(crate) vars: Vars,
    /// Extended computed properties (RGBA fills, box-sizing, multicol).
    pub(crate) extra: Option<Box<extra::ExtraComputed>>,
}

impl Default for Computed {
    fn default() -> Self {
        Self {
            size: ROOT_PX,
            line: Line::Normal,
            weight: 400,
            italic: false,
            family: None,
            letter: 0.0,
            word: 0.0,
            indent: Len::Px(0.0),
            align: TextAlign::Start,
            unmarked: false,
            list_style: None,
            list_image: None,
            quotes: None,
            display: Display::Block,
            margin: [Len::Px(0.0); 4],
            padding: [0.0; 4],
            border: [0.0; 4],
            border_style: [BorderStyle::None; 4],
            border_colour: [None; 4],
            width: Len::Auto,
            height: Len::Auto,
            background: false,
            float: Float::None,
            placing: Placing::Static,
            offsets: [Len::Auto; 4],
            z_index: 0,
            break_before: false,
            break_after: false,
            valign: None,
            script: Script::Normal,
            decoration: (false, false),
            fill: None,
            ink: None,
            picture: None,
            picture_size: PictureSize::Auto,
            picture_at: [0.0; 2],
            picture_repeat: true,
            flex_column: false,
            justify: Justify::Start,
            align_items: None,
            align_self: None,
            items_stretch: true,
            self_stretch: None,
            wrap: false,
            gap: 0.0,
            row_gap: 0.0,
            grow: 0.0,
            basis: Len::Auto,
            radius: Len::Px(0.0),
            rotate: 0.0,
            rise: Rise::Baseline,
            layers: Arc::from([]),
            shadows: Arc::from([]),
            outline: 0.0,
            outline_style: BorderStyle::None,
            outline_colour: None,
            outline_offset: 0.0,
            frame: None,
            frame_slice: Slice::default(),
            frame_width: [Span::Times(1.0); 4],
            frame_outset: [Span::Times(0.0); 4],
            frame_tile: [Tile::Stretch; 2],
            border_clear: [false; 4],
            opacity: 1.0,
            invisible: false,
            small_caps: Caps::Normal,
            features: [Features::default(); 5],
            last: None,
            orphans: 0,
            widows: 0,
            whole: false,
            keep_next: false,
            keep_last: false,
            dir: Dir::Auto,
            writing: false,
            orient: Orient::Mixed,
            combine: false,
            transform: Case::Normal,
            text_shadow: None,
            vars: Arc::from([]),
            extra: None,
        }
    }
}

impl Computed {
    /// A child's style: what `d` sets, the rest inherited or initial.
    pub(crate) fn child(&self, d: &Decl) -> Self {
        let size = match d.size {
            None => self.size,
            Some(FontSize::Larger) => self.size * 1.2,
            Some(FontSize::Smaller) => self.size / 1.2,
            Some(FontSize::Len(l)) => l.px(self.size, self.size).unwrap_or(self.size),
        }
        .clamp(1.0, 1000.0);
        let line = match d.line {
            None => self.line,
            Some(LineHeight::Normal) => Line::Normal,
            Some(LineHeight::Factor(f)) => Line::Factor(f),
            Some(LineHeight::Len(l)) => Line::Px(l.px(size, size).unwrap_or(0.0).max(0.0)),
        };
        let weight = match d.weight {
            None => self.weight,
            Some(Weight::Abs(w)) => w,
            Some(Weight::Bolder) => match self.weight {
                w if w < 350 => 400,
                w if w < 550 => 700,
                _ => 900,
            },
            Some(Weight::Lighter) => match self.weight {
                w if w < 550 => 100,
                w if w < 750 => 400,
                _ => 700,
            },
        };
        let px = |l: Option<Len>| l.and_then(|l| l.px(size, BASE_WIDTH)).unwrap_or(0.0);
        let border_style = d.border_style.map(Option::unwrap_or_default);
        let mut border = [0.0; 4];
        for ((b, w), s) in border.iter_mut().zip(d.border_width).zip(border_style) {
            if s != BorderStyle::None {
                *b = w.map_or(3.0, |w| px(Some(w))).max(0.0);
            }
        }
        Self {
            size,
            line,
            weight,
            italic: d.italic.unwrap_or(self.italic),
            family: d.family.clone().or_else(|| self.family.clone()),
            letter: d.letter.map_or(self.letter, |l| px(Some(l))),
            word: d.word.map_or(self.word, |l| px(Some(l))),
            indent: d.indent.map_or(self.indent, |l| l.computed(size)),
            align: d.align.unwrap_or(self.align),
            unmarked: d.unmarked.unwrap_or(self.unmarked),
            list_style: d.list_style.clone().or_else(|| self.list_style.clone()),
            list_image: match &d.list_image {
                Some(image) => image.clone(),
                None => self.list_image.clone(),
            },
            quotes: d.quotes.clone().or_else(|| self.quotes.clone()),
            display: d.display.unwrap_or(Display::Inline),
            margin: d
                .margin
                .map(|m| m.map_or(Len::Px(0.0), |m| m.computed(size))),
            padding: d.padding.map(|p| px(p).max(0.0)),
            border,
            border_style,
            border_colour: d.border_colour.map(|c| c.flatten().or(d.ink).or(self.ink)),
            width: d.width.map_or(Len::Auto, |l| l.computed(size)),
            height: d.height.map_or(Len::Auto, |l| l.computed(size)),
            background: d.background.unwrap_or(false) || d.background_image.unwrap_or(false),
            float: d.float.unwrap_or_default(),
            placing: d.placing.unwrap_or_default(),
            offsets: d.offsets.map(|o| o.map_or(Len::Auto, |l| l.computed(size))),
            z_index: d.z_index.unwrap_or(0),
            break_before: d.break_before.unwrap_or(false),
            break_after: d.break_after.unwrap_or(false),
            valign: d.valign,
            script: d.script.unwrap_or_default(),
            decoration: d.decoration.unwrap_or_default(),
            fill: d.fill.flatten(),
            ink: d.ink.or(self.ink),
            picture: d.picture.clone().flatten(),
            picture_size: d.picture_size.unwrap_or_default(),
            picture_at: d.picture_at.unwrap_or([0.0; 2]),
            picture_repeat: d.picture_repeat.unwrap_or(true),
            flex_column: d.flex_column.unwrap_or(false),
            justify: d.justify.unwrap_or_default(),
            align_items: d.align_items,
            align_self: d.align_self,
            items_stretch: d.items_stretch.unwrap_or(true),
            self_stretch: d.self_stretch,
            wrap: d.wrap.unwrap_or(false),
            gap: d.gap.map_or(0.0, |l| px(Some(l)).max(0.0)),
            row_gap: d.row_gap.map_or(0.0, |l| px(Some(l)).max(0.0)),
            grow: d.grow.unwrap_or(0.0).max(0.0),
            basis: d.basis.map_or(Len::Auto, |l| l.computed(size)),
            radius: d.radius.map_or(Len::Px(0.0), |l| l.computed(size)),
            rotate: d.rotate.unwrap_or(0.0),
            rise: d.lift.map_or(Rise::Baseline, |l| rise(l, size)),
            layers: d.layers.clone().unwrap_or_else(|| Arc::from([])),
            shadows: match (&d.shadows, &d.drop) {
                (s, Some(drop)) if !drop.is_empty() => {
                    let own = s.as_deref().unwrap_or_default().iter();
                    own.chain(drop.iter()).cloned().collect()
                }
                (s, _) => s.clone().unwrap_or_else(|| Arc::from([])),
            },
            outline: match d.outline_style.unwrap_or_default() {
                BorderStyle::None => 0.0,
                _ => d.outline_width.map_or(3.0, |w| px(Some(w))).max(0.0),
            },
            outline_style: d.outline_style.unwrap_or_default(),
            outline_colour: d.outline_colour.flatten().or(d.ink).or(self.ink),
            outline_offset: px(d.outline_offset),
            frame: d.frame.clone().flatten(),
            frame_slice: d.frame_slice.unwrap_or_default(),
            frame_width: d.frame_width.unwrap_or([Span::Times(1.0); 4]),
            frame_outset: d.frame_outset.unwrap_or([Span::Times(0.0); 4]),
            frame_tile: d.frame_tile.unwrap_or_default(),
            border_clear: d.border_clear.map(|c| c.unwrap_or(false)),
            opacity: self.opacity * d.opacity.unwrap_or(1.0) * d.filter_opacity.unwrap_or(1.0),
            invisible: d.visible.map_or(self.invisible, |v| !v),
            small_caps: d.small_caps.unwrap_or(self.small_caps),
            features: child_features(self.features, d),
            last: d.last.unwrap_or(self.last),
            orphans: d.orphans.unwrap_or(self.orphans),
            widows: d.widows.unwrap_or(self.widows),
            whole: d.whole.unwrap_or(false),
            keep_next: d.keep_next.unwrap_or(false),
            keep_last: d.keep_last.unwrap_or(false),
            dir: d.dir.unwrap_or(self.dir),
            writing: d.writing.unwrap_or(self.writing),
            orient: d.orient.unwrap_or(self.orient),
            combine: d.combine.unwrap_or(false),
            transform: d.transform.unwrap_or(self.transform),
            text_shadow: d
                .text_shadow
                .clone()
                .unwrap_or_else(|| self.text_shadow.clone()),
            vars: d.vars.clone().unwrap_or_else(|| Arc::clone(&self.vars)),
            extra: extra::compute_extra(d, size),
        }
    }

    /// The letter case its text shows.
    pub(crate) fn case(&self) -> Case {
        match (self.transform, self.small_caps) {
            (_, Caps::AllSmall) | (Case::Lower, Caps::Small) => Case::AllSmall,
            (Case::Normal, Caps::Small) => Case::Small,
            (case, _) => case,
        }
    }

    /// The type features its text asks for, settings last.
    pub(crate) fn features(&self) -> Features {
        self.features
            .into_iter()
            .fold(Features::default(), Features::then)
    }
}

fn child_features(base: [Features; 5], d: &Decl) -> [Features; 5] {
    [
        d.numeric.unwrap_or(base[0]),
        d.ligatures.unwrap_or(base[1]),
        d.position.unwrap_or(base[2]),
        d.kerning.unwrap_or(base[3]),
        d.settings.unwrap_or(base[4]),
    ]
}

impl Computed {
    /// One line's height in px.
    pub(crate) fn line_px(&self) -> f32 {
        match self.line {
            Line::Normal => 1.2 * self.size,
            Line::Factor(f) => f * self.size,
            Line::Px(p) => p,
        }
    }

    /// A side's margin in px; auto and percentages of the column, near enough.
    pub(crate) fn margin_px(&self, side: usize) -> f32 {
        let m = self.margin.get(side).copied().unwrap_or(Len::Px(0.0));
        m.px(self.size, BASE_WIDTH).unwrap_or(0.0)
    }
}

/// The largest char boundary at or below `max`.
fn floor(s: &str, max: usize) -> usize {
    (0..=max.min(s.len()))
        .rev()
        .find(|&i| s.is_char_boundary(i))
        .unwrap_or(0)
}

fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(rest.get(..start).unwrap_or_default());
        let after = rest.get(start + 2..).unwrap_or_default();
        rest = after
            .find("*/")
            .map_or("", |end| after.get(end + 2..).unwrap_or_default());
    }
    out.push_str(rest);
    out
}

/// Where the block opened just before `body` closes, nesting included.
fn matching_brace(body: &str) -> Option<usize> {
    let mut depth = 1usize;
    for (i, c) in body.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    /// No custom properties inherited.
    fn none() -> Vars {
        Arc::from([])
    }

    use super::*;

    #[test]
    fn vertical_words_take_their_older_names() {
        assert_eq!(writing_words("vertical-rl"), Some(true));
        assert_eq!(writing_words("tb-rl"), Some(true));
        assert_eq!(writing_words("horizontal-tb"), Some(false));
        assert_eq!(writing_words("sideways"), None);
        assert_eq!(orient_words("upright"), Some(Orient::Upright));
        assert_eq!(orient_words("sideways-right"), Some(Orient::Sideways));
        assert_eq!(combine_words("horizontal"), Some(true));
        assert_eq!(combine_words("digits 2"), Some(true));
        assert_eq!(combine_words("none"), Some(false));
    }

    fn el(tag: &str, classes: &[&str]) -> El {
        El {
            tag: tag.to_owned(),
            classes: classes.iter().map(|c| (*c).to_owned()).collect(),
            index: 1,
            type_index: 1,
            ..El::default()
        }
    }

    /// The cascade for the last of `chain`, each a child of the one before.
    fn cascade(css: &str, chain: &[El]) -> Decl {
        let sheet = Arc::new(Stylesheet::parse(css, "/s.css"));
        let path: Vec<Ctx<'_>> = chain.iter().map(|el| Ctx { el, before: &[] }).collect();
        declared(&[sheet], &path, &Decl::default(), (None, ""), &none())
    }

    #[test]
    fn pseudo_element_rules_style_only_their_boxes() {
        let css = "p::before { content: 'a' } p.x:before { content: 'b'; color: red } \
                   p { color: blue } p::selection { color: green } \
                   div::before span { color: red } li::marker { color: red }";
        let sheet = Arc::new(Stylesheet::parse(css, "/s.css"));
        let (plain, x) = (el("p", &[]), el("p", &["x"]));
        let content = |e: &El, which| {
            let path = [Ctx { el: e, before: &[] }];
            declared_pseudo(&[Arc::clone(&sheet)], &path, which, &none()).and_then(|d| d.content)
        };
        let text = |s: &str| Some(Content::Items(Arc::from([generated::Item::Text(s.into())])));
        assert_eq!(content(&plain, PseudoEl::Before), text("a"));
        assert_eq!(content(&x, PseudoEl::Before), text("b"));
        assert_eq!(content(&plain, PseudoEl::After), None);
        // The element's own cascade takes nothing from its boxes.
        let own = cascade(css, &[x]);
        assert_eq!(own.content, None);
        assert_eq!(own.ink, colour("blue").flatten());
        assert_eq!(cascade(css, &[el("div", &[]), el("span", &[])]).ink, None);
        let marker = [Ctx {
            el: &el("li", &[]),
            before: &[],
        }];
        let red = declared_pseudo(&[Arc::clone(&sheet)], &marker, PseudoEl::Marker, &none());
        assert_eq!(red.and_then(|d| d.ink), colour("red").flatten());
        let q = [Ctx {
            el: &el("q", &[]),
            before: &[],
        }];
        let open = declared_pseudo(&[], &q, PseudoEl::Before, &none()).and_then(|d| d.content);
        assert_eq!(
            open,
            Some(Content::Items(Arc::from([generated::Item::OpenQuote])))
        );
    }

    #[test]
    fn list_styles_counters_and_quotes_declare() {
        let d = Decl::parse(
            "list-style: upper-roman inside url(dot.png); counter-reset: ch 2 sec; \
             counter-increment: ch; quotes: '«' '»'",
        );
        assert_eq!(d.list_style, generated::list_style("upper-roman"));
        assert_eq!(d.list_image, Some(Some(Arc::from("dot.png"))));
        assert_eq!(d.unmarked, Some(false));
        let reset = d.counter_reset.as_deref().map(<[_]>::to_vec);
        assert_eq!(reset, Some(vec![("ch".into(), 2), ("sec".into(), 0)]));
        assert_eq!(d.counter_increment.as_deref().map(<[_]>::len), Some(1));
        assert_eq!(d.quotes.as_deref().map(<[_]>::len), Some(1));
        let none = Decl::parse("list-style: none");
        assert_eq!((none.unmarked, none.list_image), (Some(true), Some(None)));
        let parent = Computed::default().child(&d);
        let child = parent.child(&Decl::default());
        assert_eq!(child.list_style, generated::list_style("upper-roman"));
        assert!(child.quotes.is_some());
    }

    #[test]
    fn type_features_join_their_groups_then_the_settings() {
        let d = Decl::parse(
            "font-variant-numeric: oldstyle-nums tabular-nums; font-variant-ligatures: none; \
             font-kerning: none; font-feature-settings: \"liga\" 1, 'zero', \"tnum\" off, \"wxyz\"",
        );
        let c = Computed::default().child(&d);
        let wanted = "\"liga\" 1, \"clig\" 0, \"dlig\" 0, \"hlig\" 0, \"calt\" 0, \
                      \"kern\" 0, \"onum\" 1, \"tnum\" 0, \"zero\" 1";
        assert_eq!(c.features().source(), wanted);
        // They inherit; an unknown word voids only its own declaration.
        let child = c.child(&Decl::parse("font-variant-numeric: lining-nums wide"));
        assert_eq!(child.features(), c.features());
        let shorthand = Decl::parse("font-variant: all-small-caps slashed-zero");
        let c = Computed::default().child(&shorthand);
        assert_eq!(
            (c.case(), c.features().source()),
            (Case::AllSmall, "\"zero\" 1".into())
        );
        let font = c.child(&Decl::parse("font: small-caps 1em serif"));
        assert_eq!(
            (font.case(), font.features()),
            (Case::Small, Features::default())
        );
        let lower = Decl::parse("text-transform: lowercase; font-variant-caps: small-caps");
        assert_eq!(Computed::default().child(&lower).case(), Case::AllSmall);
    }

    #[test]
    fn last_lines_and_page_breaks_read_as_written() {
        let d = Decl::parse(
            "text-align-last: center; widows: 3; orphans: 0; break-inside: avoid; \
             page-break-after: avoid; break-before: avoid",
        );
        let c = Computed::default().child(&d);
        assert_eq!(
            (c.last, c.widows, c.orphans),
            (Some(LastLine::Center), 3, 0)
        );
        assert!(c.whole && c.keep_next && c.keep_last);
        // Lines and alignment inherit; breaks belong to their box.
        let child = c.child(&Decl::default());
        assert_eq!((child.last, child.widows), (Some(LastLine::Center), 3));
        assert!(!child.whole && !child.keep_next && !child.keep_last);
        let auto = c.child(&Decl::parse("text-align-last: auto; widows: initial"));
        assert_eq!((auto.last, auto.widows), (None, 2));
        assert!(understood("font-kerning", "none") && !understood("widows", "lots"));
    }

    #[test]
    fn declarations_read_the_supported_properties() {
        let d = Decl::parse("font-style: italic; FONT-WEIGHT: 700 !important; text-align:center");
        assert_eq!(d.italic, Some(true));
        assert_eq!(d.weight, Some(Weight::Abs(700)));
        assert_eq!(d.align, Some(TextAlign::Center));
        assert_eq!(Decl::parse("display: none").display, Some(Display::None));
        assert_eq!(
            Decl::parse("font: italic bold 1em serif").italic,
            Some(true)
        );
        let d = Decl::parse("margin: 1em 2px; border: #fff 1px double; letter-spacing: 0.2em");
        let (em, px) = (Some(Len::Em(1.0)), Some(Len::Px(2.0)));
        assert_eq!(d.margin, [em, px, em, px]);
        assert_eq!(d.border_style, [Some(BorderStyle::Double); 4]);
        assert_eq!(d.border_width, [Some(Len::Px(1.0)); 4]);
        assert_eq!(d.letter, Some(Len::Em(0.2)));
        let d = Decl::parse("border-top: solid 1px #FFF; font-family: \"Title A\", serif");
        assert_eq!(d.border_style[0], Some(BorderStyle::Solid));
        assert_eq!(d.border_style[1], None);
        assert_eq!(d.border_colour[0], Some(Some([255; 3])));
        let e = Decl::parse("border: 1px solid rgb(10, 20, 30); border-left-color: red");
        assert_eq!(e.border_colour[1], Some(Some([10, 20, 30])));
        assert_eq!(e.border_colour[3], Some(Some([255, 0, 0])));
        let e = Decl::parse("color: navy; border-bottom: thin dotted");
        let c = Computed::default().child(&e);
        assert_eq!(c.border_colour[2], Some([0, 0, 128]));
        let c = Computed::default().child(&Decl::parse("border-color: teal #fff"));
        assert_eq!(
            c.border_colour,
            [
                Some([0, 128, 128]),
                Some([255; 3]),
                Some([0, 128, 128]),
                Some([255; 3])
            ]
        );
        let family: Vec<String> = d.family.iter().flat_map(|f| f.iter().cloned()).collect();
        assert_eq!(family, ["Title A", "serif"]);
        assert_eq!(Decl::parse("color: red").ink, Some([255, 0, 0]));
        assert!(Decl::parse("color: currentcolor").is_empty());
    }

    #[test]
    fn colours_read_every_form_and_inherit() {
        let c = |v: &str| colour(v).flatten();
        assert_eq!(c("teal"), Some([0, 128, 128]));
        assert_eq!(c("rebeccapurple"), Some([102, 51, 153]));
        assert_eq!(c("#0a0"), Some([0, 170, 0]));
        assert_eq!(c("rgb(10 20 30 / 50%)"), Some([10, 20, 30]));
        assert_eq!(c("hsl(120, 100%, 25%)"), Some([0, 128, 0]));
        assert_eq!(c("hsl(0.5turn 100% 50%)"), Some([0, 255, 255]));
        assert_eq!(c("nonsense"), None);
        let parent = Computed::default().child(&Decl::parse("color: navy"));
        let child = parent.child(&Decl::parse("font-weight: bold"));
        assert_eq!(child.ink, Some([0, 0, 128]));
        assert_eq!(Computed::default().ink, None);
    }

    #[test]
    fn lengths_compute_from_every_unit() {
        let px = |v: &str| len(v).and_then(|l| l.px(10.0, 200.0));
        assert_eq!(px("12pt"), Some(16.0));
        assert_eq!(px("1.5em"), Some(15.0));
        assert_eq!(px("2rem"), Some(32.0));
        assert_eq!(px("50%"), Some(100.0));
        assert_eq!(px("1in"), Some(96.0));
        assert_eq!(px("0"), Some(0.0));
        assert_eq!(px("-0.5em"), Some(-5.0));
        assert_eq!(px("3"), None);
        assert_eq!(len("auto"), Some(Len::Auto));
    }

    #[test]
    fn selectors_combine_and_specificity_orders_them() {
        let css = "p { text-align: right } div p { text-align: center }\n\
                   div > p.x { text-align: left } #me { font-style: italic }\n\
                   p:first-child { font-weight: bold } a:hover { font-weight: 900 }\n\
                   [epub|type~=\"note\"] { display: none } p::first-letter { font-size: 3em }";
        let div = el("div", &[]);
        let align = |chain: &[El]| cascade(css, chain).align;
        assert_eq!(align(&[el("p", &[])]), Some(TextAlign::Right));
        assert_eq!(
            align(&[div.clone(), el("span", &[]), el("p", &[])]),
            Some(TextAlign::Center)
        );
        assert_eq!(
            align(&[div.clone(), el("p", &["x"])]),
            Some(TextAlign::Left)
        );
        assert_eq!(
            align(&[el("section", &[]), el("p", &["x"])]),
            Some(TextAlign::Right)
        );
        let mut me = el("p", &[]);
        me.id = Some("me".to_owned());
        me.index = 2;
        let d = cascade(css, &[me]);
        assert_eq!((d.italic, d.weight), (Some(true), None));
        assert_eq!(cascade(css, &[el("p", &[])]).weight, Some(Weight::Abs(700)));
        assert_eq!(cascade(css, &[el("a", &[])]).weight, None);
        let mut note = el("aside", &[]);
        note.attrs
            .push(("epub:type".to_owned(), "foot note".to_owned()));
        assert_eq!(cascade(css, &[note]).display, Some(Display::None));
        assert_eq!(cascade(css, &[el("p", &[])]).size, None);
    }

    #[test]
    fn siblings_match_by_what_came_before() {
        let sheet = Arc::new(Stylesheet::parse(
            "h1 + p { font-style: italic } h1 ~ ul { font-weight: bold }",
            "/s.css",
        ));
        let (body, h1, hr) = (el("body", &[]), el("h1", &[]), el("hr", &[]));
        let p = el("p", &[]);
        let decl = |before: &[El], subject: &El| {
            let path = [
                Ctx {
                    el: &body,
                    before: &[],
                },
                Ctx {
                    el: subject,
                    before,
                },
            ];
            declared(
                std::slice::from_ref(&sheet),
                &path,
                &Decl::default(),
                (None, ""),
                &none(),
            )
        };
        assert_eq!(decl(std::slice::from_ref(&h1), &p).italic, Some(true));
        assert_eq!(decl(&[h1.clone(), hr.clone()], &p).italic, None);
        assert_eq!(
            decl(&[h1, hr], &el("ul", &[])).weight,
            Some(Weight::Abs(700))
        );
    }

    #[test]
    fn important_and_inline_styles_win_in_order() {
        let sheet = Arc::new(Stylesheet::parse(
            ".a { text-align: center !important } p.a { text-align: right }",
            "/s.css",
        ));
        let p = el("p", &["a"]);
        let path = [Ctx {
            el: &p,
            before: &[],
        }];
        let d = declared(
            std::slice::from_ref(&sheet),
            &path,
            &Decl::default(),
            (Some("text-align: left"), ""),
            &none(),
        );
        assert_eq!(d.align, Some(TextAlign::Center));
        let d = declared(
            &[sheet],
            &path,
            &Decl::default(),
            (Some("text-align: left !important"), ""),
            &none(),
        );
        assert_eq!(d.align, Some(TextAlign::Left));
    }

    #[test]
    fn media_and_faces_are_read_and_print_is_not() {
        let sheet = Stylesheet::parse(
            "@media print { p { font-style: italic } }\n\
             @media screen and (min-width: 1px) { p { font-weight: bold } }\n\
             @font-face { font-family: \"title\"; src: url(../Fonts/title.ttf) format(\"truetype\") }\n\
             @font-face { font-family: broken }",
            "/OEBPS/Styles/main.css",
        );
        let p = el("p", &[]);
        let path = [Ctx {
            el: &p,
            before: &[],
        }];
        let d = declared(
            &[Arc::new(sheet)],
            &path,
            &Decl::default(),
            (None, ""),
            &none(),
        );
        assert_eq!((d.italic, d.weight), (None, Some(Weight::Abs(700))));
        let sheet = Stylesheet::parse(
            "@font-face { font-family: \"title\"; src: url(../Fonts/title.ttf) }",
            "/OEBPS/Styles/main.css",
        );
        let face = sheet.faces.first().cloned();
        assert_eq!(face.as_ref().map(|f| f.family.as_str()), Some("title"));
        assert_eq!(
            face.map(|f| f.src),
            Some(vec!["/OEBPS/Fonts/title.ttf".to_owned()])
        );
    }

    #[test]
    fn computed_styles_inherit_and_resolve_lengths() {
        let root = Computed::default();
        let div = root.child(&Decl::parse("font-size: 0.9em; line-height: 130%"));
        assert!((div.size - 14.4).abs() < 0.01);
        assert!((div.line_px() - 14.4 * 1.3).abs() < 0.01);
        let p = div.child(&Decl::parse(
            "font-size: 0.5em; margin: 1em 0 -0.5em; border-top: 1px solid",
        ));
        assert!((p.size - 7.2).abs() < 0.01);
        // Percent line heights inherit as computed lengths.
        assert_eq!(p.line, div.line);
        assert!((p.margin_px(0) - 7.2).abs() < 0.01);
        assert!((p.margin_px(2) + 3.6).abs() < 0.01);
        assert_eq!(p.border, [1.0, 0.0, 0.0, 0.0]);
        let lh = p.child(&Decl::parse("line-height: 1em; font-weight: bolder"));
        assert_eq!((lh.line, lh.weight), (Line::Px(p.size), 700));
        assert_eq!(
            p.child(&Decl::parse("line-height: 1.5")).line_px(),
            1.5 * p.size
        );
    }

    #[test]
    fn junk_and_unclosed_rules_end_quietly() {
        let sheet = Stylesheet::parse("p { font-style: italic; } q { font-weight", "/s.css");
        assert_eq!(sheet.rules.len(), 1);
        let _ = Stylesheet::parse("}}}{{{ ; @ { /* ", "/s.css");
        let _ = Stylesheet::parse("a[ { x: y } :not( { } ::: {} p + { }", "/s.css");
    }

    #[test]
    fn repeated_selectors_merge_so_each_element_meets_few_rules() {
        let sheet = Stylesheet::parse(
            "p { font-style: italic; text-align: center } .x { font-weight: bold }\n\
             p { text-align: right }",
            "/s.css",
        );
        assert_eq!(sheet.rules.len(), 2);
        let p = el("p", &["x"]);
        let path = [Ctx {
            el: &p,
            before: &[],
        }];
        let mut found = 0;
        sheet.matching((&path, None), &mut |_| found += 1);
        assert_eq!(found, 2);
        // Thousands of rules, but few an element can meet.
        let mut css = "p { font-style: italic }".repeat(5000);
        css.extend((0..5000).map(|i| format!("t{i}.c {{ font-weight: bold }}")));
        let sheet = Stylesheet::parse(&css, "/s.css");
        let p = el("p", &["c"]);
        let path = [Ctx {
            el: &p,
            before: &[],
        }];
        let mut found = 0;
        sheet.matching((&path, None), &mut |_| found += 1);
        assert_eq!(found, 1);
    }

    /// The cascade for one `tag` element of `classes` under `css`.
    fn styled(css: &str, tag: &str, classes: &[&str], inherited: &Vars) -> Decl {
        let sheet = Arc::new(Stylesheet::parse(css, "/OEBPS/css/s.css"));
        let e = el(tag, classes);
        let path = [Ctx {
            el: &e,
            before: &[],
        }];
        let inline = (None, "/OEBPS/text/c.xhtml");
        declared(&[sheet], &path, &Decl::default(), inline, inherited)
    }

    #[test]
    fn var_reads_custom_properties_through_the_cascade() {
        let css = "p { --ink: #336699; --gap: 2em; color: var(--ink); \
                   margin: var(--gap) 0; text-indent: var(--none, 3em) } \
                   p.x { --ink: #ff0000 } p.y { color: #008000 } p.z { color: var(--nothing) } \
                   p.u { background-image: var(--art); --art: url(../img/a.png) } \
                   p.w { width: calc(100% - var(--gap)) } p.s { --Case: 1em; text-indent: var(--Case) }";
        let p = styled(css, "p", &[], &none());
        assert_eq!(p.ink, Some([0x33, 0x66, 0x99]));
        let (em2, zero) = (Some(Len::Em(2.0)), Some(Len::Px(0.0)));
        assert_eq!(p.margin, [em2, zero, em2, zero]);
        assert_eq!(p.indent, Some(Len::Em(3.0)));
        assert!(p.vars.as_ref().is_some_and(|v| v.len() == 2));
        // A more specific rule wins, by name or by value.
        assert_eq!(styled(css, "p", &["x"], &none()).ink, Some([255, 0, 0]));
        assert_eq!(styled(css, "p", &["y"], &none()).ink, Some([0, 128, 0]));
        // A name with no value unsets the property, as CSS says.
        assert_eq!(styled(css, "p", &["z"], &none()).ink, None);
        let u = styled(css, "p", &["u"], &none());
        assert_eq!(u.picture, Some(Some(Arc::from("/OEBPS/img/a.png"))));
        let w = styled(css, "p", &["w"], &none()).width;
        assert_eq!(
            w,
            Some(Len::Mix {
                px: 0.0,
                em: -2.0,
                pct: 100.0
            })
        );
        // Custom property names keep their case.
        assert_eq!(styled(css, "p", &["s"], &none()).indent, Some(Len::Em(1.0)));
    }

    #[test]
    fn custom_properties_reach_children_and_generated_boxes() {
        let div = styled("div { --ink: #112233 }", "div", &[], &none());
        let parent = Computed::default().child(&div);
        let p = styled("p { color: var(--ink) }", "p", &[], &parent.vars);
        assert_eq!(p.ink, Some([0x11, 0x22, 0x33]));
        let child = parent.child(&p);
        assert_eq!(child.vars.len(), 1);
        let sheet = Arc::new(Stylesheet::parse(
            "p::before { content: 'x'; color: var(--ink) }",
            "/s.css",
        ));
        let e = el("p", &[]);
        let path = [Ctx {
            el: &e,
            before: &[],
        }];
        let before = declared_pseudo(&[sheet], &path, PseudoEl::Before, &child.vars);
        assert_eq!(before.and_then(|d| d.ink), Some([0x11, 0x22, 0x33]));
    }

    #[test]
    fn global_keywords_reset_what_lower_rules_set() {
        let css = "p { margin: 2em; color: #ff0000; font-weight: bold; display: none } \
                   p.i { margin: initial; color: inherit; display: revert } \
                   p.u { font-weight: unset; margin-top: unset } p.w { margin: inherit } \
                   p.v { color: var(--none, inherit) }";
        let i = styled(css, "p", &["i"], &none());
        assert_eq!(i.margin, [Some(Len::Px(0.0)); 4]);
        assert_eq!(i.ink, None);
        // Reverted, display keeps what the sheets below it set.
        assert_eq!(i.display, Some(Display::None));
        let u = styled(css, "p", &["u"], &none());
        assert_eq!(u.weight, None);
        assert_eq!(
            (u.margin[0], u.margin[1]),
            (Some(Len::Px(0.0)), Some(Len::Em(2.0)))
        );
        // `inherit` on what doesn't inherit is left alone.
        assert_eq!(
            styled(css, "p", &["w"], &none()).margin[0],
            Some(Len::Em(2.0))
        );
        assert_eq!(styled(css, "p", &["v"], &none()).ink, None);
        // `initial` colour is the default ink, not the parent's.
        let ink = styled("p { color: initial }", "p", &[], &none()).ink;
        assert_eq!(ink, Some([0, 0, 0]));
        // Below every rule, a reset clears what an earlier rule set.
        let merged = Stylesheet::parse("p { color: #ff0000 } p { color: unset }", "/s.css");
        let e = el("p", &[]);
        let path = [Ctx {
            el: &e,
            before: &[],
        }];
        let d = declared(
            &[Arc::new(merged)],
            &path,
            &Decl::default(),
            (None, ""),
            &none(),
        );
        assert_eq!(d.ink, None);
    }

    #[test]
    fn imports_layers_supports_and_media_read_as_css_says() {
        let css = "@charset \"utf-8\";\n@import url(\"base.css\");\n@import 'print.css' print;\n\
                   @import url(wide.css) (min-width: 2000px);\n\
                   @import \"grid.css\" layer(x) supports(display: grid) screen;\n\
                   @import url(\"../fonts.css\") supports(display: run-in);\n\
                   @import url(\"two.css\") supports((display: grid) and (color: red)) (color);\n\
                   @layer base { p.l { text-align: center !important; font-style: italic } }\n\
                   p { text-align: right; font-style: normal } \
                   p.l { text-align: left !important } \
                   @supports (display: grid) { p { font-weight: bold } }\n\
                   @supports not (display: grid) { p { color: #ff0000 } }\n\
                   @media (prefers-color-scheme: dark) { p { color: #ffffff } }\n\
                   @container (min-width: 1px) { p { text-indent: 1em } }";
        let sheet = Stylesheet::parse(css, "/OEBPS/css/s.css");
        assert_eq!(
            sheet.imports,
            [
                "/OEBPS/css/base.css",
                "/OEBPS/css/grid.css",
                "/OEBPS/css/two.css"
            ]
        );
        let p = styled(css, "p", &[], &none());
        assert_eq!(p.weight, Some(Weight::Abs(700)));
        assert_eq!((p.ink, p.indent), (None, Some(Len::Em(1.0))));
        // Rules outside a layer beat those in one, whatever the selector...
        let l = styled(css, "p", &["l"], &none());
        assert_eq!(l.italic, Some(false));
        // ...but a layer's important rules beat the rest's.
        assert_eq!(l.align, Some(TextAlign::Center));
        // Layers rank as first named; important ones reverse.
        let order = "@layer base, theme; @layer theme { p { font-weight: bold; \
                     text-align: right !important } p.n { font-style: italic } } \
                     @layer base { p { font-weight: normal; text-align: center !important } \
                     @layer inner { p.n { font-style: normal } } } \
                     @layer { p { text-indent: 1em } } @layer { p { text-indent: 2em } }";
        let p = styled(order, "p", &[], &none());
        assert_eq!(p.weight, Some(Weight::Abs(700)));
        assert_eq!(p.align, Some(TextAlign::Center));
        assert_eq!(p.indent, Some(Len::Em(2.0)));
        assert_eq!(styled(order, "p", &["n"], &none()).italic, Some(true));
        let tail = Stylesheet::parse("p { color: red }\n@import url(late.css);", "/s.css");
        assert_eq!(tail.imports, ["/late.css"]);
    }
}
