//! One question, described — and the one renderer that draws all four.
//!
//! Four questions wear this chrome: the level's own NAME (a book arriving where
//! a book of that name stands), a merged folder's per-FILE ask, a covered file's
//! GROUND, and a folder's own name collision. Each was a component that worked
//! out its sentences and then rendered its own list of `ChoiceRow`s. The
//! skeleton was already shared (`QuestionSheet`); what was not was everything
//! hanging on it — four spellings of the apply-to-all signal, four mappings from
//! `library_core::conflict::Placement` to a label, a note and a call, and four
//! places to change when answering grew a step.
//!
//! So a question describes itself as a [`SheetSpec`] and this file draws it. The
//! describers — `describe_name`, `describe_folder_merge`, `describe_covered`,
//! `describe_shelf`, one per file beside this one — are functions of the state
//! the answer will be given against, and a row carries the [`Placement`] it
//! means rather than a closure of its own. That is what puts "how does an answer
//! land" here, in one [`answer`], instead of in four click handlers.

use leptos::prelude::*;

use library_core::conflict::Placement;

use crate::components::primitives::menu::choice_row::ChoiceRow;
use crate::components::primitives::overlay::question_sheet::QuestionSheet;
use crate::services::library::conflict;
use crate::state::AppState;

/// Which state a question lives on, and so which service lands its answer.
///
/// The four asks are raised on two states and dropped by two functions; naming
/// the ROUTE rather than carrying a closure per row is what lets a sheet be
/// described without being built, and what makes "which ask is this" and "which
/// call answers it" one fact instead of two that can drift apart.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum AnswerRoute {
    /// A row's own placement on the level — the name question.
    Placement,
    /// One file of a merged folder.
    FolderMerge,
    /// A loose import of a file the library already holds, or of one a folder
    /// reads in place.
    Covered,
    /// The folder's own name collision, which lives on the shelf state.
    Shelf,
}

/// One answer as the sheet shows it: its name, what it promises, and which
/// answer it is.
pub(super) struct ChoiceSpec {
    /// The answer's own name — "Merge", "Add as new", "Replace".
    pub(super) label: &'static str,
    /// What choosing it does, in the sheet's own words. Wraps; a promise the
    /// reader has to take on faith is not a promise.
    pub(super) note: String,
    /// The answer itself, in the app's own words for it, so the click is a
    /// lookup rather than a fifth spelling of what the row meant.
    pub(super) placement: Placement,
}

/// One question, described: what it says, and the answers it offers.
///
/// A value rather than a component because the four questions differ in their
/// words and their rows and in nothing else — and because a describer that
/// returns a struct is a describer that can be read (and counted, and checked
/// against the apply's own list) without a view in the way.
pub(super) struct SheetSpec {
    /// The title — usually the arriving name.
    pub(super) heading: String,
    /// The muted line under it: where the collision is, and how many more wait.
    pub(super) subtitle: String,
    /// The question, in one sentence.
    pub(super) question: String,
    /// What cancelling promises, as the Cancel button's tooltip.
    pub(super) cancel_title: &'static str,
    /// How many more questions wait behind this one. Counted here rather than
    /// in the renderer so the subtitle's count and the switch's own count are
    /// one read of one list.
    pub(super) waiting: usize,
    /// Whether one answer may be given to every one of them. False where the
    /// next question is a different fact rather than the same one again — the
    /// level's name question, where the next arrival is another book — so a
    /// switch offering to answer for it would be lying about its reach.
    pub(super) apply_all: bool,
    /// Which service lands an answer here.
    pub(super) route: AnswerRoute,
    /// The answers, in the order the sheet shows them.
    pub(super) choices: Vec<ChoiceSpec>,
}

/// Draw one described question.
///
/// The ✕, the backdrop, the Escape key and Cancel all end in [`close`], which
/// is the one place the answer to "what does dropping this question mean"
/// lives — the level's questions clear the queue behind them, and the folder's
/// clears its own ask.
#[component]
pub(super) fn ConflictSheet(state: AppState, spec: SheetSpec) -> impl IntoView {
    let SheetSpec {
        heading,
        subtitle,
        question,
        cancel_title,
        waiting,
        apply_all,
        route,
        choices,
    } = spec;
    // The switch's state belongs to the sheet rather than to the question: a
    // question answered one at a time gets a signal no row reads, because its
    // route's answer function does not take one.
    let all = RwSignal::new(false);
    // A question whose answer repeats gets the count waiting behind it, and one
    // whose answer does not gets zero — the count `QuestionSheet` reads as "no
    // switch to draw".
    let switch = if apply_all { waiting } else { 0 };

    view! {
        <QuestionSheet
            heading=heading
            subtitle=subtitle
            question=question
            on_close=Callback::new(move |_| close(state, route))
            cancel_title=cancel_title.to_string()
            apply_all=(switch, all)
        >
            {choices
                .into_iter()
                .map(|choice| {
                    let ChoiceSpec { label, note, placement } = choice;
                    view! {
                        <ChoiceRow
                            label=label
                            note=note
                            on_click=Callback::new(move |_| {
                                answer(state, route, placement, all.get_untracked())
                            })
                        />
                    }
                })
                .collect_view()}
        </QuestionSheet>
    }
}

/// Land one answer against the ask it belongs to.
///
/// Two of the four take the apply-to-all switch's state and two do not: a
/// placement on the level is one book's own fact, and a folder's collision is
/// one arrival's — where a file of a merged folder or a covered file is one of
/// forty that often share an answer.
fn answer(state: AppState, route: AnswerRoute, placement: Placement, all: bool) {
    match route {
        AnswerRoute::Placement => conflict::answer_placement(state, placement),
        AnswerRoute::FolderMerge => conflict::answer_folder_merge(state, placement, all),
        AnswerRoute::Covered => conflict::answer_covered(state, placement, all),
        AnswerRoute::Shelf => conflict::answer_shelf(state, placement),
    }
}

/// Drop the question on screen, and with it whatever the route says goes.
fn close(state: AppState, route: AnswerRoute) {
    match route {
        AnswerRoute::Shelf => conflict::cancel_shelf(state),
        _ => conflict::cancel(state),
    }
}
