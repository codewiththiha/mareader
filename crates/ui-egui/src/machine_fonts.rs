//! Machine fonts join as fallbacks for letters the bundled faces lack.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::mpsc::{Receiver, Sender, channel};

use egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};
use egui::{Context, FontData, FontFamily, FontId, Id, LayerId, Shape};

use crate::theme::{MEDIUM, SEMIBOLD, SERIF};

/// Texts remembered as checked before the memory starts over.
const SEEN: usize = 20_000;

/// Where texts drawn some other way than as text wait for the frame's end.
fn noted() -> Id {
    Id::new("machine-fonts-noted")
}

/// Asks for faces for `text`, which this frame draws not as a text shape
/// but some other way, as a mesh: the scan of shapes would never see it.
pub fn note(ctx: &Context, text: &str) {
    if text.is_ascii() {
        return;
    }
    ctx.data_mut(|d| {
        let list = d.get_temp_mut_or_default::<Vec<String>>(noted());
        if !list.iter().any(|t| t == text) {
            list.push(text.to_owned());
        }
    });
}

/// Scans drawn text after each frame; a worker asks parley for faces.
pub struct MachineFonts {
    /// Hashes of texts already checked.
    seen: HashSet<u64>,
    /// Whether the loaded faces draw a letter, per letter asked.
    covered: HashMap<char, bool>,
    /// Letters sent to a worker, found or not: never sent twice.
    asked: HashSet<char>,
    /// Letters waiting while a worker runs.
    waiting: String,
    busy: bool,
    /// Workers send on it; browsers have none.
    #[cfg_attr(target_arch = "wasm32", expect(dead_code))]
    tx: Sender<Vec<mareader_reflow::Fallback>>,
    rx: Receiver<Vec<mareader_reflow::Fallback>>,
}

impl Default for MachineFonts {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            seen: HashSet::new(),
            covered: HashMap::new(),
            asked: HashSet::new(),
            waiting: String::new(),
            busy: false,
            tx,
            rx,
        }
    }
}

impl std::fmt::Debug for MachineFonts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MachineFonts({} letters asked)", self.asked.len())
    }
}

impl MachineFonts {
    /// Call once a frame, after everything is drawn.
    pub fn after_frame(&mut self, ctx: &Context) {
        self.take_found(ctx);
        let texts = self.new_texts(ctx);
        for text in &texts {
            for c in text.chars().filter(|&c| worth_asking(c)) {
                if self.asked.contains(&c) || self.waiting.contains(c) {
                    continue;
                }
                let has = *self.covered.entry(c).or_insert_with(|| {
                    ctx.fonts_mut(|f| f.has_glyph(&FontId::proportional(14.0), c))
                });
                if !has {
                    self.waiting.push(c);
                }
            }
        }
        self.ask(ctx);
    }

    /// Faces a worker found join every family, after what is there.
    fn take_found(&mut self, ctx: &Context) {
        while let Ok(found) = self.rx.try_recv() {
            self.busy = false;
            for face in found {
                ctx.add_font(FontInsert {
                    name: face.key,
                    data: FontData {
                        // Borrowed from the mapped file, not copied.
                        font: face.data.into(),
                        index: face.index,
                        tweak: Default::default(),
                    },
                    families: families(),
                });
            }
            // Letters missing before may be drawn now.
            self.covered.clear();
            ctx.request_repaint();
        }
    }

    /// The non-ASCII texts drawn this frame and not checked before.
    fn new_texts(&mut self, ctx: &Context) -> Vec<String> {
        if self.seen.len() > SEEN {
            self.seen.clear();
        }
        let mut layers: Vec<LayerId> = ctx.memory(|m| m.layer_ids().collect());
        layers.push(LayerId::background());
        let mut texts = Vec::new();
        let noted: Vec<String> = ctx.data_mut(|d| d.remove_temp(noted())).unwrap_or_default();
        for text in noted {
            if self.seen.insert(hash(&text)) {
                texts.push(text);
            }
        }
        let seen = &mut self.seen;
        ctx.graphics(|g| {
            for layer in layers {
                for clipped in g.get(layer).into_iter().flat_map(|l| l.all_entries()) {
                    collect(&clipped.shape, seen, &mut texts);
                }
            }
        });
        texts
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn ask(&mut self, ctx: &Context) {
        if self.busy || self.waiting.is_empty() {
            return;
        }
        let letters = std::mem::take(&mut self.waiting);
        self.asked.extend(letters.chars());
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        let worker = std::thread::Builder::new()
            .name("machine-fonts".into())
            .spawn(move || {
                let found = mareader_reflow::fallbacks(&letters);
                if tx.send(found).is_ok() {
                    ctx.request_repaint();
                }
            });
        self.busy = worker.is_ok();
    }

    /// A browser has no machine fonts to offer.
    #[cfg(target_arch = "wasm32")]
    fn ask(&mut self, _ctx: &Context) {
        self.asked.extend(self.waiting.drain(..));
    }
}

fn families() -> Vec<InsertFontFamily> {
    [
        FontFamily::Proportional,
        FontFamily::Monospace,
        FontFamily::Name(MEDIUM.into()),
        FontFamily::Name(SEMIBOLD.into()),
        FontFamily::Name(SERIF.into()),
    ]
    .into_iter()
    .map(|family| InsertFontFamily {
        family,
        priority: FontPriority::Lowest,
    })
    .collect()
}

fn collect(shape: &Shape, seen: &mut HashSet<u64>, out: &mut Vec<String>) {
    match shape {
        Shape::Text(t) => {
            let text = t.galley.text();
            if text.is_ascii() {
                return;
            }
            if seen.insert(hash(text)) {
                out.push(text.to_owned());
            }
        }
        Shape::Vec(shapes) => {
            for s in shapes {
                collect(s, seen, out);
            }
        }
        _ => {}
    }
}

fn hash(text: &str) -> u64 {
    let mut h = std::hash::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

/// Letters a face could draw: not spaces, controls or private icons.
fn worth_asking(c: char) -> bool {
    let private = matches!(u32::from(c), 0xE000..=0xF8FF | 0xF0000..);
    !(c.is_ascii() || c.is_whitespace() || c.is_control() || private || c == '\u{FFFD}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noted_texts_are_checked_once_and_ascii_never() {
        let ctx = Context::default();
        let mut fonts = MachineFonts::default();
        note(&ctx, "မင်္ဂလာပါ");
        note(&ctx, "plain");
        note(&ctx, "မင်္ဂလာပါ");
        assert_eq!(fonts.new_texts(&ctx), vec!["မင်္ဂလာပါ".to_owned()]);
        note(&ctx, "မင်္ဂလာပါ");
        assert!(fonts.new_texts(&ctx).is_empty(), "seen before");
    }

    #[test]
    fn icons_and_spaces_are_never_asked_for() {
        assert!(worth_asking('က'));
        assert!(worth_asking('本'));
        assert!(!worth_asking('a'));
        assert!(!worth_asking('\u{E001}'), "private-use icons stay local");
        assert!(!worth_asking('\u{3000}'), "an ideographic space");
    }
}
