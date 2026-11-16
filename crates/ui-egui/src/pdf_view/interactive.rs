//! Live CSS `:hover` transitions and `@keyframes` overlay compositor.
use std::collections::HashMap;

use egui::epaint::Vertex;
use egui::{Color32, ColorImage, Pos2, Rect, Shape, TextureHandle, TextureOptions, Ui, pos2, vec2};
use mareader_appearance::PageTone;
use mareader_session::{DocId, InteractiveItem, Transform2d};

use super::ShowContext;
use crate::selection::Sheet;

/// Maximum cached item textures per pane before pruning.
const MAX_TEXTURES: usize = 32;
/// Maximum step delta (seconds) so tab switches do not jump mid-transition.
const MAX_DT: f64 = 0.05;

/// Per-pane runtime state for interactive/animated EPUB elements.
#[derive(Debug, Default, Clone, PartialEq)]
pub(super) struct InteractiveState {
    doc: Option<DocId>,
    tone: Option<PageTone>,
    started_at: Option<f64>,
    tracks: HashMap<(usize, u64), Track>,
    textures: HashMap<(usize, u64, bool, u16), CachedTex>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Track {
    progress: f32,
    delay_used: f32,
    want: bool,
    last_t: f64,
}

#[derive(Clone)]
struct CachedTex {
    tex: TextureHandle,
    /// Placement rect `[x, y, w, h]` in shown page points.
    rect_pt: [f32; 4],
}

impl std::fmt::Debug for CachedTex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CachedTex")
            .field("id", &self.tex.id())
            .field("rect_pt", &self.rect_pt)
            .finish()
    }
}

impl PartialEq for CachedTex {
    fn eq(&self, other: &Self) -> bool {
        self.tex.id() == other.tex.id() && self.rect_pt == other.rect_pt
    }
}

/// Evaluates hover/animation tracks and paints active item overlays.
pub(super) fn paint(cx: &mut ShowContext<'_>, ui: &mut Ui) {
    let doc_id = cx.place.doc;
    let state = &mut cx.view.interactive;
    if state.doc != Some(doc_id) {
        state.doc = Some(doc_id);
        state.started_at = None;
        state.tracks.clear();
        state.textures.clear();
    }
    let Some(input) = cx.input.as_ref() else {
        return;
    };
    let (req_scale, tone) = (input.scale.value(), input.picture_tone);
    if state.tone != Some(tone) {
        state.tone = Some(tone);
        state.textures.clear();
    }
    let now = ui.ctx().input(|i| i.time);
    let started = *state.started_at.get_or_insert(now);
    let pointer = ui.ctx().input(|i| i.pointer.hover_pos());

    let mut need_repaint = false;
    let mut draws: Vec<DrawOverlay> = Vec::new();
    let sheets = cx.paint.sheets.clone();
    for sheet in &sheets {
        let items = cx.doc.page_interactive(sheet.page);
        if items.is_empty() {
            continue;
        }
        need_repaint |= collect_sheet(state, sheet, items, (pointer, now, started), &mut draws);
    }

    draws.sort_by_key(|d| (d.z, d.idx));
    for d in draws {
        let key = (d.page, d.idx, d.id);
        let Some((tex, rect_pt)) = ensure_texture(cx, ui, key, d.use_hover_tex, (req_scale, tone))
        else {
            continue;
        };
        paint_mesh(ui, &d, &tex, rect_pt);
    }
    if need_repaint {
        ui.ctx().request_repaint();
    }
}

fn collect_sheet(
    state: &mut InteractiveState,
    sheet: &Sheet,
    items: &[InteractiveItem],
    (pointer, now, started): (Option<Pos2>, f64, f64),
    draws: &mut Vec<DrawOverlay>,
) -> bool {
    let mut need_repaint = false;
    let pointer_pt = pointer
        .filter(|p| sheet.shown.contains(*p))
        .map(|p| (p - sheet.rect.min) / sheet.scale.max(0.001));
    let hovered_id = pointer_pt.and_then(|pt| hit_test(items, pt.x, pt.y));
    for (idx, item) in items.iter().enumerate() {
        let is_hovered = hovered_id == Some(item.id);
        let (eased, transitioning) =
            advance_track(state, (sheet.page, item.id), item, is_hovered, now);
        let (anim_tf, anim_alpha, anim_active) = sample_anim(item, (now - started) as f32);
        need_repaint |= transitioning || anim_active;
        if eased <= 0.001 && !anim_active {
            continue;
        }
        let hover = item.interaction.hover.as_ref();
        let hover_tf = hover
            .and_then(|h| h.transform.as_ref())
            .map_or(item.base_transform, |h| item.base_transform.lerp(h, eased));
        let hover_alpha = hover
            .and_then(|h| h.opacity)
            .map_or(item.base_opacity, |h| {
                item.base_opacity + (h - item.base_opacity) * eased
            });
        let z = if is_hovered {
            item.z.saturating_add(1000)
        } else {
            item.z
        };
        draws.push(DrawOverlay {
            page: sheet.page,
            idx,
            id: item.id,
            z,
            sheet_min: sheet.rect.min,
            sheet_scale: sheet.scale,
            clip: sheet.shown,
            rect: item.rect,
            em: item.em,
            tf: if anim_active { anim_tf } else { hover_tf },
            alpha: (if anim_active { anim_alpha } else { hover_alpha }).clamp(0.0, 1.0),
            use_hover_tex: eased > 0.15,
        });
    }
    need_repaint
}

struct DrawOverlay {
    page: usize,
    idx: usize,
    id: u64,
    z: i32,
    sheet_min: Pos2,
    sheet_scale: f32,
    clip: Rect,
    rect: [f32; 4],
    em: f32,
    tf: Transform2d,
    alpha: f32,
    use_hover_tex: bool,
}

fn hit_test(items: &[InteractiveItem], x: f32, y: f32) -> Option<u64> {
    let mut best: Option<(i32, usize, u64)> = None;
    for (idx, item) in items.iter().enumerate() {
        let [rx, ry, rw, rh] = item.rect;
        if x >= rx && x <= rx + rw && y >= ry && y <= ry + rh {
            let cand = (item.z, idx, item.id);
            if best.is_none_or(|prev| (cand.0, cand.1) >= (prev.0, prev.1)) {
                best = Some(cand);
            }
        }
    }
    best.map(|(_, _, id)| id)
}

fn advance_track(
    state: &mut InteractiveState,
    key: (usize, u64),
    item: &InteractiveItem,
    hovered: bool,
    now: f64,
) -> (f32, bool) {
    if !item.interaction.has_hover() {
        return (0.0, false);
    }
    let tr = item.interaction.transition;
    let dur = tr.map_or(0.0, |t| t.duration_ms as f32 / 1000.0);
    let delay = tr.map_or(0.0, |t| t.delay_ms as f32 / 1000.0);
    let Some(track) = state.tracks.get_mut(&key) else {
        if !hovered {
            return (0.0, false);
        }
        let instant = dur <= 0.001 && delay <= 0.001;
        let p = if instant { 1.0 } else { 0.0 };
        state.tracks.insert(
            key,
            Track {
                progress: p,
                delay_used: 0.0,
                want: true,
                last_t: now,
            },
        );
        return (p, !instant);
    };
    let dt = (now - track.last_t).clamp(0.0, MAX_DT) as f32;
    track.last_t = now;
    if track.want != hovered {
        track.want = hovered;
        track.delay_used = 0.0;
    }
    if dur <= 0.001 && delay <= 0.001 {
        track.progress = if hovered { 1.0 } else { 0.0 };
    } else if track.delay_used < delay {
        track.delay_used = (track.delay_used + dt).min(delay);
    } else if dur > 0.001 {
        let step = dt / dur;
        track.progress = if hovered {
            (track.progress + step).min(1.0)
        } else {
            (track.progress - step).max(0.0)
        };
    } else {
        track.progress = if hovered { 1.0 } else { 0.0 };
    }
    let target = if hovered { 1.0 } else { 0.0 };
    let moving = (track.progress - target).abs() > 0.001;
    let eased = tr.map_or(track.progress, |t| t.easing.eval(track.progress));
    if !hovered && track.progress <= 0.001 {
        state.tracks.remove(&key);
    }
    (eased, moving)
}

fn sample_anim(item: &InteractiveItem, elapsed: f32) -> (Transform2d, f32, bool) {
    let Some(anim) = item.interaction.animation.as_ref() else {
        return (item.base_transform, item.base_opacity, false);
    };
    let s = anim.sample(elapsed);
    (
        s.transform.unwrap_or(item.base_transform),
        s.opacity.unwrap_or(item.base_opacity),
        s.active,
    )
}

fn ensure_texture(
    cx: &mut ShowContext<'_>,
    ui: &Ui,
    (page, idx, id): (usize, usize, u64),
    hovered: bool,
    (req_scale, tone): (f32, PageTone),
) -> Option<(TextureHandle, [f32; 4])> {
    let scale_key = (req_scale * 100.0).round().clamp(10.0, 60000.0) as u16;
    let key = (page, id, hovered, scale_key);
    if let Some(hit) = cx.view.interactive.textures.get(&key) {
        return Some((hit.tex.clone(), hit.rect_pt));
    }
    if cx.view.interactive.textures.len() >= MAX_TEXTURES {
        cx.view.interactive.textures.clear();
    }
    let mut pix = cx
        .doc
        .render_interactive_item(page, idx, req_scale, hovered)?;
    tone_pixels(&mut pix.pixels.rgba, tone);
    let size = [pix.pixels.width as usize, pix.pixels.height as usize];
    let image = ColorImage::from_rgba_premultiplied(size, &pix.pixels.rgba);
    let name = format!("epub-item-{page}-{id}-{hovered}");
    let tex = ui.ctx().load_texture(name, image, TextureOptions::LINEAR);
    let rect_pt = pix.rect;
    cx.view.interactive.textures.insert(
        key,
        CachedTex {
            tex: tex.clone(),
            rect_pt,
        },
    );
    Some((tex, rect_pt))
}

fn tone_pixels(rgba: &mut [u8], tone: PageTone) {
    if tone.blend.is_none() {
        return;
    }
    for px in rgba.as_chunks_mut::<4>().0 {
        let [r, g, b, a] = *px;
        if a == 0 {
            continue;
        }
        let af = f32::from(a) / 255.0;
        let straight = [r, g, b].map(|c| (f32::from(c) / 255.0 / af).clamp(0.0, 1.0));
        let out = tone.apply(straight);
        let to_u8 = |v: f32| (v.clamp(0.0, 1.0) * af * 255.0).round() as u8;
        px[0] = to_u8(out[0]);
        px[1] = to_u8(out[1]);
        px[2] = to_u8(out[2]);
    }
}

fn paint_mesh(ui: &Ui, d: &DrawOverlay, tex: &TextureHandle, rect_pt: [f32; 4]) {
    let m = d.tf.affine_coeffs(d.rect, d.em);
    let [x0, y0, w, h] = rect_pt;
    let map = |cx: f32, cy: f32| -> Pos2 {
        let px = (m[0] * f64::from(cx) + m[2] * f64::from(cy) + m[4]) as f32;
        let py = (m[1] * f64::from(cx) + m[3] * f64::from(cy) + m[5]) as f32;
        d.sheet_min + vec2(px * d.sheet_scale, py * d.sheet_scale)
    };
    let a = (d.alpha * 255.0).round().clamp(0.0, 255.0) as u8;
    let color = Color32::from_rgba_premultiplied(a, a, a, a);
    let mut mesh = egui::Mesh::with_texture(tex.id());
    let corners = [
        (map(x0, y0), pos2(0.0, 0.0)),
        (map(x0 + w, y0), pos2(1.0, 0.0)),
        (map(x0 + w, y0 + h), pos2(1.0, 1.0)),
        (map(x0, y0 + h), pos2(0.0, 1.0)),
    ];
    for (pos, uv) in corners {
        mesh.vertices.push(Vertex { pos, uv, color });
    }
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    ui.painter().with_clip_rect(d.clip).add(Shape::mesh(mesh));
}
