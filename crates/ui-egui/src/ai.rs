//! The AI service behind Explain and the AI settings; desktop only.

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
use std::sync::Arc;

use mareader_engine::page_ai::extract::Source;
use mareader_engine::page_ai::state::Handle;
#[cfg(not(target_arch = "wasm32"))]
use mareader_engine::page_ai::state::{News, Writing};
use mareader_engine::page_ai::view::Models;
use mareader_library::AiTask;
use mareader_settings::{AiSettings, ModelPreset};
#[cfg(not(target_arch = "wasm32"))]
use mareader_settings::{SummaryStyle, language_name};
use mareader_study::Explainer;

#[cfg(not(target_arch = "wasm32"))]
use crate::library::{Ask, Proposal, Spot};
use crate::library::{Heard, Paused};
#[cfg(not(target_arch = "wasm32"))]
use mareader_engine::toast::Toast;

#[cfg(not(target_arch = "wasm32"))]
pub use mareader_assist::{Board, Job};

/// The service once attached, and the settings its answerer follows.
#[derive(Default)]
pub struct Ai {
    #[cfg(not(target_arch = "wasm32"))]
    assist: Option<mareader_assist::Assist>,
    made_for: Option<AiSettings>,
    /// The library run under way; dropping it stops the run.
    #[cfg(not(target_arch = "wasm32"))]
    organizing: Option<mareader_assist::Organizing>,
}

impl Ai {
    /// Starts the service, keys under `root`, maybe an unbundled `helper`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn attach(&mut self, ctx: &egui::Context, root: PathBuf, helper: Option<PathBuf>) -> bool {
        let ctx = ctx.clone();
        let waker = Arc::new(move || ctx.request_repaint());
        let started = mareader_assist::Assist::new(root, waker);
        match started.map(|a| helper.into_iter().fold(a, |a, h| a.with_helper(h))) {
            Ok(assist) => {
                self.assist = Some(assist);
                self.made_for = None;
                true
            }
            Err(_) => false,
        }
    }

    /// Takes in finished jobs; a new answerer if `ai` changed since.
    pub fn follow(&mut self, ai: &AiSettings) -> Option<Arc<dyn Explainer>> {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(assist) = &mut self.assist {
            assist.poll();
            if self.made_for.as_ref() != Some(ai) {
                self.made_for = Some(ai.clone());
                return Some(assist.configure(ai));
            }
        }
        let _ = (ai, &self.made_for);
        None
    }

    /// What the AI settings show; none without the service.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn board(&self) -> Option<&Board> {
        self.assist.as_ref().map(mareader_assist::Assist::board)
    }

    /// Tasks the saved model `id` runs now.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn in_use(&self, id: mareader_settings::PresetId) -> usize {
        self.assist.as_ref().map_or(0, |a| a.in_use(id))
    }

    /// Files `ask`'s books, or carries on the paused run; false without AI.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn organize(&mut self, ask: &Ask, batch: u16, resume: bool) -> bool {
        let Some(assist) = &self.assist else {
            return false;
        };
        let req = request(ask, batch);
        self.organizing = Some(match resume {
            true => assist.resume(req),
            false => assist.organize(req),
        });
        true
    }

    /// The run a stop, a failure or quitting left, as the library shows it.
    pub fn paused(&self) -> Option<Paused> {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(run) = self.assist.as_ref().and_then(|a| a.paused()) {
            let (done, total) = run.steps();
            return Some(Paused {
                done,
                total,
                books: run.books().map(mareader_library::BookId).collect(),
                whole: run.whole,
                ready: run.finished(),
            });
        }
        None
    }

    /// Sets the paused run aside for an Undo; false if there was none.
    pub fn set_aside(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(assist) = &self.assist {
            return assist.set_aside();
        }
        false
    }

    pub fn bring_back(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(assist) = &self.assist {
            return assist.bring_back();
        }
        false
    }

    /// Drops the paused run for good, once its plan is applied.
    pub fn forget(&self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(assist) = &self.assist {
            assist.forget();
        }
    }

    /// What the library run said since the last call.
    pub fn organized(&mut self) -> Vec<Heard> {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(run) = &mut self.organizing {
            let heard: Vec<Heard> = run.poll().into_iter().map(heard).collect();
            let over = heard.iter().any(|h| !matches!(h, Heard::Progress { .. }));
            if over {
                self.organizing = None;
            }
            return heard;
        }
        Vec::new()
    }

    pub fn stop_organizing(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(mut run) = self.organizing.take() {
            run.cancel();
        }
    }

    /// Rewrites a page; none without the service.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn rewrite(&self, job: mareader_assist::PageJob) -> Option<mareader_assist::Rewriting> {
        self.assist.as_ref().map(|a| a.rewrite(job))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn send(&mut self, job: Job) {
        if let Some(assist) = &mut self.assist {
            assist.send(job);
        }
    }
}

impl Models for Ai {
    #[cfg(not(target_arch = "wasm32"))]
    fn capacity(&self, preset: &ModelPreset) -> Option<u32> {
        self.board().map(|board| board.capacity(preset))
    }

    #[cfg(target_arch = "wasm32")]
    fn capacity(&self, _: &ModelPreset) -> Option<u32> {
        None
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn start(&self, task: &AiTask, source: &Source, pages: usize) -> Result<Handle, &'static str> {
        let rewrite = match task {
            AiTask::Summary => mareader_assist::Rewrite::Summarize(SummaryStyle::Paragraphs),
            AiTask::Points => mareader_assist::Rewrite::Summarize(SummaryStyle::Points),
            AiTask::Translate { language } => {
                let name = language_name(language).unwrap_or(language);
                mareader_assist::Rewrite::Translate(name.to_owned())
            }
            AiTask::Simplify => mareader_assist::Rewrite::Simplify,
        };
        let job = mareader_assist::PageJob {
            rewrite,
            pieces: source.pieces.clone(),
            pages,
        };
        let off = "The AI service couldn't start. Restart the app to try again.";
        let live = self.rewrite(job).ok_or(off)?;
        Ok(Handle::Live(Box::new(Writer(live))))
    }

    #[cfg(target_arch = "wasm32")]
    fn start(&self, _: &AiTask, _: &Source, _: usize) -> Result<Handle, &'static str> {
        Err("Page AI needs the desktop app.")
    }
}

/// The service's writer, wrapped so the ledger's handle can hold it.
#[cfg(not(target_arch = "wasm32"))]
struct Writer(mareader_assist::Rewriting);

/// The service's own line to its model, as a run reads it.
#[cfg(not(target_arch = "wasm32"))]
impl Writing for Writer {
    fn news(&mut self) -> Vec<News> {
        let came = mareader_assist::Rewriting::poll(&mut self.0)
            .into_iter()
            .map(|n| match n {
                mareader_assist::Rewrote::Partial(r) => News::Partial(r),
                mareader_assist::Rewrote::Done(r) => News::Done(r),
            });
        came.collect()
    }
}

/// The library's ask, keyed as the service wants it.
#[cfg(not(target_arch = "wasm32"))]
fn request(ask: &Ask, batch: u16) -> mareader_assist::Request {
    use mareader_assist::{Existing, Loose};
    let books = (ask.books.iter())
        .map(|b| Loose {
            key: b.id.0,
            title: b.title.clone(),
            author: b.author.clone().unwrap_or_default(),
        })
        .collect();
    let shelves = (ask.shelves.iter())
        .map(|s| Existing {
            key: s.id.0,
            name: s.name.clone(),
            parent: s.parent.map(|p| p.0),
        })
        .collect();
    mareader_assist::Request {
        books,
        shelves,
        batch: usize::from(batch),
        whole: ask.whole,
    }
}

/// The service's news, in the library's words.
#[cfg(not(target_arch = "wasm32"))]
fn heard(news: mareader_assist::Organized) -> Heard {
    use mareader_assist::{Organized, Stage};
    match news {
        Organized::Progress(p) => Heard::Progress {
            filing: p.stage == Stage::Filing,
            done: p.done,
            total: p.total,
        },
        Organized::Done(Ok(plan)) => Heard::Plan(proposal(plan)),
        Organized::Done(Err(f)) => Heard::Failed(failed(&f)),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn proposal(plan: mareader_assist::Plan) -> Proposal {
    use mareader_library::{BookId, ShelfId};
    let spot = |s: mareader_assist::Spot| match s {
        mareader_assist::Spot::Existing(key) => Spot::Shelf(ShelfId(key)),
        mareader_assist::Spot::New(i) => Spot::New(i),
    };
    Proposal {
        shelves: (plan.shelves.into_iter())
            .map(|s| (s.name, s.parent.map(spot)))
            .collect(),
        placed: (plan.placed.into_iter())
            .map(|(key, s)| (BookId(key), spot(s)))
            .collect(),
        left: plan.left.into_iter().map(BookId).collect(),
    }
}

/// What a failed run's toast says; a busy model asks to wait.
#[cfg(not(target_arch = "wasm32"))]
fn failed(f: &mareader_study::Failure) -> Toast {
    use mareader_study::Why;
    let said = match f.why {
        Why::InUse => f
            .detail
            .clone()
            .unwrap_or_else(|| f.why.message().to_owned()),
        Why::NotHere => "No AI is set up to organize with. Choose a model in Settings › AI.".into(),
        Why::Refused => "The model declined to organize these books.".into(),
        Why::BadAnswer => "The model's answers couldn't be read. Try another model.".into(),
        why => format!("Couldn't organize the books. {}", why.message()),
    };
    match f.why {
        Why::InUse | Why::NotHere | Why::NoKey | Why::Locked => Toast::warning(said),
        _ => Toast::error(said),
    }
}
