//! A book's type, and a PDF's AI pages', in the Appearance tab.
use std::sync::Arc;

use egui::{Button, RichText, Ui};
use mareader_session::{FACES, Kind, bundled, has_family};
use mareader_settings::{
    AppearanceMenuStyle, BookDesign, ColumnAlign, FontChoice, FontFamily, SystemFont, TextSettings,
};

use mareader_library::{BookId, Design};

use super::{disclosure, divider, group, note, row, section, switch_row};
use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent};
use crate::widgets;
use mareader_engine::command::Command;
use mareader_engine::library::Action;
use mareader_engine::scope::{Scope, Shown};

const PICKER_WIDTH: f32 = 180.0;

/// The focused book's own choice, over the setting above.
fn this_book(ui: &mut Ui, (book, own): (BookId, Option<Design>), out: &mut Commands) {
    let hint = "This book only, over the setting above; saved with the book";
    row(ui, "This book", hint, |ui| {
        let shown = own.map(|d| d != Design::Off);
        let mut pick = shown;
        // Added right to left, so they read left to right.
        ui.selectable_value(&mut pick, Some(false), "Off");
        ui.selectable_value(&mut pick, Some(true), "On");
        ui.selectable_value(&mut pick, None, "As set");
        if pick != shown {
            let design = pick.map(|on| if on { Design::Auto } else { Design::Off });
            out.push(Sent::Engine(Command::Library(Action::Design(book, design))));
        }
    });
}

/// A book's settings, grouped as Book, Font, Paragraph, Page and Colour.
pub(super) fn tab(
    ui: &mut Ui,
    t: &mut TextSettings,
    menu: &mut AppearanceMenuStyle,
    cx: &Scope,
    out: &mut Commands,
) {
    section(ui, "Book");
    group(ui, |ui| {
        let hint = "Its own boxes, rules, fonts and spacing; off, plain text";
        row(ui, "Book styles", hint, |ui| {
            let was = t.book_design != BookDesign::Off;
            let mut on = was;
            // Added right to left, so they read left to right.
            ui.selectable_value(&mut on, false, "Off");
            ui.selectable_value(&mut on, true, "On");
            if on != was {
                t.book_design = if on {
                    BookDesign::Auto
                } else {
                    BookDesign::Off
                };
            }
        });
        if let Some(chosen) = cx.book_design {
            divider(ui);
            this_book(ui, chosen, out);
        }
    });
    section(ui, "Font");
    group(ui, |ui| typeface(ui, t, true));
    section(ui, "Paragraph");
    group(ui, |ui| {
        spacing(ui, t, true);
        divider(ui);
        lines(ui, t, true);
    });
    // Pages take the pane's size; the stream keeps a column of its own.
    let stream = cx.focused == Shown::Book { stream: true };
    section(ui, if stream { "Column" } else { "Page" });
    group(ui, |ui| {
        if stream {
            column(ui, t);
        } else {
            page(ui, t);
        }
        divider(ui);
        let hint = "A wider margin toward the spine, as in a bound book";
        switch_row(ui, "Book layout", hint, &mut t.book_layout);
    });
    section(ui, "Colour");
    group(ui, |ui| {
        super::colours_row(ui, out);
        divider(ui);
        super::appearance_menu_row(ui, menu);
        divider(ui);
        let hint = "How dark a book's text stays; its pictures keep their colours";
        let percent = |v: f32| format!("{v:.0} %");
        let ink = &mut t.ink_contrast;
        stepped(ui, "Text ink", hint, ink, TextSettings::INK, 5.0, percent);
        divider(ui);
        switch_row(
            ui,
            "Theme pictures",
            "Tint a book's pictures with the page; off, they keep their colours",
            &mut t.theme_pictures,
        );
        divider(ui);
        switch_row(
            ui,
            "Theme emoji",
            "Tint emoji with the page; off, they keep their colours",
            &mut t.theme_emojis,
        );
    });
}

/// The vertical stream's column: how wide it runs and where it sits.
fn column(ui: &mut Ui, t: &mut TextSettings) {
    row(
        ui,
        "Column width",
        "How wide the stream's text runs",
        |ui| {
            ui.spacing_mut().slider_width = 140.0;
            let (lo, hi) = TextSettings::COLUMN_PCT;
            ui.add(
                egui::Slider::new(&mut t.column_width_pct, lo..=hi)
                    .step_by(5.0)
                    .fixed_decimals(0)
                    .suffix(" %"),
            );
        },
    );
    divider(ui);
    row(
        ui,
        "Column alignment",
        "Where the stream's text sits",
        |ui| {
            // Added right to left, so they read left to right.
            let align = &mut t.column_align;
            ui.selectable_value(align, ColumnAlign::Right, "Right");
            ui.selectable_value(align, ColumnAlign::Center, "Center");
            ui.selectable_value(align, ColumnAlign::Left, "Left");
        },
    );
}

/// A paged book's page: it fills the pane, inside these margins.
fn page(ui: &mut Ui, t: &mut TextSettings) {
    let pt = |v: f32| format!("{v:.0} pt");
    let margin = TextSettings::PAGE_MARGIN;
    let hint = "Above and below a paged book's text";
    stepped(
        ui,
        "Top and bottom margins",
        hint,
        &mut t.page_margin_y,
        margin,
        4.0,
        pt,
    );
    divider(ui);
    let hint = "Beside a paged book's text";
    stepped(
        ui,
        "Side margins",
        hint,
        &mut t.page_margin_x,
        margin,
        4.0,
        pt,
    );
    divider(ui);
    let hint = "Past this, a wide page centres its text";
    let line = TextSettings::MAX_LINE;
    stepped(
        ui,
        "Longest line",
        hint,
        &mut t.max_line_width,
        line,
        20.0,
        pt,
    );
}

/// A PDF's AI pages: type and spacing; its page keeps their measure.
pub(super) fn ai_tab(ui: &mut Ui, t: &mut TextSettings) {
    section(ui, "AI pages");
    note(
        ui,
        "This PDF's AI pages are set in this type. A result never outgrows \
        its page: past the room, its text shrinks to fit.",
    );
    section(ui, "Typeface");
    group(ui, |ui| typeface(ui, t, false));
    section(ui, "Spacing");
    group(ui, |ui| spacing(ui, t, false));
    section(ui, "Lines");
    group(ui, |ui| lines(ui, t, false));
}

/// Justification and hyphenation; `book` adds what a book's own wins over.
fn lines(ui: &mut Ui, t: &mut TextSettings, book: bool) {
    let hint = match book && !t.override_layout {
        true => "Stretch lines to both margins; a justified book stays so",
        false => "Stretch every line but a paragraph's last to both margins",
    };
    switch_row(ui, "Full justification", hint, &mut t.justify);
    divider(ui);
    switch_row(
        ui,
        "Hyphenation",
        "Break long words at line ends where the book's language allows",
        &mut t.hyphenation,
    );
}

/// Faces, size and weight; `book` adds the book's own overrides.
fn typeface(ui: &mut Ui, t: &mut TextSettings, book: bool) {
    if book {
        let hint = "Your fonts and text size for all of a book's text";
        switch_row(ui, "Override book font", hint, &mut t.override_font);
        divider(ui);
    }
    let body = "The body text; Default follows the serif font";
    picker(ui, "Body font", body, &mut t.default_font, None);
    divider(ui);
    // A share of the standard size: a book's sizes scale from it.
    let hint = match book {
        true => "A book's headings and small print scale with it",
        false => "",
    };
    let mut pct = t.size_pct();
    let percent = |v: f32| format!("{v:.0} %");
    stepped(
        ui,
        "Text size",
        hint,
        &mut pct,
        TextSettings::SIZE_PCT,
        5.0,
        percent,
    );
    if pct != t.size_pct() {
        t.set_size_pct(pct);
    }
    divider(ui);
    if book {
        let hint = "A book's small print never sets smaller";
        let pt = |v: f32| format!("{v:.0} pt");
        let floor = &mut t.min_font_size;
        stepped(
            ui,
            "Minimum size",
            hint,
            floor,
            TextSettings::MIN_FONT,
            1.0,
            pt,
        );
        divider(ui);
    }
    let hint = "Headings keep their own";
    let weight = TextSettings::FONT_WEIGHT;
    let plain = |v: f32| format!("{v:.0}");
    stepped(
        ui,
        "Font weight",
        hint,
        &mut t.font_weight,
        weight,
        100.0,
        plain,
    );
    divider(ui);
    // Rarely changed: they stand in for faces the book itself names.
    let hint = "Faces used where a book asks for serif, sans-serif or code";
    if !disclosure(ui, "Book's own families", hint) {
        return;
    }
    divider(ui);
    let serif = Some(FontFamily::Serif);
    picker(ui, "Serif font", "", &mut t.serif_font, serif);
    divider(ui);
    let sans = Some(FontFamily::Sans);
    let hint = "Plain books' headings, and where a book asks for it";
    picker(ui, "Sans-serif font", hint, &mut t.sans_font, sans);
    divider(ui);
    let mono = Some(FontFamily::Mono);
    picker(ui, "Monospace font", "Code", &mut t.mono_font, mono);
}

/// Line, paragraph, word and letter spacing; `book` adds its override.
fn spacing(ui: &mut Ui, t: &mut TextSettings, book: bool) {
    if book {
        let hint = "Your spacing, indent and justification for its paragraphs";
        switch_row(ui, "Override book layout", hint, &mut t.override_layout);
        divider(ui);
    }
    let em2 = |v: f32| format!("{v:.2} em");
    let line = TextSettings::LINE_HEIGHT;
    let times = |v: f32| format!("{v:.1}×");
    stepped(ui, "Line spacing", "", &mut t.line_height, line, 0.1, times);
    divider(ui);
    let hint = match book && !t.override_layout {
        true => "At 1, a book's own gaps; more or less moves them",
        false => "Space between paragraphs",
    };
    let gap = TextSettings::PARAGRAPH;
    stepped(
        ui,
        "Paragraph margin",
        hint,
        &mut t.paragraph_margin,
        gap,
        0.25,
        em2,
    );
    divider(ui);
    let words = TextSettings::WORD_SPACING;
    let px = |v: f32| format!("{v:.1} px");
    stepped(ui, "Word spacing", "", &mut t.word_spacing, words, 0.5, px);
    divider(ui);
    let letters = TextSettings::LETTER_SPACING;
    stepped(
        ui,
        "Letter spacing",
        "",
        &mut t.letter_spacing,
        letters,
        0.01,
        em2,
    );
    divider(ui);
    let hint = "A paragraph's first line";
    let indent = TextSettings::INDENT;
    let em1 = |v: f32| format!("{v:.1} em");
    stepped(
        ui,
        "Text indent",
        hint,
        &mut t.text_indent,
        indent,
        0.5,
        em1,
    );
}

/// A row with a stepper over `lo..=hi`.
fn stepped(
    ui: &mut Ui,
    label: &str,
    hint: &str,
    value: &mut f32,
    (lo, hi): (f32, f32),
    step: f32,
    show: impl Fn(f32) -> String,
) {
    row(ui, label, hint, |ui| {
        let name = label.to_lowercase();
        widgets::stepper(ui, value, lo..=hi, step, show, &name);
    });
}

/// What a pick is called in the picker.
fn label(choice: &FontChoice) -> &'static str {
    match choice {
        FontChoice::Default => "Default",
        FontChoice::System(f) => f.label(),
        FontChoice::Bundled(id) => bundled(id).map_or("Default", |f| f.name),
    }
}

fn kind(family: FontFamily) -> Kind {
    match family {
        FontFamily::Serif => Kind::Serif,
        FontFamily::Sans => Kind::Sans,
        FontFamily::Mono => Kind::Mono,
    }
}

/// One slot's picker; `only` keeps a family's slot to its own faces.
fn picker(ui: &mut Ui, name: &str, hint: &str, choice: &mut FontChoice, only: Option<FontFamily>) {
    row(ui, name, hint, |ui| {
        widgets::combo(name)
            .width(PICKER_WIDTH)
            .selected_text(label(choice))
            .show_ui(ui, |ui| {
                ui.selectable_value(choice, FontChoice::Default, "Default");
                heading(ui, "Bundled");
                let ours = FACES
                    .iter()
                    .filter(|f| only.is_none_or(|o| kind(o) == f.kind));
                for face in ours {
                    let pick = FontChoice::Bundled(face.id.to_owned());
                    ui.selectable_value(choice, pick, face.name);
                }
                heading(ui, "On this computer");
                let installed = installed(ui);
                let theirs = SystemFont::ALL
                    .iter()
                    .filter(|f| only.is_none_or(|o| o == f.family()));
                for &font in theirs {
                    let pick = FontChoice::System(font);
                    let on = *choice == pick;
                    let button = Button::selectable(on, font.label());
                    let r = ui.add_enabled(installed.contains(&font), button);
                    let r = r.on_disabled_hover_text("Not installed");
                    if r.clicked() {
                        *choice = pick;
                    }
                }
            });
    });
}

fn heading(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(4.0);
    ui.label(RichText::new(text).size(t.text_small).color(t.muted));
}

/// The system faces this machine has, looked up once per run.
fn installed(ui: &Ui) -> Arc<[SystemFont]> {
    let id = egui::Id::new("installed-system-fonts");
    if let Some(known) = ui.data(|d| d.get_temp::<Arc<[SystemFont]>>(id)) {
        return known;
    }
    // A generic name always finds the system's own face.
    let generic = |n: &str| matches!(n, "serif" | "sans-serif" | "monospace");
    let found: Arc<[SystemFont]> = SystemFont::ALL
        .iter()
        .copied()
        .filter(|f| f.names().iter().any(|n| generic(n) || has_family(n)))
        .collect();
    ui.data_mut(|d| d.insert_temp(id, Arc::clone(&found)));
    found
}
