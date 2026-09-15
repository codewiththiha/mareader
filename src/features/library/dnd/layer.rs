//! The drag overlay: one fixed layer, no pointer events of its own, drawn from the controller
//! and nothing else.
//!
//! A browser drag image cannot be any of the four things this is: it is one bitmap of the
//! element the press began on, made before the drag starts, and composited by the engine.

use leptos::portal::Portal;
use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};

use crate::features::library::dnd::controller::{DragController, GhostTile};
use crate::features::library::folder_card::THUMB_CAP;

/// Named after the folder's own plate cap rather than spelled a second time beside it: a promise drawn with a different number of cells than the card it becomes is a promise about a folder the library does not have.


#[component]
pub(crate) fn DragLayer() -> impl IntoView {
    let ctrl = use_context::<DragController>().expect("the library page installs the drag session");
    let live = ctrl.live();
    let fold = ctrl.fold();
    let ghost = ctrl.ghost();
    let count = ctrl.count();
    let at = ctrl.pointer();
    let sunk = ctrl.sink();
    let several = Signal::derive(move || count.get() > 1);
    // One signal for the whole sunk state — the anchor, the scale AND the transition — because they are one fact: a separate "is animating" flag could disagree on exactly the frame that matters.
    let is_sunk = Signal::derive(move || sunk.get().is_some());
    let style = Signal::derive(move || {
        match sunk.get() {
            Some(spot) => format!("left:{:.2}px;top:{:.2}px", spot.x, spot.y),
            None => {
                let (x, y) = at.get();
                format!("left:{x:.2}px;top:{y:.2}px")
            }
        }
    });

    view! {
        <Portal>
            <Show when=move || live.get() fallback=|| ()>
                <div
                    class="lib-drag-layer"
                    class=("lib-drag-sunk", move || is_sunk.get())
                    style=move || style.get()
                    aria-hidden="true"
                >
                    <div class="lib-drag-ghost">
                        {move || {
                            if let Some(preview) = fold.get() {
                                return view! { <FoldPlate filled=preview.filled /> }.into_any();
                            }
                            let tiles = ghost.get();
                            let total = count.get();
                            view! {
                                {tiles
                                    .into_iter()
                                    .take(THUMB_CAP)
                                    .enumerate()
                                    .map(|(fan, tile)| view! { <GhostCard fan=fan tile=tile /> })
                                    .collect_view()}
                                <Show when=move || several.get() fallback=|| ()>
                                    <span class="lib-drag-count">{total}</span>
                                </Show>
                            }
                                .into_any()
                        }}
                    </div>
                </div>
            </Show>
        </Portal>
    }
}

#[component]
fn GhostCard(fan: usize, tile: GhostTile) -> impl IntoView {
    let GhostTile { cover, label, folder } = tile;
    let letter = initial(&label);
    let face = match cover {
        Some(cover) => view! { <img class="lib-drag-img" src=cover alt="" loading="lazy" /> }
            .into_any(),
        None if folder => {
            view! { <span class="lib-drag-folder"><Icon name=IconName::Open size=16 /></span> }
                .into_any()
        }
        None => view! { <span class="lib-drag-letter">{letter}</span> }.into_any(),
    };
    view! {
        <span class="lib-drag-tile" style=format!("--fan:{fan}") title=label>
            {face}
        </span>
    }
}

/// The folder card's classes rather than a look of its own: the preview is a promise about what the card on this level will look like in a moment.
#[component]
fn FoldPlate(filled: usize) -> impl IntoView {
    view! {
        <div class="lib-drag-fold">
            <div class="folder-thumb-grid">
                {(0..THUMB_CAP)
                    .map(|at| {
                        let next = at == filled;
                        let class = if at < filled {
                            "folder-thumb-cell folder-thumb-fill"
                        } else if next {
                            "folder-thumb-cell folder-thumb-next"
                        } else {
                            "folder-thumb-cell folder-thumb-empty"
                        };
                        view! {
                            <span class=class>
                                {next.then(|| view! { <Icon name=IconName::Plus size=14 /> })}
                            </span>
                        }
                    })
                    .collect_view()}
            </div>
            <span class="lib-drag-fold-label">"New shelf"</span>
        </div>
    }
}

fn initial(label: &str) -> String {
    label
        .chars()
        .next()
        .map(|letter| letter.to_uppercase().to_string())
        .unwrap_or_default()
}
