//! Typography, retypesetting, insert updates, and document lifecycle.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use mareader_session::{
    Anchor, DocId, Document, Entry, Inserts, OpenTarget, Typography, typography,
};
use mareader_settings::TextSettings;
use mareader_workspace::PaneId;

use super::state::{Sink, Slot, State};
use crate::reading::Place;
use crate::texts::TextLayers;

impl State {
    /// Each pane's book type: its `own` text, or `window`.
    pub(crate) fn set_text(
        &mut self,
        window: &TextSettings,
        own: &[(PaneId, &TextSettings)],
        said: Option<(PaneId, Anchor)>,
        sink: &mut dyn Sink,
    ) {
        let fresh = |old: Option<&Arc<Typography>>, text: &TextSettings| {
            let typo = typography(text);
            match old {
                Some(old) if **old == typo => Arc::clone(old),
                _ => Arc::new(typo),
            }
        };
        self.window = fresh(Some(&self.window), window);
        let own = own
            .iter()
            .map(|(pane, text)| (*pane, fresh(self.own.get(pane), text)));
        self.own = own.collect();
        self.retype(said, sink);
    }

    /// The results `pane`'s book should show in its pages.
    pub fn set_inserts(
        &mut self,
        pane: PaneId,
        inserts: Arc<Inserts>,
        said: Option<(PaneId, Anchor)>,
        sink: &mut dyn Sink,
    ) {
        if self.inserts.get(&pane).map_or(0, |i| i.key) == inserts.key {
            return;
        }
        match inserts.key {
            0 => self.inserts.remove(&pane),
            _ => self.inserts.insert(pane, inserts),
        };
        self.retype(said, sink);
    }

    /// Updates a pane's paged sheet size and retypesets when changed.
    pub(crate) fn set_sheet(
        &mut self,
        pane: PaneId,
        sheet: Option<(f32, f32)>,
        said: Option<(PaneId, Anchor)>,
        sink: &mut dyn Sink,
    ) {
        match sheet {
            Some(sheet) => self.sheets.insert(pane, sheet),
            None => self.sheets.remove(&pane),
        };
        self.retype(said, sink);
    }

    pub(super) fn typo(&self, pane: PaneId) -> &Arc<Typography> {
        self.own.get(&pane).unwrap_or(&self.window)
    }

    /// `pane`'s book type on its pane-sized page, if it has one.
    pub(super) fn want(&self, pane: PaneId) -> Arc<Typography> {
        let typo = self.typo(pane);
        match self.sheets.get(&pane).copied() {
            Some(sheet) if typo.sheet != Some(sheet) => Arc::new(Typography {
                sheet: Some(sheet),
                ..(**typo).clone()
            }),
            _ => Arc::clone(typo),
        }
    }

    /// Hands what `old` drew to `new` once no pane shows `old`.
    pub(super) fn hand_over(&mut self, old: DocId, new: DocId, sink: &mut dyn Sink) {
        let shown =
            (self.panes.values()).any(|s| matches!(s, Slot::Open(place) if place.doc == old));
        if !shown {
            self.adopt_page_rasters(old, new, sink);
            self.adopt_text_layers(old, new);
        }
    }

    pub(super) fn adopt_page_rasters(&self, old: DocId, new: DocId, sink: &mut dyn Sink) {
        let Some((was, is)) = self.adoption_docs(old, new) else {
            return;
        };
        sink.adopt_pages(old, new, &was, &is);
    }

    fn adopt_text_layers(&mut self, old: DocId, new: DocId) {
        let Some((was, is)) = self.adoption_docs(old, new) else {
            return;
        };
        if !was.is_complete() && is.is_complete() {
            for page in self.texts.pages_of(old) {
                let Some(mapped) = was
                    .same_page(page, &is)
                    .or_else(|| was.fallback_page(page, &is))
                else {
                    continue;
                };
                if self.texts.get(new, mapped).is_some() {
                    continue;
                }
                if let Ok(layer) = is.text_layer(mapped) {
                    self.texts.insert(new, mapped, Ok(Arc::new(layer)));
                }
            }
            self.texts.forget(old);
            return;
        }
        self.texts.adopt(old, new, |page| was.same_page(page, &is));
    }

    fn adoption_docs(&self, old: DocId, new: DocId) -> Option<(Arc<Document>, Arc<Document>)> {
        let session = self.session.as_ref()?;
        let (was, is) = (session.get(old)?, session.get(new)?);
        Some((Arc::clone(&was.doc), Arc::clone(&is.doc)))
    }

    /// Moves each book whose type is stale onto a twin in its type, or asks.
    pub(super) fn retype(&mut self, said: Option<(PaneId, Anchor)>, sink: &mut dyn Sink) {
        let wants: BTreeMap<PaneId, Arc<Typography>> =
            self.panes.keys().map(|p| (*p, self.want(*p))).collect();
        let Some(session) = &mut self.session else {
            return;
        };
        let (mut moves, mut asks, mut cancels) = (Vec::new(), Vec::new(), Vec::new());
        for (pane, slot) in &self.panes {
            let Slot::Open(place) = slot else {
                continue;
            };
            let Some(want) = wants.get(pane) else {
                continue;
            };
            let inserts = self.inserts.get(pane).cloned().unwrap_or_default();
            let Some(entry) = session.get(place.doc) else {
                continue;
            };
            let key = inserts.key;
            let looks = |e: &Entry| *e.typo == **want && e.doc.inserts_key() == key;
            if !entry.doc.is_reflowable() {
                continue;
            }
            if looks(entry) {
                if self.asked.remove(pane).is_some() && !entry.doc.is_windowed() {
                    cancels.push(pane.0);
                }
                continue;
            }
            let twin = self.panes.values().find_map(|s| match s {
                Slot::Open(place) => {
                    let e = session.get(place.doc)?;
                    let same = entry.path.is_some() && e.path == entry.path;
                    (same && looks(e) && e.doc.is_complete()).then_some(place.doc)
                }
                Slot::Opening(_) => None,
            });
            match twin {
                Some(twin) => moves.push((*pane, place.doc, twin)),
                None => {
                    let target = if self.progressive {
                        preview_target(&self.panes, &self.texts, (*pane, place, entry), said, sink)
                    } else {
                        None
                    };
                    let prime = if self.progressive {
                        sink.prime_hint(*pane, place, &entry.doc)
                    } else {
                        None
                    };
                    asks.push((*pane, place.doc, (Arc::clone(want), inserts, target, prime)));
                }
            }
        }
        for slot in cancels {
            session.cancel_retypeset(slot);
        }
        for (pane, doc, (want, inserts, target, prime)) in asks {
            let key = inserts.key;
            let same =
                |(d, t, k): &(DocId, Arc<Typography>, u64)| *d == doc && **t == *want && *k == key;
            if self.asked.values().any(same) {
                continue;
            }
            session.reset_primed(
                doc,
                Arc::clone(&want),
                Some(inserts),
                pane.0,
                (target, prime),
            );
            self.asked.insert(pane, (doc, want, key));
        }
        for (pane, old, new) in moves {
            self.retarget(pane, old, new, said, sink);
            self.hand_over(old, new, sink);
            self.release(old, sink);
        }
    }

    /// Moves `pane` from `old` to `new`, the same book in other type.
    pub(super) fn retarget(
        &mut self,
        pane: PaneId,
        old: DocId,
        new: DocId,
        said: Option<(PaneId, Anchor)>,
        sink: &mut dyn Sink,
    ) {
        let Some(session) = &self.session else {
            return;
        };
        let was = session.get(old).map(|e| &*e.doc);
        let is = session.get(new).map(|e| &*e.doc);
        if let Some(Slot::Open(place)) = self.panes.get_mut(&pane) {
            sink.retarget(pane, place, old, new, was, is, &self.texts, said);
        }
        self.asked.remove(&pane);
    }

    pub(super) fn release(&mut self, doc: DocId, sink: &mut dyn Sink) {
        let shown =
            (self.panes.values()).any(|s| matches!(s, Slot::Open(place) if place.doc == doc));
        if !shown {
            if let Some(s) = &mut self.session {
                s.close(doc);
            }
            sink.forget_pages(doc);
            self.texts.forget(doc);
            self.papers.forget(doc);
        }
    }

    /// The open document read from `file`, preferably set in `typo`.
    pub(super) fn shown(&self, file: &Path, typo: &Typography) -> Option<DocId> {
        let session = self.session.as_ref()?;
        let open = self.panes.values().filter_map(|s| match s {
            Slot::Open(place) => {
                let entry = session.get(place.doc)?;
                (entry.path.as_deref() == Some(file)).then_some((place.doc, entry))
            }
            Slot::Opening(_) => None,
        });
        let (mut first, mut matched) = (None, None);
        for (doc, entry) in open {
            first = first.or(Some(doc));
            let plain = *entry.typo == *typo && entry.doc.inserts_key() == 0;
            if plain || !entry.doc.is_reflowable() {
                matched = Some(doc);
                break;
            }
        }
        matched.or(first)
    }
}

fn preview_target(
    panes: &BTreeMap<PaneId, Slot>,
    texts: &TextLayers,
    (pane, place, entry): (PaneId, &Place, &Entry),
    said: Option<(PaneId, Anchor)>,
    sink: &dyn Sink,
) -> Option<OpenTarget> {
    let shared = panes
        .values()
        .filter(|s| matches!(s, Slot::Open(other) if other.doc == place.doc))
        .count()
        > 1;
    if shared {
        return None;
    }
    let anchor = said
        .filter(|(p, _)| *p == pane)
        .map(|(_, a)| a)
        .or_else(|| sink.reading_anchor(pane, place, &entry.doc, texts));
    let spine = anchor.and_then(|a| entry.doc.spine_place(a));
    Some(OpenTarget {
        page: entry.doc.display_page(place.current),
        pages: entry.doc.display_page_count(),
        place: anchor,
        spine,
    })
}
