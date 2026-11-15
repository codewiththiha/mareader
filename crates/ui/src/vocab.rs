//! Words above the reader's level, shaded on the pages on screen;
//! see `docs/study.md` § Vocabulary.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use egui::{CursorIcon, Id, Pos2, Rect, Ui};
use mareader_cefr::{Part, Role, State, Ticket, Vocabulary};
use mareader_pdf::Bounds;
use mareader_session::{DocId, TextLayer};
use mareader_settings::{CefrMark, Reserved};

use crate::command::Download;
use crate::selection::Pages;
use crate::study::marks::Under;
use crate::study::words;

/// Shaded words per page at most; a book's stream page holds many.
const PAGE_CAP: usize = 5_000;
/// How long the pointer rests on a word before it is looked up.
const DWELL: f64 = 0.16;
/// Pages remembered; past this the least recently used goes.
const MEMO_CAP: usize = 32;
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

/// One page's hard words: each one's characters and its run of `boxes`.
#[derive(Default)]
struct Words {
    words: Vec<(Range<usize>, Range<usize>, u8)>,
    boxes: Vec<Bounds>,
}

/// One page's hard words and their boxes, while nothing changed.
struct Memo {
    layer: usize,
    level: u8,
    generation: u64,
    /// When it was last asked for, to let the stalest go.
    used: u64,
    words: Arc<Words>,
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

#[derive(Default)]
pub struct Vocab {
    service: Vocabulary,
    memo: HashMap<(DocId, usize), Memo>,
    /// Counts memo asks, to know the least recently used.
    asks: u64,
    /// The role asked for the open card, and its word, if any.
    role: Option<(Ticket, String)>,
}

impl Vocab {
    /// Keeps the dataset in `dir`; `waker` repaints the app as work lands.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn attach(
        &mut self,
        dir: std::path::PathBuf,
        waker: mareader_cefr::Waker,
    ) -> Result<(), String> {
        self.memo.clear();
        self.service.attach(dir, waker)
    }

    /// Answers on this thread, as tests need.
    pub fn set_inline(&mut self, inline: bool) {
        self.service.set_inline(inline);
    }

    pub fn attached(&self) -> bool {
        self.service.attached()
    }

    pub fn state(&self, part: Part) -> &State {
        self.service.state(part)
    }

    pub fn words(&self) -> Option<u64> {
        self.service.words()
    }

    pub fn act(&mut self, action: Download) {
        #[cfg(not(target_arch = "wasm32"))]
        match action {
            Download::Start => self.service.download(),
            Download::Pause => self.service.pause(),
            Download::Cancel => self.service.cancel(),
            Download::Remove => self.service.remove(),
        }
        let _ = action;
    }

    /// Whether word levels can be asked for.
    pub fn ready(&self) -> bool {
        *self.service.state(Part::Dataset) == State::Ready
    }

    /// Whether word levels are on their way: fetching or converting.
    pub fn coming(&self) -> bool {
        let state = self.service.state(Part::Dataset);
        matches!(state, State::Fetching { .. } | State::Converting)
    }

    /// Each key's band where known; the rest are asked for.
    pub fn bands(&mut self, keys: &[String]) -> Vec<Option<u8>> {
        self.service.bands(keys)
    }

    /// Asks for the open card's word's role; a newer card's replaces it.
    pub fn ask_role(&mut self, word: &str, sentence: &str) {
        let ticket = self.service.role(word, sentence);
        self.role = ticket.map(|t| (t, word.to_owned()));
    }

    /// Takes in answers; the open card's word and role if it came.
    pub fn poll(&mut self) -> (bool, Option<(String, Role)>) {
        let polled = self.service.poll();
        let mut role = None;
        for (ticket, answer) in polled.roles {
            if let Some((asked, word)) = self.role.take() {
                match asked == ticket {
                    true => role = answer.map(|r| (word, r)),
                    false => self.role = Some((asked, word)),
                }
            }
        }
        (polled.changed, role)
    }

    /// `page`'s hard words, planned again only when something changed.
    fn page(&mut self, doc: DocId, page: usize, layer: &Arc<TextLayer>, level: u8) -> Arc<Words> {
        let id = Arc::as_ptr(layer).addr();
        let generation = self.service.generation();
        self.asks += 1;
        if let Some(m) = self.memo.get_mut(&(doc, page))
            && (m.layer, m.level, m.generation) == (id, level, generation)
        {
            m.used = self.asks;
            return Arc::clone(&m.words);
        }
        let (text, index) = words::read(layer);
        let spans = self.service.plan_banded_chars(&text, level, PAGE_CAP);
        let mut words = Words::default();
        for (s, band) in spans {
            let first = index.get(s.start).copied().flatten();
            let last = s
                .end
                .checked_sub(1)
                .and_then(|e| index.get(e).copied().flatten());
            let (Some(first), Some(last)) = (first, last) else {
                continue;
            };
            let start = words.boxes.len();
            words.boxes.extend(layer.text.line_boxes(first..last + 1));
            words
                .words
                .push((first..last + 1, start..words.boxes.len(), band));
        }
        words.words.shrink_to_fit();
        words.boxes.shrink_to_fit();
        let words = Arc::new(words);
        let stalest = (self.memo.len() >= MEMO_CAP)
            .then(|| self.memo.iter().min_by_key(|(_, m)| m.used))
            .flatten()
            .map(|(key, _)| *key);
        if let Some(key) = stalest {
            self.memo.remove(&key);
        }
        let memo = Memo {
            layer: id,
            level,
            generation,
            used: self.asks,
            words: Arc::clone(&words),
        };
        self.memo.insert((doc, page), memo);
        words
    }

    /// The shaded words on the pages on screen.
    pub fn place(&mut self, look: &Look, pages: &Pages<'_>) -> Shading {
        let mut out = Shading::default();
        for sheet in pages.sheets.iter().filter(|s| s.shown.is_positive()) {
            let Some(layer) = pages.texts.get(pages.doc, sheet.page) else {
                continue;
            };
            let words = self.page(pages.doc, sheet.page, &layer, look.level);
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
