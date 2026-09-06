//! The Layout tab of the reader settings modal: page indicator, floating
//! label, page chrome, window-fit and zoom behaviour — and the master switch
//! for the reader's motion, which is a layout decision before it is a theme
//! one, and which decides whether the Animations tab exists at all.

use leptos::prelude::*;

use reader_core::view::ViewMode;
use reader_core::zoom_math::FitMode;
use reader_core::settings::{
    FloatingLabelStyle, MAX_COLUMN_WIDTH_PCT, MIN_COLUMN_WIDTH_PCT, PageIndicatorStyle,
};

use crate::components::settings::common::{Row, StyleSelect};
use app_chrome::icon::IconName;
use app_chrome::icon_button::IconButton;
use crate::components::primitives::form::slider::Slider;
use crate::components::primitives::menu::section_label::SectionLabel;
use crate::components::primitives::controls::switch::Switch;
use crate::state::AppState;

#[component]
pub(crate) fn LayoutTab(state: AppState) -> impl IntoView {
    let s = state.settings;
    let indicator_off = Signal::derive(move || !s.with(|st| st.layout.page_indicator));
    let label_off = Signal::derive(move || !s.with(|st| st.layout.floating_label));
    // The horizontal strip never carries a page margin — the reader resolves
    // the stored pref to 0 while that mode is on — so the adjuster sits
    // disabled until another mode returns. The stored value is untouched and
    // comes back with it.
    let horizontal_mode = Signal::derive(move || {
        state.reader.viewer.mode.get() == ViewMode::ScrollHorizontal
    });
    // A reflowable document answers to the typography and the two width
    // dials, not to page chrome: rows that would only lie about what they
    // control for text/Markdown (No Gap is a PDF strip's concern — a
    // streamed document has no gap to remove) leave the tree entirely
    // rather than sitting disabled.
    let reflowable = Signal::derive(move || state.reader.reflowable());
    // Continuous text reading has no pages to number: while the stream is
    // live the indicator is a percentage by definition, so the style
    // selector stands disabled rather than offering a choice that is not
    // being honoured.
    let stream_live = Signal::derive(move || state.reader.reflow_streaming());
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
                    disabled=Signal::derive(move || indicator_off.get() || stream_live.get())
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
            <Show when=move || !reflowable.get()>
                <Row label="No Gap">
                    <Switch
                        checked=Signal::derive(move || s.with(|st| st.layout.no_gap))
                        on_change=Callback::new(move |v| {
                            s.update(|st| st.layout.no_gap = v);
                        })
                        title="Remove the spacing between pages in scroll view. A text \\
                               or Markdown document has no page gap — and no page strip \\
                               the switch could reach — so the row stands down for them."
                            .to_string()
                    />
                </Row>
            </Show>
            // Page Margin is the horizontal (left/right) air around each page,
            // which No Gap never touches — No Gap only removes the vertical
            // gap between stacked pages. The two stay fully independent, so
            // the margin adjuster is live whether or not No Gap is on. The one
            // exception is the horizontal scroll mode, which never carries a
            // margin: while it is on, the adjuster is disabled and the stored
            // value waits, untouched, for the other modes.
            <Row label="Page Margin">
                <span class="flex items-center gap-3">
                    <span
                        class="w-10 text-right text-sm tabular-nums text-ink"
                        class=("opacity-45", move || horizontal_mode.get())
                    >
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
                                horizontal_mode.get()
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
                                horizontal_mode.get()
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
            // Column Width is the reading measure dial: 100% is the natural
            // column the typography and the page geometry agreed on, and the
            // ends trade line length for everything else. Text and Markdown
            // answer it in every mode (the paginated card grows with the
            // column, the stream's column follows it directly); a PDF's page
            // is the document's own, so there the dial moves the fit-width
            // budget the pages resolve against instead.
            <Row label="Column Width">
                <span class="flex w-44 items-center">
                    <Slider
                        value=Signal::derive(move || s.with(|st| st.layout.column_width_pct))
                        min=MIN_COLUMN_WIDTH_PCT
                        max=MAX_COLUMN_WIDTH_PCT
                        step=5.0
                        unit="%"
                        on_change=move |v| {
                            s.update(|st| {
                                st.layout.column_width_pct =
                                    v.round().clamp(MIN_COLUMN_WIDTH_PCT, MAX_COLUMN_WIDTH_PCT);
                            });
                        }
                        label="Column width"
                    />
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
                    title="Drop shadow under pages".to_string()
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
