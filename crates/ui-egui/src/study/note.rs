//! A highlight's note beside it: read, or written in place.

use egui::text::{LayoutJob, TextWrapping};
use egui::{
    Align, Context, Id, Key, Layout, Margin, Modifiers, Rect, RichText, ScrollArea, Sense,
    TextFormat, Ui, vec2,
};
use mareader_appearance::Rgb;
use mareader_library::MarkId;

use crate::bubble::{Bubble, Side};
use crate::icons::Icon;
use crate::theme::{Tokens, color};
use crate::ui_command::{Commands, Sent};
use crate::widgets;
use mareader_engine::command::Command;
use mareader_engine::study::Action;

const WIDTH: f32 = 340.0;

/// What the note card shows this frame.
pub struct View<'a> {
    pub mark: MarkId,
    /// The highlighted words, quoted above a draft.
    pub quote: &'a str,
    /// The highlight's colour, for the quote's rule.
    pub ink: Rgb,
    /// The saved note.
    pub saved: Option<&'a str>,
    /// The draft, while writing.
    pub draft: Option<&'a mut String>,
    /// The first frame of writing: the field takes focus.
    pub fresh: bool,
    pub at: Rect,
    pub room: Rect,
}

/// Draws the note under or over its words, pointing at them.
pub fn show(ctx: &Context, view: View<'_>, t: &Tokens, out: &mut Commands) -> Rect {
    let writing = view.draft.is_some();
    let bubble = Bubble {
        id: Id::new(("study-note", view.mark, writing)),
        at: view.at,
        room: view.room,
        width: WIDTH.min(view.room.width() - 48.0).max(200.0),
        margin: Margin::same(14),
        snug: false,
        prefer: Side::Below,
    };
    let shown = bubble.show(ctx, t, |ui, cap| {
        ui.spacing_mut().item_spacing.y = 8.0;
        match view.draft {
            Some(draft) => {
                let had = view.saved.is_some();
                let note = Draft {
                    mark: view.mark,
                    quote: view.quote,
                    ink: view.ink,
                    fresh: view.fresh,
                    had,
                };
                write(ui, &note, draft, cap, t, out);
            }
            None => read(ui, view.mark, view.saved.unwrap_or_default(), cap, t, out),
        }
    });
    shown.0
}

/// The saved note, with Edit and Delete under it.
fn read(ui: &mut Ui, mark: MarkId, note: &str, cap: f32, t: &Tokens, out: &mut Commands) {
    // Long notes scroll; the buttons stay in view below.
    ScrollArea::vertical()
        .max_height((cap - 48.0).max(60.0))
        .auto_shrink([false, true])
        .show(ui, |ui| {
            let text = RichText::new(note).size(t.text * 1.05).color(t.ink);
            ui.add(egui::Label::new(text).wrap().selectable(true));
        });
    ui.horizontal(|ui| {
        let edit = egui::Button::image_and_text(Icon::Rename.image(ui, 14.0), "Edit")
            .image_tint_follows_text_color(true);
        if ui.add(edit).clicked() {
            out.push(Sent::Engine(Command::Study(Action::EditNote)));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if widgets::small_icon_button(ui, Icon::Remove, "Delete note").clicked() {
                out.push(Sent::Engine(Command::Study(Action::DeleteNote(mark))));
            }
        });
    });
}

/// What a draft is written about.
struct Draft<'a> {
    mark: MarkId,
    quote: &'a str,
    ink: Rgb,
    fresh: bool,
    /// The note was saved before: Delete is offered.
    had: bool,
}

/// The quote, the field, and Save; Ctrl+Enter saves, Escape cancels.
fn write(
    ui: &mut Ui,
    note: &Draft<'_>,
    draft: &mut String,
    cap: f32,
    t: &Tokens,
    out: &mut Commands,
) {
    let Draft {
        mark,
        quote,
        ink,
        fresh,
        had,
    } = *note;
    quoted(ui, quote, ink, t);
    let field = egui::TextEdit::multiline(draft)
        .id(Id::new("study-note-field"))
        .hint_text("Write a note…")
        .desired_rows(4)
        .desired_width(f32::INFINITY)
        .lock_focus(true);
    let r = ScrollArea::vertical()
        .max_height((cap - 96.0).max(80.0))
        .auto_shrink([false, true])
        .show(ui, |ui| ui.add(field))
        .inner;
    if fresh {
        r.request_focus();
    }
    let save_keys = ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Enter));
    ui.horizontal(|ui| {
        if had {
            let delete = egui::Button::new(RichText::new("Delete").color(t.danger)).frame(false);
            if ui.add(delete).clicked() {
                draft.clear();
                out.push(Sent::Engine(Command::Study(Action::SaveNote(mark))));
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let save = widgets::primary_button(ui, None, "Save", t.accent, t);
            let r = ui.add(save);
            widgets::tip(&r, "Save", Some(save_hint()));
            if r.clicked() || save_keys {
                out.push(Sent::Engine(Command::Study(Action::SaveNote(mark))));
            }
            if ui.button("Cancel").clicked() {
                out.push(Sent::Engine(Command::Study(Action::CancelNote)));
            }
        });
    });
}

/// The keys that save, as this platform writes them.
fn save_hint() -> &'static str {
    if cfg!(target_os = "macos") {
        "⌘Enter"
    } else {
        "Ctrl+Enter"
    }
}

/// The highlighted words, two lines at most, after a rule in its colour.
fn quoted(ui: &mut Ui, quote: &str, ink: Rgb, t: &Tokens) {
    let width = ui.available_width() - 12.0;
    let mut job = LayoutJob::single_section(
        quote.split_whitespace().collect::<Vec<_>>().join(" "),
        TextFormat::simple(egui::FontId::proportional(t.text_small), t.muted),
    );
    job.wrap = TextWrapping {
        max_width: width.max(40.0),
        max_rows: 2,
        break_anywhere: false,
        overflow_character: Some('…'),
    };
    let galley = ui.painter().layout_job(job);
    let size = vec2(ui.available_width(), galley.size().y);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let rule = Rect::from_min_size(rect.min, vec2(3.0, rect.height()));
    ui.painter().rect_filled(rule, 1.5, color(ink));
    ui.painter()
        .galley(rect.min + vec2(12.0, 0.0), galley, t.muted);
}
