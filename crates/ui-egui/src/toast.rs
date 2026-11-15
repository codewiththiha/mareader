//! Toasts: a stack in the bottom-right corner, newest in front.

mod card;
mod stack;

use crate::ui_command::{Commands, Sent, UiCommand};
use egui::{Context, Id, Rect, pos2};
use mareader_engine::toast::{Toast, Tone};

/// At least this long is left once a held stack is let go.
pub const HOVER_GRACE: f64 = 1.5;
/// How often a running time line is redrawn.
const LINE_STEP: f32 = 1.0 / 30.0;
pub const FADE: f32 = 0.2;
/// At most this many are kept; the oldest goes without a fade.
const KEEP: usize = 12;
/// Clear of the window's edges.
const INSET: f32 = 16.0;
const WIDTH: f32 = 360.0;
/// Fixed card height, shared by one-line and two-line toasts alike.
const SPOT_H: f32 = 52.0;

/// How a toast leaves.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Exit {
    Fade,
    /// Thrown sideways from where it was let go.
    Swipe(f32),
}

/// A toast on screen; its clock starts on its first frame.
struct Live {
    id: u64,
    toast: Toast,
    /// How often the same words came while it was up.
    times: u32,
    born: Option<f64>,
    left: Option<f64>,
    gone: Option<(f64, Exit)>,
    /// How far it has been dragged sideways.
    drag: f32,
    dragging: bool,
    /// Where the drag left it last frame, so its hit area moves with it.
    offset: f32,
    /// Drawn in the opened list, where the clocks stand still.
    listed: bool,
}

impl Live {
    fn going(&self) -> bool {
        self.gone.is_some()
    }

    /// The share of its time still to run, for its line.
    fn remaining(&self) -> f32 {
        let left = self.left.unwrap_or_else(|| self.toast.lasts());
        (left / self.toast.lasts()).clamp(0.0, 1.0) as f32
    }
}

/// Where the open menu sits, if one is open.
fn open_menus(ctx: &Context) -> Vec<Rect> {
    if !egui::Popup::is_any_open(ctx) {
        return Vec::new();
    }
    let shown: Vec<(Id, Rect)> = ctx.memory(|m| {
        let up = |l: &egui::LayerId| l.order == egui::Order::Foreground && m.areas().is_visible(l);
        let layers = m.layer_ids().filter(up);
        layers
            .filter_map(|l| Some((l.id, m.area_rect(l.id)?)))
            .collect()
    });
    // Asked outside the memory's lock, which the check takes too.
    let open = |(id, _): &(Id, Rect)| egui::Popup::is_id_open(ctx, *id);
    shown.into_iter().filter(open).map(|(_, r)| r).collect()
}

/// What the toasts must leave uncovered, by pass.
#[derive(Clone, Default)]
struct Keep {
    clear: Vec<(u64, Rect)>,
}

fn keep_id() -> Id {
    Id::new("toast-keep")
}

/// A summoned menu or card at `rect`: toasts there leave, newcomers wait.
pub fn keep_clear(ctx: &Context, rect: Rect) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| (d.get_temp_mut_or_default::<Keep>(keep_id()).clear).push((pass, rect)));
}

/// This pass's and the last's, either side of where the toasts draw.
fn kept(ctx: &Context) -> Keep {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        let keep = d.get_temp_mut_or_default::<Keep>(keep_id());
        let recent = |(at, _): &(u64, Rect)| *at + 1 >= pass;
        keep.clear.retain(recent);
        let found = keep.clone();
        let now = |(at, _): &(u64, Rect)| *at == pass;
        keep.clear.retain(now);
        found
    })
}

/// Rects recently asked to stay clear, this pass and the last.
pub fn kept_clear(ctx: &Context) -> Vec<Rect> {
    let pass = ctx.cumulative_pass_nr();
    ctx.data(|d| {
        let keep = d.get_temp::<Keep>(keep_id()).unwrap_or_default();
        let recent = keep.clear.iter().filter(move |(at, _)| at + 1 >= pass);
        recent.map(|(_, r)| *r).collect()
    })
}

/// Where toasts show by default in the bottom-right corner.
pub fn bed(area: Rect, lift: f32) -> Rect {
    let width = WIDTH.min(area.width() - 2.0 * INSET).max(200.0);
    let foot = area.bottom() - INSET - lift;
    Rect::from_min_max(
        pos2(area.right() - INSET - width, foot - SPOT_H),
        pos2(area.right() - INSET, foot),
    )
}

/// The stack, oldest first.
#[derive(Default)]
pub struct Toasts {
    live: Vec<Live>,
    next: u64,
    /// Opened into a list by a click.
    open: bool,
    /// The pointer was over the stack last frame.
    hovered: bool,
    last: Option<f64>,
    /// Where the stack showed last frame, while any is up.
    seen: Option<Rect>,
    /// Something summoned sat where the stack goes, last frame.
    blocked: bool,
    /// Toasts that came while blocked, and since when they wait.
    waiting: Vec<(Toast, Option<f64>)>,
}

impl Toasts {
    /// Puts `toast` in front, or holds it while something is in the way.
    pub fn push(&mut self, toast: Toast) {
        match self.blocked {
            true => self.waiting.push((toast, None)),
            false => self.put(toast),
        }
    }

    /// Puts `toast` in front; the same words again count instead of stacking.
    fn put(&mut self, toast: Toast) {
        let same = |l: &Live| {
            !l.going()
                && l.toast.undo.is_none()
                && toast.undo.is_none()
                && l.toast.tone == toast.tone
                && l.toast.player == toast.player
                && l.toast.message == toast.message
        };
        if let Some(at) = self.live.iter().position(same) {
            let mut again = self.live.remove(at);
            again.times += 1;
            again.left = None;
            self.live.push(again);
            return;
        }
        self.next += 1;
        self.live.push(Live {
            id: self.next,
            toast,
            times: 1,
            born: None,
            left: None,
            gone: None,
            drag: 0.0,
            dragging: false,
            offset: 0.0,
            listed: false,
        });
        let over = self.live.len().saturating_sub(KEEP);
        self.live.drain(..over);
    }

    /// Closes the toast `id`.
    pub fn close(&mut self, id: u64, now: f64) {
        let found = self.live.iter_mut().find(|l| l.id == id && !l.going());
        if let Some(live) = found {
            live.gone = Some((now, Exit::Fade));
        }
    }

    /// Closes them all.
    pub fn clear(&mut self, now: f64) {
        for live in self.live.iter_mut().filter(|l| !l.going()) {
            live.gone = Some((now, Exit::Fade));
        }
        self.waiting.clear();
        self.open = false;
    }

    /// Something summoned covers the stack: it leaves, newcomers wait.
    pub fn give_way(&mut self, now: f64) {
        for live in self.live.iter_mut().filter(|l| !l.going()) {
            live.gone = Some((now, Exit::Fade));
            live.dragging = false;
        }
        self.open = false;
    }

    /// Waiting toasts come in once clear; one that waited its time is let go.
    fn admit(&mut self, now: f64) {
        let waiting = std::mem::take(&mut self.waiting);
        for (toast, since) in waiting {
            let since = since.unwrap_or(now);
            match (self.blocked, now - since < toast.lasts()) {
                (_, false) => {}
                (true, true) => self.waiting.push((toast, Some(since))),
                (false, true) => self.put(toast),
            }
        }
    }

    /// The newest toast still up, or waiting to come in.
    pub fn current(&self) -> Option<&Toast> {
        let waiting = self.waiting.last().map(|(t, _)| t);
        waiting.or_else(|| self.shown().next())
    }

    /// The toasts still up, newest first.
    pub fn shown(&self) -> impl Iterator<Item = &Toast> {
        let up = self.live.iter().rev().filter(|l| !l.going());
        up.map(|l| &l.toast)
    }

    /// Toasts held back until what is in the way goes.
    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }

    /// Where the stack showed last frame; what sits there gives way.
    pub fn covers(&self) -> Option<Rect> {
        self.seen
    }

    /// Whether any toasts are alive or waiting to enter.
    pub fn active(&self) -> bool {
        self.live.iter().any(|l| !l.going()) || !self.waiting.is_empty()
    }

    /// Where toasts sit in `area`, if any are active.
    pub fn active_bed(&self, area: Rect, lift: f32) -> Option<Rect> {
        if !self.active() {
            return None;
        }
        Some(self.covers().unwrap_or_else(|| bed(area, lift)))
    }

    /// Opened into its list.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens the stack into its list, when there is one.
    pub fn set_open(&mut self, open: bool) {
        self.open = open && self.live.iter().any(|l| !l.going());
    }

    /// Read, opened or dragged: the clocks stop.
    fn held(&self) -> bool {
        self.hovered || self.open || self.live.iter().any(|l| l.dragging)
    }

    /// Runs the clocks unless held; ended toasts fade out.
    fn tick(&mut self, now: f64, fade: f64) {
        let held = self.held();
        let step = self.last.map_or(0.0, |last| (now - last).max(0.0));
        self.last = Some(now);
        for live in self.live.iter_mut().filter(|l| !l.going()) {
            live.born.get_or_insert(now);
            let Some(left) = live.left.as_mut() else {
                live.left = Some(live.toast.lasts());
                continue;
            };
            match held {
                true => *left = left.max(HOVER_GRACE),
                false => *left -= step,
            }
            if *left <= 0.0 {
                live.gone = Some((now, Exit::Fade));
            }
        }
        self.live
            .retain(|l| l.gone.is_none_or(|(at, _)| fade > 0.0 && now - at < fade));
        if self.live.iter().all(Live::going) {
            self.open = false;
        }
    }

    pub fn island_rect(
        &self,
        ctx: &Context,
        area: Rect,
        player: Option<Rect>,
        dict: Option<Rect>,
    ) -> Option<Rect> {
        let mut active = self.live.iter();
        let live = active.next()?;
        if active.next().is_some()
            || !self.waiting.is_empty()
            || !live.toast.player
            || live.toast.tone != Tone::Info
            || live.toast.undo.is_some()
        {
            return None;
        }
        let player = player?;
        let width = player.width() - 48.0;
        if width < 150.0 {
            return None;
        }
        let t = crate::theme::Tokens::get(ctx);
        let words = ctx.fonts_mut(|f| {
            f.layout_no_wrap(
                live.toast.message.clone(),
                egui::FontId::proportional(t.text_small),
                t.ink,
            )
        });
        if words.size().x + 46.0 > width {
            return None;
        }
        let rect = Rect::from_min_size(
            pos2(player.left() + 24.0, player.top() - 28.0),
            egui::vec2(width, 32.0),
        );
        if !area.contains_rect(rect) || dict.is_some_and(|r| r.intersects(rect)) {
            return None;
        }
        if open_menus(ctx)
            .iter()
            .chain(kept_clear(ctx).iter())
            .any(|r| r.intersects(rect))
        {
            return None;
        }
        Some(rect)
    }

    pub fn active_bed_for(
        &self,
        ctx: &Context,
        area: Rect,
        lift: f32,
        player: Option<Rect>,
        dict: Option<Rect>,
    ) -> Option<Rect> {
        if self.island_rect(ctx, area, player, dict).is_some() {
            None
        } else {
            self.active_bed(area, lift)
        }
    }

    /// Draws the stack in `area`'s bottom-right corner, `lift` above its foot.
    pub fn show(&mut self, ctx: &Context, area: Rect, lift: f32, out: &mut Commands) {
        self.show_with_player(ctx, area, lift, None, None, out);
    }

    pub fn show_with_player(
        &mut self,
        ctx: &Context,
        area: Rect,
        lift: f32,
        player: Option<Rect>,
        dict: Option<Rect>,
        out: &mut Commands,
    ) {
        let now = ctx.input(|i| i.time);
        let fade = f64::from(crate::motion::secs(ctx, FADE));
        let width = WIDTH.min(area.width() - 2.0 * INSET).max(200.0);
        let keep = kept(ctx);
        // The stack, standing on the page bar; a lone toast at least.
        let foot = area.bottom() - INSET - lift;
        let tall = self.seen.map_or(SPOT_H, |r| r.height().max(SPOT_H));
        let column = Rect::from_min_max(
            pos2(area.right() - INSET - width, foot - tall),
            pos2(area.right() - INSET, foot),
        );
        let island = self.island_rect(ctx, area, player, dict);
        let spot =
            island.unwrap_or_else(|| self.seen.filter(|r| r.intersects(column)).unwrap_or(column));
        let mut blockers = open_menus(ctx);
        blockers.extend(keep.clear.iter().map(|(_, r)| *r));
        self.blocked = blockers.iter().any(|b| b.intersects(spot));
        if self.blocked {
            self.give_way(now);
        }
        self.admit(now);
        if !self.waiting.is_empty() {
            ctx.request_repaint_after_secs(0.25);
        }
        self.tick(now, fade);
        if !ctx.egui_wants_keyboard_input() && !egui::Popup::is_any_open(ctx) {
            let undo = self
                .live
                .iter()
                .rev()
                .find(|l| !l.going())
                .filter(|l| l.toast.label == "Undo")
                .and_then(|l| {
                    l.toast
                        .undo
                        .as_deref()
                        .map(|command| (l.id, command.clone()))
                });
            if let Some((id, command)) = undo
                && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z))
            {
                out.push(Sent::Engine(command));
                out.push(Sent::Ui(UiCommand::CloseToast(id)));
            }
        }
        let opened = ctx.animate_bool_with_time(egui::Id::new("toasts-open"), self.open, FADE);
        if self.live.is_empty() {
            self.hovered = false;
            self.seen = None;
            return;
        }
        if let Some(rect) = self.island_rect(ctx, area, player, dict) {
            self.show_island(ctx, rect, now, fade);
            self.repaint(ctx, now, fade);
            return;
        }
        let corner = pos2(area.right() - INSET, area.bottom() - INSET - lift);
        // A drag from the page passes over the toasts, once it nears them.
        let ours = self.live.iter().any(|l| l.dragging);
        let page = ctx.dragged_id().is_some() && !ours;
        let drag = ctx.pointer_latest_pos().filter(|_| page);
        let place = stack::Place {
            corner,
            width,
            room: area.height() - 2.0 * INSET - lift,
            now,
            fade,
            drag,
        };
        self.hovered = if self.open {
            stack::open(ctx, self, &place, opened, out)
        } else {
            stack::closed(ctx, self, &place, out)
        };
        self.repaint(ctx, now, fade);
    }

    fn show_island(&mut self, ctx: &Context, rect: Rect, now: f64, fade: f64) {
        let Some(live) = self.live.last() else {
            return;
        };
        let t = crate::theme::Tokens::get(ctx);
        let progress = if fade <= 0.0 {
            1.0
        } else if let Some((at, _)) = live.gone {
            1.0 - ((now - at) / fade).clamp(0.0, 1.0) as f32
        } else {
            ((now - live.born.unwrap_or(now)) / fade).clamp(0.0, 1.0) as f32
        };
        let shown = rect.translate(egui::vec2(0.0, (1.0 - progress) * 12.0));
        let response = egui::Area::new(Id::new("toast-island"))
            .order(egui::Order::Middle)
            .fixed_pos(shown.min)
            .constrain(false)
            .show(ctx, |ui| {
                let (r, resp) = ui.allocate_exact_size(shown.size(), egui::Sense::hover());
                ui.set_opacity(progress);
                ui.painter().rect(
                    r,
                    egui::CornerRadius::same(12),
                    t.surface,
                    egui::Stroke::new(1.0, t.line),
                    egui::StrokeKind::Inside,
                );
                crate::icons::Icon::Done.paint(
                    ui,
                    Rect::from_center_size(
                        pos2(r.left() + 17.0, r.center().y),
                        egui::vec2(14.0, 14.0),
                    ),
                    t.accent,
                );
                ui.painter().text(
                    pos2(r.left() + 30.0, r.center().y),
                    egui::Align2::LEFT_CENTER,
                    &live.toast.message,
                    egui::FontId::proportional(t.text_small),
                    t.ink,
                );
                resp.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &live.toast.message)
                });
                ui.ctx().accesskit_node_builder(resp.id, |node| {
                    node.set_live(egui::accesskit::Live::Polite)
                });
                resp.contains_pointer()
            });
        self.hovered = response.inner;
        self.seen = Some(rect);
    }

    /// Every frame while something moves; often enough for a running line.
    fn repaint(&self, ctx: &Context, now: f64, fade: f64) {
        let moving = self.live.iter().any(|l| {
            let entering = l.born.is_none_or(|b| now - b < fade);
            l.going() || l.dragging || l.drag != 0.0 || entering
        });
        if moving {
            ctx.request_repaint();
            return;
        }
        if !self.held() && self.live.iter().any(|l| l.left.is_some()) {
            ctx.request_repaint_after_secs(LINE_STEP);
        }
    }
}

#[cfg(test)]
mod tests {
    use mareader_engine::toast::SHOW_FOR;

    use {super::*, mareader_engine::command::Command};

    const SCREEN: Rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(1200.0, 800.0));

    /// One frame at `time`, `block` summoned over the page first.
    fn frame(ctx: &Context, toasts: &mut Toasts, block: Option<Rect>, time: f64) {
        let input = egui::RawInput {
            screen_rect: Some(SCREEN),
            time: Some(time),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            if let Some(rect) = block {
                keep_clear(ui.ctx(), rect);
            }
            toasts.show(ui.ctx(), SCREEN, 0.0, &mut Commands::default());
        });
        out.textures_delta.clear();
    }

    fn up(toasts: &Toasts) -> usize {
        toasts.shown().count()
    }

    #[test]
    fn something_summoned_under_a_toast_sends_it_away_early() {
        let (ctx, mut toasts) = (Context::default(), Toasts::default());
        toasts.push(Toast::info("Took “Poetry” apart."));
        frame(&ctx, &mut toasts, None, 0.0);
        frame(&ctx, &mut toasts, None, 0.1);
        assert_eq!(up(&toasts), 1);
        // A bar along the foot, reaching under the corner.
        let bar = Rect::from_min_max(pos2(500.0, 730.0), pos2(1000.0, 776.0));
        frame(&ctx, &mut toasts, Some(bar), 0.2);
        assert_eq!(up(&toasts), 0, "it leaves long before its time");
        // Words that come meanwhile wait for the bar to go.
        toasts.push(Toast::info("Added to “Poetry”."));
        frame(&ctx, &mut toasts, Some(bar), 0.3);
        assert_eq!((up(&toasts), toasts.waiting()), (0, 1));
        let said = toasts.current().map(|t| t.message.as_str());
        assert_eq!(said, Some("Added to “Poetry”."));
        frame(&ctx, &mut toasts, None, 0.4);
        frame(&ctx, &mut toasts, None, 0.5);
        assert_eq!((up(&toasts), toasts.waiting()), (1, 0));
    }

    #[test]
    fn a_toast_kept_waiting_past_its_time_never_shows() {
        let (ctx, mut toasts) = (Context::default(), Toasts::default());
        let bar = Rect::from_min_max(pos2(500.0, 730.0), pos2(1200.0, 800.0));
        frame(&ctx, &mut toasts, Some(bar), 0.0);
        toasts.push(Toast::info("Copied"));
        frame(&ctx, &mut toasts, Some(bar), 0.1);
        frame(&ctx, &mut toasts, Some(bar), SHOW_FOR + 0.5);
        frame(&ctx, &mut toasts, None, SHOW_FOR + 0.6);
        assert_eq!((up(&toasts), toasts.waiting()), (0, 0));
    }

    #[test]
    fn toasts_stay_in_their_corner_bed() {
        let (ctx, mut toasts) = (Context::default(), Toasts::default());
        toasts.push(Toast::info("Copied"));
        frame(&ctx, &mut toasts, None, 0.0);
        let expected = bed(SCREEN, 0.0);
        let shown = toasts.covers().expect("toasts have a bounding box");
        assert!((shown.right() - expected.right()).abs() < 1.0);
        assert!((shown.bottom() - expected.bottom()).abs() < 1.0);
        assert!(toasts.active());
    }

    #[test]
    fn active_bed_reports_area_only_while_active() {
        let (ctx, mut toasts) = (Context::default(), Toasts::default());
        assert_eq!(toasts.active_bed(SCREEN, 0.0), None);
        toasts.push(Toast::info("Copied"));
        assert!(toasts.active_bed(SCREEN, 0.0).is_some());
        frame(&ctx, &mut toasts, None, 0.0);
        toasts.clear(0.1);
        frame(&ctx, &mut toasts, None, 0.5);
        assert_eq!(toasts.active_bed(SCREEN, 0.0), None);
    }

    #[test]
    fn far_from_the_corner_nothing_changes() {
        let (ctx, mut toasts) = (Context::default(), Toasts::default());
        toasts.push(Toast::info("Copied"));
        let menu = Rect::from_min_size(pos2(100.0, 100.0), egui::vec2(200.0, 300.0));
        frame(&ctx, &mut toasts, Some(menu), 0.0);
        frame(&ctx, &mut toasts, Some(menu), 0.1);
        assert_eq!(up(&toasts), 1);
    }

    #[test]
    fn one_line_and_two_line_toasts_keep_a_fixed_card_size() {
        let ctx = Context::default();
        let t = crate::theme::Tokens::get(&ctx);
        let mut short = Toasts::default();
        short.push(Toast::info("Copied"));
        frame(&ctx, &mut short, None, 0.0);
        let short_card = card::measure(&ctx, &short.live[0], WIDTH, &t);
        let short_box = short.covers().expect("short toast");

        let mut long = Toasts::default();
        long.push(
            Toast::info("Removed highlight from the second paragraph of chapter four").undo(
                Command::Study(mareader_engine::study::Action::Restore(Vec::new())),
            ),
        );
        frame(&ctx, &mut long, None, 0.0);
        let long_card = card::measure(&ctx, &long.live[0], WIDTH, &t);
        let long_box = long.covers().expect("long toast");

        assert_eq!(short_card.height, SPOT_H);
        assert_eq!(long_card.height, SPOT_H);
        assert_eq!(short_box.size(), long_box.size());
        assert_eq!(short_box.width(), WIDTH);
    }
}
