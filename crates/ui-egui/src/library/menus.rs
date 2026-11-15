//! Right-click menus on the page, the selection bar, and picked marks.

use crate::icons::Icon;
use crate::theme::{self, Tokens};
use crate::ui_command::{Commands, Sent};
use crate::widgets;
use egui::{
    Button, Color32, Id, Popup, PopupCloseBehavior, Rect, RectAlign, RichText, Sense, Stroke,
    StrokeKind, Ui, pos2, vec2,
};
use mareader_engine::command::Command;
use mareader_engine::library::{Action, BookAction, Named, ShelfAction};
use mareader_library::picks::{Pick, PickAction, Picks};
use mareader_library::{Book, BookId, Library, Link, LinkTo, Move, Shelf, ShelfId, natural};

const ADD_TO_SHELF: &str = "library-add-to-shelf";
const BAR_H: f32 = 44.0;

fn act(out: &mut Commands, action: Action) {
    out.push(Sent::Engine(Command::Library(action)));
}

fn book(out: &mut Commands, action: BookAction) {
    act(out, Action::Book(action));
}

fn shelf(out: &mut Commands, action: ShelfAction) {
    act(out, Action::Shelf(action));
}

/// The file manager's own name, as its menus say it.
fn file_manager() -> &'static str {
    if cfg!(target_os = "macos") {
        "Show in Finder"
    } else if cfg!(windows) {
        "Show in Explorer"
    } else {
        "Show in folder"
    }
}

/// One book's menu.
pub fn book_menu(ui: &mut Ui, b: &Book, _t: &Tokens, out: &mut Commands) {
    let (id, missing) = (b.id, b.missing());
    let open = ui.add_enabled_ui(!missing, |ui| {
        widgets::menu_row(ui, Icon::Book, "Open", None, false)
    });
    if open.inner.clicked() {
        out.push(Sent::Engine(Command::OpenBook(id)));
        ui.close();
    }
    if widgets::menu_row(ui, Icon::SelectSeveral, "Select", None, false).clicked() {
        act(out, Action::Pick(PickAction::Toggle(Pick::Book(id))));
        ui.close();
    }
    widgets::menu_rule(ui);
    let rename = widgets::menu_row(ui, Icon::Rename, "Rename…", None, false)
        .on_hover_text("The name the library shows — the file on disk keeps its own");
    if rename.clicked() {
        act(out, Action::AskRename(Named::Book(id)));
        ui.close();
    }
    let copy = ui
        .add_enabled_ui(!missing, |ui| {
            widgets::menu_row(ui, Icon::Copy, "Duplicate", None, false)
        })
        .inner
        .on_hover_text(
            "A second copy of this book, the library's own — highlights and all — filed beside it",
        );
    if copy.clicked() {
        book(out, BookAction::Duplicate(vec![Pick::Book(id)]));
        ui.close();
    }
    if widgets::menu_row(ui, Icon::SplitRight, "Read separately", None, false).clicked() {
        book(out, BookAction::Twin(id));
        ui.close();
    }
    if b.path().is_some() {
        let reveal = ui.add_enabled_ui(!missing, |ui| {
            widgets::menu_row(ui, Icon::Folder, file_manager(), None, false)
        });
        if reveal.inner.clicked() {
            book(out, BookAction::Reveal(id));
            ui.close();
        }
    }
    if missing && widgets::menu_row(ui, Icon::SearchFolder, "Find again…", None, false).clicked()
    {
        act(out, Action::FindAgain(id));
        ui.close();
    }
    widgets::menu_rule(ui);
    if widgets::menu_row_danger(ui, Icon::Remove, "Remove from library").clicked() {
        ask_remove(&[Pick::Book(id)], out);
        ui.close();
    }
}

/// One link's menu: what it points at stays whatever is done here.
pub fn link_menu(ui: &mut Ui, link: &Link, _t: &Tokens, out: &mut Commands) {
    let pick = Pick::Link(link.id);
    let open = widgets::menu_row(ui, Icon::Open, "Open", None, false)
        .on_hover_text("Show what it points at");
    if open.clicked() {
        act(out, Action::Reveal(super::view::links::target(link)));
        ui.close();
    }
    if widgets::menu_row(ui, Icon::SelectSeveral, "Select", None, false).clicked() {
        act(out, Action::Pick(PickAction::Toggle(pick)));
        ui.close();
    }
    widgets::menu_rule(ui);
    let rename = widgets::menu_row(ui, Icon::Rename, "Rename…", None, false);
    let tip = match link.target {
        LinkTo::Book(_) => "The name the pointer wears — the book it points at keeps its own",
        LinkTo::Shelf(_) => "The name the pointer wears — the shelf it points at keeps its own",
    };
    if rename.on_hover_text(tip).clicked() {
        act(out, Action::AskRename(Named::Link(link.id)));
        ui.close();
    }
    let tip = match link.target {
        LinkTo::Book(_) => "A second copy of the book it points at, filed where the link is",
        LinkTo::Shelf(_) => "A second pointer at the same shelf",
    };
    if widgets::menu_row(ui, Icon::Copy, "Duplicate", None, false)
        .on_hover_text(tip)
        .clicked()
    {
        book(out, BookAction::Duplicate(vec![pick]));
        ui.close();
    }
    widgets::menu_rule(ui);
    if widgets::menu_row_danger(ui, Icon::Remove, "Remove this link").clicked() {
        ask_remove(&[pick], out);
        ui.close();
    }
}

/// One shelf's menu.
pub fn shelf_menu(ui: &mut Ui, model: &Library, s: &Shelf, _t: &Tokens, out: &mut Commands) {
    let id = s.id;
    if widgets::menu_row(ui, Icon::Shelf, "Open shelf", None, false).clicked() {
        shelf(out, ShelfAction::GoTo(Some(id)));
        ui.close();
    }
    if widgets::menu_row(ui, Icon::SelectSeveral, "Select", None, false).clicked() {
        act(out, Action::Pick(PickAction::Toggle(Pick::Shelf(id))));
        ui.close();
    }
    widgets::menu_rule(ui);
    let tip = match s.kind.value.rung() {
        Some(_) => "The name the library shows — a folder on disk keeps its own",
        None => "The name the library shows",
    };
    if widgets::menu_row(ui, Icon::Rename, "Rename…", None, false)
        .on_hover_text(tip)
        .clicked()
    {
        act(out, Action::AskRename(Named::Shelf(id)));
        ui.close();
    }
    duplicate_shelf(ui, id, out);
    if model.shelf_dir(id).is_some()
        && widgets::menu_row(ui, Icon::Folder, file_manager(), None, false).clicked()
    {
        shelf(out, ShelfAction::Reveal(id));
        ui.close();
    }
    watch_entry(ui, model, id, out);
    // Made inside this shelf, which opens on it.
    if widgets::menu_row(ui, Icon::FolderPlus, "New shelf", None, false).clicked() {
        shelf(out, ShelfAction::GoTo(Some(id)));
        shelf(out, ShelfAction::New);
        ui.close();
    }
    widgets::menu_rule(ui);
    if widgets::menu_row_danger(ui, Icon::Remove, "Take shelf apart").clicked() {
        shelf(out, ShelfAction::Apart(id));
        ui.close();
    }
}

/// A shelf's Duplicate entry, in its card menu and its crumb.
pub fn duplicate_shelf(ui: &mut Ui, id: ShelfId, out: &mut Commands) {
    let tip = "A second shelf of your own, holding fresh copies of its books";
    if widgets::menu_row(ui, Icon::Copy, "Duplicate", None, false)
        .on_hover_text(tip)
        .clicked()
    {
        book(out, BookAction::Duplicate(vec![Pick::Shelf(id)]));
        ui.close();
    }
}

/// Watching a folder's shelf: the toggle writes the shelf's own rung.
fn watch_entry(ui: &mut Ui, model: &Library, id: ShelfId, out: &mut Commands) {
    let Some(watch) = model.shelf_watch(id) else {
        return;
    };
    let deep = watch.rung.is_some();
    let label = match (watch.on, deep) {
        (true, true) => "Stop watching this subfolder",
        (true, false) => "Stop watching for new books",
        (false, true) => "Watch this subfolder for new books",
        (false, false) => "Watch for new books",
    };
    let tip = match (watch.on, deep) {
        (true, true) => {
            "Stop checking this subfolder for new books; the books already here stay, \
             and the rest of the tree keeps watching its own"
        }
        (true, false) => "Stop checking this folder; the books already here stay",
        (false, _) => "Check this folder for new books when the app opens or you come back to it",
    };
    let under = match &watch.rung {
        Some(rung) => format!("Only “{rung}” and the folders inside it"),
        None => format!("The whole “{}” folder", watch.folder),
    };
    let icon = if watch.on {
        Icon::Stop
    } else {
        Icon::SearchFolder
    };
    if widgets::menu_row(ui, icon, label, None, false)
        .on_hover_text(tip)
        .clicked()
    {
        shelf(out, ShelfAction::Watch(id, !watch.on));
        ui.close();
    }
    widgets::menu_note(ui, &under);
}

/// The selection's menu, on any picked card.
pub fn picks_menu(ui: &mut Ui, picks: &Picks, _t: &Tokens, out: &mut Commands) {
    let n = picks.len();
    widgets::menu_section(ui, None, &format!("{n} selected"));
    if widgets::menu_row(ui, Icon::FolderPlus, "New shelf from these", None, false).clicked() {
        shelve_together(picks, out);
        ui.close();
    }
    if widgets::menu_row(ui, Icon::Copy, &format!("Duplicate ({n})"), None, false).clicked() {
        book(out, BookAction::Duplicate(picks.items()));
        act(out, Action::Pick(PickAction::Clear));
        ui.close();
    }
    if widgets::menu_row(ui, Icon::Close, "Clear selection", None, false).clicked() {
        act(out, Action::Pick(PickAction::Clear));
        ui.close();
    }
    widgets::menu_rule(ui);
    if widgets::menu_row_danger(ui, Icon::Remove, &format!("Remove ({n})")).clicked() {
        ask_remove(&picks.items(), out);
        ui.close();
    }
}

/// A new shelf where the reader is, holding the selection; selecting ends.
fn shelve_together(picks: &Picks, out: &mut Commands) {
    book(out, BookAction::ShelveTogether(picks.items()));
    act(out, Action::Pick(PickAction::Clear));
}

/// Selecting ends as the remove sheet itemises what's going.
fn ask_remove(picks: &[Pick], out: &mut Commands) {
    if picks.is_empty() {
        return;
    }
    let Move {
        books,
        shelves,
        links,
        ..
    } = Move::of(picks);
    act(out, Action::Pick(PickAction::Clear));
    book(
        out,
        BookAction::AskRemove {
            books,
            shelves,
            links,
        },
    );
}

/// The page's own menu, on empty space; `paused` tells a stopped run.
pub fn page_menu(
    ui: &mut Ui,
    selecting: bool,
    sequence: &[Pick],
    organizer: Organizer<'_>,
    out: &mut Commands,
) {
    if widgets::menu_row(ui, Icon::FolderPlus, "New shelf", None, false).clicked() {
        shelf(out, ShelfAction::New);
        ui.close();
    }
    organize_rows(ui, organizer, out);
    if sequence.is_empty() {
        return;
    }
    widgets::menu_rule(ui);
    let select_all_keys = if cfg!(target_os = "macos") {
        "⌘A"
    } else {
        "Ctrl+A"
    };
    if selecting {
        if widgets::menu_row(ui, Icon::Close, "Clear selection", Some("Esc"), false).clicked() {
            act(out, Action::Pick(PickAction::Clear));
            ui.close();
        }
    } else if widgets::menu_row(
        ui,
        Icon::SelectSeveral,
        "Select all",
        Some(select_all_keys),
        false,
    )
    .clicked()
    {
        act(out, Action::Pick(PickAction::Only(sequence.to_vec())));
        ui.close();
    }
}

/// What the selection bar needs from the page.
pub struct Bar<'a> {
    pub model: &'a Library,
    pub picks: &'a Picks,
    /// The shelf open on the page; the cards shown, in order.
    pub at: Option<ShelfId>,
    pub sequence: &'a [Pick],
    /// The AI is organizing already.
    pub busy: bool,
}

/// The rows that ask the AI to shelve loose books, and what they say.
pub const ORGANIZE: &str = "Organize with AI";
pub const ORGANIZE_TIP: &str = "Let the AI sort imported books on no shelf onto shelves; \
                                you see its plan before anything moves";
pub const ORGANIZING: &str = "The AI is organizing already";
pub const RESUME: &str = "Resume organizing";
pub const RESUME_TIP: &str = "Carry on the stopped run where it left off";
pub const ANEW: &str = "Organize from the start";
pub const ANEW_TIP: &str = "Drop the stopped run and ask the AI again";

/// Organize with AI as menu rows; a stopped run offers resume or restart.
pub fn organize_rows(ui: &mut Ui, organizer: Organizer<'_>, out: &mut Commands) {
    let busy = organizer.busy;
    let mut row = |ui: &mut Ui, icon, label, keys: Option<&str>, tip, action| {
        let r = ui.add_enabled_ui(!busy, |ui| widgets::menu_row(ui, icon, label, keys, false));
        if r.inner
            .on_hover_text(tip)
            .on_disabled_hover_text(ORGANIZING)
            .clicked()
        {
            act(out, action);
            ui.close();
        }
    };
    match organizer.paused {
        Some(said) => {
            let resume = Action::ResumeOrganizing;
            row(ui, Icon::Resume, RESUME, Some(said), RESUME_TIP, resume);
            row(
                ui,
                Icon::Rescan,
                ANEW,
                None,
                ANEW_TIP,
                Action::OrganizeAnew(None),
            );
        }
        None => row(
            ui,
            Icon::Ai,
            ORGANIZE,
            None,
            ORGANIZE_TIP,
            Action::Organize(None),
        ),
    }
}

/// Where the organizer stands, for the menus offering it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Organizer<'a> {
    /// A run is going or its plan waits.
    pub busy: bool,
    /// How far a stopped run got.
    pub paused: Option<&'a str>,
}

/// The bar along the page's foot while selecting.
pub fn selection_bar(ui: &mut Ui, page: Rect, bar: &Bar<'_>, t: &Tokens, out: &mut Commands) {
    let picks = bar.picks;
    if !picks.active() {
        return;
    }
    let n = picks.len();
    let width = page.width().min(560.0);
    let rect = Rect::from_center_size(
        pos2(page.center().x, page.bottom() - BAR_H / 2.0 - 16.0),
        vec2(width, BAR_H),
    );
    // Summoned by a hold: toasts give it the room.
    crate::toast::keep_clear(ui.ctx(), rect);
    let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    // Clicks on the bar's own background stay off the page.
    ui.interact(rect, Id::new("library-selection-bar"), Sense::CLICK);
    let shadow = egui::epaint::Shadow {
        offset: [0, 4],
        blur: 16,
        spread: 0,
        color: t.shadow,
    };
    let p = ui.painter();
    p.add(shadow.as_shape(rect, egui::CornerRadius::from(t.radius_lg)));
    let edge = Stroke::new(1.0, t.line);
    p.rect(rect, t.radius_lg, t.surface, edge, StrokeKind::Inside);
    let inner = rect.shrink2(vec2(12.0, 6.0));
    let row = egui::Layout::left_to_right(egui::Align::Center);
    ui.scope_builder(egui::UiBuilder::new().max_rect(inner).layout(row), |ui| {
        let text = RichText::new(format!("{n} selected")).font(theme::medium(13.0));
        ui.label(text.color(t.muted));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Done").clicked() {
                act(out, Action::Pick(PickAction::Clear));
            }
            let remove = Button::new(RichText::new(format!("Remove ({n})")).color(t.danger));
            let remove = ui.add_enabled(n > 0, remove.frame(false));
            if remove
                .on_hover_text("Remove the selected books and shelves")
                .clicked()
            {
                ask_remove(&picks.items(), out);
            }
            add_to_shelf(ui, bar, out);
            organize_picks(ui, bar, out);
            if ui.button("All").clicked() {
                act(out, Action::Pick(PickAction::Add(bar.sequence.to_vec())));
            }
        });
    });
}

/// Asks the AI to shelve the picked books; it says which it can.
fn organize_picks(ui: &mut Ui, bar: &Bar<'_>, out: &mut Commands) {
    let books: Vec<BookId> = (bar.picks.items().into_iter())
        .filter_map(|p| match p {
            Pick::Book(id) => Some(id),
            Pick::Shelf(_) | Pick::Link(_) => None,
        })
        .collect();
    let on = !bar.busy && !books.is_empty();
    let r = ui.add_enabled_ui(on, |ui| {
        widgets::icon_button(ui, Icon::Ai, "Organize these with AI", false)
    });
    let tip = match () {
        _ if bar.busy => ORGANIZING,
        _ => "Pick books to organize",
    };
    let r = r.inner.on_disabled_hover_text(tip);
    if r.clicked() {
        act(out, Action::Organize(Some(books)));
        act(out, Action::Pick(PickAction::Clear));
    }
}

/// Files the selection on a shelf it can go on, or on a new one.
fn add_to_shelf(ui: &mut Ui, bar: &Bar<'_>, out: &mut Commands) {
    let picks = bar.picks;
    let r = ui.add_enabled(!picks.is_empty(), Button::new("Add to shelf"));
    let r = r.on_hover_text("Add the selected items to a shelf");
    let carried = Move::of(&picks.items());
    let shelves = &carried.shelves;
    // Never itself, nor inside one of its own shelves.
    let fits = |to: ShelfId| {
        let path = bar.model.path_to(to);
        !shelves.iter().any(|s| path.iter().any(|p| p.id == *s))
    };
    let mut choices: Vec<&Shelf> = bar.model.shelves().filter(|s| fits(s.id)).collect();
    choices.sort_by(|a, b| natural(&a.name.value, &b.name.value));
    Popup::from_toggle_button_response(&r)
        .id(Id::new(ADD_TO_SHELF))
        .sense(crate::widgets::POPUP_SENSE)
        .close_behavior(PopupCloseBehavior::CloseOnClick)
        .align(RectAlign::TOP_START)
        .gap(6.0)
        .width(widgets::MENU_W)
        .show(|ui| {
            widgets::menu_style(ui, widgets::MENU_W);
            widgets::menu_section(ui, None, "Add to shelf");
            egui::ScrollArea::vertical()
                .max_height(260.0)
                .show(ui, |ui| {
                    for s in &choices {
                        let row = widgets::menu_row(ui, Icon::Shelf, &s.name.value, None, false);
                        if row.clicked() {
                            let mv = Move {
                                from: bar.at,
                                to: Some(s.id),
                                keep: true,
                                ..carried.clone()
                            };
                            shelf(out, ShelfAction::Carry(mv));
                            act(out, Action::Pick(PickAction::Clear));
                        }
                    }
                });
            widgets::menu_rule(ui);
            if widgets::menu_row(ui, Icon::Plus, "New shelf", None, false).clicked() {
                shelve_together(picks, out);
            }
            widgets::close_on_key(ui);
        });
}

/// A card's pick mark: a filled check if picked, else an empty ring.
pub fn mark(ui: &Ui, center: egui::Pos2, picked: bool, t: &Tokens) {
    let (p, r) = (ui.painter(), 10.0);
    if picked {
        p.circle(center, r, t.accent, Stroke::new(1.5, Color32::WHITE));
        let tick = Rect::from_center_size(center, vec2(13.0, 13.0));
        Icon::Check.paint(ui, tick, Color32::WHITE);
    } else {
        let fill = t.surface.gamma_multiply(0.85);
        p.circle(center, r, fill, Stroke::new(1.5, t.muted));
    }
}

/// The page's look while files from outside hover over it.
pub fn file_hint(ui: &Ui, page: Rect, onto: Option<&str>, t: &Tokens) {
    let p = ui.painter();
    p.rect_filled(page, t.radius_lg, t.accent.gamma_multiply(0.06));
    let ring = Stroke::new(2.0, t.accent.gamma_multiply(0.7));
    p.rect_stroke(page.shrink(4.0), t.radius_lg, ring, StrokeKind::Inside);
    let text = match onto {
        Some(name) => format!("Drop to add to “{name}”"),
        None => "Drop to add to your library".into(),
    };
    let g = p.layout_no_wrap(text, theme::medium(14.0), Color32::WHITE);
    let pill = Rect::from_center_size(
        pos2(page.center().x, page.bottom() - 48.0),
        g.size() + vec2(28.0, 14.0),
    );
    p.rect_filled(pill, pill.height() / 2.0, t.accent);
    p.galley(pill.min + vec2(14.0, 7.0), g, Color32::WHITE);
}

/// The model's shelf name, for the drop hint.
pub fn shelf_name(model: &Library, id: Option<ShelfId>) -> Option<String> {
    id.and_then(|id| model.shelf(id))
        .map(|s| s.name.value.clone())
}
