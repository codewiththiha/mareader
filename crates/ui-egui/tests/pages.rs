//! How pages sit: gap, margin, default fit, kept zoom and the label.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_engine::command::Command;
use mareader_engine::pane::Fit;
use mareader_settings::StartupFit;
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::ui_command::Sent;

use crate::common::{Dials, reader_200 as reader};

type H = Harness<'static, MareaderApp>;

fn sheets(h: &H) -> Vec<egui::Rect> {
    let pane = h.state().active_pane();
    let sheets = h.state().documents().sheets(pane);
    sheets.iter().map(|s| s.rect).collect()
}

fn zoom(h: &H) -> (f32, Option<Fit>) {
    let pane = h.state().active_pane();
    let z = h.state().documents().zoom_state(pane);
    z.map_or((0.0, None), |z| (z.zoom, z.fit))
}

fn go(h: &mut H, page: usize) {
    let pane = h.state().active_pane();
    h.state_mut()
        .queue(Sent::Engine(Command::GoTo { pane, page }));
    // Past the glide, then past the kept zoom's settle.
    h.run();
    h.run_steps(30);
    h.run();
}

#[test]
fn no_gap_lets_strip_pages_touch() {
    let mut h = reader(&[("pdf", "sample:5")]);
    // Zoomed well out, several pages share the view.
    for _ in 0..5 {
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Minus);
    }
    h.run();
    let s = sheets(&h);
    assert!((s[1].top() - s[0].bottom() - 12.0).abs() < 1.0, "{s:?}");
    h.dial(|s| s.layout.no_gap = true);
    h.run();
    let s = sheets(&h);
    assert!((s[1].top() - s[0].bottom()).abs() < 1.0, "{s:?}");
}

#[test]
fn the_page_margin_is_air_beside_a_fitted_page() {
    let mut h = reader(&[("pdf", "sample:5")]);
    let edge = sheets(&h)[0];
    h.dial(|s| s.layout.page_margin = 32.0);
    h.run();
    let inset = sheets(&h)[0];
    assert!((edge.width() - inset.width() - 64.0).abs() < 1.0);
    assert!((inset.left() - edge.left() - 32.0).abs() < 1.0);
}

#[test]
fn documents_open_at_the_default_fit() {
    let mut h = reader(&[]);
    h.dial(|s| s.layout.startup_fit = StartupFit::Page);
    h.state_mut().set_option("pdf", "sample:5").ok();
    h.run_steps(3);
    h.run();
    let (zoom, fit) = zoom(&h);
    assert_eq!(fit, Some(Fit::Page));
    // A letter page by its height: 700 high less the strip's ends.
    assert!(zoom < 1.0, "{zoom}");
}

#[test]
fn a_fit_follows_pages_of_another_size_and_auto_resize_a_set_zoom() {
    // Every seventh sample page lies on its side, 792 wide.
    let mut h = reader(&[("pdf", "sample:20")]);
    let near = |h: &H, want: f32| (zoom(h).0 - want).abs() < 0.01;
    let (letter, fit) = zoom(&h);
    assert_eq!(fit, Some(Fit::Width));
    // A fit refits with Scale my zoom off too.
    h.dial(|s| s.layout.auto_resize = false);
    go(&mut h, 6);
    let want = letter * 612.0 / 792.0;
    assert!(near(&h, want), "{:?} want {want}", zoom(&h));
    go(&mut h, 7);
    assert!(near(&h, letter));
    // Off, a zoom set by hand holds.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Minus);
    h.run();
    let (set, fit) = zoom(&h);
    assert_eq!(fit, None);
    go(&mut h, 13);
    assert!(near(&h, set), "off, the set zoom holds");
    go(&mut h, 14);
    // On, it keeps its scale: a wide page shows as wide as the last.
    h.dial(|s| s.layout.auto_resize = true);
    go(&mut h, 13);
    let want = set * 612.0 / 792.0;
    assert!(near(&h, want), "{:?} want {want}", zoom(&h));
    go(&mut h, 14);
    assert!(near(&h, set));
}

#[test]
fn a_set_zoom_shrinks_with_the_pane_and_grows_back_only_to_itself() {
    let mut h = reader(&[("pdf", "sample:5")]);
    let resize = |h: &mut H, width: f32| {
        h.set_size(egui::vec2(width, 700.0));
        h.run();
        h.run_steps(30);
        h.run();
        zoom(h).0
    };
    // What fit width does from a wide pane to a narrow one.
    let narrow = resize(&mut h, 600.0);
    let wide = resize(&mut h, 900.0);
    assert!(narrow < wide, "{narrow} < {wide}");
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Minus);
    h.run();
    let (set, fit) = zoom(&h);
    assert_eq!(fit, None);
    // Narrower, it shrinks by the fit's ratio.
    let want = set * narrow / wide;
    let shrunk = resize(&mut h, 600.0);
    assert!((shrunk - want).abs() < 0.01, "{shrunk} want {want}");
    assert_eq!(zoom(&h).1, None, "still set by hand");
    // Back, and wider still, it returns to what was set and no more.
    assert!((resize(&mut h, 900.0) - set).abs() < 0.001);
    assert!((resize(&mut h, 1400.0) - set).abs() < 0.001);
    // Off, a set zoom holds whatever the pane does.
    h.dial(|s| s.layout.auto_resize = false);
    assert!((resize(&mut h, 600.0) - set).abs() < 0.001);
}

#[test]
fn the_floating_label_shows_while_the_bar_hides_and_room_allows() {
    let mut h = reader(&[("pdf", "sample:5"), ("label", "chapter")]);
    let label = "Pane label: Chapter 1: Opening";
    // Fitted edge to edge, there is no room beside the page.
    assert!(h.query_by_label(label).is_none());
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Minus);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Minus);
    h.run();
    let _ = h.get_by_label(label);
    // A floating sidebar slides over the label instead of hiding it.
    h.state_mut().set_option("float", "true").ok();
    h.state_mut().set_option("sidebar", "outline").ok();
    h.run();
    let _ = h.get_by_label(label);
    let _ = h.get_by_label("Chapter 1: Opening");
}

#[test]
fn by_default_the_label_always_shows_the_chapter() {
    let h = reader(&[("pdf", "sample:5")]);
    let _ = h.get_by_label("Pane label: Chapter 1: Opening");
    let l = &h.state().settings().layout;
    assert!(l.floating_label_persist && !l.progress_bar && !l.page_shadow);
}

#[test]
fn always_show_keeps_the_label_under_a_pinned_bar() {
    let mut h = reader(&[
        ("pdf", "sample:5"),
        ("label", "chapter"),
        ("pinned", "true"),
    ]);
    let label = "Pane label: Chapter 1: Opening";
    assert!(h.query_by_label(label).is_none());
    h.dial(|s| s.layout.floating_label_persist = true);
    h.run();
    let _ = h.get_by_label(label);
}

#[test]
fn the_margin_stepper_steps_by_four_and_reads_off_at_zero() {
    let mut h = reader(&[("pdf", "sample:5"), ("settings", "layout")]);
    let _ = h.get_by_label("Off");
    h.get_by_label("More page margin").scroll_to_me();
    h.run();
    h.get_by_label("More page margin").click();
    h.run();
    h.get_by_label("More page margin").click();
    h.run();
    assert_eq!(h.state().settings().layout.page_margin, 8.0);
    let _ = h.get_by_label("8 px");
}

#[test]
fn the_progress_line_follows_the_scroll() {
    let mut h = reader(&[("pdf", "sample:60")]);
    let pane = h.state().active_pane();
    let progress = |h: &H| {
        h.state()
            .documents()
            .reading(pane)
            .map_or(-1.0, |r| r.progress)
    };
    assert!(progress(&h) < 0.05, "{}", progress(&h));
    go(&mut h, 30);
    let mid = progress(&h);
    assert!((0.4..0.6).contains(&mid), "{mid}");
    h.state_mut().queue(Sent::Engine(Command::LastPage));
    h.run();
    assert!(progress(&h) > 0.99, "{}", progress(&h));
}

fn tiles(h: &H) -> usize {
    h.state().documents().meter().tiles
}

#[test]
fn a_deep_zoom_draws_tiles_once_the_view_rests() {
    let mut h = reader(&[("pdf", "sample:3")]);
    assert_eq!(tiles(&h), 0, "a fitted page is one texture");
    // At 500 %, a page is 3960 px high: past egui's 2048 texture side.
    for _ in 0..12 {
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Equals);
    }
    h.run_steps(30);
    h.run();
    let rested = tiles(&h);
    assert!((1..=6).contains(&rested), "{rested} tiles on screen");
    // A pan asks for none until it stops.
    h.hover_at(egui::pos2(450.0, 400.0));
    for _ in 0..10 {
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -150.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        h.step();
        assert!(tiles(&h) <= rested, "a tile arrived mid-pan");
    }
    h.run_steps(30);
    h.run();
    assert!(tiles(&h) > rested, "the new rows came once it rested");
    let bytes = h.state().documents().meter().page_bytes;
    assert!(bytes < mareader_ui_egui::pages::CAP_BYTES, "{bytes}");
}

/// The leftmost and rightmost edges of the pages on screen.
fn span(h: &H) -> (f32, f32) {
    let rects = sheets(h);
    let left = rects.iter().map(|r| r.min.x).fold(f32::MAX, f32::min);
    let right = rects.iter().map(|r| r.max.x).fold(f32::MIN, f32::max);
    (left, right)
}

#[test]
fn a_turn_to_a_spread_of_other_sizes_fits_on_landing() {
    let mut h = reader(&[("pdf", "sample:20")]);
    h.state_mut().queue(Sent::Engine(Command::View(
        mareader_engine::pane::ViewMode::Spread,
    )));
    h.run();
    let pane = h.state().active_pane();
    // Pages 7 and 8: one on its side, one upright. Then two upright.
    for page in [6, 2, 13] {
        h.state_mut()
            .queue(Sent::Engine(Command::GoTo { pane, page }));
        h.run();
        let (left, right) = span(&h);
        assert!(
            left.abs() < 0.5 && (right - 900.0).abs() < 0.5,
            "{page}: {left}..{right}"
        );
    }
}

#[test]
fn a_strip_centres_upright_pages_in_its_widest_column() {
    let h = reader(&[("pdf", "sample:20")]);
    let (z, _) = zoom(&h);
    let pane = h.state().active_pane();
    let offset = h.state().documents().offset(pane).unwrap_or_default();
    // Page 7 lies on its side, so the column is 792 points wide.
    let centred = (792.0 - 612.0) * z / 2.0;
    assert!(
        (offset.x - centred).abs() < 0.5,
        "{offset:?} want {centred}"
    );
    let first = sheets(&h)[0];
    assert!(
        first.min.x.abs() < 0.5,
        "fitted, the page fills the pane: {first:?}"
    );
}
