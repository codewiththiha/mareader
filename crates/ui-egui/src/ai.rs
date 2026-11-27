//! The AI service behind Explain and the AI settings; desktop only.

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

use mareader_engine::page_ai::extract::Source;
use mareader_engine::page_ai::state::Handle;
use mareader_engine::page_ai::view::Models;
use mareader_library::AiTask;
use mareader_settings::{AiSettings, ModelPreset};
#[cfg(not(target_arch = "wasm32"))]
use mareader_settings::{SummaryStyle, language_name};
use mareader_study::pending::Answerers;

#[cfg(not(target_arch = "wasm32"))]
use mareader_engine::library::{Ask, Proposal, Spot};
use mareader_engine::library::{Heard, Paused};
#[cfg(not(target_arch = "wasm32"))]
use mareader_engine::toast::Toast;

#[cfg(not(target_arch = "wasm32"))]
pub use mareader_assist::{Board, Job};

/// The service once attached, and the settings its answerer follows.
#[derive(Default)]
pub struct Ai {
    #[cfg(not(target_arch = "wasm32"))]
    assist: Option<mareader_assist::Assist>,
    made_for: Option<(AiSettings, Option<String>, Option<Vec<String>>)>,
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

    /// Takes in finished jobs; when `ai` changed since, follows it and
    /// hands back the answerers for the study tools. None on the web.
    pub fn follow(&mut self, ai: &AiSettings, dictionary: Option<&str>) -> Option<Answerers> {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(assist) = &mut self.assist {
            assist.poll();
            let apple = assist.board().apple.ready().and_then(|state| match state {
                mareader_assist::AppleState::Ready(languages) => Some(languages.clone()),
                _ => None,
            });
            let key = (ai.clone(), dictionary.map(str::to_owned), apple.clone());
            if self.made_for.as_ref() != Some(&key) {
                self.made_for = Some(key);
                let resolved = resolved_settings(ai, dictionary, apple);
                let _ = assist.configure(&resolved);
                return Some(assist.answerers());
            }
        }
        let _ = (ai, dictionary, &self.made_for);
        None
    }

    /// Probes Apple's language list when any saved model can need it.
    #[cfg(target_os = "macos")]
    pub fn probe_translation(&mut self, ai: &AiSettings) {
        let apple = ai
            .presets
            .iter()
            .any(|m| m.provider == mareader_settings::Provider::Apple);
        let idle = self
            .board()
            .is_some_and(|b| b.apple == mareader_assist::Load::Idle);
        if apple && idle {
            self.send(Job::Probe);
        }
    }

    /// What the AI settings show; none without the service.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn board(&self) -> Option<&Board> {
        self.assist.as_ref().map(mareader_assist::Assist::board)
    }

    /// Every AI job queued or running, for Settings.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn jobs(&self) -> Vec<mareader_assist::JobInfo> {
        self.assist
            .as_ref()
            .map(mareader_assist::Assist::jobs)
            .unwrap_or_default()
    }

    /// Tasks `preset` may run at once: the number the queue lets in.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn capacity(&self, preset: &ModelPreset) -> Option<u32> {
        self.assist.as_ref().map(|a| a.capacity(preset))
    }

    /// Toasts for tasks that wait behind another task on their model.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn waiting(&self) -> Vec<Toast> {
        let Some(assist) = &self.assist else {
            return Vec::new();
        };
        let told = |q: &mareader_assist::Queued| Toast::info(q.said());
        assist.queued().iter().map(told).collect()
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
            let heard: Vec<Heard> = run.poll().into_iter().filter_map(heard).collect();
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
        Ai::capacity(self, preset)
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
        Ok(Handle::Live(Box::new(live)))
    }

    #[cfg(target_arch = "wasm32")]
    fn start(&self, _: &AiTask, _: &Source, _: usize) -> Result<Handle, &'static str> {
        Err("Page AI needs the desktop app.")
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

/// Resolves language choices for the first model without changing Settings.
#[cfg(not(target_arch = "wasm32"))]
fn resolved_settings(
    ai: &AiSettings,
    dictionary: Option<&str>,
    apple: Option<Vec<String>>,
) -> AiSettings {
    use mareader_settings::{Provider, TaskId, effective_language};
    let mut resolved = ai.with_dictionary_language(dictionary);
    let state = apple.map(mareader_assist::AppleState::Ready);
    for id in [TaskId::Explain, TaskId::StudyCards] {
        if ai.model(id).is_some_and(|p| p.provider == Provider::Apple) {
            let language = match id {
                TaskId::Explain => &mut resolved.tasks.explain.language,
                _ => &mut resolved.tasks.study.language,
            };
            let speaks = |tag: &str| state.as_ref().and_then(|s| s.speaks(tag));
            if let Some(tag) = effective_language(language.as_deref(), None, &speaks) {
                *language = Some(tag.to_owned());
            }
        }
    }
    resolved
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod language_tests {
    use super::*;
    use mareader_settings::{Provider, TaskId};

    #[test]
    fn late_probe_and_model_switch_resolve_without_losing_preferences() {
        let mut ai = AiSettings::default();
        let apple = ai.add_preset(Provider::Apple, "");
        let other = ai.add_preset(Provider::Ollama, "test");
        ai.tasks.choose(TaskId::Explain, Some(apple));
        ai.tasks.explain.language = Some("my".into());
        assert_eq!(
            resolved_settings(&ai, Some("fr"), None)
                .tasks
                .explain
                .language
                .as_deref(),
            Some("my")
        );
        let known = Some(vec!["en-US".into(), "ja-JP".into()]);
        let resolved = resolved_settings(&ai, Some("fr"), known.clone());
        assert_eq!(resolved.tasks.explain.language.as_deref(), Some("en"));
        assert_eq!(resolved.tasks.study.language.as_deref(), Some("en"));
        let only_japanese = resolved_settings(&ai, Some("fr"), Some(vec!["ja-JP".into()]));
        assert_eq!(only_japanese.tasks.explain.language.as_deref(), Some("ja"));
        assert_eq!(ai.tasks.explain.language.as_deref(), Some("my"));
        ai.tasks.choose(TaskId::Explain, Some(other));
        let restored = resolved_settings(&ai, Some("fr"), known);
        assert_eq!(restored.tasks.explain.language.as_deref(), Some("my"));
        assert_eq!(restored.tasks.study.language.as_deref(), Some("fr"));
    }
}

/// The service's news, in the library's words; a wait is toasted apart.
#[cfg(not(target_arch = "wasm32"))]
fn heard(news: mareader_assist::Organized) -> Option<Heard> {
    use mareader_assist::{Organized, Stage};
    Some(match news {
        Organized::Partial(p) => Heard::Progress {
            filing: p.stage == Stage::Filing,
            done: p.done,
            total: p.total,
        },
        Organized::Done(Ok(plan)) => Heard::Plan(proposal(plan)),
        Organized::Done(Err(f)) => Heard::Failed(failed(&f)),
        _ => return None,
    })
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
        Why::NotHere => f.said(),
        Why::Refused => "The model declined to organize these books.".into(),
        Why::BadAnswer => "The model's answers couldn't be read. Try another model.".into(),
        why => format!("Couldn't organize the books. {}", why.message()),
    };
    match f.why {
        Why::InUse | Why::NotHere | Why::NoKey | Why::Locked => Toast::warning(said),
        _ => Toast::error(said),
    }
}
