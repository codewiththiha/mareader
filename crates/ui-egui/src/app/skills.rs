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

/// The skills' own keys: who answers, the reader's skills, and a skill run
/// on the staged selection or a page.
///
/// `None` for a key that is not one of them, so the next family is asked.
pub(super) fn own_key(app: &mut MareaderApp, key: &str, value: &str) -> Option<Result<(), String>> {
    match key {
        "skill_answers" => Some(app.option_skill_answers(value)),
        "custom_skills" => Some(app.option_custom_skills(value)),
        "skill" => Some(app.option_skill(value)),
        "page_skill" => Some(app.option_page_skill(value)),
        _ => None,
    }
}

impl MareaderApp {
    // Who answers skills: `skill_answers=sample`, echo, silent or failing.
    fn option_skill_answers(&mut self, value: &str) -> Result<(), String> {
        let hint = "skill_answers: sample, echo, silent, failing or none";
        let script = mareader_study::skill::Scripted::parse(value).ok_or(hint)?;
        self.session.skills_set_runner(std::sync::Arc::new(script));
        Ok(())
    }

    /// Skills by name, `_` for a space: `custom_skills=Quiz_me,Grammar`.
    fn option_custom_skills(&mut self, value: &str) -> Result<(), String> {
        let skills: Vec<_> = (value.split(',').enumerate())
            .map(|(i, name)| {
                serde_json::json!({
                    "id": i + 1,
                    "name": name.replace('_', " "),
                    "prompt": "Explain this passage to a learner.",
                })
            })
            .collect();
        self.queue(Sent::Engine(Command::SetField {
            path: "ai.tasks.skills.custom".into(),
            value: serde_json::Value::Array(skills).to_string(),
        }));
        Ok(())
    }

    /// A skill on the staged selection: `selection=0:quick skill=translate`.
    fn option_skill(&mut self, value: &str) -> Result<(), String> {
        let hint = "skill: translate, summarize, simplify or custom-N, after selection";
        let skill = mareader_settings::Skill::parse(value).ok_or(hint)?;
        let staged = self.ui.staged.back_mut().ok_or(hint)?;
        staged.skill = Some(skill);
        Ok(())
    }

    /// A skill on a page, its card at a spot: `page_skill=0:120:300:custom-1`.
    fn option_page_skill(&mut self, value: &str) -> Result<(), String> {
        let hint = "page_skill: page:x:y:skill, as 0:120:300:custom-1";
        let parts: Vec<&str> = value.splitn(4, ':').collect();
        let [page, x, y, skill] = parts.as_slice() else {
            return Err(hint.into());
        };
        let skill = mareader_settings::Skill::parse(skill).ok_or(hint)?;
        let number = |s: &str| s.parse::<f32>().map_err(|_| hint.to_owned());
        self.ui.staged.push_back(Staged {
            page: page.parse().map_err(|_| hint)?,
            skill: Some(skill),
            spot: Some((number(x)?, number(y)?)),
            ..Staged::default()
        });
        Ok(())
    }
}
