//! The AI's corner stacks in the frame, after every visual of its own.

use super::*;
use crate::activity::stack::{self, At, Button, Line};
use crate::ui_command::UiCommand;
use egui::{Context, Id};
use mareader_engine::activity::place::Spot;
use mareader_engine::activity::{Key, News};
use mareader_engine::command::Command;
use mareader_engine::library::Action;
use mareader_engine::page_ai::Action as PageAi;
use mareader_engine::skills::Action as Skill;
use mareader_engine::study::state::Key as Ask;

impl MareaderApp {
    /// The stacks of the AI's work its own visuals leave unseen.
    pub(super) fn show_activity(&mut self, ctx: &Context, out: &mut Commands) {
        let now = ctx.input(|i| i.time);
        if self.route() == Route::Library {
            // The library page shows its organizer's card and its plan.
            let organize = |key: Key| matches!(key, Key::Organize { .. });
            self.ui.activity.in_view_where(ctx, now, &organize);
        }
        // The word card open on a word says how its answer comes.
        if let Some((pane, ask)) = self.ui.study.shown_card() {
            crate::activity::seen::in_view(ctx, Key::Explain { pane, ask });
        }
        let jobs = self.session.activities(now);
        let session = &self.session;
        let ending = |key| session.activity_ending(key);
        for news in self.ui.activity.update(ctx, jobs, &ending) {
            let toast = match news {
                News::Failed(a) => Toast::error(a.failed_said()),
                News::Stopped(a) => Toast::info(a.stopped_said()),
            };
            self.ui.toasts.push(toast);
        }
        let panes: Vec<PaneId> = self.ui.pane_rects.iter().map(|(p, _)| *p).collect();
        for (spot, lines) in self.ui.activity.stacks(ctx, &panes) {
            let Some(at) = self.stack_at(ctx, spot) else {
                continue;
            };
            self.show_stack(ctx, at, &lines, out);
        }
        if self.ui.activity.lingering(now) {
            ctx.request_repaint_after_secs(0.1);
        }
    }

    fn show_stack(&mut self, ctx: &Context, at: At, lines: &[Line], out: &mut Commands) {
        let now = ctx.input(|i| i.time);
        let shown = stack::show(ctx, at, lines);
        if shown.hovered {
            let keys: Vec<Key> = lines.iter().map(|l| l.key).collect();
            self.ui.activity.hold(&keys, now);
        }
        if let Some((button, key)) = shown.pressed {
            self.press_activity(button, key, now, out);
        }
    }

    /// Whether `pane`'s page bar is up, which the stack rises over.
    fn bar_up(&self, pane: PaneId) -> bool {
        !self.ui.chromeless && self.ui.page_bars.get(&pane).is_some_and(|b| b.shown())
    }

    /// Where `spot`'s stack stands: a pane's corner, or the workspace's,
    /// the bottom left pane's while the panes show.
    fn stack_at(&self, ctx: &Context, spot: Spot) -> Option<At> {
        let pane_at = |pane: PaneId, rect: Rect| At {
            id: stack::id(pane),
            rect,
            bar: self.bar_up(pane),
            taken: crate::activity::seen::taken(ctx, pane),
        };
        let rects = &self.ui.pane_rects;
        match spot {
            Spot::Pane(pane) => {
                let (_, rect) = rects.iter().find(|(p, _)| *p == pane)?;
                Some(pane_at(pane, *rect))
            }
            Spot::Workspace => {
                let whole = rects.iter().map(|(_, r)| *r).reduce(|a, b| a.union(b));
                let Some(whole) = whole else {
                    return Some(workspace_at(ctx));
                };
                let corner = whole.left_bottom() + egui::vec2(24.0, -24.0);
                let (pane, _) = rects.iter().find(|(_, r)| r.contains(corner))?;
                Some(At {
                    rect: whole,
                    ..pane_at(*pane, whole)
                })
            }
        }
    }

    fn press_activity(&mut self, button: Button, key: Key, now: f64, out: &mut Commands) {
        match (button, key) {
            (Button::View, Key::Cards { pane, .. } | Key::Page { pane, .. })
            | (Button::View, Key::Pages { pane, .. } | Key::Follow { pane, .. })
            | (Button::View, Key::Judge { pane, .. }) => {
                if let Some(page) = self.ui.activity.view(key, now) {
                    out.push(Sent::Engine(Command::GoTo { pane, page }));
                }
            }
            (Button::View, Key::Organize { .. }) => {
                self.ui.activity.view(key, now);
                out.push(Sent::Ui(UiCommand::ShowLibrary));
            }
            (Button::Cancel, Key::Cards { pane, .. } | Key::Judge { pane, .. }) => {
                self.ui.study_mode.ask_off(pane);
            }
            (Button::View, Key::Explain { pane, ask }) => {
                let page = self.ui.activity.view(key, now);
                if let (Some(page), Ask::Mark(mark)) = (page, ask) {
                    out.push(Sent::Engine(Command::GoTo { pane, page }));
                    self.ui.study.reopen(&mut self.session, pane, mark);
                }
            }
            (Button::Cancel, Key::Organize { .. }) => {
                let stop = Command::Library(Action::StopOrganizing);
                out.push(Sent::Engine(stop));
            }
            (Button::Cancel, Key::Explain { ask, .. }) => {
                self.session.study_cancel(ask);
            }
            (Button::View, Key::Skill { ask, .. }) => {
                self.ui.activity.view(key, now);
                out.push(Sent::Engine(Command::Skill(Skill::Show(ask))));
            }
            (Button::Cancel, Key::Skill { ask, .. }) => {
                out.push(Sent::Engine(Command::Skill(Skill::Close(ask))));
            }
            (Button::Cancel, Key::Page { run, .. }) => page_ai(out, PageAi::StopRun(run)),
            (Button::Cancel, Key::Pages { pane, .. }) => page_ai(out, PageAi::StopPages(pane)),
            (Button::Cancel, Key::Follow { pane, .. }) => page_ai(out, PageAi::StopFollow(pane)),
            (Button::Continue, Key::Follow { pane, .. }) => {
                if let Some(page) = self.session.documents().current(pane) {
                    self.session.page_ai_resume(pane, page, &self.ui.ai, now);
                }
            }
            (Button::Continue, _) => {}
            (Button::Dismiss, _) => self.ui.activity.dismiss(key, now),
        }
    }
}

fn page_ai(out: &mut Commands, action: PageAi) {
    out.push(Sent::Engine(Command::PageAi(action)));
}

/// The workspace's corner away from the reader: the window's, under the bar.
fn workspace_at(ctx: &Context) -> At {
    let mut rect = ctx.content_rect();
    rect.min.y += theme::TITLE_BAR_H;
    At {
        id: Id::new("activity-stack-workspace"),
        rect,
        bar: false,
        taken: None,
    }
}
