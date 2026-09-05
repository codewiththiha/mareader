//! The Fonts tab: typography for the reflowable formats (TXT, Markdown).
//!
//! PDF pages are rasters — their type was fixed at authoring time — so this
//! tab exists only while a text document is open (the modal resolves that,
//! the same way it resolves the Animations tab). Every control writes into
//! `Settings::text`, which the typography effect paints onto `<html>` and
//! the measure column turns into a re-cut — so a knob here moves the whole
//! pipeline, and nothing in this file touches layout directly.
//!
//! The font pickers offer the system faces today; the choice type
//! (`FontChoice`) already has a built-in variant, so shipping bundled fonts
//! later is an options-list change, not a schema change.

use leptos::prelude::*;

use reflow_core::typography::{FontChoice, SystemFont, TextColumnAlign, TextSettings};

use crate::components::settings::common::{Row, StyleSelect};
use app_chrome::icon::IconName;
use app_chrome::icon_button::IconButton;
use crate::components::primitives::menu::section_label::SectionLabel;
use crate::components::primitives::controls::switch::Switch;
use crate::state::AppState;

/// One write path for every knob: mutate, then sanitize — so a clamped
/// range is enforced no matter which control produced the value. Shared
/// with the Appearance menu, whose ink-intensity slider writes through the
/// same path for the same reason.
pub(crate) fn update_text(state: AppState, apply: impl Fn(&mut TextSettings) + 'static) {
    state.settings.update(move |st| {
        apply(&mut st.text);
        reflow_core::typography::sanitize(&mut st.text);
    });
}

/// The picker's option list: `Default`, then every system face. Built-in
/// fonts join this list the day the app ships any (`FontChoice::BuiltIn`).
fn font_options() -> Vec<(FontChoice, &'static str)> {
    let mut options = vec![(FontChoice::Default, "Default")];
    for font in SystemFont::all() {
        options.push((FontChoice::System(*font), font.label()));
    }
    options
}

fn font_label(choice: &FontChoice) -> &'static str {
    match choice {
        FontChoice::Default => "Default",
        FontChoice::System(font) => font.label(),
        FontChoice::BuiltIn(_) => "Built-in",
    }
}

/// One font picker row. The pickers all share one shape — read a slot of
/// the settings, write a choice back — so the slot lives in two function
/// pointers instead of four near-identical components.
#[component]
fn FontPickerRow(
    state: AppState,
    label: &'static str,
    pick: fn(&TextSettings) -> &FontChoice,
    write: fn(&mut TextSettings, FontChoice),
) -> impl IntoView {
    let s = state.settings;
    let options = StoredValue::new(font_options());
    view! {
        <Row label=label>
            <StyleSelect
                value=Signal::derive(move || s.with(|st| pick(&st.text).clone()))
                on_change=Callback::new(move |choice: FontChoice| {
                    update_text(state, move |t| write(t, choice.clone()));
                })
                options=options.get_value()
                label_of=font_label
                disabled=Signal::derive(|| false)
            />
        </Row>
    }
}

/// A −/+ adjuster row: the current value formatted on the left, the two
/// steppers on the right, each disabled at its end of the range.
#[component]
fn StepperRow(
    label: &'static str,
    /// The formatted current value ("17 px", "1.7×", …).
    display: Signal<String>,
    #[prop(into)]
    minus_disabled: Signal<bool>,
    #[prop(into)]
    plus_disabled: Signal<bool>,
    on_minus: Callback<()>,
    on_plus: Callback<()>,
    /// What the steppers adjust; each button's tooltip derives from it
    /// ("Decrease font size" / "Increase font size").
    #[prop(into)]
    title: String,
) -> impl IntoView {
    let minus_title = format!("Decrease {title}");
    let plus_title = format!("Increase {title}");
    view! {
        <Row label=label>
            <span class="flex items-center gap-3">
                <span class="w-14 text-right text-sm tabular-nums text-ink">
                    {move || display.get()}
                </span>
                <span class="flex gap-1.5">
                    <IconButton
                        icon=IconName::Minus
                        size=14
                        title=minus_title
                        class="rounded-full bg-line/60 hover:bg-line".to_string()
                        disabled=minus_disabled
                        on_click=move || on_minus.run(())
                    />
                    <IconButton
                        icon=IconName::Plus
                        size=14
                        title=plus_title
                        class="rounded-full bg-line/60 hover:bg-line".to_string()
                        disabled=plus_disabled
                        on_click=move || on_plus.run(())
                    />
                </span>
            </span>
        </Row>
    }
}

#[component]
pub(crate) fn FontsTab(state: AppState) -> impl IntoView {
    let s = state.settings;

    view! {
        <SectionLabel text="Typeface" />
        <div class="divide-y divide-line rounded-xl border border-line">
            <FontPickerRow state=state label="Font" pick=|t| &t.default_font write=|t, f| t.default_font = f />
            <FontPickerRow state=state label="Serif Font" pick=|t| &t.serif_font write=|t, f| t.serif_font = f />
            <FontPickerRow state=state label="Sans-serif Font" pick=|t| &t.sans_font write=|t, f| t.sans_font = f />
            <FontPickerRow state=state label="Monospace Font" pick=|t| &t.mono_font write=|t, f| t.mono_font = f />
            <StepperRow
                label="Font Size"
                display=Signal::derive(move || {
                    format!("{} px", s.with(|st| st.text.font_size) as u32)
                })
                minus_disabled=Signal::derive(move || s.with(|st| st.text.font_size) <= 10.0)
                plus_disabled=Signal::derive(move || s.with(|st| st.text.font_size) >= 32.0)
                on_minus=Callback::new(move |_| {
                    update_text(state, |t| t.font_size -= 1.0);
                })
                on_plus=Callback::new(move |_| {
                    update_text(state, |t| t.font_size += 1.0);
                })
                title="font size".to_string()
            />
            <StepperRow
                label="Font Weight"
                display=Signal::derive(move || {
                    s.with(|st| st.text.font_weight).to_string()
                })
                minus_disabled=Signal::derive(move || s.with(|st| st.text.font_weight) <= 100)
                plus_disabled=Signal::derive(move || s.with(|st| st.text.font_weight) >= 900)
                on_minus=Callback::new(move |_| {
                    update_text(state, |t| t.font_weight = t.font_weight.saturating_sub(100));
                })
                on_plus=Callback::new(move |_| {
                    update_text(state, |t| t.font_weight = t.font_weight.saturating_add(100));
                })
                title="font weight".to_string()
            />
        </div>

        <SectionLabel text="Spacing" />
        <div class="divide-y divide-line rounded-xl border border-line">
            <StepperRow
                label="Line Spacing"
                display=Signal::derive(move || {
                    format!("{:.1}×", s.with(|st| st.text.line_height))
                })
                minus_disabled=Signal::derive(move || s.with(|st| st.text.line_height) <= 1.0)
                plus_disabled=Signal::derive(move || s.with(|st| st.text.line_height) >= 3.0)
                on_minus=Callback::new(move |_| {
                    update_text(state, |t| t.line_height -= 0.1);
                })
                on_plus=Callback::new(move |_| {
                    update_text(state, |t| t.line_height += 0.1);
                })
                title="line spacing".to_string()
            />
            <StepperRow
                label="Paragraph Margin"
                display=Signal::derive(move || {
                    format!("{:.2} em", s.with(|st| st.text.paragraph_margin))
                })
                minus_disabled=Signal::derive(move || s.with(|st| st.text.paragraph_margin) <= 0.0)
                plus_disabled=Signal::derive(move || s.with(|st| st.text.paragraph_margin) >= 3.0)
                on_minus=Callback::new(move |_| {
                    update_text(state, |t| t.paragraph_margin -= 0.25);
                })
                on_plus=Callback::new(move |_| {
                    update_text(state, |t| t.paragraph_margin += 0.25);
                })
                title="paragraph margin".to_string()
            />
            <StepperRow
                label="Word Spacing"
                display=Signal::derive(move || {
                    format!("{:.1} px", s.with(|st| st.text.word_spacing))
                })
                minus_disabled=Signal::derive(move || s.with(|st| st.text.word_spacing) <= -2.0)
                plus_disabled=Signal::derive(move || s.with(|st| st.text.word_spacing) >= 10.0)
                on_minus=Callback::new(move |_| {
                    update_text(state, |t| t.word_spacing -= 0.5);
                })
                on_plus=Callback::new(move |_| {
                    update_text(state, |t| t.word_spacing += 0.5);
                })
                title="word spacing".to_string()
            />
            <StepperRow
                label="Letter Spacing"
                display=Signal::derive(move || {
                    format!("{:.2} em", s.with(|st| st.text.letter_spacing))
                })
                minus_disabled=Signal::derive(move || s.with(|st| st.text.letter_spacing) <= -0.02)
                plus_disabled=Signal::derive(move || s.with(|st| st.text.letter_spacing) >= 0.3)
                on_minus=Callback::new(move |_| {
                    update_text(state, |t| t.letter_spacing -= 0.01);
                })
                on_plus=Callback::new(move |_| {
                    update_text(state, |t| t.letter_spacing += 0.01);
                })
                title="letter spacing".to_string()
            />
            <StepperRow
                label="Text Indent"
                display=Signal::derive(move || {
                    format!("{:.1} em", s.with(|st| st.text.text_indent))
                })
                minus_disabled=Signal::derive(move || s.with(|st| st.text.text_indent) <= 0.0)
                plus_disabled=Signal::derive(move || s.with(|st| st.text.text_indent) >= 4.0)
                on_minus=Callback::new(move |_| {
                    update_text(state, |t| t.text_indent -= 0.5);
                })
                on_plus=Callback::new(move |_| {
                    update_text(state, |t| t.text_indent += 0.5);
                })
                title="text indent".to_string()
            />
        </div>

        <SectionLabel text="Layout" />
        <div class="divide-y divide-line rounded-xl border border-line">
            <Row label="Column Align">
                <StyleSelect
                    value=Signal::derive(move || s.with(|st| st.text.column_align))
                    on_change=Callback::new(move |v: TextColumnAlign| {
                        update_text(state, move |t| t.column_align = v);
                    })
                    options=vec![
                        (TextColumnAlign::Left, "Left"),
                        (TextColumnAlign::Center, "Center"),
                        (TextColumnAlign::Right, "Right"),
                    ]
                    label_of=|v: &TextColumnAlign| v.label()
                    disabled=Signal::derive(|| false)
                />
            </Row>
            <Row label="Use Book Layout">
                <Switch
                    checked=Signal::derive(move || s.with(|st| st.text.book_layout))
                    on_change=Callback::new(move |v| {
                        update_text(state, move |t| t.book_layout = v);
                    })
                    title="Face pages into an open book: a gutter margin faces the spine".to_string()
                />
            </Row>
            <Row label="Full Justification">
                <Switch
                    checked=Signal::derive(move || s.with(|st| st.text.justify))
                    on_change=Callback::new(move |v| {
                        update_text(state, move |t| t.justify = v);
                    })
                    title="Stretch every line to both margins".to_string()
                />
            </Row>
            <Row label="Hyphenation">
                <Switch
                    checked=Signal::derive(move || s.with(|st| st.text.hyphenation))
                    on_change=Callback::new(move |v| {
                        update_text(state, move |t| t.hyphenation = v);
                    })
                    title="Break words at line ends where the language allows".to_string()
                />
            </Row>
        </div>
    }
}
