//! Interactive hover/animated region extraction and item rasterization.
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use vello_cpu::kurbo::Affine;
use vello_cpu::peniko::ImageAlphaType;
use vello_cpu::{Pixmap, RenderContext, Resources};

use crate::anim::{HoverDelta, InteractiveItem, ItemPixmap, Transform2d};
use crate::block::{Finish, TextShadow};
use crate::raster::{MAX_SIDE, Painter, Pixels, reach};

use super::{Deco, Flow, Piece, Set, span_height};

/// Extracts all interactive / animated items on one laid-out page.
pub(super) fn extract_page(
    flow_blocks: &[Set],
    pieces: &[Piece],
    decos: &[Deco],
    (origin, fit, turned, page_w): ((f32, f32), f32, bool, f32),
) -> Vec<InteractiveItem> {
    let any_deco = decos.iter().any(|d| d.interaction.is_some());
    let any_piece = pieces.iter().any(|p| {
        flow_blocks
            .get(p.block)
            .is_some_and(|s| s.interaction().is_some())
    });
    if !any_deco && !any_piece {
        return Vec::new();
    }
    let to_shown = |[x, y, w, h]: [f32; 4]| -> [f32; 4] {
        let [ox, oy] = [origin.0, origin.1];
        let [lx, ly] = [ox + x, oy + y];
        if fit != 1.0 {
            [lx * fit, ly * fit, w * fit, h * fit]
        } else if turned {
            [page_w - ly - h, lx, h, w]
        } else {
            [lx, ly, w, h]
        }
    };
    let mut items: Vec<InteractiveItem> = Vec::new();
    let mut by_box: HashMap<usize, usize> = HashMap::new();
    collect_interactive_decos(decos, fit, &mut items, &mut by_box);
    collect_interactive_pieces(pieces, flow_blocks, fit, &mut items, &by_box);
    for item in &mut items {
        item.rect = to_shown(item.laid_rect);
    }
    items
}

fn collect_interactive_decos(
    decos: &[Deco],
    fit: f32,
    items: &mut Vec<InteractiveItem>,
    by_box: &mut HashMap<usize, usize>,
) {
    for (di, deco) in decos.iter().enumerate() {
        let Some(interaction) = deco.interaction.as_ref().filter(|i| !i.is_empty()) else {
            continue;
        };
        let key = deco.box_key;
        if let Some(&idx) = by_box.get(&key).filter(|_| key != 0) {
            if let Some(item) = items.get_mut(idx) {
                item.deco_indices.push(di);
                item.laid_rect = union_rect(item.laid_rect, deco.rect);
            }
            continue;
        }
        let id = if key != 0 {
            key as u64
        } else {
            (di as u64) | (1u64 << 60)
        };
        if key != 0 {
            by_box.insert(key, items.len());
        }
        items.push(InteractiveItem {
            id,
            rect: [0.0; 4],
            z: deco.z,
            em: deco.em * fit,
            base_transform: deco.transform.unwrap_or(Transform2d::IDENTITY),
            base_opacity: deco.finish.as_deref().map_or(1.0, |f| f.opacity),
            interaction: Arc::clone(interaction),
            deco_indices: vec![di],
            piece_indices: Vec::new(),
            laid_rect: deco.rect,
            laid_em: deco.em,
        });
    }
}

fn collect_interactive_pieces(
    pieces: &[Piece],
    flow_blocks: &[Set],
    fit: f32,
    items: &mut Vec<InteractiveItem>,
    by_box: &HashMap<usize, usize>,
) {
    for (pi, piece) in pieces.iter().enumerate() {
        let Some(set) = flow_blocks.get(piece.block) else {
            continue;
        };
        for b in &set.boxes {
            if let Some(&idx) = by_box.get(&b.key)
                && let Some(item) = items.get_mut(idx)
            {
                item.piece_indices.push(pi);
                item.laid_rect = union_rect(item.laid_rect, piece_rect(set, piece));
            }
        }
        let Some(interaction) = set.interaction().filter(|i| !i.is_empty()) else {
            continue;
        };
        let pr = piece_rect(set, piece);
        if pr[2] > 0.5 && pr[3] > 0.5 {
            let id = ((set.source as u64) << 16) ^ (pi as u64) ^ (1u64 << 61);
            items.push(InteractiveItem {
                id,
                rect: [0.0; 4],
                z: set.spot.as_deref().map_or(1, |(_, s)| s.z + 1),
                em: set.em * fit,
                base_transform: set.transform().unwrap_or(Transform2d::IDENTITY),
                base_opacity: 1.0,
                interaction: Arc::clone(interaction),
                deco_indices: Vec::new(),
                piece_indices: vec![pi],
                laid_rect: pr,
                laid_em: set.em,
            });
        }
    }
}

fn piece_rect(set: &Set, piece: &Piece) -> [f32; 4] {
    let top = set.units.get(piece.units.start).map_or(0.0, |u| u.0) + piece.dy;
    let h = span_height(set, piece.units.clone());
    match &set.draw {
        super::Draw::Image { rect, .. } => [
            set.x + piece.dx + rect[0],
            piece.dy + rect[1],
            rect[2],
            rect[3],
        ],
        _ => [set.x + piece.dx, top, set.width, h],
    }
}

fn union_rect(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    if a[2] <= 0.0 || a[3] <= 0.0 {
        return b;
    }
    if b[2] <= 0.0 || b[3] <= 0.0 {
        return a;
    }
    let x0 = a[0].min(b[0]);
    let y0 = a[1].min(b[1]);
    let x1 = (a[0] + a[2]).max(b[0] + b[2]);
    let y1 = (a[1] + a[3]).max(b[1] + b[3]);
    [x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0)]
}

impl Flow {
    /// Interactive or animated elements laid out on `page`.
    pub fn interactive(&self, page: usize) -> &[InteractiveItem] {
        self.interactive.get(page).map_or(&[][..], Vec::as_slice)
    }

    /// Renders a single interactive item at `scale` (in shown page pixels/pt),
    /// applying its `:hover` visual overrides when `hovered` is true.
    pub fn render_item(
        &self,
        page: usize,
        item_idx: usize,
        scale: f32,
        hovered: bool,
    ) -> Option<ItemPixmap> {
        let item = self.interactive.get(page)?.get(item_idx)?;
        let pieces = self.pages.get(page)?;
        let decos = self.decos.get(page)?;
        if !scale.is_finite() || scale <= 0.0 {
            return None;
        }
        let fit = self.fit(page);
        let draw_scale = scale * fit;
        let hover = hovered.then(|| item.interaction.hover.as_ref()).flatten();
        let pad = item_bleed(item, decos, hover);
        let [lx, ly, lw, lh] = item.laid_rect;
        let padded = [lx - pad, ly - pad, lw + 2.0 * pad, lh + 2.0 * pad];
        let w = (padded[2] * draw_scale).ceil().clamp(1.0, MAX_SIDE as f32) as u32;
        let h = (padded[3] * draw_scale).ceil().clamp(1.0, MAX_SIDE as f32) as u32;
        let (w16, h16) = (u16::try_from(w).ok()?, u16::try_from(h).ok()?);
        let (ox, oy) = self.origin(page);
        let pixels = catch_unwind(AssertUnwindSafe(|| {
            let s = f64::from(draw_scale);
            let view = Affine::scale(s)
                * Affine::translate((-f64::from(padded[0]), -f64::from(padded[1])));
            let mut p = Painter::new(
                RenderContext::new(w16, h16),
                Resources::new(),
                view,
                draw_scale,
            );
            for &di in &item.deco_indices {
                if let Some(base) = decos.get(di) {
                    let styled = apply_hover_deco(base, hover);
                    p.deco(&styled, &self.decoded);
                }
            }
            // Standalone pieces (like `img:hover`) can also gain hover shadow.
            if item.deco_indices.is_empty()
                && let Some(h) = hover
                && (!h.shadows.is_empty() || h.fill.is_some() || h.border_colour.is_some())
            {
                let synth = synthetic_hover_deco(item, h);
                p.deco(&synth, &self.decoded);
            }
            let text_override = hover.and_then(|h| {
                let col = h.text_colour.map(|[r, g, b, _]| [r, g, b]);
                (col.is_some() || h.text_shadow.is_some()).then_some((col, h.text_shadow))
            });
            for &pi in &item.piece_indices {
                if let Some(piece) = pieces.get(pi)
                    && let Some(set) = self.blocks.get(piece.block)
                {
                    p.piece_hovered(
                        set,
                        piece,
                        &self.decoded,
                        || self.laid(piece.block),
                        text_override,
                    );
                }
            }
            let mut pixmap = Pixmap::new(w16, h16);
            p.finish(&mut pixmap);
            Pixels {
                width: w,
                height: h,
                rgba: pixmap.take_rgba8(ImageAlphaType::AlphaPremultiplied),
            }
        }))
        .ok()?;
        let shown = self.shown_rect(page, [ox + padded[0], oy + padded[1], padded[2], padded[3]]);
        Some(ItemPixmap {
            pixels,
            rect: shown,
        })
    }
}

fn item_bleed(item: &InteractiveItem, decos: &[Deco], hover: Option<&HoverDelta>) -> f32 {
    let mut pad = 2.0f32;
    for &di in &item.deco_indices {
        if let Some(f) = decos.get(di).and_then(|d| d.finish.as_deref()) {
            pad = pad.max(reach(f, item.laid_em) + 2.0);
        }
    }
    if let Some(h) = hover {
        for s in &h.shadows {
            if !s.inset {
                let spread = s.at[0].abs().max(s.at[1].abs()) + s.spread.max(0.0);
                let r = (spread + s.blur.max(0.0) * 1.6) * item.laid_em;
                pad = pad.max(r + 2.0);
            }
        }
    }
    pad.min(128.0)
}

fn apply_hover_deco(base: &Deco, hover: Option<&HoverDelta>) -> Deco {
    let mut d = base.clone();
    // Base transform is applied by the GPU compositor during animation.
    d.transform = None;
    let Some(h) = hover else {
        return d;
    };
    if let Some(fill) = h.fill {
        d.fill_rgba = Some(fill);
        d.fill = None;
    }
    if let Some([r, g, b, _]) = h.border_colour {
        d.colours = [Some([r, g, b]); 4];
    }
    if !h.shadows.is_empty() {
        let mut finish = d.finish.as_deref().cloned().unwrap_or_default();
        finish.shadows = h.shadows.to_vec();
        d.finish = Some(Arc::new(finish));
    }
    d
}

fn synthetic_hover_deco(item: &InteractiveItem, h: &HoverDelta) -> Deco {
    let finish = (!h.shadows.is_empty()).then(|| {
        Arc::new(Finish {
            shadows: h.shadows.to_vec(),
            ..Finish::default()
        })
    });
    Deco {
        rect: item.laid_rect,
        border: [0.0; 4],
        edges: [crate::block::Edge::None; 4],
        colours: [None; 4],
        fill: None,
        fill_rgba: h.fill,
        backdrop: None,
        em: item.laid_em,
        radius: 0.0,
        finish,
        continued: [false, false],
        box_key: 0,
        clip: false,
        transform: None,
        interaction: None,
        z: item.z,
    }
}

/// Optional text colour & shadow override used during hover item rasterization.
pub(crate) type TextHoverOverride = Option<(Option<[u8; 3]>, Option<TextShadow>)>;
