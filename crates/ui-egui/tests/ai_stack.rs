//! The AI stack's lines for an explained word whose glow and card are away.

use std::time::Duration;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_engine::command::Command;
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::activity::stack::{CANCEL, STACK, VIEW};
use mareader_ui_egui::ui_command::Sent;

use crate::common::ai_reader;

type H = Harness<'static, MareaderApp>;

/// A reader whose AI Dictionary answers after a moment.
fn slow_reader() -> H {
    ai_reader(&[("pdf", "sample:5"), ("explain", "slow:300")])
}

fn send(h: &mut H, command: Command) {
    h.state_mut().queue(Sent::Engine(command));
    h.run_steps(30);
}

/// Explains `word` on the first page, then turns to page 3, away from it.
fn explain_and_turn_away(h: &mut H, word: &str) {
    let pane = h.state().active_pane();
    let text = word.to_owned();
    send(
        h,
        Command::SelectText {
            pane,
            page: 0,
            text,
        },
    );
    send(h, Command::Explain(pane));
    assert!(h.state().study().card_open(), "its card opens");
    assert!(h.query_by_label(STACK).is_none(), "the card says it");
    send(h, Command::GoTo { pane, page: 3 });
    assert!(
        !h.state().study().card_open(),
        "scrolled away, the card goes"
    );
    assert!(
        until(h, |h| h.query_by_label(STACK).is_some()),
        "the corner says it"
    );
    h.get_by_label(STACK).hover();
    h.run_steps(20);
}

/// Steps until `done` holds, the explainer's wait passing meanwhile.
fn until(h: &mut H, done: impl Fn(&H) -> bool) -> bool {
    for _ in 0..400 {
        if done(h) {
            return true;
        }
        h.run_steps(1);
        std::thread::sleep(Duration::from_millis(5));
    }
    done(h)
}

/// PROGRESS.md §5, step 5: done out of view, View scrolls back and opens it.
#[test]
fn an_answer_landed_out_of_view_waits_until_view_opens_its_card() {
    let mut h = slow_reader();
    explain_and_turn_away(&mut h, "quick");
    h.get_by_label_contains("Explaining “quick”");
    let landed = |h: &H| h.query_by_label_contains("Explained “quick”").is_some();
    assert!(until(&mut h, landed), "it says the answer came");
    h.get_by_label(VIEW).click();
    h.run_steps(40);
    assert_eq!(h.state().study().card_word(), Some("quick"));
    h.get_by_label("Synonyms");
    assert!(h.query_by_label(STACK).is_none(), "seen, the line goes");
}

/// PROGRESS.md §5, step 6: a row that isn't study's stops at once.
#[test]
fn an_explain_out_of_view_cancels_at_once_with_a_toast() {
    let mut h = slow_reader();
    explain_and_turn_away(&mut h, "quick");
    h.get_by_label(CANCEL).click();
    h.run_steps(4);
    let toast = h.state().toast().map(|t| t.message.clone());
    let said = toast.unwrap_or_default();
    assert!(said.starts_with("Stopped explaining"), "{said:?}");
    assert!(until(&mut h, |h| h.query_by_label(STACK).is_none()));
    std::thread::sleep(Duration::from_millis(400));
    h.run_steps(10);
    assert!(h.query_by_label(STACK).is_none(), "its answer never lands");
}
