//! The cards the frame keeps: set off it, drawn and cached.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};

use egui::{Context, Rect, Vec2, pos2, vec2};
use mareader_engine::page_ai::cards::{Ask, Done, Set, set, worker};
use mareader_reflow::PAD;
use mareader_session::Raster;

use mareader_engine::page_ai::cards::Job;

use crate::gpu::Gpu;
use crate::pages::PageTexture;

/// Seconds an unseen card is kept before its texture goes.
const KEEP: f64 = 3.0;

/// A set result, ready to draw.
#[derive(Clone)]
pub struct Card {
    pub texture: PageTexture,
    /// Its width and depth, in page units.
    pub size: Vec2,
    /// Pictures and emoji, in page units from the card's corner.
    pub pictures: Vec<Rect>,
    pub emojis: Vec<Rect>,
}

struct Held {
    key: u64,
    card: Option<Card>,
    seen: f64,
}

/// Each slot's latest card; a stale one shows until its next is set.
pub struct Cards {
    tx: Option<Sender<Ask>>,
    rx: Option<Receiver<Done>>,
    held: HashMap<u64, Held>,
    /// Slots being set now, by key.
    busy: HashMap<u64, u64>,
    /// Cards the frame set, without a worker.
    ready: Vec<Done>,
}

impl Default for Cards {
    fn default() -> Self {
        let (tx, rx) = worker().unzip();
        Self {
            tx,
            rx,
            held: HashMap::new(),
            busy: HashMap::new(),
            ready: Vec::new(),
        }
    }
}

impl Cards {
    /// `slot`'s card for `key`; else its last, while `job` is set.
    pub fn get(
        &mut self,
        slot: u64,
        key: u64,
        now: f64,
        job: impl FnOnce() -> Job,
    ) -> Option<Card> {
        let fresh = self.held.get(&slot).is_some_and(|h| h.key == key);
        if !fresh && self.busy.get(&slot) != Some(&key) {
            let unsent = match &self.tx {
                Some(tx) => tx.send((slot, key, job())).err().map(|e| e.0.2),
                None => Some(job()),
            };
            match unsent {
                Some(job) => self.inline(slot, key, &job),
                None => {
                    self.busy.insert(slot, key);
                }
            }
        }
        let held = self.held.get_mut(&slot)?;
        held.seen = now;
        held.card.clone()
    }

    /// Without a worker the frame sets it, for the next receive.
    fn inline(&mut self, slot: u64, key: u64, job: &Job) {
        let set = set(job);
        self.ready.push(Done { slot, key, set });
        self.busy.insert(slot, key);
    }

    /// Sets cards on the frame; else a thread sets them.
    pub fn set_inline(&mut self, inline: bool) {
        if inline {
            (self.tx, self.rx) = (None, None);
        } else if self.tx.is_none() {
            (self.tx, self.rx) = worker().unzip();
        }
    }

    /// Takes finished cards to the GPU; drops ones long unseen.
    pub fn receive(&mut self, ctx: &Context, gpu: Option<&Gpu>, now: f64) -> bool {
        let mut any = false;
        let mut news = std::mem::take(&mut self.ready);
        news.extend(self.rx.iter().flat_map(Receiver::try_iter));
        for done in news {
            if self.busy.get(&done.slot) == Some(&done.key) {
                self.busy.remove(&done.slot);
            }
            let card = done.set.and_then(|s| upload(ctx, gpu, done.slot, s));
            // A failed set keeps the last card rather than none.
            let last = self.held.remove(&done.slot).and_then(|h| h.card);
            self.held.insert(
                done.slot,
                Held {
                    key: done.key,
                    card: card.or(last),
                    seen: now,
                },
            );
            any = true;
        }
        self.held.retain(|_, h| now - h.seen < KEEP);
        // A card being set wakes the frame that shows it.
        if !self.busy.is_empty() {
            ctx.request_repaint_after_secs(0.05);
        }
        any
    }
}

fn upload(ctx: &Context, gpu: Option<&Gpu>, slot: u64, s: Set) -> Option<Card> {
    let raster = Raster {
        width: s.pixels.width,
        height: s.pixels.height,
        pixels: s.pixels.rgba,
    };
    let texture = match gpu {
        Some(gpu) if s.pdf => PageTexture::Gpu(gpu.upload(&on_white(raster))?),
        Some(gpu) => PageTexture::Gpu(gpu.upload(&raster)?),
        None => {
            let size = [raster.width as usize, raster.height as usize];
            let image = egui::ColorImage::from_rgba_premultiplied(size, &raster.pixels);
            let name = format!("page-ai-{slot:x}");
            PageTexture::Egui(ctx.load_texture(name, image, egui::TextureOptions::LINEAR))
        }
    };
    let rect = |r: [f32; 4]| Rect::from_min_size(pos2(r[0] - PAD, r[1] - PAD), vec2(r[2], r[3]));
    Some(Card {
        texture,
        size: vec2(s.size.0, s.size.1),
        pictures: s.pictures.into_iter().map(rect).collect(),
        emojis: s.emojis.into_iter().map(rect).collect(),
    })
}

/// Over white, so a PDF's filters theme it as they theme its page.
fn on_white(mut raster: Raster) -> Raster {
    for px in raster.pixels.as_chunks_mut::<4>().0 {
        let [.., alpha] = *px;
        let under = 255 - u16::from(alpha);
        for c in px.iter_mut() {
            *c = (u16::from(*c) + under).min(255) as u8;
        }
    }
    raster
}
