//! Picked pages asked in groups, headless, with canned answers.

use egui::{Event, Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_engine::command::Command;
use mareader_engine::page_ai::Action;
use mareader_library::{AiTask, PageSpan};
use mareader_settings::PageTask;
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::ui_command::Sent;

type H = Harness<'static, MareaderApp>;

fn reader(options: &[(&str, &str)]) -> H {
    let mut app = MareaderApp::default();
    for (k, v) in [
        ("workers", "0"),
        ("explain", "canned"),
        ("animations", "false"),
        ("provider", "ollama"),
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

fn ask(h: &mut H, pages: impl IntoIterator<Item = usize>, task: PageTask) {
    let pane = h.state().active_pane();
    let pages = pages.into_iter().collect();
    let action = Action::RunPages {
        pane,
        pages,
        task,
        tag: None,
    };
    h.state_mut().queue(Sent::Engine(Command::PageAi(action)));
    h.run_steps(30);
    h.run();
}

fn spans(h: &H) -> Vec<PageSpan> {
    let pane = h.state().active_pane();
    let kept = h.state().kept_ai(pane).into_iter();
    kept.map(|(_, span)| span).collect()
}

fn shows(h: &H, page: usize) -> bool {
    let pane = h.state().active_pane();
    h.state().page_ai().shows(pane, page)
}

fn go(h: &mut H, page: usize) {
    let pane = h.state().active_pane();
    h.state_mut()
        .queue(Sent::Engine(Command::GoTo { pane, page }));
    h.run_steps(30);
    h.run();
}

/// A blank spot near page `page`'s top left corner, which must be shown.
fn margin(h: &H, page: usize) -> Pos2 {
    let pane = h.state().active_pane();
    let sheets = h.state().documents().sheets(pane);
    let sheet = sheets.iter().find(|s| s.page == page);
    sheet.map(|s| s.shown.min).unwrap() + egui::vec2(12.0, 12.0)
}

fn press(h: &mut H, pos: Pos2, button: PointerButton) {
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
    h.run();
}

#[test]
fn eleven_pages_go_as_five_five_and_one() {
    let mut h = reader(&[("pdf", "sample:12")]);
    ask(&mut h, 0..11, PageTask::Summarize);
    let pane = h.state().active_pane();
    let kept = h.state().kept_ai(pane);
    assert!(kept.iter().all(|(task, _)| *task == AiTask::Summary));
    assert_eq!(
        spans(&h),
        [
            PageSpan::Pages { from: 0, to: 4 },
            PageSpan::Pages { from: 5, to: 9 },
            PageSpan::Page { page: 10 },
        ]
    );
    assert!(shows(&h, 0), "the first group shows");
    // Asking again shows what is kept rather than asking anew.
    ask(&mut h, 0..11, PageTask::Summarize);
    assert_eq!(spans(&h).len(), 3);
}

#[test]
fn a_groups_later_pages_carry_it_and_the_original_comes_back_whole() {
    let mut h = reader(&[("pdf", "sample:12")]);
    ask(&mut h, 0..5, PageTask::Simplify);
    go(&mut h, 2);
    assert!(shows(&h, 2), "a later page of the group is covered");
    let spot = margin(&h, 2);
    press(&mut h, spot, PointerButton::Secondary);
    h.get_by_label("Show original page").click();
    h.run();
    assert!(!shows(&h, 2));
    go(&mut h, 0);
    assert!(!shows(&h, 0), "the whole group gave way");
}

#[test]
fn pages_per_request_sets_the_group_size() {
    let mut h = reader(&[("pdf", "sample:12")]);
    let mut settings = h.state().settings().clone();
    settings.ai.tasks.pages.per_request = 2;
    h.state_mut()
        .queue(Sent::Engine(Command::SetSettings(Box::new(settings))));
    h.run();
    // Gaps split groups too: 7 is on its own.
    ask(&mut h, [0, 1, 2, 3, 4, 7], PageTask::Simplify);
    assert_eq!(
        spans(&h),
        [
            PageSpan::Pages { from: 0, to: 1 },
            PageSpan::Pages { from: 2, to: 3 },
            PageSpan::Page { page: 4 },
            PageSpan::Page { page: 7 },
        ]
    );
}

#[test]
fn a_group_redone_replaces_its_one_result() {
    let mut h = reader(&[("pdf", "sample:12")]);
    ask(&mut h, 0..5, PageTask::Simplify);
    let bubble = h.get_by_label("Simplified · pages 1–5").rect().center();
    press(&mut h, bubble, PointerButton::Secondary);
    h.get_by_label("Redo").click();
    h.run_steps(30);
    h.run();
    assert_eq!(spans(&h), [PageSpan::Pages { from: 0, to: 4 }]);
    assert!(shows(&h, 0));
}

#[test]
fn a_books_group_is_one_result_laid_over_its_pages() {
    let mut h = reader(&[("epub", "sample:3")]);
    let pane = h.state().active_pane();
    let base = |h: &H| {
        let reading = h.state().documents().reading(pane).unwrap();
        (
            reading.document.base_count(),
            reading.document.inserts_key(),
        )
    };
    let (count, key) = base(&h);
    assert!(count >= 3, "{count} pages");
    assert_eq!(key, 0);
    ask(&mut h, 0..3, PageTask::Simplify);
    let kept = spans(&h);
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert!(matches!(kept.first(), Some(PageSpan::Blocks { .. })));
    assert!(base(&h).1 != 0, "laid into the pages");
}

#[test]
fn the_thumbnails_menu_asks_the_picked_pages() {
    let mut h = reader(&[("pdf", "sample:12"), ("sidebar", "thumbnails")]);
    for page in ["Page 2", "Page 3"] {
        h.get_by_label(page).click_modifiers(Modifiers::COMMAND);
        h.run();
    }
    h.get_by_label("Page 3").click_secondary();
    h.run();
    h.get_by_label("Summarize").click();
    h.run_steps(30);
    h.run();
    assert_eq!(spans(&h), [PageSpan::Pages { from: 1, to: 2 }]);
    assert_eq!(h.state().thumb_picks(), [1, 2], "the picks stay");
}

#[test]
fn a_books_group_covers_all_its_pages_while_it_writes() {
    let mut h = reader(&[("epub", "sample:3"), ("page_ai_hold", "true")]);
    let pane = h.state().active_pane();
    let action = Action::RunPages {
        pane,
        pages: vec![0, 1, 2],
        task: PageTask::Simplify,
        tag: None,
    };
    // Held runs keep the pill busy, so steps, not `run`.
    h.state_mut().queue(Sent::Engine(Command::PageAi(action)));
    h.run_steps(30);
    assert!(spans(&h).is_empty(), "still writing");
    assert!(shows(&h, 0), "the first page shows the words so far");
    h.state_mut()
        .queue(Sent::Engine(Command::GoTo { pane, page: 2 }));
    h.run_steps(30);
    assert!(shows(&h, 2), "a later page is covered too");
    let reading = h.state().documents().reading(pane).unwrap();
    assert_eq!(reading.document.inserts_key(), 0, "nothing laid in yet");
}
