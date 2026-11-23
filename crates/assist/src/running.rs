//! An answer on its way: one handle and one way to start every AI job.
//!
//! A job runs on the service's runtime and reports through a channel. Its
//! [`Running`] polls the news, waits for the end on a worker thread, or
//! cancels; dropping it cancels the job and frees its model.

use std::future::Future;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};

use mareader_study::{Failure, Why};
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;

/// Wakes the UI when a job has news.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// News from a job.
#[derive(Clone, Debug, PartialEq)]
pub enum Event<O, P = O> {
    /// Its model was full; this many run or wait before it.
    Queued { behind: usize },
    /// A model took it; its name.
    Started { by: String },
    /// What has come so far, shown while the rest streams in.
    Partial(P),
    /// Asked again after `why`; `attempt` counts from 1.
    Again { why: Why, attempt: u8 },
    /// The last event: the result, or why there's none.
    Done(Result<O, Failure>),
}

/// A job under way; dropping it cancels the job and frees its model.
pub struct Running<O, P = O> {
    rx: Receiver<Event<O, P>>,
    task: Option<JoinHandle<()>>,
    /// The result was handed out; nothing more comes.
    over: bool,
}

impl<O, P> Running<O, P> {
    /// Events since the last call, oldest first.
    pub fn poll(&mut self) -> Vec<Event<O, P>> {
        self.rx.try_iter().collect()
    }

    /// The result once it came, partials skipped; after that, none.
    pub fn answer(&mut self) -> Option<Result<O, Failure>> {
        while !self.over {
            match self.rx.try_recv() {
                Ok(Event::Done(done)) => {
                    self.over = true;
                    return Some(done);
                }
                Ok(_) => {}
                Err(TryRecvError::Empty) => return None,
                Err(TryRecvError::Disconnected) => {
                    self.over = true;
                    return Some(Err(Why::NotHere.into()));
                }
            }
        }
        None
    }

    /// Stops the job; no more events come.
    pub fn cancel(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }

    /// Blocks a worker thread until the end. Never the UI thread.
    pub fn wait(self) -> Result<O, Failure> {
        self.wait_with(|_| {})
    }

    /// Like [`Self::wait`], handing each partial to `partial` as it comes.
    pub fn wait_with(self, partial: impl Fn(P)) -> Result<O, Failure> {
        loop {
            match self.rx.recv() {
                Ok(Event::Partial(p)) => partial(p),
                Ok(Event::Done(done)) => return done,
                Ok(_) => {}
                Err(_) => return Err(Why::NotHere.into()),
            }
        }
    }
}

impl<O, P> Drop for Running<O, P> {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// A job's line to its handle: partials sent here wake the UI.
pub(crate) struct Say<O, P> {
    tx: Sender<Event<O, P>>,
    waker: Waker,
}

impl<O, P> Clone for Say<O, P> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            waker: Arc::clone(&self.waker),
        }
    }
}

impl<O, P> Say<O, P> {
    pub fn send(&self, news: Event<O, P>) {
        let _ = self.tx.send(news);
        (self.waker)();
    }

    pub fn partial(&self, news: P) {
        self.send(Event::Partial(news));
    }
}

/// Starts `job` on `rt`; its result ends the news. No runtime: not here.
pub(crate) fn start<O, P, F, Fut>(rt: Option<&Runtime>, waker: &Waker, job: F) -> Running<O, P>
where
    O: Send + 'static,
    P: Send + 'static,
    F: FnOnce(Say<O, P>) -> Fut,
    Fut: Future<Output = Result<O, Failure>> + Send + 'static,
{
    let (tx, rx) = channel();
    let Some(rt) = rt else {
        let _ = tx.send(Event::Done(Err(Why::NotHere.into())));
        return Running {
            rx,
            task: None,
            over: false,
        };
    };
    let work = job(Say {
        tx: tx.clone(),
        waker: Arc::clone(waker),
    });
    let waker = Arc::clone(waker);
    let task = rt.spawn(async move {
        let done = work.await;
        let _ = tx.send(Event::Done(done));
        waker();
    });
    Running {
        rx,
        task: Some(task),
        over: false,
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn runtime() -> Runtime {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build();
        rt.expect("a runtime")
    }

    fn quiet() -> Waker {
        Arc::new(|| {})
    }

    #[test]
    fn partials_then_the_result_reach_the_handle() {
        let rt = runtime();
        let job: Running<u8, &str> = start(Some(&rt), &quiet(), |say| async move {
            say.partial("half");
            Ok(2)
        });
        let seen = std::sync::Mutex::new(Vec::new());
        let done = job.wait_with(|p| seen.lock().expect("lock").push(p));
        assert_eq!(done, Ok(2));
        assert_eq!(*seen.lock().expect("lock"), ["half"]);
    }

    #[test]
    fn the_answer_is_handed_out_once() {
        let rt = runtime();
        let mut job: Running<u8> = start(Some(&rt), &quiet(), |_| async { Ok(7) });
        let started = std::time::Instant::now();
        let got = loop {
            if let Some(got) = job.answer() {
                break got;
            }
            assert!(started.elapsed().as_secs() < 10, "no answer");
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        assert_eq!(got, Ok(7));
        assert_eq!(job.answer(), None, "once only");
    }

    #[test]
    fn without_a_runtime_it_is_not_here() {
        let job: Running<u8> = start(None, &quiet(), |_| async { Ok(1) });
        assert_eq!(job.wait().map_err(|f| f.why), Err(Why::NotHere));
    }

    #[test]
    fn dropping_the_handle_cancels_the_job() {
        let rt = runtime();
        let (tx, rx) = channel::<()>();
        let job: Running<()> = start(Some(&rt), &quiet(), |_| async move {
            let _keep = tx;
            std::future::pending::<()>().await;
            Ok(())
        });
        drop(job);
        let gone = rx.recv_timeout(std::time::Duration::from_secs(10));
        let dropped = std::sync::mpsc::RecvTimeoutError::Disconnected;
        assert_eq!(gone, Err(dropped), "the job went with its handle");
    }
}
