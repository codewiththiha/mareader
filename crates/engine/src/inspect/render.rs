//! Headless page rasterization, base64 encoding, and event summary helpers.

use serde_json::{Value, json};

use super::resolve_pane;
use crate::event::Event;
use crate::session::Session;
use crate::toast::Tone;

/// Lower bound on headless page render scale.
pub const MIN_RENDER_SCALE: f32 = 0.25;
/// Upper bound on headless page render scale.
pub const MAX_RENDER_SCALE: f32 = 4.0;

/// CPU-rasterized RGBA8 page image produced without a UI or display server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedPage {
    pub page: usize,
    pub page_count: usize,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// CPU-rasterizes one page of `pane`'s open document to RGBA8 pixels.
pub fn render_page(
    session: &Session,
    pane: Option<u64>,
    page: Option<usize>,
    scale: f32,
) -> Result<RenderedPage, String> {
    let pane_id = resolve_pane(session, pane)?;
    let docs = session.documents();
    let doc = docs
        .document_of(pane_id)
        .ok_or_else(|| format!("pane {} has no open document", pane_id.0))?;
    let page_count = doc.page_count();
    if page_count == 0 {
        return Err("document has zero pages".to_owned());
    }
    let target = page.unwrap_or_else(|| docs.current(pane_id).unwrap_or(0));
    if target >= page_count {
        return Err(format!(
            "page {target} out of range (page_count={page_count})"
        ));
    }
    let safe_scale = if scale.is_finite() {
        scale.clamp(MIN_RENDER_SCALE, MAX_RENDER_SCALE)
    } else {
        1.5
    };
    let raster = doc
        .render(target, safe_scale)
        .map_err(|e| format!("render page {target}: {e}"))?;
    Ok(RenderedPage {
        page: target,
        page_count,
        width: raster.width,
        height: raster.height,
        pixels: raster.pixels,
    })
}

/// Serializes drained session events into structured JSON plus top-level lists.
pub fn events_summary(events: &[Event]) -> Value {
    let mut toasts = Vec::new();
    let mut exports = Vec::new();
    let mut word_lists = Vec::new();
    for event in events {
        match event {
            Event::Toast(t) => toasts.push(json!({
                "tone": match t.tone {
                    Tone::Info => "info",
                    Tone::Warning => "warning",
                    Tone::Error => "error",
                },
                "message": &t.message,
            })),
            Event::Export {
                name,
                format,
                contents,
            } => exports.push(json!({
                "name": name,
                "format": format.extension(),
                "contents": contents,
            })),
            Event::WordList { made, .. } => word_lists.push(json!({
                "words": made.words,
                "markdown": &made.markdown,
                "missing": &made.missing,
            })),
            _ => {}
        }
    }
    json!({
        "count": events.len(),
        "toasts": toasts,
        "exports": exports,
        "word_lists": word_lists,
    })
}

/// Encodes raw bytes using RFC 4648 base64 without extra crate dependencies.
pub fn encode_base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    let mut idx = 0;
    while idx < input.len() {
        let b0 = input.get(idx).copied().unwrap_or(0);
        let b1 = input.get(idx + 1).copied();
        let b2 = input.get(idx + 2).copied();
        let n =
            (u32::from(b0) << 16) | (u32::from(b1.unwrap_or(0)) << 8) | u32::from(b2.unwrap_or(0));
        let push = |out: &mut String, shift: u32| {
            let slot = ((n >> shift) & 0x3f) as usize;
            if let Some(&ch) = TABLE.get(slot) {
                out.push(ch as char);
            }
        };
        push(&mut out, 18);
        push(&mut out, 12);
        if b1.is_some() {
            push(&mut out, 6);
        } else {
            out.push('=');
        }
        if b2.is_some() {
            push(&mut out, 0);
        } else {
            out.push('=');
        }
        idx += 3;
    }
    out
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::command::Command;
    use crate::documents::OpenRequest;
    use crate::host::Fake;
    use crate::inspect::read_pages;

    fn open_sample_pdf() -> Result<Session, String> {
        let mut session = Session::headless(Arc::new(Fake::default()));
        let bytes = mareader_pdf::sample::pdf(2, "Mareader sample text");
        let req = OpenRequest::bytes(bytes, "sample.pdf");
        session.execute(Command::Open(req), 0.0);
        for _ in 0..200 {
            session.pump();
            let opened = session.documents().page_count(session.active()).is_some();
            if !session.documents().opening() && opened {
                return Ok(session);
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        Err("sample PDF failed to open".to_owned())
    }

    #[test]
    fn read_pages_and_render_page_work_on_real_pdf() {
        let Ok(session) = open_sample_pdf() else {
            return;
        };
        let Ok(read) = read_pages(&session, None, Some(0), Some(1)) else {
            return;
        };
        assert_eq!(read.get("page_count").and_then(Value::as_u64), Some(2));
        let text = read
            .get("pages")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(|p| p.get("text"))
            .and_then(Value::as_str)
            .unwrap_or("");
        assert!(text.contains("Page 1 of 2"));
        assert!(text.contains("quick brown fox"));

        let Ok(rendered) = render_page(&session, None, Some(0), 1.0) else {
            return;
        };
        assert!(rendered.width > 0 && rendered.height > 0);
        assert_eq!(
            rendered.pixels.len(),
            (rendered.width as usize) * (rendered.height as usize) * 4
        );
    }
}
