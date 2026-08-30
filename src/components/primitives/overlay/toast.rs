//! Toast data model + visual. The gloss undo toast and the app-global error
//! toast used to be two separate shells with two separate timers; both are
//! now `ToastData` through [`toast_host`](super::toast_host).
//!
//! Data is deliberately separate from the auto-dismiss controller: the gloss
//! undo keeps its own *generation-guarded* timer (undo semantics — a steal
//! must not clear a newer batch), while ordinary toasts use the host's timer.

use std::time::Duration;

use leptos::prelude::*;

use crate::components::primitives::icon::{Icon, IconName};

/// Visual tone of a toast.
///
/// `Error` is the app-global failure toast (open-flow, toolbar); `Undo` is
/// the gloss "Removed n highlights — Undo" style (neutral with an accent
/// action). That is the whole current surface; a new tone lands with its
/// producer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToastTone {
    #[default]
    Error,
    /// The "removed X — Undo" style: neutral with an accent action.
    Undo,
}

/// An optional action on a toast (Undo, Open, Save…).
#[derive(Debug, Clone)]
pub struct ToastAction {
    pub label: String,
    pub on_click: Callback<()>,
}

/// Equality by label only: the callback is deliberately excluded (it is a
/// new closure per batch). This exists so a `Memo<Option<ToastData>>` can
/// dedupe identical batches without the callback identity entering the
/// comparison.
impl PartialEq for ToastAction {
    fn eq(&self, other: &Self) -> bool {
        self.label == other.label
    }
}

/// One toast. `id` is monotonic per producer so a stale timer can never wipe
/// a newer toast (the host's equality guard). `PartialEq` (via
/// [`ToastAction`]'s label-only comparison) lets a memo dedupe identical
/// batches.
#[derive(Debug, Clone, PartialEq)]
pub struct ToastData {
    pub id: u64,
    pub message: String,
    pub tone: ToastTone,
    /// How long the toast stays up. `None` = no auto-dismiss.
    pub duration: Option<Duration>,
    pub action: Option<ToastAction>,
}

impl ToastData {
    pub fn new(id: u64, message: impl Into<String>, tone: ToastTone) -> Self {
        Self {
            id,
            message: message.into(),
            tone,
            duration: Some(Duration::from_millis(3500)),
            action: None,
        }
    }
}

fn tone_classes(tone: ToastTone) -> (&'static str, IconName) {
    match tone {
        ToastTone::Error => (
            "border-red-400/50 bg-red-950/95 text-red-100",
            IconName::Close,
        ),
        ToastTone::Undo => (
            "border-line bg-surface text-ink",
            IconName::Undo,
        ),
    }
}

/// The toast visual: message + optional action, no positioning, no timing.
#[component]
pub fn ToastPanel(toast: ToastData) -> impl IntoView {
    let (tone_class, icon) = tone_classes(toast.tone);
    let action = toast.action;
    view! {
        <div
            class=format!(
                "surface-toast flex max-w-[min(90vw,32rem)] items-center gap-2 rounded-xl border px-4 py-2.5 text-sm shadow-xl {tone_class}"
            )
            role="status"
        >
            <Icon name=icon size=16 />
            <span>{toast.message}</span>
            {action.map(|a| {
                let label = a.label;
                let on_click = a.on_click;
                view! {
                    <button
                        type="button"
                        on:click=move |_| on_click.run(())
                        class="rounded-full px-3 py-1 text-sm font-semibold text-accent \
                               transition-colors hover:bg-line \
                               focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                    >
                        {label}
                    </button>
                }
            })}
        </div>
    }
}
