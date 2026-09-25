//! The suggestion list under the library's search bar: the books a half-typed
//! query is probably about, best first, matched characters lit up.
//!
//! A panel, not a page: the shelf filters live behind it, so this answers
//! "take me to that book" where the grid answers "narrow this shelf".

use leptos::prelude::*;

use library_core::query::Suggestion;

use crate::features::library::cover_thumb::CoverThumb;
use crate::state::AppState;

/// Spans outside the text are ignored rather than trusted: a highlight is a
/// courtesy, and a courtesy that panics on a stale span takes the bar with
/// it.
fn pieces(text: &str, spans: &[(usize, usize)]) -> Vec<(String, bool)> {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<(String, bool)> = Vec::with_capacity(spans.len() * 2 + 1);
    let mut at = 0usize;
    let mut si = 0usize;
    while at < chars.len() {
        while si < spans.len() && spans[si].0 < at {
            si += 1;
        }
        if si < spans.len() && spans[si].0 == at && spans[si].1 > at {
            let end = spans[si].1.min(chars.len());
            out.push((chars[at..end].iter().collect(), true));
            at = end;
            si += 1;
        } else {
            let end = spans
                .get(si)
                .map(|s| s.0.max(at + 1))
                .unwrap_or(chars.len())
                .min(chars.len());
            out.push((chars[at..end].iter().collect(), false));
            at = end;
        }
    }
    out
}

fn highlighted(text: &str, spans: &[(usize, usize)]) -> Vec<AnyView> {
    pieces(text, spans)
        .into_iter()
        .map(|(chunk, lit)| {
            if lit {
                view! { <span class="lib-suggest-hit">{chunk}</span> }.into_any()
            } else {
                chunk.into_any()
            }
        })
        .collect()
}

#[component]
pub(crate) fn SearchSuggestions(
    state: AppState,
    suggestions: ReadSignal<Vec<Suggestion>>,
    active: RwSignal<usize>,
    pick: Callback<String>,
) -> impl IntoView {
    view! {
        <div>
            <div id="lib-suggest-list" role="listbox" aria-label="Search suggestions" class="lib-suggest-list">
                <For
                    each=move || {
                        suggestions.get().into_iter().enumerate().collect::<Vec<_>>()
                    }
                    // Keyed by the id and its spans: a suggestion whose lit
                    // characters never grow as the query does is a row that
                    // stopped listening.
                    key=|(_, s)| {
                        (
                            s.book.id.clone(),
                            s.title_spans.clone(),
                            s.author_spans.clone(),
                            s.path_spans.clone(),
                        )
                    }
                    children=move |(at, s)| {
                        let path = s.book.path().to_string();
                        let title = s.book.title();
                        let fallback_letter = title.chars().next().unwrap_or('?').to_string();
                        let author = s.book.author();
                        // The author when the book has one; the address when
                        // that is what the query hit; nothing otherwise — a row
                        // whose title is its stem would say the stem twice.
                        let (sub_text, sub_spans) = match (&author, !s.path_spans.is_empty()) {
                            (Some(a), _) => (a.clone(), s.author_spans.clone()),
                            (None, true) => (path.clone(), s.path_spans.clone()),
                            (None, false) => (String::new(), Vec::new()),
                        };
                        let pick_id = s.book.id.clone();
                        let row_id = format!("lib-sug-{at}");

                        view! {
                            <div
                                id=row_id
                                role="option"
                                aria-selected=move || active.get() == at
                                title=path
                                class="lib-suggest-row"
                                class=("lib-suggest-row-active", move || active.get() == at)
                                on:pointerdown=move |ev: leptos::ev::PointerEvent| {
                                    ev.prevent_default();
                                    pick.run(pick_id.clone());
                                }
                                on:pointerenter=move |_| active.set(at)
                            >
                                <span class="lib-suggest-thumb" aria-hidden="true">
                                    <CoverThumb
                                        state=state
                                        path=Signal::stored(path.clone())
                                        alt=Signal::stored(String::new())
                                        img_class=""
                                        fallback=Callback::new(move |_| {
                                            view! { <span>{fallback_letter.clone()}</span> }
                                                .into_any()
                                        })
                                    />
                                </span>
                                <span class="lib-suggest-text">
                                    <span class="lib-suggest-title">
                                        {highlighted(&title, &s.title_spans)}
                                    </span>
                                    {(!sub_text.is_empty()).then(|| {
                                        view! {
                                            <span class="lib-suggest-sub">
                                                {highlighted(&sub_text, &sub_spans)}
                                            </span>
                                        }
                                    })}
                                </span>
                            </div>
                        }
                    }
                />
            </div>
            <div class="lib-suggest-foot" aria-hidden="true">
                "Choose with ↑ ↓ · Enter goes to the book · Esc closes"
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::pieces;

    #[test]
    fn an_unlit_text_is_one_plain_piece() {
        assert_eq!(pieces("Dune", &[]), vec![("Dune".to_string(), false)]);
    }

    #[test]
    fn hits_split_the_text_and_keep_every_character() {
        let cut = pieces("Mathematical Proofs", &[(0, 1), (2, 4), (13, 14)]);
        assert_eq!(
            cut,
            vec![
                ("M".to_string(), true),
                ("a".to_string(), false),
                ("th".to_string(), true),
                ("ematical ".to_string(), false),
                ("P".to_string(), true),
                ("roofs".to_string(), false),
            ]
        );
        let rejoined: String = cut.iter().map(|(s, _)| s.as_str()).collect();
        assert_eq!(
            rejoined, "Mathematical Proofs",
            "nothing is lost or doubled"
        );
    }

    #[test]
    fn spans_the_text_has_no_room_for_are_ignored() {
        let cut = pieces("Dune", &[(2, 99)]);
        assert_eq!(
            cut,
            vec![("Du".to_string(), false), ("ne".to_string(), true)]
        );
        assert_eq!(
            pieces("Dune", &[(9, 12)]),
            vec![("Dune".to_string(), false)]
        );
        let joined: String = pieces("ab", &[(0, 1), (0, 2)])
            .iter()
            .map(|(s, _)| s.as_str())
            .collect();
        assert_eq!(joined, "ab");
    }
}
