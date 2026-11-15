//! Page painting, tile grid rendering, band clipping, and corner clearing.

use super::*;

/// Glyphs may overhang the column a little: a hung hyphen, an italic.
const STREAM_SLACK: f32 = 24.0;
/// What a revealed hit keeps clear at the top: the find bar and its air.
pub(crate) const FIND_CLEARANCE: f32 = 72.0;

/// A tiled page's grid, drawn over its backdrop where tiles have arrived.
pub(super) struct Tiles {
    /// What of the page shows: a stream cuts it to its band.
    pub clip: Rect,
    pub page: Rect,
    pub anchor: Anchor,
    pub tiling: Tiling,
    /// The tiles' scale; its part is set per tile.
    pub key: PageKey,
}

impl Tiles {
    /// Draws the tiles on screen, asking for missing ones if `ask`.
    pub(super) fn paint(
        &self,
        ui: &Ui,
        pages: &mut PageCache,
        mut ask: Option<(&mut Demand, Urgency)>,
        (quads, tone): (&mut Vec<Quad>, PageTone),
    ) {
        let t = &self.tiling;
        let size = vec2(t.width as f32, t.height as f32);
        let to_screen = self.page.size() / size;
        let clip = self.clip;
        for row in 0..t.rows() {
            for col in 0..t.cols() {
                let Some([x, y, w, h]) = t.rect(col, row) else {
                    continue;
                };
                let at = vec2(x as f32, y as f32) * to_screen;
                let rect =
                    Rect::from_min_size(self.page.min + at, vec2(w as f32, h as f32) * to_screen);
                if !rect.intersects(clip) {
                    continue;
                }
                let (Ok(col), Ok(row)) = (u8::try_from(col), u8::try_from(row)) else {
                    continue;
                };
                let key = PageKey {
                    part: Part::Tile { col, row },
                    ..self.key
                };
                match pages.lookup_tile(key) {
                    Some(PageTexture::Gpu(page)) => {
                        let quad = Quad::page((rect, clip), Some(page), tone, self.anchor);
                        quads.push(quad);
                    }
                    Some(PageTexture::Egui(h)) => {
                        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                        let painter = ui.painter().with_clip_rect(clip);
                        painter.image(h.id(), rect, uv, Color32::WHITE);
                    }
                    None => {
                        if let Some((demand, urgency)) = ask.as_mut()
                            && !pages.touch(key)
                        {
                            demand.push((*urgency, key.into()));
                        }
                    }
                }
            }
        }
    }
}

/// The whole page placed so `band` fills `cut`, and a clip to the band.
pub(super) fn streamed(
    cut: Rect,
    band: &Band,
    size: Option<&PageSize>,
    zoom: f32,
    ppp: f32,
) -> (Rect, Rect) {
    let size = size.map_or(vec2(band.width, band.height), |s| vec2(s.width, s.height));
    let min = cut.min - vec2(band.x, band.y) * zoom;
    let page = Rect::from_min_size(min, size * zoom).round_to_pixels(ppp);
    (page, cut.expand2(vec2(STREAM_SLACK * zoom, 0.0)))
}

/// What a page shows before it is drawn: a blank page, themed.
pub(crate) fn blank(t: &Tokens) -> Color32 {
    let [r, g, b] = t.page.blank().map(|c| (c * 255.0).round() as u8);
    Color32::from_rgb(r, g, b)
}

/// Painters for the parts of `clip` clear of `frame`'s round corners.
fn clear_of_corners(ui: &Ui, clip: Rect, (frame, radius): (Rect, f32)) -> Vec<egui::Painter> {
    if radius <= 0.0 {
        return vec![ui.painter().with_clip_rect(clip)];
    }
    let (f, r) = (frame, radius);
    let (top, bottom) = (f.top() + r, f.bottom() - r);
    let (left, right) = (f.left() + r, f.right() - r);
    [
        Rect::from_min_max(pos2(left, f.top()), pos2(right, top)),
        Rect::from_min_max(pos2(f.left(), top), pos2(f.right(), bottom)),
        Rect::from_min_max(pos2(left, bottom), pos2(right, f.bottom())),
    ]
    .into_iter()
    .map(|band| band.intersect(clip))
    .filter(|band| band.is_positive())
    .map(|band| ui.painter().with_clip_rect(band))
    .collect()
}

/// Draws `quads` from `first` again over `pictures` in `tone`, undimmed.
pub(crate) fn keep_pictures(
    quads: &mut Vec<Quad>,
    first: usize,
    pictures: impl Iterator<Item = Rect>,
    tone: PageTone,
) {
    let end = quads.len();
    for picture in pictures {
        for idx in first..end {
            let next = quads
                .get(idx)
                .filter(|q| q.texture.is_some())
                .and_then(|q| {
                    let clip = q.clip.intersect(picture);
                    clip.is_positive().then(|| Quad {
                        clip,
                        tone,
                        ..q.clone()
                    })
                });
            if let Some(quad) = next {
                quads.push(quad);
            }
        }
    }
}

/// Paints a page, or queues it in `quads`; nothing crosses `round`'s corners.
pub(crate) fn paint_page(
    ui: &Ui,
    ((rect, clip), anchor, round): ((Rect, Rect), Anchor, (Rect, f32)),
    look: Lookup,
    (t, shadow): (&Tokens, bool),
    quads: &mut Vec<Quad>,
) {
    // Shadows reach past the page; a cut page casts none.
    let painter = ui.painter().with_clip_rect(clip);
    let shade = clear_of_corners(ui, ui.clip_rect(), round);
    let bands = clear_of_corners(ui, clip, round);
    // Themed pages match the gutter's paper; only this outlines them.
    if shadow {
        let lift = |offset, blur, alpha| egui::Shadow {
            offset: [0, offset],
            blur,
            spread: 0,
            color: Color32::from_black_alpha(alpha),
        };
        // mareader's `0 1px 2px` and `0 8px 24px -6px` box shadows.
        for band in &shade {
            band.add(lift(1, 2, 31).as_shape(rect, CornerRadius::ZERO));
            band.add(lift(8, 24, 56).as_shape(rect.shrink(6.0), CornerRadius::ZERO));
        }
    }
    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    match look {
        Lookup::Sharp(PageTexture::Gpu(page)) | Lookup::Stale(PageTexture::Gpu(page)) => {
            quads.push(Quad::page((rect, clip), Some(page), t.page, anchor));
        }
        // Without a GPU there is no page pass, so pages stay unthemed.
        Lookup::Sharp(PageTexture::Egui(h)) | Lookup::Stale(PageTexture::Egui(h)) => {
            painter.image(h.id(), rect, uv, Color32::WHITE);
        }
        // A blank page, themed and textured; the fill is the no-GPU fallback.
        Lookup::Missing => {
            for band in &bands {
                band.rect_filled(rect, 0.0, blank(t));
            }
            quads.push(Quad::page((rect, clip), None, t.page, anchor));
        }
        Lookup::Failed => {
            for band in &bands {
                band.rect_filled(rect, 0.0, blank(t));
            }
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "This page couldn't be drawn",
                egui::FontId::proportional(13.0),
                Color32::from_gray(110),
            );
        }
    }
}
