//! The AI glow round a word whose answer is still coming, headless.

use std::time::{Duration, Instant};

use egui::{Event, Modifiers, PointerButton};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_engine::command::Command;
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::glow;
use mareader_ui_egui::ui_command::Sent;

use crate::common::{ai_reader as reader, find, page};

type H = Harness<'static, MareaderApp>;
type Out = Result<(), Box<dyn std::error::Error>>;

fn asking(h: &H) -> usize {
    h.state().session().study().asking_marks().len()
}

fn click(h: &mut H, pos: egui::Pos2) {
    h.hover_at(pos);
    h.step();
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }
    h.run();
}

#[test]
fn a_word_glows_while_its_answer_comes_even_with_its_card_shut() -> Out {
    let mut h = reader(&[("pdf", "sample:5")]);
    h.state_mut().set_option("explain", "slow:400")?;
    let pane = h.state().active_pane();
    {
        let (page, text) = (0, "quick".to_owned());
        let select = Command::SelectText { pane, page, text };
        h.state_mut().queue(Sent::Engine(select));
    }
    h.run();
    h.get_by_label("AI Dictionary").click();
    h.run_steps(4);
    assert_eq!(asking(&h), 1, "the answer is coming");
    assert!(glow::shown(&h.ctx) >= 1, "its word glows");
    // Shut while it works: the ask goes on, and so does the glow.
    h.key_press(egui::Key::Escape);
    h.run_steps(4);
    assert!(!h.state().study().card_open());
    assert!(glow::shown(&h.ctx) >= 1, "still glowing");
    let start = Instant::now();
    while asking(&h) > 0 && start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    assert_eq!(asking(&h), 0, "the answer came");
    h.run_steps(4);
    assert_eq!(glow::shown(&h.ctx), 0, "the glow is gone");
    assert!(!h.state().study().card_open(), "the card stays shut");
    // The answer waits on its mark.
    let (layer, sheet) = page(&h, 0)?;
    let c = layer.text.chars().get(find(&layer, "quick")? + 2);
    let bounds = c.ok_or("no such character")?.bounds;
    let at = sheet.to_screen(bounds).center();
    click(&mut h, at);
    let pos = h.state().study().card_answer().map(|a| a.pos.clone());
    assert_eq!(pos.as_deref(), Some("adjective"));
    Ok(())
}
