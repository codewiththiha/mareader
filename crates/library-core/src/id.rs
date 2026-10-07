//! Book, shelf and folder ids: a timestamp plus a crate-owned counter.

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

/// Import-run ids for the dock; the `t` prefix separates them.
pub fn next_task_id(now_ms: u64) -> String {
    format!("t{now_ms:x}-{}", next_seq())
}

/// One tested "has enough time passed" rule for the app's cooldowns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cooldown {
    last_ms: Option<u64>,
    span_ms: u64,
}

impl Cooldown {
    pub fn new(span_ms: u64) -> Self {
        Self {
            last_ms: None,
            span_ms,
        }
    }

    /// Whether the span since the last arm has passed; arms itself.
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

    /// Start the span at `now`.
    pub fn arm(&mut self, now_ms: u64) {
        self.last_ms = Some(now_ms);
    }

    /// Whether `now` sits inside the span; never extends it.
    pub fn within(&self, now_ms: u64) -> bool {
        self.last_ms
            .is_some_and(|last| now_ms.saturating_sub(last) < self.span_ms)
    }
}

/// Whether a token is a shelf id.
pub fn is_shelf(id: &str) -> bool {
    id.starts_with('s')
}

/// Explicit-seq mint for the `v1` migration, which must be
/// deterministic.
pub(crate) fn new_id(now_ms: u64, seq: u32) -> String {
    format!("b{now_ms:011x}{seq:04x}")
}

/// Crate-private: ids mint off this crate's counter.
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
        // The explicit-seq half mints at a different tick on purpose.
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
        assert_eq!(
            all.len(),
            7,
            "counter and prefix together keep all seven apart"
        );
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
        assert_eq!(
            sorted.len(),
            first.len(),
            "4096 ids in one tick, all distinct"
        );
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
        assert!(
            gate.due(1_000),
            "a fresh cooldown does not hold the first ask back"
        );
        assert!(!gate.due(4_000), "inside the span is held");
        assert!(gate.due(6_000), "past the span, and armed again");
        assert!(!gate.due(9_000));
        assert!(gate.due(11_500));
    }

    #[test]
    fn an_armed_cooldown_holds_the_very_next_ask() {
        let mut grace = Cooldown::new(1_000);
        grace.arm(100);
        assert!(
            !grace.due(500),
            "the arm, not a question, started this span"
        );
        assert!(grace.due(1_200));
    }

    #[test]
    fn asking_whether_a_span_is_running_never_extends_it() {
        let mut grace = Cooldown::new(1_000);
        assert!(!grace.within(500), "nobody armed it");
        grace.arm(100);
        assert!(grace.within(500));
        assert!(grace.within(1_000), "polling does not push the span out");
        assert!(
            !grace.within(1_100),
            "past the span is past it, however often asked"
        );
        // The polls above changed nothing: the arm still holds from 100.
        assert!(grace.within(1_099));
    }
}
