//! Export and copy-all controls for the notes panel.

use egui::{Id, Ui};
use mareader_engine::command::Command;
use mareader_engine::export::{self, Entry, Format, Sheet};
use mareader_engine::notes_panel::page_of;
use mareader_pdf::active_entry;

use super::Row;
use crate::documents::Reading;
use crate::icons::Icon;
use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent, UiCommand};
use crate::widgets;

/// Export, then Copy all: the list as shown, so a filter narrows both.
pub(super) fn share(
    ui: &mut Ui,
    reading: &Reading,
    placed: &[(Option<usize>, Option<&str>, &Row)],
    _t: &Tokens,
    out: &mut Commands,
) {
    let tally = || {
        let mut count = 0;
        let mut notes = 0;
        for &(_, _, r) in placed {
            if !r.bookmark {
                count += 1;
                notes += usize::from(r.note.is_some());
            }
        }
        export::tally(count, notes)
    };
    // Written only when asked for, not each frame.
    let render = |format| {
        let shown: Vec<&Row> = placed
            .iter()
            .map(|&(_, _, r)| r)
            .filter(|r| !r.bookmark)
            .collect();
        let entries = entries(reading, &shown);
        let sheet = Sheet {
            title: &reading.name,
            entries: &entries,
            now_ms: mareader_engine::library::wall_ms(),
        };
        export::render(&sheet, format)
    };
    let menu = Id::new(("notes-export", reading.doc));
    let open = egui::Popup::is_id_open(ui.ctx(), menu);
    let r = widgets::icon_button(ui, Icon::Download, "Export…", open);
    egui::Popup::from_toggle_button_response(&r)
        .id(menu)
        .sense(widgets::POPUP_SENSE)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .align(egui::RectAlign::BOTTOM_END)
        .width(widgets::MENU_W)
        .gap(4.0)
        .show(|ui| {
            widgets::menu_style(ui, widgets::MENU_W);
            let what = format!("Export {}", tally());
            widgets::menu_section(ui, None, &what);
            for format in Format::ALL {
                let label = format!("{} (.{})", format.label(), format.extension());
                if widgets::menu_row(ui, Icon::Download, &label, None, false).clicked() {
                    out.push(Sent::Engine(Command::Export {
                        name: export::file_name(&reading.name, format),
                        format,
                        contents: render(format),
                    }));
                    ui.close();
                }
            }
            widgets::close_on_key(ui);
        });
    if widgets::icon_button(ui, Icon::Copy, "Copy all as Markdown", false).clicked() {
        ui.ctx().copy_text(render(Format::Markdown));
        let copied = format!("Copied {}", tally());
        out.push(Sent::Ui(UiCommand::Notify(
            mareader_engine::toast::Toast::info(copied),
        )));
    }
}

/// The rows as exported, each with its page and chapter.
fn entries(reading: &Reading, rows: &[&Row]) -> Vec<Entry> {
    let outline = reading.document.outline();
    let chapter = |page| {
        let i = active_entry(outline, page)?;
        outline.get(i).map(|e| e.title.clone())
    };
    rows.iter()
        .map(|r| {
            let page = page_of(&reading.document, r.spot);
            Entry {
                quote: r.quote.clone(),
                note: r.note.clone(),
                page,
                chapter: page.and_then(chapter),
                pen: r.pen,
                color: r.ink,
                added: r.added,
            }
        })
        .collect()
}
