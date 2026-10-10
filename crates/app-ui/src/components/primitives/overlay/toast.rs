//! Toast data model and visual; the auto-dismiss controller is separate.

use std::time::Duration;

use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};

/// Visual tone of a toast: the global error, the gloss undo, a plain note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToastTone {
    #[default]
    Error,
    /// The "removed X — Undo" style: neutral with an accent action.
    Undo,
    /// A neutral note: neutral surface, check mark, no action.
    Info,
}

/// An optional action on a toast (Undo, Open, Save…).
#[derive(Debug, Clone)]
pub struct ToastAction {
    pub label: String,
    pub on_click: Callback<()>,
}

/// Equality by label only; the callback is excluded.
impl PartialEq for ToastAction {
    fn eq(&self, other: &Self) -> bool {
        self.label == other.label
    }
}

/// One toast; `id` is monotonic so a stale timer cannot wipe a newer one.
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
        ToastTone::Undo => ("border-line bg-surface text-ink", IconName::Undo),
        ToastTone::Info => ("border-line bg-surface text-ink", IconName::Check),
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
