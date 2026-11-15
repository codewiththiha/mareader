//! The keys `set_option` keeps, and what each of them may touch.
//!
//! A key that sets the engine's state asks the engine for it, so a shot, a
//! query string and a command line all mean one thing by it. Two lines are
//! held here: an option handler reaches neither the settings nor the pane tree
//! through a mutable accessor, and the three keys that write the engine's own
//! state land through its door.

use egui_kittest::Harness;
use mareader_settings::Bar;
use mareader_ui_egui::MareaderApp;

/// The six families' own keys, as source.
const FAMILIES: [&str; 6] = [
    include_str!("../src/app/appearance.rs"),
    include_str!("../src/app/library.rs"),
    include_str!("../src/app/study.rs"),
    include_str!("../src/app/page_ai.rs"),
    include_str!("../src/app/read_aloud.rs"),
    include_str!("../src/app/panes.rs"),
];

/// A window with `options` set, as the other suites build one.
fn with(options: &[(&str, &str)]) -> Harness<'static, MareaderApp> {
    let mut app = MareaderApp::default();
    app.set_option("workers", "0").ok();
    for (key, value) in options {
        app.set_option(key, value).ok();
    }
    let mut h = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(60)
        .build_eframe(|_| app);
    h.run_steps(4);
    h
}

/// Each `option_` handler's body, cut at the method after it.
fn handlers(source: &str) -> Vec<&str> {
    let mut found = Vec::new();
    for rest in source.split("fn option_").skip(1) {
        let cut = rest.split_once("\n    fn ");
        found.push(cut.map_or(rest, |(body, _)| body));
    }
    found
}

#[test]
fn no_option_key_writes_the_engine_through_an_accessor() {
    let mut counted = 0;
    for source in FAMILIES {
        for body in handlers(source) {
            counted += 1;
            for name in ["settings_mut", "tree_mut"] {
                assert!(!body.contains(name), "an option key still names {name}");
            }
        }
    }
    assert!(counted > 30, "{counted} handlers is too few");
}

#[test]
fn a_pin_key_reaches_every_bar() {
    let h = with(&[("pinned", "true")]);
    for bar in Bar::ALL {
        assert!(h.state().settings().chrome.pinned(bar), "{bar:?} is free");
    }
}

#[test]
fn a_panes_key_splits_and_leaves_the_focus_alone() {
    let h = with(&[("panes", "3")]);
    let app = h.state();
    assert_eq!(app.tree().len(), 3);
    assert_eq!(Some(app.active()), app.tree().panes().first().copied());
}

#[test]
fn a_preset_key_keeps_the_look_it_finds() {
    let h = with(&[("save_preset", "Look/Page")]);
    let kept = &h.state().settings().user_presets;
    assert_eq!(kept.len(), 1);
    assert_eq!(kept.first().map(|p| p.name.as_str()), Some("Look"));
    assert_eq!(kept.first().map(|p| p.group.as_str()), Some("Page"));
}
