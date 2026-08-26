//! The reader titlebar's collision-aware entries: view mode, fit, zoom,
//! appearance. The route composes the toolbar; this module owns the item
//! definitions so `page.rs` stays a coordinator, not a toolbar factory.

use leptos::html;
use leptos::prelude::*;

use pdf_core::layout::ViewMode;
use pdf_core::math::FitMode;
use crate::components::app_shell::adaptive_toolbar::ToolbarItem;
use crate::components::menus::appearance_menu::appearance_entry;
use crate::components::viewer_controls::zoom_controls::zoom_entries;
use crate::components::primitives::button::{Button, ButtonVariant};
use crate::components::primitives::icon::{Icon, IconName};
use crate::components::primitives::segmented::{SegmentOption, Segmented, SegmentedLabel};
use crate::components::primitives::tooltip::Tooltip;
use crate::state::AppState;

fn view_mode_entry(state: AppState) -> ToolbarItem {
    let mode = state.reader.viewer.mode;

    ToolbarItem::pair(
        "view-mode",
        75,
        move || {
            view! {
                <Tooltip text="View mode">
                    <Segmented
                        options=vec![
                            SegmentOption {
                                value: ViewMode::Single,
                                label: SegmentedLabel::Icon(IconName::SinglePage),
                                title: "Single page view",
                            },
                            SegmentOption {
                                value: ViewMode::Continuous,
                                label: SegmentedLabel::Icon(IconName::Continuous),
                                title: "Continuous scroll view",
                            },
                        ]
                        value={mode.read_only()}
                        on_change=move |m: ViewMode| state.reader.viewer.mode.set(m)
                    />
                </Tooltip>
            }
            .into_any()
        },
        move |done| {
            view! {
                <div class="w-full px-1 py-1">
                    <Segmented
                        full_width=true
                        options=vec![
                            SegmentOption {
                                value: ViewMode::Single,
                                label: SegmentedLabel::IconText(IconName::SinglePage, "Single"),
                                title: "Single page view",
                            },
                            SegmentOption {
                                value: ViewMode::Continuous,
                                label: SegmentedLabel::IconText(IconName::Continuous, "Continuous"),
                                title: "Continuous scroll view",
                            },
                        ]
                        value={mode.read_only()}
                        on_change=move |m: ViewMode| {
                            state.reader.viewer.mode.set(m);
                            done.run(());
                        }
                    />
                </div>
            }
            .into_any()
        },
    )
}

fn fit_entry(state: AppState) -> ToolbarItem {
    ToolbarItem::pair(
        "fit",
        70,
        move || {
            view! {
                <div class="flex items-center gap-1">
                    <Tooltip text="Fit width (Cmd/Ctrl+0)">
                        <Button
                            on_click=move |_| state.reader.viewer.fit.set(FitMode::Width)
                            variant=ButtonVariant::Ghost
                            title="Fit width (Cmd/Ctrl+0)"
                        >
                            <Icon name=IconName::FitWidth size=18 />
                        </Button>
                    </Tooltip>
                    <Tooltip text="Fit page">
                        <Button
                            on_click=move |_| state.reader.viewer.fit.set(FitMode::Page)
                            variant=ButtonVariant::Ghost
                            title="Fit page"
                        >
                            <Icon name=IconName::FitPage size=18 />
                        </Button>
                    </Tooltip>
                </div>
            }
            .into_any()
        },
        move |done| {
            view! {
                <div class="flex w-full items-center gap-1 px-1 py-1">
                    <button type="button"
                        on:click=move |_| {
                            state.reader.viewer.fit.set(FitMode::Width);
                            done.run(());
                        }
                        class="inline-flex h-9 min-w-0 flex-1 items-center justify-center gap-1.5 rounded-md border border-line bg-surface px-2 text-sm text-ink hover:bg-line"
                    >
                        <Icon name=IconName::FitWidth size=14 />
                        <span>"Fit width"</span>
                    </button>
                    <button type="button"
                        on:click=move |_| {
                            state.reader.viewer.fit.set(FitMode::Page);
                            done.run(());
                        }
                        class="inline-flex h-9 min-w-0 flex-1 items-center justify-center gap-1.5 rounded-md border border-line bg-surface px-2 text-sm text-ink hover:bg-line"
                    >
                        <Icon name=IconName::FitPage size=14 />
                        <span>"Fit page"</span>
                    </button>
                </div>
            }
            .into_any()
        },
    )
}

/// The reader bar's entries, in collapse-priority order: view mode, fit,
/// zoom step + readout, appearance (essential).
pub fn reader_toolbar_entries(
    state: AppState,
    appearance_open: RwSignal<bool>,
    collapsed_ids: RwSignal<Vec<&'static str>>,
    overflow_ref: NodeRef<html::Div>,
) -> Vec<ToolbarItem> {
    let mut entries = vec![view_mode_entry(state), fit_entry(state)];
    entries.extend(zoom_entries(state));          // zoom-step (80), readout (MAX)
    entries.push(appearance_entry(
        state,
        appearance_open,
        collapsed_ids,
        overflow_ref,
    ));                                          // MAX
    entries
}
