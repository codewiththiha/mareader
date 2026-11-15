//! The keyboard focus ring egui lacks: shown after Tab, hidden by a press.

use egui::{Context, Id, Key, Rect, Stroke, StrokeKind};

use crate::theme::Tokens;

/// The ring's width, as mareader's outline.
pub const RING: f32 = 2.0;

/// How a widget wants its ring drawn, when the button's default won't do.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ring {
    pub radius: f32,
    /// Around the widget, as on words, not just inside its edge.
    pub outside: bool,
    /// How far past the widget's rect it sits, as a card's padded backing.
    pub grow: f32,
}

fn mode() -> Id {
    Id::new("focus-keyboard")
}

/// Call before keys are read. True on Tab, so hidden chrome comes up.
pub fn begin(ctx: &Context) -> bool {
    let (tab, press) = ctx.input(|i| (i.key_pressed(Key::Tab), i.pointer.any_pressed()));
    if tab || press {
        ctx.data_mut(|d| d.insert_temp(mode(), tab && !press));
    }
    tab && !press
}

/// Whether focus came from the keyboard and should be seen.
pub fn keyboard(ctx: &Context) -> bool {
    ctx.data(|d| d.get_temp(mode())).unwrap_or(false)
}

/// Turns the ring on, for keys other than Tab that hand out focus.
pub fn by_key(ctx: &Context) {
    ctx.data_mut(|d| d.insert_temp(mode(), true));
}

/// The focused widget, if its ring shows; `late` allows last pass's place.
fn shown(ctx: &Context, late: bool) -> Option<egui::WidgetRect> {
    if !keyboard(ctx) {
        return None;
    }
    let id = ctx.memory(|m| m.focused())?;
    let w = ctx.viewport(|v| {
        let now = v.this_pass.widgets.get(id);
        now.or_else(|| v.prev_pass.widgets.get(id).filter(|_| late))
            .copied()
    })?;
    // A whole popup or window takes focus too; ringing all of it helps no one.
    (w.layer_id.id != id).then_some(w)
}

/// Where the ring is, or was last frame if not drawn yet.
pub fn rect(ctx: &Context) -> Option<Rect> {
    shown(ctx, true).map(|w| w.rect)
}

/// Whether the ring sits in `area`, which should then stay up.
pub fn within(ctx: &Context, area: Rect) -> bool {
    rect(ctx).is_some_and(|r| area.intersects(r))
}

/// Shapes the widget's ring this frame, if it has focus.
pub fn shape(resp: &egui::Response, ring: Ring) {
    if resp.has_focus() {
        resp.ctx
            .data_mut(|d| d.insert_temp(resp.id.with("ring"), ring));
    }
}

/// Call last: rings the focused widget, on top of its own layer.
pub fn paint(ctx: &Context) {
    // A text field shows its own focus.
    let Some(w) = shown(ctx, false).filter(|_| !ctx.text_edit_focused()) else {
        return;
    };
    let t = Tokens::get(ctx);
    let ring = ctx.data_mut(|d| d.remove_temp::<Ring>(w.id.with("ring")));
    let ring = ring.unwrap_or(Ring {
        radius: t.radius.min(w.rect.height() / 2.0),
        ..Ring::default()
    });
    let (rect, clip) = (w.rect.expand(ring.grow), w.interact_rect.expand(ring.grow));
    let (clip, kind) = match ring.outside {
        true => (clip.expand(RING + 1.0), StrokeKind::Outside),
        false => (clip, StrokeKind::Inside),
    };
    ctx.layer_painter(w.layer_id)
        .with_clip_rect(clip)
        .rect_stroke(rect, ring.radius, Stroke::new(RING, t.accent), kind);
}
