use std::sync::{Mutex, PoisonError};

use egui::Context;
use mareader_engine::host::Machine;
use mareader_engine::session::Waker;

use super::*;

impl MareaderApp {
    /// Reopens what was read at exit, if its file is still there.
    pub fn resume(&mut self) -> bool {
        if self.ui.resumed {
            return false;
        }
        let Some(request) = self.session.resume_request() else {
            return false;
        };
        self.ui.resumed = true;
        self.queue(Sent::Engine(Command::Open(request)));
        true
    }

    /// Wires every controller this crate still holds to `machine`'s places.
    ///
    /// The host and the waker are the session's from its construction, and
    /// each service takes the same waker, so work off this thread can ask for
    /// a frame. A place left unnamed attaches nothing, and its controller
    /// keeps its state in memory.
    pub(super) fn wire(&mut self, ctx: &Context, machine: &Machine) {
        self.session.set_repaint(crate::repaint::of(ctx));
        let base = Machine::under(self.session.host().as_ref());
        if let Some(root) = machine.library.clone()
            && machine.library != base.library
        {
            self.session.wire_library(root);
        }
        if let Some(dir) = machine.cache.clone()
            && machine.cache != base.cache
        {
            self.session.wire_library_cache(dir);
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.wire_services(ctx, machine, &base);
        self.drain(ctx);
    }

    /// The services only a desktop has: words, voices and the AI.
    #[cfg(not(target_arch = "wasm32"))]
    fn wire_services(&mut self, ctx: &Context, machine: &Machine, base: &Machine) {
        if let Some(dir) = machine.dictionaries.clone()
            && machine.dictionaries != base.dictionaries
            && let Err(why) = self.session.wire_dict(dir)
        {
            self.refused("Dictionaries are off", why);
        }
        if let Some(dir) = machine.vocabulary.clone()
            && machine.vocabulary != base.vocabulary
        {
            self.session.wire_vocab(dir);
        }
        if let Some(dir) = machine.voices.clone()
            && (machine.voices != base.voices || machine.offered.is_some())
        {
            let offered = machine.offered.unwrap_or(mareader_speech::MODELS);
            if let Err(why) = self.session.wire_voices(dir, offered) {
                self.refused("Voice models are off", why);
            }
        }
        if let Some(root) = machine.ai.clone() {
            self.ui.ai.attach(ctx, root, machine.ai_helper.clone());
            // A run stopped before quitting waits on its card.
            self.session.library_set_paused(self.ui.ai.paused());
        }
    }

    /// A service that refused to start says so, through the session's events.
    #[cfg(not(target_arch = "wasm32"))]
    fn refused(&mut self, what: &str, why: String) {
        self.session.report(Toast::info(format!("{what}: {why}")));
    }
}

/// Target for the session's waker, rebound once `eframe` supplies its context.
#[derive(Clone)]
pub(super) struct WakeSlot(Arc<Mutex<Context>>);

impl WakeSlot {
    pub(super) fn new(ctx: &Context) -> (Self, Waker) {
        let slot = Self(Arc::new(Mutex::new(ctx.clone())));
        let shared = Arc::clone(&slot.0);
        let wake: Waker = Arc::new(move || {
            let ctx = {
                let held = shared.lock().unwrap_or_else(PoisonError::into_inner);
                held.clone()
            };
            ctx.request_repaint();
        });
        (slot, wake)
    }

    pub(super) fn bind(&self, ctx: &Context) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = ctx.clone();
    }
}
