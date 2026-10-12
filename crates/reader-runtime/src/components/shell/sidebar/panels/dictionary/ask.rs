//! The panel's ask: the word, the two shores, the packs it rides.

use leptos::prelude::*;
use reader_core::settings::Settings;

use crate::services::dict::{self, Asked, EntryMirror, PackMirror, Pair};

/// How long a keystroke waits before the packs are asked.
pub(super) const SEARCH_DEBOUNCE_MS: u64 = 180;

/// One dropdown row: the value it carries, and the label it shows.
pub(super) type Choice = (Option<String>, String);

/// The panel's ask. One for the window, so no pane owns it.
#[derive(Clone, Copy)]
pub(super) struct Ask {
    /// The word the panel is holding.
    pub query: RwSignal<String>,
    /// The rows the packs answered with.
    pub results: RwSignal<Vec<EntryMirror>>,
    /// Whether an ask has been made since the box was emptied.
    pub asked: RwSignal<bool>,
}

impl Ask {
    /// The window's ask, minted beside the pack rows.
    pub fn window() -> Self {
        let held = dict::panel();
        Self {
            query: held.query,
            results: held.results,
            asked: held.asked,
        }
    }

    /// The ask to send. Reads untracked: a debounce fires outside
    /// any effect.
    pub fn firer(
        self,
        packs: RwSignal<Vec<PackMirror>>,
        pair: Signal<Option<Pair>>,
        seat: Signal<Option<String>>,
    ) -> impl Fn() + 'static {
        let query = self.query;
        let results = self.results;
        let asked = self.asked;
        move || {
            let ask = query.get_untracked().trim().to_string();
            if ask.is_empty() {
                results.set(Vec::new());
                asked.set(false);
                return;
            }
            // The word speaks last: an ask settles only once it is read.
            let settled = pair
                .get_untracked()
                .and_then(|pair| dict::resolve(&packs.get_untracked(), &pair, &ask));
            let from = settled.as_ref().map(|got| got.from.clone());
            let to = settled.as_ref().map(|got| got.to.clone());
            // A pair names the packs it rides; a seat names one by hand.
            let filter = if from.is_some() {
                None
            } else {
                seat.get_untracked().map(|id| vec![id])
            };
            dict::search(ask, filter, from, to, move |entries| {
                results.set(entries);
                asked.set(true);
            });
        }
    }
}

/// The pair, when asked for: the card's language stands in for a target.
pub(super) fn pair(settings: RwSignal<Settings>) -> Signal<Option<Pair>> {
    Signal::derive(move || {
        settings.with(|st| {
            st.dict.pair.then(|| Pair {
                from: st.dict.from.clone(),
                to: st.dict.to.clone().or_else(|| st.dict.default_lang.clone()),
            })
        })
    })
}

/// The seat an ask that names no pair rides: one pack, or every one.
pub(super) fn seat(
    settings: RwSignal<Settings>,
    packs: RwSignal<Vec<PackMirror>>,
) -> Signal<Option<String>> {
    Signal::derive(move || {
        let rows = packs.get();
        settings
            .with(|st| st.dict.langs.first().cloned())
            .filter(|id| rows.iter().any(|pack| pack.built && &pack.id == id))
    })
}

/// What the pair settled on, once the word has had its say.
pub(super) fn settled(
    query: RwSignal<String>,
    pair: Signal<Option<Pair>>,
    packs: RwSignal<Vec<PackMirror>>,
) -> Signal<Option<Asked>> {
    Signal::derive(move || {
        let ask = pair.get()?;
        dict::resolve(&packs.get(), &ask, &query.get())
    })
}

/// The word's own language, when it is not the one being asked.
pub(super) fn otherwise(
    query: RwSignal<String>,
    pair: Signal<Option<Pair>>,
    packs: RwSignal<Vec<PackMirror>>,
    langs: Signal<Vec<String>>,
) -> Signal<Option<String>> {
    Signal::derive(move || {
        let ask = pair.get()?;
        let rows = packs.get();
        let got = dict::resolve(&rows, &ask, &query.get())?;
        let looks = dict_core::detect(&query.get(), &langs.get())
            .unwrap_or_else(|| dict_core::HUB.to_string());
        (looks != got.from).then_some(looks)
    })
}

/// One row per shore, and one that reads the word for itself.
pub(super) fn from_options(langs: Signal<Vec<String>>) -> Signal<Vec<Choice>> {
    Signal::derive(move || {
        let mut rows = vec![(None, "Detect".to_string())];
        for lang in langs.get() {
            rows.push((Some(lang.clone()), dict_core::lang_label(&lang)));
        }
        rows
    })
}

/// Every shore but the one the ask is written in.
pub(super) fn to_options(
    langs: Signal<Vec<String>>,
    settled: Signal<Option<Asked>>,
) -> Signal<Vec<Choice>> {
    Signal::derive(move || {
        let taken = settled.get().map(|got| got.from).unwrap_or_default();
        langs
            .get()
            .into_iter()
            .filter(|lang| *lang != taken)
            .map(|lang| (Some(lang.clone()), dict_core::lang_label(&lang)))
            .collect()
    })
}

/// One row per built pack, and one that names no pack at all.
pub(super) fn pack_options(packs: RwSignal<Vec<PackMirror>>) -> Signal<Vec<Choice>> {
    Signal::derive(move || {
        let mut rows = vec![(None, "Every pack".to_string())];
        for pack in packs.get() {
            if pack.built {
                rows.push((Some(pack.id.clone()), pack.label.clone()));
            }
        }
        rows
    })
}

/// Detect answers on the word: the dropdown shows its choice.
pub(super) fn from_current(
    settled: Signal<Option<Asked>>,
    settings: RwSignal<Settings>,
) -> Signal<Option<String>> {
    Signal::derive(move || {
        settled
            .get()
            .map(|got| got.from)
            .or_else(|| settings.with(|st| st.dict.from.clone()))
    })
}

/// The shore the answers are wanted in; a settled pair outranks the knob.
pub(super) fn to_current(
    settled: Signal<Option<Asked>>,
    settings: RwSignal<Settings>,
) -> Signal<Option<String>> {
    Signal::derive(move || {
        settled
            .get()
            .map(|got| got.to)
            .or_else(|| settings.with(|st| st.dict.to.clone()))
    })
}
