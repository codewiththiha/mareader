//! The zoom sources: thin reactive watchers that re-ask the zoom controller
//! when the world under the reader changes.
//!
//! Neither watcher computes a scale, writes a zoom signal, or calls the
//! engine — they POST COMMANDS ([`ZoomCommand`]) and the controller's one
//! transition pipeline does the rest. The one question either asks back is
//! `posting_gate`: whether a transaction is already open that it must not
//! disturb, and whether the burst it is holding should carry this change too.
//! That is a gate on posting, not a share of the maths. It is the whole point of
//! the split: fit width, fit page, a sidebar slide, a window resize and a manual
//! `+` all land through the same resolve → relayout → commit path, so they can no
//! longer race along separate code paths.
//!
//! What the two watchers differ in is CADENCE, which is the whole difference
//! between a page that rides a resize and a page that waits for one:
//!
//! - `follow_watcher`: the SPACE the page has moved — a sidebar slide or a
//!   window drag, both of which report a burst of container sizes. It fires on
//!   every frame of that burst, because a scale frozen through it leaves the
//!   page host wider than the box it now has to fit in, and the flex engine
//!   squishes the paper (a letter page went from a 0.77 aspect to 0.58) until
//!   the refit finally lands. Only the crisp RENDER waits: a follow holds its
//!   commit until the container has been quiet, so a slide costs one raster
//!   pass instead of one per frame.
//!
//!   Only the WINDOW half of that has a switch (Animations → "Canvas Follows
//!   Window"), and the tell is the viewport itself: it moves on a drag and not
//!   when the rail takes the reader's column. With that switch off the follow
//!   does not stop happening — it stops being animated: the frames are dropped
//!   and the end frame lands once the burst goes quiet. The rail half is not
//!   switchable, because following a measured container is what makes an
//!   instant resize instant: deferring it left the page cropped for the whole
//!   settle window, and the crop is what flipped the strip's overflow, which is
//!   a second, worse frame.
//! - `fit_watcher`: the world under the scale changed in a way that is not a
//!   burst — a view-mode flip, a new document, and (only while the Auto Resize
//!   setting is on) a page turn in a mixed-size book. Single events, so they
//!   debounce and land through the ordinary transition, and a page-only change
//!   must NOT follow the layout per frame:
//!   scrolling through a book of alternating sizes would re-fit at every row
//!   boundary. It posts the fit when a fit owns the scale and the reader's
//!   own `desired` when they zoomed by hand, so neither case is
//!   left stale against the page under the eyes. Choosing or dropping a fit is
//!   the one event it does not postpone: that is a click, and a click answers
//!   in the frame it lands. With Auto Resize off the page dependency is not
//!   even subscribed, so a page turn cannot reach this watcher at all.
//!
//! Both fires are deliberately UNTWEENED. A refit that tracks a live resize has
//! to land in the frame it was asked for; queueing a 120ms animation against
//! each new width had the page visibly chasing the window. Manual zooms keep
//! their tween — they are one gesture, not a stream.

use std::time::Duration;

use leptos::prelude::*;

use pdf_core::math::FitMode;

use app_chrome::hooks::use_timeout::use_debounce;
use app_chrome::hooks::use_viewport::use_viewport;
use crate::state::reader::ZoomCommand;
use crate::state::{AppState, SidebarMode};
use crate::viewer::zoom::config::FOLLOW_SETTLE_MS;
use crate::viewer::zoom::command::{Gate, posting_gate};

/// Trailing debounce for a discrete refit: the same window of quiet a held
/// follow waits for before it commits, so a page turn and the end of a resize
/// are settled by one number rather than two that can drift apart.
const REFIT_DEBOUNCE: Duration = Duration::from_millis(FOLLOW_SETTLE_MS);

/// Re-resolve the scale when the world under it changes in a way that is not
/// the container moving: a fit mode chosen or dropped, or a view-mode flip.
/// Must be called once from the reader shell (ReaderPage), alongside
/// `follow_watcher`.
///
/// A PAGE turn joins that list only while Settings → Layout → Auto Resize is
/// on. The page matters to the scale because the ceiling a hand-picked zoom is
/// held to is the fit width OF THE PAGE UNDER THE EYES: with Auto Resize on, a
/// landscape plate at 200 percent shrinks to stay readable instead of sitting
/// cropped, and — because the ceiling never writes `desired` — the next
/// portrait page grows back to exactly the zoom the reader chose.
///
/// With it off, arriving at a new page changes NOTHING: not the scale, not the
/// scroll position, not the measured column. That is the difference between "fit
/// the sheet I am looking at" and "fit the window I am looking at", and a reader
/// who picked a zoom by hand wants the second one — a page too wide for the
/// window then overflows and scrolls, which is a choice with an affordance,
/// rather than a size the app picked back over their head mid-sentence.
pub fn fit_watcher(state: AppState) {
    // Built in the owner that calls us, not inside the effect: the debouncer
    // is one per watcher and disarms itself on cleanup, so a fire cannot
    // land on a reader that has already been disposed.
    let vs = state.reader.viewer;
    let refit = use_debounce(REFIT_DEBOUNCE, move || {
        // Asked again AT THE FIRE, not only when the burst armed it: a gesture
        // that started while the fire was pending owns the transaction, and
        // resolving a fit into it mid-tween is the snap this gate exists for.
        // (A dropped fire is cheap: a manual zoom clears the fit mode, so the
        // reader has taken the scale over anyway, and the next page, mode or
        // container change re-arms this.)
        let cmd = match posting_gate(vs.zoom.transition.get_untracked()) {
            Gate::StandDown => return,
            // A slide is mid-flight: hand the change to its held commit rather
            // than land a second transaction that renders in the middle of the
            // burst. A follow resolves the same question — the fit when a fit
            // owns the scale, the ceiling otherwise — so nothing is lost.
            Gate::Follow => ZoomCommand::Follow,
            Gate::Now if vs.fit.get_untracked() == FitMode::None => {
                ZoomCommand::Constrain
            }
            Gate::Now => ZoomCommand::Refit,
        };
        // Untweened: a fit tracks the window, so it must land in the frame it
        // was resolved in rather than chase it for the length of a tween.
        vs.zoom.post(cmd, false);
    });

    // The fit the last run answered, so "a fit mode was CHOSEN" can be told
    // apart from "the page under a fit changed". Opening a document lands in
    // the chosen branch too, which is right: the first fit is a decision, not a
    // scroll artefact.
    let last_fit = StoredValue::new_local(vs.fit.get_untracked());

    Effect::new(move |_| {
        // Every dependency is a tracked read; none of the values are needed
        // locally — the controller re-reads the world when it resolves. The
        // container is deliberately NOT one of them: a slide or a drag is a
        // stream, and streams are `follow_watcher`'s business.
        let fit = vs.fit.get();
        let chosen = last_fit.get_value() != fit;
        let _ = vs.mode.get();
        // Read CONDITIONALLY, so that turning the setting off also drops the
        // subscription: while Auto Resize is off, a page turn does not re-run
        // this effect at all. The setting itself is read as a dependency just
        // above the branch, so flipping it re-runs this and re-arms or
        // disarms the page dependency in the same frame.
        if state.settings.with(|st| st.layout.auto_resize) {
            let _ = vs.page.get();
        }
        // The transaction is read TRACKED, on purpose. A fit or mode change
        // that arrives while a gesture owns the transaction must not be
        // LOST — the edge is left unconsumed (last_fit still names the
        // pre-gesture fit), and this very subscription re-runs the effect
        // when the gesture commits, so the choice lands late instead of
        // never. An untracked read here is how a fit click during a zoom
        // used to vanish: the edge was consumed below, the gate returned
        // StandDown, and nothing ever came back for it.
        if matches!(posting_gate(vs.zoom.transition.get()), Gate::StandDown) {
            return; // a gesture owns the transaction; nothing is consumed
        }
        last_fit.set_value(fit);
        // A fit the reader just picked is an answer to a click, not a burst: it
        // moves the page NOW and sharpens on the follow's held commit, exactly
        // like riding a resize. Debouncing it would hold the page still for the
        // whole window for no reason.
        if chosen {
            vs.zoom.post(ZoomCommand::Follow, false);
            return;
        }
        // Postpones any pending fire and schedules one at the end of the
        // current burst of changes.
        refit.trigger();
    });
}

/// Follow the space the page has, frame by frame. This is what makes the
/// sidebar slide and the window drag MOVE the canvas instead of snapping it
/// when the drag ends, and it covers both readers: while a fit mode is active
/// the follow re-fits, and while the reader has zoomed by hand it keeps the
/// chosen zoom (`desired`, clamped) — a page the reader zoomed in on stays at
/// that scale across the change and overflows with a scroll affordance rather
/// than being snapped back to the fit width. The ceiling is computed from the
/// remembered `desired` and never from the live scale times a container ratio.
///
/// What following costs is memory, not frames: a held transaction raises the
/// strip's eviction grace for as long as it is open, so a long burst keeps the
/// pages it has already drawn (DOM node and last bitmap) alive until the commit,
/// where `sweep()` releases them. A deferred follow has a shorter burst and so
/// holds less; that is the whole trade, and the frame it saves is the one that
/// left the page cropped.
///
/// It follows on EVERY frame, and that is the point rather than an oversight:
/// a `land()` handed to the next animation frame paints after the container
/// has already shrunk, and a page row the flex engine may not resize is then a
/// few pixels wider than its box — a scrollbar flicker for the length of the
/// burst. Landing in the frame the size was reported is what the follow costs
/// (one relayout per frame, one raster at the end) and what it buys.
///
/// Must be called once from the reader shell (ReaderPage), alongside
/// `fit_watcher`.
pub fn follow_watcher(state: AppState, sidebar: RwSignal<SidebarMode>) {
    let vs = state.reader.viewer;
    // The same end frame, delivered late instead of frame by frame, for the one
    // burst that is allowed to skip its frames. Debounced by the window a held
    // follow already commits on, so the two cadences agree about when
    // "finished" means finished.
    let late = use_debounce(REFIT_DEBOUNCE, move || {
        if matches!(
            posting_gate(vs.zoom.transition.get_untracked()),
            Gate::StandDown
        ) {
            return; // a gesture took the transaction over; it commits itself
        }
        vs.zoom.post(ZoomCommand::Follow, false);
    });
    // The window's own box, as a signal fed by a `resize` listener rather than a
    // `getBoundingClientRect` here: this effect runs on every frame of a burst,
    // and a layout read inside a ResizeObserver callback forces the very flush
    // the follow exists to avoid. The listener fires before the observer's
    // callback in the same rendering update, so the value is never stale.
    let viewport = use_viewport();
    // What the viewport measured the last time this ran — a memory, not a
    // dependency; the effect must not subscribe to the resize signal, or it
    // would run twice per frame and attribute one of them to the wrong switch.
    let last_viewport = StoredValue::new_local(viewport.get_untracked());

    Effect::new(move |_| {
        let _ = vs.container_size.get();
        let _ = vs.page_margin.get();
        // Tracked so a toggle starts the follow on the frame the rail MOVES,
        // not only once its animation has begun resizing the container. The
        // value itself is not: the page is sized from the space that is
        // actually available, whatever took it.
        let _ = sidebar.get();
        let now = viewport.get_untracked();
        let before = last_viewport.get_value();
        last_viewport.set_value(now);
        // The rail takes the reader's column without moving the window, so an
        // unchanged viewport means this frame is the sidebar's — and the
        // sidebar's follow is unconditional (see the module doc for why
        // deferring it is worse than doing it). Only a window drag may wait.
        // UNTRACKED read of the switch, deliberately: the flag that turns a
        // follow off must not be what re-triggers a follow.
        let window_dragged = (now.0 - before.0).abs() >= 0.5 || (now.1 - before.1).abs() >= 0.5;
        if window_dragged && !vs.motion.get_untracked().canvas_resize {
            late.trigger();
            return;
        }
        late.cancel();
        // The transaction is read UNTRACKED, on purpose. Tracking it is what
        // forced a one-shot "I just committed, do not re-apply the ceiling"
        // echo onto the old implementation, and this pipeline does not need
        // the echo: after a gesture lands, the ceiling answers the next real
        // container change, which is the case it was written for. Zooming past
        // the fit stays put until then, exactly as it does in every desktop
        // reader.
        if matches!(
            posting_gate(vs.zoom.transition.get_untracked()),
            Gate::StandDown
        ) {
            return; // a gesture owns the transaction; let it land first
        }
        // Posting once per frame is not per-frame WORK. `commands` is a
        // single-slot signal: a post that arrives before the controller has
        // drained the previous one simply replaces it, and the controller
        // resolves at most one command per frame anyway. Coalescing these
        // through a `request_animation_frame` of our own would buy back one
        // signal write and pay a whole frame of latency for it — which is the
        // frame that leaves the page cropped inside its new box.
        vs.zoom.post(ZoomCommand::Follow, false);
    });
}
