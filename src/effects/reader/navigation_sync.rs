//! Navigation sync: keeps `viewer.page` and the continuous/horizontal scroll
//! position in sync around the virtualizers. Wired once from ReaderPage.
//!
//! - scroll -> page: the virtualizer's dominant-page signal
//! - page -> scroll: `scroll_to_index(Start, Auto)` — `Auto` resolves to a glide,
//!   and the reader's scroll switch is what decides whether it may (see
//!   `scroll_mode`)
//! - mode flip -> scroll: `scroll_to_index(Start, Instant)` when re-entering
//!   continuous mode
//!
//! Both directions stand down while a zoom transaction is in flight: a zoom
//! moves the geometry, and the transaction's anchor — not a window churn's
//! idea of the dominant item — decides where the reader lands. Sync resumes
//! against the committed geometry when the transition ends.
//!
//! A page write that arrives WHILE a transaction holds the geometry (the
//! resume jump on open, an outline click during a fit slide, a search hit
//! mid-gesture) is not dropped, though — it is held by [`JumpGate`] and
//! replayed on the frame the transaction closes. Without that replay, every
//! open from the library lost its resume point: the reader mounts, its
//! container's first measure opens a fit transaction, the resume jump lands
//! one frame later inside it, and the dominant arm "corrected" the page back
//! to 1 — which reading_progress then dutifully saved over the real position.

use std::cell::Cell;
use std::rc::Rc;

use leptos::prelude::*;

use pdf_core::layout::ViewMode;
use virtual_list_leptos::{Align, ScrollMode, Virtualizer};

use crate::state::ReaderState;

use super::on_mode_change::on_mode_change;

/// Shared suppression flag for the two one-way syncs.
pub(super) struct NavSyncState {
    pub suppress: Rc<Cell<bool>>,
    /// A mode restore is in flight: the scroll→page sync must stand down so it
    /// does not read the not-yet-restored strip and clobber the preserved page.
    ///
    /// A REACTIVE signal on purpose, not a `Cell`. The dominant arm reads it
    /// as a dependency and returns early while it is up; when the restore
    /// lands and it falls, that write re-runs the arm against the now-true
    /// dominant. A `Cell` hand-off left the arm with no reason to re-run after
    /// a mode flip, so the page counter sat on the pre-restore value until the
    /// reader scrolled. Making it reactive closes that without re-introducing
    /// the read-a-strip-mid-restore race the flag exists to prevent.
    pub defer: RwSignal<bool>,
}

impl NavSyncState {
    fn new() -> Self {
        Self {
            suppress: Rc::new(Cell::new(false)),
            defer: RwSignal::new(false),
        }
    }
}

/// The page a strip's dominant item (0-based index) corresponds to, SAFELY.
///
/// The naive `dominant as u32 + 1` is a real footgun: a strip that has not
/// yet resolved a window (a freshly-mounted mode flip, a mid-fit measure)
/// can report a sentinel or out-of-range index, and if that index is
/// `usize::MAX` the `as u32 + 1` WRAPS to 0 — so a view-mode change would
/// reset the reader to page 0, which reading-progress then persisted over
/// the real position. Clamping to `[1, page_count]` makes a momentary
/// no-window read harmless instead of destructive.
fn page_from_dominant(dominant: usize, num_pages: u32) -> u32 {
    let raw = dominant.saturating_add(1) as u64;
    raw.clamp(1, u64::from(num_pages.max(1))) as u32
}

/// How a page-to-scroll jump should travel: gliding while the reader's scroll
/// switch allows it, in one step when it does not. Read UNTRACKED, because the
/// flag that stops a jump gliding must not be what re-runs the jump.
fn scroll_mode(state: ReaderState) -> ScrollMode {
    if state.viewer.motion.get_untracked().scroll_glide {
        ScrollMode::Auto
    } else {
        ScrollMode::Instant
    }
}

/// Decides whether a run of the page→scroll effect may command the strip, in
/// a form pure enough to unit-test on the host.
///
/// The two inputs it distinguishes are "the page signal changed" and "the
/// zoom transaction flag changed" — the effect re-runs for both, and only the
/// first is a navigation intent. A write that lands while a transaction holds
/// the geometry is HELD, with its value; the first run after the transaction
/// closes replays it. A transaction that closes with no held write moves
/// nothing, which is what keeps a zoom commit from scrolling the top of the
/// current page back under the reader's eyes.
///
/// Holding the VALUE (not just a flag) is what lets the replay survive the
/// dominant arm: both effects re-run in the same flush when a transaction
/// closes, and if the dominant arm runs first it reads the not-yet-jumped
/// strip and "corrects" the page back to the stale dominant item — a replay
/// that only remembered "something was held" would then scroll to the
/// clobbered page. So the dominant arm DEFERS to [`JumpGate::pending`] for
/// exactly that flush, and a replay whose page signal was clobbered anyway
/// RE-ASSERTS the held page alongside the scroll.
#[derive(Debug, Default)]
pub(crate) struct JumpGate {
    /// The page the last `admit` saw, so a real write can be told apart from
    /// a re-run caused by the transaction flag flipping.
    last_page: Cell<u32>,
    /// A page write that arrived while a transaction was open, with its
    /// value, awaiting its replay.
    held: Cell<Option<u32>>,
}

impl JumpGate {
    fn new() -> Self {
        Self::default()
    }

    /// The page the strip should be scrolled to on this run, if any — as
    /// `(page, reassert)`, where `reassert` says the page SIGNAL no longer
    /// names that page (it was clobbered after the hold) and must be written
    /// back before the scroll, or the next scroll event would correct the
    /// strip right back off the jumped-to page.
    fn admit(&self, page: u32, zooming: bool) -> Option<(u32, bool)> {
        let changed = page != self.last_page.get();
        self.last_page.set(page);
        if zooming {
            if changed {
                self.held.set(Some(page));
            }
            return None;
        }
        if let Some(held) = self.held.take() {
            return Some((held, held != page));
        }
        changed.then_some((page, false))
    }

    /// A held write waiting for its replay — the dominant arm defers to it
    /// for the flush that closes the transaction, instead of correcting the
    /// page from a strip the replay has not moved yet.
    fn pending(&self) -> Option<u32> {
        self.held.get()
    }
}

/// Must be called once from the app root (ReaderPage), alongside the zoom sources.
pub fn navigation_sync(
    state: ReaderState,
    virtualizer: Virtualizer,
    h_virtualizer: Virtualizer,
) {
    let nav = NavSyncState::new();
    let mode = state.viewer.mode;
    // The tracked form of "a zoom transaction is in flight", for the effects
    // that must RESYNC when one lands (below); the untracked
    // `zooming_now()` stays on the ones that must only not fight it.
    let zooming = state.viewer.zooming();
    // One gate per axis: its page→scroll arm holds navigation writes that
    // arrive mid-transaction, and its dominant arm defers to the held write
    // on the flush that closes the transaction (see JumpGate).
    let gate = Rc::new(JumpGate::new());
    let h_gate = Rc::new(JumpGate::new());

    // The mode-restore flag is shared with on_mode_change, which raises it
    // during a mode flip and lowers it once the restored scroll has landed.
    on_mode_change(state, virtualizer.clone(), h_virtualizer.clone(), nav.defer.clone());

    {
        let suppress = nav.suppress.clone();
        let defer = nav.defer.clone();
        let page = state.viewer.page;
        let v = virtualizer.clone();
        let gate = gate.clone();
        Effect::new(move |_| {
            if mode.get() != ViewMode::ScrollVertical {
                return;
            }
            // While on_mode_change is re-anchoring the strip after a mode flip,
            // the strip is still sitting at its initial (unrestored) offset and
            // its dominant would misreport the page — reading it now resets the
            // reader back to (a transient) page 0/1. Stand down; the restored
            // scroll re-runs this effect with the true dominant.
            if defer.get() {
                return;
            }
            let dominant = page_from_dominant(v.dominant().get(), state.document.num_pages.get());
            // During a zoom transaction the virtualizer's window is frozen,
            // but a mid-zoom wheel can still rewindow and move the dominant
            // item through no fault of the reader. The zoom anchor already
            // knows the page; syncing it here is what made the page number
            // flicker to a neighbour mid-gesture.
            //
            // TRACKED, because a container follow holds a transaction open for
            // the whole burst of a sidebar slide or a window drag: shrinking the
            // page fits more of the book on screen, so the dominant item
            // legitimately moves while the flag is up, and an untracked guard
            // would drop that page and leave the counter on the old one until
            // the reader scrolled again. Landing on the commit is the same one
            // write, at the moment the scale is final.
            if zooming.get() {
                return;
            }
            // A held navigation replays in this same flush and the strip has
            // not moved yet — correcting the page from the stale dominant now
            // is exactly how the resume jump used to die. Let the replay land
            // first; the re-run it causes reads the TRUE dominant.
            if gate.pending().is_some() {
                return;
            }
            if page.get_untracked() == dominant {
                return;
            }
            suppress.set(true);
            page.set(dominant);
        });
    }

    {
        let suppress = nav.suppress.clone();
        let page = state.viewer.page;
        let v = virtualizer.clone();
        let zooming = zooming.clone();
        let gate = gate.clone();
        Effect::new(move |_| {
            if mode.get() != ViewMode::ScrollVertical {
                return;
            }
            // Scroll restoration is the transaction's job while one is open;
            // letting a page write fight the anchor mid-zoom is the other half
            // of the loop. The gate holds the write instead of losing it, and
            // the tracked `zooming` read is what brings this effect back on
            // the frame the transaction closes, to replay it.
            let page_now = page.get();
            let zooming = zooming.get();
            let Some((target, reassert)) = gate.admit(page_now, zooming) else {
                // A stand-down consumed the run. The echo flag's scroll event
                // is never coming (both arms and the DOM echo stand down for
                // the transaction's duration), so it must not survive to eat
                // the replay either.
                if zooming {
                    suppress.set(false);
                }
                return;
            };
            // A replay against a clobbered page signal re-asserts the page:
            // the counter must lead the strip, or the next scroll event would
            // "correct" the strip right back off the jumped-to page.
            if reassert {
                suppress.set(false);
                page.set(target);
            }
            if suppress.get() {
                suppress.set(false);
                return;
            }
            if target == 0 {
                return;
            }
            // The glide is the animation; the jump is not. With the reader's
            // scroll switch off the column lands on the page in one write
            // (`Auto` is what resolves to a smooth scroll when the distance is
            // short, so this is the only decision to make here).
            v.scroll_to_index(
                (target - 1) as usize,
                Align::Start,
                scroll_mode(state),
            );
        });
    }

    {
        // dominant page follows the horizontal strip
        let (suppress, page, v) = (nav.suppress.clone(), state.viewer.page, h_virtualizer.clone());
        let defer = nav.defer.clone();
        let h_gate = h_gate.clone();
        Effect::new(move |_| {
            if mode.get() != ViewMode::ScrollHorizontal {
                return;
            }
            // Same restore guard as the vertical arm (see above).
            if defer.get() {
                return;
            }
            let dominant = page_from_dominant(v.dominant().get(), state.document.num_pages.get());
            // Tracked for the same reason as the vertical arm: a follow keeps
            // the window frozen for a whole burst, and this is what catches the
            // page up when it lands.
            if zooming.get() {
                return;
            }
            // Defer to a held navigation for the closing flush (see the
            // vertical twin for the reasoning).
            if h_gate.pending().is_some() {
                return;
            }
            if page.get_untracked() == dominant {
                return;
            }
            suppress.set(true);
            page.set(dominant);
        });
    }

    {
        // page changes drive the horizontal strip
        let (suppress, page, v) = (nav.suppress.clone(), state.viewer.page, h_virtualizer.clone());
        let zooming = zooming.clone();
        let h_gate = h_gate.clone();
        Effect::new(move |_| {
            if mode.get() != ViewMode::ScrollHorizontal {
                return;
            }
            // While a zoom transaction is in flight the anchor owns the
            // strip position; re-centering a page here would fight it. The
            // gate holds the write and replays it when the transaction lands.
            let page_now = page.get();
            let zooming = zooming.get();
            let Some((target, reassert)) = h_gate.admit(page_now, zooming) else {
                if zooming {
                    suppress.set(false);
                }
                return;
            };
            if reassert {
                suppress.set(false);
                page.set(target);
            }
            if suppress.get() {
                suppress.set(false);
                return;
            }
            if target == 0 {
                return;
            }
            v.scroll_to_index((target - 1) as usize, Align::Center, scroll_mode(state));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The open-path regression: a page write that lands while a transaction
    /// is open is held, and the first quiet run replays it.
    #[test]
    fn a_page_write_during_a_transaction_replays_when_it_lands() {
        let gate = JumpGate::new();
        // Mount-time first run: page 1, no transaction.
        assert_eq!(gate.admit(1, false), Some((1, false)));
        // The mount fit opens a transaction; the resume jump arrives inside it.
        assert_eq!(gate.admit(1, true), None);
        assert_eq!(gate.pending(), None);
        assert_eq!(gate.admit(42, true), None);
        assert_eq!(gate.pending(), Some(42));
        // Frames pass with the transaction still open: nothing moves, and the
        // dominant arm can see the hold and stand aside.
        assert_eq!(gate.admit(42, true), None);
        assert_eq!(gate.pending(), Some(42));
        // The transaction closes: the held jump lands.
        assert_eq!(gate.admit(42, false), Some((42, false)));
        assert_eq!(gate.pending(), None);
    }

    /// The clobber path: the dominant arm corrects the page to the stale
    /// dominant before the replay runs — the replay must still name the HELD
    /// page and say the counter needs re-asserting.
    #[test]
    fn a_replayed_jump_survives_a_clobbered_page_signal() {
        let gate = JumpGate::new();
        assert_eq!(gate.admit(1, false), Some((1, false)));
        assert_eq!(gate.admit(42, true), None); // held: resume jump
        // The dominant arm ran first and wrote page back to 1.
        assert_eq!(gate.admit(1, false), Some((42, true)));
        // The re-assert write re-runs the arm: an ordinary page change now.
        assert_eq!(gate.admit(42, false), Some((42, false)));
    }

    /// A transaction that closes with no held write must not scroll — that is
    /// the invariant a zoom commit depends on.
    #[test]
    fn a_transaction_closing_alone_moves_nothing() {
        let gate = JumpGate::new();
        assert_eq!(gate.admit(7, false), Some((7, false)));
        assert_eq!(gate.admit(7, true), None); // gesture opens
        assert_eq!(gate.admit(7, true), None); // frames pass
        assert_eq!(gate.admit(7, false), None); // commit lands: no write held
        assert_eq!(gate.pending(), None);
    }

    /// An ordinary page change with no transaction in flight jumps at once.
    #[test]
    fn an_ordinary_page_change_jumps_immediately() {
        let gate = JumpGate::new();
        assert_eq!(gate.admit(3, false), Some((3, false)));
        assert_eq!(gate.admit(9, false), Some((9, false)));
        // Re-runs with the same page (mode flips, transaction echoes) are
        // not navigation intents.
        assert_eq!(gate.admit(9, false), None);
    }

    /// A write held by one transaction survives intermediate no-op runs and
    /// later writes replace it — the newest page is the one that lands.
    #[test]
    fn the_newest_held_write_wins_the_replay() {
        let gate = JumpGate::new();
        assert_eq!(gate.admit(1, false), Some((1, false)));
        assert_eq!(gate.admit(10, true), None);
        assert_eq!(gate.admit(20, true), None);
        assert_eq!(gate.admit(20, false), Some((20, false)));
    }

    /// The view-mode-change regression: a strip that reports a sentinel index
    /// (no window resolved yet) maps to a real page, never to 0.
    #[test]
    fn a_sentinel_dominant_index_never_becomes_page_zero() {
        // usize::MAX used to wrap to 0 through `as u32 + 1`.
        assert_eq!(page_from_dominant(usize::MAX, 300), 300);
        // A truly empty strip reads as page 1 (the first page), not 0.
        assert_eq!(page_from_dominant(0, 300), 1);
        // An index beyond the book clamps to the last page.
        assert_eq!(page_from_dominant(999, 50), 50);
        // With no pages known yet, everything clamps to page 1.
        assert_eq!(page_from_dominant(999, 0), 1);
        // Ordinary indices round-trip.
        assert_eq!(page_from_dominant(41, 300), 42);
    }
}
