//! Book and shelf ids.
//!
//! An id has to be stable across sessions and unique across a library, but it
//! never has to be unpredictable or sortable across machines, so a timestamp
//! plus a counter is all the guarantee the library needs.
//!
//! The counter is the crate's own rather than the caller's, and that is a
//! correctness rule: two folder imports run concurrently, and a seq derived
//! from a list a task snapshotted before its own walk is the same number
//! minted twice in one millisecond.

use std::sync::atomic::{AtomicU32, Ordering};

/// Relaxed ordering: the webview is single-threaded, so this only has to hand out distinct numbers.
static SEQ: AtomicU32 = AtomicU32::new(0);

fn next_seq() -> u32 {
    SEQ.fetch_add(1, Ordering::Relaxed)
}

pub fn next_id(now_ms: u64) -> String {
    new_id(now_ms, next_seq())
}

/// One counter across the three kinds is one less thing two mints can disagree about.
pub fn next_shelf_id(now_ms: u64) -> String {
    new_shelf_id(now_ms, next_seq())
}

pub fn next_folder_id(now_ms: u64) -> String {
    new_folder_id(now_ms, next_seq())
}

/// The dock's run ids: the `t` prefix keeps them out of the book/shelf/folder namespaces the
/// ledger and the shelves key by, and the same counter guarantees two runs minted in one
/// millisecond never share a card.
pub fn next_task_id(now_ms: u64) -> String {
    format!("t{now_ms:x}-{}", next_seq())
}

/// A monotonic per-run counter for "is this the same reveal as the last one": two reveals of
/// one thing in a row must differ, or the second reads as a repeat of the first and notifies
/// nobody.
pub fn next_nonce(now_ms: u64) -> u64 {
    // The millisecond is folded in for the same reason the seq exists: two nonces minted in
    // one tick by different callers still have to differ, and the shared counter's order
    // keeps the fold monotonic for the life of the session.
    now_ms.wrapping_mul(1_000).wrapping_add(u64::from(next_seq()))
}

/// One "has enough time passed" answer, so the cooldowns scattered through the app (a rescan
/// per focus, a picker's just-closed grace) are one tested thing rather than a stamp and a
/// subtraction at each site.
///
/// Not a static: the caller owns where the cooldown lives, this type only owns the rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cooldown {
    last_ms: Option<u64>,
    span_ms: u64,
}

impl Cooldown {
    pub fn new(span_ms: u64) -> Self {
        Self { last_ms: None, span_ms }
    }

    /// Whether `now` is past the span since the last arm — and arms itself when it is, so
    /// asking is the whole of the protocol. A cooldown nobody armed holds nothing back.
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

    /// Mark the span as running from `now` without asking anything: the picker's "just
    /// closed" grace is armed by the close, not by a question.
    pub fn arm(&mut self, now_ms: u64) {
        self.last_ms = Some(now_ms);
    }

    /// Whether `now` sits inside the span since the last arm — a question that changes
    /// nothing, for the callers that ask repeatedly (the picker's grace is polled by every
    /// focus the window gets; polling must not extend it).
    pub fn within(&self, now_ms: u64) -> bool {
        self.last_ms.is_some_and(|last| now_ms.saturating_sub(last) < self.span_ms)
    }
}

/// Whether a token is a SHELF's id: the letter prefix is what makes the kinds disjoint.
pub fn is_shelf(id: &str) -> bool {
    id.starts_with('s')
}

/// A fresh id: the millisecond it was minted at, plus a per-millisecond counter.
///
/// The explicit-seq form is public for the one mint that is deliberately
/// deterministic, the `v1` migration ([`crate::blob::migrate::migrate_v1`]),
/// which must produce the same ids if it ever runs twice over the same blob.
pub fn new_id(now_ms: u64, seq: u32) -> String {
    format!("b{now_ms:011x}{seq:04x}")
}

/// Crate-private, because an id mints off this crate's counter or the next concurrent mint cannot be sure it differs.
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
    fn nonces_minted_together_never_agree() {
        assert_ne!(next_nonce(7), next_nonce(7));
        assert_ne!(next_nonce(7), next_nonce(8));
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
