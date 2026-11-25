//! An explained word's ask: cancelled from its mark's menu, or failed.

use egui::{Event, Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_engine::command::Command;
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::ui_command::Sent;

use crate::common::{ai_reader as reader, find, page};

type H = Harness<'static, MareaderApp>;
type Out = Result<(), Box<dyn std::error::Error>>;

fn marks(h: &H) -> usize {
    let pane = h.state().active_pane();
    h.state().marks(pane).len()
}

/// A press at `pos`, for a wait that keeps asking for frames.
fn press(h: &mut H, pos: Pos2, button: PointerButton) {
    h.hover_at(pos);
    h.step();
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }
    h.run_steps(10);
}

#[test]
fn a_failed_answer_says_why_and_unmarks_the_word() {
    let mut h = reader(&[("pdf", "sample:5"), ("explain", "none")]);
    let pane = h.state().active_pane();
    let (page, text) = (0, "quick".to_owned());
    h.state_mut()
        .queue(Sent::Engine(Command::SelectText { pane, page, text }));
    h.run();
    h.state_mut().queue(Sent::Engine(Command::Explain(pane)));
    h.run();
    assert!(h.state().study().card_answer().is_none());
    // Nothing on this machine can answer: no retry is offered.
    assert!(h.query_by_label("Try again").is_none());
    assert_eq!(marks(&h), 0, "the word as it was");
    h.run_steps(4);
    assert!(h.state().study().card_open(), "the card stays to say why");
}

#[test]
fn while_its_answer_comes_a_mark_s_menu_cancels_it_and_unmarks_the_word() -> Out {
    let mut h = reader(&[
        ("pdf", "sample:5"),
        ("explain", "slow:20000"),
        ("selection", "0:quick"),
        ("gloss", "open"),
    ]);
    h.run_steps(10);
    assert_eq!(marks(&h), 1, "marked at once");
    let (layer, sheet) = page(&h, 0)?;
    let c = layer.text.chars().get(find(&layer, "quick")? + 2);
    let bounds = c.ok_or("no such character")?.bounds;
    let at = sheet.to_screen(bounds).center();
    press(&mut h, at, PointerButton::Secondary);
    let remove = h.query_by_label("Remove highlight");
    assert!(remove.is_none(), "not while it asks");
    h.get_by_label("Cancel").click();
    h.run_steps(10);
    assert_eq!(marks(&h), 0, "the word as it was");
    assert!(h.query_by_label("Undo").is_none(), "nothing to undo");
    assert!(!h.state().study().card_open());
    Ok(())
}
