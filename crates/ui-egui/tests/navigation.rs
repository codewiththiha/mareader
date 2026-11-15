//! Navigation, headless: the outline, thumbnails and the page bar.

use egui::accesskit::{Role, Toggled};
use egui::{Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use mareader_engine::command::Command;
use mareader_engine::pane::ViewMode;
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::ui_command::Sent;

type H = Harness<'static, MareaderApp>;

fn reader(options: &[(&str, &str)]) -> H {
    let mut app = MareaderApp::default();
    app.set_option("workers", "0").ok();
    app.set_option("pdf", "sample:40").ok();
    for (k, v) in options {
        app.set_option(k, v).ok();
    }
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(200)
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    h
}

fn current(h: &H) -> usize {
    let pane = h.state().active_pane();
    h.state().documents().current(pane).unwrap_or(usize::MAX)
}

fn go(h: &mut H, page: usize) {
    let pane = h.state().active_pane();
    h.state_mut()
        .queue(Sent::Engine(Command::GoTo { pane, page }));
    h.run();
}

/// Brings the bar up by pointing at the pane's bottom edge.
fn raise_bar(h: &mut H) {
    h.hover_at(egui::pos2(600.0, 697.0));
    h.run();
}

fn selected(h: &H, label: &str) -> Option<bool> {
    h.query_by_label(label)
        .map(|n| n.accesskit_node().toggled() == Some(Toggled::True))
}

#[test]
fn clicking_a_chapter_jumps_there_and_marks_it() {
    let mut h = reader(&[("sidebar", "outline")]);
    assert_eq!(selected(&h, "Chapter 1: Opening"), Some(true));
    h.get_by_label("Chapter 3: Reading").click();
    h.run();
    assert_eq!(current(&h), 10);
    assert_eq!(selected(&h, "Chapter 3: Reading"), Some(true));
    assert_eq!(selected(&h, "Chapter 1: Opening"), Some(false));
    // Reading on marks the deepest entry holding the page.
    go(&mut h, 12);
    assert_eq!(selected(&h, "3.1 Scrolling"), Some(true));
}

fn book(options: &[(&str, &str)]) -> H {
    let mut app = MareaderApp::default();
    app.set_option("workers", "0").ok();
    for (k, v) in options {
        app.set_option(k, v).ok();
    }
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(200)
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    h
}

fn outline_page(h: &H, title: &str) -> Option<usize> {
    let pane = h.state().active_pane();
    let reading = h.state().documents().reading(pane)?;
    let entry = reading.document.outline().iter().find(|e| e.title == title);
    entry.map(|e| e.page)
}

#[test]
fn a_book_s_contents_jump_to_its_parts_and_follow_the_page() {
    let mut h = book(&[
        ("epub", "sample:3"),
        ("sidebar", "outline"),
        ("view", "single"),
        ("animations", "false"),
    ]);
    let chapter = outline_page(&h, "Chapter 1");
    let part = outline_page(&h, "Part 1.2");
    assert!(chapter.is_some() && chapter < part, "{chapter:?} {part:?}");
    h.get_by_label("Part 1.2").click();
    h.run();
    assert_eq!(Some(current(&h)), part);
    assert_eq!(selected(&h, "Part 1.2"), Some(true));
    // Reading back marks the chapter again.
    go(&mut h, chapter.unwrap_or_default());
    assert_eq!(selected(&h, "Chapter 1"), Some(true));
    assert_eq!(selected(&h, "Part 1.2"), Some(false));
}

#[test]
fn the_thumbnails_follow_the_page_and_a_click_jumps() {
    let mut h = reader(&[("sidebar", "thumbnails")]);
    assert_eq!(selected(&h, "Page 1"), Some(true));
    // Far pages aren't drawn until the reader gets near them.
    assert!(h.query_by_label("Page 31").is_none());
    go(&mut h, 30);
    h.run_steps(30);
    assert_eq!(selected(&h, "Page 31"), Some(true));
    assert!(h.query_by_label("Page 1").is_none());
    h.get_by_label("Page 32").click();
    h.run();
    assert_eq!(current(&h), 31);
}

#[test]
fn the_page_entry_jumps_and_snaps_back_from_nonsense() {
    let mut h = reader(&[]);
    raise_bar(&mut h);
    let enter = |h: &mut H, text: &str| {
        h.get_by_role(Role::TextInput).click();
        h.run();
        h.key_press_modifiers(Modifiers::COMMAND, Key::A);
        h.get_by_role(Role::TextInput).type_text(text);
        h.key_press(Key::Enter);
        h.run();
    };
    enter(&mut h, "12");
    assert_eq!(current(&h), 11);
    enter(&mut h, "page");
    assert_eq!(current(&h), 11);
    // Past the end lands on the last page.
    enter(&mut h, "400");
    assert_eq!(current(&h), 39);
    assert_eq!(
        h.get_by_role(Role::TextInput)
            .accesskit_node()
            .value()
            .as_deref(),
        Some("40")
    );
}

#[test]
fn the_bar_turns_by_spreads_and_hides_when_left() {
    let mut h = reader(&[]);
    h.state_mut()
        .queue(Sent::Engine(Command::View(ViewMode::Spread)));
    h.run();
    raise_bar(&mut h);
    h.get_by_label("Next page").click();
    h.run();
    assert_eq!(current(&h), 2);
    h.get_by_label("Previous page").click();
    h.run();
    assert_eq!(current(&h), 0);
    h.hover_at(egui::pos2(600.0, 300.0));
    h.run_steps(60);
    assert!(h.query_by_label("Next page").is_none());
}

#[test]
fn a_short_page_jumped_to_in_a_stream_stays_the_page_read() {
    // Its front matter breaks often: pages far shorter than the pane.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../epub/tests/books");
    let raven = dir.join("the-raven-gutenberg-17192.epub");
    let mut h = book(&[("epub", raven.to_str().unwrap_or_default())]);
    h.state_mut()
        .queue(Sent::Engine(Command::View(ViewMode::Vertical)));
    h.run();
    for page in 1..6 {
        go(&mut h, page);
        h.run_steps(3);
        assert_eq!(current(&h), page);
    }
    // Moving the strip hands the page back to the one under its centre.
    h.hover_at(egui::pos2(600.0, 350.0));
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -3000.0),
        modifiers: Modifiers::NONE,
        phase: egui::TouchPhase::Move,
    });
    h.run();
    h.run_steps(3);
    assert!(current(&h) > 5, "{}", current(&h));
}
