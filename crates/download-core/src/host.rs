//! The two things a downloader cannot own: an executor and an event sink.

use std::future::Future;

use crate::Progress;

/// Where a download runs, and where its progress goes.
pub trait Host: Clone + Send + Sync + 'static {
    /// Run a task off the caller's thread; a tokio timer must drive it.
    fn spawn(&self, task: impl Future<Output = ()> + Send + 'static);
    /// One snapshot for the feature's channel, sent outside every lock.
    fn publish(&self, progress: &Progress);
}
