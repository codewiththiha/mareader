//! The notes panel: a book's highlights and notes, by chapter, searchable.

mod share;

use crate::documents::Reading;
use crate::icons::Icon;
use crate::theme::{self, Tokens, color};
use crate::ui_command::{Commands, Sent};
use crate::widgets;
use egui::text::{LayoutJob, TextWrapping};
use egui::{
    Align, Color32, FontId, Galley, Id, Key, Rect, RichText, ScrollArea, Sense, TextFormat, Ui,
    vec2,
};
use mareader_appearance::Rgb;
use mareader_engine::command::Command;
use mareader_engine::skills::{Action as SkillAction, AskId};
use mareader_engine::study::Action;
use mareader_library::{MarkId, Pen, Spot};
use mareader_session::Document;
use std::sync::{Arc, Weak};

use mareader_engine::notes_panel::{bookmark_chapter, page_of, query};

const PAD_X: f32 = 12.0;
/// The colour rule's width and the gap after it.
const RULE: f32 = 3.0;
const RULE_GAP: f32 = 10.0;

/// One highlight or bookmark as the panel lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: MarkId,
    pub spot: Spot,
    pub quote: String,
    pub note: Option<String>,
    pub ink: Rgb,
    pub pen: Pen,
    /// When it was made, in wall milliseconds.
    pub added: u64,
    /// A bookmarked page rather than highlighted words.
    pub bookmark: bool,
}

/// A selection answer listed alongside the book's annotations.
#[derive(Clone)]
pub struct SelectionRow {
    pub id: AskId,
    pub spot: Option<Spot>,
    pub name: String,
    pub source: String,
    pub pinned: bool,
    pub pin_status: mareader_engine::library::SaveStatus,
    pub found: bool,
}

/// Which rows show: everything, notes, bookmarks, or selection answers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Filter {
    #[default]
    All,
    Notes,
    Bookmarks,
    SelectionAi,
}

impl Filter {
    fn keeps(self, row: &Row) -> bool {
        match self {
            Self::All => true,
            Self::Notes => row.note.is_some(),
            Self::Bookmarks => row.bookmark,
            Self::SelectionAi => false,
        }
    }
}

/// The search's words, and the wash that marks them.
#[derive(Clone, Copy)]
struct Find<'a> {
    words: &'a [String],
    wash: Color32,
}

impl Find<'_> {
    /// More lines show while searching, so a match isn't cut off.
    fn rows(self, rows: usize) -> usize {
        if self.words.is_empty() {
            rows
        } else {
            rows + 6
        }
    }
}

#[derive(Clone)]
struct CachedPlaces {
    doc: Weak<Document>,
    rows: Weak<[Row]>,
    places: Arc<[(Option<usize>, Option<String>)]>,
}

fn cached_places(
    ui: &mut Ui,
    reading: &Reading,
    rows: &Arc<[Row]>,
) -> Arc<[(Option<usize>, Option<String>)]> {
    let key = Id::new(("notes-places", reading.doc));
    if let Some(hit) = ui.data(|d| d.get_temp::<CachedPlaces>(key))
        && hit
            .doc
            .upgrade()
            .is_some_and(|d| Arc::ptr_eq(&d, &reading.document))
        && hit.rows.upgrade().is_some_and(|r| Arc::ptr_eq(&r, rows))
    {
        return hit.places;
    }
    let book = reading.document.as_ref();
    let places: Arc<[(Option<usize>, Option<String>)]> = rows
        .iter()
        .map(|r| {
            let page = page_of(book, r.spot);
            let chapter = page.and_then(|p| bookmark_chapter(book, p));
            (page, chapter)
        })
        .collect::<Vec<_>>()
        .into();
    ui.data_mut(|d| {
        d.insert_temp(
            key,
            CachedPlaces {
                doc: Arc::downgrade(&reading.document),
                rows: Arc::downgrade(rows),
                places: Arc::clone(&places),
            },
        );
    });
    places
}

/// Draws the panel for `reading`'s document and its `rows`.
pub fn show(
    ui: &mut Ui,
    reading: &Reading,
    rows: &Arc<[Row]>,
    ai: &[SelectionRow],
    out: &mut Commands,
) {
    let t = Tokens::get(ui.ctx());
    let id = Id::new(("notes-filter", reading.doc));
    let mut filter: Filter = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    let noted = rows.iter().filter(|r| r.note.is_some()).count();
    let kept = rows.iter().filter(|r| r.bookmark).count();
    if rows.is_empty() && ai.is_empty() {
        let hint = "Select text, then highlight it or add a note.";
        widgets::panel_empty(ui, Some(Icon::Note), "No highlights yet", hint);
        return;
    }
    // The last bookmark gone, its filter goes too.
    if (kept == 0 && filter == Filter::Bookmarks)
        || (ai.is_empty() && filter == Filter::SelectionAi)
    {
        filter = Filter::All;
    }
    let query_id = Id::new(("notes-query", reading.doc));
    let mut typed: String = ui.data(|d| d.get_temp(query_id)).unwrap_or_default();
    let words = query::terms(&typed);
    let places = cached_places(ui, reading, rows);
    let placed: Vec<(Option<usize>, Option<&str>, &Row)> = rows
        .iter()
        .zip(places.iter())
        .filter(|(r, _)| filter.keeps(r))
        .filter(|(r, (_, c))| {
            let note = r.note.as_deref().unwrap_or_default();
            let texts = [r.quote.as_str(), note, c.as_deref().unwrap_or_default()];
            query::keeps(&words, &texts)
        })
        .map(|(r, (p, c))| (*p, c.as_deref(), r))
        .collect();
    let shown_ai: Vec<&SelectionRow> = ai
        .iter()
        .filter(|a| {
            matches!(filter, Filter::All | Filter::SelectionAi)
                && query::keeps(&words, &[a.source.as_str(), a.name.as_str()])
        })
        .collect();
    ui.add_space(8.0);
    // The book and its tools, then the filters on a row of their own.
    ui.horizontal(|ui| {
        ui.add_space(PAD_X);
        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(PAD_X - 6.0);
            // Exports carry highlights; bookmarks are places, not words.
            let any = placed.iter().any(|(_, _, r)| !r.bookmark);
            ui.add_enabled_ui(any, |ui| {
                share::share(ui, reading, &placed, &t, out);
            });
            ui.with_layout(egui::Layout::left_to_right(Align::Center), |ui| {
                let title = RichText::new(&reading.name).strong();
                crate::shaped::line(ui, title, t.text, t.ink);
            });
        });
    });
    ui.add_space(2.0);
    filters(ui, &mut filter, (rows.len(), noted, kept, ai.len()));
    ui.data_mut(|d| d.insert_temp(id, filter));
    ui.add_space(6.0);
    search(ui, &mut typed);
    ui.data_mut(|d| d.insert_temp(query_id, typed));
    ui.add_space(4.0);
    if placed.is_empty() && shown_ai.is_empty() && !words.is_empty() {
        widgets::panel_empty(
            ui,
            Some(Icon::Note),
            "No matches",
            "Try fewer or other words.",
        );
        return;
    }
    if placed.is_empty() && shown_ai.is_empty() {
        widgets::panel_empty(
            ui,
            Some(Icon::Note),
            "No notes yet",
            "Select text and choose Note.",
        );
        return;
    }
    let find = Find {
        words: &words,
        wash: t.accent_soft,
    };
    ScrollArea::vertical()
        .id_salt(("notes-list", reading.doc))
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let mut last: Option<&str> = None;
            for (i, &(page, chapter, row)) in placed.iter().enumerate() {
                if let Some(c) = chapter
                    && last != Some(c)
                {
                    let rest = placed.iter().skip(i);
                    let n = rest.take_while(|(_, d, _)| *d == Some(c)).count();
                    heading(ui, c, n, find, &t);
                }
                last = chapter;
                if row.bookmark {
                    bookmark(ui, reading, (page, chapter), row, find, &t, out);
                } else {
                    entry(ui, reading, page, row, find, &t, out);
                }
            }
            for answer in shown_ai {
                selection_entry(ui, answer, out);
            }
            ui.add_space(12.0);
        });
}

fn filters(
    ui: &mut Ui,
    filter: &mut Filter,
    (rows, noted, kept, ai): (usize, usize, usize, usize),
) {
    let pad = egui::Margin::symmetric(PAD_X as i8, 0);
    egui::Frame::NONE.inner_margin(pad).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            let chips = [
                (Filter::All, format!("All · {}", rows + ai)),
                (Filter::Notes, format!("Notes · {noted}")),
                (Filter::Bookmarks, format!("Bookmarks · {kept}")),
                (Filter::SelectionAi, format!("Selection AI · {ai}")),
            ];
            let some = |f: &&(Filter, String)| {
                (f.0 != Filter::Bookmarks || kept > 0) && (f.0 != Filter::SelectionAi || ai > 0)
            };
            for (f, label) in chips.iter().filter(some) {
                if ui.selectable_label(*filter == *f, label).clicked() {
                    *filter = *f;
                }
            }
        });
    });
}

fn selection_entry(ui: &mut Ui, answer: &SelectionRow, out: &mut Commands) {
    let state = match (answer.pinned, answer.pin_status) {
        (false, _) => "This session",
        (true, mareader_engine::library::SaveStatus::Pending) => "Saving…",
        (true, mareader_engine::library::SaveStatus::Failed) => "Save failed",
        (true, mareader_engine::library::SaveStatus::Saved) => "Pinned",
    };
    let label = format!("{} · {state}\n{}", answer.name, answer.source);
    let response = ui.add_enabled(answer.found, egui::Button::new(label).wrap());
    if response.clicked() {
        out.push(Sent::Engine(Command::Skill(SkillAction::Show(answer.id))));
    }
    if !answer.found {
        ui.label("This passage could not be found in the current layout.");
    }
}

/// The search over quotes, notes and chapters; Escape clears it.
fn search(ui: &mut Ui, typed: &mut String) {
    let pad = egui::Margin::symmetric(PAD_X as i8, 0);
    egui::Frame::NONE.inner_margin(pad).show(ui, |ui| {
        let res = widgets::input(typed)
            .id(Id::new("notes-search"))
            .icon(Icon::Search)
            .hint("Search highlights and notes")
            .label("Search notes")
            .clearable(true)
            .compact()
            .show(ui);
        // The list read the old words this frame; draw again.
        if res.changed() {
            ui.ctx().request_repaint();
        }
        let escaped = ui.input(|i| i.key_pressed(Key::Escape));
        if res.response.lost_focus() && escaped {
            typed.clear();
            ui.ctx().request_repaint();
        }
    });
}

/// A chapter's heading and how many of the rows under it are its.
fn heading(ui: &mut Ui, title: &str, n: usize, find: Find<'_>, t: &Tokens) {
    let pad = egui::Margin {
        left: PAD_X as i8,
        right: PAD_X as i8,
        top: 10,
        bottom: 2,
    };
    egui::Frame::NONE.inner_margin(pad).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                let count = RichText::new(n.to_string())
                    .size(t.text_small)
                    .color(t.muted);
                ui.label(count);
                let r = if crate::shaped::needs_shaping(title) {
                    crate::shaped::line(ui, title, 12.0, t.muted)
                } else {
                    let job = styled(title, theme::medium(12.0), t.muted, find);
                    ui.add(egui::Label::new(job).truncate())
                };
                let label = format!("{title}, {n}");
                r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &label));
            });
        });
    });
}

/// One highlight: its page, the words, and the note under them.
fn entry(
    ui: &mut Ui,
    reading: &Reading,
    page: Option<usize>,
    row: &Row,
    find: Find<'_>,
    t: &Tokens,
    out: &mut Commands,
) {
    let full = ui.available_width();
    let text_w = (full - PAD_X * 2.0 - RULE - RULE_GAP).max(40.0);
    let open = is_open(ui, row.id);
    let rows = |n: usize| if open { usize::MAX } else { find.rows(n) };
    let quote = Text::new(ui, &row.quote, 13.5, t.ink, find, text_w, rows(3));
    let note = (row.note.as_deref())
        .map(|n| Text::new(ui, n, 13.0, t.muted, find, text_w - 18.0, rows(4)));
    let more = open || quote.cut() || note.as_ref().is_some_and(Text::cut);
    let meta = page.map(|p| format!("Page {}", p + 1)).unwrap_or_default();
    let meta_h = 16.0;
    let note_h = note.as_ref().map_or(0.0, |g| g.size().y + 6.0);
    let more_h = if more { MORE_H } else { 0.0 };
    let height = 10.0 + meta_h + quote.size().y + note_h + more_h + 10.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(full, height), Sense::click());
    let label = match &row.note {
        Some(n) => format!("{}. Note: {n}", row.quote),
        None => row.quote.clone(),
    };
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    let p = ui.painter();
    let card = rect.shrink2(vec2(6.0, 2.0));
    if resp.hovered() || resp.has_focus() {
        p.rect_filled(card, t.radius, t.hover);
    }
    let left = rect.left() + PAD_X;
    let top = rect.top() + 10.0;
    let rule = Rect::from_min_size(egui::pos2(left, top + meta_h), vec2(RULE, quote.size().y));
    p.rect_filled(rule, 1.5, color(row.ink));
    let x = left + RULE + RULE_GAP;
    let small = FontId::proportional(t.text_small);
    p.text(
        egui::pos2(x, top),
        egui::Align2::LEFT_TOP,
        meta,
        small,
        t.muted,
    );
    let qh = quote.size().y;
    quote.paint(ui, egui::pos2(x, top + meta_h), t.ink);
    if let Some(g) = &note {
        let y = top + meta_h + qh + 6.0;
        let mark = Rect::from_min_size(egui::pos2(x, y + 1.0), vec2(13.0, 13.0));
        Icon::Note.paint(ui, mark, t.muted);
        g.paint(ui, egui::pos2(x + 18.0, y), t.muted);
    }
    if more {
        more_toggle(
            ui,
            row.id,
            egui::pos2(x, top + meta_h + qh + note_h + 2.0),
            open,
            t,
        );
    }
    if resp.clicked()
        && let Some(page) = page
    {
        out.push(Sent::Engine(Command::GoTo {
            pane: reading.pane,
            page,
        }));
    }
    let noted = row.note.is_some();
    widgets::context_menu(&resp, |ui| {
        let write = if noted { "Edit note" } else { "Add note" };
        if widgets::menu_row(ui, Icon::Note, write, None, false).clicked() {
            if let Some(page) = page {
                out.push(Sent::Engine(Command::GoTo {
                    pane: reading.pane,
                    page,
                }));
            }
            out.push(Sent::Engine(Command::Study(Action::WriteNoteIn(
                reading.pane,
                row.id,
            ))));
            ui.close();
        }
        if noted && widgets::menu_row(ui, Icon::Remove, "Delete note", None, false).clicked() {
            out.push(Sent::Engine(Command::Study(Action::DeleteNote(row.id))));
            ui.close();
        }
        widgets::menu_rule(ui);
        if widgets::menu_row_danger(ui, Icon::Remove, "Remove highlight").clicked() {
            out.push(Sent::Engine(Command::Study(Action::Remove(vec![row.id]))));
            ui.close();
        }
    });
}

/// One bookmark: its page, chapter and note, a ribbon for its rule.
fn bookmark(
    ui: &mut Ui,
    reading: &Reading,
    (page, chapter): (Option<usize>, Option<&str>),
    row: &Row,
    find: Find<'_>,
    t: &Tokens,
    out: &mut Commands,
) {
    let full = ui.available_width();
    let text_w = (full - PAD_X * 2.0 - RULE - RULE_GAP).max(40.0);
    let at = page.map(|p| format!("Page {}", p + 1));
    let (name, meta) = match (chapter, at) {
        (Some(c), Some(at)) => (c.to_owned(), at),
        (None, Some(at)) => (at, "Bookmark".to_owned()),
        (_, None) => (row.quote.clone(), "Bookmark".to_owned()),
    };
    let open = is_open(ui, row.id);
    let rows = |n: usize| if open { usize::MAX } else { find.rows(n) };
    let title = Text::new(ui, &name, 13.5, t.ink, find, text_w, 2);
    let note = (row.note.as_deref())
        .map(|n| Text::new(ui, n, 13.0, t.muted, find, text_w - 18.0, rows(4)));
    let more = open || note.as_ref().is_some_and(Text::cut);
    let meta_h = 16.0;
    let note_h = note.as_ref().map_or(0.0, |g| g.size().y + 6.0);
    let more_h = if more { MORE_H } else { 0.0 };
    let height = 10.0 + meta_h + title.size().y + note_h + more_h + 10.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(full, height), Sense::click());
    let place = match page {
        Some(p) if meta != "Bookmark" => format!("Bookmark, page {}: {name}", p + 1),
        _ => format!("Bookmark: {name}"),
    };
    let label = match &row.note {
        Some(n) => format!("{place}. Note: {n}"),
        None => place,
    };
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    let p = ui.painter();
    if resp.hovered() || resp.has_focus() {
        p.rect_filled(rect.shrink2(vec2(6.0, 2.0)), t.radius, t.hover);
    }
    let left = rect.left() + PAD_X;
    let top = rect.top() + 10.0;
    // Centred on where a highlight's rule runs, the text in line.
    let mid = egui::pos2(left + RULE / 2.0, top + meta_h + 8.0);
    Icon::Bookmark.paint(ui, Rect::from_center_size(mid, vec2(12.0, 12.0)), t.accent);
    let x = left + RULE + RULE_GAP;
    let small = FontId::proportional(t.text_small);
    let p = ui.painter();
    p.text(
        egui::pos2(x, top),
        egui::Align2::LEFT_TOP,
        meta,
        small,
        t.muted,
    );
    let th = title.size().y;
    title.paint(ui, egui::pos2(x, top + meta_h), t.ink);
    if let Some(g) = &note {
        let y = top + meta_h + th + 6.0;
        let mark = Rect::from_min_size(egui::pos2(x, y + 1.0), vec2(13.0, 13.0));
        Icon::Note.paint(ui, mark, t.muted);
        g.paint(ui, egui::pos2(x + 18.0, y), t.muted);
    }
    if more {
        more_toggle(
            ui,
            row.id,
            egui::pos2(x, top + meta_h + th + note_h + 2.0),
            open,
            t,
        );
    }
    let go = |out: &mut Commands| {
        if let Some(page) = page {
            out.push(Sent::Engine(Command::GoTo {
                pane: reading.pane,
                page,
            }));
        }
    };
    if resp.clicked() {
        go(out);
    }
    let noted = row.note.is_some();
    widgets::context_menu(&resp, |ui| {
        let write = if noted { "Edit note" } else { "Add note" };
        if widgets::menu_row(ui, Icon::Note, write, None, false).clicked() {
            go(out);
            out.push(Sent::Engine(Command::Study(Action::WriteNoteIn(
                reading.pane,
                row.id,
            ))));
            ui.close();
        }
        if noted && widgets::menu_row(ui, Icon::Remove, "Delete note", None, false).clicked() {
            out.push(Sent::Engine(Command::Study(Action::DeleteNote(row.id))));
            ui.close();
        }
        widgets::menu_rule(ui);
        if widgets::menu_row_danger(ui, Icon::Remove, "Remove bookmark").clicked() {
            out.push(Sent::Engine(Command::Study(Action::Remove(vec![row.id]))));
            ui.close();
        }
    });
}

/// `text` with its spacing evened, the search's words washed.
fn styled(text: &str, font: FontId, ink: Color32, find: Find<'_>) -> LayoutJob {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let plain = TextFormat::simple(font, ink);
    let lit = TextFormat {
        background: find.wash,
        ..plain.clone()
    };
    let mut job = LayoutJob::default();
    let mut at = 0;
    for r in query::marks(&flat, find.words) {
        if let Some(s) = flat.get(at..r.start) {
            job.append(s, 0.0, plain.clone());
        }
        if let Some(s) = flat.get(r.clone()) {
            job.append(s, 0.0, lit.clone());
        }
        at = r.end;
    }
    if let Some(s) = flat.get(at..) {
        job.append(s, 0.0, plain);
    }
    job
}

/// A row's words: egui's galley, or parley's for scripts egui garbles.
enum Text {
    Galley(Arc<Galley>),
    Shaped {
        flat: String,
        size: f32,
        width: f32,
        lines: usize,
        extent: egui::Vec2,
        cut: bool,
    },
}

impl Text {
    /// `raw` at `size` in `ink`, wrapped at `width` to `lines` at most.
    fn new(
        ui: &Ui,
        raw: &str,
        size: f32,
        ink: Color32,
        find: Find<'_>,
        width: f32,
        lines: usize,
    ) -> Self {
        let flat = raw.split_whitespace().collect::<Vec<_>>().join(" ");
        if let Some((extent, cut)) = crate::shaped::measure(ui.ctx(), &flat, size, width, lines) {
            return Self::Shaped {
                flat,
                size,
                width,
                lines,
                extent,
                cut,
            };
        }
        let job = styled(raw, FontId::proportional(size), ink, find);
        Self::Galley(clamp(ui, job, width, lines))
    }

    fn size(&self) -> egui::Vec2 {
        match self {
            Self::Galley(g) => g.size(),
            Self::Shaped { extent, .. } => *extent,
        }
    }

    /// Whether lines were left out.
    fn cut(&self) -> bool {
        match self {
            Self::Galley(g) => g.elided,
            Self::Shaped { cut, .. } => *cut,
        }
    }

    /// Shaped text is drawn only once it scrolls into view.
    fn paint(&self, ui: &Ui, at: egui::Pos2, ink: Color32) {
        match self {
            Self::Galley(g) => ui.painter().galley(at, g.clone(), ink),
            Self::Shaped {
                flat,
                size,
                width,
                lines,
                extent,
                ..
            } => {
                if !ui.is_rect_visible(Rect::from_min_size(at, *extent)) {
                    return;
                }
                let drawn = crate::shaped::block(ui.ctx(), flat, *size, *width, *lines);
                if let Some(d) = drawn {
                    d.paint(ui.painter(), at, ink);
                }
            }
        }
    }
}

/// Whether a row's long words show whole.
fn is_open(ui: &Ui, id: MarkId) -> bool {
    ui.data(|d| d.get_temp::<bool>(open_id(id)))
        .unwrap_or(false)
}

fn open_id(id: MarkId) -> Id {
    Id::new(("notes-open", id))
}

/// The room a Show more line takes under a row.
const MORE_H: f32 = 20.0;

/// "Show more" under a cut row, "Show less" under an opened one.
fn more_toggle(ui: &Ui, id: MarkId, at: egui::Pos2, open: bool, t: &Tokens) {
    let label = if open { "Show less" } else { "Show more" };
    let font = FontId::proportional(t.text_small);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, t.accent);
    let rect =
        Rect::from_min_size(at, vec2(galley.size().x + 8.0, MORE_H)).translate(vec2(-4.0, 0.0));
    // Laid after the row, so its own clicks don't reach the row.
    let resp = ui.interact(rect, open_id(id).with("toggle"), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        ui.painter().rect_filled(rect, t.radius, t.hover);
    }
    let y = rect.center().y - galley.size().y / 2.0;
    ui.painter().galley(egui::pos2(at.x, y), galley, t.accent);
    if resp.clicked() {
        ui.ctx().data_mut(|d| d.insert_temp(open_id(id), !open));
    }
}

/// A laid-out `job` wrapped to `rows` at most.
fn clamp(ui: &Ui, mut job: LayoutJob, w: f32, rows: usize) -> Arc<Galley> {
    job.wrap = TextWrapping {
        max_width: w,
        max_rows: rows,
        break_anywhere: false,
        overflow_character: Some('…'),
    };
    ui.painter().layout_job(job)
}

/// The reading order of `spot`, to list highlights as the book runs.
pub fn order(spot: &Spot) -> (usize, usize) {
    mareader_engine::notes_panel::reading_order(spot)
}
