//! Opening a reflowable document: the PDF open's shape, reading text.

use std::sync::Arc;

use leptos::prelude::*;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::spawn_local;

use md_core::MarkdownHeading;
use reader_core::document::PageSize;
use reader_core::filename::document_title;
use reader_core::format::Format;
use reader_core::view::ViewMode;
use reflow_core::block::TextBlock;
use reflow_core::geometry::{PAGE_HEIGHT, geometry};
use reflow_core::pager::estimate_heights;

use crate::state::document::reflow::estimate_metrics;
use runtime_contract::boundary::ShellApi;

use crate::pane::session::{FormatSession, MdSession, TxtSession};

/// What a format contributes: blocks, a title, and its headings.
struct Parsed {
    blocks: Vec<TextBlock>,
    title: Option<String>,
    /// The author line, when the format has one (front matter).
    author: Option<String>,
    headings: Vec<MarkdownHeading>,
}

/// The document's bytes as text, through the shell's gated read.
async fn read_file_text(path: &str) -> Result<String, String> {
    if !tauri_bridge::has_tauri() {
        if path.starts_with("/samples/") {
            return fetch_sample_text(path).await;
        }
        return Err(
            "Opening files is only available in the desktop app. Drag and drop runs through \
             the shell too."
                .to_string(),
        );
    }
    let args = js_sys::Object::new();
    _ = js_sys::Reflect::set(&args, &"path".into(), &JsValue::from_str(path));
    let value = tauri_bridge::invoke("read_file_text", args.into())
        .await
        .map_err(|e| e.as_string().unwrap_or_else(|| format!("{e:?}")))?;
    value
        .as_string()
        .ok_or_else(|| "read_file_text returned no text".to_string())
}

/// A bundled sample's text; a missing sample must fail as missing.
async fn fetch_sample_text(path: &str) -> Result<String, String> {
    use wasm_bindgen::JsCast;
    let describe = |e: JsValue| e.as_string().unwrap_or_else(|| format!("{e:?}"));
    let method = |target: &JsValue, name: &str| -> Result<js_sys::Function, String> {
        js_sys::Reflect::get(target, &JsValue::from_str(name))
            .map_err(describe)?
            .dyn_into::<js_sys::Function>()
            .map_err(|_| format!("no {name}()"))
    };
    let window: JsValue = web_sys::window().ok_or("no window")?.into();
    let request = method(&window, "fetch")?
        .call1(&window, &JsValue::from_str(path))
        .map_err(describe)?;
    let response = wasm_bindgen_futures::JsFuture::from(js_sys::Promise::from(request))
        .await
        .map_err(describe)?;
    let ok = js_sys::Reflect::get(&response, &JsValue::from_str("ok"))
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let headers =
        js_sys::Reflect::get(&response, &JsValue::from_str("headers")).map_err(describe)?;
    let kind = method(&headers, "get")?
        .call1(&headers, &JsValue::from_str("content-type"))
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_default();
    if !ok || kind.starts_with("text/html") {
        return Err(format!("{path}: no such sample"));
    }
    let text = method(&response, "text")?
        .call0(&response)
        .map_err(describe)?;
    wasm_bindgen_futures::JsFuture::from(js_sys::Promise::from(text))
        .await
        .map_err(describe)?
        .as_string()
        .ok_or_else(|| format!("{path}: not text"))
}

/// Shared flow for the reflowable formats: replace, read, parse,
/// populate.
pub(super) fn open_reflowable(
    state: crate::context::ReaderContext,
    path: String,
    format: Format,
    saved_page: u32,
    saved_fraction: Option<f64>,
    stamp: u64,
) {
    if !state.pane.admits_work() {
        return;
    }
    // The session replaces the pane's owner before the read; the old one
    // goes first.
    let reflow = state.reader.document.content.reflow;
    let session = match format {
        Format::Markdown => FormatSession::Markdown(MdSession::new(&path, reflow)),
        _ => FormatSession::Text(TxtSession::new(&path, reflow)),
    };
    let release = state.pane.replace_document(session);
    #[cfg(feature = "pdf")]
    pdf_engine::session::drop_retained_search();
    spawn_local(async move {
        // THIS pane's previous document is released first.
        release.settled().await;
        if !state.pane.owns_generation(stamp) {
            return;
        }
        let raw = match read_file_text(&path).await {
            Ok(raw) => raw,
            Err(message) => {
                if state.pane.owns_generation(stamp) {
                    super::abandon(state, message);
                }
                return;
            }
        };
        // The read finished; a second open may have taken over.
        if !state.pane.owns_generation(stamp) {
            return;
        }
        let parsed = parse(format, &raw);
        // The blocks own their text; the bytes are dropped.
        drop(raw);
        if parsed.blocks.is_empty() {
            super::abandon(state, "This file has no readable text.".to_string());
            return;
        }
        // Everything below is synchronous, so no tail outlives its stamp.
        ready(state, path, format, parsed, saved_page, saved_fraction);
    });
}

/// The format's own step: normalise, parse, then split oversized
/// blocks.
fn parse(format: Format, raw: &str) -> Parsed {
    match format {
        Format::Markdown => {
            let blocks = md_core::subdivide_prose(md_core::parse_markdown(raw));
            // Keyed on the SUBDIVIDED blocks: a split shifts every index after
            // it.
            let headings = md_core::headings_of_blocks(&blocks);
            Parsed {
                blocks,
                title: md_core::document_title(raw),
                author: md_core::document_author(raw),
                headings,
            }
        }
        // Anything else reflowable is read as text, the same answer
        // `format_of` gave at the door.
        _ => Parsed {
            blocks: txt_core::subdivide_paragraphs(txt_core::parse_plain_text(raw)),
            title: None,
            author: None,
            headings: Vec::new(),
        },
    }
}

/// The document read and parsed: seed the state and flip the route.
fn ready(
    state: crate::context::ReaderContext,
    path: String,
    format: Format,
    parsed: Parsed,
    saved_page: u32,
    saved_fraction: Option<f64>,
) {
    let settings = state.settings.get_untracked();
    // The geometry the first cut is estimated against, from the same
    // dials the refine uses.
    let geo = geometry(settings.text.book_layout)
        .with_extra_inline(state.reader.viewer.page_margin.get_untracked())
        .with_column_pct(state.reader.viewer.column_width_pct.get_untracked());
    let name = document_title(parsed.title.as_deref());
    let Parsed {
        blocks,
        title,
        author,
        headings,
    } = parsed;

    // Document identity through the shared handshake; the outline starts
    // SEEDED.
    super::enter::identity(
        &state,
        super::enter::DocumentIdentity {
            format,
            path: path.clone(),
            title,
            author,
            page1_size: PageSize {
                width: geo.width,
                height: PAGE_HEIGHT,
            },
            outline: Some(Arc::new(Vec::new())),
        },
    );

    // The content is already empty; the reset guarantees no stale tail
    // survives.
    state.reader.document.content.reflow.reset();
    super::enter::load_marks(&state);

    // The content: blocks in, estimate cut out.
    let metrics = estimate_metrics(&settings.text, &geo);
    let heights = estimate_heights(&blocks, &metrics);
    state
        .reader
        .document
        .content
        .reflow
        .blocks
        .set(Arc::new(blocks));
    state
        .reader
        .document
        .content
        .reflow
        .headings
        .set(Arc::new(headings));

    // The seed scale, from the same shared step the PDF seed uses.
    let (startup_fit, scale) = super::enter::startup_scale(&state, (geo.width, PAGE_HEIGHT));

    // The reading position and zoom, seeded in the PDF open's order.
    let streaming = state.reader.viewer.mode.get_untracked() == ViewMode::ScrollVertical;
    state
        .reader
        .document
        .content
        .reflow
        .resume_fraction
        .set(if streaming { saved_fraction } else { None });
    state.reader.viewer.awaiting_anchor.set(true);
    state.reader.viewer.fit.set(startup_fit);
    state.reader.viewer.zoom.initialize(scale);
    state.reader.viewer.scroll_top.set(0.0);

    // The estimate's cut, before the resume page is chosen.
    let cut = state
        .reader
        .document
        .content
        .reflow
        .set_initial_heights(state.reader, heights, geo);
    state.reader.document.publish_cut(&cut);
    let resume = super::enter::resume_page(saved_page, cut.num_pages);
    state.reader.viewer.page.set(resume);

    super::enter::enter_ready(state);

    // The shelf record last, as for a PDF: no cover, no thumbnail
    // warmup.
    state
        .api
        .read_point(&runtime_contract::boundary::ReadPoint {
            book_id: state.reader.document.book_id.get_untracked(),
            path: path.clone(),
            page: resume,
            num_pages: cut.num_pages,
            fraction: saved_fraction,
            title: name,
            author: None,
        });
}
