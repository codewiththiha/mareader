//! The window's own chrome, headless: captions, drag, edges, title, dividers.

use std::time::Duration;

use egui::{Event, Modifiers, PointerButton, Pos2, ViewportCommand, ViewportId, pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::ui_command::{Sent, UiCommand};

const SIZE: egui::Vec2 = egui::vec2(1200.0, 800.0);

fn with(options: &[(&str, &str)], size: egui::Vec2) -> Harness<'static, MareaderApp> {
    let mut app = MareaderApp::default();
    app.set_option("workers", "0").ok();
    for (k, v) in options {
        app.set_option(k, v).ok();
    }
    let mut h = Harness::builder()
        .with_size(size)
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(400)
        .build_eframe(|_| app);
    h.run_steps(4);
    h
}

fn commands(h: &Harness<'static, MareaderApp>) -> Vec<ViewportCommand> {
    let out = h.output().viewport_output.get(&ViewportId::ROOT);
    out.map(|v| v.commands.clone()).unwrap_or_default()
}

/// Steps a few frames; true once one of them sends a matching command.
fn sends(h: &mut Harness<'static, MareaderApp>, want: impl Fn(&ViewportCommand) -> bool) -> bool {
    for _ in 0..4 {
        h.step();
        if commands(h).iter().any(&want) {
            return true;
        }
    }
    false
}

fn button(h: &mut Harness<'static, MareaderApp>, at: Pos2, pressed: bool) {
    h.event(Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    });
}

/// The rightmost node so named, as caption buttons sit at the bar's end.
fn rightmost(h: &Harness<'static, MareaderApp>, label: &str) -> Option<Pos2> {
    let boxes = h.query_all_by_label(label).filter_map(|n| {
        let b = n.accesskit_node().bounding_box()?;
        Some(pos2(
            ((b.x0 + b.x1) / 2.0) as f32,
            ((b.y0 + b.y1) / 2.0) as f32,
        ))
    });
    boxes.max_by(|a, b| a.x.total_cmp(&b.x))
}

fn click_caption(h: &mut Harness<'static, MareaderApp>, label: &str) {
    let at = rightmost(h, label).unwrap_or_else(|| panic!("no {label} button"));
    h.hover_at(at);
    h.step();
    button(h, at, true);
    button(h, at, false);
}

#[test]
fn windows_captions_minimise_maximise_and_close() {
    let mut h = with(&[("platform", "windows"), ("pinned", "true")], SIZE);
    click_caption(&mut h, "Minimize");
    let min = sends(&mut h, |c| matches!(c, ViewportCommand::Minimized(true)));
    assert!(min, "Minimize sends Minimized");
    click_caption(&mut h, "Maximize");
    let max = sends(&mut h, |c| matches!(c, ViewportCommand::Maximized(true)));
    assert!(max, "Maximize sends Maximized");
    click_caption(&mut h, "Close");
    assert!(sends(&mut h, |c| matches!(c, ViewportCommand::Close)));
}

#[test]
fn gnome_captions_minimise_and_close() {
    let mut h = with(&[("platform", "gnome"), ("pinned", "true")], SIZE);
    click_caption(&mut h, "Minimize");
    let min = sends(&mut h, |c| matches!(c, ViewportCommand::Minimized(true)));
    assert!(min, "Minimize sends Minimized");
    click_caption(&mut h, "Close");
    assert!(sends(&mut h, |c| matches!(c, ViewportCommand::Close)));
}

#[test]
fn the_empty_bar_drags_the_window_and_a_double_click_maximises() {
    let mut h = with(&[("platform", "windows"), ("pinned", "true")], SIZE);
    let at = pos2(450.0, 24.0);
    h.hover_at(at);
    h.step();
    button(&mut h, at, true);
    h.step();
    h.hover_at(at + egui::vec2(30.0, 4.0));
    assert!(sends(&mut h, |c| matches!(c, ViewportCommand::StartDrag)));
    button(&mut h, at + egui::vec2(30.0, 4.0), false);
    h.run_steps(30);
    h.hover_at(at);
    h.step();
    for _ in 0..2 {
        button(&mut h, at, true);
        button(&mut h, at, false);
        h.step();
    }
    let max = |c: &ViewportCommand| matches!(c, ViewportCommand::Maximized(true));
    assert!(commands(&h).iter().any(max) || sends(&mut h, max));
}

#[test]
fn a_frameless_windows_edge_starts_a_resize_and_a_mac_leaves_it_to_the_system() {
    let edge = pos2(SIZE.x - 2.0, 400.0);
    let resize = |c: &ViewportCommand| matches!(c, ViewportCommand::BeginResize(_));
    for (platform, frameless) in [("windows", true), ("gnome", true), ("mac", false)] {
        let mut h = with(&[("platform", platform)], SIZE);
        h.hover_at(edge);
        h.step();
        button(&mut h, edge, true);
        assert_eq!(sends(&mut h, resize), frameless, "{platform}");
        button(&mut h, edge, false);
    }
}

/// The title's span in the bar; elsewhere it may name a page or a tab.
fn title_box(h: &Harness<'static, MareaderApp>, title: &str) -> Option<(f32, f32)> {
    let bar = mareader_ui_egui::theme::TITLE_BAR_H as f64;
    h.query_all_by_label(title).find_map(|n| {
        let b = n.accesskit_node().bounding_box()?;
        (b.y1 <= bar).then_some((b.x0 as f32, b.x1 as f32))
    })
}

#[test]
fn the_title_centres_truncates_and_hides_for_want_of_room() {
    let title = "A Mareader Sample";
    let wide = with(&[("pdf", "sample:3"), ("pinned", "true")], SIZE);
    let (l, r) = title_box(&wide, title).unwrap_or_else(|| panic!("no title shown"));
    let centre = (l + r) / 2.0;
    assert!((centre - SIZE.x / 2.0).abs() < 2.0, "{l}..{r} off centre");
    let pinned = [("pdf", "sample:3"), ("pinned", "true")];
    let narrow = with(&pinned, egui::vec2(380.0, 600.0));
    let at = title_box(&narrow, title);
    let full = r - l;
    assert!(
        at.is_some_and(|(l, r)| r - l < full - 4.0),
        "{at:?}, full {full}"
    );
    let tiny = with(&pinned, egui::vec2(250.0, 600.0));
    assert_eq!(title_box(&tiny, title), None, "no room, no title");
    let empty = with(&[("pinned", "true")], SIZE);
    assert_eq!(title_box(&empty, title), None, "nothing open, no title");
}

#[test]
fn a_narrow_bar_folds_dictionary_into_the_menu() {
    let pinned = [("pdf", "sample:3"), ("pinned", "true")];
    let mut h = with(&pinned, egui::vec2(380.0, 600.0));
    assert!(h.query_by_label("Dictionary").is_none(), "no room for it");
    h.state_mut().queue(Sent::Ui(UiCommand::ShowViewMenu));
    h.run();
    h.get_by_label("Dictionary").click();
    h.run();
    // In the reader it floats over the page.
    assert!(h.state().settings().dictionary.window.open);
}

fn widths(h: &Harness<'static, MareaderApp>) -> Vec<f32> {
    h.state()
        .pane_rects()
        .iter()
        .map(|(_, r)| r.width())
        .collect()
}

#[test]
fn dragging_a_divider_resizes_panes_within_limits() {
    let mut h = with(&[("panes", "2")], SIZE);
    h.run_steps(4);
    let before = widths(&h);
    assert_eq!(before.len(), 2, "{before:?}");
    let rects = h.state().pane_rects().to_vec();
    let split = rects
        .iter()
        .map(|(_, r)| r.right())
        .fold(f32::MAX, f32::min);
    let gap = rects.iter().map(|(_, r)| r.left()).fold(f32::MIN, f32::max);
    let start = pos2((split + gap) / 2.0, 400.0);
    h.hover_at(start);
    h.step();
    button(&mut h, start, true);
    for x in [start.x - 20.0, start.x - 150.0, 400.0] {
        h.hover_at(pos2(x, 400.0));
        h.step();
    }
    button(&mut h, pos2(400.0, 400.0), false);
    h.run_steps(3);
    let after = widths(&h);
    assert!(after[0] < before[0] - 100.0, "{before:?} to {after:?}");
    // Past the limit the divider stops at its minimum share.
    let at = pos2(400.0, 400.0);
    h.hover_at(at);
    h.step();
    button(&mut h, at, true);
    for x in [380.0, 200.0, 5.0] {
        h.hover_at(pos2(x, 400.0));
        h.step();
    }
    button(&mut h, pos2(5.0, 400.0), false);
    h.run_steps(3);
    let least = widths(&h)[0];
    let share = SIZE.x * mareader_workspace::MIN_RATIO;
    assert!(least >= share - 20.0, "{least} under {share}");
}

#[test]
fn a_popover_closes_on_escape_and_on_a_press_outside() {
    let open = |h: &Harness<'static, MareaderApp>| {
        let id = egui::Id::new(mareader_ui_egui::titlebar::APPEARANCE_POPOVER);
        egui::Popup::is_id_open(&h.ctx, id)
    };
    let mut h = with(&[("pdf", "sample:3")], SIZE);
    h.state_mut().queue(Sent::Ui(UiCommand::ShowAppearance));
    h.run_steps(4);
    assert!(open(&h), "the popover opens");
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(!open(&h), "Escape closes it");
    h.state_mut().queue(Sent::Ui(UiCommand::ShowAppearance));
    h.run_steps(4);
    assert!(open(&h));
    let away = pos2(300.0, 600.0);
    h.hover_at(away);
    h.step();
    button(&mut h, away, true);
    button(&mut h, away, false);
    h.run_steps(3);
    assert!(!open(&h), "a press outside closes it");
}

/// Steps until a frame asks for no repaint at all, then checks it stays so.
fn settles(h: &mut Harness<'static, MareaderApp>) -> Result<(), String> {
    let idle = |h: &Harness<'static, MareaderApp>| {
        let out = h.output().viewport_output.get(&ViewportId::ROOT);
        out.is_some_and(|v| v.repaint_delay == Duration::MAX)
    };
    for step in 0..900 {
        h.step();
        if idle(h) {
            h.run_steps(1);
            return match idle(h) {
                true => Ok(()),
                false => Err(format!("woke again after settling at {step}")),
            };
        }
    }
    let causes = h.ctx.repaint_causes();
    Err(format!("still drawing after 15 s: {causes:?}"))
}

#[test]
fn an_idle_window_draws_nothing() {
    let away = pos2(600.0, 450.0);
    let states: [&[(&str, &str)]; 4] = [
        &[("pdf", "sample:5")],
        &[("epub", "sample:3"), ("sidebar", "outline")],
        &[("books", "6"), ("shelves", "demo"), ("library", "show")],
        &[("pdf", "sample:5"), ("panes", "2"), ("toast", "Saved")],
    ];
    for options in states {
        let mut h = with(options, SIZE);
        h.hover_at(away);
        let settled = settles(&mut h);
        assert!(settled.is_ok(), "{options:?}: {settled:?}");
    }
}

/// Whether hovering the top, then the bottom, brings each bar up.
fn bars(h: &mut Harness<'static, MareaderApp>) -> (bool, bool) {
    h.hover_at(pos2(600.0, 10.0));
    h.run_steps(30);
    let top = h.query_by_label("Appearance").is_some();
    h.hover_at(pos2(600.0, SIZE.y - 4.0));
    h.run_steps(30);
    let bottom = h.query_by_label("Next page").is_some();
    (top, bottom)
}

fn toast(h: &Harness<'static, MareaderApp>) -> String {
    h.state()
        .toast()
        .map(|t| t.message.clone())
        .unwrap_or_default()
}

#[test]
fn distraction_free_hides_every_bar_until_escape_or_its_key() {
    let mut h = with(&[("pdf", "sample:3"), ("sidebar", "outline")], SIZE);
    assert_eq!(bars(&mut h), (true, true), "both bars rise on hover");
    let key = |h: &mut Harness<'static, MareaderApp>| {
        h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, egui::Key::F);
        h.run_steps(2);
    };
    key(&mut h);
    assert!(!h.state().sidebar().is_open(), "the sidebar goes too");
    assert!(toast(&h).contains("Esc"), "{}", toast(&h));
    assert!(h.query_by_label("Find").is_none(), "Shift+F is not Find");
    assert_eq!(bars(&mut h), (false, false));
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert_eq!(bars(&mut h), (true, true), "Escape brings them back");
    // The View menu turns it on; the same key turns it off.
    h.hover_at(pos2(600.0, 10.0));
    h.run_steps(30);
    h.get_by_label("View & tools").click();
    h.run_steps(2);
    h.get_by_label("Distraction-free").click();
    h.run_steps(2);
    assert_eq!(bars(&mut h), (false, false));
    key(&mut h);
    assert_eq!(bars(&mut h), (true, true));
}

#[test]
fn distraction_free_leaves_the_library_its_bar() {
    let mut h = with(&[("pdf", "sample:3"), ("chromeless", "true")], SIZE);
    assert_eq!(bars(&mut h), (false, false));
    h.state_mut().queue(Sent::Ui(UiCommand::ShowLibrary));
    h.run_steps(4);
    assert!(bars(&mut h).0, "the library has no other way out");
    // Back to reading, the bars hide again.
    h.state_mut().queue(Sent::Ui(UiCommand::ShowLibrary));
    h.run_steps(4);
    assert_eq!(bars(&mut h), (false, false));
}

#[test]
fn an_interface_zoom_saved_before_is_put_back_to_full_size() {
    let mut app = MareaderApp::default();
    app.set_option("workers", "0").ok();
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 600.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(400)
        .build_eframe(|cc| {
            // As egui restores a 110 % left from older builds.
            cc.egui_ctx.set_zoom_factor(1.1);
            app
        });
    h.run_steps(4);
    assert!(
        (h.ctx.zoom_factor() - 1.0).abs() < 1e-6,
        "{}",
        h.ctx.zoom_factor()
    );
}
