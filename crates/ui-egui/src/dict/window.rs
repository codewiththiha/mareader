//! The reader's floating Dictionary: moved, resized, folded or a page.

use egui::{Context, Id, Rect, Vec2, pos2, vec2};
use mareader_engine::command::Command;
use mareader_engine::dict::{Action, Window};
use mareader_engine::session::Session;
use mareader_settings::DictionarySettings;

use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent};

use super::window_chrome::{AreaState, paint_window_area};
use super::window_geom::{Morph, PILL, aside, covering, hang, start};
use super::window_tab::{drag_hint, edge_tab};
use super::{Voice, bounds, page::Page};

pub use super::window_geom::{fit, from_pill, keep_pill, to_pill};

/// The controls' names, for screen readers and tests.
pub const MINIMIZE: &str = "Minimize the dictionary";
pub const RESTORE: &str = "Restore the dictionary";
pub const CLOSE: &str = "Close the dictionary";
pub const EXPAND: &str = "Open as a page";

/// What one frame of the window needs.
pub struct Frame<'a> {
    pub session: &'a mut Session,
    pub page: &'a mut Page,
    pub voice: &'a Voice,
    /// Its rect while dragged or resized, saved on release.
    pub live: &'a mut Option<Rect>,
    /// Whether the pointer is moving or resizing it.
    pub dragging: &'a mut bool,
    /// Folded when last drawn; a change moves the rect to suit.
    pub was_folded: &'a mut Option<bool>,
    /// Home and aside rects while stepped away from an obstacle.
    pub evacuated: &'a mut Option<(Rect, Rect)>,
    /// The original window sliding partly beyond an edge.
    pub parked: &'a mut Option<crate::park::Tuck>,
    pub settings: &'a DictionarySettings,
    pub parking: mareader_settings::EdgeParking,
    /// Where it may sit: below the bar, beside any rail.
    pub stage: Rect,
    /// Active toasts occupying the corner, if any.
    pub toasts: Option<Rect>,
    /// The player's intended resting rect, if it is shown or returning.
    pub player: Option<Rect>,
}

/// How long it takes to appear or leave, and to step aside for obstacles.
const APPEAR: f32 = 0.18;
const DODGE: f32 = 0.24;

/// How far it has come in: 0 closed, 1 open; it keeps drawing until 0.
pub fn appearing(ctx: &Context, open: bool) -> f32 {
    let secs = crate::motion::secs(ctx, APPEAR);
    ctx.animate_bool_with_time(Id::new("dict-window-appear"), open, secs)
}

/// Draws the window; returns its rect and whether the user interacted.
pub fn show(ctx: &Context, mut f: Frame<'_>, appear: f32, out: &mut Commands) -> (Rect, bool) {
    let command_count = out.len();
    let t = Tokens::get(ctx);
    let w = &f.settings.window;
    let closing = !w.open;
    let saved = w
        .rect
        .map(|[x, y, wd, h]| Rect::from_min_size(pos2(x, y), vec2(wd, h)));
    let folded = w.minimized;
    let (mut rect, mut parked, turned, interacting) =
        prepare_frame(ctx, &mut f, saved, closing, folded);
    let mut returned = dodge_obstacles(ctx, &mut f, &mut rect, (closing, interacting, parked), out);
    let (morph, shown, tucked_rest) = resolve_parking(
        ctx,
        &mut f,
        (&mut rect, &mut parked, &mut returned),
        (closing, folded),
        out,
    );
    let area_state = AreaState {
        shown,
        appear,
        closing,
        folded,
        parked: parked.is_some(),
        tucked_rest,
    };
    let (layer, released) = paint_window_area(ctx, &mut f, &mut rect, &morph, &area_state, &t, out);
    if *f.dragging && !released && f.parking == mareader_settings::EdgeParking::Tab {
        let left = crate::park::Edge::Left.over(rect, f.stage);
        let right = crate::park::Edge::Right.over(rect, f.stage);
        let over = left.max(right);
        if over > rect.width() * 0.18 {
            drag_hint(ctx, f.stage, left >= right, over > rect.width() * 0.5);
        }
    }
    if released {
        *f.dragging = false;
    }
    if closing {
        f.page.hide();
        *f.parked = None;
        return (shown, false);
    }
    let (revealed, tab_rect) = handle_park_reveal(ctx, &mut f, &mut parked, shown, tucked_rest);
    commit_rect(
        &mut f,
        &mut parked,
        rect,
        &morph,
        (saved, folded, released, returned, turned),
        out,
    );
    *f.parked = parked;
    let interacted = detect_interaction(
        ctx,
        &f,
        (shown, layer, command_count),
        (revealed, released),
        out,
    );
    (tab_rect.unwrap_or(shown), interacted)
}

fn resolve_parking(
    ctx: &Context,
    f: &mut Frame<'_>,
    (rect, parked, returned): (&mut Rect, &mut Option<crate::park::Tuck>, &mut bool),
    (closing, folded): (bool, bool),
    out: &mut Commands,
) -> (Morph, Rect, bool) {
    let tucked = crate::park::factor(
        ctx,
        Id::new("dict-window-park"),
        parked.is_some_and(|tuck| !tuck.revealing),
        parked.is_some(),
    );
    if let Some(tuck) = *parked
        && tuck.revealing
        && tucked <= 0.0
    {
        *rect = tuck.resume;
        *parked = None;
        *f.evacuated = None;
        *f.live = None;
        out.push(Sent::Engine(Command::Dict(Action::Window(Window::Place(
            bounds(*rect),
        )))));
        *returned = true;
    }
    let short = f
        .evacuated
        .is_some_and(|(home, aside)| aside.height() < home.height());
    let morph = Morph::new(ctx, *rect, folded, f.stage, short);
    let mut shown = parked.map_or_else(|| morph.shown(), |tuck| tuck.rect(tucked));
    let active = !closing && tucked >= 1.0 && parked.is_some_and(|tuck| !tuck.revealing);
    let pull = crate::park::magnetic_offset(
        ctx,
        Id::new("dict-window-park-magnet"),
        *parked,
        shown,
        f.stage,
        active,
    );
    shown = shown.translate(pull);
    let tucked_rest = parked.is_some_and(|tuck| !tuck.revealing && tucked >= 1.0);
    (morph, shown, tucked_rest)
}

fn keep_rect(r: Rect, stage: Rect, folded: bool) -> Rect {
    match folded {
        true => keep_pill(r, stage),
        false => fit(r, stage),
    }
}

fn prepare_frame(
    ctx: &Context,
    f: &mut Frame<'_>,
    saved: Option<Rect>,
    closing: bool,
    folded: bool,
) -> (Rect, Option<crate::park::Tuck>, bool, bool) {
    let mut rect = f.live.or(saved).unwrap_or_else(|| start(f.stage));
    let mut parked = *f.parked;
    let pointer_down = ctx.input(|i| i.pointer.any_down());
    let interacting = *f.dragging || pointer_down;
    if closing || parked.is_some() || !pointer_down {
        *f.dragging = false;
    }
    if closing {
        parked = None;
    }
    if f.parking == mareader_settings::EdgeParking::Off
        && let Some(mut tuck) = parked
    {
        tuck.revealing = true;
        parked = Some(tuck);
    }
    let turned = f.was_folded.is_some_and(|was| was != folded);
    if turned {
        if let Some(tuck) = parked.take() {
            rect = tuck.resume;
            *f.live = None;
        }
        rect = match folded {
            true => to_pill(rect, f.stage),
            false => from_pill(rect, f.stage),
        };
    }
    *f.was_folded = Some(folded);
    if let Some(tuck) = parked {
        rect = tuck.resume;
    } else {
        rect = match f.live.is_some() {
            true => hang(rect, f.stage, folded),
            false => keep_rect(rect, f.stage, folded),
        };
    }
    (rect, parked, turned, interacting)
}

fn dodge_obstacles(
    ctx: &Context,
    f: &mut Frame<'_>,
    rect: &mut Rect,
    (closing, interacting, parked): (bool, bool, Option<crate::park::Tuck>),
    out: &mut Commands,
) -> bool {
    let obstacles = [f.toasts, f.player];
    if !closing
        && !interacting
        && parked.is_none()
        && let Some(block) = covering(*rect, obstacles)
    {
        let home = (*f.evacuated).map_or(*rect, |(home, _)| home);
        let prefer_up = f.player.is_some_and(|player| player.intersects(home));
        *f.evacuated = Some((home, aside(home, block, f.stage, prefer_up)));
    }
    let dodging = (*f.evacuated).is_some_and(|(home, _)| covering(home, obstacles).is_some());
    let secs = crate::motion::secs(ctx, DODGE);
    let k = ctx.animate_bool_with_time(Id::new("dict-window-dodge"), dodging, secs);
    if let Some((home, aside)) = *f.evacuated {
        if k > 0.0 {
            let e = k * k * (3.0 - 2.0 * k);
            *rect = Rect::from_min_max(home.min.lerp(aside.min, e), home.max.lerp(aside.max, e));
        } else if !dodging {
            *rect = home;
            *f.evacuated = None;
            *f.live = None;
            out.push(Sent::Engine(Command::Dict(Action::Window(Window::Place(
                bounds(home),
            )))));
            return true;
        }
    }
    false
}

fn handle_park_reveal(
    ctx: &Context,
    f: &mut Frame<'_>,
    parked: &mut Option<crate::park::Tuck>,
    shown: Rect,
    tucked_rest: bool,
) -> (bool, Option<Rect>) {
    let mut tab_rect = None;
    let clicked = if tucked_rest && f.parking == mareader_settings::EdgeParking::Tab {
        parked.is_some_and(|tuck| {
            let (rect, clicked) = edge_tab(ctx, tuck.edge, shown, f.stage, f.page);
            tab_rect = Some(rect);
            clicked
        })
    } else {
        tucked_rest
            && crate::park::reveal(
                ctx,
                Id::new("dict-window-edge-reveal"),
                shown,
                f.stage,
                "Show the dictionary",
            )
    };
    let mut revealed = false;
    if clicked && let Some(mut tuck) = *parked {
        revealed = true;
        tuck.revealing = true;
        *parked = Some(tuck);
        *f.live = None;
        *f.evacuated = None;
    }
    (revealed, tab_rect)
}

fn commit_rect(
    f: &mut Frame<'_>,
    parked: &mut Option<crate::park::Tuck>,
    mut rect: Rect,
    morph: &Morph,
    (saved, folded, released, returned, turned): (Option<Rect>, bool, bool, bool, bool),
    out: &mut Commands,
) {
    if released && parked.is_none() {
        let shape = if folded && morph.fold >= 1.0 {
            Rect::from_min_size(rect.min, PILL)
        } else {
            rect
        };
        let left = crate::park::Edge::Left.over(shape, f.stage);
        let right = crate::park::Edge::Right.over(shape, f.stage);
        let edge = if left.max(right) > shape.width() * 0.5 {
            Some(if left >= right {
                crate::park::Edge::Left
            } else {
                crate::park::Edge::Right
            })
        } else {
            None
        };
        if let Some(edge) = edge.filter(|_| f.parking != mareader_settings::EdgeParking::Off) {
            let home = keep_rect(shape, f.stage, folded);
            let resume = if folded {
                Rect::from_min_size(home.min, rect.size())
            } else {
                home
            };
            *parked = Some(crate::park::Tuck::new(edge, shape, home, f.stage).with_resume(resume));
            *f.live = None;
            *f.evacuated = None;
        } else {
            rect = keep_rect(rect, f.stage, folded);
            *f.live = None;
            *f.evacuated = None;
            out.push(Sent::Engine(Command::Dict(Action::Window(Window::Place(
                bounds(rect),
            )))));
        }
    } else if !returned && turned {
        rect = keep_rect(rect, f.stage, folded);
        *f.live = None;
        out.push(Sent::Engine(Command::Dict(Action::Window(Window::Place(
            bounds(rect),
        )))));
    } else if parked.is_some() {
        *f.live = None;
    } else if !returned && f.evacuated.is_none() && Some(rect) != saved {
        *f.live = Some(if released {
            keep_rect(rect, f.stage, folded)
        } else {
            hang(rect, f.stage, folded)
        });
    }
}

fn detect_interaction(
    ctx: &Context,
    f: &Frame<'_>,
    (shown, layer, command_count): (Rect, egui::LayerId, usize),
    (revealed, released): (bool, bool),
    out: &Commands,
) -> bool {
    let pointer_action = ctx.input(|i| {
        i.pointer.any_pressed() || i.pointer.any_released() || i.smooth_scroll_delta != Vec2::ZERO
    });
    let pointer_on_window = ctx
        .pointer_latest_pos()
        .is_some_and(|pos| shown.contains(pos) && ctx.layer_id_at(pos) == Some(layer));
    let keyboard_action = f.page.typing()
        && ctx.input(|i| {
            i.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::Text(_) | egui::Event::Paste(_) | egui::Event::Key { .. }
                )
            })
        });
    let command_interaction = out.iter().skip(command_count).any(|command| match command {
        Sent::Engine(Command::Dict(Action::Window(Window::Place(_)))) => false,
        Sent::Engine(Command::Dict(_)) => true,
        _ => false,
    });
    revealed
        || released
        || *f.dragging
        || command_interaction
        || (pointer_action && pointer_on_window)
        || keyboard_action
}
