//! The import dock: a card per run, clear of the shelf's centre.

use std::time::Duration;

use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};
use app_chrome::layers::TOAST;

use crate::services::dismiss_task;
use crate::state::library::TaskPhase;

const HOLD_MS: u64 = 1600;

/// The ring's radius: the SVG and `dock.css` derive it, and CI checks both.
const RING_RADIUS: f64 = 15.0;
const CIRCUMFERENCE: f64 = 2.0 * std::f64::consts::PI * RING_RADIUS;

/// `inner_html`, not `view!`: one set of circles, a property the browser
/// interpolates.
const RING: &str = "<svg viewBox='0 0 36 36' width='36' height='36' aria-hidden='true'>\
<circle class='import-ring-track' cx='18' cy='18' r='15'/>\
<circle class='import-ring-fill' cx='18' cy='18' r='15'/>\
</svg>";

#[component]
pub(crate) fn ProgressDock(state: crate::context::LibraryContext) -> impl IntoView {
    view! {
        <div class=format!("import-dock pointer-events-none {TOAST}")>
            <For
                each=move || state.library.tasks.get()
                key=|task| task.id.clone()
                let:task
            >
                <DockCard state=state id=task.id />
            </For>
        </div>
    }
}

/// Reads the task back by id: `For` keys on it, so a count still reaches.
#[component]
fn DockCard(state: crate::context::LibraryContext, id: String) -> impl IntoView {
    let timer_id = id.clone();
    let close_id = id.clone();
    let task = Signal::derive(move || {
        state
            .library
            .tasks
            .with(|tasks| tasks.iter().find(|t| t.id == id).cloned())
    });

    Effect::new(move |_| {
        let Some(current) = task.get() else {
            return;
        };
        if !current.phase.is_finished() {
            return;
        }
        let finished_id = timer_id.clone();
        let handle = set_timeout_with_handle(
            move || {
                // The hold can outlive the dock: a disposed library
                // has no tasks.
                if state.library.tasks.try_get_untracked().is_none() {
                    return;
                }
                dismiss_task(state, &finished_id);
            },
            Duration::from_millis(HOLD_MS),
        )
        .ok();
        on_cleanup(move || {
            if let Some(handle) = handle {
                handle.clear();
            }
        });
    });

    view! {
        <div
            class="import-card surface-toast pointer-events-auto"
            class=("import-card-failed", move || {
                task.get()
                    .is_some_and(|t| t.phase == TaskPhase::Failed)
            })
        >
            <span class="import-ring-wrap">
                <span
                    class=move || {
                        // The ring spins without a fraction: a scan has no
                        // total.
                        let base = "import-ring";
                        let Some(current) = task.get() else {
                            return base.to_string();
                        };
                        if current.phase == TaskPhase::Scanning {
                            format!("{base} import-ring-spin")
                        } else {
                            base.to_string()
                        }
                    }
                    style=move || {
                        let offset = task
                            .get()
                            .and_then(|t| t.fraction())
                            .map(|f| CIRCUMFERENCE * (1.0 - f))
                            .unwrap_or(CIRCUMFERENCE);
                        format!("--ring-offset:{offset:.2}")
                    }
                    role="progressbar"
                    aria-label="Import progress"
                    aria-valuemin="0"
                    aria-valuemax="100"
                    aria-valuenow=move || {
                        task.get()
                            .and_then(|t| t.percent())
                            .map(|p| p.to_string())
                            .unwrap_or_default()
                    }
                    inner_html=RING
                ></span>
                <span class="import-ring-value">
                    {move || {
                        let current = task.get()?;
                        match current.phase {
                            TaskPhase::Done => {
                                Some(view! { <Icon name=IconName::Check size=15 /> }.into_any())
                            }
                            TaskPhase::Failed => {
                                Some(view! { <Icon name=IconName::Close size=15 /> }.into_any())
                            }
                            _ => current.percent().map(|percent| {
                                view! { <span class="import-ring-percent">{percent}</span> }
                                    .into_any()
                            }),
                        }
                    }}
                </span>
            </span>

            <span class="min-w-0 flex-1">
                <span class="block truncate text-xs font-medium text-ink">
                    {move || {
                        task.get()
                            .map(|t| t.headline())
                            .unwrap_or_else(|| "Importing…".to_string())
                    }}
                </span>
                <span class="block max-w-40 truncate text-[11px] text-muted">
                    {move || {
                        let Some(current) = task.get() else {
                            return String::new();
                        };
                        match current.phase {
                            TaskPhase::Failed => current
                                .error
                                .clone()
                                .unwrap_or_else(|| current.label.clone()),
                            TaskPhase::Done => current.label.clone(),
                            _ => {
                                if current.name.is_empty() {
                                    current.label.clone()
                                } else {
                                    current.name.clone()
                                }
                            }
                        }
                    }}
                </span>
            </span>

            // Not offered while running: a dismiss that no-ops is a lie.
            <Show when=move || task.get().is_some_and(|t| t.phase.is_finished())>
                <button
                    class="icon-ghost import-card-close"
                    type="button"
                    title="Dismiss"
                    aria-label="Dismiss this import"
                    on:click={
                        // The children closure re-runs, so the
                        // handler owns a copy.
                        let id = close_id.clone();
                        move |_| {
                            dismiss_task(state, &id);
                        }
                    }
                >
                    <Icon name=IconName::Close size=11 />
                </button>
            </Show>
        </div>
    }
}
