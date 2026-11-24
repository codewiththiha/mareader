//! Every kind of AI job proves its place in the stack. At work,
//! `Session::activities` lists it, and while its own visual is in view its
//! line keeps out of the stack. A new kind can't build until [`scene`]
//! gives it one, so its test always exists.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_engine::activity::Kind;
use mareader_engine::command::Command;
use mareader_engine::page_ai::{Action as PageAi, How};
use mareader_engine::skills::Action;
use mareader_settings::{PageTask, Skill};
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::activity::seen;
use mareader_ui_egui::activity::stack::STACK;

use crate::ai_stack::{held_reader, send, slow_reader};
use crate::common::ai_reader;

type H = Harness<'static, MareaderApp>;
type Shelves = (tempfile::TempDir, tempfile::TempDir);
/// The app with a job at work, and the folders it must outlive.
type Scene = (H, Option<Shelves>);

/// The scene where `kind`'s job is at work with its own visual in view.
fn scene(kind: Kind) -> fn() -> Scene {
    match kind {
        Kind::Cards => cards,
        // The library page says what it organizes.
        Kind::Organize => organize,
        Kind::Explain => explain,
        Kind::Skill => skill,
        Kind::Page => page,
        Kind::Pages => pages,
        Kind::Follow => follow,
        Kind::Judge => judge,
    }
}

#[test]
fn every_kind_of_job_is_listed_and_its_visual_keeps_it_out_of_the_stack() {
    for &kind in Kind::ALL {
        let (h, _shelves) = scene(kind)();
        let now = h.ctx.input(|i| i.time);
        let jobs = h.state().session().activities(now);
        let Some(key) = jobs.iter().map(|j| j.key).find(|k| k.kind() == kind) else {
            panic!("{kind:?} is at work, but Session::activities never lists it")
        };
        assert!(seen::shown(&h.ctx, key), "{key:?} shows but never says so");
        let stacked = h.query_by_label(STACK).is_some();
        assert!(!stacked, "{kind:?} in view: the stack stays clear");
    }
}

/// A deck being written, its waiting card at the gate in view.
fn cards() -> Scene {
    use crate::study_cards::{studying_at, wait, writing};
    let (mut h, pane, shelves) = studying_at("silent", 1, Some(3));
    assert!(wait(&mut h, |h| writing(h, pane)), "the AI is asked");
    h.run_steps(4);
    (h, Some(shelves))
}

fn organize() -> Scene {
    let options = [("copies", "5"), ("organize", "running")];
    (crate::organize::page(&options), None)
}

/// A word explained on the page read, its card open.
fn explain() -> Scene {
    let mut h = slow_reader();
    let pane = h.state().active_pane();
    let text = "quick".to_owned();
    send(
        &mut h,
        Command::SelectText {
            pane,
            page: 0,
            text,
        },
    );
    send(&mut h, Command::Explain(pane));
    (h, None)
}

/// A skill asked of words on the page read, its card beside them.
fn skill() -> Scene {
    let mut h = ai_reader(&[("pdf", "sample:12"), ("skill_answers", "silent")]);
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
    let skill = Skill::Builtin(PageTask::Simplify);
    send(&mut h, Command::Skill(Action::Run { pane, skill }));
    (h, None)
}

/// The page read being rewritten, its bubble on it.
fn page() -> Scene {
    rewrite(How::default())
}

fn rewrite(how: How) -> Scene {
    let mut h = held_reader();
    let pane = h.state().active_pane();
    let task = PageTask::Simplify;
    let run = PageAi::Run {
        pane,
        page: 0,
        task,
        how,
    };
    send(&mut h, Command::PageAi(run));
    (h, None)
}

/// Picked pages asked in groups, the first page's bubble in view.
fn pages() -> Scene {
    let mut h = held_reader();
    let pane = h.state().active_pane();
    let pages = (0..4).collect();
    let task = PageTask::Simplify;
    let run = PageAi::RunPages {
        pane,
        pages,
        task,
        tag: None,
    };
    send(&mut h, Command::PageAi(run));
    (h, None)
}

/// A continuous run writing the page read, its bubble spinning on it.
fn follow() -> Scene {
    rewrite(How {
        continuous: true,
        remember: false,
    })
}

/// The judge checking an answer, its recall open on the word.
fn judge() -> Scene {
    let (mut h, _pane, shelves) = crate::study_recall::recalling("silent");
    h.state_mut().set_option("recall", "quick:right").unwrap();
    h.run_steps(10);
    (h, Some(shelves))
}
