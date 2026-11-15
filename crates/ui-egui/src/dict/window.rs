//! The reader's floating Dictionary: moved, resized, folded or a page.

use super::{Voice, bounds, page::Page};
use crate::icons::Icon;
use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent, UiCommand};
use crate::widgets;
use egui::{
    Align, Context, CursorIcon, Id, Layout, Order, Rect, RichText, Sense, Stroke, Ui, UiBuilder,
    Vec2, pos2, vec2,
};
use mareader_engine::command::Command;
use mareader_engine::dict::{Action, Window};
use mareader_engine::session::Session;
use mareader_settings::{DictionarySettings, WINDOW_MIN};

/// The controls' names, for screen readers and tests.
pub const MINIMIZE: &str = "Minimize the dictionary";
pub const RESTORE: &str = "Restore the dictionary";
pub const CLOSE: &str = "Close the dictionary";
pub const EXPAND: &str = "Open as a page";

/// Its size on first opening.
const START: Vec2 = vec2(380.0, 520.0);
const HEAD: f32 = 38.0;
/// The band along each edge that resizes.
const GRIP: f32 = 6.0;
const PILL: Vec2 = vec2(240.0, HEAD);
const SHORT_MIN: f32 = HEAD + 24.0;
/// Room kept between the window and the stage's edges.
const INSET: f32 = 12.0;

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

/// How long it takes to appear or leave, and to fold.
const APPEAR: f32 = 0.18;
const FOLD: f32 = 0.24;
/// How long it takes to step aside for a toast, or come back.
const DODGE: f32 = 0.24;

/// How far it has come in: 0 closed, 1 open; it keeps drawing until 0.
pub fn appearing(ctx: &Context, open: bool) -> f32 {
    let secs = crate::motion::secs(ctx, APPEAR);
    ctx.animate_bool_with_time(Id::new("dict-window-appear"), open, secs)
}

/// Draws the window; returns its rect and whether the user interacted.
pub fn show(ctx: &Context, f: Frame<'_>, appear: f32, out: &mut Commands) -> (Rect, bool) {
    let command_count = out.len();
    let t = Tokens::get(ctx);
    let w = &f.settings.window;
    let closing = !w.open;
    let saved = w
        .rect
        .map(|[x, y, wd, h]| Rect::from_min_size(pos2(x, y), vec2(wd, h)));
    let folded = w.minimized;
    let mut rect = f.live.or(saved).unwrap_or_else(|| start(f.stage));
    let keep = |r: Rect| match folded {
        true => keep_pill(r, f.stage),
        false => fit(r, f.stage),
    };
    let mut parked = *f.parked;
    let mut returned = false;
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
            false => keep(rect),
        };
    }

    let obstacles = [f.toasts, f.player];
    if !closing
        && !interacting
        && parked.is_none()
        && let Some(block) = covering(rect, obstacles)
    {
        let home = (*f.evacuated).map_or(rect, |(home, _)| home);
        let prefer_up = f.player.is_some_and(|player| player.intersects(home));
        *f.evacuated = Some((home, aside(home, block, f.stage, prefer_up)));
    }
    let dodging = (*f.evacuated).is_some_and(|(home, _)| covering(home, obstacles).is_some());
    let secs = crate::motion::secs(ctx, DODGE);
    let k = ctx.animate_bool_with_time(Id::new("dict-window-dodge"), dodging, secs);
    if let Some((home, aside)) = *f.evacuated {
        if k > 0.0 {
            let e = k * k * (3.0 - 2.0 * k);
            rect = Rect::from_min_max(home.min.lerp(aside.min, e), home.max.lerp(aside.max, e));
        } else if !dodging {
            rect = home;
            *f.evacuated = None;
            *f.live = None;
            out.push(Sent::Engine(Command::Dict(Action::Window(Window::Place(
                bounds(home),
            )))));
            returned = true;
        }
    }

    let tucked = crate::park::factor(
        ctx,
        Id::new("dict-window-park"),
        parked.is_some_and(|tuck| !tuck.revealing),
        parked.is_some(),
    );
    if let Some(tuck) = parked
        && tuck.revealing
        && tucked <= 0.0
    {
        rect = tuck.resume;
        parked = None;
        *f.evacuated = None;
        *f.live = None;
        out.push(Sent::Engine(Command::Dict(Action::Window(Window::Place(
            bounds(rect),
        )))));
        returned = true;
    }
    let short = f
        .evacuated
        .is_some_and(|(home, aside)| aside.height() < home.height());
    let morph = Morph::new(ctx, rect, folded, f.stage, short);
    let mut shown = parked.map_or_else(|| morph.shown(), |tuck| tuck.rect(tucked));
    let active = !closing && tucked >= 1.0 && parked.is_some_and(|tuck| !tuck.revealing);
    let pull = crate::park::magnetic_offset(
        ctx,
        Id::new("dict-window-park-magnet"),
        parked,
        shown,
        f.stage,
        active,
    );
    shown = shown.translate(pull);
    let tucked_rest = parked.is_some_and(|tuck| !tuck.revealing && tucked >= 1.0);
    let mut released = false;
    let area = egui::Area::new(Id::new("dict-window"))
        .order(Order::Middle)
        .fixed_pos(shown.min)
        .constrain(false)
        .movable(false)
        .interactable(!closing && parked.is_none());
    let layer = egui::LayerId::new(Order::Middle, area.layer().id);
    let grow = 0.96 + 0.04 * appear;
    let anchor = pos2(shown.center().x, shown.top());
    let lift = vec2(0.0, 8.0 * (1.0 - appear));
    let scale = egui::emath::TSTransform::from_translation(anchor.to_vec2() + lift)
        * egui::emath::TSTransform::from_scaling(grow)
        * egui::emath::TSTransform::from_translation(-anchor.to_vec2());
    ctx.set_transform_layer(layer, scale);
    area.show(ctx, |ui| {
        ui.multiply_opacity(appear);
        ui.set_clip_rect(f.stage);
        if closing {
            ui.disable();
        }
        ui.set_min_size(shown.size());
        ui.set_max_size(shown.size());
        if !(tucked_rest && f.parking == mareader_settings::EdgeParking::Tab) {
            paint(ui, shown, morph.fold, &t);
        }
        if tucked_rest {
            let _ = ui.allocate_exact_size(shown.size(), Sense::hover());
            return;
        }
        if parked.is_some() {
            ui.disable();
        }
        let head = Rect::from_min_size(shown.min, vec2(shown.width(), HEAD));
        let grab = ui.interact(head, Id::new("dict-window-head"), Sense::click_and_drag());
        if grab.dragged() {
            rect = rect.translate(grab.drag_delta());
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        }
        released |= grab.drag_stopped();
        if folded && grab.clicked() {
            out.push(Sent::Engine(Command::Dict(Action::Window(
                Window::Minimize(false),
            ))));
        } else if !folded && grab.double_clicked() {
            out.push(Sent::Engine(Command::Dict(Action::Window(
                Window::Minimize(true),
            ))));
        }
        let named = Named {
            word: f.page.text().trim(),
            glance: f.page.glance().filter(|_| morph.fold > 0.5),
        };
        if folded {
            grab.clone().on_hover_text("Click to restore");
        }
        let head_shown = (2.0 * morph.fold - 1.0).abs();
        ui.scope(|ui| {
            ui.multiply_opacity(head_shown);
            header(ui, head, morph.fold > 0.5, &named, &t, out);
        });
        if folded {
            f.page.ask(ui.ctx(), f.session, f.settings);
        }
        let body_shown = (1.0 - 2.0 * morph.fold).max(0.0);
        if body_shown <= 0.0 {
            return;
        }
        ui.multiply_opacity(body_shown);
        let line = Stroke::new(1.0, t.line);
        ui.painter().hline(shown.x_range(), head.bottom(), line);
        let open = morph.open;
        let body = Rect::from_min_max(
            pos2(shown.left() + 12.0, head.bottom() + 10.0),
            pos2(
                shown.left() + open.width() - 12.0,
                shown.top() + open.height() - 10.0,
            ),
        );
        let cut = Rect::from_min_max(head.left_bottom(), shown.max).shrink(1.0);
        let builder = UiBuilder::new()
            .max_rect(body)
            .layout(Layout::top_down(Align::Min));
        ui.scope_builder(builder, |ui| {
            ui.set_clip_rect(body.expand(2.0).intersect(cut));
            f.page.compact(ui, f.session, (f.settings, f.voice), out);
        });
        let (edges_released, edges_dragging) = if morph.fold <= 0.0 && !closing && parked.is_none()
        {
            edges(ui, shown, &mut rect)
        } else {
            (false, false)
        };
        released |= edges_released;
        if grab.dragged() || edges_dragging {
            *f.dragging = true;
            *f.evacuated = None;
            if f.toasts
                .is_some_and(|toast| shown.intersects(toast) || rect.intersects(toast))
            {
                out.push(Sent::Ui(UiCommand::ClearToasts));
            }
        }
    });
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

    let mut revealed = false;
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
    if clicked && let Some(mut tuck) = parked {
        revealed = true;
        tuck.revealing = true;
        parked = Some(tuck);
        *f.live = None;
        *f.evacuated = None;
    }

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
            let home = if folded {
                keep_pill(shape, f.stage)
            } else {
                fit(shape, f.stage)
            };
            let resume = if folded {
                Rect::from_min_size(home.min, rect.size())
            } else {
                home
            };
            parked = Some(crate::park::Tuck::new(edge, shape, home, f.stage).with_resume(resume));
            *f.live = None;
            *f.evacuated = None;
        } else {
            rect = keep(rect);
            *f.live = None;
            *f.evacuated = None;
            out.push(Sent::Engine(Command::Dict(Action::Window(Window::Place(
                bounds(rect),
            )))));
        }
    } else if !returned && turned {
        rect = keep(rect);
        *f.live = None;
        out.push(Sent::Engine(Command::Dict(Action::Window(Window::Place(
            bounds(rect),
        )))));
    } else if parked.is_some() {
        *f.live = None;
    } else if !returned && f.evacuated.is_none() && Some(rect) != saved {
        *f.live = Some(if released {
            keep(rect)
        } else {
            hang(rect, f.stage, folded)
        });
    }
    *f.parked = parked;
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
    let interacted = revealed
        || released
        || *f.dragging
        || command_interaction
        || (pointer_action && pointer_on_window)
        || keyboard_action;
    (tab_rect.unwrap_or(shown), interacted)
}

fn drag_hint(ctx: &Context, stage: Rect, left: bool, ready: bool) {
    let t = Tokens::get(ctx);
    let x = if left {
        stage.left() + 4.0
    } else {
        stage.right() - 10.0
    };
    let rail = Rect::from_min_max(
        pos2(x, stage.top() + 12.0),
        pos2(x + 6.0, (stage.bottom() - 12.0).max(stage.top() + 12.0)),
    );
    let painter = ctx.layer_painter(egui::LayerId::new(
        Order::Foreground,
        Id::new("dict-edge-hint"),
    ));
    painter.rect_filled(
        rail,
        3.0,
        t.accent.gamma_multiply(if ready { 0.9 } else { 0.5 }),
    );
    let text = if ready {
        "Release to tuck"
    } else {
        "Keep going to tuck"
    };
    let at = if left {
        pos2(rail.right() + 8.0, rail.center().y)
    } else {
        pos2(rail.left() - 8.0, rail.center().y)
    };
    let align = if left {
        egui::Align2::LEFT_CENTER
    } else {
        egui::Align2::RIGHT_CENTER
    };
    painter.text(at, align, text, egui::FontId::proportional(12.0), t.ink);
}

fn edge_tab(
    ctx: &Context,
    edge: crate::park::Edge,
    shown: Rect,
    stage: Rect,
    page: &Page,
) -> (Rect, bool) {
    use egui::{Align2, FontId, WidgetInfo, WidgetType, epaint::TextShape};
    let t = Tokens::get(ctx);
    let word = page.text().trim();
    let label = if word.is_empty() { "Dictionary" } else { word };
    let short: String = label.chars().take(30).collect();
    let text =
        ctx.fonts_mut(|fonts| fonts.layout_no_wrap(short, FontId::proportional(12.0), t.ink));
    let size = vec2(
        30.0_f32.min(stage.width().max(0.0)),
        (text.size().x + 68.0).min(stage.height().max(0.0)),
    );
    let x = if edge == crate::park::Edge::Left {
        stage.left()
    } else {
        stage.right() - size.x
    };
    let y = shown
        .top()
        .clamp(stage.top(), (stage.bottom() - size.y).max(stage.top()));
    let rect = Rect::from_min_size(pos2(x, y), size);
    if size.x <= 0.0 || size.y <= 0.0 {
        return (rect, false);
    }
    let response = egui::Area::new(Id::new("dict-edge-tab"))
        .order(Order::Foreground)
        .fixed_pos(rect.min)
        .constrain(false)
        .movable(false)
        .show(ctx, |ui| {
            ui.set_clip_rect(rect.intersect(stage));
            let hit = ui.interact(rect, Id::new("dict-edge-tab-button"), Sense::click());
            hit.widget_info(|| {
                WidgetInfo::labeled(
                    WidgetType::Button,
                    true,
                    format!("Restore dictionary: {label}"),
                )
            });
            let fill = if hit.hovered() { t.card } else { t.surface };
            ui.painter().rect_filled(rect, t.radius_lg, fill);
            ui.painter().rect_stroke(
                rect,
                t.radius_lg,
                Stroke::new(1.0, t.line),
                egui::StrokeKind::Inside,
            );
            let icon =
                Rect::from_center_size(pos2(rect.center().x, rect.top() + 17.0), Vec2::splat(15.0));
            Icon::Dictionary.paint(ui, icon, t.accent);
            let text_pos = if edge == crate::park::Edge::Left {
                pos2(
                    rect.center().x - text.size().y / 2.0,
                    rect.top() + 27.0 + text.size().x,
                )
            } else {
                pos2(rect.center().x + text.size().y / 2.0, rect.top() + 27.0)
            };
            let angle = if edge == crate::park::Edge::Left {
                -std::f32::consts::FRAC_PI_2
            } else {
                std::f32::consts::FRAC_PI_2
            };
            ui.painter()
                .add(TextShape::new(text_pos, text.clone(), t.ink).with_angle(angle));
            let count = page.entry_count();
            if count > 0 {
                let c = pos2(rect.center().x, rect.bottom() - 28.0);
                ui.painter().circle_filled(c, 9.0, t.accent_soft);
                ui.painter().text(
                    c,
                    Align2::CENTER_CENTER,
                    count.to_string(),
                    FontId::proportional(10.0),
                    t.accent,
                );
            }
            let arrow = if edge == crate::park::Edge::Left {
                Icon::ChevronRight
            } else {
                Icon::ChevronLeft
            };
            arrow.paint(
                ui,
                Rect::from_center_size(
                    pos2(rect.center().x, rect.bottom() - 12.0),
                    Vec2::splat(13.0),
                ),
                t.muted,
            );
            hit
        })
        .inner;
    let id = Id::new("dict-edge-peek-start");
    let now = ctx.input(|i| i.time);
    let since = ctx.data_mut(|data| {
        if response.hovered() {
            let start = data.get_temp::<f64>(id).unwrap_or(now);
            data.insert_temp(id, start);
            Some(start)
        } else {
            data.remove_temp::<f64>(id);
            None
        }
    });
    if let Some(since) = since {
        let wait = (since + 0.15 - now).max(0.0);
        if wait > 0.0 {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
        } else {
            let x = if edge == crate::park::Edge::Left {
                rect.right() + 6.0
            } else {
                rect.left() - 216.0
            };
            egui::Area::new(Id::new("dict-edge-peek"))
                .order(Order::Tooltip)
                .fixed_pos(pos2(
                    x.clamp(stage.left(), (stage.right() - 210.0).max(stage.left())),
                    rect.top(),
                ))
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_max_width(210.0_f32.min(stage.width().max(0.0)));
                        ui.label(
                            RichText::new("Peek · click the tab to restore")
                                .small()
                                .color(t.muted),
                        );
                        ui.label(label);
                        if let Some(glance) = page.glance() {
                            ui.label(RichText::new(glance).color(t.accent));
                        }
                    });
                });
        }
    }
    (rect, response.clicked())
}

/// The window turning into its pill: both ends, and `fold` from 0 to 1.
struct Morph {
    open: Rect,
    pill: Rect,
    fold: f32,
}

impl Morph {
    fn new(ctx: &Context, rect: Rect, folded: bool, stage: Rect, short: bool) -> Self {
        let secs = crate::motion::secs(ctx, FOLD);
        let id = Id::new("dict-window-fold");
        let fold = ctx.animate_bool_with_time(id, folded, secs);
        // Each end is kept as last seen, so the turn starts where it was.
        let kept = |which: &str| ctx.data(|d| d.get_temp::<Rect>(id.with(which)));
        let (open, pill) = match folded {
            true => {
                let pill = Rect::from_min_size(rect.min, PILL);
                (kept("open").unwrap_or_else(|| from_pill(rect, stage)), pill)
            }
            false => {
                let pill = Rect::from_min_size(to_pill(rect, stage).min, PILL);
                (rect, kept("pill").unwrap_or(pill))
            }
        };
        let open_size = if short {
            rect.size()
        } else {
            rect.size().max(Vec2::from(WINDOW_MIN))
        };
        let open = Rect::from_min_size(open.min, open_size);
        ctx.data_mut(|d| {
            if fold <= 0.0 {
                d.insert_temp(id.with("open"), open);
            }
            if fold >= 1.0 {
                d.insert_temp(id.with("pill"), pill);
            }
        });
        Self { open, pill, fold }
    }

    fn shown(&self) -> Rect {
        let e = ease(self.fold);
        Rect::from_min_max(
            self.open.min.lerp(self.pill.min, e),
            self.open.max.lerp(self.pill.max, e),
        )
    }
}

/// Fast out, soft landing.
fn ease(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    1.0 - (1.0 - x).powi(3)
}

/// Shadow, paper and hairline, rounding as it folds into the pill.
fn paint(ui: &Ui, r: Rect, fold: f32, t: &Tokens) {
    let radius = egui::emath::lerp(t.radius_lg..=HEAD / 2.0, ease(fold));
    let shadow = egui::Shadow {
        offset: [0, 6],
        blur: 24,
        spread: 0,
        color: t.shadow,
    };
    let painter = ui.painter();
    painter.add(shadow.as_shape(r, radius));
    painter.rect(
        r,
        radius,
        t.paper,
        Stroke::new(1.0, t.line),
        egui::StrokeKind::Inside,
    );
}

/// What the folded pill names: the word, and its top answer at a glance.
struct Named<'a> {
    word: &'a str,
    glance: Option<&'a str>,
}

/// Icon and title, then open as a page, fold and close.
fn header(ui: &mut Ui, head: Rect, folded: bool, n: &Named<'_>, t: &Tokens, out: &mut Commands) {
    let row = head.shrink2(vec2(10.0, 0.0));
    let builder = UiBuilder::new()
        .max_rect(row)
        .layout(Layout::right_to_left(Align::Center));
    ui.scope_builder(builder, |ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        // The title is part of the handle, not text to select.
        ui.style_mut().interaction.selectable_labels = false;
        let send = |w: Window| Sent::Engine(Command::Dict(Action::Window(w)));
        if widgets::small_icon_button(ui, Icon::Close, CLOSE).clicked() {
            out.push(send(Window::Close));
        }
        let (icon, label) = match folded {
            true => (Icon::ChevronUp, RESTORE),
            false => (Icon::Minus, MINIMIZE),
        };
        if widgets::small_icon_button(ui, icon, label).clicked() {
            out.push(send(Window::Minimize(!folded)));
        }
        if !folded && widgets::small_icon_button(ui, Icon::Fullscreen, EXPAND).clicked() {
            out.push(send(Window::Expand));
        }
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.add(Icon::Dictionary.image(ui, 16.0).tint(t.accent));
            // Folded, it names the word last searched and its answer.
            let title = match folded && !n.word.is_empty() {
                true => n.word,
                false => "Dictionary",
            };
            let title = RichText::new(title).font(crate::theme::medium(t.text));
            let Some(glance) = n.glance else {
                crate::shaped::line(ui, title, t.text, t.ink);
                return;
            };
            // The word keeps at most half, the answer the rest, on one line.
            let half = vec2(
                (ui.available_width() * 0.5).max(40.0),
                ui.available_height(),
            );
            let lead = Layout::left_to_right(Align::Center);
            ui.allocate_ui_with_layout(half, lead, |ui| {
                crate::shaped::line(ui, title, t.text, t.ink);
            });
            crate::shaped::line(ui, glance, t.text_small, t.muted);
        });
    });
}

/// The eight resize bands; returns (any_released, any_dragging).
fn edges(ui: &Ui, r: Rect, rect: &mut Rect) -> (bool, bool) {
    let mut released = false;
    let mut dragging = false;
    let sides = [
        (-1, 0),
        (1, 0),
        (0, -1),
        (0, 1),
        (-1, -1),
        (1, -1),
        (-1, 1),
        (1, 1),
    ];
    for (i, (dx, dy)) in sides.into_iter().enumerate() {
        let zone = band(r, dx, dy);
        let resp = ui.interact(zone, Id::new(("dict-window-edge", i)), Sense::drag());
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(cursor(dx, dy));
        }
        if resp.dragged() {
            *rect = stretch(*rect, dx, dy, resp.drag_delta());
            dragging = true;
        }
        released |= resp.drag_stopped();
    }
    (released, dragging)
}

/// `r` with the named sides moved by `d`, kept at least the minimum.
fn stretch(mut r: Rect, dx: i8, dy: i8, d: Vec2) -> Rect {
    let [min_w, min_h] = WINDOW_MIN;
    match dx {
        -1 => r.min.x = (r.min.x + d.x).min(r.max.x - min_w),
        1 => r.max.x = (r.max.x + d.x).max(r.min.x + min_w),
        _ => {}
    }
    match dy {
        -1 => r.min.y = (r.min.y + d.y).min(r.max.y - min_h),
        1 => r.max.y = (r.max.y + d.y).max(r.min.y + min_h),
        _ => {}
    }
    r
}

/// The strip inside `r` along the sides `dx` and `dy` name.
fn band(r: Rect, dx: i8, dy: i8) -> Rect {
    let span = |d: i8, lo: f32, hi: f32| match d {
        -1 => (lo, lo + GRIP),
        1 => (hi - GRIP, hi),
        _ => (lo + GRIP, hi - GRIP),
    };
    let (x0, x1) = span(dx, r.left(), r.right());
    let (y0, y1) = span(dy, r.top(), r.bottom());
    Rect::from_min_max(pos2(x0, y0), pos2(x1, y1))
}

fn cursor(dx: i8, dy: i8) -> CursorIcon {
    match (dx, dy) {
        (-1, -1) | (1, 1) => CursorIcon::ResizeNwSe,
        (1, -1) | (-1, 1) => CursorIcon::ResizeNeSw,
        (_, 0) => CursorIcon::ResizeHorizontal,
        _ => CursorIcon::ResizeVertical,
    }
}

/// First opened: at the stage's top right, beside the page.
fn start(stage: Rect) -> Rect {
    let min = pos2(stage.right() - START.x - INSET, stage.top() + INSET);
    Rect::from_min_size(min, START)
}

/// Folded, the rect starts at the pill: top right on the right half.
pub fn to_pill(r: Rect, stage: Rect) -> Rect {
    let right = r.center().x > stage.center().x;
    let x = if right { r.right() - PILL.x } else { r.left() };
    Rect::from_min_size(pos2(x, r.top()), r.size())
}

/// Unfolded, it grows from the pill away from the nearer edges.
pub fn from_pill(r: Rect, stage: Rect) -> Rect {
    let pill = Rect::from_min_size(r.min, PILL);
    let x = match pill.center().x > stage.center().x {
        true => pill.right() - r.width(),
        false => pill.left(),
    };
    let y = match pill.center().y > stage.center().y {
        true => pill.bottom() - r.height(),
        false => pill.top(),
    };
    fit(Rect::from_min_size(pos2(x, y), r.size()), stage)
}

/// The pill wholly on the stage, by its own size; the window's size kept.
pub fn keep_pill(r: Rect, stage: Rect) -> Rect {
    let room = stage.shrink(INSET);
    let far = (room.max - PILL).max(room.min);
    let min = r.min.clamp(room.min, far);
    Rect::from_min_size(min, r.size())
}

/// `r` no larger than the stage and wholly on it.
pub fn fit(r: Rect, stage: Rect) -> Rect {
    fit_min(r, stage, Vec2::from(WINDOW_MIN))
}

fn fit_min(r: Rect, stage: Rect, minimum: Vec2) -> Rect {
    let room = stage.shrink(INSET);
    let room_size = room.size().max(Vec2::ZERO);
    let floor = minimum.min(room_size);
    let size = r.size().max(floor).min(room_size.max(floor));
    let far_x = (room.max.x - size.x).max(room.min.x);
    let far_y = (room.max.y - size.y).max(room.min.y);
    let x = r.min.x.clamp(room.min.x, far_x);
    let y = r.min.y.clamp(room.min.y, far_y);
    Rect::from_min_size(pos2(x, y), size)
}

/// Like `fit`, but a dragged window may hang beyond either side.
fn hang(r: Rect, stage: Rect, folded: bool) -> Rect {
    let room = stage.shrink(INSET);
    let height = if folded { PILL.y } else { r.height() };
    let far_y = (room.bottom() - height).max(room.top());
    Rect::from_min_size(pos2(r.left(), r.top().clamp(room.top(), far_y)), r.size())
}

/// The blocks in `obstacles` touching `r`, united into one area.
fn covering(r: Rect, obstacles: [Option<Rect>; 2]) -> Option<Rect> {
    let mut mass = None;
    for block in obstacles.into_iter().flatten() {
        if block.intersects(r) {
            mass = Some(mass.map_or(block, |m: Rect| m.union(block)));
        }
    }
    mass
}

/// Steps beside a toast or above the player, shrinking only as a last resort.
fn aside(home: Rect, block: Rect, stage: Rect, prefer_up: bool) -> Rect {
    let room = stage.shrink(INSET);
    let clear = |r: &Rect| room.contains_rect(*r) && !r.intersects(block);
    let dy = (home.bottom() - (block.top() - 8.0)).max(0.0);
    let up = fit(home.translate(vec2(0.0, -dy)), stage);
    let dx = (home.right() - (block.left() - 8.0)).max(0.0);
    let left = fit(home.translate(vec2(-dx, 0.0)), stage);
    let first = if prefer_up { up } else { left };
    let second = if prefer_up { left } else { up };
    if clear(&first) {
        return first;
    }
    if clear(&second) {
        return second;
    }
    let min_w = WINDOW_MIN[0];
    let narrow = (block.left() - 8.0 - home.left() >= min_w).then(|| {
        fit(
            Rect::from_min_max(home.min, pos2(block.left() - 8.0, home.bottom())),
            stage,
        )
    });
    let height = (block.top() - 8.0 - home.top()).min(home.height());
    let short = (height >= SHORT_MIN).then(|| {
        fit_min(
            Rect::from_min_size(home.min, vec2(home.width(), height)),
            stage,
            vec2(min_w, SHORT_MIN),
        )
    });
    [narrow, short]
        .into_iter()
        .flatten()
        .filter(|r| !r.intersects(block))
        .max_by(|a, b| (a.width() * a.height()).total_cmp(&(b.width() * b.height())))
        .unwrap_or(up)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_stays_on_the_stage_and_never_below_its_minimum() {
        let stage = Rect::from_min_size(pos2(0.0, 40.0), vec2(1000.0, 700.0));
        let off = Rect::from_min_size(pos2(900.0, -50.0), vec2(500.0, 100.0));
        let r = fit(off, stage);
        assert!(stage.contains_rect(r), "{r:?}");
        assert_eq!(r.height(), WINDOW_MIN[1]);
        let shrunk = stretch(r, -1, 1, vec2(400.0, -400.0));
        assert_eq!(shrunk.size(), Vec2::from(WINDOW_MIN));
        assert_eq!(shrunk.max.x, r.max.x, "the right edge held");
    }

    #[test]
    fn the_dictionary_shortens_when_the_player_leaves_no_full_height() {
        let stage = Rect::from_min_size(pos2(0.0, 40.0), vec2(900.0, 700.0));
        let home = Rect::from_min_size(pos2(150.0, 200.0), vec2(600.0, 400.0));
        let player = Rect::from_min_size(pos2(260.0, 416.0), vec2(380.0, 224.0));
        let short = aside(home, player, stage, true);
        assert!(short.height() < WINDOW_MIN[1], "{short:?}");
        assert_eq!(short.bottom(), player.top() - 8.0);
        assert!(!short.intersects(player));
    }

    #[test]
    fn the_pill_reaches_every_edge_and_unfolds_onto_the_stage() {
        let stage = Rect::from_min_size(pos2(0.0, 40.0), vec2(1000.0, 700.0));
        let room = stage.shrink(INSET);
        let window = Rect::from_min_size(pos2(600.0, 60.0), START);
        // Folding on the right half keeps the pill under the − button.
        let folded = to_pill(window, stage);
        assert_eq!(folded.min.x + PILL.x, window.right());
        // Dragged into the bottom right corner, the pill gets there.
        let far = keep_pill(folded.translate(vec2(2000.0, 2000.0)), stage);
        assert_eq!(far.min + PILL, room.max);
        // And grows up and to the left from there, wholly on the stage.
        let open = from_pill(far, stage);
        assert!(stage.contains_rect(open), "{open:?}");
        assert_eq!(open.size(), START);
        assert_eq!(open.max, room.max);
    }
}
