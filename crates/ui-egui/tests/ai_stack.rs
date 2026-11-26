//! The AI stack's lines for jobs whose own visuals are away: an explained
//! word's glow and card, a skill's card, a page's rewrite and its groups.

use std::time::Duration;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_engine::command::Command;
use mareader_engine::page_ai::{Action as PageAi, How};
use mareader_engine::skills::Action;
use mareader_settings::{PageTask, Skill};
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::activity::stack::{CANCEL, STACK, VIEW};
use mareader_ui_egui::ui_command::Sent;
use mareader_ui_egui::z_order::{self, Tier};

use crate::common::ai_reader;

type H = Harness<'static, MareaderApp>;

/// A reader whose AI Dictionary answers after a moment.
pub(crate) fn slow_reader() -> H {
    ai_reader(&[("pdf", "sample:5"), ("explain", "slow:300")])
}

pub(crate) fn send(h: &mut H, command: Command) {
    h.state_mut().queue(Sent::Engine(command));
    h.run_steps(30);
}

/// Explains `word` on the first page, then turns to page 3, away from it.
fn explain_and_turn_away(h: &mut H, word: &str) {
    let pane = h.state().active_pane();
    let text = word.to_owned();
    send(
        h,
        Command::SelectText {
            pane,
            page: 0,
            text,
        },
    );
    send(h, Command::Explain(pane));
    assert!(h.state().study().card_open(), "its card opens");
    send(h, Command::GoTo { pane, page: 3 });
    assert!(
        !h.state().study().card_open(),
        "scrolled away, the card goes"
    );
    assert!(
        until(h, |h| h.query_by_label(STACK).is_some()),
        "the corner says it"
    );
    h.get_by_label(STACK).hover();
    h.run_steps(20);
}

/// Steps until `done` holds, the explainer's wait passing meanwhile.
fn until(h: &mut H, done: impl Fn(&H) -> bool) -> bool {
    for _ in 0..400 {
        if done(h) {
            return true;
        }
        h.run_steps(1);
        std::thread::sleep(Duration::from_millis(5));
    }
    done(h)
}

/// PROGRESS.md section 5, step 5: done out of view, View scrolls back and
/// opens it.
#[test]
fn an_answer_landed_out_of_view_waits_until_view_opens_its_card() {
    let mut h = slow_reader();
    explain_and_turn_away(&mut h, "quick");
    h.get_by_label_contains("Explaining “quick”");
    let landed = |h: &H| h.query_by_label_contains("Explained “quick”").is_some();
    assert!(until(&mut h, landed), "it says the answer came");
    h.get_by_label(VIEW).click();
    h.run_steps(40);
    assert_eq!(h.state().study().card_word(), Some("quick"));
    h.get_by_label("Synonyms");
    assert!(h.query_by_label(STACK).is_none(), "seen, the line goes");
}

/// PROGRESS.md section 5, step 6: a row that isn't study's stops at once.
#[test]
fn an_explain_out_of_view_cancels_at_once_with_a_toast() {
    let mut h = slow_reader();
    explain_and_turn_away(&mut h, "quick");
    h.get_by_label(CANCEL).click();
    h.run_steps(4);
    let toast = h.state().toast().map(|t| t.message.clone());
    let said = toast.unwrap_or_default();
    assert!(said.starts_with("Stopped explaining"), "{said:?}");
    assert!(until(&mut h, |h| h.query_by_label(STACK).is_none()));
    std::thread::sleep(Duration::from_millis(400));
    h.run_steps(10);
    assert!(h.query_by_label(STACK).is_none(), "its answer never lands");
    let pane = h.state().active_pane();
    assert!(h.state().marks(pane).is_empty(), "nor its mark");
}

/// A skill run on the first page's words while the reader is on page 8.
fn skill_away(answers: &str) -> H {
    let mut h = ai_reader(&[("pdf", "sample:12"), ("skill_answers", answers)]);
    let pane = h.state().active_pane();
    let text = "quick brown".to_owned();
    send(
        &mut h,
        Command::SelectText {
            pane,
            page: 0,
            text,
        },
    );
    send(&mut h, Command::GoTo { pane, page: 8 });
    let skill = Skill::Builtin(PageTask::Simplify);
    send(&mut h, Command::Skill(Action::Run { pane, skill }));
    assert!(h.query_by_label("Ask again").is_none(), "its card is away");
    h
}

#[test]
fn a_skill_out_of_view_runs_in_the_stack_and_cancels_with_a_toast() {
    let mut h = skill_away("silent");
    h.get_by_label(STACK).hover();
    h.run_steps(20);
    h.get_by_label_contains("Simplify · page 1");
    h.get_by_label(CANCEL).click();
    h.run_steps(4);
    assert!(h.state().skills().asks().is_empty(), "its card is closed");
    let toast = h.state().toast().map(|t| t.message.clone());
    assert_eq!(toast.as_deref(), Some("Stopped Simplify"));
    assert!(h.query_by_label(STACK).is_none());
}

/// PROGRESS.md section 5, step 4: a failure shows red about 3 s, with a toast.
#[test]
fn a_skill_failing_out_of_view_shows_red_a_moment_and_toasts() {
    let mut h = skill_away("failing");
    let toast = h.state().toast().map(|t| t.message.clone());
    assert_eq!(toast.as_deref(), Some("Simplify didn’t finish"));
    h.get_by_label(STACK).hover();
    h.run_steps(20);
    let said = h
        .query_all_by_label_contains("Simplify didn’t finish")
        .count();
    assert_eq!(said, 2, "its red line, and the toast above the stack");
    h.hover_at(egui::pos2(300.0, 300.0));
    h.run_steps(240);
    assert!(h.query_by_label(STACK).is_none(), "it goes by itself");
}

#[test]
fn progress_and_toast_share_the_page_counter_bottom_inset() {
    let mut h = skill_away("silent");
    h.state_mut()
        .set_option("toast", "Highlights_saved")
        .unwrap();
    h.run_steps(5);
    h.get_by_label(STACK).hover();
    h.run_steps(25);
    let pane = h.state().pane_rects()[0].1;
    let stack = h.get_by_label(STACK).rect();
    let toast = h.get_by_label("Highlights saved").rect();
    let foot = pane.bottom() - mareader_ui_egui::page_bar::COUNTER_INSET;
    assert!((stack.bottom() - foot).abs() < 1.0, "{stack:?} {pane:?}");
    assert!((toast.bottom() - foot).abs() < 1.0, "{toast:?} {pane:?}");
    assert!((toast.width() - 320.0).abs() < 1.0, "{toast:?}");
    assert!((toast.height() - 44.0).abs() < 1.0, "{toast:?}");
    assert!(!toast.intersects(stack));
}

#[test]
fn an_open_progress_stack_lifts_a_toast_on_a_narrow_pane() {
    let mut h = skill_away("silent");
    h.set_size(egui::vec2(360.0, 700.0));
    h.run_steps(20);
    h.state_mut()
        .set_option("toast", "Highlights_saved")
        .unwrap();
    h.run_steps(4);
    h.get_by_label(STACK).hover();
    h.run_steps(30);
    let stack = h.get_by_label(STACK).rect();
    let toast = h.get_by_label("Highlights saved").rect();
    assert!(!stack.intersects(toast), "{stack:?} {toast:?}");
    assert!(toast.bottom() < stack.top(), "{stack:?} {toast:?}");
}

/// Leaving the stack downward crosses the page bar's edge: there the bar
/// waits for a rest, as the sidebar's edge does by the Library button.
#[test]
fn the_page_bar_waits_for_a_rest_under_the_stack() {
    let mut h = skill_away("silent");
    let pane = h.state().active_pane();
    let rects = h.state().pane_rects().to_vec();
    let rect = rects.iter().find(|(p, _)| *p == pane).map(|(_, r)| *r);
    let rect = rect.expect("the pane is drawn");
    let bar = z_order::layer(Tier::PaneBar, egui::Id::new(("page-bar", pane)));
    let up = |h: &H| h.ctx.memory(|m| m.areas().is_visible(&bar));
    let stack = h.get_by_label(STACK).rect();
    h.hover_at(egui::pos2(stack.center().x, rect.bottom() - 2.0));
    h.run_steps(6);
    assert!(!up(&h), "passing under the stack");
    h.run_steps(40);
    assert!(up(&h), "a rest there brings it");
    h.hover_at(egui::pos2(300.0, 300.0));
    h.run_steps(90);
    assert!(!up(&h), "left, it goes");
    h.hover_at(egui::pos2(rect.right() - 40.0, rect.bottom() - 2.0));
    h.run_steps(6);
    assert!(up(&h), "elsewhere the edge brings it at once");
}

/// A reader whose page rewrites stay mid-answer.
pub(crate) fn held_reader() -> H {
    let held = [
        ("pdf", "sample:12"),
        ("provider", "ollama"),
        ("page_ai_hold", "true"),
    ];
    ai_reader(&held)
}

/// Asks `action`, sees its bubble keep it from the stack, then turns away.
fn rewrite_and_turn_away(h: &mut H, action: PageAi) {
    let pane = h.state().active_pane();
    send(h, Command::PageAi(action));
    send(h, Command::GoTo { pane, page: 11 });
    h.get_by_label(STACK).hover();
    h.run_steps(20);
}

/// Cancels the one line, and returns the toast that says so.
fn cancel(h: &mut H) -> Option<String> {
    h.get_by_label(CANCEL).click();
    h.run_steps(3);
    h.state().toast().map(|t| t.message.clone())
}

/// Row 5: a page rewrite away from view runs in the stack, and stops.
#[test]
fn a_page_rewrite_out_of_view_runs_in_the_stack_and_cancels() {
    let mut h = held_reader();
    let pane = h.state().active_pane();
    let how = How::default();
    let task = PageTask::Simplify;
    rewrite_and_turn_away(
        &mut h,
        PageAi::Run {
            pane,
            page: 0,
            task,
            how,
        },
    );
    h.get_by_label_contains("Simplifying page 1");
    let toast = cancel(&mut h);
    assert_eq!(toast.as_deref(), Some("Stopped simplifying page 1"));
    assert!(
        h.state().page_ai().ledger.runs.is_empty(),
        "the run stopped"
    );
}

/// Row 5: picked pages show their groups' progress, and stop together.
#[test]
fn picked_pages_out_of_view_count_their_groups_and_cancel() {
    let mut h = held_reader();
    let pane = h.state().active_pane();
    let pages = (0..4).collect();
    let task = PageTask::Simplify;
    rewrite_and_turn_away(
        &mut h,
        PageAi::RunPages {
            pane,
            pages,
            task,
            tag: None,
        },
    );
    h.get_by_label_contains("Simplifying pages 1–4 · 0 of");
    let toast = cancel(&mut h);
    assert_eq!(toast.as_deref(), Some("Stopped simplifying pages 1–4"));
    assert!(!h.state().page_ai().ledger.grouping(pane));
}

/// A continuous run's line keeps out while its page's bubble spins in view.
#[test]
fn a_continuous_run_leaves_the_stack_while_its_bubble_spins_in_view() {
    let mut h = held_reader();
    let pane = h.state().active_pane();
    let how = How {
        continuous: true,
        remember: false,
    };
    let task = PageTask::Simplify;
    let run = PageAi::Run {
        pane,
        page: 0,
        task,
        how,
    };
    send(&mut h, Command::PageAi(run));
    assert!(h.query_by_label(STACK).is_none(), "its bubble says it");
    send(&mut h, Command::GoTo { pane, page: 11 });
    h.get_by_label(STACK).hover();
    h.run_steps(20);
    h.get_by_label_contains("Simplifying");
}
