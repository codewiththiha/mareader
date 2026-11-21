//! Proactive study mode in the reader, headless: the page menu's switch,
//! the gate that holds the view, and the deck that opens it.

use std::time::Duration;

use egui::{Event, Modifiers, PointerButton};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_engine::study_mode::DeckState;
use mareader_study::mode::Missing;
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::study_mode::deck::{KNOWN, REVEAL, START};
use mareader_ui_egui::study_mode::menu::KEEPS;
use mareader_workspace::PaneId;

use crate::common::{Dials, levels_reader, page, scroll, shelves};

type H = Harness<'static, MareaderApp>;
type Out = Result<(), Box<dyn std::error::Error>>;

/// Opens the page menu on the first page's blank corner.
fn page_menu(h: &mut H) -> Out {
    let (_, sheet) = page(h, 0)?;
    let at = sheet.shown.min + egui::vec2(12.0, 12.0);
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
    Ok(())
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
    let (words, packs) = shelves();
    let mut h = levels_reader(words.path(), Some(packs.path()));
    h.dial(|s| s.study.mode.strict = false);
    h.run();
    let pane = h.state().active_pane();
    page_menu(&mut h)?;
    h.get_by_label("Study mode").click();
    h.run();
    Ok((h, pane, (words, packs)))
}

#[test]
fn the_page_menu_turns_study_mode_on_and_keeps_the_pages_as_written() -> Out {
    let (mut h, pane, _shelves) = studying()?;
    assert!(h.state().session().study_mode().on(pane));
    if h.query_by_label(KEEPS).is_none() {
        page_menu(&mut h)?;
    }
    assert!(h.query_by_label(KEEPS).is_some(), "the AI tasks wait");
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
    assert!(h.query_by_label(Missing::Cards.reason()).is_some());
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
    assert!(cards(&h, pane) > 0, "quick is a new word");
    h.key_press(egui::Key::Space);
    h.run();
    assert!(
        h.query_by_label(REVEAL).is_none(),
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
