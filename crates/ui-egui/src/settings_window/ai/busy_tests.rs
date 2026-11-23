#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_settings::Provider;

use super::*;

fn ids(n: u32) -> Vec<PresetId> {
    (1..=n).map(PresetId).collect()
}

#[test]
fn a_drop_lands_where_the_room_opens_and_arrows_step_once() {
    let mut list = ids(4);
    assert!(reorder(&mut list, 3, 0), "the last to the top");
    assert_eq!(list, [4, 1, 2, 3].map(PresetId));
    assert!(!reorder(&mut list, 1, 1));
    assert!(reorder(&mut list, 0, 9), "past the end is the end");
    assert_eq!(list, [1, 2, 3, 4].map(PresetId));
    assert!(!moved(&mut list, 0, Move::Up) && !moved(&mut list, 3, Move::Down));
    assert!(moved(&mut list, 1, Move::Down));
    assert_eq!(list, [1, 3, 2, 4].map(PresetId));
    assert!(moved(&mut list, 0, Move::Out));
    assert_eq!(list, [3, 2, 4].map(PresetId));
    // Rows at 0, 53, 106: past a row's middle is past the row.
    let land = |y| landing(0.0, 3, 0, y);
    assert_eq!((land(10.0), land(90.0), land(500.0)), (0, 1, 2));
    assert_eq!(landing(0.0, 3, 2, -40.0), 0);
}

#[test]
fn others_slide_aside_for_a_carried_backup() {
    let down = Carry { from: 0, to: 2 };
    assert_eq!([0, 1, 2, 3].map(|i| down.place(i)), [2, 0, 1, 3]);
    let up = Carry { from: 3, to: 1 };
    assert_eq!([0, 1, 2, 3].map(|i| up.place(i)), [0, 2, 3, 1]);
}

struct State {
    ai: AiSettings,
    fonts: bool,
}

fn saved(n: usize) -> AiSettings {
    let mut ai = AiSettings {
        presets: Vec::new(),
        ..AiSettings::default()
    };
    for i in 0..n {
        let id = ai.add_preset(Provider::Ollama, &format!("q{i}"));
        if let Some(p) = ai.preset_mut(id) {
            p.name = format!("m{i}");
        }
    }
    ai
}

fn harness(ai: AiSettings) -> Harness<'static, State> {
    let state = State { ai, fonts: false };
    Harness::builder()
        .with_size(egui::vec2(720.0, 1400.0))
        .build_ui_state(
            |ui, s: &mut State| {
                // Named fonts work from the frame after they're installed.
                if !s.fonts {
                    crate::theme::install_fonts(ui.ctx());
                    let t = crate::theme::Tokens::get(ui.ctx());
                    crate::theme::apply(ui.ctx(), &t);
                    egui_extras::install_image_loaders(ui.ctx());
                    s.fonts = true;
                    ui.ctx().request_repaint();
                } else {
                    show(ui, &mut s.ai);
                }
            },
            state,
        )
}

fn order(h: &Harness<'static, State>) -> Vec<PresetId> {
    let ai = &h.state().ai;
    ai.tasks.backups.ids(&ai.presets)
}

#[test]
fn backups_wait_for_a_second_model_then_show_the_first_three() {
    let mut h = harness(saved(1));
    h.run_steps(8);
    assert!(
        h.query_by_label("Backup models").is_none(),
        "one model: hidden"
    );
    assert!(h.query_by_label("Who goes first").is_some());
    let mut h = harness(saved(4));
    h.run_steps(8);
    assert!(h.query_by_label("Backup 3: m2").is_some());
    assert!(h.query_by_label("Backup 4: m3").is_none(), "three to start");
    h.get_by_role(egui::accesskit::Role::ComboBox).click();
    h.run_steps(60);
    h.get_by_label("m3").click();
    h.run_steps(60);
    assert_eq!(order(&h), ids(4));
    assert!(h.state().ai.tasks.backups.arranged());
}

#[test]
fn the_title_folds_the_list_to_one_line_and_the_line_opens_it() {
    let mut h = harness(saved(3));
    h.run_steps(8);
    h.get_by_label("Backup models").click();
    h.run_steps(60);
    assert!(h.query_by_label("Backup 1: m0").is_none(), "shut");
    h.get_by_label("Show all backups").click();
    h.run_steps(60);
    assert!(h.query_by_label("Backup 1: m0").is_some(), "open again");
    assert!(h.query_by_label("Show all backups").is_none());
    assert_eq!(order(&h), ids(3), "folding leaves the order");
}

#[test]
fn a_backup_says_which_tasks_skip_it() {
    let mut ai = saved(3);
    let (a, b) = (PresetId(1), PresetId(2));
    ai.tasks.models.insert(TaskId::Explain, vec![a]);
    ai.tasks.models.insert(TaskId::Organize, vec![b]);
    let line = |ai: &AiSettings, id| line(ai, ai.preset(id).unwrap());
    // Every task but the organizer follows the AI Dictionary.
    assert_eq!(
        line(&ai, a),
        "Skipped by AI Dictionary and 3 more (their own model)"
    );
    assert_eq!(line(&ai, b), "Skipped by Organize library (its own model)");
    assert_eq!(line(&ai, PresetId(3)), "Ollama · q2");
    let named = ai.add_preset(Provider::Gemini, "");
    assert_eq!(line(&ai, named), "Backs up every task");
}

#[test]
fn arrows_and_remove_arrange_the_list_and_the_link_undoes_it() {
    let mut h = harness(saved(3));
    h.run_steps(8);
    assert!(h.query_by_label("Go back to your first 3 models").is_none());
    h.get_by_label("Move m1 up").click();
    h.run_steps(60);
    assert_eq!(order(&h), [2, 1, 3].map(PresetId));
    h.get_by_label("Remove m0").click();
    h.run_steps(60);
    assert_eq!(order(&h), [2, 3].map(PresetId));
    h.get_by_label("Go back to your first 3 models").click();
    h.run_steps(60);
    assert!(!h.state().ai.tasks.backups.arranged());
    assert_eq!(order(&h), ids(3));
}

#[test]
fn a_backup_dragged_below_another_lands_there() {
    let mut h = harness(saved(3));
    h.run_steps(8);
    let row = |h: &Harness<'static, State>, name: &str| h.get_by_label(name).rect();
    let (top, last) = (row(&h, "Backup 1: m0"), row(&h, "Backup 3: m2"));
    let from = pos2(top.left() + 120.0, top.center().y);
    let to = pos2(from.x, last.bottom() - 4.0);
    h.hover_at(from);
    h.run_steps(8);
    h.drag_at(from);
    h.run_steps(8);
    for step in 1..=6 {
        h.hover_at(from.lerp(to, step as f32 / 6.0));
        h.run_steps(8);
    }
    h.event(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(8);
    assert_eq!(order(&h), [2, 3, 1].map(PresetId));
}

#[test]
fn a_picked_class_is_kept_and_its_usual_one_is_no_pick() {
    let mut h = harness(saved(1));
    h.run_steps(8);
    // Rows follow TaskId::ALL; the AI Dictionary's comes first.
    let first = TaskId::ALL[0];
    let background = h.query_all_by_label("Background").next().unwrap();
    background.click();
    h.run_steps(60);
    assert_eq!(h.state().ai.class(first), Class::Ahead);
    let usual = first.spec().class.label();
    h.query_all_by_label(usual).next().unwrap().click();
    h.run_steps(60);
    assert!(h.state().ai.tasks.priority.is_empty());
}
