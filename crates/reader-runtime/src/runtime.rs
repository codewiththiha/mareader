//! The reader runtime: the explicit lifecycle owner between the shell and
//! the reader's resources.
//!
//! Phase 1's contract, in one place:
//!
//! ```text
//! /reader mounts  → ReaderRuntime::begin_mount → (effects install) → mark_ready
//! document close  → Shell command: navigate to the library → the route flip
//! /reader unmounts→ ReaderRuntime::dispose (ordered, idempotent, observable)
//! ```
//!
//! Ownership rule: domain signals stay in `ReaderState` (they are the
//! reader's reactive model); everything that must DIE with the reader —
//! the document session, the virtualizers, the pdf.js engine session's
//! document-level operations — is owned and driven from here. The shell
//! (`ReaderContext`) holds this runtime as a Copy handle for coordination; it
//! does not own reader resources.
//!
//! Disposal is a state machine, not a flag pile: `New → Mounting → Ready →
//! Disposing → Disposed`. Work-ops (open, render, register, document
//! close) are refused once `Disposing` is entered; teardown-ops (destroy,
//! sweep) stay admitted until `Disposed`. A disposed runtime is never
//! revived — the next `/reader` entry runs `begin_mount`, which starts a
//! NEW generation under the same slot (the same stamping philosophy the
//! document session has always used). Async tails capture the generation
//! they belong to and are stale-guarded by it.

use leptos::prelude::*;
use serde::Serialize;
use wasm_bindgen_futures::spawn_local;

use crate::services::document::session;

// ---------------------------------------------------------------------------
// Lifecycle state machine (pure — host-testable)
// ---------------------------------------------------------------------------

/// The reader runtime's lifecycle. Compiler-visible states; the transitions
/// live on [`RuntimeCore`] and nowhere else.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeLifecycle {
    #[default]
    New,
    Mounting,
    Ready,
    Disposing,
    Disposed,
}

impl RuntimeLifecycle {
    /// Whether reader WORK may start in this state (opens, renders, page
    /// registrations, document close). Refused from `Disposing` on: new
    /// work cannot start while disposal is progressing.
    pub fn admits_work(self) -> bool {
        !matches!(self, Self::Disposing | Self::Disposed)
    }

    /// Whether TEARDOWN work may run (engine destroy, sweeps). Admitted one
    /// state longer than work: the disposing tail is teardown by definition.
    pub fn admits_teardown(self) -> bool {
        self != Self::Disposed
    }
}

/// The runtime's mutable core: the current state plus the generation stamp
/// every async tail captures. One generation per mount; never reused.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct RuntimeCore {
    pub lifecycle: RuntimeLifecycle,
    pub generation: u64,
}

impl RuntimeCore {
    /// `New | Disposing | Disposed → Mounting` as a NEW generation — a
    /// disposed runtime is never revived, and a mount that lands while a
    /// disposal tail is still awaiting the engine starts a fresh generation
    /// the stale tail cannot touch (its completion is generation-guarded).
    /// `Mounting → Mounting` is the idempotent re-entry; `Ready` refuses:
    /// the runtime is already mounted.
    pub fn begin_mount(&mut self) -> Result<u64, RuntimeLifecycle> {
        match self.lifecycle {
            RuntimeLifecycle::New | RuntimeLifecycle::Disposing | RuntimeLifecycle::Disposed => {
                self.generation += 1;
                self.lifecycle = RuntimeLifecycle::Mounting;
                Ok(self.generation)
            }
            RuntimeLifecycle::Mounting => Ok(self.generation),
            other => Err(other),
        }
    }

    /// `Mounting → Ready`. The mount finished installing effects/resources.
    pub fn mark_ready(&mut self) -> Result<(), RuntimeLifecycle> {
        if self.lifecycle == RuntimeLifecycle::Mounting {
            self.lifecycle = RuntimeLifecycle::Ready;
            Ok(())
        } else {
            Err(self.lifecycle)
        }
    }

    /// Any live state → `Disposing`, exactly once. `false` from `Disposing`
    /// (a dispose is already running) and `Disposed` (idempotent no-op).
    pub fn begin_dispose(&mut self) -> bool {
        if self.lifecycle.admits_work() {
            self.lifecycle = RuntimeLifecycle::Disposing;
            true
        } else {
            false
        }
    }

    /// `Disposing → Disposed` FOR THIS GENERATION: the teardown tail's last
    /// act, guarded by the generation the tail captured — a mount that
    /// started a new generation while the engine destroy was still in
    /// flight makes the stale completion a no-op instead of killing the
    /// fresh runtime.
    pub fn finish_dispose(&mut self, generation: u64) -> Result<(), RuntimeLifecycle> {
        if self.lifecycle == RuntimeLifecycle::Disposing && self.generation == generation {
            self.lifecycle = RuntimeLifecycle::Disposed;
            Ok(())
        } else {
            Err(self.lifecycle)
        }
    }
}

// ---------------------------------------------------------------------------
// Resources: the browser/runtime things that die with the reader
// ---------------------------------------------------------------------------

/// The reader-owned runtime resources. Deliberately narrow: virtualizers
/// register here when the strips mount and are disposed BY the runtime's
/// dispose sequence — component cleanup stays as the inner safety net, not
/// the owner (Phase 1 §9: do not rely only on Leptos cleanup). Engine
/// document-level operations route through [`PdfSessionHandle`].
#[derive(Clone, Default)]
pub struct ReaderResources {
    virtualizers: Vec<virtual_list_leptos::Virtualizer>,
}

impl ReaderResources {
    pub fn track(&mut self, v: &virtual_list_leptos::Virtualizer) {
        if !self.virtualizers.iter().any(|known| known == v) {
            self.virtualizers.push(v.clone());
        }
    }

    /// The owner's own cleanup dropped one. Returns whether it was ours.
    pub fn untrack(&mut self, v: &virtual_list_leptos::Virtualizer) -> bool {
        let Some(at) = self.virtualizers.iter().position(|known| known == v) else {
            return false;
        };
        self.virtualizers.remove(at);
        true
    }

    pub fn virtualizer_count(&self) -> usize {
        self.virtualizers.len()
    }

    /// Consume the registry, handing every still-registered virtualizer to
    /// the caller (the disposal tails): the registry is out of the picture
    /// from that moment, so a mount landing mid-tail cannot be touched.
    pub fn into_virtualizers(self) -> Vec<virtual_list_leptos::Virtualizer> {
        self.virtualizers
    }
}

// ---------------------------------------------------------------------------
// The PDF session handle: the engine boundary behind one owner
// ---------------------------------------------------------------------------

/// The reader runtime's handle onto the PDF engine session. Every call is
/// guarded by the lifecycle state captured when the handle was made: work
/// ops no-op once disposal began, teardown ops stay admitted until
/// `Disposed`. This is the production path for document-level engine
/// operations; the module-level `pdf_engine::api` functions it delegates to
/// are the adapter's internals, with removal tied to the Phase 4 format
/// split (they remain for the page-host surface, classified in the Phase 1
/// report).
#[derive(Clone, Copy)]
pub struct PdfSessionHandle {
    work: bool,
    teardown: bool,
}

impl PdfSessionHandle {
    /// Open a document. The caller checks [`RuntimeLifecycle::admits_work`]
    /// before starting an open (the open flow itself decides what a refused
    /// open means for the UI); the handle records the decision.
    pub fn work_admitted(&self) -> bool {
        self.work
    }

    /// Open a document through the engine session. The work admission is
    /// decided by the caller (the open flow owns what a refusal means for
    /// the UI) via [`Self::work_admitted`]; this delegation keeps every
    /// document-level engine entry on the handle.
    pub async fn open(
        &self,
        path: &str,
    ) -> Result<pdf_engine::types::OpenResult, pdf_engine::api::EngineError> {
        pdf_engine::api::open(path).await
    }

    pub async fn destroy(&self) {
        if !self.teardown {
            return;
        }
        let _ = pdf_engine::api::destroy().await;
    }

    pub fn sweep(&self) {
        if !self.teardown {
            return;
        }
        pdf_engine::api::sweep();
    }

    pub fn sweep_snapshots(&self) {
        if !self.teardown {
            return;
        }
        pdf_engine::api::sweep_snapshots();
    }
}

/// The runtime's self-reported view, published to the diagnostics surface on
/// every transition (Phase 1 §12: the runtime itself reports its lifecycle
/// and its live resource count; disposal completion is observable from the
/// runtime, not inferred from its surroundings).
#[derive(Clone, Copy, Debug)]
pub struct RuntimeView {
    pub lifecycle: RuntimeLifecycle,
    pub generation: u64,
    pub virtualizer_count: usize,
}

// ---------------------------------------------------------------------------
// The runtime itself
// ---------------------------------------------------------------------------

/// The reader runtime owner. Copy by design: the shell and the reader tree
/// share ONE runtime through Copy handles onto the same core/resources, the
/// same way they share the signals.
#[derive(Clone, Copy)]
pub struct ReaderRuntime {
    core: RwSignal<RuntimeCore>,
    resources: StoredValue<ReaderResources, LocalStorage>,
}

impl Default for ReaderRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl ReaderRuntime {
    /// A new runtime, one per session. Its reported generation is seeded from
    /// the session's ORDINAL rather than from zero, so `begin_mount` publishes
    /// ordinal `n` and no two sessions of one artifact can claim the same
    /// identity (§21). A disposal is therefore never mistaken for a first
    /// mount, and the browser suite can assert "a NEW runtime, not the revived
    /// one" from the number alone.
    pub fn new() -> Self {
        let ordinal = crate::diagnostics::next_session_ordinal();
        let core = RuntimeCore {
            generation: ordinal.saturating_sub(1),
            ..RuntimeCore::default()
        };
        crate::diagnostics::publish_runtime_view(core.lifecycle, core.generation, 0);
        Self {
            core: RwSignal::new(core),
            resources: StoredValue::new_local(ReaderResources::default()),
        }
    }

    /// The lifecycle as this runtime reports it. A runtime whose own arena
    /// owner has been disposed reads as `Disposed`: the bookkeeping is gone
    /// because the runtime is gone, and a caller asking after that gets the
    /// truth rather than a panic — a wasm abort here would poison the
    /// artifact for every later session.
    pub fn lifecycle(&self) -> RuntimeLifecycle {
        self.core
            .try_get_untracked()
            .map(|core| core.lifecycle)
            .unwrap_or(RuntimeLifecycle::Disposed)
    }

    /// The generation stamp async tails capture to reject stale results.
    /// The stamp is read through `try_` for the same reason the lifecycle is:
    /// a tail that outlives the arena must not panic on it. A disposed
    /// runtime answers with a stamp no live generation can carry — there is
    /// no generation left to be stale FOR.
    pub fn generation(&self) -> u64 {
        self.core
            .try_get_untracked()
            .map(|core| core.generation)
            .unwrap_or(u64::MAX)
    }

    /// Write to the core and republish the diagnostics view. The async
    /// disposal tails call this AFTER their owner may already be disposed —
    /// the engine destroy outlives the session's reactive scope — and a
    /// disposed signal must not turn a completed teardown into a panic
    /// (wasm's abort would poison the artifact for every later session). A
    /// dead runtime's bookkeeping is dropped, not fatal.
    fn set(&self, f: impl FnOnce(&mut RuntimeCore)) {
        let _ = self.core.try_update(|core| {
            f(core);
            crate::diagnostics::publish_runtime_view(
                core.lifecycle,
                core.generation,
                self.resources
                    .try_with_value(|r| r.virtualizer_count())
                    .unwrap_or(0),
            );
        });
    }

    /// The `/reader` route mount: start (or restart, as a NEW generation) the
    /// runtime. A restart that lands while a disposal tail is still awaiting
    /// the engine takes the stale resources away from that tail (disposING
    /// them here — every stale instance is route-dead and its dispose is
    /// idempotent), so the tail can never touch the fresh generation's
    /// registrations. Effects and resource registration happen between this
    /// and [`Self::mark_ready`].
    pub fn begin_mount(&self) -> u64 {
        let mut started = None;
        self.set(|core| {
            if core.begin_mount().is_ok() {
                started = Some(core.generation);
            }
        });
        if started.is_some() {
            let stale = self.resources.try_get_value().unwrap_or_default();
            let _ = self.resources.try_set_value(ReaderResources::default());
            for v in stale.into_virtualizers() {
                v.dispose();
            }
        }
        started.unwrap_or_else(|| self.generation())
    }

    /// The mount's effects and resources are installed: the runtime is live.
    pub fn mark_ready(&self) {
        self.set(|core| {
            let _ = core.mark_ready();
        });
    }

    /// Register a reader virtualizer with the runtime's resource owner.
    /// Called where `use_virtualizer` returns; the registering owner's
    /// cleanup pairs with [`Self::untrack_virtualizer`].
    pub fn track_virtualizer(&self, v: &virtual_list_leptos::Virtualizer) {
        // Registration and its cleanup can run either side of the arena's
        // death (a strip's cleanup against a runtime whose session already
        // ended): the resources are borrowed through `try_` on both ends, so
        // a dead registry drops the bookkeeping instead of aborting.
        let _ = self.resources.try_update_value(|r| r.track(v));
    }

    /// The registering owner's cleanup dropped its virtualizer.
    pub fn untrack_virtualizer(&self, v: &virtual_list_leptos::Virtualizer) {
        let _ = self.resources.try_update_value(|r| {
            r.untrack(v);
        });
    }

    /// The guarded engine-session handle. Capture it FRESH at each use — the
    /// guards snapshot the lifecycle at creation.
    pub fn pdf(&self) -> PdfSessionHandle {
        let lifecycle = self.lifecycle();
        PdfSessionHandle {
            work: lifecycle.admits_work(),
            teardown: lifecycle.admits_teardown(),
        }
    }

    /// Dispose the runtime: close the active document IF one is open, tear
    /// down every reader-owned resource in dependency order, and become
    /// unusable. Safe to call exactly once; later calls are no-ops that
    /// return `false`. Never revives — the next mount is a new generation.
    ///
    /// Order (adapted to the real dependency graph Phase 0 mapped: strip
    /// callbacks can straggle past the document's death, so the signal
    /// liveness guards from Phase 0 run ahead of the explicit teardown):
    /// enter `Disposing` (work refused from here) → claim the session stamp
    /// if a document was open → close the paper session → spawn the tail:
    /// close the document session (destroy awaited, sweeps) → dispose every
    /// registered virtualizer → mark `Disposed` → report disposal
    /// completion. The DURABLE half (the read point) already went across the
    /// boundary while the session was alive — the dispose export flushes
    /// before it unmounts, because this function runs with the reactive tree
    /// already being torn down.
    pub fn dispose(&self, state: crate::context::ReaderContext) -> bool {
        let mut began = false;
        self.set(|core| {
            began = core.begin_dispose();
        });
        if !began {
            return false;
        }
        // This runs inside the unmount's cleanup: the session's reactive
        // owner is being torn down, so the reads below are try_ (a disposed
        // slice has nothing left to say) and the reactive reset that a
        // DOCUMENT close owes is not repeated here — the state is about to
        // be dropped whole, and the durable write already happened while the
        // session was alive (the dispose export's flush). Only the
        // engine-side close has to run: the paper session outlives the
        // artifact's mount.
        let doc_open = state
            .reader
            .document
            .status
            .try_get_untracked()
            .map(|s| s != pdf_engine::types::DocStatus::Idle)
            .unwrap_or(false);
        let stamp = doc_open.then(session::claim);
        if let Some(stamp) = stamp {
            crate::diagnostics::note_reader_runtime_dispose_begin(stamp);
        }
        pdf_engine::backdrop::document_close();
        // The resources leave the registry NOW: from this moment the tail
        // alone owns them, and a mount that lands before the engine destroy
        // resolves starts with a clean registry the stale tail cannot reach.
        let stale = self.resources.try_get_value().unwrap_or_default();
        let _ = self.resources.try_set_value(ReaderResources::default());
        let pdf = self.pdf();
        let rt = *self;
        let generation = self.generation();
        spawn_local(async move {
            if stamp.is_some() {
                pdf.destroy().await;
                pdf.sweep();
                pdf.sweep_snapshots();
            }
            for v in stale.into_virtualizers() {
                v.dispose();
            }
            rt.set(|core| {
                let _ = core.finish_dispose(generation);
            });
            // The terminal report, published from OUTSIDE the arena. This tail
            // resumes after the unmount that began it has disposed the session
            // signals, so the `set` above is refused and the view would stay
            // `Disposing` forever — a runtime that never says it finished,
            // which is precisely what the Shell's manager awaits and what the
            // baseline probe reads (§12, §21). The view is a thread-local for
            // this beat: a runtime still has to be able to report its own
            // completion after its reactive scope is gone.
            crate::diagnostics::publish_runtime_view(RuntimeLifecycle::Disposed, generation, 0);
            if let Some(stamp) = stamp {
                crate::diagnostics::note_reader_runtime_dispose_complete(stamp);
                app_state::memory::log_heap("close");
            }
            // The final push for this session: the runtime is disposed, the
            // engine is drained, and the Shell's baseline verdict reads this
            // digest.
            crate::diagnostics::publish_digest(&state.api);
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// create → dispose: the machine walks New → (mount skipped; dispose is
    /// legal from any live state) → Disposing → Disposed, once.
    #[test]
    fn a_new_runtime_disposes_once() {
        let mut core = RuntimeCore::default();
        assert_eq!(core.lifecycle, RuntimeLifecycle::New);
        assert!(core.begin_dispose());
        assert_eq!(core.lifecycle, RuntimeLifecycle::Disposing);
        assert!(core.finish_dispose(core.generation).is_ok());
        assert_eq!(core.lifecycle, RuntimeLifecycle::Disposed);
        // A second dispose is a no-op, not a revival.
        assert!(!core.begin_dispose());
        assert!(core.finish_dispose(core.generation).is_err());
    }

    /// create → document open → dispose: the mounting/ready path.
    #[test]
    fn mount_readies_and_dispose_ends() {
        let mut core = RuntimeCore::default();
        let generation = core.begin_mount().expect("mount from New");
        assert_eq!(generation, 1);
        assert_eq!(core.lifecycle, RuntimeLifecycle::Mounting);
        core.mark_ready().expect("ready from Mounting");
        assert_eq!(core.lifecycle, RuntimeLifecycle::Ready);
        assert!(core.begin_dispose());
        assert!(core.finish_dispose(core.generation).is_ok());
    }

    /// create → dispose → dispose again: the second call never re-enters.
    #[test]
    fn dispose_is_idempotent() {
        let mut core = RuntimeCore::default();
        core.begin_mount().expect("mount");
        assert!(core.begin_dispose());
        assert!(!core.begin_dispose(), "dispose twice must not re-enter");
        assert!(core.finish_dispose(core.generation).is_ok());
        assert!(!core.begin_dispose(), "dispose after disposed is a no-op");
    }

    /// create → dispose → new runtime: the slot restarts as a NEW generation;
    /// the old generation is never reused.
    #[test]
    fn a_disposed_slot_restarts_as_a_new_generation() {
        let mut core = RuntimeCore::default();
        core.begin_mount().expect("mount 1");
        assert_eq!(core.generation, 1);
        core.mark_ready().expect("ready");
        core.begin_dispose();
        core.finish_dispose(core.generation).expect("disposed");
        let generation = core.begin_mount().expect("mount after dispose");
        assert_eq!(generation, 2, "the next mount is a new runtime generation");
        assert_eq!(core.lifecycle, RuntimeLifecycle::Mounting);
        core.mark_ready().expect("ready 2");
        assert_ne!(generation, 1);
    }

    /// New work cannot start while disposal is progressing.
    #[test]
    fn work_is_refused_from_disposing_on() {
        let mut core = RuntimeCore::default();
        core.begin_mount().expect("mount");
        core.mark_ready().expect("ready");
        assert!(core.lifecycle.admits_work());
        core.begin_dispose();
        assert!(!core.lifecycle.admits_work(), "Disposing refuses new work");
        assert!(
            core.lifecycle.admits_teardown(),
            "Disposing still admits teardown"
        );
        core.finish_dispose(core.generation).expect("disposed");
        assert!(!core.lifecycle.admits_teardown(), "Disposed admits nothing");
    }

    /// The ready runtime refuses a second mount (no accidental restart while
    /// live); the mounting runtime tolerates the re-entry (idempotent); a
    /// disposing runtime restarts as a new generation (fast close→reopen).
    #[test]
    fn mount_guards() {
        let mut core = RuntimeCore::default();
        core.begin_mount().expect("mount");
        assert!(
            core.begin_mount().is_ok(),
            "mounting re-entry is idempotent"
        );
        core.mark_ready().expect("ready");
        assert!(
            core.begin_mount().is_err(),
            "a Ready runtime is already mounted"
        );
    }

    /// finish_dispose only completes from Disposing (the ordered tail's
    /// marker): a live runtime cannot be marked Disposed out of order.
    #[test]
    fn finish_requires_disposing() {
        let mut core = RuntimeCore::default();
        assert!(core.finish_dispose(core.generation).is_err());
        core.begin_mount().expect("mount");
        assert!(core.finish_dispose(core.generation).is_err());
        core.begin_dispose();
        assert!(core.finish_dispose(core.generation).is_ok());
    }

    /// A mount landing while a disposal tail is still in flight (the fast
    /// close → reopen window) starts a NEW generation the stale tail cannot
    /// finish: the tail's completion is generation-guarded and no-ops.
    #[test]
    fn a_mount_during_disposal_orphans_the_stale_completion() {
        let mut core = RuntimeCore::default();
        core.begin_mount().expect("mount 1");
        core.mark_ready().expect("ready");
        let stale_generation = core.generation;
        assert!(core.begin_dispose());
        // The reopen lands before the tail finished:
        let fresh = core.begin_mount().expect("mount during disposal");
        assert_ne!(fresh, stale_generation);
        core.mark_ready().expect("the fresh runtime readies");
        // The stale tail's completion lands late: it must NOT dispose the
        // fresh generation.
        assert!(core.finish_dispose(stale_generation).is_err());
        assert_eq!(core.lifecycle, RuntimeLifecycle::Ready);
        // The fresh runtime's own disposal works normally.
        assert!(core.begin_dispose());
        assert!(core.finish_dispose(core.generation).is_ok());
    }
}
