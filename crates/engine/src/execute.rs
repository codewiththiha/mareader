//! The reducer: every command, carried out directly against [`Session`].
//!
//! `Session::execute` is the one place a command takes effect. What the
//! session owns it changes directly; front-end view updates, dialogs, and
//! runtime-backed jobs leave as typed [`Event`]s. Time arrives as `now`, in
//! seconds, from whoever keeps a clock.

mod library;
mod panes;
mod study;
mod study_mode;
mod voice;

use serde_json::{Map, Value};

use crate::command::Command;
use crate::command_spec;
use crate::event::Event;
use crate::guard;
use crate::journal::{MAX_BATCH_DEPTH, MAX_BATCH_LEN, Step, split_coalesce};
use crate::session::{Done, Session};
use crate::toast::Toast;

impl Session {
    /// Carries out `intent`, unless the active route has no use for it.
    pub fn execute(&mut self, intent: Command, now: f64) -> Done {
        let id = intent.id();
        match guard::catch_panic(|| self.execute_inner(intent, now)) {
            Ok(done) => {
                if done.refused.is_none()
                    && !self.recording_json
                    && let Some(spec) = command_spec::by_id(id)
                    && spec.journal
                    && spec.params.is_empty()
                {
                    self.journal
                        .record(id, Value::Object(Map::new()), now, None);
                }
                done
            }
            Err(msg) => {
                self.emit_panic("execute", &msg);
                Done::refused(format!("engine panic: {msg}"))
            }
        }
    }

    fn execute_inner(&mut self, intent: Command, now: f64) -> Done {
        self.reconcile_texts();
        if !self.route().takes(&intent) || study_mode::locks(self, &intent) {
            return Done::quiet();
        }
        dispatch(self, intent, now)
    }

    /// Carries out the command an id names, built from a JSON parameter value.
    ///
    /// Supports an optional `"coalesce"` string key in `params` so consecutive
    /// journal entries with the same id and key merge into a single step.
    pub fn execute_json(&mut self, id: &str, params: Value, now: f64) -> Result<Done, String> {
        let Some(spec) = command_spec::by_id(id) else {
            return Err(format!("no command has the id {id:?}"));
        };
        let (clean, coalesce) = split_coalesce(params).map_err(|why| format!("{id}: {why}"))?;
        let intent = guard::catch_panic(|| (spec.parse)(self, &clean))
            .map_err(|msg| {
                self.emit_panic("parse", &msg);
                format!("{id}: engine panic: {msg}")
            })?
            .map_err(|why| format!("{id}: {why}"))?;
        let prev = self.recording_json;
        self.recording_json = true;
        let done = self.execute(intent, now);
        self.recording_json = prev;
        if let Some(why) = done.refused {
            return Err(why);
        }
        if spec.journal {
            self.journal
                .record(spec.id, clean, now, coalesce.as_deref());
        }
        Ok(done)
    }

    /// Carries out the command an id names, built from a script's params.
    ///
    /// The door a client with no window uses: the registry finds the row, the
    /// row's `parse` builds the command from the JSON, and `execute` carries it
    /// out. What the command comes to leaves as events, as any other command's
    /// does; what the params cannot build is an `Err` naming the id and the key
    /// that failed. Empty params are an empty object, so a command that reads
    /// nothing needs none.
    pub fn execute_id(&mut self, id: &str, params: &str, now: f64) -> Result<(), String> {
        let held = match params.trim().is_empty() {
            true => Value::Object(Map::new()),
            false => serde_json::from_str(params).map_err(|why| format!("{id}: {why}"))?,
        };
        self.execute_json(id, held, now).map(|_| ())
    }
}

/// Runs a `command.batch` step list with depth and length caps.
fn run_batch(session: &mut Session, steps: Vec<Step>, now: f64) -> Done {
    if session.batch_depth >= MAX_BATCH_DEPTH {
        return Done::refused(format!("batch nesting exceeds {MAX_BATCH_DEPTH}"));
    }
    if steps.len() > MAX_BATCH_LEN {
        return Done::refused(format!("batch exceeds {MAX_BATCH_LEN} commands"));
    }
    session.batch_depth += 1;
    let mut dirty = false;
    let mut refused = None;
    for (idx, step) in steps.into_iter().enumerate() {
        match session.execute_json(&step.id, step.params, now) {
            Ok(done) => dirty |= done.dirty,
            Err(why) => {
                refused = Some(format!("batch step {}: {why}", idx + 1));
                break;
            }
        }
    }
    session.batch_depth = session.batch_depth.saturating_sub(1);
    Done { dirty, refused }
}

/// Exhaustive, so a new command cannot build until it is given a home.
fn dispatch(session: &mut Session, intent: Command, now: f64) -> Done {
    match intent {
        Command::Split(_)
        | Command::FocusPane(_)
        | Command::CyclePane { .. }
        | Command::ClosePane(_)
        | Command::SetRatio(..)
        | Command::Open(_)
        | Command::Export { .. }
        | Command::CopyPages { .. }
        | Command::Search { .. }
        | Command::Zoom(_)
        | Command::View(_)
        | Command::Arrow(_)
        | Command::Page { .. }
        | Command::AutoScroll
        | Command::FirstPage
        | Command::LastPage
        | Command::GoTo { .. }
        | Command::Turn { .. } => panes::run(session, intent, now),
        Command::MovePane { .. }
        | Command::SwapPanes { .. }
        | Command::SetSidebarOverlay(_)
        | Command::SetTitlebarPinned(_)
        | Command::ShowRoute(_)
        | Command::ShowSpace(_)
        | Command::SetAppearance(_)
        | Command::SetSettings(_)
        | Command::SetField { .. }
        | Command::SetInkContrast(_)
        | Command::SetQuickText(_)
        | Command::SetIndependence(_)
        | Command::SavePreset { .. }
        | Command::DeletePreset(_)
        | Command::SetDictionary(_)
        | Command::SetLibraryView(_)
        | Command::Vocab(_)
        | Command::Pronunciation(_) => session.apply(intent),
        #[cfg(not(target_arch = "wasm32"))]
        Command::AttachVocab(_) => session.apply(intent),
        Command::OpenSettings(tab) => emit_changed(session, Event::OpenSettings(tab)),
        Command::Batch(steps) => run_batch(session, steps, now),
        Command::WordList { .. }
        | Command::WordListWindow { .. }
        | Command::Study(_)
        | Command::Explain(_)
        | Command::LookUp(_)
        | Command::Highlight { .. }
        | Command::Note(_)
        | Command::Bookmark(_)
        | Command::ClearSelection(_)
        | Command::SelectText { .. }
        | Command::Dict(_) => study::run(session, intent),
        Command::PageAi(action) => page_ai(session, action, now),
        Command::Skill(action) => session.skills_act(action),
        Command::StudyMode(action) => study_mode::run(session, action, now),
        Command::Space(action) => session.space(action),
        #[cfg(not(target_arch = "wasm32"))]
        Command::Ai(job) => emit_changed(session, Event::Ai(job)),
        Command::PlaceReadMarker { .. }
        | Command::ReadAloud { .. }
        | Command::ReadAloudAct(_)
        | Command::ToggleReadAloud
        | Command::StopReadAloud
        | Command::RevealVoice(..)
        | Command::VoiceModel(..)
        | Command::TryVoice
        | Command::TryVoiceOf(_)
        | Command::StopVoiceTry
        | Command::SayWord(_)
        | Command::SayWordWith { .. }
        | Command::StopSaying => run_timed(session, intent, now, voice::run),
        Command::OpenBook(_)
        | Command::OpenMark(_)
        | Command::OpenAiSelection(_)
        | Command::PlaceBook { .. }
        | Command::Library(_)
        | Command::ForgetPositions { .. } => run_timed(session, intent, now, library::run),
        Command::Update(action) => session.run_update(action),
    }
}

fn emit_changed(session: &mut Session, event: Event) -> Done {
    session.emit(event);
    Done::changed()
}

fn run_timed(
    session: &mut Session,
    intent: Command,
    now: f64,
    f: fn(&mut Session, Command, f64),
) -> Done {
    f(session, intent, now);
    Done::changed()
}

fn note(session: &mut Session, toast: Toast) {
    session.emit(Event::Toast(toast));
}

fn page_ai(session: &mut Session, action: crate::page_ai::Action, now: f64) -> Done {
    use crate::page_ai::Action;
    match action {
        Action::StopFollow(pane) => session.page_ai_stop_follow(pane),
        Action::StopPages(pane) => session.page_ai_stop_groups(pane),
        Action::StopRun(run) => session.page_ai_stop_run(run),
        Action::Show { pane, page, slot } => {
            session.page_ai_show_result(pane, page, slot, now);
        }
        Action::Copy(slot) => session.page_ai_copy(slot),
        Action::Delete(slot) => session.page_ai_delete(slot),
        other => session.emit(Event::PageAi(other)),
    }
    Done::changed()
}

/// One setting written by its path; a value that will not serialize is not.
fn set(session: &mut Session, path: &str, json: serde_json::Result<String>) {
    if let Ok(value) = json {
        session.apply(Command::SetField {
            path: path.into(),
            value,
        });
    }
}
