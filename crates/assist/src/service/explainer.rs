//! Explain's answerer for the study tools.

use std::sync::Arc;

use mareader_settings::TaskId;
use mareader_study::{Explainer, Failure, Pos, WordInfo};

use crate::backend::Backend;
use crate::dispatch::{Dispatcher, Followed};
use crate::retry::attempt;
use crate::running::Say;
use crate::task::Explain;

/// The study tools' answerer: blocks its worker thread, not the UI.
pub(super) struct AiExplainer(pub(super) Dispatcher, pub(super) Option<Arc<Followed>>);

impl AiExplainer {
    fn now(&self) -> Arc<Followed> {
        self.1.clone().unwrap_or_else(|| self.0.followed())
    }
}

impl Explainer for AiExplainer {
    fn snapshot(&self) -> Option<Arc<dyn Explainer>> {
        Some(Arc::new(Self(self.0.clone(), Some(self.now()))))
    }

    fn compatible_snapshot(&self) -> Option<Arc<dyn Explainer>> {
        let now = self.now();
        let next = self.0.compatible(&now, TaskId::Explain)?;
        Some(Arc::new(Self(self.0.clone(), Some(next))))
    }

    fn explain(
        &self,
        word: &str,
        context: &str,
        partial: &(dyn Fn(WordInfo) + Sync),
    ) -> Result<WordInfo, Failure> {
        let said = self.explain_by((word, context, None), partial);
        said.map(|(info, _)| info)
    }

    fn explain_by(
        &self,
        words: (&str, &str, Option<Pos>),
        partial: &(dyn Fn(WordInfo) + Sync),
    ) -> Result<(WordInfo, String), Failure> {
        self.explain_until(words, partial, &|| false)
    }

    /// Dropping the job once `stop` says so aborts its request and frees
    /// its model.
    fn explain_until(
        &self,
        (word, context, hint): (&str, &str, Option<Pos>),
        partial: &(dyn Fn(WordInfo) + Sync),
        stop: &(dyn Fn() -> bool + Sync),
    ) -> Result<(WordInfo, String), Failure> {
        let now = self.now();
        let language = now.ai.explain_language();
        let tag = now.ai.tasks.explain.language.clone();
        let (word, context): (Arc<str>, Arc<str>) = (word.into(), context.into());
        let step = move |backend: Arc<dyn Backend>, retry, say: Say<WordInfo, WordInfo>| {
            let (word, context) = (Arc::clone(&word), Arc::clone(&context));
            async move {
                let task = Explain {
                    word: &word,
                    context: &context,
                    language,
                    hint,
                };
                attempt(backend.as_ref(), &task, &|info| say.partial(info), retry).await
            }
        };
        let id = TaskId::Explain;
        let job = self
            .0
            .dispatch()
            .ask_in(now.asker(id), now.route(id), tag, step);
        job.wait_until(partial, stop)
    }

    fn by(&self) -> String {
        self.now().by(TaskId::Explain)
    }

    fn language(&self) -> Option<&'static str> {
        let now = self.now();
        Some(now.ai.explain_language().unwrap_or("English"))
    }
}

#[cfg(test)]
mod late_tests {
    use super::*;
    use crate::backend::{BoxFuture, Partial};
    use crate::dispatch::Seat;
    use crate::prompt::Prompt;
    use mareader_settings::{AiSettings, Provider};
    use serde_json::{Value, json};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    struct Apple(Mutex<Vec<String>>);
    impl Backend for Apple {
        fn ask<'a>(
            &'a self,
            prompt: &'a Prompt,
            _: Partial<'a>,
        ) -> BoxFuture<'a, Result<Value, Failure>> {
            self.0.lock().unwrap().push(prompt.system.clone());
            Box::pin(async {
                Ok(json!({"pos":"noun", "meaning":"a fox", "synonyms":[], "usages":[]}))
            })
        }
    }

    #[test]
    fn an_ask_queued_before_probe_can_repin_without_changing_its_model() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let jobs = Dispatcher::new(rt, Arc::new(|| {}));
        let mut ai = AiSettings::default();
        ai.presets.clear();
        let apple = ai.add_preset(Provider::Apple, "");
        ai.tasks.choose(TaskId::Explain, Some(apple));
        ai.tasks.explain.language = Some("my".into());
        let backend = Arc::new(Apple(Mutex::new(Vec::new())));
        jobs.follow(&ai, |p| Seat {
            preset: p.clone(),
            backend: backend.clone(),
        });
        let held = jobs
            .slots
            .try_take(ai.preset(apple).unwrap(), TaskId::Explain)
            .unwrap();
        let original: Arc<dyn Explainer> = Arc::new(AiExplainer(jobs.clone(), None));
        let pinned = original.snapshot().unwrap();
        let on_thread = pinned.clone();
        let worker = std::thread::spawn(move || on_thread.explain("fox", "the fox", &|_| {}));
        let started = Instant::now();
        while jobs.slots.queued(apple) == 0 {
            assert!(started.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(5));
        }
        // The reader changes model while the original is still in line.
        let other = ai.add_preset(Provider::Ollama, "new");
        ai.tasks.choose(TaskId::Explain, Some(other));
        ai.tasks.explain.language = Some("fr".into());
        jobs.follow(&ai, |p| Seat {
            preset: p.clone(),
            backend: backend.clone(),
        });
        jobs.apple_languages(Some(vec!["en-US".into()]));
        drop(held);
        assert_eq!(
            worker.join().unwrap().unwrap_err().why,
            mareader_study::Why::Language
        );
        assert!(
            backend.0.lock().unwrap().is_empty(),
            "unsupported request was never sent"
        );
        let next = pinned.compatible_snapshot().unwrap();
        assert_eq!(next.language(), Some("English"));
        assert_eq!(next.by(), pinned.by());
        assert!(next.explain("fox", "the fox", &|_| {}).is_ok());
        let sent = backend.0.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert!(sent[0].contains("English"), "{}", sent[0]);
        assert!(!sent[0].contains("Burmese"), "{}", sent[0]);
        assert_eq!(original.language(), Some("French"));
    }
}
