//! The corner stacks of the AI's work.
//!
//! Every frame the session lists each job going on; the [`Ledger`] keeps
//! how each ended. A job shows only while its own visual is out of view, as
//! [`seen`] tells, and [`place`] sets which corners show what. See
//! `docs/study-mode.md` section The corner stack.

pub mod ring;
pub mod seen;
pub mod stack;

use egui::Context;
use mareader_engine::activity::place::{self, Spot};
use mareader_engine::activity::{Activity, End, Ending, Key, Ledger, News};
use mareader_workspace::PaneId;

use stack::Line;

/// The window's half of the stack: how each job ended, across frames.
#[derive(Default)]
pub struct ActivityUi {
    ledger: Ledger,
}

impl ActivityUi {
    /// Takes this frame's `jobs`; says what a toast should tell.
    pub fn update(
        &mut self,
        ctx: &Context,
        jobs: Vec<Activity>,
        ending: &dyn Fn(Key) -> Ending,
    ) -> Vec<News> {
        let now = ctx.input(|i| i.time);
        let shown = |key| seen::shown(ctx, key);
        let news = self.ledger.update(jobs, (ending, &shown), now);
        let seen: Vec<Key> = self
            .ledger
            .rows(now)
            .into_iter()
            .filter(|row| row.end == End::Done && seen::shown(ctx, row.activity.key))
            .map(|row| row.activity.key)
            .collect();
        for key in seen {
            self.ledger.seen(key);
        }
        news
    }

    /// The stacks and their lines, oldest first, over `panes`: the jobs
    /// whose own visual is out of view.
    pub fn stacks(&self, ctx: &Context, panes: &[PaneId]) -> Vec<(Spot, Vec<Line>)> {
        let now = ctx.input(|i| i.time);
        let rows = self.ledger.rows(now);
        let hidden = rows
            .iter()
            .filter(|row| !seen::shown(ctx, row.activity.key));
        let items: Vec<_> = hidden
            .map(|r| (r.activity.key, r.activity.key.scope()))
            .collect();
        let line = |key: Key| {
            let row = rows.iter().find(|row| row.activity.key == key);
            row.map(|row| Line::of(row, now))
        };
        let lines = |keys: Vec<Key>| keys.into_iter().filter_map(line).collect();
        let stacks = place::place(panes, &items).into_iter();
        stacks.map(|(spot, keys)| (spot, lines(keys))).collect()
    }

    /// Says the jobs `which` picks show their own work on this page.
    pub fn in_view_where(&self, ctx: &Context, now: f64, which: &dyn Fn(Key) -> bool) {
        let rows = self.ledger.rows(now);
        let keys = rows
            .iter()
            .map(|row| row.activity.key)
            .filter(|&key| which(key));
        keys.for_each(|key| seen::in_view(ctx, key));
    }

    /// Whether any line goes by itself soon, so frames keep coming.
    pub fn lingering(&self, now: f64) -> bool {
        self.ledger.rows(now).iter().any(|row| row.lingers())
    }

    /// The page View goes to for `key`, and drops its waiting line.
    pub fn view(&mut self, key: Key, now: f64) -> Option<usize> {
        let page = self
            .ledger
            .rows(now)
            .iter()
            .find(|r| r.activity.key == key)
            .map(|r| r.activity.page);
        self.ledger.seen(key);
        page
    }

    /// A job that ended between frames, out of view: its line, as `end`.
    pub fn land(&mut self, activity: Activity, end: End, now: f64) {
        self.ledger.land(activity, end, now);
    }

    pub fn dismiss(&mut self, key: Key, now: f64) {
        self.ledger.dismiss(key, now);
    }

    /// Holds `pane`'s lingering lines while its stack is hovered.
    pub fn hold(&mut self, keys: &[Key], now: f64) {
        for &key in keys {
            self.ledger.hold(key, now);
        }
    }
}
