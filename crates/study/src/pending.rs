//! An answer on its way, as the engine sees every AI job, and who answers.
//!
//! The engine holds a [`Pending`] and asks it for news each frame: what has
//! come so far, then the end. Dropping it cancels the job. The AI service
//! implements it; the engine never names the service.

use std::sync::Arc;

use crate::Explainer;
use crate::explain::Failure;
use crate::mode::{CardWriter, Judge};
use crate::skill::Skiller;

/// News from a job: the answer so far, then the end.
#[derive(Clone, Debug, PartialEq)]
pub enum News<O, P = O> {
    Partial(P),
    Done(Result<O, Failure>),
}

/// A job under way; dropping it cancels the job.
pub trait Pending<O, P = O>: Send {
    /// What has come since the last look; nothing after the end.
    fn news(&mut self) -> Vec<News<O, P>>;

    /// The result once it came, partials skipped; after that, none.
    fn take(&mut self) -> Option<Result<O, Failure>> {
        self.news().into_iter().find_map(|news| match news {
            News::Done(done) => Some(done),
            News::Partial(_) => None,
        })
    }
}

/// Everyone the engine asks, on the settings the service follows.
#[derive(Clone)]
pub struct Answerers {
    pub explainer: Arc<dyn Explainer>,
    pub cards: Arc<dyn CardWriter>,
    pub judge: Arc<dyn Judge>,
    pub skills: Arc<dyn Skiller>,
}
