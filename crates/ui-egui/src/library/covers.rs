//! Covers read on a worker, cached on disk, shown as textures.

use std::collections::{HashMap, VecDeque};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

use egui::load::SizedTexture;
use egui::{ColorImage, Context, TextureHandle, TextureOptions, vec2};
use mareader_library::{Book, ContentId};

/// Covers are kept at most this many pixels on their long edge.
pub const SIDE: u32 = 512;
/// The disk cache's version; other versions' folders are deleted.
const VERSION: u32 = 1;
/// Texture bytes kept; the least recently shown go first.
const BUDGET: usize = 64 << 20;
/// Textures made per frame, so a fast scroll doesn't stall one.
const UPLOADS: usize = 4;
/// Seconds a cover takes to fade in.
const FADE: f64 = 0.2;

/// What to draw for a book's cover this frame.
#[derive(Clone, Copy, Debug)]
pub enum Cover {
    Art {
        texture: SizedTexture,
        alpha: f32,
    },
    /// Not read yet.
    Waiting,
    /// It has no art, or it wouldn't read: a drawn cover instead.
    Drawn,
}

/// Straight-alpha RGBA.
struct Art {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

#[derive(Clone, Debug)]
struct Want {
    content: ContentId,
    path: Option<PathBuf>,
}

/// What the frame and the worker share.
#[derive(Default)]
struct Shared {
    /// Best first; replaced whenever the page asks for others.
    wanted: VecDeque<Want>,
    busy: Option<ContentId>,
    done: Vec<(ContentId, Option<Art>)>,
    quit: bool,
}

type Link = Arc<(Mutex<Shared>, Condvar)>;

fn lock(link: &Link) -> MutexGuard<'_, Shared> {
    link.0.lock().unwrap_or_else(PoisonError::into_inner)
}

enum Slot {
    /// Read, waiting for its texture.
    Read(Art),
    Shown {
        texture: TextureHandle,
        since: f64,
        /// The last frame it was asked for.
        used: u64,
    },
    Drawn,
}

#[derive(Default)]
pub struct Covers {
    slots: HashMap<ContentId, Slot>,
    link: Option<Link>,
    inline: bool,
    frame: u64,
    /// This frame's asks, best first.
    asked: Vec<Want>,
    /// The asks last handed to the worker.
    sent: Vec<ContentId>,
    /// Covers read and waiting for a texture that this frame asked for.
    near: Vec<ContentId>,
    bytes: usize,
}

impl Drop for Covers {
    /// The worker stops after its cover; nothing waits for it.
    fn drop(&mut self) {
        if let Some(link) = &self.link {
            lock(link).quit = true;
            link.1.notify_all();
        }
    }
}

impl Covers {
    /// Covers read on the frame, as tests and shots want.
    pub fn set_inline(&mut self, inline: bool) {
        self.inline = inline;
    }

    /// `book`'s cover, asked for if it isn't here yet.
    pub fn get(&mut self, ctx: &Context, book: &Book, animate: bool) -> Cover {
        let frame = self.frame;
        match self.slots.get_mut(&book.content) {
            Some(Slot::Shown {
                texture,
                since,
                used,
            }) => {
                *used = frame;
                let age = ctx.input(|i| i.time) - *since;
                let alpha = match animate {
                    true => (age / FADE).clamp(0.0, 1.0) as f32,
                    false => 1.0,
                };
                if alpha < 1.0 {
                    ctx.request_repaint();
                }
                Cover::Art {
                    texture: SizedTexture::from_handle(texture),
                    alpha,
                }
            }
            Some(Slot::Drawn) => Cover::Drawn,
            Some(Slot::Read(_)) => {
                self.near.push(book.content);
                Cover::Waiting
            }
            None => {
                self.prefetch(book);
                Cover::Waiting
            }
        }
    }

    /// Asks for `book`'s cover after those already asked this frame.
    pub fn prefetch(&mut self, book: &Book) {
        if let Some(slot) = self.slots.get(&book.content) {
            if matches!(slot, Slot::Read(_)) && !self.near.contains(&book.content) {
                self.near.push(book.content);
            }
            return;
        }
        if self.asked.iter().any(|w| w.content == book.content) {
            return;
        }
        self.asked.push(Want {
            content: book.content,
            path: book.path().map(Path::to_path_buf),
        });
    }

    /// Hands this frame's asks over and makes a few textures.
    ///
    /// `cache` is `<dir>/covers`, where covers are kept between launches;
    /// none keeps them in memory only.
    pub fn end_frame(&mut self, ctx: &Context, cache: Option<&Path>) {
        // Best first: what waits already, then this frame's new asks.
        let mut near = std::mem::take(&mut self.near);
        near.extend(self.asked.iter().map(|w| w.content));
        if self.inline {
            // As many as become textures, so none pile up waiting.
            let take = self.asked.len().min(UPLOADS);
            for want in self.asked.drain(..take).collect::<Vec<_>>() {
                let art = load(&want, versioned(cache).as_deref());
                self.arrive(want.content, art);
            }
            if !self.asked.is_empty() {
                ctx.request_repaint();
            }
        } else {
            self.trade(ctx, cache);
        }
        // Read for books scrolled past: the disk cache has them again.
        let waits = |c: &ContentId, s: &mut Slot| !matches!(s, Slot::Read(_)) || near.contains(c);
        self.slots.retain(waits);
        self.upload(ctx, &near);
        self.evict();
        self.asked.clear();
        self.frame += 1;
    }

    /// Takes finished covers; new asks replace old, so scrolled-past drop.
    fn trade(&mut self, ctx: &Context, cache: Option<&Path>) {
        if self.asked.is_empty() && self.sent.is_empty() && self.link.is_none() {
            return;
        }
        let link = match &self.link {
            Some(link) => Arc::clone(link),
            None => match self.spawn(ctx, cache) {
                Some(link) => link,
                // No thread: covers are read on the frame instead.
                None => return self.inline = true,
            },
        };
        let (done, busy) = {
            let mut shared = lock(&link);
            (std::mem::take(&mut shared.done), shared.busy)
        };
        for (content, art) in done {
            self.arrive(content, art);
        }
        let slots = &self.slots;
        self.asked
            .retain(|w| !slots.contains_key(&w.content) && Some(w.content) != busy);
        let asks: Vec<ContentId> = self.asked.iter().map(|w| w.content).collect();
        if asks != self.sent {
            lock(&link).wanted = self.asked.drain(..).collect();
            link.1.notify_one();
            self.sent = asks;
        }
    }

    fn spawn(&mut self, ctx: &Context, cache: Option<&Path>) -> Option<Link> {
        let link: Link = Arc::default();
        let (theirs, ctx, cache) = (Arc::clone(&link), ctx.clone(), versioned(cache));
        std::thread::Builder::new()
            .name("covers".into())
            .spawn(move || work(&theirs, &ctx, cache.as_deref()))
            .ok()?;
        self.link = Some(Arc::clone(&link));
        Some(link)
    }

    fn arrive(&mut self, content: ContentId, art: Option<Art>) {
        let slot = art.map_or(Slot::Drawn, Slot::Read);
        self.slots.insert(content, slot);
        self.sent.retain(|c| *c != content);
    }

    fn upload(&mut self, ctx: &Context, near: &[ContentId]) {
        let now = ctx.input(|i| i.time);
        let read = |c: &&ContentId| matches!(self.slots.get(c), Some(Slot::Read(_)));
        let waiting: Vec<ContentId> = near.iter().filter(read).copied().collect();
        if waiting.len() > UPLOADS {
            ctx.request_repaint();
        }
        for content in waiting.into_iter().take(UPLOADS) {
            let Some(Slot::Read(art)) = self.slots.remove(&content) else {
                continue;
            };
            let size = [art.width as usize, art.height as usize];
            let image = ColorImage::from_rgba_unmultiplied(size, &art.pixels);
            let name = format!("cover-{:x}", content.0);
            let texture = ctx.load_texture(name, image, TextureOptions::LINEAR);
            self.bytes += art.pixels.len();
            let used = self.frame;
            self.slots.insert(
                content,
                Slot::Shown {
                    texture,
                    since: now,
                    used,
                },
            );
            ctx.request_repaint();
        }
    }

    /// Drops the least recently shown textures past the budget.
    fn evict(&mut self) {
        while self.bytes > BUDGET {
            let oldest = self
                .slots
                .iter()
                .filter_map(|(c, s)| match s {
                    Slot::Shown { used, .. } if *used < self.frame => Some((*used, *c)),
                    _ => None,
                })
                .min();
            let Some((_, content)) = oldest else { return };
            if let Some(Slot::Shown { texture, .. }) = self.slots.remove(&content) {
                let [w, h] = texture.size();
                self.bytes = self.bytes.saturating_sub(w * h * 4);
            }
        }
    }

    /// The cover textures' bytes, held under a 64 MiB budget.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Covers read and waiting for a texture; for tests and meters.
    pub fn waiting(&self) -> usize {
        let read = |s: &&Slot| matches!(s, Slot::Read(_));
        self.slots.values().filter(read).count()
    }

    /// Covers kept as textures; for tests.
    pub fn shown(&self) -> usize {
        let shown = |s: &&Slot| matches!(s, Slot::Shown { .. });
        self.slots.values().filter(shown).count()
    }
}

/// The cache's own folder for this build; other versions' are deleted.
fn versioned(cache: Option<&Path>) -> Option<PathBuf> {
    cache.map(|c| c.join(format!("v{VERSION}")))
}

fn work(link: &Link, ctx: &Context, cache: Option<&Path>) {
    if let Some(dir) = cache {
        prune(dir);
    }
    loop {
        let want = {
            let mut shared = lock(link);
            loop {
                if shared.quit {
                    return;
                }
                if let Some(want) = shared.wanted.pop_front() {
                    shared.busy = Some(want.content);
                    break want;
                }
                shared = link.1.wait(shared).unwrap_or_else(PoisonError::into_inner);
            }
        };
        let art = load(&want, cache);
        let mut shared = lock(link);
        shared.busy = None;
        shared.done.push((want.content, art));
        drop(shared);
        ctx.request_repaint();
    }
}

/// From the disk cache, else from the book, which then fills the cache.
fn load(want: &Want, cache: Option<&Path>) -> Option<Art> {
    let stem = format!("{:032x}", want.content.0);
    if let Some(art) = cache.and_then(|dir| cached(dir, &stem)) {
        return Some(art);
    }
    let raster = mareader_session::cover(want.path.as_deref()?, SIDE)?;
    let art = Art {
        width: raster.width,
        height: raster.height,
        pixels: raster.pixels,
    };
    if let Some(dir) = cache {
        keep(dir, &stem, &art);
    }
    Some(art)
}

fn cached(dir: &Path, stem: &str) -> Option<Art> {
    ["jpg", "png"].into_iter().find_map(|ext| {
        let bytes = std::fs::read(dir.join(format!("{stem}.{ext}"))).ok()?;
        let img = image::load_from_memory(&bytes).ok()?.into_rgba8();
        Some(Art {
            width: img.width(),
            height: img.height(),
            pixels: img.into_raw(),
        })
    })
}

/// Opaque covers keep as JPEG, the rest as PNG; failures only cost time.
fn keep(dir: &Path, stem: &str, art: &Art) {
    let pixels = art.pixels.as_chunks::<4>().0;
    let opaque = pixels.iter().all(|p| p[3] == 255);
    let mut out = Cursor::new(Vec::new());
    let (ext, written) = if opaque {
        let rgb: Vec<u8> = pixels.iter().flat_map(|&[r, g, b, _]| [r, g, b]).collect();
        let mut jpeg = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85);
        let color = image::ExtendedColorType::Rgb8;
        (
            "jpg",
            jpeg.encode(&rgb, art.width, art.height, color).is_ok(),
        )
    } else {
        let img = image::RgbaImage::from_raw(art.width, art.height, art.pixels.clone());
        let png = img.map(|i| i.write_to(&mut out, image::ImageFormat::Png).is_ok());
        ("png", png.unwrap_or(false))
    };
    if written {
        let path = dir.join(format!("{stem}.{ext}"));
        let _ = mareader_library::disk::write_atomic(&path, out.get_ref());
    }
}

/// Deletes other versions' folders beside `dir`.
fn prune(dir: &Path) {
    let (Some(parent), Some(name)) = (dir.parent(), dir.file_name()) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let other = entry.file_name();
        let other = other.to_string_lossy();
        let versioned = other
            .strip_prefix('v')
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
        if versioned && other != name.to_string_lossy() {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// Where `size` fits in `bounds` keeping its shape, bottom-aligned.
pub fn fit(bounds: egui::Rect, size: egui::Vec2) -> egui::Rect {
    let scale = (bounds.width() / size.x).min(bounds.height() / size.y);
    let shown = size * scale;
    let min = egui::pos2(bounds.center().x - shown.x / 2.0, bounds.bottom() - shown.y);
    egui::Rect::from_min_size(min, shown)
}

/// The centred part of an image of `size` shaped like `bounds`.
pub fn crop(bounds: egui::Rect, size: egui::Vec2) -> egui::Rect {
    let (want, have) = (bounds.height() / bounds.width(), size.y / size.x);
    let (w, h) = match have > want {
        true => (1.0, want / have),
        false => (have / want, 1.0),
    };
    egui::Rect::from_center_size(egui::pos2(0.5, 0.5), vec2(w, h))
}
