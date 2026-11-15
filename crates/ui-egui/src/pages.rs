//! Page textures: one byte-capped cache serving every pane.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use egui::{ColorImage, Context, TextureHandle, TextureOptions};
use mareader_session::{DocId, PageKey, Part, Raster, Scale};

use crate::gpu::GpuPage;

/// Textures past this are evicted, least recently drawn first.
pub const CAP_BYTES: usize = 256 << 20;
/// What the cache keeps at rest: pages in view, then the newest.
pub const REST_BYTES: usize = 64 << 20;

/// A page on the GPU: ours for the page pass, or egui's without one.
#[derive(Clone)]
pub enum PageTexture {
    Gpu(Arc<GpuPage>),
    /// Headless runs have no device; egui keeps the pixels.
    Egui(TextureHandle),
}

impl PageTexture {
    pub fn egui(ctx: &Context, key: PageKey, raster: &Raster) -> Self {
        let size = [raster.width as usize, raster.height as usize];
        let image = ColorImage::from_rgba_premultiplied(size, &raster.pixels);
        let name = format!("page-{:?}-{}-{:?}", key.doc, key.page, key.part);
        Self::Egui(ctx.load_texture(name, image, TextureOptions::LINEAR))
    }
}

impl std::fmt::Debug for PageTexture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Gpu(_) => f.write_str("Gpu"),
            Self::Egui(h) => write!(f, "Egui({:?})", h.id()),
        }
    }
}

/// What a pane can draw for a page right now.
#[derive(Debug, Clone)]
pub enum Lookup {
    /// Drawn at the scale asked for.
    Sharp(PageTexture),
    /// Another scale, drawn stretched until the sharp one arrives.
    Stale(PageTexture),
    Failed,
    Missing,
}

struct Slot {
    scale: Scale,
    texture: PageTexture,
    bytes: usize,
    /// The frame it was last drawn in, and last drawn sharp in.
    used: u64,
    sharp: u64,
}

/// A tile of a page too big for one texture, at one exact scale.
struct Tile {
    texture: PageTexture,
    bytes: usize,
    used: u64,
}

/// A slot or a tile to evict, from cold to warm.
enum Cold {
    Slot((DocId, usize), Scale),
    Tile(PageKey),
}

pub struct PageCache {
    pages: HashMap<(DocId, usize), Vec<Slot>>,
    tiles: HashMap<PageKey, Tile>,
    /// Pages drawn in tiles this frame, at the tiles' scale.
    tiled: HashSet<(DocId, usize, Scale)>,
    failed: HashSet<(DocId, usize)>,
    failed_tiles: HashSet<PageKey>,
    bytes: usize,
    cap: usize,
    frame: u64,
}

impl Default for PageCache {
    fn default() -> Self {
        Self::new(CAP_BYTES)
    }
}

impl PageCache {
    pub fn new(cap: usize) -> Self {
        Self {
            pages: HashMap::new(),
            tiles: HashMap::new(),
            tiled: HashSet::new(),
            failed: HashSet::new(),
            failed_tiles: HashSet::new(),
            bytes: 0,
            cap,
            frame: 1,
        }
    }

    /// Keeps an uploaded page; a slot at the same scale is replaced.
    pub fn insert(&mut self, key: PageKey, texture: PageTexture, bytes: usize) {
        if key.part != Part::Whole {
            let used = self.frame;
            let tile = Tile {
                texture,
                bytes,
                used,
            };
            self.bytes += bytes;
            if let Some(old) = self.tiles.insert(key, tile) {
                self.bytes -= old.bytes;
            }
            return;
        }
        let slots = self.pages.entry((key.doc, key.page)).or_default();
        if let Some(i) = slots.iter().position(|s| s.scale == key.scale) {
            self.bytes -= slots.swap_remove(i).bytes;
        }
        self.bytes += bytes;
        slots.push(Slot {
            scale: key.scale,
            texture,
            bytes,
            used: self.frame,
            sharp: 0,
        });
        self.failed.remove(&(key.doc, key.page));
    }

    pub fn fail(&mut self, key: PageKey) {
        if key.part == Part::Whole {
            self.failed.insert((key.doc, key.page));
        } else {
            self.failed_tiles.insert(key);
        }
    }

    /// The tile `key` exactly, marked as drawn; else the backdrop shows.
    pub fn lookup_tile(&mut self, key: PageKey) -> Option<PageTexture> {
        self.tiled.insert((key.doc, key.page, key.scale));
        let tile = self.tiles.get_mut(&key)?;
        tile.used = self.frame;
        Some(tile.texture.clone())
    }

    /// The best texture for `key`, marked as drawn this frame.
    pub fn lookup(&mut self, key: PageKey) -> Lookup {
        let frame = self.frame;
        let Some(slots) = self.pages.get_mut(&(key.doc, key.page)) else {
            return self.missing(key);
        };
        if let Some(s) = slots.iter_mut().find(|s| s.scale == key.scale) {
            s.used = frame;
            s.sharp = frame;
            return Lookup::Sharp(s.texture.clone());
        }
        // The nearest scale; on a tie, the sharper one.
        let nearest = slots.iter_mut().min_by_key(|s| {
            let away = (i32::from(s.scale.steps()) - i32::from(key.scale.steps())).abs();
            (away, -i32::from(s.scale.steps()))
        });
        match nearest {
            Some(s) => {
                s.used = frame;
                Lookup::Stale(s.texture.clone())
            }
            None => self.missing(key),
        }
    }

    fn missing(&self, key: PageKey) -> Lookup {
        if self.failed.contains(&(key.doc, key.page)) {
            Lookup::Failed
        } else {
            Lookup::Missing
        }
    }

    /// Prefetched pages count as used, so eviction spares them.
    pub fn touch(&mut self, key: PageKey) -> bool {
        let frame = self.frame;
        if key.part != Part::Whole {
            let tile = self.tiles.get_mut(&key).map(|t| t.used = frame);
            return tile.is_some() || self.failed_tiles.contains(&key);
        }
        let slot = self
            .pages
            .get_mut(&(key.doc, key.page))
            .and_then(|slots| slots.iter_mut().find(|s| s.scale == key.scale));
        slot.map(|s| s.used = frame).is_some() || self.failed.contains(&(key.doc, key.page))
    }

    /// Hands `old`'s textures to `new` where `map` finds the same page;
    /// the rest go.
    pub fn adopt(&mut self, old: DocId, new: DocId, map: impl Fn(usize) -> Option<usize>) {
        let pages: Vec<usize> = (self.pages.keys())
            .filter(|(d, _)| *d == old)
            .map(|(_, p)| *p)
            .collect();
        for page in pages {
            let Some(to) = map(page).filter(|to| !self.pages.contains_key(&(new, *to))) else {
                continue;
            };
            if let Some(slots) = self.pages.remove(&(old, page)) {
                self.pages.insert((new, to), slots);
            }
        }
        let tiles: Vec<PageKey> = self
            .tiles
            .keys()
            .filter(|k| k.doc == old)
            .copied()
            .collect();
        for key in tiles {
            let moved = map(key.page).map(|page| PageKey {
                doc: new,
                page,
                ..key
            });
            let Some(to) = moved.filter(|k| !self.tiles.contains_key(k)) else {
                continue;
            };
            if let Some(tile) = self.tiles.remove(&key) {
                self.tiles.insert(to, tile);
            }
        }
        self.forget(old);
    }

    /// Drops every texture of a closed document.
    pub fn forget(&mut self, doc: DocId) {
        self.pages.retain(|(d, _), slots| {
            if *d == doc {
                self.bytes -= slots.iter().map(|s| s.bytes).sum::<usize>();
            }
            *d != doc
        });
        self.failed.retain(|(d, _)| *d != doc);
        self.tiles.retain(|k, t| {
            if k.doc == doc {
                self.bytes -= t.bytes;
            }
            k.doc != doc
        });
        self.failed_tiles.retain(|k| k.doc != doc);
    }

    /// Evicts superseded scales, then cold pages past the cap; never drawn.
    pub fn end_frame(&mut self) {
        let frame = self.frame;
        let mut freed = 0;
        for slots in self.pages.values_mut() {
            if slots.iter().any(|s| s.sharp == frame) {
                slots.retain(|s| {
                    let keep = s.used == frame;
                    freed += if keep { 0 } else { s.bytes };
                    keep
                });
            }
        }
        self.bytes -= freed;
        self.drop_superseded_tiles();
        if self.bytes > self.cap {
            self.evict_cold(self.cap);
        }
        self.pages.retain(|_, slots| !slots.is_empty());
        self.frame += 1;
    }

    /// A drawn page keeps only tiles at the scale it wants them at.
    fn drop_superseded_tiles(&mut self) {
        let frame = self.frame;
        let tiled = std::mem::take(&mut self.tiled);
        let pages = &self.pages;
        let shown = |k: &PageKey| {
            let sharp = pages.get(&(k.doc, k.page));
            let whole = sharp.is_some_and(|s| s.iter().any(|s| s.sharp == frame));
            whole || tiled.iter().any(|d| (d.0, d.1) == (k.doc, k.page))
        };
        let mut freed = 0;
        self.tiles.retain(|k, t| {
            let keep = t.used == frame || !shown(k) || tiled.contains(&(k.doc, k.page, k.scale));
            freed += if keep { 0 } else { t.bytes };
            keep
        });
        self.bytes -= freed;
    }

    /// At rest: cold textures go down to [`REST_BYTES`]; those in view stay.
    pub fn rest(&mut self) {
        self.evict_cold(REST_BYTES);
        self.pages.retain(|_, slots| !slots.is_empty());
    }

    /// Evicts what wasn't drawn this frame, coldest first, down to `to`.
    fn evict_cold(&mut self, to: usize) {
        let frame = self.frame;
        let slots = self.pages.iter().flat_map(|(page, slots)| {
            slots
                .iter()
                .map(move |s| (s.used, Cold::Slot(*page, s.scale)))
        });
        let tiles = self.tiles.iter().map(|(k, t)| (t.used, Cold::Tile(*k)));
        let mut cold: Vec<(u64, Cold)> = slots.chain(tiles).filter(|(u, _)| *u < frame).collect();
        cold.sort_unstable_by_key(|(used, _)| *used);
        for (_, item) in cold {
            if self.bytes <= to {
                break;
            }
            match item {
                Cold::Slot(page, scale) => {
                    if let Some(slots) = self.pages.get_mut(&page)
                        && let Some(i) = slots.iter().position(|s| s.scale == scale)
                    {
                        self.bytes -= slots.swap_remove(i).bytes;
                    }
                }
                Cold::Tile(key) => {
                    if let Some(t) = self.tiles.remove(&key) {
                        self.bytes -= t.bytes;
                    }
                }
            }
        }
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Whole-page textures; tiles count apart, see [`Self::tiles`].
    pub fn len(&self) -> usize {
        self.pages.values().map(Vec::len).sum()
    }

    pub fn tiles(&self) -> usize {
        self.tiles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pages.is_empty() && self.tiles.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Out = Result<(), &'static str>;

    fn raster(side: u32) -> Raster {
        Raster {
            width: side,
            height: side,
            pixels: vec![255; (side * side * 4) as usize],
        }
    }

    fn put(c: &mut PageCache, ctx: &Context, key: PageKey, side: u32) {
        let r = raster(side);
        c.insert(key, PageTexture::egui(ctx, key, &r), r.bytes());
    }

    use crate::testing::doc;

    fn key(doc: DocId, page: usize, scale: f32) -> PageKey {
        PageKey {
            doc,
            page,
            scale: Scale::covering(scale),
            part: Part::Whole,
        }
    }

    #[test]
    fn a_stale_scale_stands_in_until_the_sharp_one_is_drawn() -> Out {
        let ctx = Context::default();
        let mut c = PageCache::default();
        let d = doc()?;
        put(&mut c, &ctx, key(d, 0, 1.0), 8);
        assert!(matches!(c.lookup(key(d, 0, 2.0)), Lookup::Stale(_)));
        c.end_frame();
        put(&mut c, &ctx, key(d, 0, 2.0), 16);
        assert!(matches!(c.lookup(key(d, 0, 2.0)), Lookup::Sharp(_)));
        c.end_frame();
        assert_eq!((c.len(), c.bytes()), (1, 16 * 16 * 4));
        Ok(())
    }

    #[test]
    fn at_rest_cold_pages_go_and_the_pages_in_view_stay() -> Out {
        let ctx = Context::default();
        let mut c = PageCache::default();
        let d = doc()?;
        let r = raster(4);
        let big = REST_BYTES / 4;
        // One a frame, so page 7 is the newest of them.
        for page in 0..8 {
            let k = key(d, page, 1.0);
            c.insert(k, PageTexture::egui(&ctx, k, &r), big);
            c.end_frame();
        }
        // Page 0 is in view again.
        assert!(matches!(c.lookup(key(d, 0, 1.0)), Lookup::Sharp(_)));
        c.rest();
        assert_eq!(c.bytes(), REST_BYTES);
        assert!(c.touch(key(d, 0, 1.0)) && c.touch(key(d, 7, 1.0)));
        assert!(!c.touch(key(d, 1, 1.0)), "the coldest went");
        Ok(())
    }

    #[test]
    fn the_cap_evicts_cold_pages_but_never_visible_ones() -> Out {
        let ctx = Context::default();
        let one = 8 * 8 * 4;
        let mut c = PageCache::new(2 * one);
        let d = doc()?;
        for page in 0..4 {
            put(&mut c, &ctx, key(d, page, 1.0), 8);
        }
        c.end_frame();
        // Pages 2 and 3 are on screen; 0 and 1 went cold.
        for page in [2, 3] {
            assert!(matches!(c.lookup(key(d, page, 1.0)), Lookup::Sharp(_)));
        }
        c.end_frame();
        assert_eq!(c.bytes(), 2 * one);
        assert!(!c.touch(key(d, 0, 1.0)), "evicted");
        // Over the cap, what is on screen still stays.
        for page in 4..7 {
            put(&mut c, &ctx, key(d, page, 1.0), 8);
            assert!(matches!(c.lookup(key(d, page, 1.0)), Lookup::Sharp(_)));
        }
        c.end_frame();
        assert_eq!(c.len(), 3);
        Ok(())
    }

    #[test]
    fn forgetting_a_document_frees_all_of_it() -> Out {
        let ctx = Context::default();
        let mut c = PageCache::default();
        let d = doc()?;
        put(&mut c, &ctx, key(d, 0, 1.0), 8);
        c.fail(key(d, 1, 1.0));
        assert!(matches!(c.lookup(key(d, 1, 1.0)), Lookup::Failed));
        c.forget(d);
        assert!(c.is_empty() && c.bytes() == 0);
        assert!(matches!(c.lookup(key(d, 1, 1.0)), Lookup::Missing));
        Ok(())
    }

    fn tile(doc: DocId, page: usize, scale: f32, col: u8) -> PageKey {
        PageKey {
            part: Part::Tile { col, row: 0 },
            ..key(doc, page, scale)
        }
    }

    #[test]
    fn growing_a_document_keeps_unchanged_page_textures() -> Out {
        let ctx = Context::default();
        let mut c = PageCache::default();
        let mut session = mareader_session::Session::new(0, || {});
        let bytes = || std::sync::Arc::new(mareader_pdf::sample::pdf(1, "Cache"));
        let one = session.open(
            mareader_session::Source::Bytes(bytes()),
            None,
            Default::default(),
        );
        let two = session.open(
            mareader_session::Source::Bytes(bytes()),
            None,
            Default::default(),
        );
        let budget = mareader_session::Budget {
            items: 1,
            bytes: usize::MAX,
        };
        let events = session.poll(budget).into_iter().chain(session.poll(budget));
        let ids: Vec<DocId> = events
            .filter_map(|event| match event {
                mareader_session::Event::Opened {
                    ticket,
                    result: Ok(id),
                } if ticket == one || ticket == two => Some(id),
                _ => None,
            })
            .collect();
        let (Some(old), Some(new)) = (ids.first().copied(), ids.get(1).copied()) else {
            return Err("the samples didn't open");
        };
        for page in 0..3 {
            put(&mut c, &ctx, key(old, page, 1.0), 8);
        }
        c.adopt(old, new, |page| (page < 2).then_some(page));
        assert!(c.touch(key(new, 0, 1.0)) && c.touch(key(new, 1, 1.0)));
        assert!(!c.touch(key(old, 0, 1.0)) && !c.touch(key(new, 2, 1.0)));
        assert_eq!(c.len(), 2);
        Ok(())
    }

    #[test]
    fn tiles_share_the_byte_cap_and_go_cold_first() -> Out {
        let ctx = Context::default();
        let one = 8 * 8 * 4;
        let mut c = PageCache::new(3 * one);
        let d = doc()?;
        put(&mut c, &ctx, key(d, 0, 1.0), 8);
        for col in 0..3 {
            put(&mut c, &ctx, tile(d, 0, 4.0, col), 8);
        }
        assert_eq!((c.len(), c.tiles(), c.bytes()), (1, 3, 4 * one));
        c.end_frame();
        // The backdrop and one tile on screen; the cap takes cold tiles.
        assert!(matches!(c.lookup(key(d, 0, 1.0)), Lookup::Sharp(_)));
        assert!(c.lookup_tile(tile(d, 0, 4.0, 1)).is_some());
        c.end_frame();
        assert!(c.bytes() <= 3 * one, "{} bytes", c.bytes());
        assert!(c.touch(tile(d, 0, 4.0, 1)), "the drawn tile stays");
        Ok(())
    }

    #[test]
    fn a_page_drawn_at_another_scale_drops_its_old_tiles() -> Out {
        let ctx = Context::default();
        let mut c = PageCache::default();
        let d = doc()?;
        put(&mut c, &ctx, tile(d, 0, 4.0, 0), 8);
        put(&mut c, &ctx, tile(d, 0, 4.0, 1), 8);
        put(&mut c, &ctx, tile(d, 0, 8.0, 0), 8);
        c.end_frame();
        // Zoomed to 8×, one tile shows: its neighbours at 8× would stay.
        assert!(c.lookup_tile(tile(d, 0, 8.0, 0)).is_some());
        c.end_frame();
        assert_eq!(c.tiles(), 1);
        // Zoomed out, the whole page is sharp and needs no tiles.
        put(&mut c, &ctx, key(d, 0, 1.0), 8);
        assert!(matches!(c.lookup(key(d, 0, 1.0)), Lookup::Sharp(_)));
        c.end_frame();
        assert_eq!(c.tiles(), 0);
        assert!(!c.touch(tile(d, 0, 8.0, 0)));
        Ok(())
    }
}
