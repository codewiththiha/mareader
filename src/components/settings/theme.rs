//! The Theme tab of the reader settings modal: AI highlight palette (with a
//! custom colour picker), gloss opacity, the word card's density — and the
//! blend backdrop's paper settings, which are an appearance decision before
//! they are a layout one: they decide what colour the reader looks at.

use leptos::html;
use leptos::prelude::*;

use pdf_core::settings::{GlossColor, GlossDensity, PaperArea, PaperMode};
use pdf_paper::{MAX_SCAN_PAGES, MIN_SCAN_PAGES};

use crate::components::settings::common::{Row, StyleSelect};
use crate::components::primitives::form::slider::Slider;
use crate::components::primitives::icon::IconName;
use crate::components::primitives::icon_button::IconButton;
use crate::components::primitives::overlay::lanes::OverlayPolicy;
use crate::components::primitives::section_label::SectionLabel;
use crate::components::primitives::separator::Separator;
use crate::components::primitives::switch::Switch;
use crate::components::shell::titlebar::toolbar_popover::MenuPopover;
use crate::state::AppState;

#[component]
pub(crate) fn ThemeTab(state: AppState) -> impl IntoView {
    let s = state.settings;
    let custom_open = RwSignal::new(false);
    let custom_anchor: NodeRef<html::Div> = NodeRef::new();
    let blend_off = Signal::derive(move || !s.with(|st| st.layout.blend_mode));

    view! {
        <SectionLabel text="AI Appearance" />
        <div class="rounded-xl border border-line">
            <div class="grid grid-cols-6 gap-2 px-4 py-4">
                {GlossColor::all()
                    .into_iter()
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
                            view! {
                                <div node_ref=custom_anchor class="relative flex flex-col items-center">
                                    <button
                                        type="button"
                                        title="Custom…"
                                        aria-pressed=move || active.get().to_string()
                                        on:click=move |_| {
                                            s.update(|st| st.gloss_color = GlossColor::Custom);
                                            custom_open.set(true);
                                        }
                                        class="flex flex-col items-center gap-1.5 rounded-lg py-1 focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
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
                                    </button>
                                    <CustomColorPicker state=state open=custom_open anchor=custom_anchor />
                                </div>
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
        <div class="mt-5"><Separator vertical=false /></div>
        <SectionLabel text="Paper" />
        <div class="divide-y divide-line rounded-xl border border-line">
            <Row label="Blend Mode">
                <Switch
                    checked=Signal::derive(move || s.with(|st| st.layout.blend_mode))
                    on_change=Callback::new(move |v| {
                        s.update(|st| st.layout.blend_mode = v);
                    })
                    title="Paint the reader background with the page's own paper \
                           colour, through the same filter the pages use"
                        .to_string()
                />
            </Row>
            <Row label="Paper Mode">
                <StyleSelect
                    value=Signal::derive(move || s.with(|st| st.layout.blend_scope))
                    on_change=Callback::new(move |v| {
                        s.update(|st| st.layout.blend_scope = v);
                    })
                    options=vec![
                        (PaperMode::Fixed, "Fixed"),
                        (PaperMode::Continuous, "Continuous"),
                    ]
                    label_of=|v: &PaperMode| v.label()
                    disabled=blend_off
                />
            </Row>
            <Row label="Detection">
                <StyleSelect
                    value=Signal::derive(move || s.with(|st| st.layout.blend_area))
                    on_change=Callback::new(move |v| {
                        s.update(|st| st.layout.blend_area = v);
                    })
                    options=vec![
                        (PaperArea::WholePage, "Whole Page"),
                        (PaperArea::Edges, "Edges"),
                    ]
                    label_of=|v: &PaperArea| v.label()
                    disabled=blend_off
                />
            </Row>
            // The fixed scan's page budget: at most this many pages are
            // sampled for the book's one colour. Continuous mode needs no
            // budget — it detects each page as the reader reaches it, one
            // by one — so the stepper sleeps while it is on, and wakes with
            // the persisted value when the mode returns to Fixed.
            <Row label="Scan Pages">
                <span class="flex items-center gap-3">
                    <span class=move || {
                        if s.with(|st| {
                            st.layout.blend_mode && st.layout.blend_scope == PaperMode::Fixed
                        }) {
                            "w-14 text-right text-sm tabular-nums text-ink"
                        } else {
                            "w-14 text-right text-sm tabular-nums text-muted/60"
                        }
                    }>
                        {move || s.with(|st| st.layout.blend_scan_pages)}
                    </span>
                    <span class="flex gap-1.5">
                        <IconButton
                            icon=IconName::Minus
                            size=14
                            title="Scan fewer pages (Fixed mode)"
                            class="rounded-full bg-line/60 hover:bg-line".to_string()
                            disabled=Signal::derive(move || {
                                if blend_off.get() {
                                    return true;
                                }
                                s.with(|st| {
                                    st.layout.blend_scope == PaperMode::Continuous
                                        || st.layout.blend_scan_pages <= MIN_SCAN_PAGES
                                })
                            })
                            on_click=move || {
                                s.update(|st| {
                                    st.layout.blend_scan_pages =
                                        (st.layout.blend_scan_pages - 10)
                                            .clamp(MIN_SCAN_PAGES, MAX_SCAN_PAGES);
                                });
                            }
                        />
                        <IconButton
                            icon=IconName::Plus
                            size=14
                            title="Scan more pages (Fixed mode)"
                            class="rounded-full bg-line/60 hover:bg-line".to_string()
                            disabled=Signal::derive(move || {
                                if blend_off.get() {
                                    return true;
                                }
                                s.with(|st| {
                                    st.layout.blend_scope == PaperMode::Continuous
                                        || st.layout.blend_scan_pages >= MAX_SCAN_PAGES
                                })
                            })
                            on_click=move || {
                                s.update(|st| {
                                    st.layout.blend_scan_pages =
                                        (st.layout.blend_scan_pages + 10)
                                            .clamp(MIN_SCAN_PAGES, MAX_SCAN_PAGES);
                                });
                            }
                        />
                    </span>
                </span>
            </Row>
        </div>
        <div class="mt-5"><Separator vertical=false /></div>
        <p class="mt-2 text-xs text-muted">
            "Colour, tint, textures and presets live in the palette menu on the title bar."
        </p>
    }
}

fn hsl_to_hex(h: f64, s: f64, l: f64) -> String {
    let (s, l) = (s / 100.0, l / 100.0);
    let a = s * l.min(1.0 - l);
    let f = |n: f64| {
        let k = (n + h / 30.0) % 12.0;
        let c = l - a * ((k - 3.0).min(9.0 - k)).clamp(-1.0, 1.0);
        (c * 255.0).round() as u8
    };
    format!("#{:02x}{:02x}{:02x}", f(0.0), f(8.0), f(4.0))
}

fn hex_to_hsl(hex: &str) -> (f64, f64, f64) {
    let c = |i: usize| {
        u8::from_str_radix(hex.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0) as f64 / 255.0
    };
    let (r, g, b) = (c(1), c(3), c(5));
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    let d = max - min;
    if d == 0.0 {
        return (0.0, 0.0, l * 100.0);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == r {
        ((g - b) / d) % 6.0
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    ((h * 60.0 + 360.0) % 360.0, s * 100.0, l * 100.0)
}

#[component]
fn CustomColorPicker(
    state: AppState,
    open: RwSignal<bool>,
    anchor: NodeRef<html::Div>,
) -> impl IntoView {
    let s = state.settings;
    let (init_h, init_sat, init_li) = hex_to_hsl(&s.with_untracked(|st| st.gloss_custom.clone()));
    let (h, set_h) = signal(init_h);
    let (sat, set_sat) = signal(init_sat);
    let (li, set_li) = signal(init_li);
    Effect::new(move |_| {
        if !open.get() {
            return;
        }
        let (hh, ss, ll) = hex_to_hsl(&s.with_untracked(|st| st.gloss_custom.clone()));
        set_h.set(hh);
        set_sat.set(ss);
        set_li.set(ll);
    });
    let hex = move || hsl_to_hex(h.get(), sat.get(), li.get());
    Effect::new(move |_| {
        if open.get() {
            s.update(|st| st.gloss_custom = hex());
        }
    });
    view! {
        <MenuPopover
            open=open
            anchor=anchor
            width=224
            class="space-y-3 p-3".to_string()
            // The picker floats INSIDE the settings modal; the in-dialog
            // policy keeps the modal from evicting itself when the picker
            // opens (same reasoning as the tab's StyleSelects).
            policy=OverlayPolicy::IN_DIALOG
            hold_titlebar=false
        >
            <Slider
                value=h
                min=0.0
                max=360.0
                step=1.0
                label="Hue"
                on_change=move |v| set_h.set(v)
                class="hue-strip"
            />
            <Slider
                value=sat
                min=0.0
                max=100.0
                step=1.0
                label="Saturation"
                on_change=move |v| set_sat.set(v)
            />
            <Slider
                value=li
                min=5.0
                max=95.0
                step=1.0
                label="Lightness"
                on_change=move |v| set_li.set(v)
            />
            <div class="flex items-center justify-between text-xs text-muted">
                <span>"Preview"</span>
                <span
                    class="h-6 w-10 rounded-md border border-line"
                    style=move || format!("background:{}", hex())
                />
            </div>
        </MenuPopover>
    }
}
