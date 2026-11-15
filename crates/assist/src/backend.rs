//! The one seam between tasks and providers.

use std::future::Future;
use std::pin::Pin;

use mareader_study::{Failure, Why};
use serde_json::Value;

use crate::prompt::Prompt;
use crate::task::Task;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Box a provider's answer future at the backend seam.
pub fn boxed<'a>(
    answer: impl Future<Output = Result<Value, Failure>> + Send + 'a,
) -> BoxFuture<'a, Result<Value, Failure>> {
    Box::pin(answer)
}

/// Sees an answer as it fills in.
pub type Partial<'a> = &'a (dyn Fn(Value) + Sync);

/// Answers a [`Prompt`] with JSON; a new provider implements only this.
pub trait Backend: Send + Sync {
    fn ask<'a>(
        &'a self,
        prompt: &'a Prompt,
        partial: Partial<'a>,
    ) -> BoxFuture<'a, Result<Value, Failure>>;

    /// Requests it can spread over at once, as one per usable key.
    fn lanes(&self) -> BoxFuture<'_, u32> {
        Box::pin(async { 1 })
    }
}

/// Implement a backend whose `answer` method already returns its future.
macro_rules! forward_backend {
    ($type:ty) => {
        impl $crate::backend::Backend for $type {
            fn ask<'a>(
                &'a self,
                prompt: &'a $crate::prompt::Prompt,
                partial: $crate::backend::Partial<'a>,
            ) -> $crate::backend::BoxFuture<'a, Result<serde_json::Value, mareader_study::Failure>>
            {
                $crate::backend::boxed(self.answer(prompt, partial))
            }
        }
    };
}
pub(crate) use forward_backend;

/// Runs `task` on `backend`; `partial` sees readable answers so far.
pub async fn run<T: Task>(
    backend: &dyn Backend,
    task: &T,
    partial: &(dyn Fn(T::Output) + Sync),
) -> Result<T::Output, Failure> {
    let prompt = task.prompt();
    let seen = |v: Value| {
        if let Some(out) = task.read(&v, false) {
            partial(out);
        }
    };
    let answer = backend.ask(&prompt, &seen).await?;
    task.read(&answer, true)
        .ok_or_else(|| Failure::detailed(Why::BadAnswer, unread(&answer)))
}

/// What came instead of a readable answer, for the card's small print.
fn unread(answer: &Value) -> String {
    match answer {
        Value::Object(fields) if fields.is_empty() => "the answer was an empty object".into(),
        Value::Object(fields) => {
            let keys: Vec<&str> = fields.keys().map(String::as_str).collect();
            format!(
                "the answer held {}, not the fields asked for",
                keys.join(", ")
            )
        }
        _ => "the answer was not an object".into(),
    }
}

/// Answers nothing, always for one reason.
#[derive(Clone, Debug)]
pub struct Nobody(pub Failure);

impl Backend for Nobody {
    fn ask<'a>(&'a self, _: &'a Prompt, _: Partial<'a>) -> BoxFuture<'a, Result<Value, Failure>> {
        let failure = self.0.clone();
        Box::pin(async move { Err(failure) })
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
pub(crate) fn block<F: Future>(f: F) -> F::Output {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build();
    rt.expect("a runtime").block_on(f)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use std::sync::Mutex;

    use serde_json::json;

    use super::*;
    use crate::task::Explain;

    /// Streams two snapshots, then answers.
    struct Script;

    impl Backend for Script {
        fn ask<'a>(
            &'a self,
            _: &'a Prompt,
            partial: Partial<'a>,
        ) -> BoxFuture<'a, Result<Value, Failure>> {
            Box::pin(async move {
                partial(json!({}));
                partial(json!({"pos": "adj"}));
                Ok(json!({"pos": "adj", "meaning": "fast"}))
            })
        }
    }

    #[test]
    fn a_task_runs_on_any_backend() {
        let task = Explain {
            word: "quick",
            context: "the quick fox",
            language: None,
        };
        let seen = Mutex::new(Vec::new());
        let push = |i: mareader_study::WordInfo| {
            if let Ok(mut s) = seen.lock() {
                s.push(i.pos);
            }
        };
        let got = block(run(&Script, &task, &push)).map(|i| i.meaning);
        assert_eq!(got.as_deref(), Ok("fast"));
        assert_eq!(seen.lock().map(|s| s.clone()).unwrap_or_default(), ["adj"]);
        let none = Nobody(Why::NoKey.into());
        let failed = block(run(&none, &task, &|_| {})).map_err(|f| f.why);
        assert_eq!(failed, Err(Why::NoKey));
    }
}
