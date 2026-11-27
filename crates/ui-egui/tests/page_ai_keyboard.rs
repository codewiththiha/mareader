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

#[test]
fn translation_toggle_is_independent_and_remembered_without_starting() -> Out {
    let mut h = reader(&[
        ("pdf", "sample:40"),
        ("provider", "ollama"),
        ("page_ai_hold", "true"),
    ]);
    let pane = h.state().active_pane();
    let (_, sheet) = page(&h, 0)?;
    press(&mut h, margin(sheet), PointerButton::Secondary);
    assert!(h.query_by_label("Translate continuously").is_none());
    h.get_by_label("Translate page…").hover();
    h.run();
    h.get_by_label("Translate continuously").click();
    h.run();
    assert_eq!(
        h.state().settings().ai.tasks.translate.start,
        PageStart::Continuous
    );
    assert_eq!(h.state().settings().ai.tasks.simplify.start, PageStart::Ask);
    assert!(h.state().page_ai().ledger.following(pane).is_none());
    assert!(h.query_by_label("Translate page…").is_some());
    h.event(Event::Text("burm".into()));
    h.run();
    h.get_by_label("Burmese").click();
    h.run_steps(4);
    assert_eq!(
        h.state().settings().ai.tasks.translate.language.as_deref(),
        Some("my")
    );
    assert!(h.state().page_ai().ledger.following(pane).is_some());
    Ok(())
}

#[test]
fn keyboard_can_toggle_simplify_without_running_it() -> Out {
    let mut h = reader(&[("pdf", "sample:5"), ("provider", "ollama")]);
    let pane = h.state().active_pane();
    let (_, sheet) = page(&h, 0)?;
    press(&mut h, margin(sheet), PointerButton::Secondary);
    h.get_by_label("Simplify page").focus();
    h.run();
    h.key_press(egui::Key::ArrowRight);
    h.run();
    h.get_by_label("Simplify continuously").focus();
    h.run();
    h.key_press(egui::Key::Space);
    h.run();
    assert_eq!(
        h.state().settings().ai.tasks.simplify.start,
        PageStart::Continuous
    );
    assert_eq!(
        h.state().settings().ai.tasks.translate.start,
        PageStart::Ask
    );
    assert!(h.state().page_ai().ledger.following(pane).is_none());
    h.key_press(egui::Key::Space);
    h.run();
    assert_eq!(
        h.state().settings().ai.tasks.simplify.start,
        PageStart::Page
    );
    Ok(())
}

#[test]
fn translate_parent_uses_the_remembered_mode_without_an_override() -> Out {
    let mut h = reader(&[
        ("pdf", "sample:40"),
        ("provider", "ollama"),
        ("page_ai_hold", "true"),
    ]);
    let pane = h.state().active_pane();
    h.dial(|s| s.ai.tasks.translate.start = PageStart::Continuous);
    let (_, sheet) = page(&h, 0)?;
    press(&mut h, margin(sheet), PointerButton::Secondary);
    h.get_by_label("Translate page…").click();
    h.run_steps(4);
    assert!(h.state().page_ai().ledger.following(pane).is_some());
    assert_eq!(h.state().settings().ai.tasks.translate.language, None);
    assert_eq!(
        h.state().settings().ai.tasks.translate.start,
        PageStart::Continuous
    );
    Ok(())
}

#[test]
fn turning_translate_off_changes_the_next_parent_click() -> Out {
    let mut h = reader(&[
        ("pdf", "sample:40"),
        ("provider", "ollama"),
        ("page_ai_hold", "true"),
    ]);
    let pane = h.state().active_pane();
    h.dial(|s| s.ai.tasks.translate.start = PageStart::Continuous);
    let (_, sheet) = page(&h, 0)?;
    press(&mut h, margin(sheet), PointerButton::Secondary);
    h.get_by_label("Translate page…").hover();
    h.run();
    h.get_by_label("Translate continuously").click();
    h.run();
    assert_eq!(
        h.state().settings().ai.tasks.translate.start,
        PageStart::Page
    );
    h.get_by_label("Translate page…").click();
    h.run_steps(4);
    assert!(h.state().page_ai().ledger.following(pane).is_none());
    assert!(
        h.state()
            .page_ai()
            .ledger
            .runs
            .iter()
            .any(|r| matches!(r.task, mareader_library::AiTask::Translate { .. }))
    );
    Ok(())
}

#[test]
fn keyboard_can_reach_and_toggle_each_continuous_switch_with_tab() -> Out {
    for (row, name, translate) in [
        ("Translate page…", "Translate continuously", true),
        ("Simplify page", "Simplify continuously", false),
    ] {
        let mut h = reader(&[("pdf", "sample:5"), ("provider", "ollama")]);
        let (_, sheet) = page(&h, 0)?;
        press(&mut h, margin(sheet), PointerButton::Secondary);
        h.get_by_label(row).focus();
        h.run();
        h.key_press(egui::Key::ArrowRight);
        h.run();
        let mut focused = h.get_by_label(name).is_focused();
        for _ in 0..20 {
            if focused {
                break;
            }
            h.key_press(egui::Key::Tab);
            h.run_steps(3);
            focused = h.query_by_label(name).is_some_and(|node| node.is_focused());
        }
        assert!(focused, "Tab reaches {name}");
        h.key_press(egui::Key::Space);
        h.run();
        let tasks = &h.state().settings().ai.tasks;
        assert_eq!(
            if translate {
                tasks.translate.start
            } else {
                tasks.simplify.start
            },
            PageStart::Continuous
        );
    }
    Ok(())
}
