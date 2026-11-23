//! One way to ask again: after an unreadable answer, or a server briefly away.
//!
//! ai-pool already asks a key again after a 5xx or 408, so such a failure
//! comes marked `retried` and isn't waited on twice. The requests each
//! failure can cost are listed in `docs/assist.md`.

use std::time::Duration;

use mareader_settings::Retry;
use mareader_study::{Failure, Why};

use crate::backend::{Backend, run};
use crate::task::Task;

/// Pauses a server is given to come back when no one is watching a spinner.
pub const WAITS: [Duration; 2] = [Duration::from_secs(3), Duration::from_secs(12)];

/// The reader is waiting: one re-ask, one short pause.
pub const WAITED: Retry = Retry {
    unreadable: 1,
    waits: &[Duration::from_secs(2)],
};

/// Work made ahead, or shown as it comes: one re-ask, then [`WAITS`].
pub const AHEAD: Retry = Retry {
    unreadable: 1,
    waits: &WAITS,
};

/// `task`'s answer, asked again as `retry` allows; else the last failure.
pub async fn attempt<T: Task>(
    backend: &dyn Backend,
    task: &T,
    partial: &(dyn Fn(T::Output) + Sync),
    retry: Retry,
) -> Result<T::Output, Failure> {
    let (mut unreadable, mut waits) = (0, retry.waits.iter());
    loop {
        match run(backend, task, partial).await {
            Ok(out) => return Ok(out),
            Err(f) if f.why == Why::BadAnswer && unreadable < retry.unreadable => unreadable += 1,
            Err(f) if passing(&f) => match waits.next() {
                Some(wait) => tokio::time::sleep(*wait).await,
                None => return Err(f),
            },
            Err(f) => return Err(f),
        }
    }
}

/// Busy, offline and slow servers often recover in seconds.
fn passing(f: &Failure) -> bool {
    matches!(f.why, Why::Busy | Why::Offline | Why::TimedOut) && !f.retried
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use std::sync::Mutex;

    use serde_json::{Value, json};

    use super::*;
    use crate::backend::{BoxFuture, Partial, block};
    use crate::prompt::Prompt;

    const QUICK: Retry = Retry {
        unreadable: 1,
        waits: &[Duration::ZERO; 2],
    };

    /// Gives its answers in turn, repeating the last; counts the asks.
    struct Turns(Mutex<Vec<Result<Value, Failure>>>, Mutex<usize>);

    impl Backend for Turns {
        fn ask<'a>(
            &'a self,
            _: &'a Prompt,
            _: Partial<'a>,
        ) -> BoxFuture<'a, Result<Value, Failure>> {
            *self.1.lock().expect("unpoisoned") += 1;
            let mut turns = self.0.lock().expect("unpoisoned");
            let next = match turns.len() {
                0 | 1 => turns.first().cloned(),
                _ => Some(turns.remove(0)),
            };
            Box::pin(async move { next.unwrap_or(Err(Why::NotHere.into())) })
        }
    }

    /// Reads `{"ok": …}`.
    struct Ok1;

    impl Task for Ok1 {
        type Output = Value;

        fn prompt(&self) -> Prompt {
            Prompt {
                name: "ok",
                system: String::new(),
                user: String::new(),
                schema: json!({}),
            }
        }

        fn read(&self, answer: &Value, _: bool) -> Option<Value> {
            answer.get("ok").cloned()
        }
    }

    /// What `turns` come to under `retry`, and how often it was asked.
    fn ask(turns: Vec<Result<Value, Failure>>, retry: Retry) -> (Result<Value, Why>, usize) {
        let backend = Turns(Mutex::new(turns), Mutex::new(0));
        let got = block(attempt(&backend, &Ok1, &|_| {}, retry)).map_err(|f| f.why);
        let asked = *backend.1.lock().expect("unpoisoned");
        (got, asked)
    }

    #[test]
    fn a_blip_or_a_bad_answer_is_asked_again() {
        let busy = Err(Why::Busy.into());
        let turns = vec![busy, Ok(json!({"no": 1})), Ok(json!({"ok": 1}))];
        let (got, asked) = ask(turns, QUICK);
        assert_eq!((got, asked), (Ok(json!(1)), 3));
    }

    #[test]
    fn a_server_that_stays_away_is_asked_once_per_wait() {
        let (got, asked) = ask(vec![Err(Why::Offline.into())], QUICK);
        assert_eq!((got, asked), (Err(Why::Offline), 3));
        let (got, asked) = ask(vec![Ok(json!(2))], QUICK);
        assert_eq!((got, asked), (Err(Why::BadAnswer), 2), "one re-ask");
    }

    #[test]
    fn what_the_pool_already_retried_is_not_waited_on() {
        let tried = Failure::from(Why::Busy).after_retries();
        let (got, asked) = ask(vec![Err(tried)], QUICK);
        assert_eq!((got, asked), (Err(Why::Busy), 1));
        let (got, asked) = ask(vec![Err(Why::Setup.into())], QUICK);
        assert_eq!((got, asked), (Err(Why::Setup), 1), "no use asking");
    }
}
