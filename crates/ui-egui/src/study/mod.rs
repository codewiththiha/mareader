//! Study tools on the page: the selection bar, marks and the word card;
//! see `docs/study.md`.

mod acting;
mod answers;
mod bar;
mod card;
mod gloss;
pub use gloss::Gloss;
mod keeping;
mod marking;
pub mod marks;
mod note;
mod picking;
pub use picking::STYLE_ID as PICK_STYLE;
mod restyle;
pub(crate) use restyle::shown as mark_color;

use std::cell::RefCell;
use std::sync::Arc;

use egui::{Context, Pos2, Rect, Vec2};
use mareader_cefr::Role;
use mareader_engine::command::Command;
use mareader_engine::session::Session;
use mareader_engine::study::Action;
use mareader_engine::study::state::{Key, State as StudyState};
use mareader_engine::study::words::Asked;
use mareader_library::{BookId, Library, Mark, MarkId, MarkKind, Spot, Style};
use mareader_settings::{GlossSettings, HighlightSettings};
use mareader_study::WordInfo;
use mareader_workspace::PaneId;

pub use bar::{BarPress, offer as offer_skills, show as show_bar};

use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent};

#[derive(Clone, Debug, PartialEq)]
struct Card {
    pane: PaneId,
    key: Key,
    word: String,
    context: String,
    /// The word on screen, as of the last frame it was seen.
    at: Rect,
    /// Pane scroll when a loose card opened; scrolling dismisses it.
    offset: Option<Vec2>,
    /// Seen on screen this frame; a mark scrolled away closes it.
    seen: bool,
    /// The word's role in its sentence, once the tagger said.
    role: Option<Role>,
}

mareader_engine::impl_card!(Card);

/// The right-click menu on a mark.
#[derive(Clone, Copy, Debug, PartialEq)]
struct MarkMenu {
    pane: PaneId,
    mark: MarkId,
    at: Pos2,
    /// The mark's words on screen, for a note opened from here.
    rect: Rect,
    /// A highlight's: whether it has a note; none for an AI mark.
    noted: Option<bool>,
    /// A highlight's look, to change; none for an AI mark.
    style: Option<Style>,
    /// Its words were drawn this frame: it follows them, else goes.
    seen: bool,
}

/// A highlight's note beside it: read, or being written.
#[derive(Clone, Debug, PartialEq)]
struct Note {
    pane: PaneId,
    /// The mark as it was opened, its note kept up to date here.
    mark: Mark,
    at: Rect,
    seen: bool,
    /// The text being written; none while the note is only read.
    draft: Option<String>,
    /// The highlight was made for this note: Cancel takes it back.
    placeholder: bool,
    /// Frames left to give the field focus, as the card measures first.
    focus: u8,
}

/// Window-ephemeral study overlays: card, menu, draft note, picking, and cache.
#[derive(Default)]
pub struct StudyUi {
    card: Option<Card>,
    stash: Option<Card>,
    menu: Option<MarkMenu>,
    note: Option<Note>,
    /// Marks picked to remove together, in one pane.
    picking: Option<picking::Picking>,
    /// Last frame's card and menu, so a press outside closes them.
    card_rect: Option<Rect>,
    menu_rect: Option<Rect>,
    note_rect: Option<Rect>,
    /// Books' marks as last listed: every pane asks each frame.
    listed: RefCell<Vec<Listed>>,
    /// The notes panel's rows as last built, cached by marks stamp.
    noted: RefCell<Option<Noted>>,
}

/// Read-only view over engine study state and window study overlays.
#[derive(Clone, Copy)]
pub struct StudyView<'a> {
    state: &'a StudyState,
    ui: &'a StudyUi,
}

impl<'a> StudyView<'a> {
    pub fn new(state: &'a StudyState, ui: &'a StudyUi) -> Self {
        Self { state, ui }
    }

    pub fn card_open(&self) -> bool {
        self.ui.card_open()
    }

    pub fn card_word(&self) -> Option<&'a str> {
        self.ui.card_word()
    }

    pub fn card_role(&self) -> Option<&'a Role> {
        self.ui.card_role()
    }

    pub fn card_answer(&self) -> Option<&'a WordInfo> {
        self.state
            .answer(self.ui.card.as_ref()?.key)
            .map(|s| &s.info)
    }

    pub fn card_by(&self) -> Option<&'a str> {
        self.state
            .answer(self.ui.card.as_ref()?.key)
            .map(|s| s.by.as_str())
    }

    pub fn menu_rect(&self) -> Option<Rect> {
        self.ui.menu_rect()
    }

    pub fn picking(&self) -> bool {
        self.ui.picking()
    }

    pub fn picked(&self, pane: PaneId) -> Option<Vec<MarkId>> {
        self.ui.picked(pane)
    }

    pub fn note_open(&self) -> Option<(MarkId, Option<&'a str>)> {
        self.ui.note_open()
    }
}

/// A book's bookmarks and where each is.
pub type Bookmarks = Arc<[(MarkId, Spot)]>;

/// Books listed at most; a ninth lets the oldest go.
const LISTED_CAP: usize = 8;

/// A book's marks and bookmarks, while none changed and the look held.
struct Listed {
    stamp: u64,
    book: BookId,
    look: HighlightSettings,
    shown: Arc<[marks::Shown]>,
    bookmarks: Bookmarks,
}

struct Noted {
    stamp: u64,
    book: BookId,
    look: HighlightSettings,
    rows: Arc<[crate::notes_panel::Row]>,
}

impl StudyUi {
    /// `pane` drew no pages this frame, as while it re-sets: the card stays.
    pub fn hold(&mut self, pane: PaneId) {
        if let Some(card) = self.card.as_mut().filter(|c| c.pane == pane) {
            card.seen = true;
        }
        if let Some(note) = self.note.as_mut().filter(|n| n.pane == pane) {
            note.seen = true;
        }
        if let Some(menu) = self.menu.as_mut().filter(|m| m.pane == pane) {
            menu.seen = true;
        }
    }

    /// Escape: drops a draft, closes the card or menu, else stops picking.
    pub fn escape(&mut self, session: &mut Session) -> bool {
        if self.note.is_some() {
            self.cancel_note(session);
            return true;
        }
        if self.card.is_some() || self.menu.is_some() {
            self.card = None;
            self.stash = None;
            self.menu = None;
            return true;
        }
        self.picking.take().is_some()
    }

    /// Before the panes draw: marks must show themselves again.
    pub fn begin_frame(&mut self) {
        if let Some(card) = &mut self.card {
            card.seen = matches!(card.key, Key::Loose(_));
        }
        // A draft stays while its words scroll away; a read note goes.
        if let Some(note) = &mut self.note {
            note.seen = note.draft.is_some();
        }
        if let Some(menu) = &mut self.menu {
            menu.seen = false;
        }
    }

    fn drop_card(&mut self) {
        self.card = None;
        self.stash = None;
    }

    /// After the panes: the card and the mark menu, over them.
    pub fn show(
        &mut self,
        ctx: &Context,
        (state, main): (&StudyState, &Library),
        (panes, heard): (&[(PaneId, Rect)], mareader_engine::sounds::Heard<'_>),
        (look, pens): (&GlossSettings, &HighlightSettings),
        (dict_ready, pane_offset): (bool, impl Fn(PaneId) -> Option<Vec2>),
        out: &mut Commands,
    ) {
        let pressed = ctx.input(|i| {
            let p = i.pointer.press_origin();
            p.filter(|_| i.pointer.any_pressed())
        });
        let outside = |r: Option<Rect>| pressed.is_some_and(|p| !r.is_some_and(|r| r.contains(p)));
        if self.card.is_some() && outside(self.card_rect) {
            self.drop_card();
        }
        let room = |pane| panes.iter().find(|(p, _)| *p == pane).map(|(_, r)| *r);
        // A press elsewhere, or its words scrolled away, closes the menu.
        let gone = |m: &MarkMenu| !m.seen || room(m.pane).is_none();
        if outside(self.menu_rect) || self.menu.as_ref().is_some_and(gone) {
            self.menu = None;
        }
        // A press elsewhere keeps a draft; a note only read just closes.
        if let Some(n) = &self.note
            && (outside(self.note_rect) || room(n.pane).is_none() || !n.seen)
        {
            match n.draft {
                Some(_) => out.push(Sent::Engine(Command::Study(Action::SaveNote(n.mark.id)))),
                None => self.note = None,
            }
        }
        // A scrolled-away mark, scrolled loose card, or closed pane drops it.
        let loose_moved = |c: &Card| {
            matches!(c.key, Key::Loose(_))
                && match (c.offset, pane_offset(c.pane)) {
                    (Some(a), Some(b)) => (a - b).length() > 0.5,
                    _ => false,
                }
        };
        if self
            .card
            .as_ref()
            .is_some_and(|c| !c.seen || room(c.pane).is_none() || loose_moved(c))
        {
            self.drop_card();
        }
        let t = Tokens::get(ctx);
        // A card View reopened waits until its words are placed.
        let placed = self.card.as_ref().filter(|c| c.at.is_finite());
        self.card_rect = placed.and_then(|c| {
            let extra = (room(c.pane)?, heard, look, dict_ready);
            show_card(ctx, c, state, extra, &t, out)
        });
        self.menu_rect = self.menu.map(|m| {
            card::menu(
                ctx,
                (m.pane, m.mark),
                (m.at, m.rect),
                m.noted,
                (m.style, pens),
                &t,
                out,
            )
        });
        self.note_rect = match &mut self.note {
            Some(n) => room(n.pane).map(|room| {
                // Opened from the panel, before its words were placed.
                let at = match n.at.is_finite() {
                    true => n.at,
                    false => Rect::from_center_size(room.center(), egui::Vec2::splat(1.0)),
                };
                let view = note::View {
                    mark: n.mark.id,
                    quote: &n.mark.word,
                    ink: ink(&n.mark, pens).color,
                    saved: n.mark.note.value.as_deref(),
                    draft: n.draft.as_mut(),
                    fresh: n.focus > 0,
                    at,
                    room,
                };
                n.focus = n.focus.saturating_sub(1);
                note::show(ctx, view, &t, out)
            }),
            None => None,
        };
        if self
            .picking
            .as_ref()
            .is_some_and(|p| room(p.pane).is_none())
        {
            self.picking = None;
        }
        if let Some(p) = &self.picking
            && let Some(r) = room(p.pane)
        {
            // The highlights picked, with their looks; AI marks keep theirs.
            let ids = p.ids.iter().filter_map(|id| state.mark(main, *id));
            let words = ids.filter(|m| m.kind == MarkKind::Highlight);
            let looks: Vec<_> = words.map(|m| (m.id, m.look())).collect();
            picking::bar(ctx, p, (r, &looks, pens), &t, out);
        }
    }
}

fn show_card(
    ctx: &Context,
    c: &Card,
    state: &StudyState,
    (room, heard, look, show_tabs): (
        Rect,
        mareader_engine::sounds::Heard<'_>,
        &GlossSettings,
        bool,
    ),
    t: &Tokens,
    out: &mut Commands,
) -> Option<Rect> {
    let answer = match (state.answer(c.key), state.failure(c.key)) {
        (Some(said), _) => card::Answer::Ready(&said.info, &said.by),
        (None, Some(e)) => card::Answer::Failed(e),
        (None, None) => match state.partial(c.key) {
            Some(info) => card::Answer::Partial(info),
            None => card::Answer::Waiting,
        },
    };
    let view = card::View {
        word: &c.word,
        role: c.role.as_ref(),
        answer,
        show_tabs,
        at: c.at,
        room,
        density: look.density,
        saying: heard.saying(&c.word),
        sound: heard.lines(&c.word),
    };
    Some(card::show(ctx, &view, t, out))
}

/// Explain's ask, as the app gathers it.
pub struct Explained<'a> {
    pub pane: PaneId,
    pub book: Option<BookId>,
    /// The document's name: a session's marks are kept by it.
    pub name: &'a str,
    pub asked: Asked,
    /// The word on screen.
    pub rect: Rect,
}

/// How `m` is drawn; a highlight never takes a reserved look.
fn ink(m: &Mark, look: &HighlightSettings) -> marks::Ink {
    if m.kind == MarkKind::Explain {
        return marks::Ink {
            pen: look.ai.pen,
            color: look.ai.color,
            ai: true,
        };
    }
    let style = m.look();
    let color = restyle::shown(style, look);
    marks::Ink {
        pen: style.pen,
        color,
        ai: false,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::path::PathBuf;

    use mareader_library::Gloss as MarkGloss;
    use mareader_library::{ContentId, DeviceId, Format, Measure, NewMark, Opened};
    use mareader_study::Canned;

    use super::*;

    fn marked(said: &str) -> (Library, MarkId) {
        let mut main = Library::new(DeviceId(1));
        let opened = Opened {
            path: PathBuf::from("/a.pdf"),
            measure: Measure {
                content: ContentId(7),
                stamp: Default::default(),
            },
            format: Format::Pdf,
            title: "A".into(),
            author: None,
        };
        let book = main.record_open(opened, 1);
        let spot = Spot::Page {
            page: 0,
            rect: [1.0, 2.0, 3.0, 4.0],
        };
        let new = NewMark::explain("quick".into(), "the quick fox".into(), spot);
        let id = main.add_mark(book, new, 2).unwrap();
        let gloss = MarkGloss {
            meaning: said.into(),
            by: "Ollama · gemma".into(),
            ..MarkGloss::default()
        };
        main.set_gloss(id, Some(gloss), 3);
        (main, id)
    }

    #[test]
    fn a_saved_answer_shows_without_asking_and_ask_again_replaces_it() {
        let (mut main, id) = marked("Fast.");
        let mut state = StudyState::default();
        state.set_inline(true);
        let wake: mareader_engine::session::Waker = Arc::new(|| {});
        state.prepare_card(
            &mut main,
            Arc::clone(&wake),
            Key::Mark(id),
            "quick",
            "the quick fox",
        );
        let meaning = state.answer(Key::Mark(id)).map(|s| s.info.meaning.clone());
        assert_eq!(meaning.as_deref(), Some("Fast."));
        assert_eq!(
            state.answer(Key::Mark(id)).map(|s| s.by.as_str()),
            Some("Ollama · gemma")
        );

        state.set_explainer(Arc::new(Canned));
        state.ask_again(&mut main, wake, Key::Mark(id), "quick", "the quick fox");
        assert_eq!(
            state.answer(Key::Mark(id)).map(|s| s.by.as_str()),
            Some("Sample answers")
        );
        let saved = main.mark(id).and_then(|m| m.gloss.value.clone()).unwrap();
        assert_eq!(saved.by, "Sample answers");
        assert_ne!(saved.meaning, "Fast.");
    }

    #[test]
    fn listed_marks_are_shared_until_one_changes() {
        let (mut main, id) = marked("Fast.");
        let state = StudyState::default();
        let ui = StudyUi::default();
        let look = HighlightSettings::default();
        let book = main.mark(id).map(|m| m.book.value);
        let first = ui.marks_of(&state, &main, book, "", &look);
        let again = ui.marks_of(&state, &main, book, "", &look);
        assert!(
            Arc::ptr_eq(&first, &again),
            "an unchanged book is not listed again"
        );
        assert!(main.set_note(id, Some("a note".into()), 4));
        let noted = ui.marks_of(&state, &main, book, "", &look);
        assert!(
            noted.first().is_some_and(|m| m.noted),
            "a change shows at once"
        );
        main.set_marks_alive(&[id], false, 5);
        assert!(ui.marks_of(&state, &main, book, "", &look).is_empty());
        assert!(ui.bookmarks_of(&state, &main, book, "").is_empty());
    }
}
