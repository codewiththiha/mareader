//! Keyboard access to the page translation menu.

use super::*;

#[test]
fn keyboard_opens_translate_search_and_picks_a_language() -> Out {
    let mut h = reader(&[("pdf", "sample:5"), ("provider", "ollama")]);
    let (_, sheet) = page(&h, 0)?;
    press(&mut h, margin(sheet), PointerButton::Secondary);
    h.get_by_label("Translate page…").focus();
    h.run();
    h.key_press(egui::Key::ArrowRight);
    h.run();
    h.event(Event::Text("burm".into()));
    h.run();
    assert!(h.query_by_label("Burmese").is_some());
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert_eq!(
        h.state().settings().ai.tasks.translate.language.as_deref(),
        Some("my")
    );
    Ok(())
}
