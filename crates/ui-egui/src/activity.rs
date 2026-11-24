//! The corner stack of the AI's work, for every pane.
//!
//! Every frame the session lists each job going on; the [`Ledger`] keeps
//! how each ended. A job shows in its pane's corner only while its own
//! visual is out of view, as [`seen`] tells. See
//! `docs/study-mode.md` § The corner stack.

pub mod ring;
pub mod seen;
pub mod stack;

use egui::Context;
use mareader_engine::activity::{Activity, End, Key, Ledger, News, Scope};
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
        landed: &dyn Fn(Key) -> bool,
    ) -> Vec<News> {
        let now = ctx.input(|i| i.time);
        let shown = |key| seen::shown(ctx, key);
        let news = self.ledger.update(jobs, (landed, &shown), now);
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

    /// `pane`'s lines, oldest first: its jobs whose own visual is out of view.
    pub fn lines(&self, ctx: &Context, pane: PaneId) -> Vec<Line> {
        let now = ctx.input(|i| i.time);
        let rows = self.ledger.rows(now).into_iter();
        let here = rows.filter(|row| row.activity.key.scope() == Scope::Pane(pane));
        let hidden = here.filter(|row| !seen::shown(ctx, row.activity.key));
        hidden.map(|row| Line::of(row, now)).collect()
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
