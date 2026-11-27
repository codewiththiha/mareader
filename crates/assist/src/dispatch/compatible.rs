//! Re-resolve a pinned model after its capabilities arrive.

use mareader_settings::{Provider, TaskId};
use std::sync::Arc;

use super::{Dispatcher, Followed};

impl Dispatcher {
    /// Re-resolves a pinned task only if its Apple target is now unsupported.
    /// The original models and other settings stay fixed.
    pub fn compatible(&self, original: &Followed, task: TaskId) -> Option<Arc<Followed>> {
        if original.route(task).first()?.preset.provider != Provider::Apple {
            return None;
        }
        let state = crate::board::AppleState::Ready(self.apple_languages.lock().ok()?.clone()?);
        let old = match task {
            TaskId::Explain => &original.ai.tasks.explain.language,
            TaskId::StudyCards => &original.ai.tasks.study.language,
            _ => return None,
        };
        let tag = old.as_deref()?;
        if state.speaks(tag) != Some(false) {
            return None;
        }
        let chosen = mareader_settings::effective_language(Some(tag), None, &|t| state.speaks(t))?;
        let mut next = original.clone();
        match task {
            TaskId::Explain => next.ai.tasks.explain.language = Some(chosen.to_owned()),
            TaskId::StudyCards => next.ai.tasks.study.language = Some(chosen.to_owned()),
            _ => return None,
        }
        Some(Arc::new(next))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::Nobody;
    use crate::dispatch::Seat;
    use mareader_settings::{AiSettings, PresetId};
    use mareader_study::Why;

    #[test]
    fn a_late_probe_repins_only_the_original_apple_request() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let d = Dispatcher::new(rt, Arc::new(|| {}));
        let mut ai = AiSettings::default();
        ai.presets.clear();
        let apple = ai.add_preset(Provider::Apple, "");
        let other = ai.add_preset(Provider::Ollama, "new");
        ai.tasks.choose(TaskId::Explain, Some(apple));
        ai.tasks.choose(TaskId::StudyCards, Some(apple));
        ai.tasks.explain.language = Some("my".into());
        ai.tasks.study.language = Some("fr".into());
        let make = |p: &mareader_settings::ModelPreset| Seat {
            preset: p.clone(),
            backend: Arc::new(Nobody(Why::NotHere.into())),
        };
        d.follow(&ai, make);
        let old = d.followed();
        assert!(d.compatible(&old, TaskId::Explain).is_none(), "unknown");
        // A new preference does not change the already pinned route.
        ai.tasks.choose(TaskId::Explain, Some(other));
        ai.tasks.explain.language = Some("ja".into());
        d.follow(&ai, make);
        d.apple_languages(Some(vec!["en-US".into(), "ja-JP".into()]));
        let explain = d.compatible(&old, TaskId::Explain).unwrap();
        assert_eq!(explain.ai.tasks.explain.language.as_deref(), Some("en"));
        assert_eq!(
            explain.route(TaskId::Explain)[0].preset.id,
            PresetId(apple.0)
        );
        assert_eq!(old.ai.tasks.explain.language.as_deref(), Some("my"));
        assert!(d.compatible(&explain, TaskId::Explain).is_none(), "no loop");
        let cards = d.compatible(&old, TaskId::StudyCards).unwrap();
        assert_eq!(cards.ai.tasks.study.language.as_deref(), Some("en"));
        assert_eq!(
            d.followed().ai.tasks.explain.language.as_deref(),
            Some("ja")
        );
        d.apple_languages(Some(vec!["ja-JP".into()]));
        assert_eq!(
            d.compatible(&old, TaskId::Explain)
                .unwrap()
                .ai
                .tasks
                .explain
                .language
                .as_deref(),
            Some("ja")
        );
        d.apple_languages(Some(vec![]));
        assert!(
            d.compatible(&old, TaskId::Explain).is_none(),
            "no supported catalog language"
        );
    }
}
