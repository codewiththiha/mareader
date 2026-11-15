use egui::Context;

use super::*;

use crate::page_ai::task_for;
use mareader_engine::page_ai::Action;
use mareader_settings::{PageStart, PageTask};

impl MareaderApp {
    /// What the page AI's menus asked for.
    pub(super) fn page_ai_act(&mut self, ctx: &Context, action: mareader_engine::page_ai::Action) {
        let now = ctx.input(|i| i.time);
        let task = match &action {
            Action::Run { task, .. } => task_for(*task, &self.settings().ai),
            Action::Translate { tag, .. } => {
                let mut ai = self.settings().ai.clone();
                ai.tasks.translate.pick(tag);
                let _ = self.set_field("ai", ai);
                Some(mareader_library::AiTask::Translate {
                    language: tag.clone(),
                })
            }
            _ => None,
        };
        if matches!(&action, Action::Run { .. } | Action::Translate { .. }) {
            self.page_ai_run(action, task, now);
        } else if matches!(&action, Action::RunPages { .. } | Action::PagesMenu { .. }) {
            self.page_ai_pages(action);
        } else {
            self.page_ai_result(action, now);
        }
    }

    fn page_ai_run(&mut self, action: Action, task: Option<mareader_library::AiTask>, now: f64) {
        match action {
            Action::Run {
                pane, page, how, ..
            }
            | Action::Translate {
                pane, page, how, ..
            } => {
                let Some(task) = task else { return };
                let kind = mareader_engine::page_ai::view::page_task(&task);
                if how.remember {
                    let start = match how.continuous {
                        true => PageStart::Continuous,
                        false => PageStart::Page,
                    };
                    let mut tasks = self.settings().ai.tasks.clone();
                    match kind {
                        PageTask::Translate => tasks.translate.start = start,
                        PageTask::Simplify => tasks.simplify.start = start,
                        PageTask::Summarize => {}
                    }
                    let _ = self.set_field("ai.tasks", tasks);
                }
                self.session.execute(Command::ClearSelection(pane), 0.0);
                match how.continuous && kind != PageTask::Summarize {
                    true => self
                        .session
                        .page_ai_follow(pane, page, &self.ui.ai, task, true, now),
                    false => self.session.page_ai_run(pane, page, &self.ui.ai, task, now),
                }
            }
            _ => {}
        }
    }

    fn page_ai_pages(&mut self, action: Action) {
        match action {
            Action::RunPages {
                pane,
                pages,
                task,
                tag,
            } => {
                let task = match (task, tag) {
                    (PageTask::Translate, Some(tag)) => {
                        let mut ai = self.settings().ai.clone();
                        ai.tasks.translate.pick(&tag);
                        let _ = self.set_field("ai", ai);
                        Some(mareader_library::AiTask::Translate { language: tag })
                    }
                    (task, _) => {
                        let ai = self.settings().ai.clone();
                        task_for(task, &ai)
                    }
                };
                let Some(task) = task else { return };
                let page = pages.first().copied().unwrap_or_default();
                self.session
                    .page_ai_run_pages(pane, page, &self.ui.ai, &pages, task);
            }
            Action::PagesMenu { pane, pages, at } => {
                self.ui.page_ai.open_pages_menu(pane, pages, at)
            }
            _ => {}
        }
    }

    fn page_ai_result(&mut self, action: Action, now: f64) {
        if let Action::Redo { pane, page, slot } = action {
            self.session
                .page_ai_again(pane, page, &self.ui.ai, slot, now);
        }
    }
}

/// The page ai's own keys: what reads this window's own state.
///
/// `None` for a key that is not one of them, so the next family is asked.
pub(super) fn own_key(app: &mut MareaderApp, key: &str, value: &str) -> Option<Result<(), String>> {
    match key {
        "page_ai" => Some(app.option_page_ai(key, value)),
        "page_ai_hold" => Some(app.option_page_ai_hold(key, value)),
        _ => None,
    }
}

impl MareaderApp {
    // A kept result, `3:simplify`; a canned run, `follow:simplify`.
    fn option_page_ai(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let (page, task) = value.split_once(':').ok_or("page_ai: page:task")?;
        let follow = page == "follow";
        // A range, as 2-6, asks those pages as picked ones.
        let (page, last) = page.split_once('-').unwrap_or((page, page));
        let number = |p: &str| p.parse::<usize>().map_err(|e| e.to_string());
        let (page, last) = match follow {
            true => (0, 0),
            false => (number(page)?, number(last)?),
        };
        let task = match task {
            "summary" => mareader_library::AiTask::Summary,
            "points" => mareader_library::AiTask::Points,
            "simplify" => mareader_library::AiTask::Simplify,
            tag => mareader_library::AiTask::Translate {
                language: tag.to_owned(),
            },
        };
        if follow || last > page {
            self.session.page_ai_set_canned(true);
        }
        self.ui.page_ai_seed = Some((page, task, follow, last));

        Ok(())
    }

    // Page AI runs stay mid-answer: `page_ai_hold=true`.
    fn option_page_ai_hold(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.session.page_ai_set_held(value == "true");
        Ok(())
    }
}
