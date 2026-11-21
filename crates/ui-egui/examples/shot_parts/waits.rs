//! The shot's waits on work done off the frame: a dictionary entry, a study
//! deck at the gate, and a recall's back and verdict.

use std::time::{Duration, Instant};

use egui_kittest::Harness;
use mareader_engine::study_mode::DeckState;
use mareader_study::mode::Mode;
use mareader_ui_egui::MareaderApp;

type H<'a> = Harness<'a, MareaderApp>;

/// Waits until the open dictionary card has a real entry.
pub fn dictionary(harness: &mut H<'_>) -> Result<(), String> {
    let until = Instant::now() + Duration::from_secs(30);
    loop {
        harness.run_steps(1);
        if let Some(answers) = harness.state().dict().card_answers() {
            if answers.is_empty() {
                return Err("dictionary lookup: no entry found".into());
            }
            harness.run_steps(4);
            return Ok(());
        }
        if Instant::now() > until {
            return Err("dictionary lookup: timed out".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Steps until the focused pane's deck or recall is ready to look at.
///
/// Gives up after half a minute and shoots what is there, saying so.
pub fn study(harness: &mut H<'_>, recall: bool) {
    let until = Instant::now() + Duration::from_secs(30);
    while !studied(harness, recall) {
        if Instant::now() > until {
            eprintln!("study mode: not ready after 30 s; shooting as it is");
            break;
        }
        harness.run_steps(1);
        std::thread::sleep(Duration::from_millis(50));
    }
    for _ in 0..20 {
        harness.run_steps(2);
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// An open or failed deck in Proactive mode; a recall with its back.
fn studied(harness: &H<'_>, recall: bool) -> bool {
    let app = harness.state();
    let pane = app.active_pane();
    let state = app.session().study_mode();
    let Some(run) = state.run(pane) else {
        return false;
    };
    if recall {
        let recalling = state.recalling(pane);
        return recalling.is_some_and(|r| r.back.is_some() || r.failure.is_some());
    }
    match run.mode {
        Mode::Proactive => matches!(run.deck, DeckState::Open(_) | DeckState::Failed { .. }),
        _ => true,
    }
}
