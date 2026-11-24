//! Pages summarized, translated or simplified by the AI, shown over them.

mod cards;
pub mod menu;
pub mod paint;

use std::collections::HashMap;
use std::sync::Arc;

use egui::{Context, Id, Pos2, Rect, vec2};
use mareader_library::{AiPageId, AiTask, BookId, Library, PageSpan};
use mareader_reflow::{PAGE_WIDTH, Typography};
use mareader_session::{Document, Scale};
use mareader_settings::{AiSettings, PageTask, SummaryStyle, language_name};
use mareader_study::page::Rewritten;
use mareader_workspace::PaneId;

use crate::gpu::Gpu;
use mareader_engine::page_ai::cards::Job as CardJob;
use mareader_engine::page_ai::extract::{self, Source};
use mareader_engine::page_ai::view::{AiView, group_span};
use mareader_engine::page_ai::{Slot, pages_of};
use mareader_engine::session::Session;
use mareader_engine::stable_hash::hash;

use mareader_engine::page_ai::ledger::Ledger;
pub(crate) use paint::Overlay;

/// Seconds a result takes to fade in over its page.
const FADE_IN: f32 = 0.3;
/// Seconds the menu's page wash takes to come and to go.
const WASH_FADE: f32 = 0.22;

/// Where the menu's Read aloud row says it is hovered, for its word.
fn aloud_hot(pane: PaneId) -> Id {
    Id::new(("page-ai-aloud-hot", pane))
}

/// What pdf_view draws of the page AI in one pane.
#[derive(Clone, Default)]
pub struct PaneAi {
    pub overlays: Vec<Overlay>,
    /// The pages the open menu would ask about, washed to show it.
    pub wash: Option<paint::Wash>,
    /// Where the pane's top is clear of the window's controls.
    pub top: f32,
}

/// A result to place: its task, span, words, source and identity.
type What = (AiTask, PageSpan, Arc<Rewritten>, Option<Arc<Source>>, u64);

/// What the page menu shows.
#[derive(Clone, Debug, PartialEq)]
enum Step {
    Tasks,
    Languages,
    /// This page or continuous, for a task and maybe a language.
    How(PageTask, Option<String>),
}

#[derive(Clone, Debug, PartialEq)]
struct Menu {
    pane: PaneId,
    page: usize,
    at: Pos2,
    step: Step,
    /// Picked pages, asked as groups; none for the page's own menu.
    pages: Vec<usize>,
    /// Where reading aloud would start, if the book can be read.
    aloud: Option<mareader_engine::read_aloud::ReadFrom>,
    /// Where on its page it was opened; none for picked pages.
    pin: Option<Pin>,
    /// When it first showed, for its entrance.
    born: Option<f64>,
}

/// A right-click's place on its page, so its menu rides along with it.
#[derive(Clone, Debug, PartialEq)]
pub struct Pin {
    /// The click, in page points.
    pub point: Pos2,
    /// The word under it, in page points, and its text.
    pub word: Option<mareader_pdf::Bounds>,
    pub text: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct BubbleMenu {
    pane: PaneId,
    page: usize,
    slot: Slot,
    at: Pos2,
}

/// The page AI's window half: card textures, menus, and menu wash state.
#[derive(Default)]
pub struct PageAiUi {
    cards: cards::Cards,
    menu: Option<Menu>,
    menu_rect: Option<Rect>,
    bubble_menu: Option<BubbleMenu>,
    bubble_rect: Option<Rect>,
    /// The menu's pages, kept while their wash fades after it.
    washed: Option<(PaneId, Vec<usize>)>,
    /// The right-clicked word in the washed page, lit beside it.
    washed_word: Option<mareader_speech::Spot>,
    /// Language search input in the open menu.
    filter: String,
    /// The menu's "Remember my choice" toggle.
    remember: bool,
    pub(crate) study: crate::study_mode::menu::Look,
}

/// Read-only view over engine page-AI ledger and window menu state.
#[derive(Clone, Copy)]
pub struct PageAiView<'a> {
    pub ledger: &'a Ledger,
    ui: &'a PageAiUi,
}

impl<'a> PageAiView<'a> {
    pub fn new(ledger: &'a Ledger, ui: &'a PageAiUi) -> Self {
        Self { ledger, ui }
    }

    pub fn shows(&self, pane: PaneId, page: usize) -> bool {
        self.ledger.shows(pane, page)
    }

    pub fn menu_rect(&self) -> Option<Rect> {
        self.ui.menu_rect()
    }

    pub fn washed(&self, pane: PaneId) -> Option<&'a [usize]> {
        self.ui.washed(pane)
    }

    pub fn washed_word(&self, pane: PaneId) -> Option<mareader_speech::Spot> {
        self.ui.washed_word(pane)
    }

    pub fn on_screen(&self, pane: PaneId) -> bool {
        self.ledger.on_screen(pane)
    }

    pub fn kept(&self, main: &Library, who: (Option<BookId>, &str)) -> Vec<(AiTask, PageSpan)> {
        self.ledger.kept(main, who)
    }
}

/// The task a menu row means, as kept; Translate needs a language.
pub fn task_for(task: PageTask, ai: &AiSettings) -> Option<AiTask> {
    Some(match task {
        PageTask::Summarize => match ai.tasks.summarize.style {
            SummaryStyle::Paragraphs => AiTask::Summary,
            SummaryStyle::Points => AiTask::Points,
        },
        PageTask::Translate => AiTask::Translate {
            language: ai.tasks.translate.language.clone()?,
        },
        PageTask::Simplify => AiTask::Simplify,
    })
}

/// How a bubble names its result.
fn label(task: &AiTask) -> String {
    match task {
        AiTask::Summary => "Summary".to_owned(),
        AiTask::Points => "Key points".to_owned(),
        AiTask::Translate { language } => {
            let name = language_name(language).unwrap_or(language);
            format!("Translation · {name}")
        }
        AiTask::Simplify => "Simplified".to_owned(),
    }
}

/// What a group's page past its result says, as a block to set.
fn group_note(task: &AiTask, (from, to): (usize, usize)) -> Arc<Vec<mareader_reflow::Block>> {
    use mareader_reflow::{Block, Content, Emphasis, Role, Text};
    let done = match task {
        AiTask::Summary | AiTask::Points => "summarized",
        AiTask::Translate { .. } => "translated",
        AiTask::Simplify => "simplified",
    };
    let words = format!(
        "Pages {}–{} are {done} together, starting on page {}.",
        from + 1,
        to + 1,
        from + 1
    );
    let mut text = Text::new(Role::Paragraph);
    let italic = Emphasis {
        italic: true,
        ..Emphasis::default()
    };
    text.push(&words, italic);
    Arc::new(vec![Block {
        content: Content::Text(text),
        frame: Default::default(),
        break_before: false,
    }])
}

/// "pages 3–7", counted from one.
fn pages_label((from, to): (usize, usize)) -> String {
    format!("pages {}–{}", from + 1, to + 1)
}

impl PageAiUi {
    /// Uploads finished card rasters to the GPU and drops stale textures.
    pub fn receive_cards(&mut self, ctx: &Context, gpu: Option<&Gpu>) {
        let now = ctx.input(|i| i.time);
        if self.cards.receive(ctx, gpu, now) {
            ctx.request_repaint();
        }
    }

    /// What `pane`'s pages on screen show of the AI this frame.
    pub fn overlays(&mut self, ctx: &Context, session: &mut Session, pane: PaneId) -> PaneAi {
        let wash = self.wash(ctx, pane);
        let Some((view, index)) = session.page_ai_prepare_overlays(pane) else {
            return PaneAi {
                overlays: Vec::new(),
                wash,
                top: 0.0,
            };
        };
        let groups = session
            .page_ai()
            .spread_groups(session.library_model(), pane, &view);
        let (mut overlays, mut drawn) = (Vec::new(), Vec::new());
        for page in view.visible.clone() {
            let own = view.document.to_base(page);
            let over = groups
                .iter()
                .find(|(from, to, _)| *from < own && own <= *to);
            let (mut slots, mut bubbles, mut card) = (Vec::new(), Vec::new(), None);
            {
                let (ledger, main) = (session.page_ai(), session.library_model());
                for own in view.document.base_pages(page) {
                    for (slot, bubble) in Self::bubbles(ledger, main, &view, (pane, own), &index) {
                        let active = bubble.active;
                        bubbles.push(bubble);
                        slots.push((slot, own));
                        if active && card.is_none() && ledger.carded(pane, &view, slot) {
                            card = Some((own, slot));
                        }
                    }
                }
                if slots.is_empty() && over.is_none() {
                    let key = (pane, view.layout, view.document.to_base(page));
                    if view.document.base_pages(page).is_empty()
                        && let Some(shown) = ledger.showing.get(&key)
                    {
                        drawn.push((page, vec![(shown.slot, key.2)]));
                    }
                    continue;
                }
            }
            for (n, b) in bubbles.iter_mut().enumerate() {
                b.label = (n + 1).to_string();
            }
            let mut card = card
                .and_then(|(own, slot)| self.shown(ctx, session, pane, &view, (page, own), slot));
            if let (None, Some(&(from, _, slot))) = (&card, over) {
                card = self.shown(ctx, session, pane, &view, (page, from), slot);
                slots.push((slot, from));
            }
            overlays.push(Overlay {
                page,
                bubbles,
                card,
            });
            drawn.push((page, slots));
        }
        session.page_ai_record_drawn(pane, view.layout, drawn);
        PaneAi {
            overlays,
            wash,
            top: 0.0,
        }
    }

    /// Own page `own`'s results as bubbles, kept ones first.
    fn bubbles(
        ledger: &Ledger,
        main: &Library,
        view: &AiView,
        (pane, own): (PaneId, usize),
        index: &HashMap<usize, Vec<AiPageId>>,
    ) -> Vec<(Slot, paint::Bubble)> {
        let saved = index.get(&own).map_or(&[][..], Vec::as_slice);
        let runs = ledger
            .runs
            .iter()
            .filter(|r| r.layout == view.layout && r.page == own)
            .filter(|r| r.redo_of.is_none_or(|o| !saved.contains(&o)));
        let shown = ledger.showing.get(&(pane, view.layout, own));
        let active = shown.map(|s| s.slot);
        let here = match &*view.document {
            Document::Epub { book, .. } => extract::span(&book.base_excerpts(own), book.blocks()),
            Document::Pdf(_) => None,
        };
        let mut out = Vec::new();
        for id in saved {
            let Some(p) = ledger.saved(main, *id) else {
                continue;
            };
            let redoing = ledger.runs.iter().find(|r| r.redo_of == Some(*id));
            let covered = pages_of(&view.document, &p.span).filter(|(a, b)| b > a);
            // A group is checked against all the pages it was asked for.
            let stale = match (&p.span, here, covered) {
                (PageSpan::Blocks { from, to }, Some(_), Some((_, last))) => {
                    group_span(&view.document, (own, last)) != Some([*from, *to])
                }
                (PageSpan::Blocks { from, to }, Some(span), None) => [*from, *to] != span,
                _ => false,
            };
            let state = match (redoing, stale) {
                (Some(r), _) if r.failed.is_none() => paint::State::Busy,
                (_, true) => paint::State::Stale,
                _ => paint::State::Ready,
            };
            let slot = Slot::Saved(*id);
            let mut tip = label(&p.task);
            if let Some(pages) = covered {
                tip = format!("{tip} · {}", pages_label(pages));
            }
            if state == paint::State::Stale {
                tip.push_str(" · made for another text size");
            }
            let active =
                active == Some(slot) || redoing.is_some_and(|r| active == Some(Slot::Run(r.id)));
            out.push((
                slot,
                paint::Bubble {
                    label: String::new(),
                    tip,
                    active,
                    state,
                },
            ));
        }
        for r in runs {
            let slot = Slot::Run(r.id);
            let mut what = label(&r.task);
            if r.last > r.page {
                what = format!("{what} · {}", pages_label((r.page, r.last)));
            }
            let (state, tip) = match &r.failed {
                Some(why) => (paint::State::Failed, format!("{what} · {why}")),
                None => (paint::State::Busy, format!("{what} · writing…")),
            };
            let bubble = paint::Bubble {
                label: String::new(),
                tip,
                active: active == Some(slot),
                state,
            };
            out.push((slot, bubble));
        }
        out
    }

    /// The card of `slot` on `page`, set for the pane's type and zoom.
    fn shown(
        &mut self,
        ctx: &Context,
        session: &mut Session,
        pane: PaneId,
        view: &AiView,
        (page, own): (usize, usize),
        slot: Slot,
    ) -> Option<paint::Shown> {
        let since = session
            .page_ai()
            .showing
            .get(&(pane, view.layout, own))?
            .since;
        let old = match slot {
            Slot::Saved(id) => id,
            Slot::Run(id) => {
                let r = session.page_ai().runs.iter().find(|r| r.id == id)?;
                match (&r.shown, r.redo_of) {
                    (Some(shown), _) => {
                        let ident = hash((id, r.shown_at.to_bits()));
                        let what = (
                            r.task.clone(),
                            r.span.clone(),
                            Arc::clone(shown),
                            Some(Arc::clone(&r.source)),
                            ident,
                        );
                        return self.place(ctx, pane, view, page, what, since);
                    }
                    // A redo shows what it replaces until its own words come.
                    (None, Some(old)) => old,
                    (None, None) => return Some(self.blank_card(view, page, since, ctx)),
                }
            }
        };
        let (task, span) = {
            let p = session.page_ai().saved(session.library_model(), old)?;
            (p.task.clone(), p.span.clone())
        };
        let result = session.page_ai_result(old)?;
        let what = (task, span, result, None, hash(old));
        self.place(ctx, pane, view, page, what, since)
    }

    fn room(view: &AiView, page: usize) -> (egui::Vec2, egui::Vec2, f32) {
        match &*view.document {
            Document::Epub { book, .. } if !book.fixed(page) => {
                let [x, y, w, h] = book.text_area(page);
                let width = book.page_size().0;
                (vec2(x, y), vec2(w, h), width)
            }
            _ => {
                let size = view.document.page_sizes().get(page).copied();
                let (w, h) = size.map_or((612.0, 792.0), |s| (s.width, s.height));
                let m = 0.08 * w.min(h);
                (vec2(m, m), vec2(w - 2.0 * m, h - 2.0 * m), w)
            }
        }
    }

    fn alpha(ctx: &Context, since: f64) -> f32 {
        let fade = crate::motion::secs(ctx, FADE_IN);
        let now = ctx.input(|i| i.time);
        let a = if fade <= 0.0 {
            1.0
        } else {
            ((now - since) as f32 / fade).clamp(0.0, 1.0)
        };
        if a < 1.0 {
            ctx.request_repaint();
        }
        a
    }

    /// Paper alone, while the first words are on their way.
    fn blank_card(&self, view: &AiView, page: usize, since: f64, ctx: &Context) -> paint::Shown {
        let (at, room, width) = Self::room(view, page);
        paint::Shown {
            card: None,
            at,
            room,
            alpha: Self::alpha(ctx, since),
            width,
        }
    }

    fn place(
        &mut self,
        ctx: &Context,
        pane: PaneId,
        view: &AiView,
        page: usize,
        (task, span, result, source, ident): What,
        since: f64,
    ) -> Option<paint::Shown> {
        let (at, mut room, width) = Self::room(view, page);
        // A group runs over its pages; a PDF's take its first page's room.
        let group = pages_of(&view.document, &span).filter(|(from, to)| to > from);
        let (pages, part, note) = match group {
            Some((from, to)) => {
                if !view.document.is_reflowable() {
                    room = Self::room(view, from).1;
                }
                let own = view.document.to_base(page);
                let note = group_note(&task, (from, to));
                (to - from + 1, own.saturating_sub(from), Some(note))
            }
            None => (1, 0, None),
        };
        let mut typo: Typography = (*view.typo).clone();
        if matches!(*view.document, Document::Pdf(_)) {
            typo.font_size *= width / PAGE_WIDTH;
        }
        if let AiTask::Translate { language } = &task {
            typo.language = language.clone();
        }
        let ppp = ctx.pixels_per_point();
        let scale = Scale::covering(view.zoom * ppp).value();
        let slot = hash((pane, view.layout, page, &task, &span));
        let key = hash((
            ident,
            format!("{typo:?}"),
            room.x.to_bits(),
            room.y.to_bits(),
            scale.to_bits(),
        ));
        let document = Arc::clone(&view.document);
        let make = || {
            let blocks = match &*document {
                Document::Epub { book, .. } => {
                    let source = source.unwrap_or_else(|| match &span {
                        PageSpan::Blocks { from, to } => {
                            Arc::new(extract::book(book.blocks(), [*from, *to]))
                        }
                        PageSpan::Page { .. } | PageSpan::Pages { .. } => Arc::default(),
                    });
                    extract::blocks(&result, &source, book.blocks())
                }
                Document::Pdf(_) => extract::blocks(&result, &Source::default(), &[]),
            };
            let book = matches!(*document, Document::Epub { .. }).then(|| Arc::clone(&document));
            CardJob {
                blocks: Arc::new(blocks),
                typo,
                room: (room.x, room.y),
                scale,
                book,
                pages,
                part,
                note,
            }
        };
        let now = ctx.input(|i| i.time);
        let card = self.cards.get(slot, key, now, make);
        Some(paint::Shown {
            card,
            at,
            room,
            alpha: Self::alpha(ctx, since),
            width,
        })
    }

    /// A bubble was pressed: its result shows, or the page again.
    pub fn pick(
        &mut self,
        session: &mut Session,
        pane: PaneId,
        page: usize,
        index: usize,
        at: Pos2,
        now: f64,
    ) {
        if let Some(slot) = session.page_ai_pick(pane, page, index, now) {
            self.menu = None;
            self.bubble_menu = Some(BubbleMenu {
                pane,
                page,
                slot,
                at,
            });
        }
    }

    /// Closes any menus tied to a closed pane.
    pub fn close_pane(&mut self, pane: PaneId) {
        if self.menu.as_ref().is_some_and(|m| m.pane == pane) {
            self.menu = None;
        }
        if self.bubble_menu.as_ref().is_some_and(|m| m.pane == pane) {
            self.bubble_menu = None;
        }
    }

    /// The menu's pages in `pane`, fading in with it and out after.
    fn wash(&mut self, ctx: &Context, pane: PaneId) -> Option<paint::Wash> {
        let open = self.menu.as_ref().is_some_and(|m| m.pane == pane);
        let id = Id::new(("page-ai-wash", pane));
        let alpha = ctx.animate_bool_with_time(id, open, WASH_FADE);
        let (at, pages) = self.washed.as_ref()?;
        if *at != pane {
            return None;
        }
        if alpha <= 0.0 && !open {
            self.washed = None;
            return None;
        }
        let hovered = ctx.data(|d| d.get_temp::<bool>(aloud_hot(pane)));
        let id = aloud_hot(pane).with("lit");
        let hot = ctx.animate_bool_with_time(id, open && hovered == Some(true), 0.15);
        Some(paint::Wash {
            pages: pages.clone(),
            alpha,
            word: self.washed_word,
            hot,
        })
    }

    /// Ties the page's open menu to where it was opened on its page.
    pub fn pin_menu(&mut self, pin: Pin) {
        if let Some(m) = &mut self.menu {
            m.pin = Some(pin);
        }
    }

    /// Where the page's menu was drawn last frame; for tests.
    pub fn menu_rect(&self) -> Option<Rect> {
        self.menu.as_ref().and(self.menu_rect)
    }

    /// The right-clicked word lit in `pane`, at or in it; for tests.
    pub fn washed_word(&self, pane: PaneId) -> Option<mareader_speech::Spot> {
        self.washed(pane).and(self.washed_word)
    }

    /// The pages washed in `pane`, while the menu shows or fades; for tests.
    pub fn washed(&self, pane: PaneId) -> Option<&[usize]> {
        let (at, pages) = self.washed.as_ref()?;
        (*at == pane).then_some(pages.as_slice())
    }

    /// Opens the page's menu at a right-click; `aloud` offers read-aloud.
    pub fn open_menu(
        &mut self,
        pane: PaneId,
        page: usize,
        at: Pos2,
        aloud: Option<mareader_engine::read_aloud::ReadFrom>,
    ) {
        self.bubble_menu = None;
        self.filter.clear();
        self.remember = false;
        self.washed = Some((pane, vec![page]));
        self.washed_word = aloud.and_then(|a| a.at);
        self.menu = Some(Menu {
            pane,
            page,
            at,
            step: Step::Tasks,
            pages: Vec::new(),
            aloud,
            pin: None,
            born: None,
        });
    }

    /// Opens the menu for picked pages at its languages, at `at`.
    pub fn open_pages_menu(&mut self, pane: PaneId, pages: Vec<usize>, [x, y]: [f32; 2]) {
        let at = Pos2 { x, y };
        let Some(&page) = pages.first() else {
            return;
        };
        self.bubble_menu = None;
        self.filter.clear();
        self.washed = Some((pane, pages.clone()));
        self.washed_word = None;
        self.menu = Some(Menu {
            pane,
            page,
            at,
            step: Step::Languages,
            pages,
            aloud: None,
            pin: None,
            born: None,
        });
    }

    /// Opens a bubble's menu, at a right-click.
    pub fn open_bubble_menu(
        &mut self,
        ledger: &Ledger,
        pane: PaneId,
        page: usize,
        index: usize,
        at: Pos2,
    ) {
        if let Some((_, slot, _)) = ledger.slot_at(pane, page, index) {
            self.menu = None;
            self.bubble_menu = Some(BubbleMenu {
                pane,
                page,
                slot,
                at,
            });
        }
    }

    /// A menu is open, so Escape is the page AI's.
    pub fn escape(&mut self) -> bool {
        let open = self.menu.is_some() || self.bubble_menu.is_some();
        self.menu = None;
        self.bubble_menu = None;
        open
    }

    /// Tests: results are set on the frame, not on a thread.
    pub fn set_inline(&mut self, inline: bool) {
        self.cards.set_inline(inline);
    }
}
