//! Skills on a selection or a page, off the UI thread.
//!
//! Each runs as Selection AI, in its place in line; its route starts at the
//! skill's own model when it has one. Dropping a job frees its place.

use std::sync::Arc;

use mareader_settings::{Skill, TaskId, no_model};
use mareader_study::skill::{SkillAsk, SkillJob, Skiller};
use mareader_study::{Failure, Why};

use super::Assist;
use crate::backend::Backend;
use crate::dispatch::Dispatcher;
use crate::retry;
use crate::running::{Running, Say, start};
use crate::skill::{SkillTask, rule_for};

impl Assist {
    /// Who answers skills, on the settings followed when each starts.
    pub fn skills(&self) -> Arc<dyn Skiller> {
        Arc::new(AiSkills(self.jobs_on.clone()))
    }
}

struct AiSkills(Dispatcher);

impl Skiller for AiSkills {
    fn by(&self, skill: &str) -> String {
        let skill = Skill::parse(skill);
        skill
            .map(|s| self.0.followed().skill_by(s))
            .unwrap_or_default()
    }

    fn run(&self, ask: SkillAsk) -> Box<SkillJob> {
        let now = self.0.followed();
        let skill = Skill::parse(&ask.skill);
        let rule = skill.and_then(|s| rule_for(&now.ai, s, ask.language.as_deref()));
        let route = skill.map(|s| now.skill_route(s)).unwrap_or_default();
        let failed = match (&rule, route.is_empty()) {
            (None, _) => Some(Failure::detailed(
                Why::Setup,
                "This skill has no prompt yet.",
            )),
            (Some(_), true) => Some(Failure::detailed(
                Why::NotHere,
                no_model(TaskId::SelectionAi),
            )),
            (Some(_), false) => None,
        };
        if let Some(failure) = failed {
            let (rt, waker) = (self.0.rt(), &self.0.waker);
            let job: Running<String> = start(rt, waker, |_| async { Err(failure) });
            return Box::new(job);
        }
        let task = Arc::new(SkillTask {
            rule: rule.unwrap_or_default(),
            text: ask.text,
        });
        let step = move |backend: Arc<dyn Backend>, retry, say: Say<String, String>| {
            let task = Arc::clone(&task);
            async move {
                let partial = |t: String| say.partial(t);
                retry::attempt(backend.as_ref(), task.as_ref(), &partial, retry).await
            }
        };
        let who = now.asker(TaskId::SelectionAi);
        let job: Running<String> = self.0.dispatch().ask(who, route, step);
        Box::new(job)
    }
}
