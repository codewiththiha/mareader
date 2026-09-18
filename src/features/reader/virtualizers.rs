//! Builds and wires the reader's two virtualizers: the vertical scroll list
//! and the horizontal strip. Extracted out of `ReaderPage` so the page
//! component stays a layout + effect coordinator rather than a setup pile.
//!
//! On top of the windowing this carries the SMART VIRTUALIZER (see
//! Mareader.md, "The smart virtualizer"): the scroll-phase machine
//! (`scroll_kinetics`), the phase-driven paint window (`render_gate`), the
//! ghost placeholder (the engine renders it, `ghost` picks the page), the
//! engine's render gate and settle flush, the byte budget on the retained
//! raw rasters, and the fling-time zombie trim. One `SmartCfg` is the whole
//! surface a caller tunes.

use std::cell::Cell;
use std::hash::Hash;
use std::rc::Rc;

use leptos::prelude::*;
use leptos::task::spawn_local;
use virtual_list::Viewport;
use virtual_list_leptos::{VirtualizerOptions, use_virtualizer};

use reader_core::view::{RENDER_BUDGET, ViewMode};

use crate::epoch::epoch_signal;
use crate::features::reader::ghost::modal_page;
use crate::features::reader::render_gate::{merge_phases, paint_range, PrefetchCfg};
use crate::features::reader::scroll_kinetics::{KineticsConfig, ScrollKinetics, ScrollPhase};
use crate::state::{AppearanceSignal, ReaderState};
use crate::zoom::config::{MAX_ZOMBIES, STRIP_SCROLL_GRACE_MS};

/// The smart virtualizer's tuning surface. Everything the phase machine,
/// the paint window and the engine governor read from; `Default` is the
/// calibrated set, and a caller that wants deeper prefetch changes one
/// field, not a dozen constants.
#[derive(Clone, Copy, Debug)]
pub struct SmartCfg {
    pub kinetics: KineticsConfig,
    pub prefetch: PrefetchCfg,
    /// The ghost raster's height, in px (its width follows the modal
    /// page's aspect).
    pub ghost_h: u32,
    /// The byte budget the engine holds its retained raw rasters to.
    pub budget_mb: u32,
}

impl Default for SmartCfg {
    fn default() -> Self {
        Self {
            kinetics: KineticsConfig::default(),
            prefetch: PrefetchCfg::default(),
            ghost_h: 140,
            budget_mb: 96,
        }
    }
}

/// The context the page slots read: the paint window and the ghost
/// placeholder. Provided by `ReaderPage` from the handles below, so a strip
/// never has to ask which virtualizer is active.
#[derive(Clone, Copy)]
pub struct SmartVirtualizer {
    /// The absolute 0-based paint window for the current scroll phase, or
    /// `None` while a fling parks every render.
    pub paint_window: Signal<Option<(usize, usize)>>,
    /// The ghost placeholder's image url, when the open document has one.
    pub ghost: Signal<Option<String>>,
}

/// The handles `ReaderPage` hands to the viewer components and effects. Both
/// virtualizers always exist (they are hooks); a view binds only the one for
/// its axis when it mounts. The `StoredValue`s are the `Clone`-safe wrappers
/// the components pass by value, while the raw handles drive the effects.
pub(crate) struct ReaderVirtualizers {
    pub virtualizer: virtual_list_leptos::Virtualizer,
    pub h_virtualizer: virtual_list_leptos::Virtualizer,
    pub virtualizer_view: StoredValue<virtual_list_leptos::Virtualizer, LocalStorage>,
    pub h_virtualizer_view: StoredValue<virtual_list_leptos::Virtualizer, LocalStorage>,
    /// The smart-virtualizer surface, provided as context by `ReaderPage`.
    pub smart: SmartVirtualizer,
}

/// Page-1 height, the estimate for any page whose own size is unknown.
fn fallback_height(state: ReaderState) -> f64 {
    state
        .document
        .content
        .metrics.page1_size
        .get_untracked()
        .map_or(0.0, |size| size.height)
}

/// Keeps `css_heights` — the shared measurement store behind the vertical
/// virtualizer and the zoom commit path — filled from the intrinsic sizes.
///
/// The rule is simply "an empty store gets seeded": the open flow empties it
/// for every new document (the same book included), the zoom coordinator
/// rescales it in place, and the pages overwrite entries as they measure.
/// So emptiness is exactly "a book just arrived", and nothing else — not a
/// zoom tick, not a re-measure — can trigger a re-seed that would clobber
/// live heights.
fn seed_css_heights(state: ReaderState) {
    Effect::new(move |_| {
        let count = state.document.num_pages.get() as usize;
        let filled = state.document.content.metrics.css_heights.with(|heights| !heights.is_empty());
        let scale = state.viewer.zoom.display.get();
        if filled || count == 0 || scale <= 0.0 {
            return;
        }
        // Tracked reads: the store is only worth filling once the sizes are
        // there, and they arrive in the same open that emptied it.
        let fallback = state
            .document
            .content
            .metrics
            .page1_size
            .get()
            .map_or(0.0, |size| size.height);
        let seeded: Vec<f64> = state.document.content.metrics.intrinsic.with(|sizes| {
            (0..count)
                .map(|index| {
                    sizes
                        .get(index)
                        .map(|size| size.height)
                        .filter(|height| *height > 0.0)
                        .unwrap_or(fallback)
                        * scale
                })
                .collect()
        });
        if seeded.iter().all(|height| *height <= 0.0) {
            return; // nothing measured yet; keep the store empty for the next run
        }
        state.document.content.metrics.css_heights.set(seeded);
    });
}

/// A fingerprint of the document's geometry: page count plus every
/// intrinsic size. The virtualizers rebuild their layouts when it changes.
fn geometry_epoch(state: ReaderState) -> Signal<u64> {
    epoch_signal(move |hasher| {
        state.document.num_pages.get().hash(hasher);
        state.document.content.metrics.intrinsic.with(|sizes| {
            sizes.len().hash(hasher);
            for size in sizes {
                size.width.to_bits().hash(hasher);
                size.height.to_bits().hash(hasher);
            }
        })
    })
}

pub(crate) fn use_reader_virtualizers(state: ReaderState, cfg: SmartCfg) -> ReaderVirtualizers {
    seed_css_heights(state);

    let count = Signal::derive(move || state.document.num_pages.get() as usize);
    let estimate = move |index: usize| {
        let gap = state.viewer.page_gap.get_untracked();
        let measured = state
            .document
            .content.metrics
            .css_heights
            .with_untracked(|heights| heights.get(index).copied())
            .filter(|height| *height > 0.0);
        if let Some(height) = measured {
            return height + gap;
        }
        let intrinsic = state
            .document
            .content.metrics
            .intrinsic
            .with_untracked(|sizes| sizes.get(index).map(|size| size.height))
            .filter(|height| *height > 0.0);
        intrinsic.unwrap_or_else(|| fallback_height(state)) * state.viewer.zoom.visual_scale() + gap
    };
    // Both strips estimate from the live DISPLAY scale — the scale the
    // layout is relaid out to as a zoom runs — so the two axes can never
    // disagree about how big a page is.
    let h_estimate = move |index: usize| {
        state.document.content.metrics.intrinsic.with_untracked(|sizes| {
            sizes.get(index).map(|s| s.width).unwrap_or(0.0)
        }) * state.viewer.zoom.visual_scale()
            + 2.0 * state.viewer.page_margin.get_untracked()
    };
    let epoch = geometry_epoch(state);
    let pinned_sig: RwSignal<Option<(usize, usize)>> = RwSignal::new(None);
    let initial_vh = {
        let (_, height) = state.viewer.container_size.get_untracked();
        if height > 1.0 {
            height
        } else {
            800.0
        }
    };
    // Start the window on the RESUME page rather than at the top: page 1 is
    // never in a fresh open's first window, so it is never mounted, never
    // rendered, and its raster can never flash past on the way to the page
    // the reader actually resumes on. Summed under the SAME estimates the
    // virtualizer builds its layout from, so the first window sits exactly
    // where the mount anchor (`anchor_to_page`) is about to aim — the
    // anchor still re-asserts, it simply agrees on its first frame. The
    // reader's page is seeded by the open flow BEFORE the route flips
    // (`enter_ready` last), so it is already the resume page here.
    let resume_index = {
        let count0 = state.document.num_pages.get_untracked() as usize;
        ((state.viewer.page.get_untracked().max(1) as usize) - 1).min(count0)
    };
    let mut v_off = 0.0;
    let mut h_off = 0.0;
    for index in 0..resume_index {
        v_off += estimate(index);
        h_off += h_estimate(index);
    }
    // Zombie retention: an item that leaves the window mid-fling (or in a
    // zoom's geometry commit — the controller raises the grace for that)
    // keeps its DOM briefly instead of popping out. The bind hooks hand the
    // scroll container to the phase tracker (and back on unmount), so the
    // tracker's listener lives exactly as long as the container binding.
    let v_kin = ScrollKinetics::new(cfg.kinetics);
    let h_kin = ScrollKinetics::new(cfg.kinetics);

    let virtualizer = use_virtualizer(
        VirtualizerOptions::list(count, estimate)
            .gap(0.0)
            .budget(RENDER_BUDGET)
            .initial(Viewport::main_only(initial_vh), v_off)
            .pinned(pinned_sig.into())
            .epoch(epoch)
            .retention(STRIP_SCROLL_GRACE_MS, MAX_ZOMBIES)
            .on_bind({
                let kin = v_kin.clone();
                move |el: web_sys::Element| kin.attach(&el)
            })
            .on_unbind({
                let kin = v_kin.clone();
                move || kin.detach()
            }),
    );

    let h_virtualizer = use_virtualizer(
        VirtualizerOptions::list(count, h_estimate)
            .axis(virtual_list_leptos::Axis::Horizontal)
            .gap(0.0)
            .budget(RENDER_BUDGET)
            .padding(0.0, 0.0)
            .initial(Viewport::new(1200.0, initial_vh), h_off)
            .epoch(epoch)
            .retention(STRIP_SCROLL_GRACE_MS, MAX_ZOMBIES)
            .on_bind({
                let kin = h_kin.clone();
                move |el: web_sys::Element| kin.attach(&el)
            })
            .on_unbind({
                let kin = h_kin.clone();
                move || kin.detach()
            }),
    );
    let h_virtualizer_view = StoredValue::new_local(h_virtualizer.clone());

    // The engine only sweeps its rasters inside render activity; after a
    // zoom-out or a mode flip nothing renders, so the big rasters would stay
    // pinned until the 30s idle timer. Sweep the moment scrolling settles
    // instead — both virtualizers, registered once (the views rebind the
    // SAME shared virtualizer on every mode flip).
    virtualizer.on_scroll_idle(pdf_engine::api::sweep);
    h_virtualizer.on_scroll_idle(pdf_engine::api::sweep);

    {
        let v = virtualizer.clone();
        Effect::new(move |_| {
            let mut pin = None;
            if state.viewer.zoom.transition.get().is_some() {
                let dominant = v.dominant().get_untracked();
                pin = Some((dominant, dominant));
            }
            if let Some((first, last)) = state.viewer.selected_pages.get() {
                let selected = (first.saturating_sub(1) as usize, last.saturating_sub(1) as usize);
                pin = Some(match pin {
                    Some((a, b)) => (a.min(selected.0), b.max(selected.1)),
                    None => selected,
                });
            }
            pinned_sig.set(pin);
        });
    }

    // ---- the smart virtualizer ---------------------------------------------

    // The merged scroll phase: the highest pressure the two axes report,
    // with a zoom folded in as a fling (its geometry churn is the same
    // pressure class — and the zoom's own render suspension already assumes
    // it). One signal; the paint window, the engine gate, the retention and
    // the settle flush all read from it.
    let v_phase = v_kin.phase_signal();
    let h_phase = h_kin.phase_signal();
    let zooming = state.viewer.zoom.transition;
    let phase = Signal::derive(move || {
        let merged = merge_phases(&[v_phase.get(), h_phase.get()]);
        if zooming.get().is_some() {
            merged.max_pressure(ScrollPhase::Fling)
        } else {
            merged
        }
    });

    // The dominant page of the ACTIVE axis: the paint window and the settle
    // flush both grow from it, and the inactive axis's dominant is a stale
    // number from whenever its mode was last live. The virtualizers' own
    // dominant signals are local (their closures hold the Rc core), so the
    // mirror below carries them into two global signals — effects make no
    // thread-safety promise, the derive above must.
    let v_dom = RwSignal::new(0);
    let h_dom = RwSignal::new(0);
    {
        let v = virtualizer.clone();
        let sig = v_dom;
        Effect::new(move |_| sig.set(v.dominant().get()));
    }
    {
        let hv = h_virtualizer.clone();
        let sig = h_dom;
        Effect::new(move |_| sig.set(hv.dominant().get()));
    }
    let dominant = Signal::derive(move || {
        if state.viewer.mode.get() == ViewMode::ScrollHorizontal {
            h_dom.get()
        } else {
            v_dom.get()
        }
    });

    let paint_window = Signal::derive(move || {
        paint_range(
            phase.get(),
            dominant.get(),
            state.document.num_pages.get() as usize,
            &cfg.prefetch,
        )
    });

    // The engine's render lane follows the phase: parked while a fling (or
    // a zoom — the merge above already ranks it as one) is in flight, open
    // the moment it is not. Jobs keep queuing while parked and drain by
    // priority on the reopen.
    {
        Effect::new(move |_| {
            pdf_engine::api::set_render_gate(phase.get() == ScrollPhase::Fling);
        });
    }

    // The settle flush: the moment the phase leaves the parked states, the
    // pages the reader landed on jump the engine queue, the byte budget is
    // enforced from the landing page out, and the sweep drops what the
    // fling's churn left behind.
    {
        let forward = cfg.prefetch.forward;
        let budget_bytes = cfg.budget_mb.saturating_mul(1024).saturating_mul(1024);
        Effect::new(move |_| {
            if !matches!(phase.get(), ScrollPhase::Idle | ScrollPhase::Cruising) {
                return;
            }
            // The landing page is read untracked on purpose: the flush is
            // owed by the TRANSITION out of the parked states, and chasing
            // every dominant tick during steady reading would re-promote
            // pages the paint window has already covered.
            let d = dominant.get_untracked();
            let pages: Vec<u32> = (d..d + forward + 1).map(|i| (i + 1) as u32).collect();
            pdf_engine::api::promote_pages(&pages);
            pdf_engine::api::enforce_page_budget((d + 1) as u32, budget_bytes);
            pdf_engine::api::sweep();
        });
    }

    // Fling-time zombie trim: while a fling is in flight nothing is worth
    // keeping — the ghosts re-mask everything, and the pages the reader
    // re-approaches get painted by the settle flush — so the retention grace
    // drops to zero and back. A zoom owns its own raised grace for its
    // duration, so this stands down while a transition is in flight rather
    // than overwriting it.
    {
        let v = virtualizer.clone();
        let hv = h_virtualizer.clone();
        Effect::new(move |_| {
            if zooming.get().is_some() {
                return;
            }
            if phase.get() == ScrollPhase::Fling {
                v.set_retention_grace(0);
                hv.set_retention_grace(0);
            } else {
                v.reset_retention_grace();
                hv.reset_retention_grace();
            }
        });
    }

    // The ghost provider: the engine renders the modal page once (and
    // re-renders it when the appearance moves — the engine dedupes against
    // its own pipeline generation, so a change that leaves the pipeline
    // alone is a no-op). Gated on a paged document: a reflowable stream has
    // no page-shaped ghost, and the signal clears with it so a reopened book
    // does not flash the last one's placeholder. The generation guard keeps
    // a slow render of one book from landing on the next.
    let ghost: RwSignal<Option<String>> = RwSignal::new(None);
    {
        let ghost_sig = ghost;
        let intrinsic = state.document.content.metrics.intrinsic;
        let appearance = use_context::<AppearanceSignal>()
            .expect("AppearanceSignal must be provided by the app root");
        let ghost_gen = Rc::new(Cell::new(0u64));
        let height = cfg.ghost_h as f64;
        Effect::new(move |_| {
            // Track the appearance: a theme change re-bakes the ghost.
            let _ = appearance.get();
            if state.reflowable() {
                ghost_sig.set(None);
                return;
            }
            let Some((page0, _, _)) = intrinsic.with(|sizes| modal_page(sizes)) else {
                return;
            };
            let revision = ghost_gen.get() + 1;
            ghost_gen.set(revision);
            let page = (page0 + 1) as u32;
            // Each run owes its task its OWN guard clone: the effect re-runs
            // on every appearance change, and moving the shared guard into
            // the first task would leave the rest blind to document swaps.
            let guard = ghost_gen.clone();
            spawn_local(async move {
                if let Some(url) = pdf_engine::api::render_ghost(page, height).await
                    && guard.get() == revision
                {
                    ghost_sig.set(Some(url));
                }
            });
        });
    }

    let smart = SmartVirtualizer {
        paint_window,
        ghost: ghost.read_only().into(),
    };

    ReaderVirtualizers {
        virtualizer: virtualizer.clone(),
        h_virtualizer: h_virtualizer.clone(),
        virtualizer_view: StoredValue::new_local(virtualizer),
        h_virtualizer_view,
        smart,
    }
}
