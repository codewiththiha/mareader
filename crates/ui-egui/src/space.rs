//! Space: every study word, across every book; see `docs/study-mode.md`.

use egui::{RichText, Ui};
use mareader_engine::command::Command;
use mareader_engine::export::Format;
use mareader_engine::session::Session;
use mareader_engine::study::Action;
use mareader_library::StudyWordId;
use mareader_library::WordStatus as LibStatus;
use mareader_study::{
    LearnedBy as StudyBy, Scope, SpaceFilter, SpaceRow, SpaceSort, WordClass, WordKey,
    WordStatus as StudyStatus, filtered, sorted, summary, to_items,
};

use crate::ui_command::{Commands, Sent};

#[derive(Clone, Default)]
struct SpaceUi {
    search: String,
    status: String,
    sort: usize,
    selected: Vec<StudyWordId>,
}

fn status_of(status: LibStatus) -> StudyStatus {
    match status {
        LibStatus::Memorized => StudyStatus::Memorized,
        LibStatus::Learning => StudyStatus::Learning,
        LibStatus::Missed => StudyStatus::Missed,
    }
}

/// Space, drawn in the panes' place like the library.
pub fn show(ui: &mut Ui, session: &mut Session, _top: f32, out: &mut Commands) {
    let id = ui.id().with("space-state");
    let mut state: SpaceUi = ui.ctx().data_mut(|d| d.get_temp(id).unwrap_or_default());
    if state.status.is_empty() {
        state.status = "all".to_owned();
    }
    let (words, rows) = space_rows(session);
    ui.vertical(|ui| {
        ui.label(RichText::new("Space").size(26.0).strong());
        ui.label("Words you are learning, memorized and missed, in every book.");
        ui.add_space(8.0);
        controls(ui, &mut state);
        ui.add_space(6.0);
        let filter = SpaceFilter {
            search: state.search.clone(),
            status: match state.status.as_str() {
                "learning" => Some(StudyStatus::Learning),
                "memorized" => Some(StudyStatus::Memorized),
                "missed" => Some(StudyStatus::Missed),
                _ => None,
            },
            ..SpaceFilter::default()
        };
        // The status tab narrows the rows before `filtered` runs.
        let narrowed: Vec<SpaceRow> = rows
            .iter()
            .filter(|r| filter.status.is_none_or(|s| r.status == s))
            .cloned()
            .collect();
        let mut search_only = filter.clone();
        search_only.status = None;
        let shown_rows = filtered(&narrowed, &search_only);
        let items = to_items(&shown_rows, Scope::Books);
        let sort = match state.sort {
            1 => SpaceSort::Az,
            2 => SpaceSort::Hardest,
            3 => SpaceSort::MostTries,
            4 => SpaceSort::Book,
            _ => SpaceSort::Recent,
        };
        let shown = sorted(items, sort);
        let (memorized, first, recalled) = summary(&rows);
        let learning = rows.iter().filter(|r| r.status == StudyStatus::Learning).count();
        let missed = rows.iter().filter(|r| r.status == StudyStatus::Missed).count();
        let rate = first
            .checked_mul(100)
            .and_then(|n| n.checked_div(recalled))
            .unwrap_or(0);
        ui.label(format!(
            "{memorized} memorized · {learning} learning · {missed} missed · first-try recall {rate}%"
        ));
        ui.horizontal(|ui| {
            if ui.button("Export CSV").clicked() {
                out.push(Sent::Engine(Command::Export {
                    name: "space.csv".to_owned(),
                    format: Format::Text,
                    contents: mareader_study::export_csv(&shown),
                }));
            }
            if ui.button("Forget selected").clicked() {
                out.push(Sent::Engine(Command::Study(Action::ForgetWords(
                    state.selected.clone(),
                ))));
                state.selected.clear();
            }
        });
        ui.add_space(6.0);
        word_rows(ui, session, &mut state, &shown, &words, out);
    });
    ui.ctx().data_mut(|d| d.insert_temp(id, state));
}

/// Space's search, status and sort controls.
fn controls(ui: &mut Ui, state: &mut SpaceUi) {
    ui.horizontal(|ui| {
        ui.label("Search");
        ui.text_edit_singleline(&mut state.search);
        ui.label("Status");
        for label in ["all", "learning", "memorized", "missed"] {
            if ui.selectable_label(state.status == label, label).clicked() {
                state.status = label.to_owned();
            }
        }
        ui.label("Sort");
        for (i, label) in ["Recent", "A–Z", "Hardest", "Most tries", "Book"]
            .iter()
            .enumerate()
        {
            if ui.selectable_label(state.sort == i, *label).clicked() {
                state.sort = i;
            }
        }
    });
}

/// Every recorded word, and its Space row with its book's name.
fn space_rows(session: &Session) -> (Vec<mareader_library::StudyWord>, Vec<SpaceRow>) {
    let words: Vec<mareader_library::StudyWord> =
        session.study_words().into_iter().cloned().collect();
    let rows: Vec<SpaceRow> = words
        .iter()
        .map(|w| {
            let book = session
                .library_model()
                .book(w.book.value)
                .map(|b| b.title.value.clone())
                .unwrap_or_else(|| "A book".to_owned());
            SpaceRow {
                key: WordKey::new(w.lemma.clone(), WordClass::of(&w.class)),
                band: w.band,
                meaning: w.meaning.clone(),
                context: w.context.clone(),
                book,
                status: status_of(w.status.value),
                by: match w.by {
                    mareader_library::LearnedBy::Card => StudyBy::Card,
                    mareader_library::LearnedBy::Recall => StudyBy::Recall,
                },
                attempts: w.attempts,
                first_try: w.first_try,
                at: w.touched,
            }
        })
        .collect();
    (words, rows)
}

/// The word rows, with selection and memorized toggles.
fn word_rows(
    ui: &mut Ui,
    session: &Session,
    state: &mut SpaceUi,
    shown: &[mareader_study::SpaceItem],
    words: &[mareader_library::StudyWord],
    out: &mut Commands,
) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        for item in shown {
            let word_id = item.books.first().and_then(|book| {
                words
                    .iter()
                    .find(|w| {
                        w.lemma == item.key.lemma
                            && session
                                .library_model()
                                .book(w.book.value)
                                .map(|b| b.title.value == *book)
                                .unwrap_or(book == "A book")
                    })
                    .map(|w| w.id)
            });
            let Some(word_id) = word_id else { continue };
            ui.horizontal(|ui| {
                let mut picked = state.selected.contains(&word_id);
                if ui.checkbox(&mut picked, "").changed() {
                    match picked {
                        true => state.selected.push(word_id),
                        false => state.selected.retain(|x| *x != word_id),
                    }
                }
                ui.label(RichText::new(&item.key.lemma).strong());
                ui.label(item.status.label());
                ui.label(item.books.join(", "));
                ui.label(&item.meaning);
                if item.status != StudyStatus::Memorized && ui.button("Mark memorized").clicked() {
                    out.push(Sent::Engine(Command::Study(Action::SetWordStatus(
                        word_id,
                        LibStatus::Memorized,
                    ))));
                }
                if item.status == StudyStatus::Memorized && ui.button("Un-memorize").clicked() {
                    out.push(Sent::Engine(Command::Study(Action::SetWordStatus(
                        word_id,
                        LibStatus::Learning,
                    ))));
                }
            });
        }
        if shown.is_empty() {
            ui.label("No study words yet. Turn study mode on in a book.");
        }
    });
}
