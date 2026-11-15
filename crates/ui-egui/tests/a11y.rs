//! Every control a screen reader reaches has a name, in every main view.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{By, NodeT as _, Queryable as _};
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::ui_command::{Sent, UiCommand};

fn with(options: &[(&str, &str)]) -> Harness<'static, MareaderApp> {
    let mut app = MareaderApp::default();
    app.set_option("workers", "0").ok();
    for (k, v) in options {
        app.set_option(k, v).ok();
    }
    let mut h = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(400)
        .build_eframe(|_| app);
    h.run_steps(6);
    h
}

/// Roles a reader acts on, so each needs a name to be found by.
fn acts(role: Role) -> bool {
    matches!(
        role,
        Role::Button
            | Role::CheckBox
            | Role::RadioButton
            | Role::Slider
            | Role::SpinButton
            | Role::ComboBox
            | Role::TextInput
            | Role::MultilineTextInput
            | Role::SearchInput
            | Role::Link
            | Role::MenuItem
            | Role::Tab
            | Role::Switch
    )
}

/// The nameless controls on screen now, described for the failure.
fn nameless(h: &Harness<'static, MareaderApp>, state: &str) -> Vec<String> {
    let by = By::new().predicate(|n| {
        let named = n.label().is_some_and(|l| !l.trim().is_empty());
        acts(n.role()) && !n.is_hidden() && !named
    });
    h.query_all(by)
        .map(|n| {
            let n = n.accesskit_node();
            let at = n.bounding_box().map(|b| (b.x0 as i32, b.y0 as i32));
            format!("{state}: {:?} value {:?} at {at:?}", n.role(), n.value())
        })
        .collect()
}

#[test]
fn every_control_has_a_name() {
    let mut missing = Vec::new();
    let reader = [("pdf", "sample:5"), ("pinned", "true")];
    let mut h = with(&reader);
    h.hover_at(egui::pos2(600.0, 797.0));
    h.run_steps(4);
    missing.extend(nameless(&h, "reader"));
    for panel in ["thumbnails", "outline", "library"] {
        let h = with(&[reader[0], reader[1], ("sidebar", panel)]);
        missing.extend(nameless(&h, panel));
    }
    let tabs = [
        "appearance",
        "layout",
        "highlights",
        "vocabulary",
        "dictionary",
        "ai",
        "models",
        "panes",
        "motion",
    ];
    for tab in tabs {
        let h = with(&[("epub", "sample:3"), ("panes", "2"), ("settings", tab)]);
        missing.extend(nameless(&h, tab));
    }
    for (state, command) in [
        ("appearance", Sent::Ui(UiCommand::ShowAppearance)),
        ("view menu", Sent::Ui(UiCommand::ShowViewMenu)),
        ("find", Sent::Ui(UiCommand::ShowFind)),
    ] {
        let mut h = with(&reader);
        h.state_mut().queue(command);
        h.run_steps(6);
        missing.extend(nameless(&h, state));
    }
    let shelves = [("books", "8"), ("shelves", "demo"), ("library", "show")];
    missing.extend(nameless(&with(&shelves), "library"));
    let list = [shelves[0], shelves[1], shelves[2], ("layout", "list")];
    missing.extend(nameless(&with(&list), "library list"));

    let mut deck = with(&[
        ("epub", "sample:3"),
        ("panes", "2"),
        ("pinned", "true"),
        ("appearance_menu", "dial_deck"),
    ]);
    deck.state_mut().queue(Sent::Ui(UiCommand::ShowAppearance));
    deck.run_steps(4);
    missing.extend(nameless(&deck, "dial deck look"));
    for tab in ["Text", "Page", "Presets"] {
        deck.get_by_label(tab).click();
        deck.run_steps(3);
        missing.extend(nameless(&deck, tab));
    }
    deck.get_by_label("Save current look as preset").click();
    deck.run_steps(3);
    missing.extend(nameless(&deck, "dial deck preset draft"));

    assert!(
        missing.is_empty(),
        "{} nameless:\n{}",
        missing.len(),
        missing.join("\n")
    );
}
