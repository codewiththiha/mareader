//! One question, described — and the one renderer that draws all four.

use leptos::prelude::*;

use library_core::conflict::Placement;

use crate::services::conflict;
use app_ui::components::primitives::menu::choice_row::ChoiceRow;
use app_ui::components::primitives::overlay::question_sheet::QuestionSheet;

/// Naming the route makes "which ask" and "which call answers it" one fact.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum AnswerRoute {
    Placement,
    FolderMerge,
    Covered,
    Shelf,
}

pub(super) struct ChoiceSpec {
    pub(super) label: &'static str,
    pub(super) note: String,
    pub(super) placement: Placement,
}

/// A value, not a component: the four differ in words and rows only.
pub(super) struct SheetSpec {
    pub(super) heading: String,
    pub(super) subtitle: String,
    pub(super) question: String,
    pub(super) cancel_title: &'static str,
    pub(super) waiting: usize,
    pub(super) apply_all: bool,
    pub(super) route: AnswerRoute,
    pub(super) choices: Vec<ChoiceSpec>,
}

/// The ✕, backdrop, Escape and Cancel all end in [`close`].
#[component]
pub(super) fn ConflictSheet(
    state: crate::context::LibraryContext,
    spec: SheetSpec,
) -> impl IntoView {
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
    let all = RwSignal::new(false);
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

fn answer(
    state: crate::context::LibraryContext,
    route: AnswerRoute,
    placement: Placement,
    all: bool,
) {
    match route {
        AnswerRoute::Placement => conflict::answer_placement(state, placement),
        AnswerRoute::FolderMerge => conflict::answer_folder_merge(state, placement, all),
        AnswerRoute::Covered => conflict::answer_covered(state, placement, all),
        AnswerRoute::Shelf => conflict::answer_shelf(state, placement),
    }
}

fn close(state: crate::context::LibraryContext, route: AnswerRoute) {
    match route {
        AnswerRoute::Shelf => conflict::cancel_shelf(state),
        _ => conflict::cancel(state),
    }
}
