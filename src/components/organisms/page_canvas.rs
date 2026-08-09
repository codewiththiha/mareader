//! The shared page host: a `.pdf-page` div containing a `<canvas>` and a
//! `.textLayer` div, driven by the JS engine. Used by BOTH view modes.
//!
//! Contract:
//!  - ids are Rust-chosen unique strings; the engine resolves elements by id.
//!  - renders (page, scale) via engine.renderPage / updatePage; reports the
//!    rendered CSS-px size through `on_geometry`.
//!  - registers on first render, unregisters (cancels) when disposed.

use std::cell::Cell;
use std::rc::Rc;

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::engine;
use crate::core::state::AppState;

#[component]
pub fn PageCanvas(
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
    /// Extra classes (e.g. absolute positioning in the continuous layout).
    #[prop(default = String::new())]
    class: String,
    /// Called with (page, width, height) CSS px after each successful render.
    #[prop(optional)]
    on_geometry: Option<Callback<(u32, f64, f64)>>,
) -> impl IntoView {
    let state = use_context::<AppState>();
    let texture = move || {
        state
            .as_ref()
            .map(|s| s.settings.get().texture.as_str().to_string())
            .unwrap_or_else(|| "none".to_string())
    };
    let host_class = move || {
        let t = texture();
        let base = if t == "none" {
            "pdf-page".to_string()
        } else {
            format!("pdf-page texture-{t}")
        };
        if class.is_empty() {
            base
        } else {
            format!("{base} {class}")
        }
    };

    let registered = Rc::new(Cell::new(false));

    // Owned clones for the side-effect closures so the originals stay for view!.
    let cid = canvas_id.clone();
    let cid_effect = canvas_id.clone();
    let hid_effect = host_id.clone();
    on_cleanup(move || engine::unregister_page(&cid));

    Effect::new(move || {
        let s = scale.get();
        if s <= 0.0 {
            return;
        }
        let page_no = page;
        let cid = cid_effect.clone();
        let hid = hid_effect.clone();
        let rt = render_text;
        let cb = on_geometry.clone();
        let do_register = registered.clone();
        spawn_local(async move {
            if !do_register.get() {
                engine::register_page(page_no, &cid, Some(&hid));
                do_register.set(true);
            }
            match engine::render_page(&cid, s, rt).await {
                Ok(r) => {
                    if let Some(host) = web_sys::window()
                        .and_then(|w| w.document())
                        .and_then(|d| d.get_element_by_id(&hid))
                    {
                        // Note: cannot use host.style() (tachys ElementExt::style shadows
                        // web_sys' inherent method); set the inline style attribute directly.
                        // The engine also sets `--scale-factor` inline on the host during
                        // render; this FULL attribute replace must carry it forward or the
                        // text layer's custom-property math (font-size + setLayerDimensions
                        // container sizing) recomputes at scale 1 and misaligns selection.
                        let _ = host.set_attribute(
                            "style",
                            &format!(
                                "width:{}px;height:{}px;--scale-factor:{}",
                                r.width, r.height, s
                            ),
                        );
                    }
                    if let Some(cb) = cb {
                        cb.run((page_no, r.width, r.height));
                    }
                }
                Err(e) => {
                    // cancelled / transient errors are logged, not fatal
                    web_sys::console::log_1(
                        &format!("[page_canvas] render page {page_no}: {e}").into(),
                    );
                }
            }
        });
    });

    view! {
        <div id=host_id class=host_class>
            <canvas id=canvas_id />
            <div class="textLayer" aria-hidden="true"></div>
        </div>
    }
}
