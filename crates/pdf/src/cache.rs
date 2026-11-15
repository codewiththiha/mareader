//! Reusable hayro render and interpreter caches, and lightweight PDF probes.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use hayro::hayro_interpret::{InterpreterCache, InterpreterSettings};
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::Pixmap;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::vello_cpu::peniko::ImageAlphaType;
use hayro::{PixmapSettings, RenderCache, RenderSettings};

use crate::document::{
    Document, MAX_PAGES, MAX_RASTER_SIDE, PageSize, REGION_SCALE, Raster, RenderError,
};
use crate::paper::{Paper, SAMPLE_SIDE};
use crate::strings::decode_text;
use crate::text::{Bounds, TextLayer};
use crate::tiles::Tiling;

/// A worker-scoped cache for repeated renders and text reads on one PDF.
pub struct RenderSession<'a> {
    doc: &'a Document,
    render: RenderCache<'a>,
    interpret: InterpreterCache<'a>,
    poisoned: bool,
}

impl<'a> RenderSession<'a> {
    pub(crate) fn new(doc: &'a Document) -> Self {
        Self {
            doc,
            render: RenderCache::new(),
            interpret: InterpreterCache::new(),
            poisoned: false,
        }
    }

    /// True once a render panicked inside hayro; drop and reopen on retry.
    pub fn poisoned(&self) -> bool {
        self.poisoned
    }

    /// Draws page `page` (0-based) at `scale` pixels per point.
    pub fn render(&mut self, page: usize, scale: f32) -> Result<Raster, RenderError> {
        let size = self.doc.size_at(page)?;
        let scale = size.fit_scale(scale);
        self.run(|doc, render, _| doc.raster_with(page, scale, render))
    }

    /// Tile `col`, `row` of page `page` split at `scale`; see [`Tiling`].
    pub fn render_tile(
        &mut self,
        page: usize,
        scale: f32,
        col: u32,
        row: u32,
    ) -> Result<Raster, RenderError> {
        let size = self.doc.size_at(page)?;
        let tiling = Tiling::new(size, scale);
        let rect = tiling.rect(col, row).ok_or(RenderError::Failed)?;
        self.run(|doc, render, _| doc.tile_with(page, tiling.scale, rect, render))
    }

    /// Box `at` of page `page`, `side` pixels on its long edge at most.
    pub fn render_region(
        &mut self,
        page: usize,
        at: Bounds,
        side: u32,
    ) -> Result<Raster, RenderError> {
        let size = self.doc.size_at(page)?;
        let x0 = at.x0.clamp(0.0, size.width);
        let y0 = at.y0.clamp(0.0, size.height);
        let (w, h) = (at.x1.min(size.width) - x0, at.y1.min(size.height) - y0);
        if !(w >= 1.0 && h >= 1.0) {
            return Err(RenderError::Failed);
        }
        let scale = (side.min(MAX_RASTER_SIDE) as f32 / w.max(h)).min(REGION_SCALE);
        let px = |v: f32| (v * scale).round() as u32;
        let rect = [px(x0), px(y0), px(w).max(1), px(h).max(1)];
        self.run(|doc, render, _| doc.tile_with(page, scale, rect, render))
    }

    /// Page `page`'s characters and links; never panics.
    pub fn text_layer(&mut self, page: usize) -> Result<TextLayer, RenderError> {
        self.doc.size_at(page)?;
        self.run(|doc, _, interpret| doc.read_layer_with(page, interpret))
    }

    /// Page `page` drawn small and sampled for its paper colour; never panics.
    pub fn paper(&mut self, page: usize) -> Result<Paper, RenderError> {
        let size = self.doc.size_at(page)?;
        let scale = SAMPLE_SIDE / size.width.max(size.height);
        Ok(Paper::detect(&self.render(page, scale)?))
    }

    fn run<T>(
        &mut self,
        f: impl FnOnce(&'a Document, &RenderCache<'a>, &InterpreterCache<'a>) -> Result<T, RenderError>,
    ) -> Result<T, RenderError> {
        let doc = self.doc;
        let render = &self.render;
        let interpret = &self.interpret;
        match catch_unwind(AssertUnwindSafe(|| f(doc, render, interpret))) {
            Ok(result) => result,
            Err(_) => {
                self.poisoned = true;
                Err(RenderError::Failed)
            }
        }
    }
}

/// Reads `/Title` and `/Author` without measuring pages or parsing the outline.
pub fn describe(bytes: Arc<Vec<u8>>) -> Option<(Option<String>, Option<String>)> {
    let read = || {
        let pdf = Pdf::new(bytes).ok()?;
        let title = pdf.metadata().title.as_deref().map(decode_text);
        let author = pdf.metadata().author.as_deref().map(decode_text);
        Some((title, author))
    };
    catch_unwind(AssertUnwindSafe(read)).ok().flatten()
}

/// Draws only page 0 to fit `side`, skipping the outline and remaining pages.
pub fn first_page(bytes: Arc<Vec<u8>>, side: u32) -> Option<Raster> {
    let read = || {
        let pdf = Pdf::new(bytes).ok()?;
        let pages = pdf.pages();
        if pages.len() > MAX_PAGES {
            return None;
        }
        let page = pages.first()?;
        let (w, h) = page.render_dimensions();
        let size = PageSize::sane(w, h);
        let raw = side.max(1) as f32 / size.width.max(size.height).max(1.0);
        let scale = size.fit_scale(raw);
        let cache = RenderCache::new();
        let settings = PixmapSettings {
            x_scale: scale,
            y_scale: scale,
            bg_color: WHITE,
        };
        let pixmap = hayro::render(
            page,
            &cache,
            &InterpreterSettings::default(),
            &RenderSettings::default(),
            &settings,
        );
        finish(pixmap).ok()
    };
    catch_unwind(AssertUnwindSafe(read)).ok().flatten()
}

pub(crate) fn finish(pixmap: Pixmap) -> Result<Raster, RenderError> {
    let (width, height) = (u32::from(pixmap.width()), u32::from(pixmap.height()));
    if width == 0 || height == 0 {
        return Err(RenderError::Failed);
    }
    // Handed over, not copied: no second page-sized buffer.
    let pixels = pixmap.take_rgba8(ImageAlphaType::AlphaPremultiplied);
    Ok(Raster {
        width,
        height,
        pixels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample;

    #[test]
    fn render_session_matches_oneshot_renders_and_text() {
        let bytes = Arc::new(sample::pdf(3, "Session Title"));
        let doc = Document::open(Arc::clone(&bytes)).ok();
        let Some(doc) = doc else {
            return;
        };
        let mut session = doc.session();
        for page in 0..3 {
            assert_eq!(session.render(page, 1.0), doc.render(page, 1.0));
            assert_eq!(
                session.render_tile(page, 2.0, 0, 0),
                doc.render_tile(page, 2.0, 0, 0)
            );
            assert_eq!(session.text_layer(page), doc.text_layer(page));
            assert_eq!(session.paper(page), doc.paper(page));
        }
        assert!(!session.poisoned());
        assert_eq!(session.render(9, 1.0), Err(RenderError::NoPage(9)));
        let title = describe(Arc::clone(&bytes)).and_then(|(t, _)| t);
        assert_eq!(title.as_deref(), Some("Session Title"));
        let thumb = first_page(bytes, 256);
        assert!(thumb.is_some_and(|r| r.height.abs_diff(256) <= 1));
    }
}
