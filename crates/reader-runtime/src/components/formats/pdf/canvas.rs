//! The shared page host: a `.pdf-page` div containing a `<canvas>` and a
//! `.textLayer` div, driven by the JS engine. Used by BOTH view modes.
//!
//! Contract: ids are Rust-chosen unique strings and the engine resolves
//! elements by id; renders (page, scale) via engine.renderPage and reports the
//! rendered CSS-px size — snapped to the device-pixel grid, the same size
//! written to the host — through `on_geometry`; registers on first render,
//! unregisters (cancels) when disposed.
//!
//! TWO EFFECTS, deliberately separated (see the zoom controller):
//!
//!   * the STRETCH effect follows the `scale` prop (wired to
//!     `viewer.zoom.display`, the live VISUAL scale). It only resizes the host
//!     so the bitmap we already have CSS-stretches to the new size; it runs
//!     every frame of a zoom and never renders.
//!   * the RENDER effect follows `viewer.zoom.committed` and is suspended
//!     while a zoom transition is in flight (`zoom_animating`). It produces
//!     the one crisp rasterisation at the end of a gesture.
//!
//! Keeping these in ONE effect was the ghost/double-image bug: a scale change
//! resized the host and kicked off a render in the same run, so every
//! intermediate frame cancelled and restarted a rasterisation, and the
//! half-drawn results flashed.

use std::cell::Cell;
use std::rc::Rc;

use leptos::html;
use leptos::prelude::*;

use super::canvas_host::{
    Completion, LastGeo, host_element, judge_completion, remove_snapshots, stretch_host,
};
use app_state::dom_contract::{HOST_PDF, TEXT_LAYER_CLASS};
use leptos::task::spawn_local;
use pdf_core::pixel_grid::snap_px;

use leptos::prelude::Signal;
use reader_core::appearance::TextureMode;

/// The gloss overlay inputs a page host renders when the document carries
/// highlights: the reader's state (which the host turns into the stroke
/// layer's resolver, since only the host knows its own page and id), the
/// persisted marks, the id of the mark currently waiting on the model so its
/// stroke can wear the processing animation, and the shared multi-select
/// state. ONE optional prop, so the inputs cannot arrive half-configured.
pub struct GlossOverlayProps {
    /// The reader's state: what the stroke layer's resolver and refresh
    /// fingerprint are built from, here rather than at the call site because
    /// the resolver needs this host's page number and element id.
    pub state: crate::state::ReaderState,
    /// The document's persisted gloss marks.
    pub marks: Signal<Vec<ai_core::gloss::GlossMark>>,
    /// Id of the gloss mark currently waiting on the model.
    pub processing: Signal<Option<String>>,
    /// Shared gloss multi-select mode.
    pub selecting: RwSignal<bool>,
    /// Shared ids selected in gloss multi-select mode.
    pub selected: RwSignal<std::collections::HashSet<String>>,
}

impl GlossOverlayProps {
    /// The reader's shared gloss state as a page host's overlay inputs —
    /// the only construction the reader's page views need.
    pub fn from_gloss(state: crate::state::ReaderState) -> Self {
        Self {
            state,
            marks: state.gloss.marks.read_only().into(),
            processing: state.gloss.processing_id.read_only().into(),
            selecting: state.gloss.selection_active,
            selected: state.gloss.selected_marks,
        }
    }

    /// Where this host's marks sit right now, in the host's own coordinates.
    pub fn resolver(
        &self,
        page: u32,
        host_id: &str,
    ) -> Callback<(ai_core::gloss::GlossMark, f64), Option<ai_core::gloss::GlossBox>> {
        crate::components::ai::anchor::stroke_resolver(
            self.state,
            Some(page),
            Some(host_id.to_string()),
        )
    }

    /// What makes this host's stroke layer look again.
    pub fn refresh(&self) -> Signal<u64> {
        crate::components::ai::anchor::layer_refresh(self.state)
    }
}

/// Register this page with its session, handing over its OWN canvas and host
/// so the engine is pinned to them: the ids stay (they key the page in the
/// session's registry), but two panes showing the same mode carry the same
/// ids, and a document-wide lookup would answer with the first pane's. No
/// canvas (the component is already gone): nothing to register.
fn register_mounted(
    pdf: &crate::pane::engine::PdfPane,
    page: u32,
    canvas_id: &str,
    host_id: &str,
    canvas: NodeRef<html::Canvas>,
    host: NodeRef<html::Div>,
) {
    let Some(canvas) = canvas
        .try_get_untracked()
        .flatten()
        .map(web_sys::Element::from)
    else {
        return;
    };
    let host = host_element(host);
    pdf.register_page(
        page,
        canvas_id,
        Some(host_id),
        pdf_engine::PageElements {
            canvas: &canvas,
            host: host.as_ref(),
        },
    );
}

#[component]
pub fn PdfPageCanvas(
    /// 1-based page number this host renders.
    page: u32,
    /// Render scale (CSS px per PDF unit).
    scale: ReadSignal<f64>,
    /// Unique id for the <canvas> element.
    canvas_id: String,
    /// Unique id for the .pdf-page host element.
    host_id: String,
    /// Whether to build a text layer for selection + search highlights.
    render_text: bool,
    /// Extra classes (e.g. absolute positioning in the continuous layout, or
    /// the cross-axis `mx-auto`/`my-auto` that centres a page and degrades to
    /// start-alignment on overflow).
    #[prop(default = String::new(), into)]
    class: String,
    /// Called with (page, width, height) CSS px after each successful render.
    #[prop(optional)]
    on_geometry: Option<Callback<(u32, f64, f64)>>,
    /// Called with (page, width, height) at SCALE 1 after each successful
    /// render: the page's true size, which the open could not afford to ask
    /// for up front. The fit modes measure against it.
    #[prop(optional)]
    on_rendered: Option<Callback<(u32, f64, f64)>>,
    /// Called with a page's true (scale-1) size as soon as the engine can
    /// state it — BEFORE this host's first raster — and answers whether the
    /// fit is moving because of it. `true` means the caller must not
    /// rasterise at the scale on screen: a refit is on its way and the
    /// commit's render paints this page once, at the size it belongs at.
    /// `None` (the default) skips the probe: hosts that are not a PDF page
    /// strip have no fit to move.
    #[prop(optional)]
    on_sized: Option<Callback<(u32, f64, f64), bool>>,
    /// The render scale (crisp target). The RENDER effect renders at this;
    /// the `scale` prop is the DISPLAY scale.
    #[prop(into)]
    render_scale: Signal<f64>,
    /// True while a zoom/layout animation is in flight (renders suspended).
    #[prop(into)]
    zoom_animating: Signal<bool>,
    /// True while this page is a RETAINED ZOMBIE — freshly evicted from the
    /// virtualization window and briefly kept mounted as a visual bridge.
    /// A zombie keeps its DOM and its last bitmap; it must not start a new
    /// rasterisation for the few frames it has left, so the render effect
    /// stands down entirely. Absent for hosts outside a virtualized strip.
    #[prop(optional)]
    dormant: Option<Signal<bool, LocalStorage>>,
    /// Whether the strip's scroll has SETTLED — the virtualizer's scroll-end
    /// window, published as a signal. While it reads false, an UNPAINTED page
    /// that is not in view stays blank instead of starting a full-resolution
    /// rasterisation: the fling gate. `None` (the default) means nothing to
    /// wait for — hosts outside a virtualized strip (single, spread) mount a
    /// page or two and sweep nothing past.
    #[prop(optional)]
    settled: Option<Signal<bool>>,
    /// Whether the page's box is inside (or near) the scroller's visible
    /// window, derived from the virtualizer's own model by the strip; page
    /// modes leave it `None`. The fling gate EXEMPTS a visible page: it
    /// rasterises even while the strip still moves — a page the reader is
    /// looking at must never sit blank — while
    /// overscan pages a fling sweeps past keep waiting for the settle. The
    /// read is tracked, so the crossing itself re-runs the render effect.
    #[prop(optional)]
    in_view: Option<Signal<bool, LocalStorage>>,
    /// Where this page belongs in the engine's page lane right now: lower
    /// starts first, `None` (the default) means the front of it. The strip
    /// derives it from the virtualizer's band, and it is read UNTRACKED at the
    /// moment a raster is issued — a rank change says who goes first, not what
    /// has to be drawn, so tracking it would restart renders for nothing.
    #[prop(optional)]
    rank: Option<Signal<u32, LocalStorage>>,
    /// True while a real zoom *gesture* owns the layout. Distinct from
    /// `zoom_animating`, which every resize-driven animation also holds — a
    /// fit slide, a window drag carrying a hand-picked zoom — for the whole
    /// burst of container sizes. Those follow the window; they are not a
    /// gesture, and rendering at their mid-burst display scale would produce a
    /// bitmap the next frame has already superseded.
    #[prop(into)]
    gesture_owns: Signal<bool>,
    /// The page's texture mode: the pane realm's answer for THIS pane, from
    /// the look the host routed to it (see `state::TextureSignal`).
    #[prop(into)]
    texture: Signal<TextureMode>,
    /// The gloss overlay: persisted marks, the processing id, and the shared
    /// multi-select state. `None` (the default) renders no layer at all, so
    /// this host stays usable outside the reader — one optional prop instead
    /// of four that only make sense together.
    #[prop(optional)]
    gloss_overlay: Option<GlossOverlayProps>,
) -> impl IntoView {
    // Texture is a prop, not context: this component is reusable without an
    // ambient provider. A Memo so only a real texture change rebuilds the
    // host class.
    let texture = Memo::new(move |_| texture.get());
    // Nothing transient rides the host class: the page's inline size is never
    // off the table for the flex engine, so there is no guard tag to toggle and
    // the memo stays keyed on the one input that changes it.
    let host_class = move || {
        let base = match texture.get().css_class() {
            Some(tex_class) => format!("pdf-page {tex_class}"),
            None => "pdf-page".to_string(),
        };
        if class.is_empty() {
            base
        } else {
            format!("{base} {class}")
        }
    };

    // The PDF session this page belongs to: the pane's session at mount
    // (see `MountedPdf`). Every engine call below goes through it, so a page
    // mounted for one document never reaches another.
    let mounted = crate::pane::engine::MountedPdf::bind();
    let pdf = move || mounted.pdf();

    let registered = Rc::new(Cell::new(false));
    // PAINTED FLAG. True after a successful render; false after a
    // cancelled/error render (which leaves the canvas wiped by pdf.js's
    // `canvas.width = ...` at render start). The no-op fast path below
    // requires `painted`, so a wiped canvas always re-renders even at
    // unchanged scale — otherwise a page cancelled mid-flight during a
    // sidebar slide (remount race) would sit blank until a scroll
    // re-triggered the effect.
    let painted = Rc::new(Cell::new(false));
    // Whether this host has asked the engine for the page's true size. One
    // probe per mounted host: the answer is cached engine-side, and the
    // fit maths only needs telling once.
    let sized = Rc::new(Cell::new(false));

    // The component's own two elements, by reference: the stretch, a
    // render's landing and the cover sweep act on THIS host — never on
    // whatever element in the document answers to its id, which a second
    // pane's page host carries too. The engine gets the same two elements
    // at registration (`register_mounted`); the ids stay as the page's key
    // in its session's registry and in the DOM contract.
    let host_ref: NodeRef<html::Div> = NodeRef::new();
    let canvas_ref: NodeRef<html::Canvas> = NodeRef::new();

    // Owned clones for the side-effect closures so the originals stay for view!.
    let cid = canvas_id.clone();
    let cid_effect = canvas_id.clone();
    let hid_effect = host_id.clone();
    // Raised the moment this owner dies. The boot microtask below is NOT
    // owner-bound — `queue_microtask` runs whatever was queued even after the
    // component unmounted — so without this flag a fast fling remount could
    // land the microtask AFTER the cleanup's unregister and re-register a
    // dead canvas, leaving the engine a PageState nothing ever drops again.
    // A StoredValue, not an Rc<Cell<_>>: the cleanup closure must be
    // Send + Sync, and the slot doubles as the truth — a handle whose arena
    // item is already gone reads None, which is a dead owner by definition.
    let disposed = StoredValue::new_local(false);
    on_cleanup(move || {
        let _ = disposed.try_set_value(true);
        pdf().unregister_page(&cid);
    });

    // Register after this view is flushed to the DOM: the registration hands
    // the engine the canvas element itself, which exists only from then.
    let gloss_host_id = host_id.clone();
    let cid_boot = canvas_id.clone();
    let hid_boot = host_id.clone();
    let registered_boot = registered.clone();
    let disposed_boot = disposed;
    queue_microtask(move || {
        // The owner died between the mount and this microtask: its cleanup
        // has already told the engine to forget this canvas, and registering
        // now would revive an entry no future unregister targets. A None
        // reads as disposed too — the arena item went with the owner.
        if disposed_boot.try_get_value().unwrap_or(true) {
            return;
        }
        debug_assert!(
            canvas_ref.try_get_untracked().flatten().is_some(),
            "PdfPageCanvas canvas must be in the DOM before register_page"
        );
        if !registered_boot.get() {
            register_mounted(&pdf(), page, &cid_boot, &hid_boot, canvas_ref, host_ref);
            registered_boot.set(true);
        }
    });

    // Last successfully rendered host geometry (CSS-px width, height, scale).
    // Guard 1 (stretch-resize) reads it to size the host before a re-render
    // lands; only written on a successful render.
    let geo = StoredValue::new_local((0.0f64, 0.0f64, 0.0f64));
    // Monotonic render generation: only the latest render may apply geometry
    // or fire the callback, so a stale in-flight render (one whose cancel the
    // engine missed) cannot overwrite a newer host size — which would re-add
    // the size jump this unit removes.
    let render_seq = StoredValue::new_local(0u32);
    // Forces the render effect to run again (a landing that left a stale,
    // stretched bitmap). Owned by this component; dropped with it.
    let rerender = Trigger::new();

    // `scale` is the DISPLAY scale (stretch target); `render_scale` the crisp
    // render target; `zoom_animating` suspends renders mid-gesture. All three
    // are explicit props so the component has no hidden ambient dependency.

    // Follows `display_scale`. Pure CSS: resize the host so the EXISTING bitmap
    // scales with the layout. Never renders — that is the whole point of the
    // split.
    Effect::new(move || {
        let s = scale.get();
        if s <= 0.0 {
            return;
        }
        let (lw, lh, ls) = geo.get_value();
        // Nothing rendered yet => nothing to stretch; the render effect owns
        // the first paint.
        if lw <= 0.0 || lh <= 0.0 || ls <= 0.0 || (ls - s).abs() <= 1e-9 {
            return;
        }
        stretch_host(
            host_ref,
            LastGeo {
                w: lw,
                h: lh,
                scale: ls,
            },
            s,
        );
    });

    Effect::new(move || {
        // Read every dependency unconditionally: a Leptos effect only
        // subscribes to what it READS during a run, so a conditional read would
        // silently drop the subscription the first time the branch was skipped.
        let anim = zoom_animating.get();
        let s_render = render_scale.get();
        // A zombie never starts a new render: its bitmap stays (the stretch
        // effect resized the host at the commit) and the page unmounts when
        // its retention grace expires. Rendering here would rasterise a page
        // on its way out.
        if dormant.as_ref().is_some_and(|d| d.get()) {
            return;
        }
        if s_render <= 0.0 {
            return;
        }
        let (gw, gh, gs) = geo.get_value();
        let has_geo = gw > 0.0 && gh > 0.0 && gs > 0.0;
        // A zoom or sidebar slide is in flight: stay out of the way entirely.
        //
        // With a bitmap, rendering here would relayout mid-animation — the
        // teleport/flicker this design removes. WITHOUT one (a page that just
        // scrolled into the window, constant during a slide because the
        // shrinking scale fits more pages), the render would be at an already
        // obsolete scale: it resolves, reports geometry, and is superseded by
        // the commit pass — measured at 3 of 11 renders on one sidebar toggle,
        // whose only visible effect was a page popping in at the wrong size.
        // The page stays blank for the gap; the commit pass (~120ms later)
        // renders once, correctly.
        //
        // FIRST PAINT. A page with NO bitmap (`!has_geo`) would sit blank
        // for the whole slide plus the commit, so it falls through to the
        // render path below, but at the DISPLAY scale — read untracked so the effect does
        // NOT re-run every frame of the slide. `on_geometry` ignores writes
        // while a transition is in flight and the commit re-renders crisply at
        // the settled scale; the stretch effect keeps tracking `display_scale`
        // afterwards, so the first-paint bitmap CSS-stretches with the
        // slide.
        if anim {
            if has_geo {
                return; // stretch effect owns it
            }
            // A sidebar slide (fit-driven) is NOT a zoom gesture: the display
            // scale is still moving and the commit renders once at the settled
            // scale ~480ms later. Rendering here produces a bitmap at an
            // obsolete scale — 2-3 wasted full-size RGBA bitmaps per toggle.
            // Only a REAL zoom gesture (which owns the layout) gets a live
            // first render at the display scale.
            //
            // But a mode flip starts a fit animation as the new view's pages
            // mount: if an UN-PAINTED page bailed here and the commit landed
            // on an unchanged scale, nothing would re-trigger this effect and
            // the page would stay blank until a remount. Gate only pages that
            // already have pixels.
            if !gesture_owns.get_untracked() && painted.get() {
                return;
            }
            // FALLTHROUGH: first render at the DISPLAY scale so the gesture
            // has pixels. Read untracked to avoid subscribing to per-frame
            // display_scale changes (which would re-run this effect every
            // frame of the zoom — a render storm).
            // The `s` used below is `s_render` by default; override it for
            // this fallthrough path.
        }
        // Use the display scale for the cold-cache first paint during an
        // animation; otherwise use the render scale (the crisp target).
        let s = if anim {
            scale.get_untracked()
        } else {
            s_render
        };
        if s <= 0.0 {
            return;
        }
        // NO-OP FAST PATH. If the page has already been rendered at
        // THIS scale AND the canvas still has its bitmap (`painted == true`),
        // re-rendering would only WIPE the live canvas (pdf.js reassigns
        // `canvas.width/height` on render start) without producing a different
        // bitmap. Because `(gs - s).abs() <= 1e-9`, the `stretch_host` guard
        // below is skipped too — nothing resizes or covers the host — and the
        // user sees the canvas disappear until a scroll re-renders it. Bail
        // out — but ONLY if `painted == true`. A wiped canvas (cancelled
        // render) must re-render.
        if has_geo && painted.get() && (gs - s).abs() <= 1e-9 {
            return;
        }
        // SCROLL-FLING GATE. An unpainted page the scroller is sweeping past
        // at speed stays blank until it is inside the band or the strip
        // settles: rasterising every page a fling flies past creates and
        // discards a full-page surface every few frames. `in_view` (tracked) is
        // the band's own verdict, and it is the whole rule — a page the reader
        // can see at reading speed renders in the frame it mounts, because the
        // band IS the mount window at that speed, so there is no dwell timer
        // here to be wrong about. `settled` (tracked) paints what the fling
        // left behind once the strip stops, which is the guaranteed wake.
        let visible_now = in_view.as_ref().is_none_or(|v| v.get());
        // Read every time so a forced re-render (below) re-runs this effect.
        rerender.track();
        // `!anim`: never give a slot back in the middle of a zoom transaction. The
        // commit judges its landing by the bitmap this page would hand up (see the
        // fallthrough warning above), so cancelling it there buys a settle and
        // costs the commit — the slot is worth less than the page it holds.
        if !anim && !painted.get() && !visible_now && settled.as_ref().is_some_and(|s| !s.get()) {
            // This page may already hold a lane slot from the frame it was in
            // view. Give it back: a raster for a page outside the band paints
            // nobody, and the lane has two slots for the pages in front of the
            // reader. Coming back into the band re-runs this effect (the
            // tracked read above), so the work is re-issued, never lost.
            pdf().cancel_page(&cid_effect);
            return;
        }
        let page_no = page;
        let cid = cid_effect.clone();
        let hid = hid_effect.clone();
        let rt = render_text;
        let cb = on_geometry;
        let rendered_cb = on_rendered;
        let do_register = registered.clone();
        let geo_async = geo;
        let seq_async = render_seq;
        let painted_async = painted.clone();
        let sized_async = sized.clone();
        let sized_cb = on_sized;

        // This effect run owns the next generation; older completions are stale.
        let my_seq = seq_async.get_value() + 1;
        seq_async.set_value(my_seq);

        // Size the host to the incoming scale for renders the stretch effect
        // did NOT precede — the search nudge, or a fit refit that lands
        // straight on render_scale. The current bitmap stretches into it and
        // stays on screen until the render lands: the engine rasterises into
        // a scratch and replaces the visible bitmap in one blit, so there is
        // no wipe to cover.
        let (lw, lh, ls) = geo.get_value();
        if lw > 0.0 && lh > 0.0 && ls > 0.0 && (ls - s).abs() > 1e-9 {
            stretch_host(
                host_ref,
                LastGeo {
                    w: lw,
                    h: lh,
                    scale: ls,
                },
                s,
            );
        }
        // Whether this run is the cold first-paint stop-gap issued INSIDE a
        // transaction (see the `anim` branch above): its completion is judged
        // differently from a render for the committed scale.
        let issued_mid_zoom = anim;
        let zooming_at_landing = zoom_animating;
        let committed_at_landing = render_scale;
        let display_at_landing = scale;

        let rerender_async = rerender;

        // The session view this render runs through, captured BEFORE the
        // await: the render, the registration it may need and the paper
        // frame drain all reach the same session.
        let pdf_async = pdf();
        spawn_local(async move {
            // A render can outlive its component: `<For>` unmounts a page that
            // scrolled out of the window while its rasterisation is still in
            // flight. Once the reactive owner is disposed, TOUCHING a
            // StoredValue panics, so every post-await access on one goes
            // through `try_get_value` (the `seq`/`painted` handles below).
            // This flag is deliberately NOT one of those: it is a plain
            // `Cell`, so it has no owner to outlive and a late read cannot
            // abort the wasm. Nothing needs doing when the cell unmounted —
            // the element is gone and the engine already dropped the
            // registration.
            if !do_register.get() {
                register_mounted(&pdf_async, page_no, &cid, &hid, canvas_ref, host_ref);
                do_register.set(true);
            }
            // SIZE BEFORE PIXELS. The fit maths measures a page by the box
            // it holds for it, and the open seeds every page with page 1's —
            // so a book whose plates differ from its letter pages rasterises
            // them at a fit that belongs to another page, shows them at that
            // size for the whole raster, and shrinks them only when the refit
            // that follows lands. Asking the engine for the page's true box
            // first (one worker round trip, no pixels) lets that refit move
            // BEFORE the raster, so the page is painted once and correctly.
            //
            // A `true` answer means the fit is moving: hold this raster. The
            // commit re-runs this effect at the settled scale — which is the
            // page's own fit, so the very first bitmap for it is already the
            // size it belongs at, and there is no second paint to snap.
            if !sized_async.get() {
                sized_async.set(true);
                if let Ok(size) = pdf_async.probe_page_size(page_no).await {
                    // A probe can outlive the host that asked for it: a rapid
                    // reopen disposes this page — and its owner's arena — while
                    // the answer is in flight, and a `Callback` IS an arena
                    // handle, so asking one anything afterwards panics the
                    // wasm. Same question, same handle as the landing guard
                    // below; a dead host owes nothing, not even the raster it
                    // was about to ask for.
                    if seq_async.try_get_value().is_none() {
                        return;
                    }
                    if let Some(cb) = sized_cb
                        && cb.run((page_no, size.width, size.height))
                    {
                        return;
                    }
                }
            }
            // A render whose session died resolves `no_session` (the engine
            // refuses retired sids, and the session re-checks itself after
            // the await), so nothing below commits into a replaced document.
            // The rank signal lives in this host's own arena, and an unmount
            // between the probe above and here leaves no band to ask — a derived
            // read would panic on the disposed value, which is not a thing a
            // raster is worth. Rank 0 is request order, the answer a host with
            // no band left gives.
            let rank_now = rank
                .as_ref()
                .and_then(|r| r.try_get_untracked())
                .unwrap_or(0);
            match pdf_async.render_page(&cid, s, rt, rank_now).await {
                Ok(r) => {
                    // Unmounted mid-render: the owner's signals are gone, and
                    // there is no host left to size.
                    let (Some(latest), Some(zooming_now), Some(committed_now)) = (
                        seq_async.try_get_value().map(|seq| seq == my_seq),
                        zooming_at_landing.try_get_untracked(),
                        committed_at_landing.try_get_untracked(),
                    ) else {
                        return;
                    };
                    match judge_completion(latest, issued_mid_zoom, zooming_now, committed_now, s) {
                        Completion::Apply => {}
                        // A newer render owns the host and its geometry.
                        Completion::Superseded => return,
                        // The engine already blitted this bitmap into the
                        // canvas, so what the canvas holds IS this render:
                        // record that (`geo` is the stretch's base — without
                        // it the host could never follow a later zoom), and
                        // size the host to the scale on screen NOW, as the
                        // stretch effect would. What stays out is the size
                        // at the RENDERED scale — written to the host it
                        // would snap it back to a zoom state that no longer
                        // holds — and the report to the strip, which belongs
                        // to that state too. If the committed scale has moved
                        // on, the render effect's fast path sees `geo` at the
                        // old scale and re-renders.
                        Completion::Stale => {
                            geo_async.try_set_value((r.width, r.height, s));
                            painted_async.set(true);
                            // The bitmap is at a scale that no longer holds:
                            // never leave it stretched (blurry) — ask for the
                            // crisp render now instead of waiting for some
                            // later dependency change that may never come.
                            if !zooming_now && (committed_now - s).abs() > 1e-9 {
                                rerender_async.notify();
                            }
                            if let Some(display) = display_at_landing.try_get_untracked() {
                                stretch_host(
                                    host_ref,
                                    LastGeo {
                                        w: r.width,
                                        h: r.height,
                                        scale: s,
                                    },
                                    display,
                                );
                            }
                            return;
                        }
                    }
                    // Successful render: the canvas now has a bitmap.
                    painted_async.set(true);
                    // Snap the rendered size to the device-pixel grid before
                    // it becomes CSS. `r.width`/`r.height` are whole CSS px,
                    // which is only whole DEVICE px when the ratio is an
                    // integer; at 125% / 150% display scaling the page's
                    // layer rect rounds independently of its neighbour's and
                    // the joint shows a hairline of the backdrop.
                    let (sw, sh) = (snap_px(r.width), snap_px(r.height));
                    // The engine stashed this render's raw frame (the one
                    // pipeline moment the page's own paper is unbaked); hand
                    // it to the session's paper state machine — every colour
                    // decision it feeds lives in the pdf-paper crate.
                    pdf_async.paper_live_frame(&cid);
                    if let Some(host) = host_element(host_ref) {
                        // Note: cannot use host.style() (tachys ElementExt::style shadows
                        // web_sys' inherent method); set the inline style attribute directly.
                        // The engine also sets `--scale-factor` inline on the host during
                        // render; this FULL attribute replace must carry it forward or the
                        // text layer's custom-property math (font-size + setLayerDimensions
                        // container sizing) recomputes at scale 1 and misaligns selection.
                        let _ = host.set_attribute(
                            "style",
                            &format!("width:{sw}px;height:{sh}px;--scale-factor:{s}"),
                        );
                        // New bitmap is live — drop an appearance-scrub cover
                        // (`.page-snapshot`, theme/scrub.ts) in the same flush.
                        remove_snapshots(&host);
                    }
                    // The geometry cache keeps the RAW size: it only ever
                    // feeds the stretch ratio, where an unrounded base keeps
                    // successive zoom steps from drifting. What leaves this
                    // component — the host's CSS box and the size the
                    // virtualizer models the strip with — is the snapped one,
                    // so painted pixels and computed offsets agree.
                    geo_async.try_set_value((r.width, r.height, s));
                    // Before the geometry report: that report lifts the
                    // first-paint gate, and a refit for a resume page whose
                    // size differs from page 1 belongs in the same beat.
                    if let Some(rendered) = rendered_cb.filter(|_| s > 0.0) {
                        rendered.run((page_no, r.width / s, r.height / s));
                    }
                    if let Some(cb) = cb {
                        cb.run((page_no, sw, sh));
                    }
                }
                Err(e) => {
                    // A stale or orphaned completion must not touch the host
                    // or its cover.
                    if seq_async.try_get_value() != Some(my_seq) {
                        return;
                    }
                    // Cancelled / transient errors are logged, not fatal;
                    // never leave a scrub cover behind. A cancelled scratch
                    // render leaves the old bitmap in place, but a scrub-time
                    // render draws in place and can leave the canvas wiped,
                    // so mark it NOT painted: the no-op fast path must not
                    // skip the re-render.
                    painted_async.set(false);
                    if let Some(host) = host_element(host_ref) {
                        remove_snapshots(&host);
                    }
                    web_sys::console::warn_1(
                        &format!("[page_canvas] render page {page_no}: {e}").into(),
                    );
                }
            }
        });
    });

    view! {
        <div
            node_ref=host_ref
            id=host_id
            class=host_class
            data-reader-host=HOST_PDF
            data-host-page=page
        >
            // `data-engine-sid`: whose canvas this is, for the engine's one
            // lookup that may run before the page registered — another
            // pane's page has this id too.
            <canvas node_ref=canvas_ref id=canvas_id data-engine-sid=mounted.sid() />
            // Placeholder text layer. The engine REPLACES this node on each
            // text render: it builds the spans in a detached `.textLayer` and
            // swaps it in atomically, so a superseded render's late spans can
            // never land on top of the current ones (that overlap was the
            // doubled text visible when selecting). Leptos does not own the
            // node's contents, so the swap is safe — but keep the class name
            // and position (immediately after the canvas) in sync with
            // `renderPageInternal` in public/engine/renderer.ts.
            <div class=TEXT_LAYER_CLASS aria-hidden="true"></div>
            // Persisted gloss highlights. Rendered by Leptos INSIDE the host,
            // so every remount repaints them from the page-space rects — the
            // reason a mark survives scrolling, zooming and reopening the book.
            {gloss_overlay
                .map(|gloss| {
                    let resolve = gloss.resolver(page, &gloss_host_id);
                    let refresh = gloss.refresh();
                    view! {
                        <crate::components::ai::gloss::mark_layer::GlossMarkLayer
                            page=Some(page)
                            marks=gloss.marks
                            resolve=resolve
                            refresh=refresh
                            scale=scale
                            processing=gloss.processing
                            selecting=gloss.selecting
                            selected=gloss.selected
                        />
                    }
                })}
        </div>
    }
}

// only the changed file was rewritten
