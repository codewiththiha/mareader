//! Split-only appearance controls for pane boundaries. Shared by the
//! Appearance popover and Settings → Workspace so both entry points edit the
//! same persisted workspace fields and paint through the same host tokens.

use leptos::prelude::*;

use reader_core::settings::{PaneCorners, PaneOutlineColor, Settings};

use crate::components::primitives::controls::switch::Switch;
use crate::components::primitives::form::row::Row;
use crate::components::primitives::form::slider::Slider;
use crate::components::primitives::menu::section_label::SectionLabel;

const OUTLINE_COLORS: [(PaneOutlineColor, &str, &str); 5] = [
    (PaneOutlineColor::Auto, "Auto", "var(--color-accent)"),
    (PaneOutlineColor::Red, "Red", "#e56b64"),
    (PaneOutlineColor::Yellow, "Yellow", "#e8c449"),
    (PaneOutlineColor::Green, "Green", "#6fd58c"),
    (PaneOutlineColor::Blue, "Blue", "#6ba3f5"),
];

#[component]
pub fn PaneSection(settings: RwSignal<Settings>, visible: Signal<bool>) -> impl IntoView {
    view! {
        <Show when=move || visible.get()>
            <SectionLabel text="Split pane appearance" />
            <div class="divide-y divide-line rounded-xl border border-line" data-setting="split-pane-appearance">
                <Row label="Outline thickness">
                    <span class="flex w-40 flex-col gap-1" data-setting="pane-outline-width">
                        <Slider
                            value=Signal::derive(move || settings.with(|s| s.workspace.pane_outline_width as f64))
                            min=0.0
                            max=8.0
                            step=1.0
                            on_change=move |v| {
                                settings.update(|s| s.workspace.pane_outline_width = v.round() as u8);
                            }
                        />
                        <span class="text-right text-xs tabular-nums text-muted">
                            {move || format!("{} px", settings.with(|s| s.workspace.pane_outline_width))}
                        </span>
                    </span>
                </Row>
                <div class="flex flex-col gap-3 px-4 py-3.5">
                    <span class="text-sm text-ink">"Outline colour"</span>
                    <div class="flex flex-wrap items-center gap-3" role="group" aria-label="Pane outline colour">
                        {OUTLINE_COLORS
                            .iter()
                            .map(|(color, label, background)| {
                                let color = *color;
                                let bg = *background;
                                let label = *label;
                                let active = move || {
                                    settings.with(|s| s.workspace.pane_outline_color == color)
                                };
                                view! {
                                    <button
                                        type="button"
                                        title=label
                                        aria-label=label
                                        aria-pressed=move || active().to_string()
                                        data-pane-outline-color=format!("{:?}", color).to_lowercase()
                                        on:click=move |_| settings.update(|s| s.workspace.pane_outline_color = color)
                                        class=move || {
                                            let base = "h-8 w-8 rounded-full border border-line";
                                            if active() {
                                                format!("{base} ring-2 ring-accent ring-offset-2 ring-offset-surface")
                                            } else {
                                                base.to_string()
                                            }
                                        }
                                        style=format!("background:{bg}")
                                    />
                                }
                            })
                            .collect_view()}
                        <label
                            title="Custom outline colour"
                            class=move || {
                                let base = "relative flex h-8 w-8 items-center justify-center rounded-full border border-line";
                                if settings.with(|s| s.workspace.pane_outline_color == PaneOutlineColor::Custom) {
                                    format!("{base} ring-2 ring-accent ring-offset-2 ring-offset-surface")
                                } else {
                                    base.to_string()
                                }
                            }
                        >
                            <input
                                type="color"
                                aria-label="Custom outline colour"
                                data-pane-outline-color="custom"
                                prop:value=move || settings.with(|s| s.workspace.pane_outline_custom.clone())
                                on:input=move |ev| {
                                    let hex = event_target_value(&ev);
                                    settings.update(|s| {
                                        s.workspace.pane_outline_color = PaneOutlineColor::Custom;
                                        s.workspace.pane_outline_custom = hex;
                                    });
                                }
                                class="h-7 w-7 cursor-pointer rounded-full border-0 bg-transparent p-0"
                            />
                        </label>
                    </div>
                </div>
                <Row label="Space between panes">
                    <span class="flex w-40 flex-col gap-1" data-setting="pane-gap">
                        <Slider
                            value=Signal::derive(move || settings.with(|s| s.workspace.pane_gap as f64))
                            min=0.0
                            max=24.0
                            step=1.0
                            on_change=move |v| {
                                settings.update(|s| s.workspace.pane_gap = v.round() as u8);
                            }
                        />
                        <span class="text-right text-xs tabular-nums text-muted">
                            {move || format!("{} px", settings.with(|s| s.workspace.pane_gap))}
                        </span>
                    </span>
                </Row>
                <div data-setting="pane-shadow">
                    <Row label="Pane drop shadow">
                        <Switch
                            checked=Signal::derive(move || settings.with(|s| s.workspace.pane_shadow))
                            on_change=Callback::new(move |on| {
                                settings.update(|s| s.workspace.pane_shadow = on);
                            })
                            title="Pane drop shadow"
                        />
                    </Row>
                </div>
                <div class="flex items-center justify-between gap-3 px-4 py-3.5">
                    <span class="text-sm text-ink">"Corners"</span>
                    <div class="flex rounded-lg border border-line p-0.5" role="group" aria-label="Pane corners">
                        <CornerChoice settings=settings corner=PaneCorners::Square label="Square" />
                        <CornerChoice settings=settings corner=PaneCorners::Rounded label="Rounded" />
                    </div>
                </div>
            </div>
        </Show>
    }
}

#[component]
fn CornerChoice(
    settings: RwSignal<Settings>,
    corner: PaneCorners,
    label: &'static str,
) -> impl IntoView {
    let selected = move || settings.with(|s| s.workspace.pane_corners == corner);
    view! {
        <button
            type="button"
            aria-pressed=move || selected().to_string()
            data-pane-corners=format!("{:?}", corner).to_lowercase()
            on:click=move |_| settings.update(|s| s.workspace.pane_corners = corner)
            class=move || {
                if selected() {
                    "rounded-md bg-accent-soft px-3 py-1 text-xs font-medium text-accent"
                } else {
                    "rounded-md px-3 py-1 text-xs text-muted hover:text-ink"
                }
            }
        >
            {label}
        </button>
    }
}
