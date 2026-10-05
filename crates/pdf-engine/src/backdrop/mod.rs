//! The paper state machine, wired to the engine's eyes — named `backdrop`
//! for what it drives. The pure colour math lives in the `pdf-paper` crate
//! (the brain); this module is its live half.
//!
//! Every colour decision — what a page's paper is, what the backdrop should
//! show right now — lives in the pure crate and in this state machine. The TS
//! engine keeps only the pixel plumbing: it stashes a raw frame per live
//! render, renders offscreen samples on request, and paints whatever paper it
//! is told to.
//!
//! OWNERSHIP. The state ([`Paper`]) belongs to one [`PdfSession`]: the
//! palette, the look-ahead set and the epoch are that document's, and every
//! function here takes the session it works for. Two sessions keep two
//! palettes; the root `--pdf-paper` shows the presenting session's (the
//! engine decides which, `presentSession`).
//!
//! The backdrop is a colour PER PAGE, blended along the reader's scroll
//! position so it arrives at the next page's paper at the same moment the page
//! itself does. Nothing is persisted: the palette is rebuilt from the frames
//! the reader paints (and a small look-ahead) every time a book opens — cheap,
//! one <=96px frame per page.
//!
//! The lifecycle, in one breath: [`configure`] (blend on/off, detection
//! area — a flip is a lookup, since every feed detects through both areas
//! and the other ladder is already warm), [`document_open`] (reset; publish
//! nothing until a colour is known), [`live_frame`] (drain each successful
//! render's stashed frame into the per-page ladders), [`position`] (per
//! scroll tick: the viewport's visible-paint-weighted mean page index), and
//! [`Paper::invalidate`] when the session is disposed.
//!
//! Every spawned task carries the session and its epoch and re-checks both
//! after each `await`, so a sample started for one document can never land
//! in another — nor in a session disposed while it ran.

use std::sync::atomic::{AtomicUsize, Ordering};

use wasm_bindgen_futures::spawn_local;

use pdf_paper::{PAPER_SHARE, PagePalette, PaperArea, PaperConfig, PaperDetector, Rgb};

use crate::api;
use crate::session::PdfSession;

mod lookahead;

use lookahead::ensure_lookahead;

// Named by the state-machine tests directly. Test-only on purpose: in a
// plain `cargo test` build it is reachable, and shipping it into the lib
// surface would only widen the module's public face.
#[cfg(test)]
use lookahead::lookahead_wants;

/// Look-ahead samples in flight across every session: the diagnostics
/// snapshot's gauge, which the baseline requires back to zero after the
/// sessions are gone. Kept in step with each session's `sampling` set by
/// [`Paper`]'s own methods (and its `Drop`).
static SAMPLES_IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);

/// How many look-ahead samples are in flight, realm-wide.
pub fn pending_samples() -> usize {
    SAMPLES_IN_FLIGHT.load(Ordering::Relaxed)
}

/// One session's paper state.
pub(crate) struct Paper {
    config: PaperConfig,
    blend_on: bool,
    doc_path: Option<String>,
    num_pages: u32,
    /// Per-page colours, one ladder PER DETECTION AREA, fed by every frame:
    /// a feed runs the detector through both areas at once (raw pixels are
    /// area-agnostic, and two ≤96px histograms cost less than the round trip
    /// of re-detecting the other one later), so an area flip resolves from
    /// the other ladder on the spot — at any scroll position, in either
    /// direction, with nothing to invalidate.
    palettes: [PagePalette; 2],
    /// Per-area first live colour — the fallback at book open, while the
    /// ladder has nothing near the reader's position yet.
    interim: [Option<Rgb>; 2],
    /// The last colour handed to the engine. Unknown resolutions HOLD it
    /// (the backdrop must not flash), so it is cleared only deliberately.
    published: Option<String>,
    /// The reader's page-ladder position as of the last [`position`] call.
    position: f64,
    /// Pages whose offscreen look-ahead sample is in flight. Private: every
    /// change goes through the methods that keep the realm gauge in step.
    sampling: std::collections::HashSet<u32>,
    /// Generation token: bumped on document open and on dispose.
    epoch: u64,
}

impl Default for Paper {
    fn default() -> Self {
        Self {
            config: PaperConfig::default(),
            blend_on: false,
            doc_path: None,
            num_pages: 0,
            palettes: [PagePalette::new(), PagePalette::new()],
            interim: [None, None],
            published: None,
            position: 1.0,
            sampling: std::collections::HashSet::new(),
            epoch: 0,
        }
    }
}

impl Drop for Paper {
    fn drop(&mut self) {
        self.clear_sampling();
    }
}

impl Paper {
    /// This session's look-ahead samples in flight.
    pub(crate) fn pending_samples(&self) -> usize {
        self.sampling.len()
    }

    fn start_sample(&mut self, page: u32) {
        if self.sampling.insert(page) {
            SAMPLES_IN_FLIGHT.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn end_sample(&mut self, page: u32) {
        if self.sampling.remove(&page) {
            SAMPLES_IN_FLIGHT.fetch_sub(1, Ordering::Relaxed);
        }
    }

    fn clear_sampling(&mut self) {
        SAMPLES_IN_FLIGHT.fetch_sub(self.sampling.len(), Ordering::Relaxed);
        self.sampling.clear();
    }

    /// The session is being disposed: every in-flight sample is abandoned
    /// (the epoch moves, the set empties) and the document is forgotten.
    /// The engine clears the root paper itself when the presenting session
    /// retires, so nothing is published from here.
    pub(crate) fn invalidate(&mut self) {
        self.epoch += 1;
        self.clear_sampling();
        self.doc_path = None;
        self.num_pages = 0;
        for palette in &mut self.palettes {
            palette.clear();
        }
        self.interim = [None, None];
        self.published = None;
    }
}

/// The ladder index a detection area resolves from.
pub(super) fn slot(area: PaperArea) -> usize {
    match area {
        PaperArea::WholePage => 0,
        PaperArea::Edges => 1,
    }
}

/// Spawn an engine-talking task for `session`, but ONLY when a real engine
/// is attached and the session is live. Host `cargo test` has no JS
/// runtime, so tasks that would talk to it simply never start — the
/// state-machine logic they drive is tested directly instead.
pub(super) fn spawn_engine<F: std::future::Future<Output = ()> + 'static>(
    session: &PdfSession,
    f: impl FnOnce(PdfSession) -> F + 'static,
) {
    if crate::bridge::has_pdf_reader() && session.is_live() {
        let session = session.clone();
        spawn_local(async move {
            f(session).await;
        });
    }
}

/// Land an offscreen sample of `page` taken at `epoch`: fed only when the
/// session is still live and still on the document the sample was taken
/// for. Returns whether the publish reads changed.
pub(super) fn land_sample(
    session: &PdfSession,
    epoch: u64,
    page: u32,
    frame: Option<&api::PaperFrame>,
) -> bool {
    if !session.is_live() {
        return false;
    }
    session.with_paper(|s| {
        if s.epoch != epoch {
            return false; // the document changed under the sample
        }
        s.end_sample(page);
        match frame {
            Some(f) => feed_state(s, f),
            None => false, // unreadable page: nothing to learn
        }
    })
}

/// The reader's paper settings changed (or are being restated to a new
/// session).
///
/// `blend_on` gates the engine-side frame stash: while it is off, live
/// renders skip the ≤96px downscale + readback entirely.
///
/// An area flip needs no re-detection and invalidates nothing: every feed
/// already answered through both areas, so the other ladder holds the
/// answer and `publish` moves the colour old → new in one step. The one
/// cold case is a session that holds nothing for this book (blend was off,
/// so nothing was stashed or fed): it samples the page under the cursor once.
pub(crate) fn configure(session: &PdfSession, blend_on: bool, mut config: PaperConfig) {
    config.sanitize();
    session.with_paper(|s| {
        if s.config.edge_width != config.edge_width {
            let edge_slot = slot(PaperArea::Edges);
            s.palettes[edge_slot].clear();
            s.interim[edge_slot] = None;
        }
        s.blend_on = blend_on;
        s.config = config;
    });
    session.set_paper_active(blend_on);
    publish(session);
    let cold = session.with_paper(|s| s.doc_path.is_some() && s.blend_on && s.published.is_none());
    if cold {
        let (epoch, page) = session.with_paper(|s| (s.epoch, s.position.floor().max(1.0) as u32));
        spawn_engine(session, move |session| async move {
            let frame = session.sample_paper_page(page).await.ok().flatten();
            if land_sample(&session, epoch, page, frame.as_ref()) {
                publish(&session);
            }
        });
    }
    ensure_lookahead(session);
}

/// A document opened in `session`: start its paper state. Nothing is
/// published until the reader's first frame lands.
pub(crate) fn document_open(session: &PdfSession, path: &str, num_pages: u32) {
    session.with_paper(|s| {
        s.epoch += 1; // abandon anything in flight for an earlier state
        s.clear_sampling();
        s.doc_path = Some(path.to_string());
        s.num_pages = num_pages;
        for palette in &mut s.palettes {
            palette.clear();
        }
        s.interim = [None, None];
        s.published = None;
        s.position = 1.0;
    });
    session.set_paper(None);
}

/// A live render of `canvas_id` just completed: drain its stashed raw frame
/// into the palette. A no-op while blend is off — the engine's stash is
/// gated on the same switch, so there is nothing to drain.
pub(crate) fn live_frame(session: &PdfSession, canvas_id: &str) {
    if !session.with_paper(|s| s.blend_on) {
        return;
    }
    if let Some(frame) = session.take_paper_frame(canvas_id) {
        feed_frame(session, &frame);
    }
}

/// The viewport's position along the page ladder (1-based, fractional; the
/// visible-paint-weighted mean page index). Per scroll tick. `pos <= 0`
/// means "geometry unknown" and holds the last position.
pub(crate) fn position(session: &PdfSession, pos: f64) {
    if !pos.is_finite() || pos <= 0.0 {
        return;
    }
    let moved = session.with_paper(|s| {
        let moved = (pos - s.position).abs() > f64::EPSILON;
        s.position = pos;
        moved
    });
    if moved {
        publish(session);
        ensure_lookahead(session);
    }
}

/// Feed one raw frame (live stash or offscreen sample) into the session.
fn feed_frame(session: &PdfSession, frame: &api::PaperFrame) {
    let changed = session.with_paper(|s| feed_state(s, frame));
    if changed {
        publish(session);
        ensure_lookahead(session);
    }
}

/// The state half of a feed, for in-borrow use. Returns whether anything
/// the publish reads has changed.
fn feed_state(s: &mut Paper, frame: &api::PaperFrame) -> bool {
    if s.doc_path.is_none() || frame.width == 0 || frame.height == 0 {
        return false;
    }
    // Detect through BOTH areas at once: the frame is raw pixels, the area
    // only chooses which of them vote, and a second ≤96px histogram is far
    // cheaper than re-detecting the other area when the setting flips.
    let (w, h) = (frame.width as usize, frame.height as usize);
    let edge = s.config.edge_width as usize;
    let mut whole = PaperDetector::new();
    whole.feed(PaperArea::WholePage, w, h, &frame.data, edge);
    let mut edges = PaperDetector::new();
    edges.feed(PaperArea::Edges, w, h, &frame.data, edge);
    let colours = [whole.dominant(PAPER_SHARE), edges.dominant(PAPER_SHARE)];

    let slot = slot(s.config.area);
    let changed = s.palettes[slot].get(frame.page) != colours[slot];
    let had_interim = s.interim[slot].is_some();
    if let Some(colour) = colours[0] {
        s.palettes[0].set(frame.page, colour);
    }
    if let Some(colour) = colours[1] {
        s.palettes[1].set(frame.page, colour);
    }
    if s.interim[0].is_none() {
        s.interim[0] = colours[0];
    }
    if s.interim[1].is_none() {
        s.interim[1] = colours[1];
    }
    changed || (!had_interim && s.interim[slot].is_some())
}

/// The colour the session resolves right now, if any: the current area's
/// ladder at the reader's position, with its first live colour as the
/// book-open fallback.
fn resolve(s: &Paper) -> Option<Rgb> {
    let slot = slot(s.config.area);
    s.palettes[slot].colour_at(s.position).or(s.interim[slot])
}

/// Hand the resolved colour to the engine — or clear it, but only when the
/// session is deliberately blank (no book, blend off). An UNKNOWN colour
/// holds what is already published: the backdrop must not flash to the
/// theme paper while a sample is still in flight.
pub(super) fn publish(session: &PdfSession) {
    let outcome = session.with_paper(|s| {
        if s.doc_path.is_none() || !s.blend_on {
            return (None, s.published.take());
        }
        match resolve(s).map(|c| c.to_hex()) {
            Some(hex) => {
                if s.published.as_deref() == Some(hex.as_str()) {
                    return (None, None); // unchanged
                }
                s.published = Some(hex.clone());
                (Some(hex), None)
            }
            None => (None, None), // hold
        }
    });
    match outcome {
        (Some(hex), _) => session.set_paper(Some(hex.as_str())),
        // Deliberate blank: no book / blend off — clear the session's paper.
        (None, Some(_)) => session.set_paper(None),
        _ => {}
    }
}

/// Test hook for the session tests: open a document in `session` and feed
/// one cream page, so its palette holds something.
#[cfg(test)]
pub(crate) fn test_feed(session: &PdfSession, path: &str) {
    session.with_paper(|s| s.blend_on = true);
    document_open(session, path, 4);
    let frame = tests::uniform(1, 16, 16, [0xfa, 0xf4, 0xe8]);
    feed_frame(session, &frame);
}

#[cfg(test)]
pub(crate) fn test_has_palette(session: &PdfSession) -> bool {
    session.with_paper(|s| s.palettes[0].contains(1))
}

// The state machine runs on the host: bridge calls are guarded, so only the
// in-Rust transitions are exercised — the colour math itself is the
// pdf-paper crate's own test surface.
#[cfg(test)]
mod tests {
    use super::*;
    use pdf_paper::PaperArea;
    use std::cell::RefCell;

    // Each test drives ONE fresh session; these wrappers bind the state
    // machine's functions to it so the assertions read as before.
    thread_local! {
        static CURRENT: RefCell<Option<PdfSession>> = const { RefCell::new(None) };
    }

    fn cur() -> PdfSession {
        CURRENT.with(|c| c.borrow().clone().expect("reset_session first"))
    }

    fn with<R>(f: impl FnOnce(&mut Paper) -> R) -> R {
        cur().with_paper(f)
    }

    fn document_open(path: &str, num_pages: u32) {
        super::document_open(&cur(), path, num_pages);
    }

    fn feed_frame(frame: &api::PaperFrame) {
        super::feed_frame(&cur(), frame);
    }

    fn position(pos: f64) {
        super::position(&cur(), pos);
    }

    fn configure(blend_on: bool, config: PaperConfig) {
        super::configure(&cur(), blend_on, config);
    }

    /// A uniform `w × h` frame of one colour.
    pub(super) fn uniform(page: u32, w: u32, h: u32, colour: [u8; 3]) -> api::PaperFrame {
        let mut data = vec![255u8; (w * h * 4) as usize];
        for i in (0..data.len()).step_by(4) {
            data[i] = colour[0];
            data[i + 1] = colour[1];
            data[i + 2] = colour[2];
        }
        api::PaperFrame {
            page,
            width: w,
            height: h,
            data,
        }
    }

    const CREAM: [u8; 3] = [0xfa, 0xf4, 0xe8];
    const INK: [u8; 3] = [0x40, 0x40, 0x40];
    const WHITE: [u8; 3] = [0xff, 0xff, 0xff];
    const MAROON: [u8; 3] = [0x80, 0x00, 0x00];

    fn reset_session(config: PaperConfig, blend_on: bool) {
        let session = PdfSession::create();
        session.with_paper(|s| {
            s.config = config;
            s.blend_on = blend_on;
        });
        CURRENT.with(|c| *c.borrow_mut() = Some(session));
    }

    fn published() -> Option<String> {
        with(|s| s.published.clone())
    }

    #[test]
    fn the_first_live_frame_publishes_its_colour() {
        reset_session(PaperConfig::default(), true);
        document_open("/fake/book.pdf", 10);
        feed_frame(&uniform(1, 32, 32, CREAM));
        assert_eq!(published().as_deref(), Some("#faf4e8"));
    }

    #[test]
    fn a_position_straddling_pages_blends_their_shares() {
        // THE regression: 40% page 1 + 60% page 2 must read as 60% of page
        // 2's colour — the old pair blend snapped to the dominant page's
        // colour instead, which read as a mismatch.
        reset_session(PaperConfig::default(), true);
        document_open("/fake/book.pdf", 10);
        feed_frame(&uniform(1, 32, 32, CREAM));
        feed_frame(&uniform(2, 32, 32, WHITE));
        position(1.6);
        let want = pdf_paper::lerp(
            Rgb::new(CREAM[0], CREAM[1], CREAM[2]),
            Rgb::new(WHITE[0], WHITE[1], WHITE[2]),
            0.6,
        )
        .to_hex();
        assert_eq!(published().as_deref(), Some(want.as_str()));
    }

    #[test]
    fn resting_on_a_page_publishes_exactly_its_colour() {
        reset_session(PaperConfig::default(), true);
        document_open("/fake/book.pdf", 10);
        feed_frame(&uniform(1, 32, 32, CREAM));
        feed_frame(&uniform(2, 32, 32, INK));
        position(2.0);
        assert_eq!(published().as_deref(), Some("#404040"));
    }

    #[test]
    fn an_artwork_frame_contributes_nothing_and_holds_the_published() {
        reset_session(PaperConfig::default(), true);
        document_open("/fake/book.pdf", 10);
        feed_frame(&uniform(1, 32, 32, CREAM));
        assert_eq!(published().as_deref(), Some("#faf4e8"));

        // An artwork page: sixteen distinct colour bands, each 6.25% of the
        // pixels — no bucket reaches the 10% paper share, so the page has
        // no colour to contribute and the backdrop holds what it had.
        let mut art = uniform(2, 32, 32, CREAM);
        for y in 0..32usize {
            for band in 0..16u8 {
                for x in (band as usize * 2)..(band as usize * 2 + 2) {
                    let i = (y * 32 + x) * 4;
                    art.data[i] = band.wrapping_mul(16);
                    art.data[i + 1] = 255 - band.wrapping_mul(15);
                    art.data[i + 2] = band.wrapping_mul(7).wrapping_add(3);
                }
            }
        }
        feed_frame(&art);
        assert_eq!(published().as_deref(), Some("#faf4e8"));
        assert!(!with(
            |s| s.palettes[0].contains(2) || s.palettes[1].contains(2)
        ));
    }

    #[test]
    fn an_area_flip_on_a_uniform_page_hands_over_without_a_gap() {
        reset_session(PaperConfig::default(), true);
        document_open("/fake/book.pdf", 10);
        feed_frame(&uniform(1, 32, 32, CREAM));
        assert_eq!(published().as_deref(), Some("#faf4e8"));

        configure(
            true,
            PaperConfig {
                area: PaperArea::Edges,
                ..PaperConfig::default()
            },
        );
        // One frame fed both ladders: the flip resolves from the other one
        // on the spot — same colour for a uniform page, no gap, and no
        // re-detection anywhere.
        assert_eq!(published().as_deref(), Some("#faf4e8"));
        assert!(with(|s| s.palettes[slot(PaperArea::Edges)].contains(1)));
        // A live frame under the new area keeps agreeing.
        feed_frame(&uniform(1, 32, 32, CREAM));
        assert_eq!(published().as_deref(), Some("#faf4e8"));
    }

    /// A 40×44 frame: cream centre that dominates by area under a maroon 5px
    /// margin — the two areas answer different colours for the same raster.
    fn split(page: u32) -> api::PaperFrame {
        let (w, h) = (40usize, 44usize);
        let mut data = vec![255u8; w * h * 4];
        for i in (0..data.len()).step_by(4) {
            data[i] = CREAM[0];
            data[i + 1] = CREAM[1];
            data[i + 2] = CREAM[2];
        }
        for y in 0..h {
            for x in 0..w {
                if y < 5 || y + 5 >= h || x < 5 || x + 5 >= w {
                    let i = (y * w + x) * 4;
                    data[i] = MAROON[0];
                    data[i + 1] = MAROON[1];
                    data[i + 2] = MAROON[2];
                }
            }
        }
        api::PaperFrame {
            page,
            width: w as u32,
            height: h as u32,
            data,
        }
    }

    #[test]
    fn an_area_flip_hands_over_in_both_directions_without_a_scroll() {
        // THE regression: the flip used to clear everything and wait on an
        // offscreen round trip, so once a scroll had moved the session's
        // window the backdrop sat on the stale colour until scrolling re-
        // fed live frames. Every feed now answers through both areas, so
        // the ladder a flip resolves from is warm wherever the reader
        // rests — the scroll-shaped feed order below is the old repro.
        reset_session(
            PaperConfig {
                area: PaperArea::Edges,
                ..PaperConfig::default()
            },
            true,
        );
        document_open("/fake/book.pdf", 10);
        feed_frame(&split(1));
        assert_eq!(published().as_deref(), Some("#800000"));
        feed_frame(&split(2)); // the scroll: other pages feed, position moves
        position(2.0);

        configure(true, PaperConfig::default()); // → WholePage
        assert_eq!(published().as_deref(), Some("#faf4e8"));

        configure(
            true,
            PaperConfig {
                area: PaperArea::Edges,
                ..PaperConfig::default()
            },
        );
        assert_eq!(published().as_deref(), Some("#800000"));
    }

    #[test]
    fn changing_edge_width_invalidates_only_the_edge_cache() {
        reset_session(
            PaperConfig {
                area: PaperArea::Edges,
                ..PaperConfig::default()
            },
            true,
        );
        document_open("/fake/book.pdf", 3);
        feed_frame(&split(1));

        let mut config = with(|s| s.config);
        config.edge_width += 1;
        configure(true, config);

        with(|s| {
            assert!(s.palettes[slot(PaperArea::WholePage)].contains(1));
            assert!(s.interim[slot(PaperArea::WholePage)].is_some());
            assert!(s.palettes[slot(PaperArea::Edges)].is_empty());
            assert!(s.interim[slot(PaperArea::Edges)].is_none());
            assert!(resolve(s).is_none());

            // `configure` already queued these samples. Clear that bookkeeping
            // to inspect the pure look-ahead decision against the new cache.
            s.clear_sampling();
            assert_eq!(lookahead_wants(s), vec![1, 2, 3]);
        });
    }

    #[test]
    fn blend_off_never_publishes() {
        reset_session(PaperConfig::default(), false);
        document_open("/fake/book.pdf", 10);
        feed_frame(&uniform(1, 32, 32, CREAM));
        assert_eq!(published(), None);
    }

    #[test]
    fn turning_blend_off_clears_a_published_colour() {
        reset_session(PaperConfig::default(), true);
        document_open("/fake/book.pdf", 10);
        feed_frame(&uniform(1, 32, 32, CREAM));
        assert_eq!(published().as_deref(), Some("#faf4e8"));
        configure(false, PaperConfig::default());
        assert_eq!(published(), None);
    }

    #[test]
    fn disposing_the_session_forgets_the_book() {
        reset_session(PaperConfig::default(), true);
        document_open("/fake/book.pdf", 10);
        feed_frame(&uniform(1, 32, 32, CREAM));
        futures::executor::block_on(cur().dispose());
        assert_eq!(published(), None);
        assert!(with(
            |s| s.palettes[0].is_empty() && s.palettes[1].is_empty()
        ));
        assert!(with(|s| s.doc_path.is_none()));
        // A disposed session ignores every later paper call.
        cur().paper_position(3.0);
        cur().paper_document_open("/fake/other.pdf", 3);
        assert!(with(|s| s.doc_path.is_none()));
    }

    #[test]
    fn a_sample_that_outlives_its_session_is_dropped() {
        reset_session(PaperConfig::default(), true);
        document_open("/fake/book.pdf", 10);
        let session = cur();
        let epoch = session.with_paper(|s| {
            s.start_sample(2);
            s.epoch
        });
        assert_eq!(session.paper_pending_samples(), 1);
        futures::executor::block_on(session.dispose());
        // The dispose abandoned the in-flight sample (the realm gauge falls
        // with the session's set), and the late answer lands nowhere.
        assert_eq!(session.paper_pending_samples(), 0);
        let frame = uniform(2, 32, 32, CREAM);
        assert!(!land_sample(&session, epoch, 2, Some(&frame)));
        assert!(with(|s| !s.palettes[0].contains(2)));
    }

    #[test]
    fn a_sample_from_before_a_reopen_is_dropped() {
        reset_session(PaperConfig::default(), true);
        document_open("/fake/book.pdf", 10);
        let session = cur();
        let epoch = session.with_paper(|s| {
            s.start_sample(2);
            s.epoch
        });
        document_open("/fake/book.pdf", 10); // epoch moves on
        let frame = uniform(2, 32, 32, CREAM);
        assert!(!land_sample(&session, epoch, 2, Some(&frame)));
        assert_eq!(session.paper_pending_samples(), 0);
    }

    #[test]
    fn two_sessions_keep_two_palettes() {
        reset_session(PaperConfig::default(), true);
        document_open("/fake/a.pdf", 10);
        feed_frame(&uniform(1, 32, 32, CREAM));
        let a = cur();
        reset_session(PaperConfig::default(), true);
        document_open("/fake/b.pdf", 10);
        feed_frame(&uniform(1, 32, 32, INK));
        let b = cur();
        assert_eq!(
            a.with_paper(|s| s.published.clone()).as_deref(),
            Some("#faf4e8")
        );
        assert_eq!(
            b.with_paper(|s| s.published.clone()).as_deref(),
            Some("#404040")
        );
        futures::executor::block_on(a.dispose());
        assert_eq!(
            b.with_paper(|s| s.published.clone()).as_deref(),
            Some("#404040")
        );
    }

    #[test]
    fn the_lookahead_names_the_pair_and_the_page_after() {
        reset_session(PaperConfig::default(), true);
        document_open("/fake/book.pdf", 10);
        feed_frame(&uniform(3, 32, 32, CREAM)); // page 3 known (a live frame)
        // Set the position directly: `position()` would mark the wanted
        // pages as in-flight (spawned samples), which is exactly what the
        // NEXT assertion must not see.
        with(|s| s.position = 3.0);
        let wants = with(|s| lookahead_wants(s));
        assert_eq!(wants, vec![4, 5]); // 3 is known; the pair's next page +1
    }

    #[test]
    fn the_lookahead_stops_at_the_last_page() {
        reset_session(PaperConfig::default(), true);
        document_open("/fake/book.pdf", 3);
        with(|s| s.position = 3.0);
        assert_eq!(with(|s| lookahead_wants(s)), vec![3]); // page 3, nothing after
    }

    #[test]
    fn the_lookahead_is_quiet_when_blend_is_off() {
        reset_session(PaperConfig::default(), false);
        document_open("/fake/book.pdf", 10);
        with(|s| s.position = 1.0);
        assert!(with(|s| lookahead_wants(s)).is_empty());
    }

    #[test]
    fn an_unsampled_position_falls_back_to_the_interim() {
        // An empty stretch of the palette (samples still in flight): the
        // first live colour holds the backdrop instead of flashing.
        reset_session(PaperConfig::default(), true);
        document_open("/fake/book.pdf", 10);
        feed_frame(&uniform(1, 32, 32, CREAM));
        position(4.2); // nothing sampled that far yet
        assert_eq!(published().as_deref(), Some("#faf4e8"));
    }
}
