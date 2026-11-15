//! Headless RGBA8-to-PNG encoding and standard base64 helper.

use std::io::Cursor;
use std::path::Path;

use image::{ImageFormat, RgbaImage};
use mareader_engine::inspect::RenderedPage;
use serde_json::{Value, json};

/// Encodes raw RGBA8 pixels (`width * height * 4` bytes) as PNG.
pub fn encode_png_rgba(width: u32, height: u32, pixels: Vec<u8>) -> Result<Vec<u8>, String> {
    let Some(img) = RgbaImage::from_raw(width, height, pixels) else {
        return Err("rendered pixel buffer size mismatch".to_owned());
    };
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Png)
        .map_err(|e| format!("PNG encode failed: {e}"))?;
    Ok(out.into_inner())
}

/// Writes `rendered` to `path` as PNG or returns inline base64 metadata JSON.
pub fn emit_rendered_page(rendered: RenderedPage, path: Option<&Path>) -> Result<Value, String> {
    let RenderedPage {
        page,
        page_count,
        width,
        height,
        pixels,
    } = rendered;
    let bytes = encode_png_rgba(width, height, pixels)?;
    if let Some(target) = path {
        std::fs::write(target, &bytes).map_err(|e| format!("write {}: {e}", target.display()))?;
        return Ok(json!({
            "page": page,
            "page_count": page_count,
            "width": width,
            "height": height,
            "bytes": bytes.len(),
            "path": target.display().to_string(),
        }));
    }
    Ok(json!({
        "page": page,
        "page_count": page_count,
        "width": width,
        "height": height,
        "bytes": bytes.len(),
        "png_base64": encode_base64(&bytes),
    }))
}

pub use mareader_engine::inspect::encode_base64;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_encoder_writes_valid_png_signature() {
        let rgba = vec![255, 0, 0, 255, 0, 255, 0, 255];
        let Ok(png) = encode_png_rgba(2, 1, rgba) else {
            return;
        };
        assert_eq!(png.get(0..8), Some(&[137, 80, 78, 71, 13, 10, 26, 10][..]));
        assert!(!encode_base64(&png).is_empty());
    }
}
