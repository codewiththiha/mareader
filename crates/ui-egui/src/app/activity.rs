//! The AI's corner stacks in the frame, after every visual of its own.

use super::*;
use crate::activity::stack::{self, At, Button};
use egui::Context;
use mareader_engine::activity::{Key, News};
use mareader_engine::command::Command;

impl MareaderApp {
    /// Each pane's stack of the AI's work its own visuals leave unseen.
    pub(super) fn show_activity(&mut self, ctx: &Context, out: &mut Commands) {
        let now = ctx.input(|i| i.time);
        let jobs = self.session.activities();
        let session = &self.session;
        let landed = |key| session.activity_landed(key);
        for news in self.ui.activity.update(ctx, jobs, &landed) {
            let toast = match news {
                News::Failed(a) => Toast::error(a.failed_said()),
                News::Stopped(a) => Toast::info(a.stopped_said()),
            };
            self.ui.toasts.push(toast);
        }
        let panes = self.ui.pane_rects.clone();
        for (pane, rect) in panes {
            let lines = self.ui.activity.lines(ctx, pane);
            if lines.is_empty() {
                continue;
            }
            let bar =
                !self.ui.chromeless && self.ui.page_bars.get(&pane).is_some_and(|b| b.shown());
            let taken = crate::activity::seen::taken(ctx, pane);
            let shown = stack::show(
                ctx,
                At {
                    pane,
                    rect,
                    bar,
                    taken,
                },
                &lines,
            );
            if shown.hovered {
                let keys: Vec<Key> = lines.iter().map(|l| l.key).collect();
                self.ui.activity.hold(&keys, now);
            }
            if let Some((button, key)) = shown.pressed {
                self.press_activity(button, key, now, out);
            }
        }
        if self.ui.activity.lingering(now) {
            ctx.request_repaint_after_secs(0.1);
        }
    }

    fn press_activity(&mut self, button: Button, key: Key, now: f64, out: &mut Commands) {
        let Key::Cards { pane, .. } = key;
        match button {
            Button::View => {
                if let Some(page) = self.ui.activity.view(key, now) {
                    out.push(Sent::Engine(Command::GoTo { pane, page }));
                }
            }
            Button::Cancel if key.asks_to_stop() => self.ui.study_mode.ask_off(pane),
            Button::Cancel => {}
            Button::Dismiss => self.ui.activity.dismiss(key, now),
        }
    }
}
