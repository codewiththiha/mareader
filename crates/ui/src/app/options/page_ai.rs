use super::*;

pub(super) fn set(app: &mut MareaderApp, key: &str, value: &str) -> Option<Result<(), String>> {
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
            self.page_ai.set_canned(true);
        }
        self.page_ai_seed = Some((page, task, follow, last));

        Ok(())
    }

    // Page AI runs stay mid-answer: `page_ai_hold=true`.
    fn option_page_ai_hold(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.page_ai.set_held(value == "true");
        Ok(())
    }
}
