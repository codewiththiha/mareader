//! The session's lifecycle owner: New → Mounting → Ready →
//! Disposing → Disposed.

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
    /// Whether reader WORK may start: refused from `Disposing` on.
    pub fn admits_work(self) -> bool {
        !matches!(self, Self::Disposing | Self::Disposed)
    }

    /// Whether TEARDOWN may run: one state longer than work.
    pub fn admits_teardown(self) -> bool {
        self != Self::Disposed
    }
}

/// The current state plus the generation stamp tails capture.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct RuntimeCore {
    pub lifecycle: RuntimeLifecycle,
    pub generation: u64,
}

impl RuntimeCore {
    /// → `Mounting` as a NEW generation; `Mounting` re-enters
    /// idempotently, `Ready` refuses.
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

    /// Any live state → `Disposing`, once; `false` from `Disposing`.
    fn begin_dispose(&mut self) -> bool {
        if self.lifecycle.admits_work() {
            self.lifecycle = RuntimeLifecycle::Disposing;
            true
        } else {
            false
        }
    }

    /// `Disposing → Disposed` for THIS generation: a stale tail's
    /// completion no-ops.
    pub fn finish_dispose(&mut self, generation: u64) -> Result<(), RuntimeLifecycle> {
        if self.lifecycle == RuntimeLifecycle::Disposing && self.generation == generation {
            self.lifecycle = RuntimeLifecycle::Disposed;
            Ok(())
        } else {
            Err(self.lifecycle)
        }
    }
}

/// The runtime's self-reported view, published on every transition.
#[derive(Clone, Copy, Debug)]
pub struct RuntimeView {
    pub lifecycle: RuntimeLifecycle,
    pub generation: u64,
}

// ---------------------------------------------------------------------------
// The runtime itself
// ---------------------------------------------------------------------------

/// The session's lifecycle owner, shared by Copy handles.
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
    /// One per session, its generation seeded from the session ordinal
    /// (§21).
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

    /// The lifecycle as reported; a disposed owner reads `Disposed`.
    pub fn lifecycle(&self) -> RuntimeLifecycle {
        self.core
            .try_get_untracked()
            .map(|core| core.lifecycle)
            .unwrap_or(RuntimeLifecycle::Disposed)
    }

    /// The stamp tails capture to reject stale results; `try_`, and
    /// `u64::MAX` once gone.
    pub fn generation(&self) -> u64 {
        self.core
            .try_get_untracked()
            .map(|core| core.generation)
            .unwrap_or(u64::MAX)
    }

    /// Write to the core and republish; a disposed signal is dropped,
    /// never fatal.
    fn set(&self, f: impl FnOnce(&mut RuntimeCore)) {
        let _ = self.core.try_update(|core| {
            f(core);
            crate::diagnostics::publish_runtime_view(core.lifecycle, core.generation);
        });
    }

    /// Start (or restart, as a NEW generation) the session mount.
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

    /// Enter `Disposing`, await the panes' tails, mark `Disposed`.
    /// Later calls are no-ops; never revives.
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
            // Published outside the arena, where the signals are gone.
            crate::diagnostics::publish_runtime_view(RuntimeLifecycle::Disposed, generation);
            // The digest leaves BEFORE the completion, which may remove the
            // frame.
            crate::diagnostics::publish_digest(&api);
            // The tail resolves the promise the manager awaits before
            // removing the frame (§5).
            crate::resolve_dispose();
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// New → Disposing → Disposed, once (dispose is legal from any
    /// live state).
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

    /// The slot restarts as a new generation; the old is never reused.
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

    /// A ready runtime refuses a second mount; mounting re-enters;
    /// disposing restarts.
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

    /// Only the ordered tail's marker completes a dispose.
    #[test]
    fn finish_requires_disposing() {
        let mut core = RuntimeCore::default();
        assert!(core.finish_dispose(core.generation).is_err());
        core.begin_mount().expect("mount");
        assert!(core.finish_dispose(core.generation).is_err());
        core.begin_dispose();
        assert!(core.finish_dispose(core.generation).is_ok());
    }

    /// The stale tail's completion no-ops; the fresh generation stands.
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
