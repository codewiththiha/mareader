//! Headless z-order tests across split panes, chrome, tools and modals.

use egui::{Event, Id, Modifiers, PointerButton, Pos2, Vec2, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_engine::command::Command;
use mareader_engine::read_aloud::ReadFrom;
use mareader_engine::settings::Tab;
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::titlebar::APPEARANCE_POPOVER;
use mareader_ui_egui::ui_command::{Sent, UiCommand};
use mareader_ui_egui::z_order::{self, Tier};
use mareader_workspace::SplitAxis;

use crate::common::click_at;

type H = Harness<'static, MareaderApp>;

fn reader(options: &[(&str, &str)]) -> H {
    let mut app = MareaderApp::default();
    for (k, v) in [
        ("workers", "0"),
        ("animations", "false"),
        ("pdf", "sample:5"),
    ]
    .into_iter()
    .chain(options.iter().copied())
    {
        app.set_option(k, v).expect("a valid headless option");
    }
    let mut h = Harness::builder()
        .with_size(vec2(1000.0, 800.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(300)
        .build_eframe(|_| app);
    h.run_steps(4);
    h.run();
    h
}

fn drag(h: &mut H, from: Pos2, by: Vec2) {
    let btn = |pos, pressed| Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    h.hover_at(from);
    h.run_steps(2);
    h.event(btn(from, true));
    h.run_steps(2);
    for i in 1..=6 {
        h.hover_at(from + by * (i as f32 / 6.0));
        h.run_steps(1);
    }
    h.event(btn(from + by, false));
    h.run_steps(4);
}

#[test]
fn split_pane_page_bar_stays_behind_floating_dictionary_window() {
    let mut h = reader(&[]);
    h.state_mut()
        .queue(Sent::Engine(Command::Split(SplitAxis::Vertical)));
    h.run_steps(4);

    let panes = h.state().pane_rects().to_vec();
    assert_eq!(panes.len(), 2, "vertical split creates two stacked panes");
    let (top_id, top_rect) = if panes[0].1.top() < panes[1].1.top() {
        panes[0]
    } else {
        panes[1]
    };

    h.state_mut()
        .set_option("dict_window", "show")
        .expect("dict window opens");
    h.run_steps(4);

    // Move the dictionary window so it spans the split seam of the top pane.
    let dict = h.state().dict().window_rect().expect("drawn dict window");
    let target_x = top_rect.center().x - dict.width() * 0.5;
    let target_y = top_rect.bottom() - dict.height() * 0.5;
    drag(
        &mut h,
        dict.left_top() + vec2(60.0, 18.0),
        vec2(target_x - dict.left(), target_y - dict.top()),
    );
    let dict = h.state().dict().window_rect().expect("moved dict window");
    assert!(
        dict.top() < top_rect.bottom() && dict.bottom() > top_rect.bottom(),
        "dict spans the top pane's bottom seam: dict={dict:?}, top={top_rect:?}"
    );

    let dict_layer = z_order::layer(Tier::FloatingTool, Id::new("dict-window"));
    let bar_id = Id::new(("page-bar", top_id));
    let bar_layer = z_order::layer(Tier::PaneBar, bar_id);

    // Hovering inside dict-window over the seam must not reveal the page bar.
    h.hover_at(pos2(dict.center().x, top_rect.bottom() - 4.0));
    h.run_steps(6);
    assert!(
        !h.ctx.memory(|m| m.areas().is_visible(&bar_layer)),
        "hovering through dict-window must not summon the page bar"
    );

    // Hovering the exposed edge summons page-bar behind dict-window.
    let exposed = pos2(top_rect.left() + 40.0, top_rect.bottom() - 4.0);
    assert!(!dict.contains(exposed));
    h.hover_at(exposed);
    h.run_steps(6);

    assert!(
        h.ctx.memory(|m| m.areas().is_visible(&bar_layer)),
        "page bar reveals on exposed pane edge"
    );
    assert!(
        z_order::is_above(&h.ctx, dict_layer, bar_layer),
        "dict-window must stack above any split pane's page bar"
    );

    let overlap = pos2(dict.center().x, top_rect.bottom() - 12.0);
    assert_eq!(
        h.ctx.layer_id_at(overlap),
        Some(dict_layer),
        "hits inside the overlap land on dict-window, not page-bar"
    );

    // Clicking the exposed page bar cannot pull it above dict-window.
    click_at(&mut h, exposed);
    h.run_steps(4);
    assert!(z_order::is_above(&h.ctx, dict_layer, bar_layer));
    assert_eq!(h.ctx.layer_id_at(overlap), Some(dict_layer));
}

#[test]
fn floating_sidebar_covers_page_bar_and_stays_under_dictionary_and_modals() {
    let mut h = reader(&[("set", "chrome.sidebar_overlay:true")]);
    let (pane_id, pane_rect) = h.state().pane_rects()[0];

    h.state_mut().queue(Sent::Ui(UiCommand::OpenSidebar(None)));
    h.state_mut()
        .set_option("dict_window", "show")
        .expect("dict window opens");
    h.run_steps(6);

    // Reveal the page bar from the right half, outside the floating sidebar.
    h.hover_at(pos2(pane_rect.right() - 40.0, pane_rect.bottom() - 4.0));
    h.run_steps(6);

    let bar = z_order::layer(Tier::PaneBar, Id::new(("page-bar", pane_id)));
    let sidebar = z_order::layer(Tier::WindowChrome, Id::new("sidebar-float"));
    let dict = z_order::layer(Tier::FloatingTool, Id::new("dict-window"));

    assert!(z_order::is_above(&h.ctx, sidebar, bar));
    assert!(z_order::is_above(&h.ctx, dict, sidebar));

    h.state_mut().queue(Sent::Ui(UiCommand::ShowSettings));
    h.run_steps(6);

    let settings = z_order::layer(Tier::Modal, Id::new("settings-window"));
    assert!(z_order::is_above(&h.ctx, settings, dict));
    assert!(z_order::is_above(&h.ctx, settings, sidebar));
}

#[test]
fn titlebar_popovers_and_settings_modal_with_combobox_stack_in_tier_order() {
    let mut h = reader(&[("pinned", "true")]);
    h.state_mut()
        .set_option("dict_window", "show")
        .expect("dict window opens");
    h.state_mut().queue(Sent::Ui(UiCommand::ShowFind));
    h.run_steps(6);

    let titlebar = z_order::layer(Tier::WindowChrome, Id::new("titlebar"));
    let dict = z_order::layer(Tier::FloatingTool, Id::new("dict-window"));
    let find = z_order::layer(Tier::FloatingTool, Id::new("find-bar"));

    assert!(z_order::is_above(&h.ctx, dict, titlebar));
    assert!(z_order::is_above(&h.ctx, find, titlebar));

    h.state_mut().queue(Sent::Ui(UiCommand::ShowAppearance));
    h.run_steps(6);
    let appearance = z_order::layer(Tier::Popover, Id::new(APPEARANCE_POPOVER));
    assert!(z_order::is_above(&h.ctx, appearance, titlebar));
    assert!(z_order::is_above(&h.ctx, appearance, dict));

    // Open Settings on the AI tab so a live ComboBox is ready to open.
    h.state_mut()
        .queue(Sent::Engine(Command::OpenSettings(Tab::Ai)));
    h.run_steps(6);

    let settings = z_order::layer(Tier::Modal, Id::new("settings-window"));
    assert!(z_order::is_above(&h.ctx, settings, dict));
    assert!(z_order::is_above(&h.ctx, settings, find));
    assert!(z_order::is_above(&h.ctx, settings, titlebar));

    if let Some(combo) = h.query_by_label("Paragraphs") {
        combo.click();
        h.run_steps(6);
        let layers: Vec<_> = h.ctx.memory(|m| m.layer_ids().collect());
        let popup = layers
            .into_iter()
            .find(|l| {
                l.order == Tier::ModalPopover.order() && egui::Popup::is_id_open(&h.ctx, l.id)
            })
            .expect("ComboBox popup opens inside Settings");
        assert!(
            z_order::is_above(&h.ctx, popup, settings),
            "ComboBox popup inside Settings must stay above the Settings modal"
        );
    }
}

#[test]
fn toasts_stay_above_floating_tools_and_toast_island_tucks_behind_player() {
    let mut h = reader(&[("set", "read_aloud.engine:\"silent\"")]);
    h.state_mut()
        .set_option("dict_window", "show")
        .expect("dict window opens");
    h.state_mut()
        .set_option("toast", "Saved")
        .expect("toast shows");
    h.run_steps(6);

    let dict = z_order::layer(Tier::FloatingTool, Id::new("dict-window"));
    let toasts = z_order::layer(Tier::Notification, Id::new("toasts"));
    assert!(z_order::is_above(&h.ctx, toasts, dict));

    h.state_mut()
        .set_option("dict_window", "hide")
        .expect("dict hides");
    h.state_mut().queue(Sent::Ui(UiCommand::ClearToasts));
    let pane = h.state().active_pane();
    h.state_mut().queue(Sent::Engine(Command::ReadAloud {
        pane,
        from: ReadFrom::default(),
    }));
    for _ in 0..300 {
        h.run_steps(1);
        if h.query_by_label("Pause").is_some() {
            break;
        }
    }
    h.get_by_label("Pause").click();
    h.run_steps(10);
    h.state_mut()
        .set_option("toast", "player:Speed_saved")
        .expect("player toast shows");
    h.run_steps(10);
    assert!(
        h.ctx
            .memory(|m| m.area_rect(Id::new("toast-island")))
            .is_some(),
        "compact player toast-island is drawn"
    );

    let (pane_id, pane_rect) = h.state().pane_rects()[0];
    h.hover_at(pos2(pane_rect.left() + 40.0, pane_rect.bottom() - 4.0));
    h.run_steps(6);

    let page_bar = z_order::layer(Tier::PaneBar, Id::new(("page-bar", pane_id)));
    let island = z_order::layer(Tier::AttachedPill, Id::new("toast-island"));
    let player = z_order::layer(Tier::FloatingTool, Id::new("read-aloud-bar"));

    assert!(z_order::is_above(&h.ctx, island, page_bar));
    assert!(z_order::is_above(&h.ctx, player, island));
}

#[test]
fn parked_dictionary_reveal_stays_below_open_modal() {
    let mut h = reader(&[]);
    h.state_mut()
        .set_option("dict_window", "show")
        .expect("dict window opens");
    h.run_steps(6);

    let at = h.state().dict().window_rect().expect("drawn dict window");
    let by = vec2(-at.left() - at.width() * 0.5 - 8.0, 0.0);
    drag(&mut h, at.left_top() + vec2(60.0, 18.0), by);
    h.run_steps(12);

    let exposed = h
        .state()
        .dict()
        .window_cover()
        .expect("parked dictionary edge");
    assert!(
        h.state().dict().window_rect().expect("parked rect").left() < 0.0,
        "dictionary is parked at the left edge"
    );

    h.state_mut().queue(Sent::Ui(UiCommand::ShowSettings));
    h.run_steps(6);

    let reveal = z_order::layer(Tier::FloatingTool, Id::new("dict-window-edge-reveal"));
    let settings = z_order::layer(Tier::Modal, Id::new("settings-window"));

    assert!(
        z_order::is_above(&h.ctx, settings, reveal),
        "Settings modal must stack above parked dictionary reveal strip"
    );
    assert_eq!(
        h.ctx.layer_id_at(exposed.center()),
        Some(settings),
        "clicks on the left edge hit the modal backdrop, not the parked strip"
    );
}
