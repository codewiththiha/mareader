//! The Dictionary page's pickers: which dictionary, or which two languages.

use egui::{Response, Ui, vec2};
use mareader_dict::{AI_PACK, HUB, Pack, ai_language, ai_only, language, languages_with};
use mareader_settings::DictionarySettings;

use super::look;
use super::page::{CONTROL, SWAP_W};
use crate::icons::Icon;
use crate::theme::Tokens;
use crate::widgets;

/// The AI dictionary's name among the packs.
pub const AI_LABEL: &str = "AI dictionary";
/// The AI side menu's first row: every language it keeps.
pub const ALL_AI: &str = "All AI words";

/// One language the AI dictionary keeps, as its side menu names it.
pub fn ai_pair(lang: &str) -> String {
    format!("{} → {}", language(HUB), language(lang))
}

/// What the page can search: the packs, then the AI dictionary's languages.
#[derive(Clone, Copy)]
pub struct Shelf<'a> {
    pub packs: &'a [&'static Pack],
    pub ai: &'a [String],
}

impl<'a> Shelf<'a> {
    /// How many dictionaries answer; the AI dictionary counts as one.
    pub fn count(&self) -> usize {
        self.packs.len() + usize::from(!self.ai.is_empty())
    }

    /// Every language to pick, the AI dictionary's after the packs'.
    pub fn languages(&self) -> Vec<&'a str> {
        languages_with(self.packs, self.ai)
    }

    /// The one dictionary `s` searches, by label; none for every one.
    pub fn chosen(&self, s: &DictionarySettings) -> Option<String> {
        let id = s.langs.first()?;
        let pack = self.packs.iter().find(|p| p.id == id);
        let kept = |l: &str| self.ai.iter().any(|a| a == l);
        let ai = match ai_language(id) {
            Some(l) => kept(l).then(|| format!("AI · {}", ai_pair(l))),
            None => (id == AI_PACK && !self.ai.is_empty()).then(|| AI_LABEL.to_owned()),
        };
        pack.map(|p| p.label()).or(ai)
    }

    /// Each dictionary, by id and label, the AI dictionary last.
    pub fn each(&self) -> Vec<(&'static str, String)> {
        let ai = (!self.ai.is_empty()).then(|| (AI_PACK, AI_LABEL.to_owned()));
        let packs = self.packs.iter().map(|p| (p.id, p.label()));
        packs.chain(ai).collect()
    }
}

pub fn pack_box(ui: &mut Ui, s: &mut DictionarySettings, shelf: Shelf<'_>, width: f32, t: &Tokens) {
    let chosen = shelf.chosen(s);
    let every = "Every dictionary";
    let shown = chosen.clone().unwrap_or_else(|| every.to_owned());
    let id = (ui.make_persistent_id("dict-pack"), "Dictionary to search");
    look::picker(ui, id, vec2(width, CONTROL), &shown, t, |ui| {
        if look::option(ui, chosen.is_none(), every, t) {
            s.langs.clear();
        }
        for (id, label) in shelf.each() {
            let on = chosen.is_some() && s.langs.first().is_some_and(|c| c == id);
            if id == AI_PACK {
                ai_row(ui, s, shelf.ai, t);
            } else if look::option(ui, on, &label, t) {
                s.langs = vec![id.to_owned()];
            }
        }
    });
}

/// The AI dictionary: a click searches all it keeps; its side menu,
/// opened on hover, one language alone, no pack beside it.
fn ai_row(ui: &mut Ui, s: &mut DictionarySettings, ai: &[String], t: &Tokens) {
    let picked = s.langs.first().cloned();
    let is = |id: &str| picked.as_deref() == Some(id);
    let on = is(AI_PACK) || picked.as_deref().and_then(ai_language).is_some();
    let mut pick = None;
    let side = |ui: &mut Ui| {
        let rows = std::iter::once((AI_PACK.to_owned(), ALL_AI.to_owned()));
        let langs = ai.iter().map(|l| (ai_only(l), ai_pair(l)));
        for (id, label) in rows.chain(langs) {
            if look::option(ui, is(&id), &label, t) {
                widgets::submenu_chosen(ui.ctx());
                pick = Some(id);
            }
        }
    };
    let paint = |ui: &mut Ui, r: &Response, open| look::branch(ui, r, (on, open), AI_LABEL, t);
    let row = (AI_LABEL, look::OPTION_H);
    if widgets::submenu_with(ui, row, AI_SIDE_W, side, paint).clicked() {
        pick = Some(AI_PACK.to_owned());
        ui.close();
    }
    if let Some(id) = pick {
        s.langs = vec![id];
    }
}

/// The AI side menu's width: room for its longest pair.
const AI_SIDE_W: f32 = 220.0;

pub fn languages_row(
    ui: &mut Ui,
    s: &mut DictionarySettings,
    shelf: Shelf<'_>,
    width: f32,
    t: &Tokens,
) {
    let langs = shelf.languages();
    let names = ("dict-from", "Translate from");
    language_box(ui, names, width, (&mut s.from, &langs), t);
    let swap = look::icon(ui, Icon::Swap, "Swap languages", vec2(SWAP_W, CONTROL));
    if swap.clicked() {
        std::mem::swap(&mut s.from, &mut s.to);
    }
    let names = ("dict-to", "Translate to");
    language_box(ui, names, width, (&mut s.to, &langs), t);
}

/// A language picker; `id` is its salt and its spoken name.
fn language_box(
    ui: &mut Ui,
    (salt, name): (&str, &str),
    width: f32,
    (value, langs): (&mut Option<String>, &[&str]),
    t: &Tokens,
) {
    let shown = value.as_deref().map_or("Automatic".to_owned(), language);
    let id = (ui.make_persistent_id(salt), name);
    look::picker(ui, id, vec2(width, CONTROL), &shown, t, |ui| {
        if look::option(ui, value.is_none(), "Automatic", t) {
            *value = None;
        }
        for l in langs {
            let on = value.as_deref() == Some(*l);
            if look::option(ui, on, &language(l), t) {
                *value = Some((*l).to_owned());
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ai_dictionary_is_one_more_dictionary_to_pick() {
        let my: Vec<&'static Pack> = mareader_dict::PACKS
            .iter()
            .filter(|p| p.id == "mcfnlp-en-my")
            .collect();
        let ai = ["ru".to_owned()];
        let shelf = Shelf {
            packs: &my,
            ai: &ai,
        };
        assert_eq!(shelf.count(), 2);
        assert_eq!(shelf.languages(), ["en", "my", "ru"]);
        let ids: Vec<&str> = shelf.each().into_iter().map(|(id, _)| id).collect();
        assert_eq!(ids, ["mcfnlp-en-my", AI_PACK]);
        let s = DictionarySettings {
            langs: vec![AI_PACK.to_owned()],
            ..DictionarySettings::default()
        };
        assert_eq!(shelf.chosen(&s).as_deref(), Some(AI_LABEL));
        let bare = Shelf {
            packs: &my,
            ai: &[],
        };
        assert_eq!(bare.chosen(&s), None, "no AI dictionary yet");
    }
}
