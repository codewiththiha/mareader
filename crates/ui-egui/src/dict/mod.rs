//! Dictionaries in the app: the lookup card, the page, the packs.

pub mod ai_edit;
pub mod card;
mod card_state;
mod entry;
mod look;
pub mod page;
mod pickers;
mod sense;
pub mod window;
mod window_chrome;
mod window_geom;
mod window_tab;

use egui::{Context, Rect, Vec2};
use mareader_cefr::Role;
use mareader_dict::{Pack, Reply, State, Ticket, WordMatch};
use mareader_engine::dict::{Action, Service as DictService};
use mareader_engine::read_aloud::state::{Offer, Speaker};
use mareader_engine::session::Session;
use mareader_engine::study::words::Asked;
use mareader_settings::DictionarySettings;
use mareader_study::Pos;
use mareader_workspace::PaneId;

use crate::theme::Tokens;
use crate::ui_command::Commands;

/// Room around a rested card's word and itself the pointer may cross.
const LEAVE: f32 = 8.0;

/// A rect as the engine's window bounds: x, y, width, height.
fn bounds(rect: Rect) -> [f32; 4] {
    [rect.min.x, rect.min.y, rect.width(), rect.height()]
}

/// A lookup card, kept where its words were selected.
#[derive(Clone, Debug, PartialEq)]
struct Card {
    pane: PaneId,
    word: String,
    context: String,
    asked: Option<Asked>,
    at: Rect,
    /// The pane's scroll when it opened; a scroll away closes it.
    offset: Option<Vec2>,
    ticket: Ticket,
    reply: Option<Reply>,
    /// Opened by resting on a shaded word; leaving it closes the card.
    rest: bool,
    /// The word's class and level in its sentence, once the tagger said.
    role: Option<Role>,
    /// The class its dictionary rows name, once in.
    dictionary: Option<Pos>,
}

mareader_engine::impl_card!(Card);

/// How the Dictionary's words sound and which the voice can say.
#[derive(Clone, Debug, Default)]
pub struct Voice {
    /// The word being said aloud.
    pub saying: Option<String>,
    pub sounds: Vec<(String, Vec<mareader_engine::sounds::Line>)>,
    /// Who says words in which language, the picked voice first.
    pub speakers: Vec<Speaker>,
    /// Languages the engine has voices for, waiting to be downloaded.
    pub offers: Vec<Offer>,
}

impl Voice {
    /// Who says words in `language`, as `fr`.
    pub fn speaker(&self, language: &str) -> Option<&Speaker> {
        let language = mareader_speech::primary(language);
        self.speakers.iter().find(|s| s.language == language)
    }

    pub fn can_say(&self, language: &str) -> bool {
        self.speaker(language).is_some()
    }

    /// The download that would give `language` a voice.
    pub fn offer(&self, language: &str) -> Option<Offer> {
        let language = mareader_speech::primary(language);
        let wanted = |o: &&Offer| mareader_speech::primary(o.tag) == language;
        self.offers.iter().find(wanted).copied()
    }

    pub fn lines(&self, word: &str) -> &[mareader_engine::sounds::Line] {
        let found = self.sounds.iter().find(|(w, _)| w == word);
        found.map_or(&[], |(_, lines)| lines.as_slice())
    }
}

/// Window-ephemeral dictionary state: card, page, floating window, and voice.
#[derive(Default)]
pub struct DictUi {
    card: Option<Card>,
    stash: Option<Card>,
    card_rect: Option<Rect>,
    page: page::Page,
    /// The floating window's rect mid-drag, and where it was drawn.
    float: Option<Rect>,
    /// Whether a pointer is moving or resizing the window.
    dragging: bool,
    float_rect: Option<Rect>,
    float_cover: Option<Rect>,
    /// Home and aside rects while stepped away from an obstacle.
    evacuated: Option<(Rect, Rect)>,
    /// The window's original shape at its edge, restoring on click.
    parked: Option<crate::park::Tuck>,
    /// Whether it was folded when last drawn, to catch it folding.
    float_folded: Option<bool>,
    /// The card keeps only entries for the word itself.
    exact_only: bool,
    /// What the voice can say, as of this frame.
    voice: Voice,
    editor: Option<ai_edit::Editor>,
}

/// Read-only view over engine dictionary service and window overlays.
#[derive(Clone, Copy)]
pub struct DictView<'a> {
    service: &'a DictService,
    ui: &'a DictUi,
}

impl<'a> DictView<'a> {
    pub fn new(service: &'a DictService, ui: &'a DictUi) -> Self {
        Self { service, ui }
    }

    pub fn attached(&self) -> bool {
        self.service.attached()
    }

    pub fn state(&self, pack: &Pack) -> &'a State {
        self.service.state(pack)
    }

    pub fn ready(&self) -> Vec<&'static Pack> {
        self.service.ready()
    }

    /// Every dictionary language, the AI dictionary's too.
    pub fn languages(&self) -> Vec<String> {
        self.service.languages()
    }

    pub fn card_role(&self) -> Option<&'a Role> {
        self.ui.card_role()
    }

    pub fn card_rested(&self) -> bool {
        self.ui.card_rested()
    }

    pub fn card_word(&self) -> Option<&'a str> {
        self.ui.card_word()
    }

    pub fn card_place(&self) -> Option<(Rect, Rect)> {
        self.ui.card_place()
    }

    pub fn card_answers(&self) -> Option<Vec<String>> {
        self.ui.card_answers()
    }

    pub fn window_rect(&self) -> Option<Rect> {
        self.ui.window_rect()
    }

    pub fn window_cover(&self) -> Option<Rect> {
        self.ui.window_cover()
    }

    pub fn typing(&self) -> bool {
        self.ui.typing()
    }

    pub fn page_text(&self) -> &'a str {
        self.ui.page_text()
    }

    pub fn page_field(&self) -> Option<Rect> {
        self.ui.page_field()
    }
}

impl DictUi {
    /// Opens the lookup card for an already-submitted dictionary query.
    pub fn show_lookup(
        &mut self,
        pane: PaneId,
        word: String,
        ticket: Ticket,
        at: Rect,
        offset: Option<Vec2>,
        asked: Option<Asked>,
    ) {
        self.stash = None;
        let context = asked
            .as_ref()
            .map(|a| a.context.clone())
            .unwrap_or_default();
        self.card = Some(Card {
            pane,
            word,
            context,
            asked,
            at,
            offset,
            ticket,
            reply: None,
            rest: false,
            role: None,
            dictionary: None,
        });
    }

    /// A rest on a shaded word opens its card, unless one was asked.
    pub fn rest(
        &mut self,
        session: &mut Session,
        pane: PaneId,
        (word, context): (String, String),
        (at, offset): (Rect, Option<Vec2>),
        asked: Option<Asked>,
    ) -> bool {
        match &self.card {
            Some(c) if !c.rest || (c.word == word && c.at == at) => return false,
            _ => {}
        }
        let ticket = session.dict_look_up(&word);
        self.show_lookup(pane, word, ticket, at, offset, asked);
        if let Some(card) = &mut self.card {
            card.rest = true;
            if card.context.is_empty() {
                card.context = context;
            }
        }
        self.card_rect = None;
        true
    }

    /// Whether the open card came from resting on a word, for tests.
    pub fn card_rested(&self) -> bool {
        self.card.as_ref().is_some_and(|c| c.rest)
    }

    /// The word the card shows.
    pub fn card_word(&self) -> Option<&str> {
        self.card.as_ref().map(|c| c.word.as_str())
    }

    /// Where the card was drawn and the words it points at, for tests.
    pub fn card_place(&self) -> Option<(Rect, Rect)> {
        Some((self.card_rect?, self.card.as_ref()?.at))
    }

    /// The card's answers once they came, for tests.
    pub fn card_answers(&self) -> Option<Vec<String>> {
        let reply = self.card.as_ref()?.reply.as_ref()?;
        Some(
            reply
                .entries
                .iter()
                .map(|e| e.answer().to_owned())
                .collect(),
        )
    }

    /// Escape: closes the card; false if none was open.
    pub fn escape(&mut self) -> bool {
        self.stash = None;
        self.card.take().is_some()
    }

    pub fn act(&mut self, action: Action) {
        if action == Action::CloseCard {
            self.escape();
        }
    }

    /// How words sound and what the voice speaks, before drawing.
    pub fn set_voice(&mut self, voice: Voice) {
        self.voice = voice;
    }

    /// Words the page and window would show the sound of.
    pub fn sound_words(&self) -> Vec<String> {
        self.page.sound_words()
    }

    /// Who says the card's word, if any voice here speaks its language.
    fn card_speaker(&self, c: &Card) -> Option<&Speaker> {
        let reply = c.reply.as_ref();
        let first = reply.and_then(|r| r.entries.first());
        let from = reply.and_then(|r| r.from.clone());
        let side = || first.map(|e| entry::sides(e, None).0);
        let language = from.map(|l| mareader_speech::primary(&l)).or_else(side);
        self.voice
            .speaker(&language.unwrap_or_else(|| mareader_dict::HUB.to_owned()))
    }

    /// Whether the card leaves out near matches, as settings say.
    pub fn set_exact_only(&mut self, on: bool) {
        self.exact_only = on;
    }

    /// Applies dictionary replies and pack changes; true if packs changed.
    pub fn receive(&mut self, changed: bool, replies: Vec<Reply>) -> bool {
        for reply in replies {
            match (&mut self.card, &mut self.stash) {
                (Some(card), _) | (_, Some(card)) if card.ticket == reply.ticket => {
                    let mut reply = reply;
                    if self.exact_only {
                        reply.entries.retain(|e| e.fit == WordMatch::Exact);
                    }
                    card.reply = Some(reply);
                }
                _ => self.page.receive(reply),
            }
        }
        if changed {
            // A pack came or went: what the page showed may differ.
            self.page.stale();
        }
        changed
    }

    /// After the panes: the card over them, if open.
    pub fn show(
        &mut self,
        ctx: &Context,
        (attached, ai_ready, languages): (bool, bool, card::Languages<'_>),
        (panes, heard): (&[(PaneId, Rect)], mareader_engine::sounds::Heard<'_>),
        pane_info: impl Fn(PaneId) -> (Option<Vec2>, Option<usize>),
        out: &mut Commands,
    ) {
        let pressed = ctx.input(|i| i.pointer.press_origin().filter(|_| i.pointer.any_pressed()));
        let outside = pressed.is_some_and(|p| !self.card_rect.is_some_and(|r| r.contains(p)));
        let room = |pane| panes.iter().find(|(p, _)| *p == pane).map(|(_, r)| *r);
        let moved = |c: &Card| match (c.offset, pane_info(c.pane).0) {
            (Some(a), Some(b)) => (a - b).length() > 0.5,
            _ => false,
        };
        // A rested card stays while the pointer is on its word or itself.
        let pointer = ctx.input(|i| i.pointer.latest_pos());
        let left = |c: &Card| {
            let near = self.card_rect.map_or(c.at, |r| r.union(c.at)).expand(LEAVE);
            c.rest && !pointer.is_some_and(|p| near.contains(p))
        };
        let dismiss = self
            .card
            .as_ref()
            .filter(|_| self.editor.is_none())
            .is_some_and(|c| outside || room(c.pane).is_none() || moved(c) || left(c));
        if dismiss {
            self.card = None;
            self.stash = None;
        }
        let t = Tokens::get(ctx);
        self.card_rect = self.card.as_ref().and_then(|c| {
            let speaker = self.card_speaker(c);
            let page = c
                .asked
                .as_ref()
                .map(|a| a.page)
                .or_else(|| pane_info(c.pane).1);
            let view = card::View {
                pane: c.pane,
                page,
                word: &c.word,
                reply: c.reply.as_ref(),
                role: c.role.as_ref(),
                dictionary: c.dictionary,
                attached,
                ai_ready,
                languages,
                at: c.at,
                room: room(c.pane)?,
                saying: heard.saying(&c.word),
                sound: heard.lines(&c.word),
                speaker,
            };
            Some(card::show(ctx, &view, &t, out))
        });
    }

    /// The Dictionary page, below `top`.
    pub fn page(
        &mut self,
        ui: &mut egui::Ui,
        session: &mut Session,
        settings: &DictionarySettings,
        top: f32,
        out: &mut Commands,
    ) {
        self.page
            .show(ui, session, (settings, &self.voice), top, out);
    }

    /// The page came up: its field takes the keys.
    pub fn arrive(&mut self) {
        self.card = None;
        self.page.arrive();
    }

    /// Draws the floating window and reports a user's interaction with it.
    pub fn window(
        &mut self,
        ctx: &Context,
        session: &mut Session,
        (settings, parking): (&DictionarySettings, mareader_settings::EdgeParking),
        (stage, toasts, player): (Rect, Option<Rect>, Option<Rect>),
        out: &mut Commands,
    ) -> bool {
        self.float_rect = None;
        self.float_cover = None;
        if !settings.window.open || settings.window.minimized {
            self.page.hide();
        }
        // Asked every frame, so even the first opening comes in softly.
        let appear = window::appearing(ctx, settings.window.open);
        if !settings.window.open {
            self.float = None;
            self.dragging = false;
            self.evacuated = None;
            self.parked = None;
        }
        if appear <= 0.0 {
            return false;
        }
        let frame = window::Frame {
            session,
            page: &mut self.page,
            voice: &self.voice,
            live: &mut self.float,
            dragging: &mut self.dragging,
            was_folded: &mut self.float_folded,
            evacuated: &mut self.evacuated,
            parked: &mut self.parked,
            settings,
            parking,
            stage,
            toasts,
            player,
        };
        let (drawn, interacted) = window::show(ctx, frame, appear, out);
        self.float_rect = settings.window.open.then_some(drawn);
        self.float_cover = settings.window.open.then_some(drawn.intersect(stage));
        interacted
    }

    pub fn edge_tab_active(&self) -> bool {
        self.parked.is_some_and(|tuck| !tuck.revealing)
    }

    /// Where the floating window was drawn, for tests.
    pub fn window_rect(&self) -> Option<Rect> {
        self.float_rect
    }

    /// The part of the floating window visible to collision checks.
    pub fn window_cover(&self) -> Option<Rect> {
        self.float_cover
    }

    /// Whether the page's or window's field had the keys last frame.
    pub fn typing(&self) -> bool {
        self.page.typing()
    }

    /// Escape on the page: clears its search; false if empty.
    pub fn page_escape(&mut self) -> bool {
        self.page.escape()
    }

    /// Searches the page for `word` at once.
    pub fn search(&mut self, word: &str) {
        self.page.search(word);
    }

    /// Shows `word` in the window, leaving the keys where they are.
    pub fn feed(&mut self, word: &str) {
        self.page.feed(word);
    }

    /// What the page's field holds, for tests.
    pub fn page_text(&self) -> &str {
        self.page.text()
    }

    /// Where the page's field was drawn last, if it was.
    pub fn page_field(&self) -> Option<Rect> {
        self.page.field_at()
    }
}

/// Bytes as megabytes, one decimal.
pub fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}

/// A count with thin spaces between thousands.
pub fn count(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('\u{202f}');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_japanese_voice_says_the_dictionarys_japanese() {
        let speaker = |language: &str| Speaker {
            language: language.into(),
            voice: None,
        };
        let voice = Voice {
            speakers: vec![speaker("en"), speaker("ja")],
            ..Voice::default()
        };
        assert!(voice.can_say("jp"));
        assert!(!voice.can_say("zh"));
    }

    #[test]
    fn counts_and_sizes_read_well() {
        assert_eq!(count(110_640), "110\u{202f}640");
        assert_eq!(count(999), "999");
        assert_eq!(megabytes(2_875_539), "2.9 MB");
    }
}
