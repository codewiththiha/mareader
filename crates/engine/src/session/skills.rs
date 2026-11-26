//! Skills on a selection or a page inside [`Session`].

use mareader_settings::Skill;
use mareader_workspace::PaneId;

use crate::event::Event;
use crate::session::Done;
use crate::skills::{Action, Anchor, AskId, Asked, State};
use crate::toast::Toast;

use super::Session;

impl Session {
    /// Skills asked, and their answers, read-only.
    pub fn skills(&self) -> &State {
        &self.skills
    }

    pub(crate) fn skills_act(&mut self, action: Action) -> Done {
        let changed = match action {
            Action::Run { pane, skill } => self.skill_on_selection(pane, skill),
            Action::RunPage {
                pane,
                page,
                x,
                y,
                skill,
            } => self.skill_on_page(pane, (page, x, y), skill),
            Action::Retry(id) => self.skills.retry(id),
            Action::Close(id) => self.skills.close(id),
            Action::Cancel(id) => self.skill_cancel(id),
            Action::Remove(id) => self.skill_remove(id),
            Action::Pin(id) => self.skill_pin(id, true),
            Action::Show(id) => {
                let known = self.skills.show(id);
                if known {
                    self.emit(Event::SkillShow(id));
                }
                known
            }
        };
        if changed {
            Done::changed()
        } else {
            Done::quiet()
        }
    }

    /// Runs `skill` on the pane's selection; its card points there.
    fn skill_on_selection(&mut self, pane: PaneId, skill: Skill) -> bool {
        let Some((text, spans)) = self.documents().selected_text_spans(pane) else {
            return false;
        };
        let spot = self.documents().highlighted(pane).map(|(spot, _)| spot);
        self.clear_document_selection(pane);
        self.skill_start(pane, skill, Anchor::Text(spans), spot, text)
    }

    /// Runs `skill` on one page; its card points where the menu opened.
    fn skill_on_page(
        &mut self,
        pane: PaneId,
        (page, x, y): (usize, f32, f32),
        skill: Skill,
    ) -> bool {
        let Some(layer) = self.documents().layer(pane, page) else {
            return false;
        };
        let text = layer.text.text(0..layer.text.len());
        if text.trim().is_empty() {
            self.emit(Event::Toast(Toast::info("This page has no text to read.")));
            return false;
        }
        let far = |c: &mareader_pdf::TextChar| ((c.bounds.y0 + c.bounds.y1) / 2.0 - y).abs();
        let near = (layer.text.chars().iter().enumerate())
            .min_by(|(_, a), (_, b)| far(a).total_cmp(&far(b)))
            .map_or(0, |(i, _)| i);
        let anchor = Anchor::Spot { page, x, y, near };
        self.skill_start(pane, skill, anchor, None, text)
    }

    fn skill_start(
        &mut self,
        pane: PaneId,
        skill: Skill,
        anchor: Anchor,
        spot: Option<mareader_library::Spot>,
        text: String,
    ) -> bool {
        let Some(doc) = self.documents().place(pane).map(|p| p.doc.get()) else {
            return false;
        };
        let Some(name) = self.settings().ai.skill_name(skill) else {
            self.emit(Event::Toast(Toast::warning("That skill is gone.")));
            return false;
        };
        let book = self.book_in(pane);
        let path = self.documents().path(pane).map(ToOwned::to_owned);
        let layout = self
            .documents()
            .document_of(pane)
            .map(|d| std::sync::Arc::downgrade(&d));
        let id = self.skills.start(Asked {
            pane,
            doc,
            skill,
            name,
            anchor,
            book,
            spot,
            path,
            layout,
            text,
        });
        if spot.is_some() && self.settings().ai.tasks.skills.pin_by_default {
            self.skill_pin(id, false);
        }
        self.emit(Event::SkillOpened(id));
        true
    }

    /// Takes in answers; drops asks whose book left their pane.
    pub(crate) fn skills_receive(&mut self) -> bool {
        let docs = &self.documents;
        let reads = |pane| docs.place(pane).map(|p| p.doc.get());
        let mut changed = self.skills.keep_read(reads);
        let views = self
            .tree
            .panes()
            .into_iter()
            .filter_map(|pane| {
                Some(crate::skills::restore::View {
                    pane,
                    doc: self.documents().place(pane)?.doc.get(),
                    book: self.book_in(pane),
                    path: self.documents().path(pane).map(ToOwned::to_owned),
                    document: self.documents().document_of(pane)?,
                })
            })
            .collect();
        changed |= self.skills.refresh(views, self.library.model());
        let (landed, news) = self.skills.receive();
        changed |= news;
        for id in landed {
            self.skill_save_landed(id);
            self.emit(Event::SkillLanded(id));
        }
        changed
    }

    /// Who answers skills from now on, as a shot scripts them.
    pub fn skills_set_runner(
        &mut self,
        runner: std::sync::Arc<dyn mareader_study::skill::Skiller>,
    ) {
        self.skills.set_runner(runner);
    }

    /// Whether `id` is still asked.
    pub fn skill_asked(&self, id: AskId) -> bool {
        self.skills.get(id).is_some()
    }
}
