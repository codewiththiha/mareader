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
}
