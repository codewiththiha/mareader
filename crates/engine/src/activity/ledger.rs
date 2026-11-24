//! How each job ended, kept once its owner forgets it.
//!
//! Fed each frame the jobs going on, it tells a job that landed from one
//! that was stopped, and says which to toast. A failure lingers a moment;
//! a landed job that waits to be seen stays until it is, or is dismissed.

use std::collections::BTreeMap;

use super::{Activity, Ending, Key, Stage};

/// Seconds a failure, or a quick *ready*, stays in the stack.
pub const LINGER: f64 = 3.0;
/// The most landed jobs kept waiting to be seen; the oldest goes first.
const KEPT: usize = 3;

/// How a job in the stack stands: going on, or ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    Going,
    Done,
    Failed,
}

/// One line of the stack, and since when it stood so.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub activity: Activity,
    pub end: End,
    pub since: f64,
}

impl Row {
    /// Whether it goes by itself once it has lingered.
    pub fn lingers(&self) -> bool {
        match self.end {
            End::Going => false,
            End::Failed => true,
            End::Done => !self.activity.keeps,
        }
    }

    /// Seconds before it goes by itself, if it does.
    pub fn left(&self, now: f64) -> Option<f64> {
        self.lingers().then_some(LINGER - (now - self.since))
    }
}

/// What a frame brought that a toast says.
#[derive(Clone, Debug, PartialEq)]
pub enum News {
    Failed(Activity),
    Stopped(Activity),
}

/// The jobs seen last frame, and the ended ones still shown.
#[derive(Default)]
pub struct Ledger {
    live: BTreeMap<Key, Row>,
    ended: Vec<Row>,
}

impl Ledger {
    /// Takes this frame's jobs at `now`; `ending` says how a job gone ended,
    /// `seen` whether its result is in view.
    pub fn update(
        &mut self,
        jobs: Vec<Activity>,
        (ending, seen): (&dyn Fn(Key) -> Ending, &dyn Fn(Key) -> bool),
        now: f64,
    ) -> Vec<News> {
        let mut news = Vec::new();
        let mut live = BTreeMap::new();
        for activity in jobs {
            let was = self.live.remove(&activity.key);
            let failed = activity.stage == Stage::Failed;
            let end = if failed { End::Failed } else { End::Going };
            let since = match &was {
                Some(row) if row.end == end => row.since,
                _ => now,
            };
            let new = was.as_ref().is_none_or(|row| row.end != End::Failed);
            if failed && new && !activity.told {
                news.push(News::Failed(activity.clone()));
            }
            self.ended.retain(|row| row.activity.key != activity.key);
            live.insert(
                activity.key,
                Row {
                    activity,
                    end,
                    since,
                },
            );
        }
        for (key, row) in std::mem::replace(&mut self.live, live) {
            if row.end != End::Failed {
                self.gone(row, (ending(key), seen(key)), (now, &mut news));
            }
        }
        self.trim(now);
        news
    }

    /// A job gone from the list: a line for how it ended, and its news.
    fn gone(
        &mut self,
        mut row: Row,
        (ending, seen): (Ending, bool),
        (now, news): (f64, &mut Vec<News>),
    ) {
        let end = match ending {
            Ending::Quiet => return,
            Ending::Stopped => {
                news.push(News::Stopped(row.activity));
                return;
            }
            Ending::Landed if row.activity.keeps && seen => return,
            Ending::Landed => End::Done,
            Ending::Failed => {
                row.activity.said = row.activity.failed_said();
                if !row.activity.told {
                    news.push(News::Failed(row.activity.clone()));
                }
                End::Failed
            }
        };
        self.ended.push(Row {
            end,
            since: now,
            ..row
        });
    }

    /// The stack's lines at `now`, oldest first: what goes on, then what
    /// ended; a failure its owner keeps shows only while it lingers.
    pub fn rows(&self, now: f64) -> Vec<&Row> {
        let up = |row: &&Row| row.left(now).is_none_or(|left| left > 0.0);
        self.live.values().chain(&self.ended).filter(up).collect()
    }

    /// The landed job's result was seen; its line goes.
    pub fn seen(&mut self, key: Key) {
        self.ended.retain(|row| row.activity.key != key);
    }

    /// Dismisses an ended line; a failure still owned stays out of sight.
    pub fn dismiss(&mut self, key: Key, now: f64) {
        self.seen(key);
        if let Some(row) = self.live.get_mut(&key).filter(|r| r.end == End::Failed) {
            row.since = now - LINGER;
        }
    }

    /// Holds a lingering line where it stands, as while hovered.
    pub fn hold(&mut self, key: Key, now: f64) {
        let rows = self.live.values_mut().chain(self.ended.iter_mut());
        for row in rows.filter(|r| r.activity.key == key && r.lingers()) {
            row.since = row.since.max(now - LINGER + 1.0);
        }
    }

    /// Lets lingering lines go once their time is up; keeps a few waiting.
    fn trim(&mut self, now: f64) {
        self.ended
            .retain(|row| row.left(now).is_none_or(|left| left > 0.0));
        let waiting = self.ended.iter().filter(|r| !r.lingers()).count();
        let mut over = waiting.saturating_sub(KEPT);
        self.ended.retain(|row| {
            let drop = over > 0 && !row.lingers();
            over -= usize::from(drop);
            !drop
        });
    }
}

#[cfg(test)]
mod tests {
    use mareader_settings::TaskId;
    use mareader_workspace::PaneId;

    use super::*;

    fn job(from: usize, stage: Stage, keeps: bool) -> Activity {
        Activity {
            key: Key::Cards {
                pane: PaneId(1),
                from,
            },
            task: TaskId::StudyCards,
            said: String::new(),
            range: format!("page {from}"),
            page: from,
            keeps,
            told: false,
            stage,
        }
    }

    const YES: &dyn Fn(Key) -> Ending = &|_| Ending::Landed;
    const STOPPED: &dyn Fn(Key) -> Ending = &|_| Ending::Stopped;
    const SEEN: &dyn Fn(Key) -> bool = &|_| true;
    const NO: &dyn Fn(Key) -> bool = &|_| false;

    fn ends(ledger: &Ledger, now: f64) -> Vec<End> {
        ledger.rows(now).iter().map(|row| row.end).collect()
    }

    #[test]
    fn a_landed_job_waits_to_be_seen_then_goes() {
        let mut ledger = Ledger::default();
        let going = job(4, Stage::Running(Some(0.5)), true);
        assert!(ledger.update(vec![going], (YES, SEEN), 0.0).is_empty());
        assert_eq!(ends(&ledger, 0.0), [End::Going]);
        assert!(ledger.update(Vec::new(), (YES, NO), 1.0).is_empty());
        assert_eq!(ends(&ledger, 60.0), [End::Done], "stays until seen");
        ledger.seen(job(4, Stage::Queued, true).key);
        assert!(ledger.rows(60.0).is_empty());
    }

    #[test]
    fn a_job_landing_in_view_is_seen_at_once() {
        let mut ledger = Ledger::default();
        ledger.update(vec![job(4, Stage::Queued, true)], (YES, SEEN), 0.0);
        ledger.update(Vec::new(), (YES, SEEN), 1.0);
        assert!(ledger.rows(1.0).is_empty());
    }

    #[test]
    fn a_quick_ready_and_a_failure_linger_then_go() {
        let mut ledger = Ledger::default();
        let jobs = vec![job(4, Stage::Queued, false), job(7, Stage::Queued, false)];
        ledger.update(jobs, (YES, NO), 0.0);
        let failed = vec![job(7, Stage::Failed, false)];
        let news = ledger.update(failed.clone(), (YES, NO), 1.0);
        assert_eq!(news, [News::Failed(job(7, Stage::Failed, false))]);
        assert_eq!(ends(&ledger, 1.0), [End::Failed, End::Done]);
        assert!(
            ledger.update(failed.clone(), (YES, NO), 2.0).is_empty(),
            "said once"
        );
        ledger.update(failed, (YES, NO), 4.5);
        assert!(ledger.rows(4.5).is_empty(), "both went after a moment");
    }

    #[test]
    fn a_hover_holds_a_lingering_line() {
        let mut ledger = Ledger::default();
        let failed = vec![job(7, Stage::Failed, false)];
        ledger.update(failed.clone(), (YES, NO), 0.0);
        ledger.hold(failed[0].key, 2.8);
        ledger.update(failed, (YES, NO), 3.5);
        assert_eq!(ends(&ledger, 3.5), [End::Failed]);
    }

    #[test]
    fn a_job_gone_unlanded_was_stopped() {
        let mut ledger = Ledger::default();
        ledger.update(vec![job(4, Stage::Queued, true)], (YES, NO), 0.0);
        let news = ledger.update(Vec::new(), (STOPPED, NO), 1.0);
        assert_eq!(news, [News::Stopped(job(4, Stage::Queued, true))]);
        assert!(ledger.rows(1.0).is_empty());
    }

    #[test]
    fn only_a_few_landed_lines_wait() {
        let mut ledger = Ledger::default();
        let jobs = (0..5).map(|n| job(n, Stage::Queued, true)).collect();
        ledger.update(jobs, (YES, NO), 0.0);
        ledger.update(Vec::new(), (YES, NO), 1.0);
        let pages: Vec<_> = ledger.rows(1.0).iter().map(|r| r.activity.page).collect();
        assert_eq!(pages, [2, 3, 4], "the oldest went first");
    }

    #[test]
    fn an_owner_that_tells_its_failure_gets_no_second_toast() {
        let mut ledger = Ledger::default();
        let told = Activity {
            told: true,
            ..job(4, Stage::Running(None), true)
        };
        ledger.update(vec![told], (YES, NO), 0.0);
        let failed: &dyn Fn(Key) -> Ending = &|_| Ending::Failed;
        assert!(ledger.update(Vec::new(), (failed, NO), 1.0).is_empty());
        assert_eq!(ends(&ledger, 1.0), [End::Failed], "its line still says so");
        assert!(ledger.rows(4.5).is_empty());
    }
}
