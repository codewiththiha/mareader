//! Page translation's direct and continuous actions.

use egui::{Event, Modifiers, PointerButton};
use egui_kittest::kittest::Queryable as _;
use mareader_library::AiTask;

use crate::common::{ai_reader as reader, page};

#[test]
fn direct_translation_does_not_create_a_manual_preference() -> Result<(), Box<dyn std::error::Error>>
{
    let mut h = reader(&[
        ("pdf", "sample:5"),
        ("provider", "ollama"),
        ("page_ai_hold", "true"),
    ]);
    let pane = h.state().active_pane();
    let (_, sheet) = page(&h, 0)?;
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
    let at = h.get_by_label("Translate page…").rect().center();
    h.hover_at(at);
    h.step();
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }
    h.run_steps(4);
    let ledger = &h.state().page_ai().ledger;
    assert!(ledger.following(pane).is_none());
    assert!(
        ledger.runs.iter().any(|r| r.task
            == AiTask::Translate {
                language: "en".into()
            }),
        "runs: {:?}",
        ledger.runs.iter().map(|r| &r.task).collect::<Vec<_>>()
    );
    assert_eq!(h.state().settings().ai.tasks.translate.language, None);
    Ok(())
}
