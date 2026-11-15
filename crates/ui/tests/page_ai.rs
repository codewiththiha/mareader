//! The page AI menu and kept results, headless.

use egui::{Event, Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_pdf::TextLayer;
use mareader_settings::PageStart;
use mareader_ui::MareaderApp;
use mareader_ui::command::Command;
use mareader_ui::page_ai::menu::FIND;
use mareader_ui::selection::Sheet;

use crate::common::{ai_reader as reader, find, page};

type H = Harness<'static, MareaderApp>;
type Out = Result<(), Box<dyn std::error::Error>>;

/// A reader with no model at all, even on a Mac.
fn bare() -> H {
    let mut h = reader(&[("pdf", "sample:5")]);
    let mut settings = h.state().settings().clone();
    settings.ai.presets.clear();
    h.state_mut()
        .queue(Command::SetSettings(Box::new(settings)));
    h.run();
    h
}

/// Character `i`'s middle on screen.
fn middle(layer: &TextLayer, sheet: Sheet, i: usize) -> Result<Pos2, String> {
    let c = layer.text.chars().get(i).ok_or("no such character")?;
    Ok(sheet.to_screen(c.bounds).center())
}

/// A blank spot near the page's top left corner.
fn margin(sheet: Sheet) -> Pos2 {
    sheet.shown.min + egui::vec2(12.0, 12.0)
}

/// A press without running to rest, for menus that start work.
fn tap(h: &mut H, pos: Pos2, button: PointerButton) {
    h.hover_at(pos);
    h.step();
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }
    h.run_steps(2);
}

fn press(h: &mut H, pos: Pos2, button: PointerButton) {
    tap(h, pos, button);
    h.run();
}

#[test]
fn without_a_model_the_page_menu_offers_setting_one_up() -> Out {
    let mut h = bare();
    let (_, sheet) = page(&h, 0)?;
    press(&mut h, margin(sheet), PointerButton::Secondary);
    assert!(h.query_by_label("Set up AI…").is_some());
    assert!(h.query_by_label("Summarize page").is_none());
    Ok(())
}

#[test]
fn with_a_model_the_page_menu_offers_the_three_tasks() -> Out {
    let mut h = reader(&[("pdf", "sample:5"), ("provider", "ollama")]);
    let (layer, sheet) = page(&h, 0)?;
    let on_text = middle(&layer, sheet, find(&layer, "quick")? + 2)?;
    press(&mut h, on_text, PointerButton::Secondary);
    for row in ["Summarize page", "Translate page…", "Simplify page"] {
        assert!(h.query_by_label(row).is_some(), "{row} is offered");
    }
    Ok(())
}

#[test]
fn a_picked_language_is_remembered_for_next_time() -> Out {
    let mut h = reader(&[("pdf", "sample:5"), ("provider", "ollama")]);
    let (_, sheet) = page(&h, 0)?;
    press(&mut h, margin(sheet), PointerButton::Secondary);
    h.get_by_label("Translate page…").click();
    h.run();
    h.get_by_label(FIND).type_text("burm");
    h.run();
    h.get_by_label("Burmese").click();
    h.run();
    h.get_by_label("This page").click();
    // The run starts at once; the busy bubble keeps the frames coming.
    h.run_steps(3);
    let translate = &h.state().settings().ai.tasks.translate;
    assert_eq!(translate.language.as_deref(), Some("my"));
    assert_eq!(translate.recent.first().map(String::as_str), Some("my"));
    Ok(())
}

#[test]
fn a_mark_or_a_selection_keeps_its_own_menu() -> Out {
    let mut h = reader(&[
        ("pdf", "sample:5"),
        ("provider", "ollama"),
        ("selection", "0:quick"),
        ("gloss", "open"),
    ]);
    h.run();
    let (layer, sheet) = page(&h, 0)?;
    let marked = middle(&layer, sheet, find(&layer, "quick")? + 2)?;
    press(&mut h, marked, PointerButton::Secondary);
    assert!(h.query_by_label("Remove highlight").is_some());
    assert!(h.query_by_label("Summarize page").is_none());
    Ok(())
}

#[test]
fn a_kept_result_covers_the_page_until_the_original_is_asked_for() -> Out {
    let mut h = reader(&[
        ("pdf", "sample:5"),
        ("provider", "ollama"),
        ("page_ai", "0:simplify"),
    ]);
    h.run_steps(4);
    h.run();
    let pane = h.state().active_pane();
    assert!(h.state().page_ai().shows(pane, 0), "the result shows");
    let (_, sheet) = page(&h, 0)?;
    press(&mut h, margin(sheet), PointerButton::Secondary);
    h.get_by_label("Show original page").click();
    h.run();
    assert!(!h.state().page_ai().shows(pane, 0));
    Ok(())
}

#[test]
fn the_wheel_over_a_result_scrolls_the_book_not_the_result() -> Out {
    let mut h = reader(&[
        ("pdf", "sample:5"),
        ("provider", "ollama"),
        ("page_ai", "0:simplify"),
    ]);
    h.run_steps(4);
    h.run();
    let pane = h.state().active_pane();
    assert!(h.state().page_ai().shows(pane, 0), "the result shows");
    let (_, before) = page(&h, 0)?;
    h.hover_at(before.shown.center());
    h.step();
    h.event(Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -300.0),
        modifiers: Modifiers::NONE,
        phase: egui::TouchPhase::Move,
    });
    h.run();
    h.run_steps(2);
    let (_, after) = page(&h, 0)?;
    let moved = before.rect.top() - after.rect.top();
    assert!(moved > 100.0, "the page moved {moved}");
    assert!(h.state().page_ai().shows(pane, 0), "and its result with it");
    Ok(())
}

/// Starts Simplify in continuous mode from the page menu.
fn simplify_on(h: &mut H, remember: bool) -> Result<(), String> {
    let (_, sheet) = page(h, 0)?;
    press(h, margin(sheet), PointerButton::Secondary);
    h.get_by_label("Simplify page").click();
    h.run();
    if remember {
        h.get_by_label("Remember my choice").click();
        h.run();
    }
    h.get_by_label("Continuous").click();
    // Its first page is being written: the frames keep coming.
    h.run_steps(3);
    Ok(())
}

#[test]
fn simplify_asks_how_and_can_remember_continuous() -> Out {
    let mut h = reader(&[("pdf", "sample:40"), ("provider", "ollama")]);
    let pane = h.state().active_pane();
    simplify_on(&mut h, true)?;
    let start = h.state().settings().ai.tasks.simplify.start;
    assert_eq!(start, PageStart::Continuous);
    assert!(h.state().page_ai().following(pane).is_some());
    Ok(())
}

#[test]
fn a_task_set_to_this_page_runs_without_asking() -> Out {
    let mut h = reader(&[("pdf", "sample:40"), ("provider", "ollama")]);
    let pane = h.state().active_pane();
    let mut settings = h.state().settings().clone();
    settings.ai.tasks.simplify.start = PageStart::Page;
    h.state_mut()
        .queue(Command::SetSettings(Box::new(settings)));
    h.run();
    let (_, sheet) = page(&h, 0)?;
    press(&mut h, margin(sheet), PointerButton::Secondary);
    h.get_by_label("Simplify page").click();
    h.run_steps(3);
    assert!(h.query_by_label("Continuous").is_none(), "no second step");
    assert!(h.state().page_ai().following(pane).is_none());
    assert!(
        h.state().page_ai().shows(pane, 0),
        "the page is being written"
    );
    Ok(())
}

#[test]
fn the_pill_stops_continuous_mode() -> Out {
    let mut h = reader(&[("pdf", "sample:40"), ("provider", "ollama")]);
    let pane = h.state().active_pane();
    simplify_on(&mut h, false)?;
    h.get_by_label("Stop").click();
    h.run_steps(3);
    assert!(h.state().page_ai().following(pane).is_none());
    Ok(())
}

#[test]
fn going_far_away_pauses_then_asks_to_carry_on_there() -> Out {
    let mut h = reader(&[("pdf", "sample:40"), ("provider", "ollama")]);
    let pane = h.state().active_pane();
    simplify_on(&mut h, false)?;
    h.state_mut().queue(Command::GoTo { pane, page: 30 });
    // Kept still a moment far from page 0, it asks.
    h.run_steps(120);
    assert!(h.query_by_label("Continue").is_some(), "the pill asks");
    h.get_by_label("Continue").click();
    h.run_steps(3);
    assert!(h.query_by_label("Continue").is_none());
    assert!(h.state().page_ai().following(pane).is_some());
    assert!(h.state().page_ai().shows(pane, 30), "it carries on there");
    Ok(())
}

#[test]
fn staying_on_a_done_page_asks_the_next_pages() -> Out {
    let mut h = reader(&[("pdf", "sample:40"), ("provider", "ollama")]);
    let pane = h.state().active_pane();
    let mut settings = h.state().settings().clone();
    settings.ai.tasks.pages.dwell = 5;
    settings.ai.tasks.pages.at_once = 3;
    h.state_mut()
        .queue(Command::SetSettings(Box::new(settings)));
    h.run();
    simplify_on(&mut h, false)?;
    h.run_steps(30);
    let ai = h.state().page_ai();
    assert!(ai.shows(pane, 0), "the first page is done");
    assert!(!ai.shows(pane, 1), "nothing ahead before the stay");
    // Five seconds on the page, at 60 frames a second.
    h.run_steps(330);
    let ai = h.state().page_ai();
    for p in 1..=3 {
        assert!(ai.shows(pane, p), "page {p} is asked");
    }
    assert!(!ai.shows(pane, 4), "no more than pages at once");
    Ok(())
}

#[test]
fn a_pdf_takes_type_settings_while_an_ai_page_shows() {
    use mareader_ui::settings_window::Tab;
    let open = |h: &mut H| {
        h.set_size(egui::vec2(1000.0, 1200.0));
        h.state_mut().queue(Command::OpenSettings(Tab::Appearance));
        h.run();
    };
    let mut plain = reader(&[("pdf", "sample:5")]);
    open(&mut plain);
    assert!(plain.query_by_label("More text size").is_none());
    assert!(!plain.state().settings_scope().ai_pages);
    let mut h = reader(&[
        ("pdf", "sample:5"),
        ("provider", "ollama"),
        ("page_ai", "0:simplify"),
    ]);
    h.run_steps(4);
    open(&mut h);
    assert!(h.state().settings_scope().ai_pages);
    let size = h.state().settings().text.font_size;
    h.get_by_label("More text size").click();
    h.run();
    assert!(
        h.state().settings().text.font_size > size,
        "the result's type"
    );
    // A book's own layout stays a book's.
    assert!(h.query_all_by_label("Book layout").next().is_none());
    // Turned to a page of its own, the PDF has no type to set.
    let pane = h.state().active_pane();
    h.state_mut().queue(Command::CloseSettings);
    h.state_mut().queue(Command::GoTo { pane, page: 4 });
    h.run_steps(4);
    open(&mut h);
    assert!(!h.state().settings_scope().ai_pages);
    assert!(h.query_by_label("More text size").is_none());
}

/// The book in front: its pages, its own pages, and what is laid in.
fn book(h: &H) -> Result<(usize, usize, u64), String> {
    let pane = h.state().active_pane();
    let docs = h.state().documents();
    let reading = docs.reading(pane).ok_or("no document")?;
    let d = &reading.document;
    Ok((d.page_count(), d.base_count(), d.inserts_key()))
}

/// A book open on page 3 with page 3's kept result shown.
fn laid() -> Result<H, String> {
    let mut h = reader(&[
        ("epub", "sample:3"),
        ("provider", "ollama"),
        ("page_ai", "2:simplify"),
    ]);
    let pane = h.state().active_pane();
    h.state_mut().queue(Command::GoTo { pane, page: 2 });
    h.run_steps(30);
    h.run();
    let (count, base, key) = book(&h)?;
    if key == 0 || count < base {
        return Err(format!("not laid in: {count} of {base}, key {key}"));
    }
    Ok(h)
}

#[test]
fn a_book_lays_a_kept_result_into_its_pages_and_takes_it_out() -> Out {
    let mut h = laid()?;
    let pane = h.state().active_pane();
    assert!(h.state().page_ai().shows(pane, 2), "the result shows");
    let (_, sheet) = page(&h, 2)?;
    press(&mut h, margin(sheet), PointerButton::Secondary);
    h.get_by_label("Show original page").click();
    h.run_steps(30);
    h.run();
    let (count, base, key) = book(&h)?;
    assert_eq!((count, key), (base, 0), "back to its own pages");
    assert!(!h.state().page_ai().shows(pane, 2));
    Ok(())
}

#[test]
fn new_type_lays_the_result_out_again() -> Out {
    let mut h = laid()?;
    let (_, base, _) = book(&h)?;
    let mut settings = h.state().settings().clone();
    settings.text.font_size *= 1.6;
    h.state_mut()
        .queue(Command::SetSettings(Box::new(settings)));
    h.run_steps(30);
    h.run();
    let (count, more, key) = book(&h)?;
    assert!(more > base, "bigger type, more pages: {more} of {base}");
    assert!(key != 0 && count >= more, "the result is laid in again");
    Ok(())
}

/// Picks `action` from the menu of the page's first result bubble.
fn bubble_menu(h: &mut H, action: &str) {
    let bubble = h.get_by_label("Simplified").rect().center();
    press(h, bubble, PointerButton::Secondary);
    h.get_by_label(action).click();
    h.run_steps(30);
    h.run();
}

#[test]
fn deleting_a_laid_result_gives_the_book_its_pages_back() -> Out {
    let mut h = laid()?;
    bubble_menu(&mut h, "Delete");
    let (count, base, key) = book(&h)?;
    assert_eq!((count, key), (base, 0), "back to its own pages");
    let pane = h.state().active_pane();
    assert!(!h.state().page_ai().shows(pane, 2));
    Ok(())
}

#[test]
fn a_redo_keeps_the_result_in_the_pages_until_done() -> Out {
    let mut h = laid()?;
    bubble_menu(&mut h, "Redo");
    let (count, base, key) = book(&h)?;
    assert!(key != 0 && count >= base, "still laid in");
    let pane = h.state().active_pane();
    assert!(h.state().page_ai().shows(pane, 2), "the new result shows");
    Ok(())
}

#[test]
fn toggling_a_result_again_and_again_keeps_one_book_open() -> Out {
    let mut h = laid()?;
    for _ in 0..8 {
        let bubble = h.get_by_label("Simplified").rect().center();
        press(&mut h, bubble, PointerButton::Primary);
        h.run_steps(30);
        h.run();
    }
    let (count, base, key) = book(&h)?;
    assert_eq!((count, key), (base, 0), "an even count of clicks: off");
    let open = h.state().documents().meter().documents;
    assert!(open <= 2, "{open} layouts of the book kept");
    Ok(())
}

#[test]
fn the_page_asked_about_is_washed_while_its_menu_is_open() -> Out {
    let mut h = reader(&[("pdf", "sample:5"), ("provider", "ollama")]);
    let pane = h.state().active_pane();
    let (_, sheet) = page(&h, 0)?;
    press(&mut h, margin(sheet), PointerButton::Secondary);
    assert_eq!(h.state().page_ai().washed(pane), Some(&[0][..]));
    // Closing the menu fades the wash away.
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(h.state().page_ai().washed(pane), None);
    Ok(())
}

#[test]
fn remembered_language_uses_translate_submenu_without_extra_row() -> Out {
    let mut h = reader(&[("pdf", "sample:5"), ("provider", "ollama")]);
    {
        let translate = &mut h.state_mut().settings_mut().ai.tasks.translate;
        translate.language = Some("ja".to_owned());
        translate.recent = vec!["ja".to_owned(), "my".to_owned()];
    }
    let (_, sheet) = page(&h, 0)?;
    press(&mut h, margin(sheet), PointerButton::Secondary);
    assert!(h.query_by_label("Translate page").is_some());
    assert!(h.query_by_label("Other language…").is_none());
    h.get_by_label("Translate page").click();
    h.run();
    assert!(h.query_by_label("Japanese").is_some());
    h.get_by_label("More languages…").click();
    h.run();
    assert!(h.query_by_label(FIND).is_some());
    Ok(())
}
