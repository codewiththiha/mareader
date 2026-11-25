//! Explain's answerer for the study tools.

use std::sync::Arc;

use mareader_settings::TaskId;
use mareader_study::{Explainer, Failure, Pos, WordInfo};

use crate::backend::Backend;
use crate::dispatch::Dispatcher;
use crate::retry::attempt;
use crate::running::Say;
use crate::task::Explain;

/// The study tools' answerer: blocks its worker thread, not the UI.
pub(super) struct AiExplainer(pub(super) Dispatcher);

impl Explainer for AiExplainer {
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
        (word, context, hint): (&str, &str, Option<Pos>),
        partial: &(dyn Fn(WordInfo) + Sync),
    ) -> Result<(WordInfo, String), Failure> {
        let now = self.0.followed();
        let language = now.ai.explain_language();
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
        let job = self.0.dispatch().ask(now.asker(id), now.route(id), step);
        job.wait_by(partial)
    }

    fn by(&self) -> String {
        self.0.followed().by(TaskId::Explain)
    }
}
