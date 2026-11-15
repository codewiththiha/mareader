//! Where the cards go: a pile in the corner, or opened into a short list.

use egui::{
    Align, Align2, Area, Button, Context, CornerRadius, CursorIcon, Frame, Id, Key, Layout, Margin,
    Modifiers, Order, Pos2, Rect, Response, RichText, ScrollArea, Sense, Stroke, Ui, UiBuilder,
    Vec2, WidgetInfo, WidgetType, pos2, vec2,
};

use super::card::{self, Body, Card};
use super::{Exit, Live, Toasts};
use crate::icons::Icon;
use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent, UiCommand};
use crate::widgets;

/// Where the stack sits and the clock it runs on.
pub(super) struct Place {
    pub corner: Pos2,
    pub width: f32,
    /// The height the stack may grow to.
    pub room: f32,
    pub now: f64,
    pub fade: f64,
    /// Where a drag from the page is, while one goes on.
    pub drag: Option<Pos2>,
}

impl Place {
    /// A page drag over the stack: faint, and let through.
    fn aside(&self, seen: Option<Rect>) -> bool {
        let near = |p: &Pos2| seen.is_none_or(|s| s.expand(NEAR).contains(*p));
        self.drag.as_ref().is_some_and(near)
    }
}

/// Cards that peek out behind the front one.
const BEHIND: f32 = 2.0;
/// Each card behind shows this much above the one before it,
const PEEK: f32 = 8.0;
/// and is this much narrower at each side.
const NARROW: f32 = 10.0;
/// A drag this far, or a flick this fast, throws a card away.
const THROW: f32 = 96.0;
const FLICK: f32 = 700.0;
/// A drag the wrong way moves the card this share of the pointer.
const RESIST: f32 = 0.3;
const STRIP_H: f32 = 24.0;
const SPACE: f32 = 6.0;
/// The opened list never takes more than this share of the room.
const LIST_SHARE: f32 = 0.6;
const LIST_MAX: f32 = 420.0;
const PANEL_PAD: f32 = 8.0;
/// How long a card takes to settle into a new place in the pile.
const SETTLE: f32 = 0.22;
/// How much of a stack shows while a drag passes under it.
const ASIDE: f32 = 0.3;
/// A page drag this near the stack makes it step aside.
const NEAR: f32 = 24.0;
/// How long stepping aside, and back, takes.
const ASIDE_FOR: f32 = 0.15;

/// Takes the pointer only over the toasts themselves, never during a page drag.
fn takes_pointer(ctx: &Context, toasts: &Toasts, aside: bool, seen: Option<Rect>) -> bool {
    let pointer = ctx.pointer_hover_pos();
    let over = seen.is_none_or(|r| pointer.is_some_and(|p| r.contains(p)));
    toasts.live.iter().any(|l| l.dragging) || (!aside && over)
}

/// The stack's opacity, easing toward faint while aside.
fn faint(ctx: &Context, id: Id, aside: bool) -> f32 {
    let time = crate::motion::secs(ctx, ASIDE_FOR);
    let p = ctx.animate_bool_with_time(id.with("aside"), aside, time);
    1.0 - (1.0 - ASIDE) * p
}

fn progress(now: f64, at: f64, fade: f64) -> f32 {
    match fade > 0.0 {
        true => ((now - at) / fade).clamp(0.0, 1.0) as f32,
        false => 1.0,
    }
}

fn ease(p: f32) -> f32 {
    1.0 - (1.0 - p).powi(3)
}

/// A card's shift and opacity as it comes or goes.
fn travel(live: &Live, place: &Place) -> (Vec2, f32) {
    let born = live.born.unwrap_or(place.now);
    let came = ease(progress(place.now, born, place.fade));
    let mut shift = vec2(0.0, (1.0 - came) * 14.0);
    let mut alpha = came;
    if let Some((at, exit)) = live.gone {
        let p = ease(progress(place.now, at, place.fade));
        alpha *= 1.0 - p;
        match exit {
            Exit::Fade => shift.y += p * 6.0,
            Exit::Swipe(from) => shift.x = from + p * place.width * 0.8,
        }
    }
    (shift, alpha)
}

/// Follows a drag, springs back when let go short, throws it when let go far.
fn swipe(ctx: &Context, live: &mut Live, (resp, held): (&Response, bool), now: f64) -> f32 {
    if resp.dragged() && !held {
        live.drag += resp.drag_delta().x;
        live.dragging = true;
    }
    let pulled = match live.drag < 0.0 {
        true => live.drag * RESIST,
        false => live.drag,
    };
    let time = if live.dragging { 0.0 } else { 0.16 };
    let shown = ctx.animate_value_with_time(Id::new(("toast-drag", live.id)), pulled, time);
    live.offset = shown;
    if resp.drag_stopped() {
        let flick = ctx.input(|i| i.pointer.velocity().x);
        if live.drag > THROW || (flick > FLICK && live.drag > 8.0) {
            live.gone = Some((now, Exit::Swipe(shown)));
        }
        live.drag = 0.0;
        live.dragging = false;
    }
    shown
}

/// How a grabbed card answered the pointer this frame.
struct Grabbed {
    resp: Response,
    /// How far it shows dragged.
    shown: f32,
    /// The pointer is on its buttons, which take it instead.
    on_controls: bool,
}

/// A card that takes the pointer: named by its words, swiped by a drag.
fn grab(ui: &Ui, live: &mut Live, (rect, card): (Rect, &Card), now: f64) -> Grabbed {
    let offset = vec2(live.offset, 0.0);
    let at = rect.translate(offset);
    let resp = ui.interact(at, Id::new(("toast", live.id)), Sense::click_and_drag());
    let words = live.toast.message.clone();
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &words));
    ui.ctx().accesskit_node_builder(resp.id, |node| {
        node.set_live(match live.toast.tone {
            mareader_engine::toast::Tone::Error => egui::accesskit::Live::Assertive,
            _ => egui::accesskit::Live::Polite,
        });
    });
    let controls = card::controls(rect, card).translate(offset);
    let (hover, origin) = ui.input(|i| (i.pointer.hover_pos(), i.pointer.press_origin()));
    let held = origin.is_some_and(|p| controls.contains(p)) && !live.dragging;
    let shown = swipe(ui.ctx(), live, (&resp, held), now);
    if live.dragging {
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    }
    Grabbed {
        resp,
        shown,
        on_controls: hover.is_some_and(|p| controls.contains(p)),
    }
}

/// Draws `live` at `rect` with `alpha`; `content` fades its insides.
fn paint(ui: &mut Ui, rect: Rect, live: &Live, card: &Card, look: (f32, f32), out: &mut Commands) {
    let (alpha, content) = look;
    let t = Tokens::get(ui.ctx());
    let mut child = ui.new_child(UiBuilder::new().max_rect(rect));
    let alpha = alpha * ui.opacity();
    child.set_opacity(alpha);
    let body = match (live.listed, content > 0.5) {
        (true, _) => Body::Listed,
        (false, true) => Body::Front,
        (false, false) => Body::Behind,
    };
    card::shell(&child, rect, live.toast.tone, body, &t);
    if content > 0.0 {
        child.set_opacity(alpha * content);
        card::content(&mut child, rect, live, card, out);
    }
}

/// The pile: the newest in front, two more peeking out above it.
pub(super) fn closed(
    ctx: &Context,
    toasts: &mut Toasts,
    place: &Place,
    out: &mut Commands,
) -> bool {
    let t = Tokens::get(ctx);
    for live in toasts.live.iter_mut() {
        live.listed = false;
    }
    let up = toasts.live.iter().filter(|l| !l.going()).count();
    // Newest first; a leaving card keeps the place it had.
    let mut deep = 0.0_f32;
    let mut order = Vec::new();
    for (at, live) in toasts.live.iter().enumerate().rev() {
        if deep > BEHIND {
            break;
        }
        order.push((at, deep));
        if !live.going() {
            deep += 1.0;
        }
    }
    let laid: Vec<(usize, f32, Card)> = order
        .into_iter()
        .filter_map(|(at, deep)| {
            let live = toasts.live.get(at)?;
            let shown =
                ctx.animate_value_with_time(Id::new(("toast-depth", live.id)), deep, SETTLE);
            Some((at, shown, card::measure(ctx, live, place.width, &t)))
        })
        .collect();
    let front = laid
        .iter()
        .find(|(at, _, _)| toasts.live.get(*at).is_some_and(|l| !l.going()))
        .or(laid.first())
        .map_or(0.0, |(_, _, c)| c.height);
    let behind = (up.saturating_sub(1) as f32).min(BEHIND);
    let badge_top = place.corner.y - front - PEEK * behind - SPACE + 1.0;
    let badge = Rect::from_min_size(pos2(place.corner.x - 22.0, badge_top), vec2(22.0, 22.0));
    let over_badge = ctx.pointer_hover_pos().is_some_and(|p| badge.contains(p));
    let fan = toasts.hovered && up > 1 && !over_badge;
    let fan_k =
        ctx.animate_bool_with_time(Id::new("toast-fan"), fan, crate::motion::secs(ctx, SETTLE));
    let step = egui::lerp(PEEK..=front + SPACE, fan_k);
    let top = place.corner.y - front - step * behind;
    let strip = Rect::from_min_max(
        pos2(place.corner.x - place.width, top - SPACE - STRIP_H),
        pos2(place.corner.x, top - SPACE),
    );
    // Room below for a card coming in, within the window's inset.
    let floor = place.corner + vec2(0.0, super::INSET - 2.0);
    let head = if fan { strip.top() } else { top - 12.0 };
    let bounds = Rect::from_two_pos(pos2(place.corner.x - place.width, head), floor);
    let seen = Rect::from_min_max(bounds.min, place.corner);
    toasts.seen = Some(seen);
    let aside = place.aside(Some(seen));
    // Placed exactly: kept on screen, it would creep up and grow.
    let area = Area::new(Id::new("toasts"))
        .order(Order::Tooltip)
        .constrain(false)
        .interactable(takes_pointer(ctx, toasts, aside, Some(seen)))
        .fixed_pos(bounds.min)
        .show(ctx, |ui| {
            ui.set_clip_rect(ctx.content_rect());
            ui.set_opacity(faint(ctx, Id::new("toasts"), aside));
            let mut hover = ui.allocate_rect(bounds, Sense::hover()).contains_pointer()
                || ctx.pointer_hover_pos().is_some_and(|p| seen.contains(p));
            // The deepest is drawn first; a leaving card above its heir.
            let mut drawn: Vec<&(usize, f32, Card)> = laid.iter().collect();
            drawn.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
            for (at, deep, card) in drawn {
                let Some(live) = toasts.live.get_mut(*at) else {
                    continue;
                };
                let blend = deep.min(1.0);
                let height = card.height + (front - card.height) * blend;
                let foot = place.corner.y - step * deep;
                let inset = NARROW * deep * (1.0 - fan_k);
                let mut rect = Rect::from_min_max(
                    pos2(place.corner.x - place.width + inset, foot - height),
                    pos2(place.corner.x - inset, foot),
                );
                let (shift, mut alpha) = travel(live, place);
                alpha *= (1.0 - 0.3 * deep * (1.0 - fan_k)).max(0.0);
                let leads = *deep < 0.5 && !live.going();
                let mut pulled = 0.0;
                if leads || (fan && !live.going()) {
                    let g = grab(ui, live, (rect, card), place.now);
                    let resp = g.resp;
                    pulled = g.shown;
                    hover |= resp.contains_pointer() || live.dragging;
                    if resp.hovered() && !live.dragging && !g.on_controls {
                        let icon = if up > 1 {
                            CursorIcon::PointingHand
                        } else {
                            CursorIcon::Grab
                        };
                        ctx.set_cursor_icon(icon);
                    }
                    if resp.clicked() && up > 1 && !g.on_controls {
                        toasts.open = true;
                        return hover;
                    }
                }
                rect = rect.translate(shift + vec2(pulled, 0.0));
                let faded = alpha * (1.0 - (pulled.abs() / place.width).min(0.6));
                let content = (1.0 - deep * (1.0 - fan_k)).clamp(0.0, 1.0);
                if let Some(live) = toasts.live.get(*at) {
                    paint(ui, rect, live, card, (faded, content), out);
                }
            }
            if up > 1 {
                if !fan {
                    let badge_strip = Rect::from_min_max(
                        pos2(place.corner.x - place.width, badge_top - STRIP_H - 1.0),
                        pos2(place.corner.x, badge_top - 1.0),
                    );
                    count_badge(ui, badge_strip, up, toasts, &t);
                }
                if fan {
                    let mut row = ui.new_child(
                        UiBuilder::new()
                            .max_rect(strip)
                            .layout(Layout::right_to_left(Align::Center)),
                    );
                    if row
                        .add(
                            Button::new(
                                RichText::new("Clear all")
                                    .size(t.text_small)
                                    .color(t.accent),
                            )
                            .frame_when_inactive(false),
                        )
                        .clicked()
                    {
                        out.push(Sent::Ui(UiCommand::ClearToasts));
                    }
                    row.label(
                        RichText::new(format!("{up} notifications · paused"))
                            .size(t.text_small)
                            .color(t.muted),
                    );
                }
            }
            hover
        });
    area.inner
}

fn count_badge(ui: &mut Ui, strip: Rect, up: usize, toasts: &mut Toasts, t: &Tokens) {
    let rect = Rect::from_min_size(
        pos2(strip.right() - 22.0, strip.bottom() + 1.0),
        vec2(22.0, 22.0),
    );
    let badge = Button::new(
        RichText::new(up.to_string())
            .size(t.text_small)
            .color(t.surface),
    )
    .fill(t.accent)
    .stroke(Stroke::NONE)
    .corner_radius(CornerRadius::same(11));
    let all = ui.put(rect, badge);
    all.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Show all notifications"));
    if all.clicked() {
        toasts.open = true;
    }
}

/// The stack opened: a short list, newest on top, scrolling past its cap.
pub(super) fn open(
    ctx: &Context,
    toasts: &mut Toasts,
    place: &Place,
    shown: f32,
    out: &mut Commands,
) -> bool {
    let t = Tokens::get(ctx);
    if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
        toasts.open = false;
    }
    for live in toasts.live.iter_mut() {
        live.listed = true;
    }
    let inner = place.width - 2.0 * PANEL_PAD;
    let cards: Vec<Card> = toasts
        .live
        .iter()
        .rev()
        .map(|live| card::measure(ctx, live, inner, &t))
        .collect();
    let up = toasts.live.iter().filter(|l| !l.going()).count();
    let tall = (place.room * LIST_SHARE).clamp(120.0, LIST_MAX);
    let rise = (1.0 - shown) * 10.0;
    let last = ctx.memory(|m| m.area_rect(Id::new("toasts-open")));
    let aside = place.aside(last);
    let area = Area::new(Id::new("toasts-open"))
        .order(Order::Tooltip)
        .interactable(takes_pointer(ctx, toasts, aside, last))
        .pivot(Align2::RIGHT_BOTTOM)
        .fixed_pos(place.corner + vec2(0.0, rise))
        .show(ctx, |ui| {
            ui.set_opacity(shown * faint(ctx, Id::new("toasts-open"), aside));
            Frame::new()
                .fill(t.surface)
                .stroke(Stroke::new(1.0, t.line))
                .corner_radius(CornerRadius::from(t.radius_lg + 4.0))
                .shadow(ui.style().visuals.popup_shadow)
                .inner_margin(Margin::same(PANEL_PAD as i8))
                .show(ui, |ui| {
                    ui.set_width(inner);
                    header(ui, up, toasts, &t, out);
                    ui.add_space(SPACE);
                    ScrollArea::vertical()
                        .max_height(tall)
                        .auto_shrink([false, true])
                        .show(ui, |ui| list(ui, toasts, &cards, place, out));
                });
        });
    toasts.seen = Some(area.response.rect);
    let pressed = ctx.input(|i| i.pointer.any_pressed());
    if pressed && !area.response.contains_pointer() {
        toasts.open = false;
    }
    area.response.contains_pointer() || toasts.open
}

fn header(ui: &mut Ui, up: usize, toasts: &mut Toasts, t: &Tokens, out: &mut Commands) {
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.label(
            RichText::new("Notifications")
                .size(t.text)
                .strong()
                .color(t.ink),
        );
        ui.label(
            RichText::new(up.to_string())
                .size(t.text_small)
                .color(t.muted),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if widgets::small_icon_button(ui, Icon::ChevronDown, "Show less").clicked() {
                toasts.open = false;
            }
            let clear = Button::new(
                RichText::new("Clear all")
                    .size(t.text_small)
                    .color(t.accent),
            )
            .frame_when_inactive(false);
            if ui.add(clear).clicked() {
                out.push(Sent::Ui(UiCommand::ClearToasts));
            }
        });
    });
}

fn list(ui: &mut Ui, toasts: &mut Toasts, cards: &[Card], place: &Place, out: &mut Commands) {
    ui.spacing_mut().item_spacing.y = SPACE;
    let count = toasts.live.len();
    for (n, card) in cards.iter().enumerate() {
        let Some(live) = count
            .checked_sub(n + 1)
            .and_then(|at| toasts.live.get_mut(at))
        else {
            continue;
        };
        let (rect, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), card.height), Sense::hover());
        let (shift, alpha) = travel(live, place);
        let pulled = match live.going() {
            true => 0.0,
            false => {
                let grabbed = grab(ui, live, (rect, card), place.now);
                if grabbed.resp.clicked() && !grabbed.on_controls {
                    toasts.open = true;
                }
                grabbed.shown
            }
        };
        let moved = rect.translate(vec2(shift.x + pulled, 0.0));
        let faded = alpha * (1.0 - (pulled.abs() / place.width).min(0.6));
        paint(ui, moved, live, card, (faded, 1.0), out);
    }
}
