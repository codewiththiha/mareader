//! Keyboard-only use: Tab's ring, what Enter presses, and modal traps.

use std::path::Path;

use egui::{Event, Key, Modifiers, PointerButton, Pos2, Rect};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_engine::command::Command;
use mareader_engine::library::Action;
use mareader_engine::library::{Pick, PickAction};
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::focus;
use mareader_ui_egui::ui_command::{Sent, UiCommand};

type H = Harness<'static, MareaderApp>;

fn reader(options: &[(&str, &str)]) -> H {
    let mut app = MareaderApp::default();
    for (k, v) in [
        ("workers", "0"),
        ("explain", "canned"),
        ("animations", "false"),
    ] {
        app.set_option(k, v).ok();
    }
    for (k, v) in options {
        app.set_option(k, v).ok();
    }
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(400)
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    h
}

fn key(h: &mut H, modifiers: Modifiers, key: Key) {
    h.key_press_modifiers(modifiers, key);
    h.run_steps(4);
}

/// Tabs until the ring passes `goal`; false after `tries` presses.
fn tab_until(h: &mut H, tries: usize, goal: impl Fn(&H, Rect) -> bool) -> bool {
    for _ in 0..tries {
        key(h, Modifiers::NONE, Key::Tab);
        if focus::rect(&h.ctx).is_some_and(|r| goal(h, r)) {
            return true;
        }
    }
    false
}

fn press(h: &mut H, pos: Pos2) {
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
    h.run_steps(4);
}

/// Page `page`'s first link, the word "quick" and the page, on screen.
fn spots(h: &H, page: usize) -> (Rect, Rect, Rect) {
    let pane = h.state().active_pane();
    let docs = h.state().documents();
    let reading = docs.reading(pane).expect("a document");
    let layer = reading.document.text_layer(page).expect("its text");
    let sheet = *docs
        .sheets(pane)
        .iter()
        .find(|s| s.page == page)
        .expect("on screen");
    let link = layer.links.first().expect("a link").bounds;
    let chars: String = layer.text.chars().iter().map(|c| c.ch).collect();
    let at = chars.find("quick").expect("quick");
    let at = chars.get(..at).unwrap_or_default().chars().count();
    let quick = layer.text.chars().get(at + 2).expect("a char").bounds;
    let word = Rect::from_center_size(sheet.to_screen(quick).center(), egui::Vec2::ZERO);
    (sheet.to_screen(link), word, sheet.rect)
}

fn current(h: &H) -> usize {
    let pane = h.state().active_pane();
    h.state().documents().current(pane).unwrap_or(0)
}

#[test]
fn tab_brings_up_the_title_bar_rings_its_buttons_and_a_press_hides_the_ring() {
    let mut h = reader(&[("pdf", "sample:12")]);
    h.hover_at(egui::pos2(500.0, 400.0));
    h.run_steps(120);
    assert!(!h.state().bar_shown(), "hidden while reading");
    key(&mut h, Modifiers::NONE, Key::Tab);
    assert!(h.state().bar_shown(), "Tab brings the bar up");
    let band = h.state().titlebar().band_bottom(&h.ctx);
    let reached = tab_until(&mut h, 40, |_, r| r.bottom() <= band);
    assert!(reached, "Tab reaches the bar's buttons");
    // Held while the ring is on it, long past the hover grace.
    h.run_steps(150);
    assert!(h.state().bar_shown());
    assert!(focus::keyboard(&h.ctx));
    press(&mut h, egui::pos2(500.0, 400.0));
    assert!(!focus::keyboard(&h.ctx), "a press hides the ring");
    assert!(focus::rect(&h.ctx).is_none());
}

#[test]
fn enter_on_a_focused_link_follows_it() {
    let mut h = reader(&[("pdf", "sample:12")]);
    // Up from under the page bar, which takes no stops beneath it.
    let (link, _, _) = spots(&h, 0);
    h.hover_at(egui::pos2(500.0, 400.0));
    h.event(Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, 350.0 - link.center().y),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(40);
    let (link, _, _) = spots(&h, 0);
    assert!(link.bottom() < 600.0, "scrolled up: {link:?}");
    let on_link = tab_until(&mut h, 10, |_, r| r.center().distance(link.center()) < 1.0);
    assert!(on_link, "the page's links are Tab stops");
    key(&mut h, Modifiers::NONE, Key::Enter);
    h.run_steps(90);
    assert_eq!(current(&h), 5, "the chapter link leads to page 6");
}

#[test]
fn shift_f10_opens_a_mark_s_menu_and_enter_its_card() {
    let mut h = reader(&[
        ("pdf", "sample:5"),
        ("selection", "0:quick"),
        ("gloss", "open"),
    ]);
    h.run();
    let (_, quick, sheet) = spots(&h, 0);
    // A press on the page's blank corner closes the card.
    press(&mut h, sheet.left_top() + egui::vec2(12.0, 12.0));
    assert!(!h.state().study().card_open());
    let on_mark = |_: &H, r: Rect| r.contains(quick.center()) && r.height() < 40.0;
    assert!(tab_until(&mut h, 20, on_mark), "a mark is a Tab stop");
    key(&mut h, Modifiers::SHIFT, Key::F10);
    h.run();
    assert!(h.query_by_label("Remove highlight").is_some(), "its menu");
    key(&mut h, Modifiers::NONE, Key::Escape);
    h.run();
    assert!(h.query_by_label("Remove highlight").is_none());
    assert!(tab_until(&mut h, 20, on_mark));
    key(&mut h, Modifiers::NONE, Key::Enter);
    h.run();
    assert_eq!(h.state().study().card_word(), Some("quick"));
}

#[test]
fn a_library_card_picks_on_shift_enter_and_opens_on_enter() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../epub/tests/books");
    let raven = dir.join("the-raven-gutenberg-17192.epub");
    let raven = raven.canonicalize().expect("the fixture");
    let mut h = reader(&[("epub", raven.to_str().expect("utf-8"))]);
    h.state_mut().queue(Sent::Ui(UiCommand::ShowLibrary));
    h.run();
    let book = h.state().library().recent()[0].id;
    let title = h.state().library().recent()[0].title.value.clone();
    let mut found = false;
    for _ in 0..20 {
        key(&mut h, Modifiers::NONE, Key::Tab);
        if h.get_by_label(&title).is_focused() {
            found = true;
            break;
        }
    }
    assert!(found, "Tab reaches the book's card");
    let picks = |h: &H| h.state().library_state().picks().contains(Pick::Book(book));
    key(&mut h, Modifiers::SHIFT, Key::Enter);
    assert!(picks(&h), "Shift+Enter starts picking");
    key(&mut h, Modifiers::NONE, Key::Enter);
    assert!(!picks(&h), "while picking, Enter toggles");
    assert!(
        h.state().library_state().picks().active(),
        "still selecting"
    );
    // Done ends selecting; the card keeps its focus.
    let done = Action::Pick(PickAction::Clear);
    h.state_mut().queue(Sent::Engine(Command::Library(done)));
    h.run();
    key(&mut h, Modifiers::NONE, Key::Enter);
    h.run();
    assert!(!h.state().library_showing(), "Enter opens it");
}

#[test]
fn tab_stays_in_the_settings_window_and_comes_round() {
    let mut h = reader(&[("pdf", "sample:3")]);
    key(&mut h, Modifiers::COMMAND, Key::Comma);
    h.run();
    assert!(h.state().settings_window().is_open());
    let layer_of = |h: &H| {
        let id = h.ctx.memory(|m| m.focused())?;
        h.ctx
            .viewport(|v| v.this_pass.widgets.get(id).map(|w| w.layer_id.id))
    };
    let window = egui::Id::new("settings-window");
    let mut seen = Vec::new();
    for _ in 0..60 {
        key(&mut h, Modifiers::NONE, Key::Tab);
        assert_eq!(layer_of(&h), Some(window), "focus stays in the window");
        let id = h.ctx.memory(|m| m.focused());
        if seen.contains(&id) {
            return;
        }
        seen.push(id);
    }
    panic!("Tab never came back round: {} stops", seen.len());
}

#[test]
fn f3_steps_through_matches_and_f6_moves_between_panes() {
    let mut h = reader(&[("pdf", "sample:5")]);
    h.hover_at(egui::pos2(500.0, 400.0));
    key(&mut h, Modifiers::COMMAND, Key::F);
    h.get_by_role(egui::accesskit::Role::TextInput)
        .type_text("QUICK");
    h.run_steps(20);
    key(&mut h, Modifiers::NONE, Key::F3);
    h.run_steps(20);
    let at = h.state().find().active().expect("a step lands");
    key(&mut h, Modifiers::NONE, Key::F3);
    assert_eq!(h.state().find().active(), Some(at + 1));
    key(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::G);
    assert_eq!(h.state().find().active(), Some(at));

    key(&mut h, Modifiers::NONE, Key::Escape);
    let split = Sent::Engine(Command::Split(mareader_workspace::SplitAxis::Horizontal));
    h.state_mut().queue(split);
    h.run();
    assert_eq!(h.state().panes().len(), 2);
    let was = h.state().active_pane();
    key(&mut h, Modifiers::NONE, Key::F6);
    assert_ne!(h.state().active_pane(), was, "F6 moves on");
    key(&mut h, Modifiers::SHIFT, Key::F6);
    assert_eq!(h.state().active_pane(), was, "Shift+F6 comes back");
}
