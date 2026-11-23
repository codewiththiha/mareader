//! The read-aloud player among panes: where it keeps once its book closes.

use egui_kittest::Harness;
use mareader_engine::command::Command;
use mareader_engine::read_aloud::ReadFrom;
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::ui_command::Sent;

/// A sample PDF in a wide window, read by the engine that says nothing.
fn reader_wide(pages: usize) -> Harness<'static, MareaderApp> {
    let mut app = MareaderApp::default();
    app.set_option("workers", "0").ok();
    app.set_option("pdf", &format!("sample:{pages}")).ok();
    app.set_option("set", "read_aloud.engine:\"silent\"").ok();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(400)
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    h
}

/// Starts reading in the focused pane and waits for the player.
fn start(h: &mut Harness<'static, MareaderApp>) {
    let pane = h.state().active_pane();
    let from = ReadFrom::default();
    h.state_mut()
        .queue(Sent::Engine(Command::ReadAloud { pane, from }));
    let bar = egui::Id::new("read-aloud-bar");
    for _ in 0..200 {
        h.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(5));
        if h.ctx.memory(|m| m.area_rect(bar)).is_some() {
            break;
        }
    }
}

#[test]
fn with_its_book_closed_the_player_centres_on_the_pane_in_front() {
    let mut h = reader_wide(12);
    start(&mut h);
    let read = h.state().active_pane();
    let split = Command::Split(mareader_workspace::SplitAxis::Horizontal);
    h.state_mut().queue(Sent::Engine(split));
    h.run();
    h.state_mut().set_option("focus", "2").ok();
    h.state_mut().set_option("pdf", "sample:3").ok();
    h.run();
    h.state_mut().queue(Sent::Engine(Command::ClosePane(read)));
    h.run_steps(10);
    // The sidebar docks, so the pane left narrows off the window's middle.
    h.state_mut().set_option("sidebar", "outline").ok();
    h.run_steps(120);
    let pane = h
        .state()
        .pane_rects()
        .first()
        .map(|(_, r)| *r)
        .expect("a pane");
    let window = h.ctx.content_rect();
    assert!(
        (pane.center().x - window.center().x).abs() > 20.0,
        "{pane:?}"
    );
    let id = egui::Id::new("read-aloud-bar");
    let bar = h.ctx.memory(|m| m.area_rect(id)).expect("player");
    let off = (bar.center().x - pane.center().x).abs();
    assert!(off < 2.0, "player {bar:?} off its pane {pane:?} by {off}");
}
