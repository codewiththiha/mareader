//! Searchable bundled and installed font choices.

use std::sync::Arc;

use egui::{Button, CornerRadius, Id, RichText, Ui};
use mareader_session::{FACES, Kind, bundled, has_family};
use mareader_settings::{FontChoice, FontFamily, SystemFont};

use super::super::row;
use crate::side_list::{self, Spec};
use crate::theme::Tokens;
use crate::widgets;

const WIDTH: f32 = 180.0;

pub(super) fn picker(
    ui: &mut Ui,
    name: &str,
    hint: &str,
    choice: &mut FontChoice,
    only: Option<FontFamily>,
) {
    row(ui, name, hint, |ui| {
        let shown = match choice {
            FontChoice::Default => "Default",
            FontChoice::System(f) => f.label(),
            FontChoice::Bundled(id) => bundled(id).map_or("Default", |f| f.name),
        };
        let mut items = vec![(FontChoice::Default, "Default".to_owned())];
        items.extend(
            FACES
                .iter()
                .filter(|f| only.is_none_or(|o| kind(o) == f.kind))
                .map(|face| {
                    (
                        FontChoice::Bundled(face.id.to_owned()),
                        face.name.to_owned(),
                    )
                }),
        );
        items.extend(
            SystemFont::ALL
                .iter()
                .filter(|f| only.is_none_or(|o| o == f.family()))
                .map(|&font| (FontChoice::System(font), font.label().to_owned())),
        );
        let installed = installed(ui);
        let enabled = |font: &FontChoice| match font {
            FontChoice::System(f) => installed.contains(f),
            _ => true,
        };
        widgets::combo(name)
            .width(WIDTH)
            .selected_text(shown)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show_ui(ui, |ui| {
                let spec = Spec {
                    items: &items,
                    find: "Find a font",
                    none: "No fonts match",
                    width: WIDTH,
                    corners: CornerRadius::same(6),
                };
                let mut group = 0;
                let picked = side_list::dropdown(
                    ui,
                    Id::new((name, "font-search")),
                    spec,
                    &enabled,
                    |ui, font, label| {
                        let next = match font {
                            FontChoice::Default => 0,
                            FontChoice::Bundled(_) => 1,
                            FontChoice::System(_) => 2,
                        };
                        if group != next && next != 0 {
                            let title = if next == 1 {
                                "Bundled"
                            } else {
                                "On this computer"
                            };
                            let t = Tokens::get(ui.ctx());
                            ui.add_space(4.0);
                            ui.label(RichText::new(title).size(t.text_small).color(t.muted));
                        }
                        group = next;
                        ui.add_enabled(
                            enabled(font),
                            Button::selectable(*choice == *font, label).truncate(),
                        )
                        .on_disabled_hover_text("Not installed")
                        .clicked()
                    },
                );
                if let Some(font) = picked {
                    *choice = font;
                    ui.close();
                }
            });
    });
}

fn kind(family: FontFamily) -> Kind {
    match family {
        FontFamily::Serif => Kind::Serif,
        FontFamily::Sans => Kind::Sans,
        FontFamily::Mono => Kind::Mono,
    }
}

fn installed(ui: &Ui) -> Arc<[SystemFont]> {
    let id = Id::new("installed-system-fonts");
    if let Some(known) = ui.data(|d| d.get_temp::<Arc<[SystemFont]>>(id)) {
        return known;
    }
    let generic = |n: &str| matches!(n, "serif" | "sans-serif" | "monospace");
    let found: Arc<[SystemFont]> = SystemFont::ALL
        .iter()
        .copied()
        .filter(|f| f.names().iter().any(|n| generic(n) || has_family(n)))
        .collect();
    ui.data_mut(|d| d.insert_temp(id, Arc::clone(&found)));
    found
}
