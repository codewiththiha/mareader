//! The "Removed n highlights — Undo" toast, composed on the unified toast
//! shell ([`ToastData`] + [`ToastPanel`]). Parking through
//! [`super::selection_mode::park_undo`] means EVERY removal path (context menu,
//! bar) gets undo for free; the batch is pinned to its document path so an
//! undo after a document switch drops instead of resurrecting marks into the
//! wrong file.
//!
//! Auto-dismiss rides the host's id-guarded `use_toast_slot` (the toast id
//! IS the batch generation): a second removal replaces the batch, and the
//! first one's timer can never clear it. Position stays domain policy — this
//! toast sits at the bottom center, above the selection bar's corner.

use leptos::prelude::*;

use crate::components::ai::gloss::controller::GlossController;
use crate::components::ai::gloss::selection_mode::{UndoBatch, UNDO_WINDOW_MS};
use crate::components::primitives::overlay::toast::{ToastAction, ToastData, ToastPanel, ToastTone};
use crate::components::primitives::overlay::toast_host::use_toast_slot;
use crate::state::AppState;

#[component]
pub fn GlossUndoToast(
    state: AppState,
    ctrl: GlossController,
    undo: RwSignal<Option<UndoBatch>>,
) -> impl IntoView {
    // The toast the batch projects to, built ONCE per batch: id = generation
    // (the host's equality guard sees a replacement as a different toast).
    // The same value feeds the auto-dismiss slot and the render, so they can
    // never disagree about the message or the action.
    let toast = Memo::new(move |_| {
        undo.with(|u| {
            u.as_ref().map(|batch| {
                let n = batch.marks.len();
                let restored = batch.marks.clone();
                let batch_path = batch.path.clone();
                ToastData {
                    id: batch.generation,
                    message: format!("Removed {n} highlight{}", if n == 1 { "" } else { "s" }),
                    tone: ToastTone::Undo,
                    duration: Some(std::time::Duration::from_millis(UNDO_WINDOW_MS as u64)),
                    action: Some(ToastAction {
                        label: "Undo".into(),
                        on_click: Callback::new(move |_| {
                            // A batch belongs to the document it came from;
                            // after a switch, drop it instead of resurrecting
                            // marks into the wrong file.
                            if state.reader.document.path.get_untracked() == batch_path {
                                ctrl.commands.restore_marks.run(restored.clone());
                            }
                            undo.set(None);
                        }),
                    }),
                }
            })
        })
    });

    use_toast_slot(
        Signal::derive(move || toast.get()),
        move |id| {
            undo.with_untracked(|u| {
                u.as_ref()
                    .is_some_and(|batch| batch.generation == id)
            })
        },
        move |id| {
            undo.update(|u| {
                if u.as_ref().is_some_and(|batch| batch.generation == id) {
                    *u = None;
                }
            });
        },
    );

    view! {
        {move || {
            toast.get().map(|toast| view! {
                // Bottom-center, clear of the selection bar's corner.
                <div
                    class="gloss-undo-toast fixed bottom-5 left-1/2 z-[var(--z-toast)] \
                           -translate-x-1/2"
                    role="status"
                >
                    <ToastPanel toast=toast />
                </div>
            })
        }}
    }
}
