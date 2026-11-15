//! Burmese, Thai or Arabic labels shaped by parley; egui misplaces marks.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use egui::{
    Color32, ColorImage, Context, Id, Painter, Pos2, Rect, Response, RichText, Sense,
    TextureHandle, TextureOptions, Ui, Vec2, WidgetInfo, WidgetType, pos2,
};

/// Past this many drawn labels the cache starts over.
const KEEP: usize = 96;
const CACHE: &str = "shaped-labels";

/// Whether egui's glyph-by-glyph text would garble `text`.
pub fn needs_shaping(text: &str) -> bool {
    text.chars().any(complex)
}

/// Scripts whose letters join, reorder or stack when written.
fn complex(c: char) -> bool {
    matches!(
        u32::from(c),
        // Hebrew, Arabic, Syriac, Thaana, N'Ko and their extensions.
        0x0590..=0x08FF
        // Devanagari to Sinhala, Thai, Lao, Tibetan, Myanmar.
        | 0x0900..=0x109F
        // Khmer, Mongolian, Tai scripts, Balinese and their neighbours.
        | 0x1780..=0x1CFF
        // Syloti Nagri to Myanmar Extended-B and Meetei Mayek.
        | 0xA800..=0xABFF
        // Arabic presentation forms.
        | 0xFB1D..=0xFDFF
        | 0xFE70..=0xFEFF
        // Brahmi and the historic Indic scripts.
        | 0x11000..=0x11FFF
    )
}

/// Shaped text ready to paint: a white mask, tinted when drawn.
#[derive(Clone)]
pub struct Shaped {
    texture: TextureHandle,
    /// In points.
    pub size: Vec2,
    /// Whether lines past the limit were left out.
    pub cut: bool,
}

impl Shaped {
    /// Paints the text with its top-left corner at `at`.
    pub fn paint(&self, painter: &Painter, at: Pos2, color: Color32) {
        let rect = Rect::from_min_size(at, self.size);
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        painter.image(self.texture.id(), rect, uv, color);
    }
}

#[derive(Clone, Default)]
struct Cache {
    labels: HashMap<u64, Option<Shaped>>,
}

/// `text` at `size` points in `color`, wrapping at the width left.
pub fn label(ui: &mut Ui, text: &str, size: f32, color: Color32) -> Response {
    let width = ui.available_width().max(1.0);
    let Some(drawn) = block(ui.ctx(), text, size, width, usize::MAX) else {
        return ui.label(RichText::new(text).size(size).color(color));
    };
    let (rect, response) = ui.allocate_exact_size(drawn.size, Sense::hover());
    drawn.paint(ui.painter(), rect.min, color);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, text));
    response
}

/// One line across the width left, cut at a word; whole on hover.
pub fn line(ui: &mut Ui, text: impl Into<RichText>, size: f32, color: Color32) -> Response {
    let rich: RichText = text.into();
    let raw = rich.text().to_owned();
    let width = ui.available_width().max(1.0);
    let Some(drawn) = block(ui.ctx(), &raw, size, width, 1) else {
        let rich = rich.size(size).color(color);
        return ui.add(egui::Label::new(rich).truncate());
    };
    // Laid right to left, filling the row keeps the text on the left.
    let fill = ui.layout().main_dir() == egui::Direction::RightToLeft;
    let w = if fill { width } else { drawn.size.x };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(w, drawn.size.y), Sense::hover());
    drawn.paint(ui.painter(), rect.min, color);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, &raw));
    if drawn.cut {
        return response.on_hover_text(raw);
    }
    response
}

/// A shaped line at `at` if in view; its height, or `None` for egui's.
pub fn paint_line(
    painter: &Painter,
    text: &str,
    size: f32,
    color: Color32,
    at: Pos2,
    width: f32,
) -> Option<f32> {
    let (extent, _) = measure(painter.ctx(), text, size, width, 1)?;
    if painter
        .clip_rect()
        .intersects(Rect::from_min_size(at, extent))
    {
        let drawn = block(painter.ctx(), text, size, width, 1);
        if let Some(d) = drawn {
            d.paint(painter, at, color);
        }
    }
    Some(extent.y)
}

/// Sizes are small: many more fit before they're forgotten.
const KEEP_SIZES: usize = 4096;
const SIZES: &str = "shaped-sizes";

/// The size `block` would draw and whether it cuts, without drawing.
pub fn measure(
    ctx: &Context,
    text: &str,
    size: f32,
    width: f32,
    lines: usize,
) -> Option<(Vec2, bool)> {
    if !needs_shaping(text) {
        return None;
    }
    let ppp = ctx.pixels_per_point();
    let width = width.max(1.0);
    let key = key_of(text, size, width, ppp, lines);
    let id = Id::new(SIZES);
    type Sizes = HashMap<u64, Option<(Vec2, bool)>>;
    if let Some(hit) = ctx.data(|d| d.get_temp::<Sizes>(id).and_then(|c| c.get(&key).copied())) {
        return hit;
    }
    let made = mareader_reflow::measure_lines(text, size * ppp, width * ppp, lines)
        .map(|(w, h, cut)| (Vec2::new(w as f32, h as f32) / ppp, cut));
    ctx.data_mut(|d| {
        let cache = d.get_temp_mut_or_default::<Sizes>(id);
        if cache.len() >= KEEP_SIZES {
            cache.clear();
        }
        cache.insert(key, made);
    });
    made
}

fn key_of(text: &str, size: f32, width: f32, ppp: f32, lines: usize) -> u64 {
    let mut h = std::hash::DefaultHasher::new();
    (
        text,
        size.to_bits(),
        width.round() as i32,
        ppp.to_bits(),
        lines,
    )
        .hash(&mut h);
    h.finish()
}

/// `text` wrapped at `width` to `lines` lines; `None` when egui draws it.
pub fn block(ctx: &Context, text: &str, size: f32, width: f32, lines: usize) -> Option<Shaped> {
    if !needs_shaping(text) {
        return None;
    }
    let ppp = ctx.pixels_per_point();
    let width = width.max(1.0);
    let key = key_of(text, size, width, ppp, lines);
    let id = Id::new(CACHE);
    if let Some(hit) = ctx.data(|d| {
        d.get_temp::<Cache>(id)
            .and_then(|c| c.labels.get(&key).cloned())
    }) {
        return hit;
    }
    let made = mareader_reflow::label_lines(text, size * ppp, width * ppp, lines).map(|l| {
        let px = l.pixels;
        let pixels = [px.width as usize, px.height as usize];
        let image = ColorImage::from_rgba_premultiplied(pixels, &px.rgba);
        Shaped {
            texture: ctx.load_texture("shaped-label", image, TextureOptions::LINEAR),
            size: Vec2::new(px.width as f32, px.height as f32) / ppp,
            cut: l.cut,
        }
    });
    ctx.data_mut(|d| {
        let cache = d.get_temp_mut_or_default::<Cache>(id);
        if cache.labels.len() >= KEEP {
            cache.labels.clear();
        }
        cache.labels.insert(key, made.clone());
    });
    made
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_scripts_that_shape_take_the_long_way() {
        assert!(needs_shaping("ကျောင်း"), "Burmese");
        assert!(needs_shaping("สวัสดี"), "Thai");
        assert!(needs_shaping("مرحبا"), "Arabic");
        assert!(!needs_shaping("Nevermore"));
        assert!(
            !needs_shaping("日本語の本"),
            "CJK sets one glyph per letter"
        );
        assert!(!needs_shaping("Привет"));
    }
}
