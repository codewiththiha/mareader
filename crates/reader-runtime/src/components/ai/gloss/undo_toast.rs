//! The "Removed n highlights — Undo" toast on the shared shell.

use leptos::prelude::*;

use crate::components::ai::gloss::controller::GlossController;
use crate::components::ai::gloss::selection_mode::{UNDO_WINDOW_MS, UndoBatch};
use app_ui::components::primitives::overlay::toast::{
    ToastAction, ToastData, ToastPanel, ToastTone,
};
use app_ui::components::primitives::overlay::toast_host::use_toast_slot;

#[component]
pub fn GlossUndoToast(
    state: crate::context::ReaderContext,
    ctrl: GlossController,
    undo: RwSignal<Option<UndoBatch>>,
) -> impl IntoView {
    // The toast for the batch, built once.
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
                            // A batch belongs to its document; else drop it.
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
        move |id| undo.with_untracked(|u| u.as_ref().is_some_and(|batch| batch.generation == id)),
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
                <div
                    class="gloss-undo-toast toast-anchor z-[var(--z-toast)]"
                    role="status"
                >
                    <ToastPanel toast=toast />
                </div>
            })
        }}
    }
}
