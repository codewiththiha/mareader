//! Words above the reader's level, shaded on the pages on screen;
//! see `docs/study.md` § Vocabulary.

use std::ops::Range;

use egui::{CursorIcon, Id, Pos2, Rect, Ui};
use mareader_settings::{CefrMark, Reserved};

use crate::selection::Pages;
use crate::study::marks::Under;

pub(crate) use mareader_engine::vocab::{ShadingSource, Vocab};

/// How long the pointer rests on a word before it is looked up.
const DWELL: f64 = 0.16;
/// How much stronger the word under the pointer is drawn.
const HOVER_LIFT: f32 = 0.2;

/// How shading looks and acts in a pane, from the settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    /// The reader's band; words above it are shaded.
    pub level: u8,
    pub mark: CefrMark,
    /// A click on a shaded word explains it.
    pub click: bool,
    /// Resting on a shaded word looks it up.
    pub hover: bool,
    pub ink: Reserved,
    pub opacity: f32,
}

/// A shaded word on screen.
#[derive(Clone, Debug, PartialEq)]
pub struct Shaded {
    pub page: usize,
    /// Its characters in the page's text layer.
    pub chars: Range<usize>,
    /// The word's highest known CEFR band.
    pub band: u8,
    bounds: Rect,
    /// Its line boxes in [`Shading`]'s list.
    rects: Range<usize>,
}

impl Shaded {
    pub fn bounds(&self) -> Rect {
        self.bounds
    }
}

/// The shaded words on screen, their line boxes in one list.
#[derive(Debug, Default)]
pub struct Shading {
    words: Vec<Shaded>,
    rects: Vec<Rect>,
}

impl Shading {
    /// Word `w`'s line boxes.
    fn rects_of(&self, w: &Shaded) -> &[Rect] {
        self.rects.get(w.rects.clone()).unwrap_or_default()
    }
}

impl std::ops::Deref for Shading {
    type Target = [Shaded];
    fn deref(&self) -> &[Shaded] {
        &self.words
    }
}

/// The shaded words on the pages on screen.
pub fn place(vocab: &mut dyn ShadingSource, look: &Look, pages: &Pages<'_>) -> Shading {
    let mut out = Shading::default();
    for sheet in pages.sheets.iter().filter(|s| s.shown.is_positive()) {
        let Some(layer) = pages.texts.get(pages.doc, sheet.page) else {
            continue;
        };
        let words = vocab.page(pages.doc, sheet.page, &layer, look.level);
        for (chars, boxes, band) in &words.words {
            let start = out.rects.len();
            let boxes = words.boxes.get(boxes.clone()).unwrap_or_default();
            let on_screen = boxes
                .iter()
                .map(|b| sheet.to_screen(*b).intersect(sheet.shown));
            out.rects.extend(on_screen.filter(|r| r.is_positive()));
            let rects = out.rects.get(start..).unwrap_or_default();
            if let Some(bounds) = rects.iter().copied().reduce(|a, b| a.union(b)) {
                out.words.push(Shaded {
                    page: sheet.page,
                    chars: chars.clone(),
                    band: *band,
                    bounds,
                    rects: start..out.rects.len(),
                });
            }
        }
    }
    out
}

/// Each shaded word in the reserved look; the one under the pointer
/// stronger.
pub fn add(shaded: &Shading, look: &Look, hovered: Option<usize>, under: &mut Under) {
    for (i, word) in shaded.iter().enumerate() {
        let lift = if hovered == Some(i) { HOVER_LIFT } else { 0.0 };
        for r in shaded.rects_of(word) {
            match look.mark {
                CefrMark::Custom => {
                    under.add(*r, look.ink.pen, look.ink.color, look.opacity + lift)
                }
                style => under.level_mark(*r, word.band, style, lift),
            }
        }
    }
}

/// The shaded word under `p`.
pub fn hit(shaded: &Shading, p: Pos2) -> Option<usize> {
    let on = |w: &Shaded| shaded.rects_of(w).iter().any(|r| r.contains(p));
    shaded.iter().position(on)
}

/// The shaded word the pointer rests on, if it is free to rest.
pub fn hovered(ui: &Ui, shaded: &Shading) -> Option<usize> {
    let (pos, down) = ui.input(|i| (i.pointer.hover_pos(), i.pointer.any_down()));
    let pos = pos.filter(|_| !down && ui.rect_contains_pointer(ui.clip_rect()))?;
    hit(shaded, pos)
}

/// What a rest is on, when it began and whether it was told.
type Rest = Option<((usize, usize), f64, bool)>;

/// The word the pointer has rested on long enough, once per rest.
pub fn dwell(
    ui: &Ui,
    id: Id,
    shaded: &[Shaded],
    hovered: Option<usize>,
    look: &Look,
) -> Option<usize> {
    if hovered.is_some() && look.click {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let now = ui.input(|i| i.time);
    let on = hovered.and_then(|i| Some((i, shaded.get(i)?)));
    let key = on.map(|(_, w)| (w.page, w.chars.start));
    let rest: Rest = ui.data(|d| d.get_temp(id)).flatten();
    let (since, told) = match rest {
        Some((k, since, told)) if Some(k) == key => (since, told),
        _ => (now, false),
    };
    let (Some(key), Some((index, _)), true) = (key, on, look.hover) else {
        ui.data_mut(|d| d.insert_temp::<Rest>(id, None));
        return None;
    };
    let ready = !told && now - since >= DWELL;
    ui.data_mut(|d| d.insert_temp::<Rest>(id, Some((key, since, told || ready))));
    if !told && !ready {
        ui.ctx()
            .request_repaint_after_secs((DWELL - (now - since)) as f32);
    }
    ready.then_some(index)
}
