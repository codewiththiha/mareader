//! Book, shelf and folder ids: a timestamp plus a counter, which is all the
//! guarantee the library needs — ids must be stable across sessions and unique
//! within one, never unpredictable or sortable across machines.
//!
//! The counter belongs to this crate, not the caller: two folder imports can
//! run concurrently, and a seq derived from a caller's snapshot of a list is
//! the same number minted twice in one millisecond.

use std::sync::atomic::{AtomicU32, Ordering};

/// Relaxed ordering: the counter only has to hand out distinct numbers.
static SEQ: AtomicU32 = AtomicU32::new(0);

fn next_seq() -> u32 {
    SEQ.fetch_add(1, Ordering::Relaxed)
}

pub fn next_id(now_ms: u64) -> String {
    new_id(now_ms, next_seq())
}

pub fn next_shelf_id(now_ms: u64) -> String {
    new_shelf_id(now_ms, next_seq())
}

pub fn next_folder_id(now_ms: u64) -> String {
    new_folder_id(now_ms, next_seq())
}

/// Import-run ids for the dock. The `t` prefix keeps them out of the
/// book/shelf/folder namespaces the ledger and shelves key by.
pub fn next_task_id(now_ms: u64) -> String {
    format!("t{now_ms:x}-{}", next_seq())
}

/// One tested "has enough time passed" rule shared by the app's cooldowns (a
/// rescan per focus, a picker's just-closed grace). Not a static: the caller
/// owns where the cooldown lives, this type only owns the rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cooldown {
    last_ms: Option<u64>,
    span_ms: u64,
}

impl Cooldown {
    pub fn new(span_ms: u64) -> Self {
        Self { last_ms: None, span_ms }
    }

    /// Whether `now` is past the span since the last arm. Arms itself when it
    /// is, so asking is the whole protocol; an unarmed cooldown holds nothing back.
    pub fn due(&mut self, now_ms: u64) -> bool {
        let due = match self.last_ms {
            None => true,
            Some(last) => now_ms.saturating_sub(last) >= self.span_ms,
        };
        if due {
            self.last_ms = Some(now_ms);
        }
        due
    }

    /// Start the span at `now` without asking — the picker's just-closed grace
    /// is armed by the close, not by a question.
    pub fn arm(&mut self, now_ms: u64) {
        self.last_ms = Some(now_ms);
    }

    /// Whether `now` sits inside the span since the last arm. Unlike [`Self::due`]
    /// this never extends the span, for callers that poll (every window focus
    /// polls the picker's grace).
    pub fn within(&self, now_ms: u64) -> bool {
        self.last_ms.is_some_and(|last| now_ms.saturating_sub(last) < self.span_ms)
    }
}

/// Whether a token is a shelf id. The letter prefix is what keeps the id kinds disjoint.
pub fn is_shelf(id: &str) -> bool {
    id.starts_with('s')
}

/// Explicit-seq mint, public only for the `v1` migration
/// ([`crate::blob::migrate::migrate_v1`]), which must be deterministic: running
/// it twice over the same blob has to produce the same ids.
pub fn new_id(now_ms: u64, seq: u32) -> String {
    format!("b{now_ms:011x}{seq:04x}")
}

/// Crate-private: an id must mint off this crate's counter, or a concurrent mint cannot be sure it differs.
fn new_shelf_id(now_ms: u64, seq: u32) -> String {
    format!("s{now_ms:011x}{seq:04x}")
}

fn new_folder_id(now_ms: u64, seq: u32) -> String {
    format!("f{now_ms:011x}{seq:04x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mints_of_one_tick_never_share_an_id() {
        let now = 1_700_000_000_000;
        // The explicit-seq half mints at a DIFFERENT tick on purpose: the
        // counter below hands out seq 0..3 — and an explicit seq 3 at the
        // same tick would be the very double-mint this test forbids.
        let other = now + 1;
        let mut all = vec![
            next_id(now),
            next_id(now),
            next_shelf_id(now),
            next_folder_id(now),
            new_id(other, 3),
            new_shelf_id(other, 3),
            new_folder_id(other, 3),
        ];
        all.sort();
        all.dedup();
        assert_eq!(all.len(), 7, "counter and prefix together keep all seven apart");
    }

    #[test]
    fn an_id_is_one_token_a_shelf_can_hold() {
        let id = new_id(1_700_000_000_000, 12);
        assert!(id.starts_with('b'));
        assert!(!id.contains(char::is_whitespace));
        assert_eq!(id.len(), 16);
        let (now, seq) = (1_700_000_000_000, 3);
        assert!(is_shelf(&new_shelf_id(now, seq)));
        assert!(!is_shelf(&new_id(now, seq)));
        assert!(!is_shelf(&new_folder_id(now, seq)));
        assert!(!is_shelf(""));
    }

    #[test]
    fn minting_in_order_is_stable_across_a_session() {
        let first: Vec<String> = (0..4096).map(|i| new_id(7, i)).collect();
        let mut sorted = first.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), first.len(), "4096 ids in one tick, all distinct");
    }

    #[test]
    fn a_task_id_is_never_a_book_shelf_or_folder_id() {
        let now = 1_700_000_000_000;
        let task = next_task_id(now);
        assert!(task.starts_with('t'), "the prefix is the namespace: {task}");
        assert!(!is_shelf(&task));
        // Two runs minted in one millisecond still get two cards.
        assert_ne!(next_task_id(now), next_task_id(now));
    }

    #[test]
    fn a_cooldown_arms_itself_by_asking() {
        let mut gate = Cooldown::new(5_000);
        assert!(gate.due(1_000), "a fresh cooldown does not hold the first ask back");
        assert!(!gate.due(4_000), "inside the span is held");
        assert!(gate.due(6_000), "past the span, and armed again");
        assert!(!gate.due(9_000));
        assert!(gate.due(11_500));
    }

    #[test]
    fn an_armed_cooldown_holds_the_very_next_ask() {
        let mut grace = Cooldown::new(1_000);
        grace.arm(100);
        assert!(!grace.due(500), "the arm, not a question, started this span");
        assert!(grace.due(1_200));
    }

    #[test]
    fn asking_whether_a_span_is_running_never_extends_it() {
        let mut grace = Cooldown::new(1_000);
        assert!(!grace.within(500), "nobody armed it");
        grace.arm(100);
        assert!(grace.within(500));
        assert!(grace.within(1_000), "polling does not push the span out");
        assert!(!grace.within(1_100), "past the span is past it, however often asked");
        // The polls above changed nothing: the arm still holds from 100.
        assert!(grace.within(1_099));
    }
}
