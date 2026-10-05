//! The AI's appearance settings: the gloss highlight palette (with the
//! native colour input on its Custom swatch), the highlight's opacity and the
//! word card's density.
//!
//! These are the AI feature's knobs, so they live with the feature and the
//! settings modal mounts them: `settings::theme::ThemeTab` composes
//! [`AiAppearanceSection`] the way it composes any other section. What is NOT
//! here is the reader's own colour — tint, textures and theme presets belong to
//! the title bar's palette menu, and the tab says so at its foot.

use leptos::prelude::*;

use reader_core::settings::{GlossColor, GlossDensity};

use crate::components::settings::common::StyleSelect;
use app_chrome::icon::IconName;
use app_chrome::icon_button::IconButton;
use app_ui::components::primitives::form::row::Row;
use app_ui::components::primitives::menu::section_label::SectionLabel;

#[component]
pub(crate) fn AiAppearanceSection(state: crate::context::ReaderContext) -> impl IntoView {
    let s = state.settings;

    view! {
        <SectionLabel text="AI Appearance" />
        <div class="rounded-xl border border-line">
            <div class="grid grid-cols-6 gap-2 px-4 py-4">
                {GlossColor::all()
                    .iter()
                    .copied()
                    .map(|c| {
                        let active = Signal::derive(move || s.with(|st| st.gloss_color) == c);
                        if c == GlossColor::Custom {
                            let ring = move || {
                                let base = "flex h-8 w-8 items-center justify-center rounded-full p-[3px]";
                                if active.get() {
                                    format!("{base} ring-2 ring-accent ring-offset-2 ring-offset-surface")
                                } else {
                                    base.to_string()
                                }
                            };
                            // The native picker, the control the split-pane
                            // outline row already uses: a transparent
                            // `<input type="color">` over the swatch, so the
                            // OS's own chooser opens on click.
                            view! {
                                <label
                                    title="Custom highlighter colour"
                                    aria-label="Custom highlighter colour"
                                    data-gloss-color="custom"
                                    class="relative flex cursor-pointer flex-col items-center gap-1.5 rounded-lg py-1 \
                                           focus-within:outline-none focus-within:ring-2 focus-within:ring-accent"
                                >
                                    <span
                                        class=ring
                                        style="background:conic-gradient(from 90deg,#e56b64,#e8c449,#6fd58c,#6ba3f5,#a58af0,#e56b64)"
                                    >
                                        <span
                                            class="h-full w-full rounded-full border border-line"
                                            style=move || {
                                                format!(
                                                    "background-color:{}",
                                                    s.with(|st| st.gloss_custom.clone())
                                                )
                                            }
                                        ></span>
                                    </span>
                                    <span class="text-xs text-muted">"Custom"</span>
                                    <input
                                        type="color"
                                        aria-label="Choose custom highlighter colour"
                                        data-gloss-color-input="custom"
                                        prop:value=move || s.with(|st| st.gloss_custom.clone())
                                        on:click=move |_| s.update(|st| st.gloss_color = GlossColor::Custom)
                                        on:input=move |ev| {
                                            let hex = event_target_value(&ev);
                                            s.update(|st| {
                                                st.gloss_color = GlossColor::Custom;
                                                st.gloss_custom = hex;
                                            });
                                        }
                                        class="absolute left-1/2 top-1 h-8 w-8 -translate-x-1/2 cursor-pointer opacity-0"
                                    />
                                </label>
                            }
                            .into_any()
                        } else {
                            let bg = move || {
                                c.resolve(&s.with(|st| st.gloss_custom.clone()))
                                    .unwrap_or_else(|| s.with(|st| st.appearance.accent_hex()))
                            };
                            let swatch = move || {
                                let base = "h-8 w-8 rounded-full border-2 border-line";
                                if active.get() {
                                    format!("{base} ring-2 ring-accent ring-offset-2 ring-offset-surface")
                                } else {
                                    base.to_string()
                                }
                            };
                            view! {
                                <button
                                    type="button"
                                    title=c.label()
                                    aria-pressed=move || active.get().to_string()
                                    on:click=move |_| s.update(|st| st.gloss_color = c)
                                    class="flex flex-col items-center gap-1.5 rounded-lg py-1 focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                                >
                                    <span class=swatch style=move || format!("background-color:{}", bg())></span>
                                    <span class="text-xs text-muted">{c.label()}</span>
                                </button>
                            }
                            .into_any()
                        }
                    })
                    .collect_view()}
            </div>
            <div class="border-t border-line">
                <Row label="Opacity">
                    <span class="flex items-center gap-3">
                        <span class="text-sm tabular-nums text-ink">
                            {move || format!("{:.1}", s.with(|st| st.gloss_opacity))}
                        </span>
                        <span class="flex gap-1.5">
                            <IconButton
                                icon=IconName::Minus
                                size=14
                                title="Less opaque"
                                class="rounded-full bg-line/60 hover:bg-line".to_string()
                                on_click=move || {
                                    s.update(|st| {
                                        st.gloss_opacity = (st.gloss_opacity - 0.1).clamp(0.1, 1.0);
                                    })
                                }
                            />
                            <IconButton
                                icon=IconName::Plus
                                size=14
                                title="More opaque"
                                class="rounded-full bg-line/60 hover:bg-line".to_string()
                                on_click=move || {
                                    s.update(|st| {
                                        st.gloss_opacity = (st.gloss_opacity + 0.1).clamp(0.1, 1.0);
                                    })
                                }
                            />
                        </span>
                    </span>
                </Row>
                <Row label="Card Density">
                    <StyleSelect
                        value=Signal::derive(move || s.with(|st| st.gloss_density))
                        on_change=Callback::new(move |v| {
                            s.update(|st| st.gloss_density = v);
                        })
                        options=vec![
                            (GlossDensity::Compact, "Compact"),
                            (GlossDensity::Comfortable, "Comfortable"),
                        ]
                        label_of=|v: &GlossDensity| v.label()
                        disabled=Signal::derive(move || false)
                    />
                </Row>
            </div>
        </div>
    }
}

// only the changed file was rewritten
