//! Dictionaries in the app: the lookup card, the page, the packs.

pub mod card;
mod entry;
mod look;
pub mod page;
pub mod window;

use egui::{Context, Rect, Vec2};
use mareader_cefr::Role;
use mareader_dict::{Dictionaries, PACKS, Pack, Query, Reply, State, Ticket, WordMatch};
use mareader_settings::DictionarySettings;
use mareader_workspace::PaneId;

use crate::command::Commands;
pub use crate::read_aloud::{Offer, Speaker};
use crate::theme::Tokens;

/// Room around a rested card's word and itself the pointer may cross.
const LEAVE: f32 = 8.0;

/// What the card, the page and the settings ask of the dictionaries.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Download(&'static str),
    Pause(&'static str),
    Cancel(&'static str),
    Remove(&'static str),
    CloseCard,
    /// The card's word, carried on into the window or the page.
    Pursue(String),
    /// The reader's floating window, which the app keeps.
    Window(Window),
}

/// What the floating window asks: shown, folded, placed or a page.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Window {
    /// Shown if closed or folded, else closed.
    Toggle,
    /// Shown, its field given the keys; the shortcut on an idle window.
    Focus,
    Close,
    Minimize(bool),
    /// Let go after a drag or resize: saved.
    Place(Rect),
    /// Closed, and the Dictionary page in the panes' place.
    Expand,
}

/// A lookup card, kept where its words were selected.
#[derive(Clone, Debug, PartialEq)]
struct Card {
    pane: PaneId,
    word: String,
    at: Rect,
    /// The pane's scroll when it opened; a scroll away closes it.
    offset: Option<Vec2>,
    ticket: Ticket,
    reply: Option<Reply>,
    /// Opened by resting on a shaded word; leaving it closes the card.
    rest: bool,
    /// The word's class and level in its sentence, once the tagger said.
    role: Option<Role>,
}

crate::word_card::impl_card!(Card);

/// How the Dictionary's words sound and which the voice can say.
#[derive(Clone, Debug, Default)]
pub struct Voice {
    /// The word being said aloud.
    pub saying: Option<String>,
    pub sounds: Vec<(String, Vec<crate::sounds::Line>)>,
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

    pub fn lines(&self, word: &str) -> &[crate::sounds::Line] {
        let found = self.sounds.iter().find(|(w, _)| w == word);
        found.map_or(&[], |(_, lines)| lines.as_slice())
    }
}

/// Word list asks kept waiting at once.
const WORD_ASKS: usize = 4;

#[derive(Default)]
pub struct Dict {
    service: Dictionaries,
    card: Option<Card>,
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
    /// Word lists' asks and, once in, their replies; oldest first.
    words: Vec<(Ticket, Option<Reply>)>,
    /// The card keeps only entries for the word itself.
    exact_only: bool,
    /// What the voice can say, as of this frame.
    voice: Voice,
}

impl Dict {
    /// Keeps packs in `dir`; `waker` repaints the app as work lands.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn attach(
        &mut self,
        dir: std::path::PathBuf,
        waker: mareader_dict::Waker,
    ) -> Result<(), String> {
        self.service.attach(dir, waker)
    }

    /// Answers on this thread, as tests need.
    pub fn set_inline(&mut self, inline: bool) {
        self.service.set_inline(inline);
    }

    pub fn attached(&self) -> bool {
        self.service.attached()
    }

    pub fn state(&self, pack: &Pack) -> &State {
        self.service.state(pack)
    }

    pub fn ready(&self) -> Vec<&'static Pack> {
        self.service.ready()
    }

    /// Look up pressed: the card for `word` beside it, answered in the
    /// default language.
    pub fn look_up(
        &mut self,
        pane: PaneId,
        word: String,
        at: Rect,
        offset: Option<Vec2>,
        to: Option<String>,
    ) {
        let query = Query::LookUp {
            word: word.clone(),
            to,
        };
        let ticket = self.service.ask(query);
        self.card = Some(Card {
            pane,
            word,
            at,
            offset,
            ticket,
            reply: None,
            rest: false,
            role: None,
        });
    }

    /// A rest on a shaded word opens its card, unless one was asked.
    pub fn rest(
        &mut self,
        pane: PaneId,
        word: String,
        at: Rect,
        offset: Option<Vec2>,
        to: Option<String>,
    ) -> bool {
        match &self.card {
            Some(c) if !c.rest || (c.word == word && c.at == at) => return false,
            _ => {}
        }
        self.look_up(pane, word, at, offset, to);
        if let Some(card) = &mut self.card {
            card.rest = true;
        }
        self.card_rect = None;
        true
    }

    /// The tagger's answer for the open card, if it is still `word`'s.
    pub fn set_role(&mut self, word: &str, role: Role) {
        crate::word_card::set_role(&mut self.card, word, role);
    }

    /// The role the open card shows, for tests.
    pub fn card_role(&self) -> Option<&Role> {
        self.card.as_ref()?.role.as_ref()
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
        self.card.take().is_some()
    }

    pub fn act(&mut self, action: Action) {
        let pack = |id: &str| PACKS.iter().find(|p| p.id == id);
        #[cfg(not(target_arch = "wasm32"))]
        match action {
            Action::Download(id) => pack(id).map(|p| self.service.download(p)),
            Action::Pause(id) => pack(id).map(|p| self.service.pause(p)),
            Action::Cancel(id) => pack(id).map(|p| self.service.cancel(p)),
            Action::Remove(id) => pack(id).map(|p| self.service.remove(p)),
            Action::CloseCard => self.card.take().map(|_| ()),
            Action::Pursue(_) | Action::Window(_) => None,
        };
        #[cfg(target_arch = "wasm32")]
        if action == Action::CloseCard {
            self.card = None;
        }
        let _ = pack;
    }

    /// The languages English words can be looked up in.
    pub fn languages(&self) -> Vec<&'static str> {
        let ready = self.service.ready().into_iter();
        ready
            .filter(|p| p.source == mareader_dict::HUB)
            .map(|p| p.target)
            .collect()
    }

    /// Asks for a word list's rows; `take_words` hands the reply over.
    pub fn ask_words(&mut self, words: Vec<String>, to: Vec<String>, near: bool) -> Ticket {
        let ticket = self.service.ask(Query::Words { words, to, near });
        // A list given up on leaves its ask; a few are kept at most.
        if self.words.len() >= WORD_ASKS {
            self.words.remove(0);
        }
        self.words.push((ticket, None));
        ticket
    }

    pub fn take_words(&mut self, ticket: Ticket) -> Option<Reply> {
        let at = self
            .words
            .iter()
            .position(|(t, r)| *t == ticket && r.is_some())?;
        self.words.remove(at).1
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

    /// Takes in replies and progress; true if a pack's state changed.
    pub fn poll(&mut self) -> bool {
        let (changed, replies) = self.service.poll();
        for reply in replies {
            if let Some((_, slot)) = self.words.iter_mut().find(|(t, _)| *t == reply.ticket) {
                *slot = Some(reply);
                continue;
            }
            match &mut self.card {
                Some(card) if card.ticket == reply.ticket => {
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
        (panes, heard): (&[(PaneId, Rect)], crate::sounds::Heard<'_>),
        offset: impl Fn(PaneId) -> Option<Vec2>,
        out: &mut Commands,
    ) {
        let pressed = ctx.input(|i| i.pointer.press_origin().filter(|_| i.pointer.any_pressed()));
        let outside = pressed.is_some_and(|p| !self.card_rect.is_some_and(|r| r.contains(p)));
        let room = |pane| panes.iter().find(|(p, _)| *p == pane).map(|(_, r)| *r);
        let moved = |c: &Card| match (c.offset, offset(c.pane)) {
            (Some(a), Some(b)) => (a - b).length() > 0.5,
            _ => false,
        };
        // A rested card stays while the pointer is on its word or itself.
        let pointer = ctx.input(|i| i.pointer.latest_pos());
        let left = |c: &Card| {
            let near = self.card_rect.map_or(c.at, |r| r.union(c.at)).expand(LEAVE);
            c.rest && !pointer.is_some_and(|p| near.contains(p))
        };
        if outside
            || self
                .card
                .as_ref()
                .is_some_and(|c| room(c.pane).is_none() || moved(c) || left(c))
        {
            self.card = None;
        }
        let t = Tokens::get(ctx);
        let attached = self.attached();
        self.card_rect = self.card.as_ref().and_then(|c| {
            let speaker = self.card_speaker(c);
            let view = card::View {
                word: &c.word,
                reply: c.reply.as_ref(),
                role: c.role.as_ref(),
                attached,
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
        settings: &DictionarySettings,
        top: f32,
        out: &mut Commands,
    ) {
        let Self {
            service,
            page,
            voice,
            ..
        } = self;
        page.show(ui, service, (settings, voice), top, out);
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
        (settings, parking): (&DictionarySettings, mareader_settings::EdgeParking),
        stage: Rect,
        toasts: Option<Rect>,
        player: Option<Rect>,
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
            service: &mut self.service,
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

/// The selection on one line, edge punctuation trimmed; none past `max`.
pub fn lookup_text(selected: &str, max: usize) -> Option<String> {
    let joined = selected.split_whitespace().collect::<Vec<_>>().join(" ");
    let edge = |c: char| !c.is_alphanumeric() && c != '%';
    let word = joined.trim_matches(edge);
    let fits = !word.is_empty() && word.chars().count() <= max;
    fits.then(|| word.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_selection_becomes_a_tidy_lookup() {
        assert_eq!(lookup_text("  “Light,”\n", 40), Some("Light".into()));
        assert_eq!(lookup_text("100% sure.", 40), Some("100% sure".into()));
        assert_eq!(lookup_text("it’s", 40), Some("it’s".into()));
        assert_eq!(lookup_text("…", 40), None);
        assert_eq!(lookup_text("a long, long phrase", 6), None);
    }

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
