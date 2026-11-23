//! Skills in the frame: their cards, the toast for one that lands out of
//! sight, and going back to it.

use super::*;
use egui::Context;
use mareader_engine::command::Command;
use mareader_engine::skills::{Action, Answer, AskId};
use mareader_engine::toast::Toast;

impl MareaderApp {
    /// After the page AI: every card whose place is on screen, and a card
    /// being brought back, once its page is laid out.
    pub(super) fn show_skills(&mut self, ctx: &Context, out: &mut Commands) {
        if self.route() != Route::Reader || self.session.skills().asks().is_empty() {
            self.ui.skill_reveal = None;
            return;
        }
        if let Some(id) = self.ui.skill_reveal {
            let at = self
                .session
                .skills()
                .get(id)
                .map(|a| (a.pane, a.anchor.reveal()));
            let done = match at {
                Some((pane, (page, chars))) => {
                    self.ui
                        .documents
                        .reveal_hit(&mut self.session, pane, page, chars)
                }
                None => true,
            };
            if done {
                self.ui.skill_reveal = None;
            } else {
                ctx.request_repaint();
            }
        }
        let docs = self.ui.documents.as_view(self.session.documents());
        let asks = self.session.skills().asks();
        crate::skill_card::show(ctx, asks, (&self.ui.pane_rects, docs), out);
    }

    /// An answer landed: in sight it shows itself; out of sight, a toast
    /// offers to go back to it.
    pub(super) fn skill_landed(&mut self, id: AskId) {
        let docs = self.ui.documents.as_view(self.session.documents());
        let Some(ask) = self.session.skills().get(id) else {
            return;
        };
        if crate::skill_card::in_view(ask, &self.ui.pane_rects, docs).is_some() {
            return;
        }
        let toast = match ask.answer {
            Answer::Failed(_) => Toast::warning(format!("{} didn't finish", ask.name)),
            _ => Toast::info(format!("{} is ready", ask.name)),
        };
        let show = Command::Skill(Action::Show(id));
        self.ui.toasts.push(toast.action("Show", show));
    }
}
