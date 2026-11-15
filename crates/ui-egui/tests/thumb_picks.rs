//! Picking pages in the thumbnails, headless, and copying their text.

use egui::{Key, Modifiers, OutputCommand};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use mareader_engine::host::Machine;
use mareader_ui_egui::MareaderApp;

use crate::common::{Dials, Window};

type H = Harness<'static, MareaderApp>;

fn reader() -> H {
    // Built in the harness's context, so a copy lands where the suite looks.
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(200)
        .build_eframe(|cc| {
            let mut app =
                MareaderApp::on(&cc.egui_ctx, Window::host(&cc.egui_ctx), Machine::default());
            app.set_option("workers", "0").ok();
            app.set_option("pdf", "sample:40").ok();
            app.set_option("sidebar", "thumbnails").ok();
            app
        });
    h.run_steps(3);
    h.run();
    h
}

fn picks(h: &H) -> Vec<usize> {
    h.state().thumb_picks()
}

fn current(h: &H) -> usize {
    let pane = h.state().active_pane();
    h.state().documents().current(pane).unwrap_or(usize::MAX)
}

fn cell(h: &H, page: usize) -> egui::Pos2 {
    h.get_by_label(&format!("Page {page}")).rect().center()
}

fn click(h: &mut H, page: usize, modifiers: Modifiers) {
    h.get_by_label(&format!("Page {page}"))
        .click_modifiers(modifiers);
    h.run();
}

/// Presses page `page` and holds still for `seconds`, then lets go.
fn long_press(h: &mut H, page: usize, seconds: f32) {
    let at = cell(h, page);
    h.hover_at(at);
    h.run();
    h.drag_at(at);
    h.run();
    h.run_steps((seconds * 60.0).ceil() as usize);
    h.event(egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run();
}

fn menu(h: &mut H, page: usize, row: &str) {
    h.get_by_label(&format!("Page {page}")).click_secondary();
    h.run();
    h.get_by_label(row).click();
}

/// What reaches the clipboard within a few frames.
fn copied(h: &mut H) -> Option<String> {
    for _ in 0..10 {
        h.step();
        let text = (h.output().platform_output.commands.iter()).find_map(|c| match c {
            OutputCommand::CopyText(t) => Some(t.clone()),
            _ => None,
        });
        if text.is_some() {
            return text;
        }
    }
    None
}

/// Page `page`'s own text, as the reader holds it.
fn text_of(h: &H, page: usize) -> String {
    let pane = h.state().active_pane();
    let reading = h.state().documents().reading(pane).unwrap();
    let layer = reading.document.text_layer(page).unwrap();
    layer.text.text(0..layer.text.len()).trim().to_owned()
}

#[test]
fn a_long_press_picks_a_page_without_going_there() {
    let mut h = reader();
    long_press(&mut h, 3, 0.6);
    assert_eq!(picks(&h), vec![2]);
    assert_eq!(current(&h), 0, "the release is no click");
    assert!(h.query_by_label("1 page selected").is_some());
    // Picking now, a plain click picks too, and again lets go.
    click(&mut h, 5, Modifiers::NONE);
    assert_eq!(picks(&h), vec![2, 4]);
    click(&mut h, 3, Modifiers::NONE);
    assert_eq!(picks(&h), vec![4]);
    assert_eq!(current(&h), 0);
}

#[test]
fn a_short_press_still_goes_to_the_page() {
    let mut h = reader();
    long_press(&mut h, 4, 0.2);
    assert!(picks(&h).is_empty());
    assert_eq!(current(&h), 3);
}

#[test]
fn modifier_clicks_pick_and_shift_takes_the_range() {
    let mut h = reader();
    click(&mut h, 2, Modifiers::COMMAND);
    assert_eq!(picks(&h), vec![1]);
    click(&mut h, 6, Modifiers::SHIFT);
    assert_eq!(picks(&h), vec![1, 2, 3, 4, 5]);
    assert_eq!(current(&h), 0);
    h.get_by_label("Clear").click();
    h.run();
    assert!(picks(&h).is_empty());
    h.get_by_label("Page 2").click();
    h.run();
    assert_eq!(current(&h), 1, "no picks, a click reads");
}

#[test]
fn escape_lets_go_of_the_picks_before_the_sidebar() {
    let mut h = reader();
    click(&mut h, 2, Modifiers::COMMAND);
    h.key_press(Key::Escape);
    h.run();
    assert!(picks(&h).is_empty());
    assert!(h.query_by_label("Page 1").is_some(), "the sidebar stays");
    h.key_press(Key::Escape);
    h.run();
    assert!(h.query_by_label("Page 1").is_none());
}

#[test]
fn the_menu_copies_the_picked_pages_in_order() {
    let mut h = reader();
    click(&mut h, 3, Modifiers::COMMAND);
    click(&mut h, 1, Modifiers::COMMAND);
    menu(&mut h, 3, "Copy text");
    let expected = format!("{}\n\n{}", text_of(&h, 0), text_of(&h, 2));
    assert!(!text_of(&h, 0).is_empty());
    assert_eq!(copied(&mut h), Some(expected));
    assert_eq!(picks(&h), vec![0, 2], "copying keeps the picks");
}

#[test]
fn an_unpicked_page_copies_or_picks_itself() {
    let mut h = reader();
    menu(&mut h, 4, "Copy page text");
    assert_eq!(copied(&mut h), Some(text_of(&h, 3)));
    h.run();
    menu(&mut h, 4, "Select");
    h.run();
    assert_eq!(picks(&h), vec![3]);
}

#[test]
fn picked_menu_offers_quick_tiles_and_clears_or_selects_all() {
    let mut h = reader();
    click(&mut h, 2, Modifiers::COMMAND);
    h.get_by_label("Page 2").click_secondary();
    h.run();
    assert!(h.query_by_label("Copy text").is_some());
    assert!(h.query_by_label("Word list").is_some());
    h.get_by_label("Select all").click();
    h.run();
    assert_eq!(picks(&h).len(), 40);
    h.get_by_label("Page 2").click_secondary();
    h.run();
    h.get_by_label("Clear selection").click();
    h.run();
    assert!(picks(&h).is_empty());
}

#[test]
fn translation_submenu_offers_recent_and_all_languages() {
    let mut h = reader();
    h.state_mut().set_option("provider", "openai").unwrap();
    h.dial(|s| s.ai.tasks.translate.pick("fr"));
    h.dial(|s| s.ai.tasks.translate.pick("my"));
    h.run();
    click(&mut h, 2, Modifiers::COMMAND);
    h.get_by_label("Page 2").click_secondary();
    h.run();
    h.get_by_label("Translate").click();
    h.run();
    assert!(h.query_by_label("French").is_some());
    assert!(h.query_by_label("Burmese").is_some());
    h.get_by_label("More languages…").click();
    h.run();
    assert!(h.query_by_label("Find a language").is_some());
}
