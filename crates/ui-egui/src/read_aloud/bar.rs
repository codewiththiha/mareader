//! Floating read-aloud player bar layout, morphing, parking, and prompts.

use egui::{Context, Id, Pos2, Rect, Ui};
use mareader_engine::read_aloud::player::{Act, Lyrics, View};
use mareader_engine::read_aloud::state::{About, State as ReadAloudState, remember_skip};
use mareader_settings::{Player as Look, ReadAloudSettings};

use crate::motion;
use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent, UiCommand};
use crate::z_order::{self, Tier};

use super::bar_layout::place_bar;
use super::control::Fit;
use super::edges::Resize;
use super::orb::show_orb;
use super::{APPEAR, GLIDE, HIDDEN, MORPH, ReadAloudUi, Shape, Stage, Tint, ask, card, control};
use super::{ease_in_out, ease_out, edges};

/// Per-frame animated morph, anchor, and colour state for the player bar.
pub(super) struct Motion {
    pub pane: Option<Rect>,
    pub room: Rect,
    pub k: f32,
    pub f: f32,
    pub shown: f32,
    pub at: Pos2,
    pub morph: f32,
    pub colours: Tokens,
}

/// Resolved bar rect, resize bounds, and parking flags for one frame.
pub(super) struct Placement {
    pub rect: Rect,
    pub width: f32,
    pub span: (f32, f32, f32),
    pub orb_k: f32,
    pub parked: bool,
    pub tucked_rest: bool,
}

impl ReadAloudUi {
    /// The player: a pill by its pane, or a card when the words are away.
    pub fn bar(
        &mut self,
        ctx: &Context,
        state: &ReadAloudState,
        stage: Stage<'_>,
        settings: &mut ReadAloudSettings,
        out: &mut Commands,
        cover: &mut dyn FnMut(&Ui, Rect) -> bool,
    ) -> Option<Act> {
        if !state.active() {
            self.shape = Shape::default();
            return None;
        }
        let shape = &mut self.shape;
        let now = ctx.input(|i| i.time);
        let m = advance_motion(ctx, shape, state, &stage, settings, now);
        let act = m.colours.lend(ctx, || {
            render_bar(ctx, shape, state, &m, (&stage, settings, out, cover))
        });
        match act {
            Some(Act::Card(to)) => {
                shape.chosen = Some(to);
                None
            }
            Some(Act::Reveal) => {
                out.push(Sent::Ui(UiCommand::ShowReading));
                None
            }
            other => other,
        }
    }
}

/// Updates drift, card/full morph, glide anchor, and tint animation.
fn advance_motion(
    ctx: &Context,
    shape: &mut Shape,
    state: &ReadAloudState,
    stage: &Stage<'_>,
    settings: &ReadAloudSettings,
    now: f64,
) -> Motion {
    let home = state.home();
    let pane = (stage.panes.iter())
        .find(|(p, _)| stage.reader && Some(*p) == home)
        .map(|(_, r)| *r);
    if stage.shown == Some(true) {
        shape.highlight_seen = true;
    }
    match stage.shown {
        Some(false) => _ = *shape.hidden_since.get_or_insert(now),
        _ => shape.hidden_since = None,
    }
    // The page shows the words unless they are elsewhere or drift.
    let drifted = shape.hidden_since.is_some_and(|t| now - t >= HIDDEN);
    let auto = pane.is_none() || !settings.follow || drifted;
    if auto != shape.auto {
        shape.auto = auto;
        shape.chosen = None;
    }
    let room = pane.unwrap_or(stage.content);
    let style = settings.player;
    if shape.last_look.replace(style).is_some_and(|l| l != style) {
        shape.player_priority = true;
        shape.unparked_until = None;
    }
    let fits = room.height() >= card::COMPACT_HEIGHT + 80.0 && room.width() >= 300.0;
    let roomy = room.height() >= card::HEIGHT + 120.0 && room.width() >= 360.0;
    let wanted = shape.chosen.unwrap_or(auto && style != Look::Bar);
    if !shape.card && wanted && fits && drifted && shape.highlight_seen {
        shape.player_priority = true;
        shape.unparked_until = None;
    }
    shape.card = wanted && fits;
    let morph = motion::secs(ctx, MORPH);
    let k = ease_in_out(ctx.animate_bool_with_time(Id::new("read-aloud-card"), shape.card, morph));
    let full = style == Look::Full && roomy;
    let f = ease_in_out(ctx.animate_bool_with_time(Id::new("read-aloud-full"), full, morph));
    let shown = advance_appear(ctx, shape, now);
    let at = advance_anchor(ctx, shape, pane, room, stage.reader, now);
    let colours = advance_tint(ctx, shape, stage.tint, now, morph);
    Motion {
        pane,
        room,
        k,
        f,
        shown,
        at,
        morph,
        colours,
    }
}

fn advance_appear(ctx: &Context, shape: &mut Shape, now: f64) -> f32 {
    let just_appeared = shape.appeared.is_none();
    let since = now - *shape.appeared.get_or_insert(now);
    if just_appeared {
        shape.unparked_until = Some(now + 4.0);
    }
    let shown = match motion::enabled(ctx) {
        true => ease_out((since / APPEAR).clamp(0.0, 1.0)) as f32,
        false => 1.0,
    };
    if shown < 1.0 {
        ctx.request_repaint();
    }
    shown
}

fn advance_anchor(
    ctx: &Context,
    shape: &mut Shape,
    pane: Option<Rect>,
    room: Rect,
    reader: bool,
    now: f64,
) -> Pos2 {
    // It sits over the page bar in a pane, else near the window's foot.
    let foot = match pane.is_some() || reader {
        true => room.bottom() - crate::page_bar::BAR_H - 12.0,
        false => room.bottom() - 20.0,
    };
    let goal = egui::pos2(room.center().x, foot);
    let dt = shape.time.map_or(0.0, |t| (now - t) as f32).min(0.1);
    shape.time = Some(now);
    let at = match shape.at {
        Some(at) if motion::enabled(ctx) => at + (goal - at) * (1.0 - (-dt * GLIDE).exp()),
        _ => goal,
    };
    shape.at = Some(at);
    if (goal - at).length() > 0.5 {
        ctx.request_repaint();
    }
    at
}

fn advance_tint(
    ctx: &Context,
    shape: &mut Shape,
    target: Option<Tokens>,
    now: f64,
    morph: f32,
) -> Tokens {
    let goal = target.unwrap_or_else(|| Tokens::get(ctx));
    let tint = shape.tint.get_or_insert(Tint {
        from: goal,
        to: goal,
        since: now,
    });
    if tint.to != goal {
        let from = tint.at(now, f64::from(morph));
        *tint = Tint {
            from,
            to: goal,
            since: now,
        };
    }
    let colours = tint.at(now, f64::from(morph));
    if colours != goal {
        ctx.request_repaint();
    }
    colours
}

type RenderEnv<'a> = (
    &'a Stage<'a>,
    &'a mut ReadAloudSettings,
    &'a mut Commands,
    &'a mut dyn FnMut(&Ui, Rect) -> bool,
);

/// Builds the frame view, computes bar placement, and paints bar/orb/ask UI.
fn render_bar(
    ctx: &Context,
    shape: &mut Shape,
    state: &ReadAloudState,
    m: &Motion,
    (stage, settings, out, cover): RenderEnv<'_>,
) -> Option<Act> {
    let now = ctx.input(|i| i.time);
    let hint = |share: f32| state.hint(share);
    let cost = state.ask_cost(settings.rate);
    let view = state.view(&hint)?;
    let estimate = mareader_engine::read_aloud::countdown::Estimate {
        left: view.left,
        at: view.at,
        playing: view.playing,
        rate: settings.rate,
    };
    let left = shape.countdown.show(estimate, now);
    let view = View { left, ..view };
    let about = state.about(stage.title)?;
    let lyrics = state.lyrics();
    let place = place_bar(ctx, shape, m, stage, settings, (now, view.escape.is_some()));
    let (mut act, resize) = if shape.orb_target.is_none() {
        paint_bar_area(
            ctx,
            shape,
            (&place, m, &view, &about, lyrics.as_ref()),
            (stage, settings, out, cover),
        )
    } else if let Some(target) = shape.orb_target {
        let k = ease_in_out(place.orb_k);
        let orb_rect = Rect::from_min_max(
            place.rect.min.lerp(target.min, k),
            place.rect.max.lerp(target.max, k),
        );
        shape.rect = Some(orb_rect);
        let toggled = show_orb(ctx, orb_rect, &view, lyrics.as_ref());
        (toggled.then_some(Act::Toggle), None)
    } else {
        (None, None)
    };
    handle_park_reveal(ctx, shape, &place, m.room);
    if let Some(a) = show_ask_prompt(
        ctx,
        shape,
        (&place, m.room, m.shown),
        (&view, cost.as_deref()),
        (settings, stage.book),
    ) {
        act = Some(a);
    }
    if matches!(act, Some(Act::Card(_))) || resize.is_some() {
        shape.player_priority = true;
        shape.unparked_until = None;
    }
    apply_resize(shape, resize, stage.toasts, place.rect, settings, out);
    act
}

type BarContent<'a> = (
    &'a Placement,
    &'a Motion,
    &'a View<'a>,
    &'a About<'a>,
    Option<&'a Lyrics>,
);

/// Paints the foreground bar area: frame, compact/full card, controls, edges.
fn paint_bar_area(
    ctx: &Context,
    shape: &mut Shape,
    (place, m, view, about, lyrics): BarContent<'_>,
    (stage, settings, out, cover): RenderEnv<'_>,
) -> (Option<Act>, Option<Resize>) {
    let escape = view.escape.is_some();
    let fit = Fit::new(place.width, escape).fill(place.width, escape);
    let mut resize = None;
    let mut act = None;
    z_order::area(ctx, Tier::FloatingTool, Id::new("read-aloud-bar"))
        .fade_in(false)
        .fixed_pos(place.rect.min)
        .constrain(false)
        .interactable(!place.parked)
        .show(ctx, |ui| {
            ui.set_opacity(m.shown);
            ui.set_clip_rect(m.room);
            let (r, _) = ui.allocate_exact_size(place.rect.size(), egui::Sense::hover());
            card::frame(ui, r, m.k);
            if place.tucked_rest {
                return;
            }
            if place.parked {
                ui.disable();
            }
            let row = r.with_min_y(r.bottom() - control::HEIGHT);
            // Words wait for room; compact and full cross-fade.
            let words = ((m.k - 0.35) / 0.65).clamp(0.0, 1.0);
            let top = r.with_max_y(row.top());
            let both = (about, lyrics);
            let light = |w: f32| ease_out(f64::from(w)) as f32;
            let small = words * (1.0 - m.f).powi(2);
            if small > 0.01 {
                let at = (top, light(small), m.f < 0.5);
                if card::compact(ui, at, both, &mut shape.ticker) {
                    act = Some(Act::Reveal);
                }
            }
            let large = words * m.f * m.f;
            if large > 0.01 {
                let at = (top, light(large), m.f >= 0.5);
                if card::show(ui, at, both, &mut shape.scroll, cover) {
                    act = Some(Act::Reveal);
                }
            }
            if let Some(a) = control::show(ui, row, (view, fit, m.k), settings, stage.book, out) {
                act = Some(a);
            }
            resize = edges::show(ui, r, place.span);
        });
    (act, resize)
}

/// Reveals the parked bar when the user clicks its edge-sliver hit target.
fn handle_park_reveal(ctx: &Context, shape: &mut Shape, place: &Placement, room: Rect) {
    if place.tucked_rest
        && crate::park::reveal(
            ctx,
            Id::new("read-aloud-edge-reveal"),
            place.rect,
            room,
            "Show the read-aloud player",
        )
        && let Some(mut tuck) = shape.parked
    {
        tuck.revealing = true;
        shape.parked = Some(tuck);
        shape.unparked_until = None;
        shape.player_priority = true;
    }
}

/// Shows the floating part-skip prompt above the player bar when active.
fn show_ask_prompt(
    ctx: &Context,
    shape: &mut Shape,
    (place, room, shown): (&Placement, Rect, f32),
    (view, cost): (&View<'_>, Option<&str>),
    (settings, book): (&mut ReadAloudSettings, Option<mareader_library::BookId>),
) -> Option<Act> {
    if view.ask.is_some() {
        if shape.asked != view.ask {
            shape.remember_skip = false;
        }
        shape.asked = view.ask;
    }
    let ease = motion::secs(ctx, 0.18);
    let asking = ctx.animate_bool_with_time(Id::new("read-aloud-asking"), view.ask.is_some(), ease);
    if !place.parked
        && let (Some(part), true) = (shape.asked, asking > 0.0)
    {
        let k = ease_out(f64::from(asking)) as f32 * shown;
        let can_remember = book.is_some_and(|b| {
            settings.book_skips.contains_key(&b) || settings.book_skips.len() < 1024
        });
        let answer = ask::show(
            ctx,
            (place.rect, room),
            part,
            cost,
            k,
            can_remember.then_some(&mut shape.remember_skip),
        );
        if let (Some(read), true) = (answer, view.ask.is_some()) {
            if !read
                && shape.remember_skip
                && let Some(b) = book
            {
                remember_skip(settings, b, part);
            }
            shape.remember_skip = false;
            return Some(Act::Answer(read));
        }
    }
    None
}

fn apply_resize(
    shape: &mut Shape,
    resize: Option<Resize>,
    toasts: Option<Rect>,
    rect: Rect,
    settings: &mut ReadAloudSettings,
    out: &mut Commands,
) {
    match resize {
        Some(Resize::To(w)) => {
            shape.dragged = Some(w);
            if toasts.is_some_and(|t| rect.intersects(t)) {
                out.push(Sent::Ui(UiCommand::ClearToasts));
            }
        }
        Some(Resize::Set(w)) => {
            (shape.dragged, settings.player_width) = (None, Some(w));
            if toasts.is_some_and(|t| rect.intersects(t)) {
                out.push(Sent::Ui(UiCommand::ClearToasts));
            }
        }
        Some(Resize::Reset) => {
            (shape.dragged, settings.player_width) = (None, None);
        }
        None => {}
    }
}
