use super::*;

use crate::page_ai::{Action, task_for};
use mareader_settings::{PageStart, PageTask};

impl MareaderApp {
    /// What the page AI's menus asked for.
    pub(super) fn page_ai_act(&mut self, ctx: &Context, action: crate::page_ai::Action) {
        let now = ctx.input(|i| i.time);
        let mut out = Commands::new();
        let task = match &action {
            Action::Run { task, .. } => task_for(*task, &self.settings.ai),
            Action::Translate { tag, .. } => {
                self.settings.ai.tasks.translate.pick(tag);
                Some(mareader_library::AiTask::Translate {
                    language: tag.clone(),
                })
            }
            _ => None,
        };
        if matches!(&action, Action::Run { .. } | Action::Translate { .. }) {
            self.page_ai_run(action, task, now, &mut out);
        } else if matches!(
            &action,
            Action::StopFollow(_) | Action::RunPages { .. } | Action::PagesMenu { .. }
        ) {
            self.page_ai_pages(action);
        } else {
            self.page_ai_result(ctx, action, now, &mut out);
        }
        self.queued.extend(out);
    }

    fn page_ai_run(
        &mut self,
        action: Action,
        task: Option<mareader_library::AiTask>,
        now: f64,
        out: &mut Commands,
    ) {
        match action {
            Action::Run {
                pane, page, how, ..
            }
            | Action::Translate {
                pane, page, how, ..
            } => {
                let Some(task) = task else { return };
                let kind = crate::page_ai::page_task(&task);
                if how.remember {
                    let start = match how.continuous {
                        true => PageStart::Continuous,
                        false => PageStart::Page,
                    };
                    let tasks = &mut self.settings.ai.tasks;
                    match kind {
                        PageTask::Translate => tasks.translate.start = start,
                        PageTask::Simplify => tasks.simplify.start = start,
                        PageTask::Summarize => {}
                    }
                }
                let book = self.library.book_in(&self.documents, pane);
                let name = self.documents.title(pane).unwrap_or_default().to_owned();
                self.documents.clear_selection(pane);
                let asked = crate::page_ai::Asked {
                    pane,
                    page,
                    book,
                    name: &name,
                    documents: &self.documents,
                    ai: &self.ai,
                    settings: &self.settings.ai,
                };
                match how.continuous && kind != PageTask::Summarize {
                    true => {
                        let model = self.library.model();
                        self.page_ai.follow(asked, model, task, true, now, out);
                    }
                    false => self.page_ai.run(asked, task, now, out),
                }
            }
            _ => {}
        }
    }

    fn page_ai_pages(&mut self, action: Action) {
        match action {
            Action::StopFollow(pane) => self.page_ai.stop_follow(pane),
            Action::RunPages {
                pane,
                pages,
                task,
                tag,
            } => {
                let task = match (task, tag) {
                    (PageTask::Translate, Some(tag)) => {
                        self.settings.ai.tasks.translate.pick(&tag);
                        Some(mareader_library::AiTask::Translate { language: tag })
                    }
                    (task, _) => task_for(task, &self.settings.ai),
                };
                let Some(task) = task else { return };
                let book = self.library.book_in(&self.documents, pane);
                let name = self.documents.title(pane).unwrap_or_default().to_owned();
                let asked = crate::page_ai::Asked {
                    pane,
                    page: pages.first().copied().unwrap_or_default(),
                    book,
                    name: &name,
                    documents: &self.documents,
                    ai: &self.ai,
                    settings: &self.settings.ai,
                };
                self.page_ai.run_pages(asked, &pages, task);
            }
            Action::PagesMenu { pane, pages, at } => self.page_ai.open_pages_menu(pane, pages, at),
            _ => {}
        }
    }

    fn page_ai_result(&mut self, ctx: &Context, action: Action, now: f64, out: &mut Commands) {
        match action {
            Action::StopPages(pane) => self.page_ai.stop_groups(pane),
            Action::Redo { pane, page, slot } => {
                let book = self.library.book_in(&self.documents, pane);
                let name = self.documents.title(pane).unwrap_or_default().to_owned();
                let asked = crate::page_ai::Asked {
                    pane,
                    page,
                    book,
                    name: &name,
                    documents: &self.documents,
                    ai: &self.ai,
                    settings: &self.settings.ai,
                };
                let model = self.library.model();
                self.page_ai.again(asked, model, slot, now, out);
            }
            Action::Show { pane, page, slot } => self.page_ai.show_result(pane, page, slot, now),
            Action::Copy(slot) => {
                if let Some(text) = self.page_ai.text_of(self.library.model(), slot) {
                    ctx.copy_text(text);
                    out.push(Command::Notify(Toast::info("Copied")));
                }
            }
            Action::Delete(slot) => self.page_ai.delete(self.library.model_mut(), slot),

            _ => {}
        }
    }
}
