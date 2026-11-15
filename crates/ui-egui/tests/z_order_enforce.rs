//! Pure `egui::Context` tests for `z_order::enforce` in shared buckets.

use egui::{Context, Id, LayerId, Pos2, RawInput, Rect, Sense, pos2, vec2};
use mareader_ui_egui::z_order::{Tier, area, enforce, is_above, layer};

fn run_pass(ctx: &Context, input: RawInput, mut draw: impl FnMut(&Context)) {
    let mut out = ctx.run_ui(input, |ui| {
        let ctx = ui.ctx().clone();
        draw(&ctx);
        enforce(&ctx);
    });
    out.textures_delta.clear();
}

fn box_area(ctx: &Context, tier: Tier, id: &str, rect: Rect) -> LayerId {
    area(ctx, tier, Id::new(id))
        .fixed_pos(rect.min)
        .constrain(false)
        .show(ctx, |ui| {
            ui.allocate_exact_size(rect.size(), Sense::click());
        })
        .response
        .layer_id
}

fn click_input(pos: Pos2, pressed: bool) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))),
        events: vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            },
        ],
        ..RawInput::default()
    }
}

fn plain_input() -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))),
        ..RawInput::default()
    }
}

#[test]
fn clicking_lower_tier_never_raises_it_above_higher_tier_in_same_bucket() {
    let ctx = Context::default();
    let tool_rect = Rect::from_min_size(pos2(100.0, 100.0), vec2(300.0, 200.0));
    let pop_rect = Rect::from_min_size(pos2(200.0, 150.0), vec2(150.0, 120.0));
    let tool_layer = layer(Tier::FloatingTool, Id::new("tool"));
    let pop_layer = layer(Tier::Popover, Id::new("pop"));

    for _ in 0..3 {
        run_pass(&ctx, plain_input(), |ctx| {
            box_area(ctx, Tier::FloatingTool, "tool", tool_rect);
            box_area(ctx, Tier::Popover, "pop", pop_rect);
        });
    }
    assert!(is_above(&ctx, pop_layer, tool_layer));

    // Click the exposed part of the floating tool so Area::begin asks for top.
    run_pass(&ctx, click_input(pos2(120.0, 120.0), true), |ctx| {
        box_area(ctx, Tier::FloatingTool, "tool", tool_rect);
        box_area(ctx, Tier::Popover, "pop", pop_rect);
    });
    run_pass(&ctx, click_input(pos2(120.0, 120.0), false), |ctx| {
        box_area(ctx, Tier::FloatingTool, "tool", tool_rect);
        box_area(ctx, Tier::Popover, "pop", pop_rect);
    });

    assert!(is_above(&ctx, pop_layer, tool_layer));
    assert_eq!(ctx.layer_id_at(pos2(220.0, 180.0)), Some(pop_layer));
}

#[test]
fn repairs_inverted_three_tier_stack_in_same_bucket() {
    let ctx = Context::default();
    let rect = Rect::from_min_size(pos2(100.0, 100.0), vec2(200.0, 150.0));
    let tool = layer(Tier::FloatingTool, Id::new("tool"));
    let pop = layer(Tier::Popover, Id::new("pop"));
    let modal = layer(Tier::Modal, Id::new("modal"));

    // Introduce Modal first, then Popover, then FloatingTool (fully inverted).
    for _ in 0..2 {
        run_pass(&ctx, plain_input(), |ctx| {
            box_area(ctx, Tier::Modal, "modal", rect);
        });
    }
    for _ in 0..2 {
        run_pass(&ctx, plain_input(), |ctx| {
            box_area(ctx, Tier::Modal, "modal", rect);
            box_area(ctx, Tier::Popover, "pop", rect);
        });
    }
    for _ in 0..5 {
        run_pass(&ctx, plain_input(), |ctx| {
            box_area(ctx, Tier::Modal, "modal", rect);
            box_area(ctx, Tier::Popover, "pop", rect);
            box_area(ctx, Tier::FloatingTool, "tool", rect);
        });
    }

    assert!(is_above(&ctx, pop, tool));
    assert!(is_above(&ctx, modal, pop));
    assert_eq!(ctx.layer_id_at(rect.center()), Some(modal));
}

#[test]
fn sibling_floating_tools_still_raise_over_each_other_below_modal() {
    let ctx = Context::default();
    let a_rect = Rect::from_min_size(pos2(40.0, 40.0), vec2(200.0, 160.0));
    let b_rect = Rect::from_min_size(pos2(120.0, 80.0), vec2(200.0, 160.0));
    let m_rect = Rect::from_min_size(pos2(400.0, 40.0), vec2(200.0, 160.0));
    let a_layer = layer(Tier::FloatingTool, Id::new("tool-a"));
    let b_layer = layer(Tier::FloatingTool, Id::new("tool-b"));
    let m_layer = layer(Tier::Modal, Id::new("modal"));

    for _ in 0..4 {
        run_pass(&ctx, plain_input(), |ctx| {
            box_area(ctx, Tier::FloatingTool, "tool-a", a_rect);
            box_area(ctx, Tier::FloatingTool, "tool-b", b_rect);
            box_area(ctx, Tier::Modal, "modal", m_rect);
        });
    }
    assert!(is_above(&ctx, b_layer, a_layer));
    assert!(is_above(&ctx, m_layer, b_layer));

    // Clicking exposed tool-a raises it over tool-b, still below modal.
    run_pass(&ctx, click_input(pos2(60.0, 60.0), true), |ctx| {
        box_area(ctx, Tier::FloatingTool, "tool-a", a_rect);
        box_area(ctx, Tier::FloatingTool, "tool-b", b_rect);
        box_area(ctx, Tier::Modal, "modal", m_rect);
    });
    assert!(is_above(&ctx, a_layer, b_layer));
    assert!(is_above(&ctx, m_layer, a_layer));
}

#[test]
fn closing_higher_tier_layer_does_not_leave_ghost_blocker() {
    let ctx = Context::default();
    let rect = Rect::from_min_size(pos2(100.0, 100.0), vec2(200.0, 150.0));
    let tool = layer(Tier::FloatingTool, Id::new("tool"));
    let modal = layer(Tier::Modal, Id::new("modal"));

    for _ in 0..3 {
        run_pass(&ctx, plain_input(), |ctx| {
            box_area(ctx, Tier::FloatingTool, "tool", rect);
            box_area(ctx, Tier::Modal, "modal", rect);
        });
    }
    assert_eq!(ctx.layer_id_at(rect.center()), Some(modal));

    // Stop drawing modal: after one settle pass, hits land on tool again.
    for _ in 0..2 {
        run_pass(&ctx, plain_input(), |ctx| {
            box_area(ctx, Tier::FloatingTool, "tool", rect);
        });
    }
    assert_eq!(ctx.layer_id_at(rect.center()), Some(tool));
}

#[test]
fn middle_bucket_chrome_stays_ordered_after_pane_bar_click() {
    let ctx = Context::default();
    let mark_rect = Rect::from_min_size(pos2(50.0, 50.0), vec2(300.0, 80.0));
    let bar_rect = Rect::from_min_size(pos2(50.0, 50.0), vec2(300.0, 80.0));
    let rail_rect = Rect::from_min_size(pos2(50.0, 50.0), vec2(150.0, 200.0));
    let mark = layer(Tier::PaneMark, Id::new("mark"));
    let bar = layer(Tier::PaneBar, Id::new("bar"));
    let rail = layer(Tier::WindowChrome, Id::new("rail"));

    for _ in 0..4 {
        run_pass(&ctx, plain_input(), |ctx| {
            area(ctx, Tier::PaneMark, Id::new("mark"))
                .interactable(false)
                .fixed_pos(mark_rect.min)
                .show(ctx, |ui| {
                    ui.allocate_exact_size(mark_rect.size(), Sense::hover());
                });
            box_area(ctx, Tier::PaneBar, "bar", bar_rect);
            box_area(ctx, Tier::WindowChrome, "rail", rail_rect);
        });
    }
    assert!(is_above(&ctx, bar, mark));
    assert!(is_above(&ctx, rail, bar));

    // Clicking bar outside rail cannot push bar above rail.
    run_pass(&ctx, click_input(pos2(250.0, 80.0), true), |ctx| {
        area(ctx, Tier::PaneMark, Id::new("mark"))
            .interactable(false)
            .fixed_pos(mark_rect.min)
            .show(ctx, |ui| {
                ui.allocate_exact_size(mark_rect.size(), Sense::hover());
            });
        box_area(ctx, Tier::PaneBar, "bar", bar_rect);
        box_area(ctx, Tier::WindowChrome, "rail", rail_rect);
    });
    assert!(is_above(&ctx, rail, bar));
    assert_eq!(ctx.layer_id_at(pos2(100.0, 80.0)), Some(rail));
}

#[test]
fn modal_popup_stays_above_modal_while_pre_modal_popup_stays_below() {
    let ctx = Context::default();
    let rect = Rect::from_min_size(pos2(100.0, 100.0), vec2(260.0, 200.0));
    let pre_id = Id::new("pre-modal-popup");
    let in_id = Id::new("in-modal-popup");
    let modal_layer = layer(Tier::Modal, Id::new("modal"));
    let pre_layer = layer(Tier::Popover, pre_id);
    let in_layer = layer(Tier::ModalPopover, in_id);

    // Open a popup before any modal exists, then bring up the modal.
    egui::Popup::open_id(&ctx, pre_id);
    for _ in 0..2 {
        run_pass(&ctx, plain_input(), |ctx| {
            egui::Popup::new(
                pre_id,
                ctx.clone(),
                egui::PopupAnchor::Position(pos2(110.0, 110.0)),
                LayerId::background(),
            )
            .open_memory(None)
            .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
            .show(|ui| {
                ui.label("before modal");
            });
        });
    }
    for _ in 0..4 {
        run_pass(&ctx, plain_input(), |ctx| {
            egui::Popup::new(
                pre_id,
                ctx.clone(),
                egui::PopupAnchor::Position(pos2(110.0, 110.0)),
                LayerId::background(),
            )
            .open_memory(None)
            .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
            .show(|ui| {
                ui.label("before modal");
            });
            box_area(ctx, Tier::Modal, "modal", rect);
        });
    }
    assert!(is_above(&ctx, modal_layer, pre_layer));

    // A popup opened during a modal stays above it even on modal clicks.
    egui::Popup::open_id(&ctx, in_id);
    for _ in 0..4 {
        run_pass(&ctx, plain_input(), |ctx| {
            box_area(ctx, Tier::Modal, "modal", rect);
            egui::Popup::new(
                in_id,
                ctx.clone(),
                egui::PopupAnchor::Position(pos2(140.0, 140.0)),
                modal_layer,
            )
            .open_memory(None)
            .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
            .show(|ui| {
                ui.label("inside modal");
            });
        });
    }
    run_pass(&ctx, click_input(pos2(115.0, 115.0), true), |ctx| {
        box_area(ctx, Tier::Modal, "modal", rect);
        egui::Popup::new(
            in_id,
            ctx.clone(),
            egui::PopupAnchor::Position(pos2(140.0, 140.0)),
            modal_layer,
        )
        .open_memory(None)
        .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
        .show(|ui| {
            ui.label("inside modal");
        });
    });
    assert!(is_above(&ctx, in_layer, modal_layer));
}
