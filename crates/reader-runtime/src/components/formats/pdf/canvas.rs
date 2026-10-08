//! The shared page host: canvas and text layer per page, both modes.

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

/// The gloss overlay inputs a host renders: the reader state, marks,
/// processing id, selection.
pub struct GlossOverlayProps {
    /// The reader's state: the stroke layer's resolver and refresh come from
    /// it.
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
    /// The reader's shared gloss state as this host's overlay inputs.
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

/// Register this page's OWN canvas and host, so the engine is pinned to
/// them.
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
    /// Extra classes: positioning in the continuous layout, or the
    /// cross-axis centring.
    #[prop(default = String::new(), into)]
    class: String,
    /// Called with (page, width, height) CSS px after each successful render.
    #[prop(optional)]
    on_geometry: Option<Callback<(u32, f64, f64)>>,
    /// Called with (page, width, height) at SCALE 1: the page's true size.
    #[prop(optional)]
    on_rendered: Option<Callback<(u32, f64, f64)>>,
    /// Called with the page's true size before its first raster; `true`
    /// means a refit.
    #[prop(optional)]
    on_sized: Option<Callback<(u32, f64, f64), bool>>,
    /// The crisp render target; the `scale` prop is the display scale.
    #[prop(into)]
    render_scale: Signal<f64>,
    /// True while a zoom/layout animation is in flight (renders suspended).
    #[prop(into)]
    zoom_animating: Signal<bool>,
    /// True while a retained zombie: it keeps its bitmap and starts no
    /// raster.
    #[prop(optional)]
    dormant: Option<Signal<bool, LocalStorage>>,
    /// Whether the strip's scroll has settled. False keeps an unpainted page
    /// blank: the fling gate.
    #[prop(optional)]
    settled: Option<Signal<bool>>,
    /// Whether the page is in the visible band: visible pages rasterise even
    /// in a fling.
    #[prop(optional)]
    in_view: Option<Signal<bool, LocalStorage>>,
    /// Where this page sits in the engine's page lane, read untracked at
    /// issue time.
    #[prop(optional)]
    rank: Option<Signal<u32, LocalStorage>>,
    /// True while a zoom gesture owns the layout; resize animations do not
    /// count.
    #[prop(into)]
    gesture_owns: Signal<bool>,
    /// The page's texture mode for THIS pane; a prop, not context.
    #[prop(into)]
    texture: Signal<TextureMode>,
    /// The gloss overlay; `None` renders no layer, so the host works outside
    /// the reader.
    #[prop(optional)]
    gloss_overlay: Option<GlossOverlayProps>,
) -> impl IntoView {
    // Texture is a prop, not context: the component needs no provider.
    let texture = Memo::new(move |_| texture.get());
    // Nothing transient rides the host class: only the texture changes it.
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

    // The session this page belongs to, captured at mount.
    let mounted = crate::pane::engine::MountedPdf::bind();
    let pdf = move || mounted.pdf();

    let registered = Rc::new(Cell::new(false));
    // PAINTED: false after a cancelled render, so a wiped canvas always
    // re-renders.
    let painted = Rc::new(Cell::new(false));
    // Whether the page's true size was asked of the engine (once).
    let sized = Rc::new(Cell::new(false));

    // This host's own two elements by reference; the ids stay the page's
    // registry key.
    let host_ref: NodeRef<html::Div> = NodeRef::new();
    let canvas_ref: NodeRef<html::Canvas> = NodeRef::new();

    // Owned clones for the closures; the originals stay for view!.
    let cid = canvas_id.clone();
    let cid_effect = canvas_id.clone();
    let hid_effect = host_id.clone();
    // Raised when the owner dies: the boot microtask must not re-register a
    // dead canvas.
    let disposed = StoredValue::new_local(false);
    on_cleanup(move || {
        let _ = disposed.try_set_value(true);
        pdf().unregister_page(&cid);
    });

    // Register after the flush: the engine gets the canvas element itself.
    let gloss_host_id = host_id.clone();
    let cid_boot = canvas_id.clone();
    let hid_boot = host_id.clone();
    let registered_boot = registered.clone();
    let disposed_boot = disposed;
    queue_microtask(move || {
        // The owner died between mount and microtask; a `None` reads as
        // disposed.
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

    // Last successful render's CSS-px width, height and scale.
    let geo = StoredValue::new_local((0.0f64, 0.0f64, 0.0f64));
    // Monotonic render generation: only the latest render applies geometry
    // or callbacks.
    let render_seq = StoredValue::new_local(0u32);
    // Forces the render effect to run again after a stale landing.
    let rerender = Trigger::new();

    // `scale` is the display target; `render_scale` the crisp one.

    // Follows `scale`. Pure CSS: resize the host so the bitmap stretches;
    // never renders.
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
        // Read every dependency unconditionally: an effect subscribes only to
        // what it reads.
        let anim = zoom_animating.get();
        let s_render = render_scale.get();
        // A zombie never starts a render: its bitmap stays until it unmounts.
        if dormant.as_ref().is_some_and(|d| d.get()) {
            return;
        }
        if s_render <= 0.0 {
            return;
        }
        let (gw, gh, gs) = geo.get_value();
        let has_geo = gw > 0.0 && gh > 0.0 && gs > 0.0;
        // Mid-animation: stay out of the way, except an unpainted page's first
        // paint.
        if anim {
            if has_geo {
                return; // stretch effect owns it
            }
            // A sidebar slide is no zoom: a live render would land obsolete.
            if !gesture_owns.get_untracked() && painted.get() {
                return;
            }
            // FALLTHROUGH: first render at the display scale so the gesture has
            // pixels.
        }
        // Cold-cache first paint uses the display scale; else the crisp one.
        let s = if anim {
            scale.get_untracked()
        } else {
            s_render
        };
        if s <= 0.0 {
            return;
        }
        // NO-OP FAST PATH: same scale and still painted, so re-rendering would
        // only wipe the canvas.
        if has_geo && painted.get() && (gs - s).abs() <= 1e-9 {
            return;
        }
        // SCROLL-FLING GATE: `in_view` (tracked) is the whole rule; `settled`
        // wakes it.
        let visible_now = in_view.as_ref().is_none_or(|v| v.get());
        // Read every time so a forced re-render (below) re-runs this effect.
        rerender.track();
        if !painted.get() && !visible_now && settled.as_ref().is_some_and(|s| !s.get()) {
            // Give the slot back; re-entering the band re-runs this effect.
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

        // This run owns the next generation; older completions are stale.
        let my_seq = seq_async.get_value() + 1;
        seq_async.set_value(my_seq);

        // Size the host for a render the stretch effect did not precede.
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
        // Whether this run is the cold first-paint stop-gap inside a zoom.
        let issued_mid_zoom = anim;
        let zooming_at_landing = zoom_animating;
        let committed_at_landing = render_scale;
        let display_at_landing = scale;

        let rerender_async = rerender;

        // The session view this render runs through, captured before the await.
        let pdf_async = pdf();
        spawn_local(async move {
            // A render can outlive its component: post-await reads use `try_`
            // on `StoredValue`.
            if !do_register.get() {
                register_mounted(&pdf_async, page_no, &cid, &hid, canvas_ref, host_ref);
                do_register.set(true);
            }
            // SIZE BEFORE PIXELS: probe the true box before the raster.
            if !sized_async.get() {
                sized_async.set(true);
                if let Ok(size) = pdf_async.probe_page_size(page_no).await {
                    // A probe can outlive its host; a dead arena owes nothing.
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
            // A dead session's render resolves `no_session`, so nothing
            // commits into a replaced document.
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
                        // Stale: record `geo` and skip the stale size.
                        Completion::Stale => {
                            geo_async.try_set_value((r.width, r.height, s));
                            painted_async.set(true);
                            // Never leave it stretched; ask for the crisp one.
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
                    // Snap to the device-pixel grid: a fractional ratio's
                    // rounding shows a hairline.
                    let (sw, sh) = (snap_px(r.width), snap_px(r.height));
                    // Hand the raw frame to the session's paper machine.
                    pdf_async.paper_live_frame(&cid);
                    if let Some(host) = host_element(host_ref) {
                        // `host.style()` is shadowed by tachys; set the full
                        // attribute so `--scale-factor` carries forward.
                        let _ = host.set_attribute(
                            "style",
                            &format!("width:{sw}px;height:{sh}px;--scale-factor:{s}"),
                        );
                        // New bitmap is live — drop an appearance-scrub cover
                        // (`.page-snapshot`, theme/scrub.ts) in the same flush.
                        remove_snapshots(&host);
                    }
                    // The geometry cache keeps the RAW size; what leaves is the
                    // snapped one.
                    geo_async.try_set_value((r.width, r.height, s));
                    // Before the geometry report, which lifts the gate.
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
                    // Cancelled errors are logged, not fatal; a wipe
                    // marks the canvas unpainted.
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
            // `data-engine-sid`: whose canvas this is, for the early lookup.
            <canvas node_ref=canvas_ref id=canvas_id data-engine-sid=mounted.sid() />
            // Placeholder text layer: the engine swaps it in atomically,
            // so late spans cannot overlap.
            <div class=TEXT_LAYER_CLASS aria-hidden="true"></div>
            // Persisted gloss marks: Leptos repaints them per remount from page
            // rects.
            {gloss_overlay
                .map(|gloss| {
                    let resolve = gloss.resolver(page, &gloss_host_id);
                    let refresh = gloss.refresh();
                    let cefr_state = gloss.state;
                    let cefr_host = gloss_host_id.clone();
                    let cefr_scale = scale;
                    view! {
                        // Red under accent: the click's stroke takes over.
                        <crate::components::cefr::pdf::PdfCefrLayer
                            state=cefr_state
                            page=page
                            host_id=cefr_host
                            scale=cefr_scale
                        />
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
