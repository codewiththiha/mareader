//! One answer: the headword, its senses, and the pack that carried it.

use leptos::prelude::*;

use crate::services::dict::{EntryMirror, PackMirror};

/// A row's two lines: the requested side leads, the source follows.
fn faces(entry: &EntryMirror, packs: &[PackMirror], wanted: Option<&str>) -> (String, String) {
    let pack = packs.iter().find(|pack| pack.id == entry.pack);
    match (wanted, pack) {
        (Some(to), Some(pack)) if pack.target == *to => {
            (entry.definition.clone(), entry.word.clone())
        }
        (Some(to), Some(pack)) if pack.source == *to => {
            (entry.word.clone(), entry.definition.clone())
        }
        _ => (entry.word.clone(), entry.definition.clone()),
    }
}

#[component]
pub(super) fn AnswerRow(
    entry: EntryMirror,
    /// Every pack's row: which side of a pair this answer came from.
    packs: Vec<PackMirror>,
    /// The shore the ask named; `None` leaves the word in the lead.
    wanted: Option<String>,
) -> impl IntoView {
    let (head, tail) = faces(&entry, &packs, wanted.as_deref());
    let tags = entry.tags.join(" · ");
    let fuzzy = entry.word_match == "fuzzy";

    view! {
        <div class="rounded-lg border border-line px-3 py-2">
            <div class="flex items-baseline gap-2">
                <span class="min-w-0 truncate text-sm font-medium text-ink">{head}</span>
                <span class="shrink-0 text-xs text-muted">{tags}</span>
                {fuzzy
                    .then(|| {
                        view! {
                            <span class="ml-auto shrink-0 rounded-full bg-line/60 px-1.5 text-xs text-muted">
                                "≈"
                            </span>
                        }
                    })}
            </div>
            <div class="pt-0.5 text-xs text-muted">{tail}</div>
            {entry
                .romanization
                .map(|rom| view! { <div class="text-xs text-muted">{rom}</div> })}
            {entry
                .sense
                .map(|sense| view! { <div class="text-xs text-muted italic">{sense}</div> })}
            {entry
                .via
                .map(|via| {
                    view! { <div class="pt-0.5 text-xs text-muted">{"via "}{via}</div> }
                })}
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One pack's row: the two shores it joins, by id.
    fn pack(id: &str, source: &str, target: &str) -> PackMirror {
        PackMirror {
            id: id.to_string(),
            source: source.to_string(),
            target: target.to_string(),
            built: true,
            ..PackMirror::default()
        }
    }

    /// One answer: the word on the pack's source side.
    fn entry(word: &str, definition: &str, pack: &str) -> EntryMirror {
        EntryMirror {
            word: word.to_string(),
            definition: definition.to_string(),
            pack: pack.to_string(),
            ..EntryMirror::default()
        }
    }

    #[test]
    fn an_unasked_row_leads_with_the_word() {
        let packs = vec![pack("a-en-jp", "en", "jp")];
        let row = entry("light", "အလင်း", "a-en-jp");
        assert_eq!(faces(&row, &packs, None).0, "light");
    }

    #[test]
    fn a_pair_answers_in_the_shore_it_named() {
        let packs = vec![pack("a-en-jp", "en", "jp")];
        let row = entry("light", "အလင်း", "a-en-jp");
        let (head, tail) = faces(&row, &packs, Some("jp"));
        assert_eq!(head, "အလင်း");
        assert_eq!(tail, "light");
    }

    #[test]
    fn a_pair_asked_backwards_leads_with_the_word_again() {
        let packs = vec![pack("a-en-jp", "en", "jp")];
        let row = entry("light", "အလင်း", "a-en-jp");
        assert_eq!(faces(&row, &packs, Some("en")).0, "light");
    }

    #[test]
    fn a_row_whose_pack_is_gone_keeps_the_word_in_the_lead() {
        let row = entry("light", "အလင်း", "a-en-jp");
        assert_eq!(faces(&row, &[], Some("jp")).0, "light");
    }
}
