//! The rail's Library panel: a shelf tree to click or drag onto panes.

use crate::icons::Icon;
use crate::library::Organizer;
use crate::theme::{self, Tokens};
use crate::ui_command::{Commands, LibraryDialog, Sent, UiCommand};
use crate::widgets;
use egui::{
    Align2, Color32, CornerRadius, CursorIcon, DragAndDrop, FontId, FontSelection, Id, Key,
    LayerId, Order, Popup, PopupCloseBehavior, Rect, RichText, ScrollArea, Sense, Stroke,
    TextWrapMode, Ui, WidgetText, pos2, vec2,
};
use mareader_engine::command::Command;
use mareader_engine::documents::Placement;
use mareader_engine::library::{Action, BookAction, Carry, Named, ShelfAction};
use mareader_library::{
    Book, BookId, Format, Home, Item, Library, LinkId, LinkTo, ShelfId, natural,
};
use mareader_settings::LibraryClick;
use mareader_workspace::{Bounds, DropTarget, PaneId};
use std::collections::BTreeSet;

const ROW_H: f32 = 28.0;
const PAD_X: f32 = 8.0;
const INDENT: f32 = 12.0;
const INDENT_MAX: f32 = 96.0;
/// Chevron and gap: a book's icon lines up under its shelf's name.
const CHEVRON_W: f32 = 18.0;
const SHELF_DEPTH_MAX: usize = 32;
/// Seconds a carried row rests on a closed shelf before it opens.
const SPRING: f64 = 0.6;
/// How near the tree's top or bottom a carried row scrolls it.
const SCROLL_EDGE: f32 = 12.0;

/// A row carried from the rail: onto a pane, or within the tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Carried {
    /// The book a pane would open; none for a shelf.
    pub book: Option<BookId>,
    pub item: Item,
    pub title: String,
    /// The shelf its row stood on; none at the top level.
    pub from: Option<ShelfId>,
}

/// One visible row of the tree, in drawing order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line {
    Shelf {
        id: ShelfId,
        name: String,
        depth: usize,
        open: bool,
        /// Books on it and under it.
        count: usize,
    },
    /// A book, or a link to one shown under its own name.
    Book {
        id: BookId,
        title: String,
        format: Format,
        depth: usize,
        missing: bool,
        /// The shelf the row stands on: a book may show on many.
        level: Option<ShelfId>,
        link: Option<LinkId>,
    },
}

impl Line {
    /// What a rename or a removal names: a link under its own name.
    pub fn named(&self) -> Named {
        match self {
            Line::Shelf { id, .. } => Named::Shelf(*id),
            Line::Book { link: Some(l), .. } => Named::Link(*l),
            Line::Book { id, .. } => Named::Book(*id),
        }
    }

    fn item(&self) -> Item {
        match self.named() {
            Named::Shelf(id) => Item::Shelf(id),
            Named::Book(id) => Item::Book(id),
            Named::Link(id) => Item::Link(id),
        }
    }

    fn name(&self) -> &str {
        match self {
            Line::Shelf { name, .. } => name,
            Line::Book { title, .. } => title,
        }
    }

    fn depth(&self) -> usize {
        match self {
            Line::Shelf { depth, .. } | Line::Book { depth, .. } => *depth,
        }
    }

    /// Where it stands: a shelf in its parent, a book on its level.
    fn level(&self, model: &Library) -> Option<ShelfId> {
        match self {
            Line::Shelf { id, .. } => model.shelf(*id).and_then(|s| model.parent_of(s)),
            Line::Book { level, .. } => *level,
        }
    }
}

/// Dropping `carried` on `target`; none if a no-op or a loop.
pub fn drop_action(
    model: &Library,
    carried: &Carried,
    target: Option<ShelfId>,
    keep: bool,
) -> Option<Action> {
    if let Item::Shelf(shelf) = carried.item
        && (carried.from == target || within(model, target, shelf))
    {
        return None;
    }
    let carry = Carry {
        items: vec![carried.item],
        from: carried.from,
    };
    carry.onto(target, keeps(carried, keep))
}

/// Whether Alt keeps a book on its shelf too: only a shelved book can.
fn keeps(carried: &Carried, alt: bool) -> bool {
    alt && carried.from.is_some() && matches!(carried.item, Item::Book(_))
}

/// Whether `at` is `shelf` or somewhere inside it.
fn within(model: &Library, mut at: Option<ShelfId>, shelf: ShelfId) -> bool {
    for _ in 0..=SHELF_DEPTH_MAX {
        match at {
            Some(id) if id == shelf => return true,
            Some(id) => at = model.shelf(id).and_then(|s| model.parent_of(s)),
            None => return false,
        }
    }
    false
}

/// Rows with `open` shelves expanded: shelves, then books, by name.
pub fn lines(model: &Library, open: &BTreeSet<ShelfId>, home: Home) -> Vec<Line> {
    let mut out = Vec::new();
    let shelved = model.shelved();
    let top: Vec<&Book> = match home {
        Home::Everything => model.books().collect(),
        Home::Unshelved => model.books().filter(|b| !shelved.contains(&b.id)).collect(),
    };
    push_level(model, None, top, 0, open, &mut out);
    out
}

fn push_level(
    model: &Library,
    shelf: Option<ShelfId>,
    books: Vec<&Book>,
    depth: usize,
    open: &BTreeSet<ShelfId>,
    out: &mut Vec<Line>,
) {
    if depth > SHELF_DEPTH_MAX {
        return;
    }
    for child in model.children(shelf) {
        let is_open = open.contains(&child.id);
        out.push(Line::Shelf {
            id: child.id,
            name: child.name.value.clone(),
            depth,
            open: is_open,
            count: count_under(model, child.id, depth),
        });
        if is_open {
            let books = model.shelf_books(child.id);
            push_level(model, Some(child.id), books, depth + 1, open, out);
        }
    }
    let line = |b: &Book, title: &str, link| Line::Book {
        id: b.id,
        title: title.to_owned(),
        format: b.format,
        depth,
        missing: b.missing(),
        level: shelf,
        link,
    };
    let mut rows: Vec<Line> = (books.into_iter())
        .map(|b| line(b, &b.title.value, None))
        .collect();
    // A link to a gone book, or to a shelf, has no row here.
    rows.extend(linked(model, shelf).map(|(l, b)| line(b, &l.name.value, Some(l.id))));
    rows.sort_by(|a, b| match (a, b) {
        (
            Line::Book {
                title: x, id: i, ..
            },
            Line::Book {
                title: y, id: j, ..
            },
        ) => natural(x, y).then(i.cmp(j)),
        _ => std::cmp::Ordering::Equal,
    });
    out.extend(rows);
}

/// The book links standing on `level`, with the books they open.
fn linked(
    model: &Library,
    level: Option<ShelfId>,
) -> impl Iterator<Item = (&mareader_library::Link, &Book)> {
    (model.links())
        .filter(move |l| l.level.value == level)
        .filter_map(|l| match l.target {
            LinkTo::Book(b) => Some((l, model.book(b)?)),
            LinkTo::Shelf(_) => None,
        })
}

/// Books on `shelf` and its shelves, each counted once, and links.
fn count_under(model: &Library, shelf: ShelfId, depth: usize) -> usize {
    let (mut seen, mut links) = (BTreeSet::new(), 0);
    let mut stack = vec![(shelf, depth)];
    while let Some((at, d)) = stack.pop() {
        seen.extend(model.shelf_books(at).into_iter().map(|b| b.id));
        links += linked(model, Some(at)).count();
        if d < SHELF_DEPTH_MAX {
            stack.extend(model.children(Some(at)).into_iter().map(|c| (c.id, d + 1)));
        }
    }
    seen.len() + links
}

/// One open pane, as the strip lists it.
#[derive(Clone, Debug)]
pub struct PaneTab {
    pub pane: PaneId,
    pub name: String,
    pub book: Option<BookId>,
}

/// What the panel reads from the app this frame.
pub struct PanelContext<'a> {
    pub model: &'a Library,
    /// Every pane, in the tree's order.
    pub tabs: &'a [PaneTab],
    pub active: PaneId,
    pub click: LibraryClick,
    pub home: Home,
    /// Books the AI is organizing: they open, but stay put.
    pub held: &'a BTreeSet<BookId>,
    pub organizer: Organizer<'a>,
}

/// A row being named in place; Enter or a click away keeps the name.
#[derive(Debug)]
struct Renaming {
    named: Named,
    text: String,
    /// Not yet focused: its text gets selected whole.
    fresh: bool,
}

/// What the panel keeps between frames.
#[derive(Debug, Default)]
pub struct PanelState {
    open: BTreeSet<ShelfId>,
    /// The rows, kept until the library, the open set or Home change.
    cache: Option<((u64, Home), Vec<Line>)>,
    renaming: Option<Renaming>,
    /// The closed shelf a carried row rests on, and since when.
    spring: Option<(ShelfId, f64)>,
}

impl PanelState {
    fn lines(&mut self, model: &Library, home: Home) -> &[Line] {
        let key = (model.revision(), home);
        let open = &self.open;
        let stale = self.cache.as_ref().is_none_or(|(k, _)| *k != key);
        if stale {
            self.cache = Some((key, lines(model, open, home)));
        }
        self.cache.as_ref().map_or(&[], |(_, l)| l.as_slice())
    }

    pub fn set_open(&mut self, shelf: ShelfId, open: bool) {
        let changed = match open {
            true => self.open.insert(shelf),
            false => self.open.remove(&shelf),
        };
        if changed {
            self.cache = None;
        }
    }

    pub fn is_open(&self, shelf: ShelfId) -> bool {
        self.open.contains(&shelf)
    }

    /// Starts naming `named` in its row, `text` selected whole.
    pub fn rename(&mut self, named: Named, text: &str) {
        self.renaming = Some(Renaming {
            named,
            text: text.to_owned(),
            fresh: true,
        });
    }

    /// What's being named in place, if anything.
    pub fn renaming(&self) -> Option<Named> {
        self.renaming.as_ref().map(|r| r.named)
    }

    /// A shelf the tree just made: its parent opens and it's named.
    pub fn made(&mut self, model: &Library, shelf: ShelfId) {
        let mut at = model.shelf(shelf).and_then(|s| model.parent_of(s));
        for _ in 0..=SHELF_DEPTH_MAX {
            let Some(id) = at else {
                break;
            };
            self.set_open(id, true);
            at = model.shelf(id).and_then(|s| model.parent_of(s));
        }
        let name = model.shelf(shelf).map_or("", |s| s.name.value.as_str());
        self.rename(Named::Shelf(shelf), name);
    }
}

/// One row's place, for drops: its rect and the shelf it files onto.
struct Spot {
    rect: Rect,
    target: Option<ShelfId>,
    /// The shelf the row is, so a resting drag can open it.
    shelf: Option<(ShelfId, bool)>,
}

/// Draws the panel; true, as it always has something to say.
pub fn show(
    ui: &mut Ui,
    cx: &PanelContext<'_>,
    state: &mut PanelState,
    out: &mut Commands,
) -> bool {
    let t = Tokens::get(ui.ctx());
    ui.spacing_mut().item_spacing.y = 0.0;
    if cx.tabs.len() > 1 {
        heading(ui, "Open", cx.tabs.len(), &t);
        for tab in cx.tabs {
            tab_row(ui, tab, tab.pane == cx.active, &t, out);
        }
        ui.add_space(6.0);
        let y = ui.cursor().top();
        let x = ui.max_rect().x_range();
        ui.painter().hline(x, y, Stroke::new(1.0, t.line));
    }
    let head = library_heading(ui, cx, &t, out);
    if cx.model.is_empty() {
        let hint = "Books you add on the Library page appear here.";
        widgets::panel_empty(ui, Some(Icon::Library), "Your library is empty", hint);
        return true;
    }
    let reading: Option<BookId> = cx
        .tabs
        .iter()
        .find(|tab| tab.pane == cx.active)
        .and_then(|tab| tab.book);
    let open: BTreeSet<BookId> = cx.tabs.iter().filter_map(|tab| tab.book).collect();
    let carried = DragAndDrop::payload::<Carried>(ui.ctx());
    let dragged = carried.as_ref().map(|c| c.item);
    let pointer = ui.ctx().pointer_latest_pos();
    let lines = state.lines(cx.model, cx.home).to_vec();
    let width = ui.available_width();
    let area = ui.available_rect_before_wrap();
    // Under the rows: the library's menu, and the top level for drops.
    let back = ui.interact(area, Id::new("library-panel-back"), Sense::click());
    widgets::context_menu(&back, |ui| library_menu(ui, cx.organizer, out));
    let mut toggles = Vec::new();
    let mut spots: Vec<Spot> = Vec::new();
    let mut named: Option<(Named, String)> = None;
    let mut finished: Option<bool> = None;
    ScrollArea::vertical()
        .id_salt("library-panel")
        .auto_shrink(false)
        .show_rows(ui, ROW_H, lines.len(), |ui, range| {
            if carried.is_some() {
                edge_scroll(ui, pointer);
            }
            for line in lines.get(range).unwrap_or_default() {
                let (rect, _) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::hover());
                let shelf = match line {
                    Line::Shelf { id, open, .. } => Some((*id, *open)),
                    Line::Book { .. } => None,
                };
                spots.push(Spot {
                    rect,
                    target: shelf.map(|(id, _)| id).or(line.level(cx.model)),
                    shelf,
                });
                if let Some(r) = state.renaming.as_mut().filter(|r| r.named == line.named()) {
                    if let Some(keep) = rename_row(ui, rect, line, r) {
                        finished = Some(keep);
                    }
                    continue;
                }
                let resp = match line {
                    Line::Shelf { id, open, .. } => {
                        let (resp, toggle) = shelf_row(ui, rect, line, dragged, &t);
                        if let Some(want) = toggle {
                            toggles.push((*id, want.unwrap_or(!*open)));
                        }
                        resp
                    }
                    Line::Book { id, link, .. } => {
                        let look = BookLook {
                            active: reading == Some(*id),
                            open: open.contains(id),
                            dragged: dragged == Some(line.item())
                                || link.is_none() && dragged == Some(Item::Book(*id)),
                            held: cx.held.contains(id),
                        };
                        book_row(ui, rect, line, look, cx.click, &t, out)
                    }
                };
                row_actions(ui, &resp, line, cx.model, &mut named, out);
            }
        });
    for (shelf, want) in toggles {
        state.set_open(shelf, want);
    }
    if let Some(keep) = finished {
        finish_rename(state, cx.model, keep, out);
    }
    if let Some((what, text)) = named {
        state.rename(what, &text);
    }
    if let Some(carried) = carried {
        let over = pointer.filter(|p| area.contains(*p) || head.contains(*p));
        match over {
            Some(at) => tree_drop(ui, cx, state, (&carried, at), (&spots, head), out),
            None => state.spring = None,
        }
    }
    true
}

/// Scrolls the tree while a carried row nears its top or bottom.
fn edge_scroll(ui: &mut Ui, pointer: Option<egui::Pos2>) {
    let clip = ui.clip_rect();
    let Some(p) = pointer.filter(|p| clip.x_range().contains(p.x)) else {
        return;
    };
    let dy = match p.y {
        y if (clip.top() - 4.0..clip.top() + SCROLL_EDGE).contains(&y) => 6.0,
        y if (clip.bottom() - SCROLL_EDGE..clip.bottom() + 4.0).contains(&y) => -6.0,
        _ => return,
    };
    ui.scroll_with_delta(vec2(0.0, dy));
    ui.ctx().request_repaint();
}

/// A row carried over the tree: lit target, chip, spring, drop.
fn tree_drop(
    ui: &Ui,
    cx: &PanelContext<'_>,
    state: &mut PanelState,
    (carried, at): (&Carried, egui::Pos2),
    (spots, head): (&[Spot], Rect),
    out: &mut Commands,
) {
    let ctx = ui.ctx();
    ctx.set_cursor_icon(CursorIcon::Grabbing);
    let spot = spots.iter().find(|s| s.rect.contains(at));
    let target = spot.and_then(|s| s.target);
    let keep = ui.input(|i| i.modifiers.alt);
    let action = drop_action(cx.model, carried, target, keep);
    let t = Tokens::get(ctx);
    let layer = LayerId::new(Order::Foreground, Id::new("library-panel-tree-drop"));
    let painter = ctx.layer_painter(layer).with_clip_rect(ui.clip_rect());
    let name = match target {
        Some(id) => cx.model.shelf(id).map_or("", |s| s.name.value.as_str()),
        None => "",
    };
    let words = match (&action, target) {
        (None, _) => "Already here".to_owned(),
        (Some(_), None) => "Move to the top level".to_owned(),
        (Some(_), Some(_)) if keeps(carried, keep) => format!("Also on {name}"),
        (Some(_), Some(_)) => format!("Move to {name}"),
    };
    if action.is_some() {
        // The shelf it lands on, or the heading for the top level.
        let lit = match target {
            Some(id) => spots
                .iter()
                .find(|s| s.shelf.is_some_and(|(s, _)| s == id))
                .map(|s| s.rect),
            None => Some(head),
        };
        if let Some(lit) = lit {
            let lit = lit.shrink2(vec2(6.0, 1.0));
            painter.rect_filled(lit, 6.0, t.accent.gamma_multiply(0.14));
            let stroke = Stroke::new(1.5, t.accent);
            painter.rect_stroke(lit, 6.0, stroke, egui::StrokeKind::Inside);
        }
    }
    chip(
        &painter,
        at + vec2(14.0, 16.0),
        &format!("{} · {words}", carried.title),
        &t,
        false,
    );
    // Resting on a closed shelf opens it, as file managers do.
    let now = ui.input(|i| i.time);
    match spot.and_then(|s| s.shelf) {
        Some((id, false)) => match state.spring {
            Some((at_id, since)) if at_id == id => {
                if now - since >= SPRING {
                    state.set_open(id, true);
                    state.spring = None;
                } else {
                    ctx.request_repaint_after(std::time::Duration::from_secs_f64(SPRING / 4.0));
                }
            }
            _ => {
                state.spring = Some((id, now));
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(SPRING / 4.0));
            }
        },
        _ => state.spring = None,
    }
    if ui.input(|i| i.pointer.any_released()) {
        DragAndDrop::clear_payload(ctx);
        state.spring = None;
        if let Some(action) = action {
            out.push(Sent::Engine(Command::Library(action)));
        }
    }
}

/// A row's own menu, keys and drag; F2 names it, Delete asks to remove.
fn row_actions(
    ui: &Ui,
    resp: &egui::Response,
    line: &Line,
    model: &Library,
    named: &mut Option<(Named, String)>,
    out: &mut Commands,
) {
    let missing = matches!(line, Line::Book { missing: true, .. });
    if resp.drag_started() && !missing {
        let book = match line {
            Line::Book { id, .. } => Some(*id),
            Line::Shelf { .. } => None,
        };
        resp.dnd_set_drag_payload(Carried {
            book,
            item: line.item(),
            title: line.name().to_owned(),
            from: line.level(model),
        });
    }
    if resp.has_focus() {
        let (f2, delete) = ui.input(|i| {
            (
                i.key_pressed(Key::F2),
                i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace),
            )
        });
        if f2 {
            *named = Some((line.named(), line.name().to_owned()));
        }
        if delete {
            out.push(Sent::Engine(Command::Library(Action::Book(ask_remove(
                line.named(),
            )))));
        }
    }
    widgets::context_menu(resp, |ui| row_menu(ui, line, model, named, out));
}

/// The menu on a shelf or book row.
fn send_and_close(ui: &mut Ui, out: &mut Commands, sent: impl Into<Sent>) {
    out.push(sent.into());
    ui.close();
}

fn row_menu(
    ui: &mut Ui,
    line: &Line,
    model: &Library,
    named: &mut Option<(Named, String)>,
    out: &mut Commands,
) {
    let mut show_library = false;
    if let Line::Shelf { id, .. } = line {
        if widgets::menu_row(ui, Icon::FolderPlus, "New shelf inside", None, false).clicked() {
            let act = Action::Shelf(ShelfAction::NewIn(Some(*id)));
            send_and_close(ui, out, Command::Library(act));
        }
        if widgets::menu_row(ui, Icon::Import, "Import books here…", None, false).clicked() {
            let cmd = UiCommand::LibraryDialog(LibraryDialog::Into(Some(*id)));
            send_and_close(ui, out, cmd);
        }
        widgets::menu_rule(ui);
    }
    if widgets::menu_row(ui, Icon::Rename, "Rename", Some("F2"), false).clicked() {
        *named = Some((line.named(), line.name().to_owned()));
        ui.close();
    }
    if let Line::Book {
        level: Some(level), ..
    } = line
    {
        let shelf = model.shelf(*level).map_or("", |s| s.name.value.as_str());
        let off = Carry {
            items: vec![line.item()],
            from: Some(*level),
        };
        let label = format!("Take off “{shelf}”");
        if let Some(action) = off.onto(None, false)
            && widgets::menu_row(ui, Icon::MoveUp, &label, None, false).clicked()
        {
            send_and_close(ui, out, Command::Library(action));
        }
    }
    if widgets::menu_row(ui, Icon::Library, "Show in Library", None, false).clicked() {
        send_and_close(ui, out, Command::Library(Action::Reveal(line.item())));
        show_library = true;
    }
    widgets::menu_rule(ui);
    if widgets::menu_row_danger(ui, Icon::Remove, "Remove…").clicked() {
        let act = Action::Book(ask_remove(line.named()));
        send_and_close(ui, out, Command::Library(act));
    }
    if show_library {
        out.push(Sent::Ui(UiCommand::ShowLibrary));
    }
}

/// Removing one row's book, shelf or link, asking first.
fn ask_remove(named: Named) -> BookAction {
    let (mut books, mut shelves, mut links) = (Vec::new(), Vec::new(), Vec::new());
    match named {
        Named::Book(id) => books.push(id),
        Named::Shelf(id) => shelves.push(id),
        Named::Link(id) => links.push(id),
    }
    BookAction::AskRemove {
        books,
        shelves,
        links,
    }
}

/// The library's menu: new shelf, imports, and the AI organizer.
fn library_menu(ui: &mut Ui, organizer: Organizer<'_>, out: &mut Commands) {
    widgets::menu_style(ui, widgets::MENU_W);
    if widgets::menu_row(ui, Icon::FolderPlus, "New shelf", None, false).clicked() {
        let act = Action::Shelf(ShelfAction::NewIn(None));
        send_and_close(ui, out, Command::Library(act));
    }
    if widgets::menu_row(ui, Icon::Import, "Import books…", None, false).clicked() {
        send_and_close(ui, out, UiCommand::LibraryDialog(LibraryDialog::Into(None)));
    }
    if widgets::menu_row(ui, Icon::Folder, "Import a folder…", None, false).clicked() {
        send_and_close(ui, out, UiCommand::LibraryDialog(LibraryDialog::Folder));
    }
    widgets::menu_rule(ui);
    crate::library::organize_rows(ui, organizer, out);
    widgets::close_on_key(ui);
}

/// The Library heading: its count, and a button for the library's menu.
fn library_heading(ui: &mut Ui, cx: &PanelContext<'_>, t: &Tokens, out: &mut Commands) -> Rect {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
    let y = rect.bottom() - 9.0;
    let button = Rect::from_center_size(pos2(rect.right() - PAD_X - 8.0, y), vec2(22.0, 22.0));
    let painter = ui.painter();
    let left = pos2(rect.left() + PAD_X + 4.0, y);
    painter.text(
        left,
        Align2::LEFT_CENTER,
        "Library",
        theme::medium(12.0),
        t.muted,
    );
    let right = pos2(button.left() - 6.0, y);
    let small = FontId::proportional(11.0);
    let count = cx.model.len().to_string();
    painter.text(right, Align2::RIGHT_CENTER, count, small, t.muted);
    let resp = ui.interact(button, Id::new("library-panel-add"), Sense::click());
    let label = "Library actions";
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let id = Id::new("library-panel-menu");
    let open = Popup::is_id_open(ui.ctx(), id);
    if resp.hovered() || open {
        ui.painter()
            .rect_filled(button, CornerRadius::same(5), t.hover);
    }
    Icon::Plus.paint(ui, button.shrink(4.0), if open { t.ink } else { t.muted });
    let resp = resp.on_hover_text("New shelf, import or organize");
    Popup::from_toggle_button_response(&resp)
        .id(id)
        .sense(widgets::POPUP_SENSE)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .width(widgets::MENU_W)
        .gap(4.0)
        .show(|ui| library_menu(ui, cx.organizer, out));
    rect
}

/// The name field over a row; true keeps the name, false drops it.
fn rename_row(ui: &mut Ui, rect: Rect, line: &Line, r: &mut Renaming) -> Option<bool> {
    let row = rect.shrink2(vec2(6.0, 1.0));
    let lead = match line {
        Line::Shelf { .. } => CHEVRON_W + 20.0,
        Line::Book { .. } => CHEVRON_W + 4.0,
    };
    let left = row.left() + indent(line.depth()) + lead - 5.0;
    let field = Rect::from_min_max(
        pos2(left, row.top() + 2.0),
        pos2(row.right() - 4.0, row.bottom() - 2.0),
    );
    let id = Id::new("library-panel-rename");
    let edit = egui::TextEdit::singleline(&mut r.text)
        .id(id)
        .font(FontId::proportional(13.0))
        .margin(egui::Margin::symmetric(4, 2))
        .desired_width(field.width())
        .char_limit(mareader_library::NAME_MAX);
    let builder = egui::UiBuilder::new().max_rect(field);
    let mut output = ui.scope_builder(builder, |ui| edit.show(ui)).inner;
    output
        .response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Name"));
    if r.fresh {
        r.fresh = false;
        output.response.request_focus();
        let end = egui::text::CCursor::new(r.text.chars().count());
        let all = egui::text::CCursorRange::two(egui::text::CCursor::new(0), end);
        output.state.cursor.set_char_range(Some(all));
        output.state.store(ui.ctx(), id);
        return None;
    }
    if ui.input(|i| i.key_pressed(Key::Escape)) {
        return Some(false);
    }
    output.response.lost_focus().then_some(true)
}

/// Ends naming in place; a kept, changed, non-blank name is given.
fn finish_rename(state: &mut PanelState, model: &Library, keep: bool, out: &mut Commands) {
    let Some(r) = state.renaming.take() else {
        return;
    };
    let now = match r.named {
        Named::Book(id) => model.book(id).map(|b| b.title.value.as_str()),
        Named::Shelf(id) => model.shelf(id).map(|s| s.name.value.as_str()),
        Named::Link(id) => model.link(id).map(|l| l.name.value.as_str()),
    };
    let name = r.text.trim();
    if keep && !name.is_empty() && now.is_some_and(|n| n != name) {
        out.push(Sent::Engine(Command::Library(Action::Rename(
            r.named,
            name.to_owned(),
        ))));
    }
}

/// A sentence-case heading with a count at the right.
fn heading(ui: &mut Ui, title: &str, count: usize, t: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
    let painter = ui.painter();
    let font = theme::medium(12.0);
    let y = rect.bottom() - 9.0;
    let left = pos2(rect.left() + PAD_X + 4.0, y);
    painter.text(left, Align2::LEFT_CENTER, title, font, t.muted);
    let right = pos2(rect.right() - PAD_X - 4.0, y);
    let small = FontId::proportional(11.0);
    painter.text(
        right,
        Align2::RIGHT_CENTER,
        count.to_string(),
        small,
        t.muted,
    );
}

fn tab_row(ui: &mut Ui, tab: &PaneTab, active: bool, t: &Tokens, out: &mut Commands) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
    let row = rect.shrink2(vec2(6.0, 1.0));
    let id = Id::new(("library-panel-tab", tab.pane));
    let resp = ui.interact(row, id, Sense::click());
    let name = &tab.name;
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, active, name));
    let painter = ui.painter();
    if active {
        painter.rect_filled(row, CornerRadius::same(6), t.hover);
    } else if resp.hovered() {
        painter.rect_filled(row, CornerRadius::same(6), t.hover.gamma_multiply(0.6));
    }
    let close = Rect::from_center_size(pos2(row.right() - 14.0, row.center().y), vec2(20.0, 20.0));
    let room = row.width() - 46.0;
    let colour = if active { t.ink } else { t.muted };
    let font = if active {
        theme::medium(13.0)
    } else {
        FontId::proportional(13.0)
    };
    let at = pos2(row.left() + 10.0, row.center().y);
    text(ui, at, name, font, colour, room);
    let x = ui.interact(close, id.with("close"), Sense::click());
    let label = format!("Close {name}");
    x.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    if x.hovered() {
        ui.painter()
            .rect_filled(close, CornerRadius::same(5), t.pressed);
    }
    if x.hovered() || resp.hovered() || active {
        Icon::Close.paint(ui, close.shrink(5.0), t.muted);
    }
    if x.clicked() {
        out.push(Sent::Engine(Command::ClosePane(tab.pane)));
    } else if resp.clicked() {
        out.push(Sent::Engine(Command::FocusPane(tab.pane)));
    }
}

/// A shelf row and its fold: `Some(None)` toggles, `Some(Some(open))` sets.
fn shelf_row(
    ui: &Ui,
    rect: Rect,
    line: &Line,
    dragged: Option<Item>,
    t: &Tokens,
) -> (egui::Response, Option<Option<bool>>) {
    let Line::Shelf {
        id,
        name,
        depth,
        open,
        count,
    } = line
    else {
        return (
            ui.interact(rect, Id::new("library-panel-none"), Sense::hover()),
            None,
        );
    };
    let row = rect.shrink2(vec2(6.0, 1.0));
    let key = Id::new(("library-panel-shelf", id));
    let resp = ui.interact(row, key, Sense::click_and_drag());
    let ink = match dragged == Some(Item::Shelf(*id)) {
        true => t.ink.gamma_multiply(0.4),
        false => t.ink,
    };
    resp.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name);
        info.selected = Some(*open);
        info
    });
    if resp.hovered() || resp.has_focus() {
        ui.painter()
            .rect_filled(row, CornerRadius::same(6), t.hover.gamma_multiply(0.6));
    }
    let mut x = row.left() + indent(*depth);
    let chevron = if *open {
        Icon::ChevronDown
    } else {
        Icon::ChevronRight
    };
    let icon = |x: f32, size: f32| {
        Rect::from_center_size(pos2(x + size / 2.0, row.center().y), vec2(size, size))
    };
    chevron.paint(ui, icon(x, 12.0), t.muted);
    x += CHEVRON_W;
    Icon::Shelf.paint(ui, icon(x, 14.0), t.muted);
    x += 20.0;
    let number = count.to_string();
    let small = FontId::proportional(11.0);
    let right = pos2(row.right() - 8.0, row.center().y);
    ui.painter()
        .text(right, Align2::RIGHT_CENTER, number, small, t.muted);
    let room = row.right() - 36.0 - x;
    text(
        ui,
        pos2(x, row.center().y),
        name,
        FontId::proportional(13.0),
        ink,
        room,
    );
    if resp.has_focus() {
        // Left and Right fold the shelf instead of moving focus away.
        let filter = egui::EventFilter {
            horizontal_arrows: true,
            ..Default::default()
        };
        ui.memory_mut(|m| m.set_focus_lock_filter(resp.id, filter));
        let keys = ui.input(|i| {
            (
                i.key_pressed(Key::ArrowRight),
                i.key_pressed(Key::ArrowLeft),
            )
        });
        match keys {
            (true, _) => return (resp, Some(Some(true))),
            (_, true) => return (resp, Some(Some(false))),
            _ => {}
        }
    }
    let toggle = resp.clicked().then_some(None);
    (resp, toggle)
}

#[derive(Clone, Copy)]
struct BookLook {
    /// The focused pane reads it.
    active: bool,
    /// Some pane reads it.
    open: bool,
    dragged: bool,
    /// The AI is organizing it.
    held: bool,
}

fn book_row(
    ui: &Ui,
    rect: Rect,
    line: &Line,
    look: BookLook,
    click: LibraryClick,
    t: &Tokens,
    out: &mut Commands,
) -> egui::Response {
    let Line::Book {
        id,
        title,
        format,
        depth,
        missing,
        level,
        link,
    } = line
    else {
        return ui.interact(rect, Id::new("library-panel-none"), Sense::hover());
    };
    let row = rect.shrink2(vec2(6.0, 1.0));
    // A missing book is shown, never opened or carried.
    let sense = match missing {
        true => Sense::hover(),
        false => Sense::click_and_drag(),
    };
    let key = Id::new(("library-panel-book", id, level, link));
    let resp = ui.interact(row, key, sense);
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, !missing, look.active, title)
    });
    let fade = if *missing || look.dragged { 0.4 } else { 1.0 };
    let painter = ui.painter();
    if look.active {
        painter.rect_filled(row, CornerRadius::same(6), t.hover);
    } else if resp.hovered() || resp.has_focus() {
        painter.rect_filled(row, CornerRadius::same(6), t.hover.gamma_multiply(0.6));
    }
    let ink = |c: Color32| c.gamma_multiply(fade);
    // The format badge tells books apart; no icon repeats on every row.
    let x = row.left() + indent(*depth) + CHEVRON_W + 4.0;
    let badge = badge_rect(ui, row, format.label(), t, fade);
    let dot = look.open && !look.active;
    let mut room = badge.left() - x - if dot { 14.0 } else { 6.0 };
    // The AI is finding it a shelf: the cards' breathing dots.
    if look.held {
        let w = crate::library::view::DOTS_W;
        let at = pos2(badge.left() - w - 4.0, row.center().y);
        crate::library::view::dots(ui, at, t.accent);
        room -= w + 8.0;
    }
    let (font, colour) = match (look.active, look.open || resp.hovered()) {
        (true, _) => (theme::medium(13.0), t.ink),
        (false, true) => (FontId::proportional(13.0), t.ink),
        (false, false) => (FontId::proportional(13.0), t.muted),
    };
    let elided = text(ui, pos2(x, row.center().y), title, font, ink(colour), room);
    if dot {
        let c = pos2(badge.left() - 9.0, row.center().y);
        ui.painter()
            .circle_filled(c, 3.0, t.accent.gamma_multiply(0.7));
    }
    let tip = match missing {
        true => Some(format!("{title} — missing; relink it on the Library page")),
        false if look.held => Some(format!(
            "{title} — the AI is organizing it; it opens as usual but stays put"
        )),
        false => elided.then(|| title.clone()),
    };
    let clicked = resp.clicked();
    let resp = match tip {
        Some(tip) => resp.on_hover_text(tip),
        None => resp,
    };
    if !clicked {
        return resp;
    }
    // A key press has no pointer click; Do nothing still lets keys open.
    let keyboard = !ui.input(|i| i.pointer.any_click());
    let placement = match click {
        _ if *missing => return resp,
        LibraryClick::Replace => Placement::Into,
        LibraryClick::Split => Placement::Beside,
        LibraryClick::DragOnly if keyboard => Placement::Beside,
        LibraryClick::DragOnly => return resp,
    };
    out.push(Sent::Engine(Command::PlaceBook {
        book: *id,
        pane: None,
        placement,
    }));
    resp
}

/// The format tag at the row's right, painted; its rect.
fn badge_rect(ui: &Ui, row: Rect, label: &str, t: &Tokens, fade: f32) -> Rect {
    let font = theme::medium(9.5);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, t.muted.gamma_multiply(fade));
    let size = vec2(galley.size().x + 8.0, 16.0);
    let rect = Rect::from_min_size(pos2(row.right() - 6.0 - size.x, row.center().y - 8.0), size);
    let painter = ui.painter();
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0, t.line),
        egui::StrokeKind::Inside,
    );
    let at = rect.center() - galley.size() / 2.0;
    painter.galley(at, galley, t.muted);
    rect
}

/// One line of text, cut to `room`; true if it was cut.
fn text(ui: &Ui, left: egui::Pos2, s: &str, font: FontId, colour: Color32, room: f32) -> bool {
    if crate::shaped::needs_shaping(s) {
        let band = Rect::from_min_size(pos2(left.x, left.y - 12.0), vec2(room, 24.0));
        // Off screen it's neither drawn nor shaped.
        if !ui.is_rect_visible(band) {
            return false;
        }
        if let Some(drawn) = crate::shaped::block(ui.ctx(), s, font.size, room.max(16.0), 1) {
            drawn.paint(
                ui.painter(),
                pos2(left.x, left.y - drawn.size.y / 2.0),
                colour,
            );
            return drawn.cut;
        }
    }
    let text = WidgetText::from(RichText::new(s).font(font).color(colour));
    let galley = text.into_galley(
        ui,
        Some(TextWrapMode::Truncate),
        room.max(16.0),
        FontSelection::Default,
    );
    let elided = galley.elided;
    let at = pos2(left.x, left.y - galley.size().y / 2.0);
    ui.painter().galley(at, galley, colour);
    elided
}

fn indent(depth: usize) -> f32 {
    PAD_X + (depth as f32 * INDENT).min(INDENT_MAX)
}

/// Where a carried book may land on the panes.
pub struct Landing<'a> {
    pub panes: &'a [(PaneId, Rect)],
    /// The rail's right edge: the tree takes drops left of it.
    pub rail: Option<f32>,
    /// The workspace holds all the panes it can: no split, only a fill.
    pub full: bool,
}

/// Lights the pane a carried book would land in; the rail is the tree's.
pub fn drop_overlay(
    ui: &Ui,
    landing: &Landing<'_>,
    empty: impl Fn(PaneId) -> bool,
    name: impl Fn(PaneId) -> String,
    out: &mut Commands,
) {
    let ctx = ui.ctx();
    let Some(carried) = DragAndDrop::payload::<Carried>(ctx) else {
        return;
    };
    let Some(pointer) = ctx.pointer_latest_pos() else {
        return;
    };
    if landing.rail.is_some_and(|edge| pointer.x < edge) {
        return;
    }
    ctx.set_cursor_icon(CursorIcon::Grabbing);
    let t = Tokens::get(ctx);
    let panes = landing.panes;
    let boxes: Vec<(PaneId, Bounds)> = panes.iter().map(|(id, r)| (*id, bounds(*r))).collect();
    let found = carried
        .book
        .and_then(|_| DropTarget::at(&boxes, (pointer.x, pointer.y), &empty));
    // A full workspace can't split; a shelf opens no pane.
    let refused = match found {
        Some(DropTarget::Split { pane, .. }) if landing.full => Some(pane),
        _ => None,
    };
    let target = found.filter(|_| refused.is_none());
    let layer = LayerId::new(Order::Foreground, Id::new("library-panel-drop"));
    let painter = ctx.layer_painter(layer);
    if let Some(pane) = refused
        && let Some((_, rect)) = panes.iter().find(|(id, _)| *id == pane)
    {
        let words = format!(
            "{MAX} panes are open. Close one to open another.",
            MAX = mareader_workspace::MAX_PANES
        );
        chip(&painter, rect.center(), &words, &t, true);
    }
    if let Some(target) = target
        && let Some((_, rect)) = panes.iter().find(|(id, _)| *id == target.pane())
    {
        let b = target.predicted(bounds(*rect));
        let lit = Rect::from_min_size(pos2(b.x, b.y), vec2(b.w, b.h)).shrink(6.0);
        painter.rect_filled(lit, 10.0, t.accent.gamma_multiply(0.14));
        let stroke = Stroke::new(2.0, t.accent);
        painter.rect_stroke(lit, 10.0, stroke, egui::StrokeKind::Inside);
        let words = target.describe(&name(target.pane()));
        chip(&painter, lit.center(), &words, &t, true);
    }
    let label = pointer + vec2(14.0, 16.0);
    chip(&painter, label, &carried.title, &t, false);
    let released = ctx.input(|i| i.pointer.any_released());
    if !released {
        return;
    }
    DragAndDrop::clear_payload(ctx);
    if let (Some(target), Some(book)) = (target, carried.book) {
        let placement = match target {
            DropTarget::Here { .. } => Placement::Into,
            DropTarget::Split { edge, .. } => Placement::Edge(edge),
        };
        out.push(Sent::Engine(Command::PlaceBook {
            book,
            pane: Some(target.pane()),
            placement,
        }));
    }
}

fn bounds(r: Rect) -> Bounds {
    Bounds::new(r.left(), r.top(), r.width(), r.height())
}

/// A pill of words; centred on `at`, else hanging from it.
fn chip(painter: &egui::Painter, at: egui::Pos2, words: &str, t: &Tokens, centred: bool) {
    let font = theme::medium(12.5);
    let galley = painter.layout_no_wrap(words.to_owned(), font, t.ink);
    let size = galley.size() + vec2(20.0, 10.0);
    let rect = match centred {
        true => Rect::from_center_size(at, size),
        false => Rect::from_min_size(at, size),
    };
    painter.rect_filled(rect, size.y / 2.0, t.surface);
    painter.rect_stroke(
        rect,
        size.y / 2.0,
        Stroke::new(1.0, t.line),
        egui::StrokeKind::Inside,
    );
    painter.galley(rect.center() - galley.size() / 2.0, galley, t.ink);
}

#[cfg(test)]
mod tests {
    use super::*;
    use mareader_library::{ContentId, DeviceId, FileStamp, Measure, Opened};
    use std::path::PathBuf;

    fn library() -> (Library, Vec<BookId>, Vec<ShelfId>) {
        let mut lib = Library::new(DeviceId(1));
        let mut ids = Vec::new();
        for (n, title) in ["zeta", "Alpha notes", "plain", "Book"].iter().enumerate() {
            let opened = Opened {
                path: PathBuf::from(format!("/lib/{n}.epub")),
                measure: Measure {
                    content: ContentId(n as u128 + 1),
                    stamp: FileStamp::default(),
                },
                format: Format::Epub,
                title: (*title).to_owned(),
                author: None,
            };
            ids.push(lib.record_open(opened, 1));
        }
        let work = lib.create_shelf("Work", None, 2).unwrap();
        let archive = lib.create_shelf("Archive", Some(work), 3).unwrap();
        let empty = lib.create_shelf("empty", None, 4).unwrap();
        lib.file(ids[2], work, 5);
        lib.file(ids[3], archive, 6);
        (lib, ids, vec![work, archive, empty])
    }

    fn names(lines: &[Line]) -> Vec<String> {
        let name = |line: &Line| match line {
            Line::Shelf {
                name, depth, count, ..
            } => format!("{depth}:[{name} {count}]"),
            Line::Book { title, depth, .. } => format!("{depth}:{title}"),
        };
        lines.iter().map(name).collect()
    }

    #[test]
    fn the_top_lists_shelves_then_what_home_shows() {
        let (lib, _, _) = library();
        let none = BTreeSet::new();
        let unshelved = lines(&lib, &none, Home::Unshelved);
        let expect = ["0:[empty 0]", "0:[Work 2]", "0:Alpha notes", "0:zeta"];
        assert_eq!(names(&unshelved), expect);
        let every = lines(&lib, &none, Home::Everything);
        assert_eq!(every.len(), 6, "{:?}", names(&every));
    }

    #[test]
    fn an_open_shelf_shows_its_shelves_and_books_one_deeper() {
        let (lib, _, shelves) = library();
        let open: BTreeSet<ShelfId> = shelves.iter().take(2).copied().collect();
        let shown = lines(&lib, &open, Home::Unshelved);
        let expect = [
            "0:[empty 0]",
            "0:[Work 2]",
            "1:[Archive 1]",
            "2:Book",
            "1:plain",
            "0:Alpha notes",
            "0:zeta",
        ];
        assert_eq!(names(&shown), expect);
    }

    #[test]
    fn the_rows_follow_the_open_set_and_the_library() {
        let (mut lib, ids, shelves) = library();
        let mut state = PanelState::default();
        assert_eq!(state.lines(&lib, Home::Unshelved).len(), 4);
        state.set_open(shelves[0], true);
        assert!(state.is_open(shelves[0]));
        // Work opens on Archive, still shut, and plain.
        assert_eq!(state.lines(&lib, Home::Unshelved).len(), 6);
        lib.unfile(ids[2], shelves[0], 9);
        let shown = names(state.lines(&lib, Home::Unshelved));
        assert!(shown.contains(&"0:plain".to_owned()), "{shown:?}");
    }

    #[test]
    fn a_link_is_a_row_under_its_own_name_and_counts_on_its_shelf() {
        let (mut lib, ids, shelves) = library();
        let top = lib.link_to(LinkTo::Book(ids[3]), None, 7).unwrap();
        assert!(lib.rename_link(top, "Shortcut", 8));
        lib.link_to(LinkTo::Book(ids[0]), Some(shelves[0]), 9);
        // A link to a shelf has no row of its own.
        lib.link_to(LinkTo::Shelf(shelves[1]), None, 10);
        let shown = lines(&lib, &BTreeSet::new(), Home::Unshelved);
        let expect = [
            "0:[empty 0]",
            "0:[Work 3]",
            "0:Alpha notes",
            "0:Shortcut",
            "0:zeta",
        ];
        assert_eq!(names(&shown), expect);
        let row = shown
            .iter()
            .find(|l| matches!(l, Line::Book { link: Some(_), .. }));
        assert!(
            matches!(row, Some(Line::Book { id, .. }) if *id == ids[3]),
            "opens its book"
        );
    }

    #[test]
    fn a_book_on_two_open_shelves_is_two_rows_of_their_own() {
        let (mut lib, ids, shelves) = library();
        lib.file(ids[3], shelves[0], 7);
        let open: BTreeSet<ShelfId> = shelves.iter().take(2).copied().collect();
        let shown = lines(&lib, &open, Home::Unshelved);
        let levels: Vec<Option<ShelfId>> = (shown.iter())
            .filter_map(|l| match l {
                Line::Book { id, level, .. } if *id == ids[3] => Some(*level),
                _ => None,
            })
            .collect();
        assert_eq!(levels, [Some(shelves[1]), Some(shelves[0])]);
    }

    fn carried(item: Item, from: Option<ShelfId>) -> Carried {
        Carried {
            book: None,
            item,
            title: String::new(),
            from,
        }
    }

    fn moved(action: Option<Action>) -> Option<(Option<ShelfId>, bool)> {
        match action {
            Some(Action::Shelf(ShelfAction::Carry(mv))) => Some((mv.to, mv.keep)),
            _ => None,
        }
    }

    #[test]
    fn a_book_moves_to_a_shelf_or_the_top_but_not_where_it_is() {
        let (lib, ids, shelves) = library();
        let book = carried(Item::Book(ids[2]), Some(shelves[0]));
        let to = |target, alt| moved(drop_action(&lib, &book, target, alt));
        assert_eq!(to(Some(shelves[2]), false), Some((Some(shelves[2]), false)));
        assert_eq!(to(Some(shelves[2]), true), Some((Some(shelves[2]), true)));
        assert_eq!(to(None, false), Some((None, false)));
        assert_eq!(to(Some(shelves[0]), false), None, "already here");
        let loose = carried(Item::Book(ids[0]), None);
        let keep = moved(drop_action(&lib, &loose, Some(shelves[0]), true));
        assert_eq!(keep, Some((Some(shelves[0]), false)), "nothing to keep");
    }

    #[test]
    fn a_shelf_never_goes_inside_itself() {
        let (lib, _, shelves) = library();
        let (work, archive, empty) = (shelves[0], shelves[1], shelves[2]);
        let shelf = carried(Item::Shelf(work), None);
        let to = |target| moved(drop_action(&lib, &shelf, target, true));
        assert_eq!(to(Some(work)), None);
        assert_eq!(to(Some(archive)), None, "its own child");
        assert_eq!(to(None), None, "already at the top");
        assert_eq!(to(Some(empty)), Some((Some(empty), false)));
        let inner = carried(Item::Shelf(archive), Some(work));
        let up = moved(drop_action(&lib, &inner, None, false));
        assert_eq!(up, Some((None, false)));
        assert!(within(&lib, Some(archive), work));
        assert!(!within(&lib, Some(work), archive));
        assert!(!within(&lib, None, work));
    }
}
