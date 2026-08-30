//! The Layout tab of the reader settings modal: page indicator, floating
//! label, page chrome, window-fit and zoom behaviour — and the master switch
//! for the reader's motion, which is a layout decision before it is a theme
//! one, and which decides whether the Animations tab exists at all.

use leptos::prelude::*;

use pdf_core::math::FitMode;
use pdf_core::settings::{FloatingLabelStyle, PageIndicatorStyle};

use crate::components::settings::common::{Row, StyleSelect};
use crate::components::primitives::icon::IconName;
use crate::components::primitives::icon_button::IconButton;
use crate::components::primitives::section_label::SectionLabel;
use crate::components::primitives::switch::Switch;
use crate::state::AppState;

#[component]
pub(crate) fn LayoutTab(state: AppState) -> impl IntoView {
    let s = state.settings;
    let indicator_off = Signal::derive(move || !s.with(|st| st.layout.page_indicator));
    let label_off = Signal::derive(move || !s.with(|st| st.layout.floating_label));
    view! {
        <SectionLabel text="Reader chrome" />
        <div class="divide-y divide-line rounded-xl border border-line">
            <Row label="Page Indicator">
                <Switch
                    checked=Signal::derive(move || s.with(|st| st.layout.page_indicator))
                    on_change=Callback::new(move |v| {
                        s.update(|st| st.layout.page_indicator = v);
                    })
                    title="Floating page indicator".to_string()
                />
            </Row>
            <Row label="Indicator Style">
                <StyleSelect
                    value=Signal::derive(move || s.with(|st| st.layout.page_indicator_style))
                    on_change=Callback::new(move |v| {
                        s.update(|st| st.layout.page_indicator_style = v);
                    })
                    options=vec![
                        (PageIndicatorStyle::PageNumber, "Page Number"),
                        (PageIndicatorStyle::Percentage, "Percentage"),
                    ]
                    label_of=|v: &PageIndicatorStyle| match v {
                        PageIndicatorStyle::PageNumber => "Page Number",
                        PageIndicatorStyle::Percentage => "Percentage",
                    }
                    disabled=indicator_off
                />
            </Row>
            <Row label="Floating Label">
                <Switch
                    checked=Signal::derive(move || s.with(|st| st.layout.floating_label))
                    on_change=Callback::new(move |v| {
                        s.update(|st| st.layout.floating_label = v);
                    })
                    title="Floating document label".to_string()
                />
            </Row>
            <Row label="Label Content">
                <StyleSelect
                    value=Signal::derive(move || s.with(|st| st.layout.floating_label_style))
                    on_change=Callback::new(move |v| {
                        s.update(|st| st.layout.floating_label_style = v);
                    })
                    options=vec![
                        (FloatingLabelStyle::FileName, "File Name"),
                        (FloatingLabelStyle::Chapter, "Current Chapter"),
                    ]
                    label_of=|v: &FloatingLabelStyle| match v {
                        FloatingLabelStyle::FileName => "File Name",
                        FloatingLabelStyle::Chapter => "Current Chapter",
                    }
                    disabled=label_off
                />
            </Row>
            <Row label="Always Show Label">
                <Switch
                    checked=Signal::derive(move || s.with(|st| st.layout.floating_label_persist))
                    on_change=Callback::new(move |v| {
                        s.update(|st| st.layout.floating_label_persist = v);
                    })
                    disabled=label_off
                    title="Keep the floating label visible even when the title bar is open (the sidebar always hides it)"
                        .to_string()
                />
            </Row>
            <Row label="Label Width Limit">
                <span class="flex items-center gap-3">
                    <span class="w-10 text-right text-sm tabular-nums text-ink">
                        {move || {
                            format!("{}%", s.with(|st| st.layout.floating_label_max_pct) as u32)
                        }}
                    </span>
                    <span class="flex gap-1.5">
                        <IconButton
                            icon=IconName::Minus
                            size=14
                            title="Lower the width limit"
                            class="rounded-full bg-line/60 hover:bg-line".to_string()
                            disabled=Signal::derive(move || {
                                label_off.get()
                                    || s.with(|st| st.layout.floating_label_max_pct) <= 10.0
                            })
                            on_click=move || {
                                s.update(|st| {
                                    st.layout.floating_label_max_pct =
                                        (st.layout.floating_label_max_pct - 10.0).clamp(10.0, 100.0);
                                })
                            }
                        />
                        <IconButton
                            icon=IconName::Plus
                            size=14
                            title="Raise the width limit"
                            class="rounded-full bg-line/60 hover:bg-line".to_string()
                            disabled=Signal::derive(move || {
                                label_off.get()
                                    || s.with(|st| st.layout.floating_label_max_pct) >= 100.0
                            })
                            on_click=move || {
                                s.update(|st| {
                                    st.layout.floating_label_max_pct =
                                        (st.layout.floating_label_max_pct + 10.0).clamp(10.0, 100.0);
                                })
                            }
                        />
                    </span>
                </span>
            </Row>
            <Row label="Progress Bar">
                <Switch
                    checked=Signal::derive(move || s.with(|st| st.layout.progress_bar))
                    on_change=Callback::new(move |v| {
                        s.update(|st| st.layout.progress_bar = v);
                    })
                    title="Reading progress bar".to_string()
                />
            </Row>
            <Row label="Default Fit">
                <StyleSelect
                    value=Signal::derive(move || s.with(|st| st.layout.default_fit))
                    on_change=Callback::new(move |v| {
                        s.update(|st| st.layout.default_fit = v);
                    })
                    options=vec![
                        (FitMode::Page, "Fit Page"),
                        (FitMode::Width, "Fit Width"),
                    ]
                    label_of=|v: &FitMode| match v {
                        FitMode::Page => "Fit Page",
                        FitMode::Width => "Fit Width",
                        FitMode::None => "Fit Page",
                    }
                    disabled=Signal::derive(move || false)
                />
            </Row>
            <Row label="No Gap">
                <Switch
                    checked=Signal::derive(move || s.with(|st| st.layout.no_gap))
                    on_change=Callback::new(move |v| {
                        s.update(|st| st.layout.no_gap = v);
                    })
                    title="Remove the spacing between pages in scroll view".to_string()
                />
            </Row>
            // No Gap suspends the margin rather than zeroing it: the readout
            // greys out and the stepper stops while the gap owns the spacing,
            // and the margin the reader dialled in comes back the moment No
            // Gap turns off again.
            <Row label="Page Margin">
                <span class="flex items-center gap-3">
                    <span class=move || {
                        if s.with(|st| st.layout.no_gap) {
                            "w-10 text-right text-sm tabular-nums text-muted/60"
                        } else {
                            "w-10 text-right text-sm tabular-nums text-ink"
                        }
                    }>
                        {move || {
                            let m = s.with(|st| st.layout.page_margin) as u32;
                            if m == 0 {
                                "Off".into()
                            } else {
                                format!("{m}")
                            }
                        }}
                    </span>
                    <span class="flex gap-1.5">
                        <IconButton
                            icon=IconName::Minus
                            size=14
                            title="Less margin"
                            class="rounded-full bg-line/60 hover:bg-line".to_string()
                            disabled=Signal::derive(move || {
                                s.with(|st| st.layout.no_gap)
                                    || s.with(|st| st.layout.page_margin) <= 0.0
                            })
                            on_click=move || {
                                s.update(|st| {
                                    st.layout.page_margin =
                                        (st.layout.page_margin - 4.0).clamp(0.0, 64.0);
                                })
                            }
                        />
                        <IconButton
                            icon=IconName::Plus
                            size=14
                            title="More margin"
                            class="rounded-full bg-line/60 hover:bg-line".to_string()
                            disabled=Signal::derive(move || {
                                s.with(|st| st.layout.no_gap)
                                    || s.with(|st| st.layout.page_margin) >= 64.0
                            })
                            on_click=move || {
                                s.update(|st| {
                                    st.layout.page_margin =
                                        (st.layout.page_margin + 4.0).clamp(0.0, 64.0);
                                })
                            }
                        />
                    </span>
                </span>
            </Row>
            <Row label="Auto Scale">
                <Switch
                    checked=Signal::derive(move || s.with(|st| st.layout.auto_scale))
                    on_change=Callback::new(move |v| {
                        s.update(|st| st.layout.auto_scale = v);
                    })
                    title="Refit to width when entering single / two-page modes".to_string()
                />
            </Row>
            <Row label="Auto Resize">
                <Switch
                    checked=Signal::derive(move || s.with(|st| st.layout.auto_resize))
                    on_change=Callback::new(move |v| {
                        s.update(|st| st.layout.auto_resize = v);
                    })
                    title="Re-fit to width when a page of a different size comes into view"
                        .to_string()
                />
            </Row>
            <Row label="Page Shadow">
                <Switch
                    checked=Signal::derive(move || s.with(|st| st.layout.page_shadow))
                    on_change=Callback::new(move |v| {
                        s.update(|st| st.layout.page_shadow = v);
                    })
                    title="Drop shadow under PDF pages".to_string()
                />
            </Row>
            <Row label="Overlay Sidebar">
                <Switch
                    checked=Signal::derive(move || s.with(|st| st.layout.sidebar_overlay))
                    on_change=Callback::new(move |v| {
                        s.update(|st| st.layout.sidebar_overlay = v);
                    })
                    title="Sidebar floats over pages and auto-hides".to_string()
                />
            </Row>
        </div>
        <SectionLabel text="Motion" />
        <div class="divide-y divide-line rounded-xl border border-line">
            <Row label="Animations">
                <Switch
                    checked=Signal::derive(move || s.with(|st| st.animations.enabled))
                    on_change=Callback::new(move |v| {
                        s.update(|st| st.animations.enabled = v);
                    })
                    title="Everything that moves in the reader. Off, a change lands as its end \
                           frame and the Animations tab goes away with the switches it holds."
                        .to_string()
                />
            </Row>
        </div>
    }
}
