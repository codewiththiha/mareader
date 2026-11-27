//! Proactive study mode in the reader, headless: the page menu's switch,
//! the gate that holds the view, the deck that opens it, and the questions
//! the deck asks before reading now or turning off.

use std::time::Duration;

use egui::{Event, Modifiers, PointerButton};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_engine::command::Command;
use mareader_engine::settings::Tab;
use mareader_engine::study_mode::DeckState;
use mareader_study::mode::Missing;
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::activity::stack::STACK;
use mareader_ui_egui::study_mode::confirm;
use mareader_ui_egui::study_mode::deck::{KNOWN, START, TURN_OFF};
use mareader_ui_egui::study_mode::flip_card::TURN_HINT;
use mareader_ui_egui::study_mode::menu::{KEEPS, SETTINGS};
use mareader_ui_egui::ui_command::Sent;
use mareader_workspace::PaneId;

use crate::common::{Dials, levels_reader, page, scroll, shelves};

type H = Harness<'static, MareaderApp>;
type Out = Result<(), Box<dyn std::error::Error>>;

/// Opens the page menu on the first page's blank corner.
fn page_menu(h: &mut H) -> Out {
    let (_, sheet) = page(h, 0)?;
    page_menu_at(h, sheet.shown.min + egui::vec2(12.0, 12.0));
    Ok(())
}

/// Opens the page menu at `at`, on a page.
fn page_menu_at(h: &mut H, at: egui::Pos2) {
    h.hover_at(at);
    h.step();
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos: at,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }
    h.run();
}

/// Steps until `done` holds, giving the engine's workers time to answer.
fn wait(h: &mut H, done: impl Fn(&H) -> bool) -> bool {
    for _ in 0..600 {
        if done(h) {
            return true;
        }
        h.run_steps(1);
        std::thread::sleep(Duration::from_millis(10));
    }
    done(h)
}

fn deck_open(h: &H, pane: PaneId) -> bool {
    let run = h.state().session().study_mode().run(pane);
    run.is_some_and(|r| matches!(r.deck, DeckState::Open(_)))
}

fn cards(h: &H, pane: PaneId) -> usize {
    let run = h.state().session().study_mode().run(pane);
    match run.map(|r| &r.deck) {
        Some(DeckState::Open(open)) => open.cards.len(),
        _ => 0,
    }
}

type Shelves = (tempfile::TempDir, tempfile::TempDir);

/// A reader with word levels and a dictionary, studying from the menu.
fn studying() -> Result<(H, PaneId, Shelves), Box<dyn std::error::Error>> {
    studying_with(false)
}

/// The same, with a time limit on each deck or none.
fn studying_with(limit: bool) -> Result<(H, PaneId, Shelves), Box<dyn std::error::Error>> {
    let (words, packs) = shelves();
    let mut h = levels_reader(words.path(), Some(packs.path()));
    h.dial(|s| s.study.mode.limit = limit);
    h.run();
    let pane = h.state().active_pane();
    page_menu(&mut h)?;
    let off = h.query_by_label("Proactive").is_none();
    assert!(off, "no modes while off");
    h.get_by_label("Study mode").click();
    h.run();
    Ok((h, pane, (words, packs)))
}

#[test]
fn the_page_menu_turns_study_mode_on_and_keeps_the_pages_as_written() -> Out {
    let (mut h, pane, _shelves) = studying()?;
    assert!(h.state().session().study_mode().on(pane));
    let closed = h.query_by_label("Proactive").is_none();
    assert!(closed, "the menu makes way as study mode starts");
    // The preparing screen covers the page; past the deck it opens again.
    assert!(at_the_deck(&mut h, pane), "the deck opens");
    h.get_by_label(START).click();
    h.run();
    // The gate scrolled the page's top corner under the title bar.
    let (_, sheet) = page(&h, 0)?;
    page_menu_at(&mut h, sheet.shown.center());
    assert!(
        h.query_by_label_contains(KEEPS).is_some(),
        "the AI tasks wait"
    );
    let on = h.query_by_label("Proactive").is_some();
    assert!(on, "modes while on");
    let at = h.get_by_label("Study mode").rect().center();
    let met = h.ctx.layer_id_at(at);
    h.get_by_label("Study mode").click();
    h.run();
    let on = h.state().session().study_mode().on(pane);
    assert!(!on, "the switch turns it off; {at:?} met {met:?}");
    Ok(())
}

#[test]
fn without_cards_to_write_the_menu_says_what_is_missing() -> Out {
    let (words, _) = shelves();
    let mut h = levels_reader(words.path(), None);
    h.dial(|s| s.ai.presets.clear());
    h.run();
    page_menu(&mut h)?;
    // Why it is off shows on the switch, not under it.
    assert!(h.query_by_label(Missing::Cards.reason()).is_none());
    let at = h.get_by_label("Study mode").rect().center();
    h.hover_at(at);
    h.run_steps(60);
    assert!(h.query_by_label(Missing::Cards.reason()).is_some());
    assert!(h.query_by_label(SETTINGS).is_none(), "no tab to open");
    h.get_by_label("Study mode").click();
    h.run();
    let pane = h.state().active_pane();
    assert!(
        !h.state().session().study_mode().on(pane),
        "the switch is off"
    );
    Ok(())
}

#[test]
fn the_study_tab_comes_with_an_ai_or_a_dictionary() {
    let open_study = |h: &mut H| {
        let open = Sent::Engine(Command::OpenSettings(Tab::Study));
        h.state_mut().queue(open);
        h.run();
        h.state().settings_window().tab() == Tab::Study
    };
    let (words, packs) = shelves();
    let mut h = levels_reader(words.path(), None);
    h.dial(|s| s.ai.presets.clear());
    assert!(!open_study(&mut h), "neither writes cards, so no tab");
    let mut h = levels_reader(words.path(), Some(packs.path()));
    h.dial(|s| s.ai.presets.clear());
    assert!(open_study(&mut h), "a dictionary brings the tab");
    let choice = h.query_by_label("Cards from").is_some();
    assert!(!choice, "one writer, no choice");
    let slider = h.query_by_label("Reading level").is_some();
    assert!(!slider, "Vocabulary sets the level");
    h.get_by_label("About Mode").click();
    h.run();
    let level = h.query_by_label_contains("Uses your Vocabulary level");
    assert!(level.is_some(), "the tab names the level it uses");
}

#[test]
fn the_gate_holds_the_view_until_the_deck_is_studied() -> Out {
    let (mut h, pane, _shelves) = studying()?;
    h.key_press(egui::Key::Escape);
    h.run();
    let gated = |h: &H| h.state().session().study_gate(pane) == Some(0);
    assert!(wait(&mut h, gated), "the first batch is locked");
    for _ in 0..4 {
        h.key_press(egui::Key::PageDown);
        h.run();
    }
    scroll(&mut h, 4000.0);
    let current = h.state().documents().reading(pane).map(|r| r.current);
    assert_eq!(current, Some(0), "the strip stops at the gate page");
    assert!(wait(&mut h, |h| deck_open(h, pane)), "the deck opens");
    let stack = h.query_by_label(STACK).is_some();
    assert!(!stack, "dictionary cards come at once, with no stack");
    assert!(cards(&h, pane) > 0, "quick is a new word");
    assert!(
        h.query_by_label_contains(TURN_HINT).is_some(),
        "the card shows its front"
    );
    h.key_press(egui::Key::Space);
    h.run();
    assert!(
        h.query_by_label_contains(TURN_HINT).is_none(),
        "Space turns the card over"
    );
    h.get_by_label(KNOWN).click();
    h.run();
    assert_eq!(cards(&h, pane), 0, "a known word leaves the deck");
    h.get_by_label(START).click();
    h.run();
    let gate = h.state().session().study_gate(pane);
    assert!(
        gate.is_none_or(|g| g > 0),
        "Start unlocks the batch: {gate:?}"
    );
    Ok(())
}

/// Takes `pane` to its gate and waits for the deck there.
fn at_the_deck(h: &mut H, pane: PaneId) -> bool {
    h.key_press(egui::Key::Escape);
    h.run();
    let gated = |h: &H| h.state().session().study_gate(pane) == Some(0);
    if !wait(h, gated) {
        return false;
    }
    for _ in 0..4 {
        h.key_press(egui::Key::PageDown);
        h.run();
    }
    scroll(h, 4000.0);
    wait(h, |h| deck_open(h, pane))
}

#[test]
fn read_now_with_time_left_asks_before_the_cards_close_for_good() -> Out {
    let (mut h, pane, _shelves) = studying_with(true)?;
    assert!(at_the_deck(&mut h, pane), "the deck opens");
    h.get_by_label(START).click();
    h.run_steps(4);
    let asked = h.query_by_label(confirm::READ_NOW).is_some();
    assert!(asked, "Read now asks first while time is left");
    h.get_by_label(confirm::KEEP).click();
    h.run_steps(4);
    assert!(deck_open(&h, pane), "keeping on keeps the deck");
    h.get_by_label(START).click();
    h.run_steps(4);
    h.get_by_label(confirm::READ_NOW).click();
    h.run_steps(4);
    let run = h.state().session().study_mode().run(pane);
    let closed = run.is_some_and(|r| r.kept.is_empty() && r.gate() != Some(0));
    assert!(closed, "the pages unlock and the cards are not kept");
    Ok(())
}

#[test]
fn turning_off_from_the_deck_asks_first() -> Out {
    let (mut h, pane, _shelves) = studying()?;
    assert!(at_the_deck(&mut h, pane), "the deck opens");
    h.get_by_label(TURN_OFF).click();
    h.run_steps(4);
    let on = |h: &H| h.state().session().study_mode().on(pane);
    assert!(on(&h), "nothing turns off before the answer");
    h.get_by_label(confirm::TURN_OFF).click();
    h.run_steps(4);
    assert!(!on(&h), "the answer turns study mode off");
    Ok(())
}

#[test]
fn writer_sizes_belong_to_models_and_do_not_select_them() {
    use mareader_settings::{Provider, TaskId};
    let (words, packs) = shelves();
    let mut h = levels_reader(words.path(), Some(packs.path()));
    h.dial(|s| {
        s.ai.presets.clear();
        let first = s.ai.add_preset(Provider::Ollama, "first");
        let second = s.ai.add_preset(Provider::Ollama, "second");
        s.ai.tasks.choose(TaskId::Explain, Some(first));
        s.ai.tasks.choose(TaskId::StudyCards, Some(first));
        if let Some(p) = s.ai.presets.iter_mut().find(|p| p.id == second) {
            p.name = "A very long second model name that should not widen this menu".into();
        }
    });
    h.state_mut()
        .queue(Sent::Engine(Command::OpenSettings(Tab::Study)));
    h.run();
    scroll(&mut h, 1000.0);
    h.run();
    h.get_by_label("Study cards").click();
    h.run();
    h.get_by_label("More cards per request for Ollama · first")
        .click();
    h.run();
    assert!(
        h.query_by_label("More cards per request for Ollama · first")
            .is_some(),
        "menu stays open"
    );
    let ai = &h.state().settings().ai;
    assert_eq!(ai.presets[0].cards_per_request, 11);
    assert_eq!(ai.presets[1].cards_per_request, 10);
    assert_eq!(ai.tasks.choice(TaskId::StudyCards), Some(ai.presets[0].id));
    h.event(Event::Text("second".into()));
    h.run();
    assert!(h.query_by_label("Find a model").is_some());
    assert!(
        h.query_by_label("More cards per request for Ollama · first")
            .is_none()
    );
    h.key_press(egui::Key::Enter);
    h.run();
    let ai = &h.state().settings().ai;
    assert_eq!(ai.tasks.choice(TaskId::StudyCards), Some(ai.presets[1].id));
}
