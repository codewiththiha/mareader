//! The few controls egui has no stock widget for.

use egui::{
    Button, Color32, CornerRadius, Frame, Margin, Rect, Response, RichText, Sense, Shadow, Stroke,
    Ui, Vec2, pos2, vec2,
};

use crate::icons::Icon;
use crate::icons::anim::{self, Act};
use crate::theme::Tokens;
use crate::ui_command::WindowCommand;

/// A popup's own frame: clicks stay in it, but Tab skips it for its rows.
pub const POPUP_SENSE: Sense = Sense::CLICK;

/// Standard context-menu inner width (`212pt` card with `6pt` frame padding).
pub const MENU_W: f32 = 200.0;
/// Grouped menu inner width (`252pt` card with `6pt` frame padding).
pub const GROUPED_MENU_W: f32 = 240.0;

const MENU_ROW_H: f32 = 30.0;
const MENU_PAD_X: f32 = 8.0;
const MENU_ICON: f32 = 15.0;
const MENU_ICON_GAP: f32 = 9.0;
const MENU_RADIUS: u8 = 8;

/// Applies the shared menu width and tight row spacing inside a popup body.
pub fn menu_style(ui: &mut Ui, width: f32) {
    ui.set_width(width);
    ui.spacing_mut().item_spacing.y = 2.0;
    ui.spacing_mut().button_padding = vec2(MENU_PAD_X, 4.0);
}

/// The pointer, unless a modal, card or menu other than `own` lies over it.
pub fn bare_pointer(ctx: &egui::Context, own: egui::LayerId) -> Option<egui::Pos2> {
    let at = ctx.pointer_hover_pos()?;
    // Chrome under something else stays down, or it would rise above it.
    let on = ctx.layer_id_at(at);
    on.is_none_or(|l| l == own || l.order == egui::Order::Background)
        .then_some(at)
}

/// Closes a menu when Enter or Space presses one of its rows.
pub fn close_on_key(ui: &Ui) {
    let ctx = ui.ctx();
    let pressed = ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Space));
    let focused = ctx.memory(|m| m.focused());
    let layer =
        focused.and_then(|id| ctx.viewport(|v| v.this_pass.widgets.get(id).map(|w| w.layer_id)));
    let submenu = focused.is_some_and(|id| {
        ctx.data(|d| d.get_temp::<bool>(id.with("submenu-trigger")))
            .unwrap_or(false)
    });
    if pressed && !submenu && layer == Some(ui.layer_id()) {
        ui.close();
    }
}

/// A right-click menu that Shift+F10 also opens, under the focused widget.
pub fn context_menu(resp: &Response, add: impl FnOnce(&mut Ui)) {
    context_menu_inner(resp, MENU_W, egui::PopupCloseBehavior::CloseOnClick, add);
}

/// A right-click menu with an explicit width.
pub fn context_menu_sized(resp: &Response, width: f32, add: impl FnOnce(&mut Ui)) {
    let close = egui::PopupCloseBehavior::CloseOnClickOutside;
    context_menu_inner(resp, width, close, add);
}

fn context_menu_inner(
    resp: &Response,
    width: f32,
    close: egui::PopupCloseBehavior,
    add: impl FnOnce(&mut Ui),
) {
    let ctx = &resp.ctx;
    let id = egui::Popup::default_response_id(resp);
    let keyed = id.with("keyed");
    let shift_f10 = egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::F10);
    if resp.has_focus() && ctx.input_mut(|i| i.consume_shortcut(&shift_f10)) {
        egui::Popup::open_id(ctx, id);
        ctx.data_mut(|d| d.insert_temp(keyed, true));
        // Its rows follow the widget in Tab order: the first takes focus.
        ctx.memory_mut(|m| m.move_focus(egui::FocusDirection::Next));
    }
    let open = egui::Popup::is_id_open(ctx, id);
    let by_key = open && ctx.data(|d| d.get_temp::<bool>(keyed)).unwrap_or(false);
    if (resp.secondary_clicked() || !open) && by_key {
        ctx.data_mut(|d| d.remove_temp::<bool>(keyed));
    }
    let popup = egui::Popup::context_menu(resp)
        .sense(POPUP_SENSE)
        .close_behavior(close);
    // No pointer to open at: it hangs from the widget instead.
    let popup = match by_key && !resp.secondary_clicked() {
        true => popup.anchor(resp.rect),
        false => popup,
    };
    popup.show(|ui| {
        menu_style(ui, width);
        // Menus animate on hover, before a selection can close them.
        let _act = anim::announce(ui, Act::Hover);
        add(ui);
        close_on_key(ui);
    });
}

/// A borderless icon button; `selected` marks a toggle that is on.
pub fn icon_button(ui: &mut Ui, icon: Icon, label: &str, selected: bool) -> Response {
    icon_button_keyed(ui, icon, label, selected, None)
}

/// [`icon_button`] whose tooltip also names its shortcut.
pub fn icon_button_keyed(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    selected: bool,
    keys: Option<&str>,
) -> Response {
    icon_button_tagged(ui, icon, label, label, selected, keys)
}

/// A keyed icon button whose animation survives label changes.
pub fn icon_button_tagged(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    tag: &str,
    selected: bool,
    keys: Option<&str>,
) -> Response {
    let t = Tokens::get(ui.ctx());
    // The tag preserves a click run across label changes.
    let resp = ui
        .push_id(("icon-button", tag), |ui| {
            let _act = anim::assume(ui, Act::Click);
            let button = Button::image(anim::glyph(ui, icon, t.icon, tag))
                .image_tint_follows_text_color(true)
                .frame_when_inactive(false)
                .selected(selected)
                .min_size(Vec2::splat(t.control));
            let resp = ui.add(button);
            anim::play(ui, &resp, icon, tag, t.icon);
            resp
        })
        .inner;
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label)
    });
    tip(&resp, label, keys);
    resp
}

/// A 24 pt icon button for rows and toasts.
pub fn small_icon_button(ui: &mut Ui, icon: Icon, label: &str) -> Response {
    let resp = ui
        .push_id(("small-icon-button", label), |ui| {
            let _act = anim::assume(ui, Act::Click);
            let button = Button::image(anim::glyph(ui, icon, 14.0, label))
                .image_tint_follows_text_color(true)
                .frame_when_inactive(false)
                .min_size(Vec2::splat(24.0));
            let resp = ui.add(button);
            anim::play(ui, &resp, icon, label, 14.0);
            resp
        })
        .inner;
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    tip(&resp, label, None);
    resp
}

/// Names a control with no visible label, for screen readers.
pub fn name(resp: &Response, label: &str) {
    resp.ctx
        .accesskit_node_builder(resp.id, |n| n.set_label(label));
}

/// Runs `add`, naming its nameless controls by `label` for screen readers.
pub fn labelled<R>(ui: &mut Ui, label: egui::Id, add: impl FnOnce(&mut Ui) -> R) -> R {
    let layer = ui.layer_id();
    let count = |ui: &Ui| {
        ui.ctx()
            .viewport(|v| v.this_pass.widgets.get_layer(layer).count())
    };
    let before = count(ui);
    let out = add(ui);
    let added: Vec<egui::Id> = ui.ctx().viewport(|v| {
        let all = v.this_pass.widgets.get_layer(layer).skip(before);
        all.filter(|w| w.sense.interactive())
            .map(|w| w.id)
            .collect()
    });
    for id in added {
        ui.ctx().accesskit_node_builder(id, |n| {
            if n.label().is_none() && n.labelled_by().is_empty() {
                n.push_labelled_by(label.accesskit_id());
            }
        });
    }
    out
}

fn is_shortcut_hint(text: &str) -> bool {
    text.contains('⌘')
        || text.contains('⌃')
        || text.contains('⇧')
        || text.contains('⌥')
        || text.contains("Ctrl")
        || text.contains("Alt")
        || text.contains("Shift")
        || text == "Esc"
        || (text.starts_with('F')
            && text.len() > 1
            && text.len() <= 3
            && text[1..].chars().all(|c| c.is_ascii_digit()))
}

/// Paints a small bordered keycap ending at `right`, vertically centred on `y`.
pub fn paint_keycap(ui: &Ui, right: f32, y: f32, keys: &str, t: &Tokens) -> f32 {
    let font = egui::FontId::proportional(10.5);
    let text = ui.painter().layout_no_wrap(keys.to_owned(), font, t.muted);
    let size = text.size() + vec2(10.0, 4.0);
    let cap = Rect::from_min_size(pos2(right - size.x, y - size.y * 0.5), size);
    let fill = t
        .paper
        .lerp_to_gamma(t.ink, if t.dark { 0.05 } else { 0.025 });
    ui.painter().rect(
        cap,
        CornerRadius::same(4),
        fill,
        Stroke::new(1.0, t.line.gamma_multiply(0.85)),
        egui::StrokeKind::Inside,
    );
    ui.painter().galley(cap.min + vec2(5.0, 2.0), text, t.muted);
    cap.left()
}

fn paint_menu_row_bg(ui: &Ui, rect: Rect, resp: &Response, on: bool, t: &Tokens) {
    let fill = if resp.is_pointer_button_down_on() {
        Some(t.pressed)
    } else if resp.hovered() || resp.has_focus() {
        Some(t.hover)
    } else if on {
        Some(t.selected)
    } else {
        None
    };
    if let Some(fill) = fill {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(MENU_RADIUS), fill);
    }
    if resp.hovered() && ui.is_enabled() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if resp.clicked() {
        let scope_id = egui::Id::new("active-submenu-click");
        let active = ui
            .ctx()
            .data(|d| d.get_temp::<Option<egui::Id>>(scope_id))
            .flatten();
        if let Some(id) = active {
            ui.ctx().data_mut(|d| d.insert_temp(id, true));
        }
    }
}

/// Paints a menu row's icon and label with hover motion.
#[allow(clippy::too_many_arguments)]
fn paint_menu_row_lead(
    ui: &Ui,
    rect: Rect,
    icon: Option<Icon>,
    label: &str,
    right_limit: f32,
    icon_tint: Color32,
    text_color: Color32,
    resp: &Response,
) {
    let y = rect.center().y;
    if let Some(icon) = icon {
        let _act = anim::announce(ui, Act::Hover);
        let at = Rect::from_min_size(
            pos2(rect.left() + MENU_PAD_X, y - MENU_ICON * 0.5),
            Vec2::splat(MENU_ICON),
        );
        anim::paint(ui, at, icon, icon_tint, label, resp);
    }
    let x = rect.left() + MENU_PAD_X + MENU_ICON + MENU_ICON_GAP;
    if right_limit <= x + 8.0 {
        return;
    }
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(13.5),
        text_color,
    );
    let clip = Rect::from_min_max(pos2(x, rect.top()), pos2(right_limit, rect.bottom()));
    ui.painter().with_clip_rect(clip).galley(
        pos2(x, y - galley.size().y * 0.5),
        galley,
        text_color,
    );
}

fn menu_inks(ui: &Ui, resp: &Response, on: bool, t: &Tokens) -> (Color32, Color32) {
    if !ui.is_enabled() {
        let dim = t.muted.gamma_multiply(0.48);
        return (dim, dim);
    }
    if let Some(custom) = ui.visuals().override_text_color {
        return (custom, custom);
    }
    if on {
        return (t.accent, t.accent);
    }
    let icon = if resp.hovered() || resp.has_focus() {
        t.ink
    } else {
        t.muted
    };
    (icon, t.ink)
}

/// One menu row with an optional trailing shortcut or status.
pub fn menu_row(ui: &mut Ui, icon: Icon, label: &str, keys: Option<&str>, on: bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let enabled = ui.is_enabled();
    let (rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), MENU_ROW_H), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, on, label));
    paint_menu_row_bg(ui, rect, &resp, on, &t);
    let y = rect.center().y;
    let right_limit = match keys.filter(|k| !k.is_empty()) {
        Some(k) if is_shortcut_hint(k) => {
            paint_keycap(ui, rect.right() - MENU_PAD_X, y, k, &t) - 6.0
        }
        Some(k) => {
            let galley = ui.painter().layout_no_wrap(
                k.to_owned(),
                egui::FontId::proportional(11.5),
                t.muted,
            );
            let x = rect.right() - MENU_PAD_X - galley.size().x;
            ui.painter()
                .galley(pos2(x, y - galley.size().y * 0.5), galley, t.muted);
            x - 6.0
        }
        None => rect.right() - MENU_PAD_X,
    };
    let (icon_tint, text_color) = menu_inks(ui, &resp, on, &t);
    paint_menu_row_lead(
        ui,
        rect,
        Some(icon),
        label,
        right_limit,
        icon_tint,
        text_color,
        &resp,
    );
    resp
}

/// Destructive menu row painted in the danger ink.
pub fn menu_row_danger(ui: &mut Ui, icon: Icon, label: &str) -> Response {
    let danger = Tokens::get(ui.ctx()).danger;
    ui.scope(|ui| {
        ui.visuals_mut().override_text_color = Some(danger);
        menu_row(ui, icon, label, None, false)
    })
    .inner
}

/// A menu row with a trailing accent badge.
pub fn menu_row_tag(ui: &mut Ui, icon: Icon, label: &str, tag: Option<&str>, on: bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let enabled = ui.is_enabled();
    let (rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), MENU_ROW_H), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, on, label));
    paint_menu_row_bg(ui, rect, &resp, on, &t);
    let y = rect.center().y;
    let right_limit = if let Some(tag) = tag.filter(|s| !s.is_empty()) {
        let galley =
            ui.painter()
                .layout_no_wrap(tag.to_owned(), crate::theme::medium(10.5), t.accent);
        let size = galley.size() + vec2(12.0, 4.0);
        let pill = Rect::from_min_size(
            pos2(rect.right() - MENU_PAD_X - size.x, y - size.y * 0.5),
            size,
        );
        ui.painter()
            .rect_filled(pill, CornerRadius::same(99), t.accent_soft);
        ui.painter()
            .galley(pill.min + vec2(6.0, 2.0), galley, t.accent);
        pill.left() - 6.0
    } else {
        rect.right() - MENU_PAD_X
    };
    let (icon_tint, text_color) = menu_inks(ui, &resp, on, &t);
    paint_menu_row_lead(
        ui,
        rect,
        Some(icon),
        label,
        right_limit,
        icon_tint,
        text_color,
        &resp,
    );
    resp
}

/// Menu row that drills into another step, ending with a stroke right chevron.
pub fn menu_row_chevron(ui: &mut Ui, icon: Icon, label: &str, hint: Option<&str>) -> Response {
    let t = Tokens::get(ui.ctx());
    let enabled = ui.is_enabled();
    let (rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), MENU_ROW_H), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    paint_menu_row_bg(ui, rect, &resp, false, &t);
    let y = rect.center().y;
    let chev = Rect::from_min_size(
        pos2(rect.right() - MENU_PAD_X - 14.0, y - 7.0),
        Vec2::splat(14.0),
    );
    Icon::ChevronRight.paint(ui, chev, t.muted);
    let mut right_limit = chev.left() - 4.0;
    if let Some(hint) = hint.filter(|s| !s.is_empty()) {
        let galley =
            ui.painter()
                .layout_no_wrap(hint.to_owned(), egui::FontId::proportional(11.5), t.muted);
        let x = right_limit - galley.size().x;
        ui.painter()
            .galley(pos2(x, y - galley.size().y * 0.5), galley, t.muted);
        right_limit = x - 6.0;
    }
    let (icon_tint, text_color) = menu_inks(ui, &resp, false, &t);
    paint_menu_row_lead(
        ui,
        rect,
        Some(icon),
        label,
        right_limit,
        icon_tint,
        text_color,
        &resp,
    );
    resp
}

/// Choice row inside a submenu: checkmark in the leading icon slot when active.
pub fn menu_check_row(ui: &mut Ui, label: &str, checked: bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let enabled = ui.is_enabled();
    let (rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), MENU_ROW_H), Sense::click());
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, checked, label)
    });
    paint_menu_row_bg(ui, rect, &resp, false, &t);
    let icon = checked.then_some(Icon::Check);
    paint_menu_row_lead(
        ui,
        rect,
        icon,
        label,
        rect.right() - MENU_PAD_X,
        t.accent,
        t.ink,
        &resp,
    );
    resp
}

#[derive(Clone, Copy, Default)]
struct SubMenu {
    over: Option<f64>,
    pinned: bool,
    pass: u64,
}

/// Submenu trigger painted on the shared menu grid with a stroke right chevron.
pub fn submenu_row(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    hint: Option<&str>,
    sub_width: f32,
    add: impl FnOnce(&mut Ui),
) -> Response {
    const GRACE: f64 = 0.24;
    let t = Tokens::get(ui.ctx());
    let (now, pass) = (ui.input(|i| i.time), ui.ctx().cumulative_pass_nr());
    let (_rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), MENU_ROW_H), Sense::click());
    ui.data_mut(|d| d.insert_temp(resp.id.with("submenu-trigger"), true));
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let sub_id = resp.id.with("submenu-popup");
    let mut state = ui
        .data_mut(|d| d.get_temp::<SubMenu>(sub_id))
        .unwrap_or_default();
    if state.pass == 0 || state.pass + 1 < pass {
        state = SubMenu {
            over: None,
            pinned: false,
            pass,
        };
    } else {
        state.pass = pass;
    }
    if resp.contains_pointer() {
        state.over = Some(now);
    }
    let open_key = resp.has_focus()
        && ui.input(|i| {
            i.key_pressed(egui::Key::ArrowRight)
                || i.key_pressed(egui::Key::Enter)
                || i.key_pressed(egui::Key::Space)
        });
    if resp.clicked() || open_key {
        state.pinned = !state.pinned || open_key;
    }
    let open = state.pinned || state.over.is_some_and(|t0| now - t0 < GRACE);
    let shown = egui::Popup::from_response(&resp)
        .id(sub_id)
        .open(open)
        .sense(POPUP_SENSE)
        .align(egui::RectAlign::RIGHT_START)
        .gap(4.0)
        .width(sub_width)
        .show(|ui| {
            let click_id = sub_id.with("child-clicked");
            let scope_id = egui::Id::new("active-submenu-click");
            ui.data_mut(|d| {
                d.insert_temp(click_id, false);
                d.insert_temp(scope_id, Some(click_id));
            });
            menu_style(ui, sub_width);
            add(ui);
            close_on_key(ui);
            ui.data_mut(|d| d.insert_temp(scope_id, None::<egui::Id>));
            ui.data(|d| d.get_temp::<bool>(click_id)).unwrap_or(false)
        });
    let pos = ui.input(|i| i.pointer.interact_pos());
    let inside = shown.as_ref().is_some_and(|s| {
        s.response.contains_pointer() || pos.is_some_and(|p| s.response.rect.contains(p))
    });
    if inside {
        state.over = Some(now);
    }
    let chosen = shown.as_ref().is_some_and(|s| s.inner);
    let any_pressed = ui.input(|i| i.pointer.any_pressed());
    let pressed_away = any_pressed && !inside && !resp.contains_pointer();
    let escape = open && ui.input(|i| i.key_pressed(egui::Key::Escape));
    if chosen || pressed_away || escape {
        state = SubMenu {
            over: None,
            pinned: false,
            pass,
        };
        if chosen {
            ui.close();
        }
    }
    if let Some(t0) = state.over {
        let left = GRACE - (now - t0);
        if !state.pinned && left > 0.0 {
            ui.ctx().request_repaint_after_secs(left as f32);
        }
    }
    ui.data_mut(|d| d.insert_temp(sub_id, state));
    let active = open && shown.is_some();
    paint_menu_row_bg(ui, resp.rect, &resp, active, &t);
    let y = resp.rect.center().y;
    let chev = Rect::from_min_size(
        pos2(resp.rect.right() - MENU_PAD_X - 14.0, y - 7.0),
        Vec2::splat(14.0),
    );
    Icon::ChevronRight.paint(ui, chev, if active { t.ink } else { t.muted });
    let mut right_limit = chev.left() - 4.0;
    if let Some(hint) = hint.filter(|s| !s.is_empty()) {
        let galley =
            ui.painter()
                .layout_no_wrap(hint.to_owned(), egui::FontId::proportional(11.5), t.muted);
        let x = right_limit - galley.size().x;
        ui.painter()
            .galley(pos2(x, y - galley.size().y * 0.5), galley, t.muted);
        right_limit = x - 6.0;
    }
    let icon_tint = if active || resp.hovered() || resp.has_focus() {
        t.ink
    } else {
        t.muted
    };
    paint_menu_row_lead(
        ui,
        resp.rect,
        Some(icon),
        label,
        right_limit,
        icon_tint,
        t.ink,
        &resp,
    );
    resp
}

/// An uppercase muted section caption with an optional icon.
pub fn menu_section(ui: &mut Ui, icon: Option<Icon>, title: &str) {
    let t = Tokens::get(ui.ctx());
    if ui.cursor().min.y > ui.min_rect().min.y + 1.0 {
        ui.add_space(4.0);
    }
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, title));
    let y = rect.center().y;
    let mut x = rect.left() + MENU_PAD_X;
    if let Some(icon) = icon {
        icon.paint(
            ui,
            Rect::from_min_size(pos2(x, y - 5.5), Vec2::splat(11.0)),
            t.muted,
        );
        x += 15.0;
    }
    ui.painter().text(
        pos2(x, y),
        egui::Align2::LEFT_CENTER,
        title.to_uppercase(),
        crate::theme::medium(10.5),
        t.muted,
    );
}

/// Subtle horizontal hairline divider inside a popup menu.
pub fn menu_rule(ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 7.0), Sense::hover());
    let y = rect.center().y.round() + 0.5;
    ui.painter().hline(
        rect.left() + 4.0..=rect.right() - 4.0,
        y,
        Stroke::new(1.0, t.line),
    );
}

/// The one filled button, its words in the ink that reads on `fill`.
pub fn primary_button(
    ui: &Ui,
    icon: Option<Icon>,
    label: &str,
    fill: Color32,
    t: &Tokens,
) -> Button<'static> {
    let ink = crate::theme::ink_on(fill);
    let text = RichText::new(label).color(ink);
    let button = match icon {
        Some(icon) => Button::image_and_text(icon.image(ui, 16.0).tint(ink), text),
        None => Button::new(text),
    };
    button.fill(fill).corner_radius(t.radius)
}

/// A small muted line in a menu, lined up with the rows' icons.
pub fn menu_note(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    let pad = ui.spacing().button_padding.x as i8;
    Frame::new()
        .inner_margin(Margin::symmetric(pad, 2))
        .show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(text).size(t.text_small).color(t.muted)).wrap());
        });
}

/// A hairline `length` tall between a bar's groups.
pub fn vrule(ui: &mut Ui, length: f32) {
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(9.0, length), Sense::hover());
    let x = rect.center().x.round() + 0.5;
    ui.painter()
        .vline(x, rect.y_range(), Stroke::new(1.0, t.line));
}

/// A sidebar panel with nothing to list: icon, title and hint, centred.
pub fn panel_empty(ui: &mut Ui, icon: Option<Icon>, title: &str, hint: &str) {
    let t = Tokens::get(ui.ctx());
    let max = ui.max_rect();
    let area = Rect::from_min_max(pos2(max.left(), ui.cursor().top()), max.max);
    let area = area.shrink2(vec2(28.0, 0.0));
    let builder = egui::UiBuilder::new()
        .max_rect(area)
        .layout(egui::Layout::top_down(egui::Align::Center));
    ui.scope_builder(builder, |ui| {
        ui.add_space((area.height() * 0.3).clamp(16.0, 200.0));
        if let Some(icon) = icon {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
            icon.paint(ui, r, t.muted);
            ui.add_space(6.0);
        }
        ui.label(
            RichText::new(title)
                .font(crate::theme::medium(14.0))
                .color(t.ink),
        );
        ui.add(egui::Label::new(RichText::new(hint).size(t.text_small).color(t.muted)).wrap());
    });
}

/// The app's tooltip: a small chip, the shortcut beside the words.
pub fn tip(resp: &Response, label: &str, keys: Option<&str>) {
    let t = Tokens::get(&resp.ctx);
    let mut tooltip = egui::Tooltip::for_enabled(resp);
    let frame = Frame::new()
        .fill(t.surface)
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(6)
        .inner_margin(Margin::symmetric(8, 4))
        .shadow(Shadow {
            offset: [0, 2],
            blur: 8,
            spread: 0,
            color: t.shadow.gamma_multiply(0.6),
        });
    tooltip.popup = tooltip.popup.frame(frame).gap(6.0);
    tooltip.show(|ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            ui.label(RichText::new(label).size(t.text_small).color(t.ink));
            if let Some(keys) = keys {
                ui.label(RichText::new(keys).size(t.text_small).color(t.muted));
            }
        });
    });
}

/// One window-caption button family per desktop convention.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptionStyle {
    /// Full-height squares, flush right; close turns red.
    Windows,
    /// Small translucent circles (GNOME/libadwaita).
    Gnome,
}

const CAPTIONS: [WindowCommand; 3] = [
    WindowCommand::Minimize,
    WindowCommand::ToggleMaximize,
    WindowCommand::Close,
];

/// Lays the three captions out left to right and returns the one clicked.
pub fn captions(ui: &mut Ui, style: CaptionStyle, maximized: bool) -> Option<WindowCommand> {
    let t = Tokens::get(ui.ctx());
    let mut clicked = None;
    for which in CAPTIONS {
        let size = match style {
            CaptionStyle::Windows => vec2(46.0, ui.available_height()),
            CaptionStyle::Gnome => Vec2::splat(24.0),
        };
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
        let (icon, label) = caption_face(which, maximized);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
        let close = which == WindowCommand::Close;
        let (fill, ink) = caption_colours(style, close, &resp, &t);
        if style == CaptionStyle::Windows {
            ui.painter().rect_filled(rect, CornerRadius::ZERO, fill);
        } else {
            ui.painter().circle_filled(rect.center(), 12.0, fill);
        }
        let glyph = match style {
            CaptionStyle::Windows => 14.0,
            CaptionStyle::Gnome => 12.0,
        };
        let mark = Rect::from_center_size(rect.center(), Vec2::splat(glyph));
        anim::paint(ui, mark, icon, ink, label, &resp);
        if resp.clicked() {
            clicked = Some(which);
        }
    }
    clicked
}

fn caption_face(which: WindowCommand, maximized: bool) -> (Icon, &'static str) {
    match which {
        WindowCommand::Minimize => (Icon::WindowMinimize, "Minimize"),
        WindowCommand::ToggleMaximize if maximized => (Icon::WindowRestore, "Restore"),
        WindowCommand::ToggleMaximize => (Icon::WindowMaximize, "Maximize"),
        WindowCommand::Close | WindowCommand::StartDrag | WindowCommand::ToggleFullscreen => {
            (Icon::Close, "Close")
        }
    }
}

fn caption_colours(
    style: CaptionStyle,
    close: bool,
    resp: &Response,
    t: &Tokens,
) -> (Color32, Color32) {
    let down = resp.is_pointer_button_down_on();
    let hot = resp.hovered() || down;
    match (style, close) {
        (CaptionStyle::Windows, true) if hot => {
            let red = if down { 0xc5_0f_1f } else { 0xe8_11_23 };
            (rgb(red), Color32::WHITE)
        }
        (CaptionStyle::Windows, _) if hot => (t.line, t.ink),
        (CaptionStyle::Windows, _) => (Color32::TRANSPARENT, t.ink),
        (CaptionStyle::Gnome, true) if hot => {
            let red = if down { 0xc5_3a_37 } else { 0xe0_52_4f };
            (rgb(red), Color32::WHITE)
        }
        (CaptionStyle::Gnome, _) => {
            let alpha = if down {
                0.34
            } else if hot {
                0.24
            } else {
                0.12
            };
            (t.ink.gamma_multiply(alpha), t.ink)
        }
    }
}

fn rgb(hex: u32) -> Color32 {
    let [_, r, g, b] = hex.to_be_bytes();
    Color32::from_rgb(r, g, b)
}

/// Stock selectable buttons, `cols` to a row, sharing the width.
pub fn choices<T: Copy + PartialEq>(
    ui: &mut Ui,
    value: &mut T,
    all: &[T],
    label: fn(T) -> &'static str,
    cols: usize,
) {
    for row in all.chunks(cols) {
        ui.columns(cols, |cols| {
            for (ui, &v) in cols.iter_mut().zip(row) {
                let button = egui::Button::selectable(*value == v, label(v));
                if ui.add_sized([ui.available_width(), 28.0], button).clicked() {
                    *value = v;
                }
            }
        });
    }
}

/// Minus, the value, plus; laid right to left, as rows place controls.
pub fn stepper(
    ui: &mut Ui,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    step: f32,
    show: impl Fn(f32) -> String,
    label: &str,
) {
    let (lo, hi) = (*range.start(), *range.end());
    let up = ui.add_enabled_ui(*value < hi, |ui| {
        small_icon_button(ui, Icon::Plus, &format!("More {label}"))
    });
    let shown = egui::Label::new(show(*value));
    ui.add_sized([44.0, 24.0], shown);
    let down = ui.add_enabled_ui(*value > lo, |ui| {
        small_icon_button(ui, Icon::Minus, &format!("Less {label}"))
    });
    // Off-grid values, as imported ones may be, snap to the grid.
    if up.inner.clicked() {
        *value = ((*value / step).floor() * step + step).min(hi);
    }
    if down.inner.clicked() {
        *value = ((*value / step).ceil() * step - step).max(lo);
    }
}

/// An on/off switch; `label` names it for screen readers and tests.
pub fn switch(ui: &mut Ui, on: &mut bool, label: &str) -> Response {
    let (rect, mut resp) = ui.allocate_exact_size(SWITCH, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let (enabled, value) = (ui.is_enabled(), *on);
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, value, label)
    });
    paint_switch(ui, rect, &resp, value);
    resp
}

const SWITCH: Vec2 = vec2(36.0, 20.0);

/// A switch's track and knob, sliding with `resp`'s animation.
fn paint_switch(ui: &Ui, rect: Rect, resp: &Response, on: bool) {
    let t = Tokens::get(ui.ctx());
    let how = ui.ctx().animate_bool_responsive(resp.id, on);
    let mut track = t.track.lerp_to_gamma(t.accent, how);
    if resp.hovered() {
        track = track.lerp_to_gamma(t.ink, 0.08);
    }
    ui.painter()
        .rect_filled(rect, CornerRadius::same(10), track);
    let x = egui::lerp(rect.left() + 10.0..=rect.right() - 10.0, how);
    // A faint edge keeps the knob off a pale track.
    let edge = Stroke::new(1.0, Color32::from_black_alpha(if t.dark { 0 } else { 28 }));
    ui.painter()
        .circle(pos2(x, rect.center().y), 8.0, Color32::WHITE, edge);
}

/// A menu row that flips a setting: icon and name, its switch at the end.
pub fn menu_switch(ui: &mut Ui, icon: Icon, label: &str, on: &mut bool) -> Response {
    menu_switch_keyed(ui, icon, label, on, None)
}

/// A switch row with an optional shortcut, kept out of its accessible name.
pub fn menu_switch_keyed(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    on: &mut bool,
    keys: Option<&str>,
) -> Response {
    let t = Tokens::get(ui.ctx());
    let enabled = ui.is_enabled();
    let size = vec2(ui.available_width(), MENU_ROW_H);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let value = *on;
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, value, label)
    });
    paint_menu_row_bg(ui, rect, &resp, false, &t);
    let y = rect.center().y;
    let knob = Rect::from_min_size(
        pos2(rect.right() - MENU_PAD_X - SWITCH.x, y - SWITCH.y * 0.5),
        SWITCH,
    );
    let mut right_limit = knob.left() - 6.0;
    if let Some(keys) = keys {
        let cap_left = paint_keycap(ui, knob.left() - 6.0, y, keys, &t);
        right_limit = cap_left - 6.0;
        tip(&resp, label, Some(keys));
    }
    let (icon_tint, text_color) = menu_inks(ui, &resp, false, &t);
    paint_menu_row_lead(
        ui,
        rect,
        Some(icon),
        label,
        right_limit,
        icon_tint,
        text_color,
        &resp,
    );
    paint_switch(ui, knob, &resp, value);
    resp
}

/// A segmented track's row height.
const SEGMENT: f32 = 24.0;

/// Choices as one segmented track, `cols` equal cells to a row.
pub fn segments<T: Copy + PartialEq>(
    ui: &mut Ui,
    value: &mut T,
    all: &[T],
    cols: usize,
    face: impl Fn(T) -> (Option<Icon>, &'static str),
) {
    segments_of(ui, SEGMENT, value, all, cols, face);
}

/// Segments with rows `row` tall, to line up with taller controls.
pub fn segments_of<T: Copy + PartialEq>(
    ui: &mut Ui,
    row: f32,
    value: &mut T,
    all: &[T],
    cols: usize,
    face: impl Fn(T) -> (Option<Icon>, &'static str),
) {
    let t = Tokens::get(ui.ctx());
    // A segment animates on selection because its track stays visible.
    let _act = anim::announce(ui, Act::Click);
    let cols = cols.max(1);
    let rows = all.len().div_ceil(cols);
    let size = vec2(ui.available_width(), row * rows as f32);
    let (track, _) = ui.allocate_exact_size(size, Sense::hover());
    let line = Stroke::new(1.0, t.line);
    ui.painter()
        .rect_stroke(track, CornerRadius::same(6), line, egui::StrokeKind::Inside);
    let w = track.width() / cols as f32;
    let row_h = row;
    let cell = |i: f32, row: f32| {
        let min = track.min + vec2(i * w, row * row_h);
        Rect::from_min_size(min, vec2(w, row_h)).shrink(2.0)
    };
    // The pick's fill slides to a new pick instead of jumping.
    let first = all.first().map_or("", |&v| face(v).1);
    let thumb = ui.id().with(("segments", first));
    if let Some(i) = all.iter().position(|v| v == value) {
        let ctx = ui.ctx();
        let x = ctx.animate_value_with_time(thumb.with("x"), (i % cols) as f32, 0.16);
        let y = ctx.animate_value_with_time(thumb.with("y"), (i / cols) as f32, 0.16);
        (ui.painter()).rect_filled(cell(x, y), CornerRadius::same(4), t.selected);
    }
    for (i, &v) in all.iter().enumerate() {
        let (icon, label) = face(v);
        let cell = cell((i % cols) as f32, (i / cols) as f32);
        let resp = ui.interact(cell, ui.id().with(("segment", label)), Sense::click());
        if resp.clicked() {
            *value = v;
        }
        let on = *value == v;
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            if !on {
                (ui.painter()).rect_filled(cell, CornerRadius::same(4), t.hover);
            }
        }
        // Accent ink marks the pick where the soft fill barely shows (Dim).
        let ink = if on { t.accent } else { t.ink };
        let font = egui::FontId::proportional(t.text);
        let galley = ui.painter().layout_no_wrap(label.to_owned(), font, ink);
        let icon_w = if icon.is_some() { 18.0 } else { 0.0 };
        let mut x = cell.center().x - (icon_w + galley.size().x) / 2.0;
        let y = cell.center().y;
        if let Some(icon) = icon {
            let at = Rect::from_min_size(pos2(x, y - 7.0), Vec2::splat(14.0));
            // Stable cell names preserve runs when earlier controls change.
            ui.push_id(("segment-icon", first, label), |ui| {
                anim::paint(ui, at, icon, ink, label, &resp);
            });
            x += icon_w;
        }
        let top = y - galley.size().y / 2.0;
        ui.painter().galley(pos2(x, top), galley, ink);
        let info = || egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, label);
        resp.widget_info(info);
    }
}

/// A dropdown with the Lucide chevron; every combo box starts here.
#[allow(clippy::disallowed_methods)]
pub fn combo(salt: impl egui::AsIdSalt) -> egui::ComboBox {
    egui::ComboBox::from_id_salt(salt).icon(combo_chevron)
}

/// A combo box's generated Lucide chevron, in place of egui's triangle.
fn combo_chevron(ui: &Ui, rect: egui::Rect, visuals: &egui::style::WidgetVisuals, open: bool) {
    let icon = if open {
        Icon::ChevronUp
    } else {
        Icon::ChevronDown
    };
    let at = egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(14.0));
    icon.image(ui, 14.0)
        .tint(visuals.fg_stroke.color)
        .paint_at(ui, at);
}

/// Fades `fill` over a scroll area's edges while content is cut off.
pub fn scroll_fades<R>(ui: &Ui, out: &egui::scroll_area::ScrollAreaOutput<R>, fill: Color32) {
    const FADE: f32 = 24.0;
    let rect = out.inner_rect;
    let above = out.state.offset.y;
    let below = out.content_size.y - rect.height() - above;
    let painter = ui.painter_at(rect);
    if above > 1.0 {
        let top = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), FADE.min(above)));
        fade(&painter, top, fill, Color32::TRANSPARENT);
    }
    if below > 1.0 {
        let h = FADE.min(below);
        let bottom = egui::Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - h), rect.max);
        fade(&painter, bottom, Color32::TRANSPARENT, fill);
    }
}

/// A vertical gradient over `rect`, `top` colour to `bottom`.
fn fade(painter: &egui::Painter, rect: egui::Rect, top: Color32, bottom: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);
    painter.add(mesh);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tagged_toggle_keeps_the_run_its_old_label_started() {
        use egui::{Event, Modifiers, PointerButton, pos2};

        const TAG: &str = "Page bookmark";
        let ctx = egui::Context::default();
        let mut at = pos2(20.0, 20.0);
        let mut clicked = Vec::new();
        let mut runs = Vec::new();
        for (n, time) in [0.0, 0.02, 0.04].into_iter().enumerate() {
            let events = match n {
                1 => vec![
                    Event::PointerMoved(at),
                    Event::PointerButton {
                        pos: at,
                        button: PointerButton::Primary,
                        pressed: true,
                        modifiers: Modifiers::default(),
                    },
                    Event::PointerButton {
                        pos: at,
                        button: PointerButton::Primary,
                        pressed: false,
                        modifiers: Modifiers::default(),
                    },
                ],
                _ => vec![Event::PointerMoved(pos2(190.0, 190.0))],
            };
            let raw = egui::RawInput {
                time: Some(time),
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), Vec2::splat(200.0))),
                events,
                ..Default::default()
            };
            let label = if n < 2 {
                "Bookmark this page"
            } else {
                "Remove bookmark"
            };
            let mut out = ctx.run_ui(raw, |ui| {
                let resp = icon_button_tagged(ui, Icon::Bookmark, label, TAG, n > 1, None);
                at = resp.rect.center();
                clicked.push(resp.clicked());
                runs.push(
                    ui.push_id(("icon-button", TAG), |ui| {
                        crate::icons::anim::run_started(ui, Icon::Bookmark, TAG)
                    })
                    .inner,
                );
            });
            out.textures_delta.clear();
        }
        assert_eq!(clicked, [false, true, false]);
        assert_eq!(
            runs,
            [false, true, true],
            "the changed label should keep the click's run"
        );
    }
}
