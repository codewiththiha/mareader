//! The reader runtime: the SESSION's lifecycle owner, between the Shell's
//! manager and the reader host.
//!
//! ```text
//! session start  → ReaderRuntime::begin_mount → ReaderHost (panes mount) → mark_ready
//! Library        → host: panes prepare to leave → Shell command: navigate
//! session end    → host disposes every pane (explicit, observable)
//!                → ReaderRuntime::dispose awaits the panes' tails → Disposed
//! ```
//!
//! Ownership rule: the runtime owns the SESSION's lifetime and nothing
//! document-shaped. Documents, virtualizers, engine sessions and the
//! listeners around them belong to the panes (`crate::pane`), which the
//! host (`crate::host`) creates and disposes; the runtime only refuses new
//! work once the session is ending and reports its own completion after the
//! panes' teardown tails resolved.
//!
//! Disposal is a state machine, not a flag pile: `New → Mounting → Ready →
//! Disposing → Disposed`. Work-ops are refused once `Disposing` is entered;
//! teardown-ops stay admitted until `Disposed`. A disposed runtime is never
//! revived — the next session runs `begin_mount`, which starts a NEW
//! generation. Async tails capture the generation they belong to and are
//! stale-guarded by it.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use serde::Serialize;

use crate::host::contract::PaneTeardown;

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

/// The runtime's self-reported view, published to the diagnostics surface on
/// every transition: the runtime itself reports its lifecycle, and disposal
/// completion is observable from the runtime, not inferred from its
/// surroundings. The resources it counts are the panes' (the host reports
/// them).
#[derive(Clone, Copy, Debug)]
pub struct RuntimeView {
    pub lifecycle: RuntimeLifecycle,
    pub generation: u64,
}

// ---------------------------------------------------------------------------
// The runtime itself
// ---------------------------------------------------------------------------

/// The session's lifecycle owner. Copy by design: the host and every pane
/// share ONE runtime through Copy handles onto the same core.
#[derive(Clone, Copy)]
pub struct ReaderRuntime {
    core: RwSignal<RuntimeCore>,
}

impl Default for ReaderRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl ReaderRuntime {
    /// A new runtime, one per session. Its reported generation is seeded from
    /// the session's ORDINAL rather than from zero, so `begin_mount` publishes
    /// ordinal `n` and no two sessions of one frame can claim the same
    /// in-frame identity (§21). A disposal is therefore never mistaken for a
    /// first mount. This stays the runtime's own lifetime stamp — the shell's
    /// reader-session count, not this number, is the identity the browser
    /// suite asserts across frames.
    pub fn new() -> Self {
        let ordinal = crate::diagnostics::next_session_ordinal();
        let core = RuntimeCore {
            generation: ordinal.saturating_sub(1),
            ..RuntimeCore::default()
        };
        crate::diagnostics::publish_runtime_view(core.lifecycle, core.generation);
        Self {
            core: RwSignal::new(core),
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
    /// Read through `try_` for the same reason the lifecycle is: a tail that
    /// outlives the arena must not panic on it. A disposed runtime answers
    /// with a stamp no live generation can carry.
    pub fn generation(&self) -> u64 {
        self.core
            .try_get_untracked()
            .map(|core| core.generation)
            .unwrap_or(u64::MAX)
    }

    /// Write to the core and republish the diagnostics view. The async
    /// disposal tail calls this AFTER its owner may already be disposed, and
    /// a disposed signal must not turn a completed teardown into a panic. A
    /// dead runtime's bookkeeping is dropped, not fatal.
    fn set(&self, f: impl FnOnce(&mut RuntimeCore)) {
        let _ = self.core.try_update(|core| {
            f(core);
            crate::diagnostics::publish_runtime_view(core.lifecycle, core.generation);
        });
    }

    /// The session mount: start (or restart, as a NEW generation) the
    /// runtime. The host and its first pane are built between this and
    /// [`Self::mark_ready`].
    pub fn begin_mount(&self) -> u64 {
        let mut started = None;
        self.set(|core| {
            if core.begin_mount().is_ok() {
                started = Some(core.generation);
            }
        });
        started.unwrap_or_else(|| self.generation())
    }

    /// The session's host is built and its panes are mounting: live.
    pub fn mark_ready(&self) {
        self.set(|core| {
            let _ = core.mark_ready();
        });
    }

    /// Dispose the runtime: enter `Disposing` (work refused from here), then
    /// await the panes' teardown tails the host handed over — the engine
    /// destroys, the virtualizers' final dispose — and only then mark
    /// `Disposed` and report completion. Safe to call exactly once; later
    /// calls are no-ops that return `false`. Never revives.
    ///
    /// The panes' SYNC teardown (read point, owner cleanup, listeners,
    /// observers, timers) already ran when the host disposed them, while
    /// the session was still alive; this function runs inside the session
    /// unmount's cleanup and only owns the ordering of the async tails.
    pub fn dispose(&self, api: crate::context::ApiHandle, panes: PaneTeardown) -> bool {
        let mut began = false;
        self.set(|core| {
            began = core.begin_dispose();
        });
        if !began {
            return false;
        }
        let rt = *self;
        let generation = self.generation();
        spawn_local(async move {
            panes.await;
            rt.set(|core| {
                let _ = core.finish_dispose(generation);
            });
            // The terminal report, published from OUTSIDE the arena. This tail
            // resumes after the unmount that began it has disposed the session
            // signals, so the `set` above is refused and the view would stay
            // `Disposing` forever — a runtime that never says it finished,
            // which is precisely what the Shell's manager awaits and what the
            // baseline probe reads (§12, §21).
            crate::diagnostics::publish_runtime_view(RuntimeLifecycle::Disposed, generation);
            // The final push for this session: the runtime is disposed, the
            // panes are gone, and the Shell's baseline verdict reads this
            // digest. It leaves BEFORE the completion: whoever awaits the
            // completion may remove the frame, and the port with it.
            crate::diagnostics::publish_digest(&api);
            // The Shell's manager awaits the dispose export's promise before
            // it removes or recycles the frame (§5): the tail's end resolves
            // it, document or not.
            crate::resolve_dispose();
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
