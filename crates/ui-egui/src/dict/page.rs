//! The Dictionary page: a search, its results and one entry in full.

use egui::{
    Align, Button, CornerRadius, Frame, Key, Layout, Margin, Modifiers, Rect, Response, RichText,
    ScrollArea, Sense, Shape, Stroke, Ui, UiBuilder, WidgetInfo, WidgetType, pos2, vec2,
};
use mareader_dict::{
    Entry, HUB, Pack, Pair, Query, Reply, Ticket, WordMatch, fold, language, languages,
};
use mareader_engine::session::Session;
use mareader_settings::DictionarySettings;

use super::{Voice, entry, look};
use crate::icons::Icon;
use crate::theme::{self, Tokens};
use crate::ui_command::{Commands, Sent, UiCommand};
use crate::{shaped, widgets};
use mareader_engine::command::Command;
use mareader_engine::settings::Tab;
use mareader_engine::toast::Toast;

/// The search field's name, for screen readers and tests.
pub const LABEL: &str = "Search the dictionaries";

/// Typing settles this long before the packs are asked.
const DEBOUNCE: f64 = 0.18;
/// From this width the results and the entry sit side by side.
const WIDE: f32 = 640.0;
const LIST_W: f32 = 300.0;
/// The page's column, centred on wide windows.
const MAX_W: f32 = 1080.0;
const MARGIN: f32 = 24.0;
/// A short answer can be looked up in turn.
const SHORT: usize = 32;

#[derive(Default)]
pub struct Page {
    text: String,
    typed_at: f64,
    /// The ask sent last, to know what its reply answers.
    asked: Option<(Ticket, Query)>,
    reply: Option<Reply>,
    /// The result shown in full.
    chosen: usize,
    /// Arriving or cleared: the field takes the keys.
    focus: bool,
    /// The chosen row moved by key, so it scrolls into view.
    reveal: bool,
    /// Drawn in the floating window: a smaller field.
    small: bool,
    /// Whether the field had the keys when last drawn.
    typing: bool,
    /// Where the field was drawn last; the title bar keeps off it.
    field_at: Option<Rect>,
}

/// What a click on the page asks of it.
enum Act {
    Choose(usize),
    Search(String),
}

impl Page {
    pub fn receive(&mut self, reply: Reply) {
        if self.asked.as_ref().is_some_and(|(t, _)| *t == reply.ticket) {
            self.reply = Some(reply);
            self.chosen = 0;
        }
    }

    /// The packs changed: ask again.
    pub fn stale(&mut self) {
        self.asked = None;
    }

    /// The page came up: the field takes the keys.
    pub fn arrive(&mut self) {
        self.focus = true;
    }

    /// Escape clears a search; false when there was none.
    pub fn escape(&mut self) -> bool {
        if self.text.is_empty() {
            return false;
        }
        self.text.clear();
        self.focus = true;
        true
    }

    /// Searches `word` at once, as a recent or a reverse lookup does.
    pub fn search(&mut self, word: &str) {
        self.text = word.to_owned();
        self.typed_at = f64::NEG_INFINITY;
        self.focus = true;
    }

    /// Searches `word` at once, but the reader keeps the keys.
    pub fn feed(&mut self, word: &str) {
        self.text = word.to_owned();
        self.typed_at = f64::NEG_INFINITY;
    }

    /// The top answer to the search, for the folded window's glance.
    pub fn glance(&self) -> Option<&str> {
        let asked = self.text.trim();
        let fresh = self.asked.as_ref().is_some_and(|(_, q)| match q {
            Query::Search { text, .. } => text == asked,
            _ => false,
        });
        let e = self.reply.as_ref()?.entries.first()?;
        (fresh && !asked.is_empty()).then(|| e.answer())
    }

    pub fn entry_count(&self) -> usize {
        let fresh = self.asked.as_ref().is_some_and(|(_, q)| match q {
            Query::Search { text, .. } => text == self.text.trim(),
            _ => false,
        });
        if fresh {
            self.reply.as_ref().map_or(0, |reply| reply.entries.len())
        } else {
            0
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn field_at(&self) -> Option<Rect> {
        self.field_at
    }

    /// Whether the field had the keys last frame; Escape drops them first.
    pub fn typing(&self) -> bool {
        self.typing
    }

    /// Not drawn this frame, so not typed into.
    pub fn hide(&mut self) {
        self.typing = false;
    }

    /// The words the page shows the sound of: the word and the one chosen.
    pub fn sound_words(&self) -> Vec<String> {
        let Some(reply) = self.reply.as_ref() else {
            return Vec::new();
        };
        let asked = self.text.trim();
        let mut words: Vec<String> = headword(reply, asked).into_iter().collect();
        let chosen = reply.entries.get(self.chosen).map(|e| e.asked().to_owned());
        words.extend(chosen.filter(|w| !words.contains(w)));
        words
    }

    /// The page in whatever room `ui` has, as the floating window shows it.
    pub fn compact(
        &mut self,
        ui: &mut Ui,
        session: &mut Session,
        (s, voice): (&DictionarySettings, &Voice),
        out: &mut Commands,
    ) {
        let t = Tokens::get(ui.ctx());
        self.small = true;
        self.contents(ui, session, (s, voice), &t, out);
    }

    /// Asks again once typing settles and the ask has changed.
    pub fn ask(&mut self, ctx: &egui::Context, session: &mut Session, s: &DictionarySettings) {
        let now = ctx.input(|i| i.time);
        let query = self.query(s);
        let settled = now - self.typed_at >= DEBOUNCE;
        let changed = self.asked.as_ref().is_none_or(|(_, q)| *q != query);
        if changed && !settled {
            let wait = (DEBOUNCE - (now - self.typed_at)) as f32;
            ctx.request_repaint_after_secs(wait);
        } else if changed {
            let ticket = session.dict_ask(query.clone());
            self.asked = Some((ticket, query));
        }
    }

    /// The ask the page's text and choices make now.
    fn query(&self, s: &DictionarySettings) -> Query {
        let pair = s.pair.then(|| Pair {
            from: s.from.clone(),
            to: s.to.clone(),
        });
        Query::Search {
            text: self.text.trim().to_owned(),
            pair,
            pack: s.langs.first().cloned().filter(|_| !s.pair),
        }
    }

    /// The whole page below `top`, its column centred.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        session: &mut Session,
        settings: (&DictionarySettings, &Voice),
        top: f32,
        out: &mut Commands,
    ) {
        let t = Tokens::get(ui.ctx());
        self.small = false;
        self.field_at = None;
        let full = ui.max_rect();
        ui.painter().rect_filled(full, 0.0, t.paper);
        let width = (full.width() - 2.0 * MARGIN).clamp(0.0, MAX_W);
        let left = full.center().x - width / 2.0;
        let bottom = (full.bottom() - MARGIN).max(top + 12.0);
        let col = Rect::from_min_max(pos2(left, top + 16.0), pos2(left + width, bottom));
        let builder = UiBuilder::new()
            .max_rect(col)
            .layout(Layout::top_down(Align::Min));
        ui.scope_builder(builder, |ui| self.contents(ui, session, settings, &t, out));
    }

    /// Field, choices and results, in whatever room `ui` has.
    fn contents(
        &mut self,
        ui: &mut Ui,
        session: &mut Session,
        (s, voice): (&DictionarySettings, &Voice),
        t: &Tokens,
        out: &mut Commands,
    ) {
        ui.spacing_mut().item_spacing.y = 10.0;
        if !session.dict().attached() {
            let line = "Dictionaries need the desktop app.";
            empty(ui, t, "No dictionaries here", line, None);
            return;
        }
        let ready = session.dict().ready();
        if ready.is_empty() {
            let line = "No dictionary is downloaded.";
            if empty(
                ui,
                t,
                "No dictionaries yet",
                line,
                Some("Get dictionaries…"),
            ) {
                out.push(Sent::Engine(Command::OpenSettings(Tab::Dictionary)));
            }
            return;
        }
        let mut draft = s.clone();
        let now = ui.input(|i| i.time);
        self.field(ui, t, now, &mut draft);
        let reply = self.reply.as_ref();
        match self.small {
            true => narrow_choices(ui, &mut draft, &ready, reply, t, out),
            false => choices(ui, &mut draft, &ready, reply, t, out),
        }
        self.ask(ui.ctx(), session, &draft);
        let asked = self.text.trim().to_owned();
        let act = if asked.is_empty() {
            home(ui, t, &ready, &mut draft)
        } else {
            match self.reply.take() {
                Some(reply) => {
                    let room = (&mut draft, &ready[..], voice);
                    let act = self.results(ui, t, &reply, &asked, room, out);
                    self.reply = Some(reply);
                    act
                }
                None => {
                    ui.label(RichText::new("Searching…").color(t.muted));
                    None
                }
            }
        };
        match act {
            Some(Act::Choose(i)) => {
                self.chosen = i;
                draft.remember(&asked);
            }
            Some(Act::Search(word)) => self.search(&word),
            None => {}
        }
        if draft != *s {
            out.push(Sent::Engine(Command::SetDictionary(Box::new(draft))));
        }
    }

    /// The search field; arrows pick a result, Enter keeps the search.
    fn field(&mut self, ui: &mut Ui, t: &Tokens, now: f64, s: &mut DictionarySettings) {
        let id = ui.make_persistent_id("dict-search");
        let had = ui.memory(|m| m.has_focus(id));
        let count = self.reply.as_ref().map_or(0, |r| r.entries.len());
        if had && count > 0 {
            let (down, up) = ui.input_mut(|i| {
                let down = i.consume_key(Modifiers::NONE, Key::ArrowDown);
                (down, i.consume_key(Modifiers::NONE, Key::ArrowUp))
            });
            if down || up {
                let at = self.chosen.min(count - 1);
                self.chosen = if down {
                    (at + 1).min(count - 1)
                } else {
                    at.saturating_sub(1)
                };
                self.reveal = true;
            }
        }
        let (height, size) = if self.small {
            (40.0, 15.0)
        } else {
            (50.0, 18.0)
        };
        let look = look::Field {
            text: &mut self.text,
            id,
            hint: "Search any word…",
            height,
            size,
        };
        let typed = look::field(ui, look, t);
        let field = typed.field;
        self.field_at = Some(field.rect);
        let name = |b: &mut egui::accesskit::Node| b.set_label(LABEL);
        ui.ctx().accesskit_node_builder(field.id, name);
        if field.changed() {
            self.typed_at = now;
        }
        if field.has_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            s.remember(&self.text);
        }
        if typed.cleared {
            self.text.clear();
            self.focus = true;
        }
        if std::mem::take(&mut self.focus) {
            field.request_focus();
        }
        self.typing = field.has_focus();
    }

    /// The reply: a list and the chosen entry, or one column when narrow.
    fn results(
        &mut self,
        ui: &mut Ui,
        t: &Tokens,
        reply: &Reply,
        asked: &str,
        (draft, ready, voice): (&mut DictionarySettings, &[&'static Pack], &Voice),
        out: &mut Commands,
    ) -> Option<Act> {
        if reply.entries.is_empty() {
            nothing(ui, t, asked, draft, ready, reply);
            return None;
        }
        let chosen = self.chosen.min(reply.entries.len() - 1);
        let reveal = std::mem::take(&mut self.reveal);
        let packs = !draft.pair && draft.langs.is_empty() && ready.len() > 1;
        let rest = ui.available_rect_before_wrap();
        let heard = Answers {
            voice,
            reply,
            packs,
        };
        if rest.width() < WIDE {
            // Compact: the word, then each sense as a card.
            let body = ScrollArea::vertical()
                .id_salt("dict-column")
                .auto_shrink(false)
                .show(ui, |ui| {
                    clear_of_bar(ui);
                    column(ui, t, &heard, asked, out);
                });
            widgets::scroll_fades(ui, &body, t.paper);
            return None;
        }
        let gap = 24.0;
        let list = Rect::from_min_size(rest.min, vec2(LIST_W, rest.height()));
        let detail = Rect::from_min_max(pos2(list.right() + gap, rest.top()), rest.max);
        let x = list.right() + gap / 2.0;
        let line = Stroke::new(1.0, t.line);
        ui.painter().vline(x, rest.y_range(), line);
        let mut act = None;
        ui.scope_builder(UiBuilder::new().max_rect(list), |ui| {
            act = results_list(ui, t, &reply.entries, asked, chosen, reveal);
        });
        ui.scope_builder(UiBuilder::new().max_rect(detail), |ui| {
            let more = full_entry(ui, t, &heard, chosen, asked, out);
            act = act.take().or(more);
        });
        ui.allocate_rect(rest, Sense::hover());
        act
    }
}

/// A reply with what drawing it needs: the voice, and whether to name packs.
struct Answers<'a> {
    voice: &'a Voice,
    reply: &'a Reply,
    /// Several packs were asked: each sense names its own.
    packs: bool,
}

impl Answers<'_> {
    /// The language an entry's answer is in.
    fn answer_in(&self, e: &Entry) -> String {
        entry::sides(e, self.reply.to.as_deref()).1
    }

    /// The language the word was asked in.
    fn asked_in(&self, e: Option<&Entry>) -> String {
        let from = self.reply.from.as_deref().map(mareader_speech::primary);
        let side = || e.map(|e| entry::sides(e, None).0);
        from.or_else(side).unwrap_or_else(|| HUB.to_owned())
    }
}

/// The word as its dictionary writes it; none for near matches only.
fn headword(reply: &Reply, asked: &str) -> Option<String> {
    let exact = reply
        .entries
        .iter()
        .find(|e| entry::near(e, asked).is_none());
    let bridged = reply.entries.iter().any(|e| e.via.is_some());
    match (exact, bridged) {
        (_, true) => Some(asked.to_owned()),
        (Some(e), false) => Some(e.asked().to_owned()),
        (None, false) => None,
    }
}

/// The word large, then its voice and how it sounds.
fn hero(ui: &mut Ui, word: &str, (voice, said_in): (&Voice, &str), size: f32, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let speaks = voice.can_say(said_in);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let button = size + 8.0;
        let room = ui.available_width() - if speaks { button + 10.0 } else { 0.0 };
        let lead = Layout::left_to_right(Align::Center);
        let row = vec2(room.max(40.0), button.min(34.0));
        ui.allocate_ui_with_layout(row, lead, |ui| {
            if shaped::needs_shaping(word) {
                shaped::label(ui, word, size, t.ink);
            } else {
                let head = RichText::new(word).font(theme::medium(size)).color(t.ink);
                ui.add(egui::Label::new(head).truncate());
            }
        });
        if speaks {
            look::say(ui, (word, said_in), voice, button.min(34.0), out);
        }
    });
    crate::sounds::show(ui, voice.lines(word), &t);
}

/// The narrow column: the word, its senses as cards, near matches after.
fn column(ui: &mut Ui, t: &Tokens, h: &Answers<'_>, asked: &str, out: &mut Commands) {
    ui.spacing_mut().item_spacing.y = 8.0;
    let list = &h.reply.entries;
    match headword(h.reply, asked) {
        Some(word) => {
            let first = list.iter().find(|e| entry::near(e, asked).is_none());
            hero(ui, &word, (h.voice, &h.asked_in(first)), 24.0, out);
            let answers = list.iter().map(|e| h.answer_in(e));
            offers(
                ui,
                h.voice,
                std::iter::once(h.asked_in(first)).chain(answers),
                out,
            );
        }
        None => {
            let line = format!("No entry for “{asked}”. The nearest:");
            ui.label(RichText::new(line).size(t.text_small).color(t.muted));
        }
    }
    let mut shown: Option<&str> = None;
    for (at, e) in list.iter().enumerate() {
        let near = entry::near(e, asked);
        if near.is_some()
            && near != shown
            && let Some(word) = near
        {
            ui.add_space(4.0);
            near_heading(ui, word, (h.voice, &entry::sides(e, None).0), out);
        }
        shown = near;
        let said = look::Said {
            voice: h.voice,
            said_in: &h.answer_in(e),
            way: h.packs,
            click: false,
            at,
        };
        look::sense(ui, e, &said, t, out);
    }
}

/// Downloads that would give these words a voice, two at most.
fn offers(ui: &mut Ui, voice: &Voice, languages: impl Iterator<Item = String>, out: &mut Commands) {
    let mut shown: Vec<mareader_engine::read_aloud::state::Offer> = Vec::new();
    for offer in languages.filter_map(|l| voice.offer(&l)) {
        if shown.len() < 2 && !shown.contains(&offer) {
            look::offer(ui, offer, out);
            shown.push(offer);
        }
    }
}

/// "≈ light": a nearest word heading its senses, with its voice.
fn near_heading(ui: &mut Ui, word: &str, (voice, said_in): (&Voice, &str), out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(RichText::new("≈").size(t.text).color(t.muted))
            .on_hover_text("The nearest entry");
        let head = RichText::new(word).font(theme::medium(16.0));
        shaped::line(ui, head, 16.0, t.ink);
        if voice.can_say(said_in) {
            look::say(ui, (word, said_in), voice, 24.0, out);
        }
    });
}

/// The mode as a sliding switch, `height` tall.
fn mode(ui: &mut Ui, s: &mut DictionarySettings, height: f32) {
    let face = |pair| match pair {
        false => (Some(Icon::Dictionary), "Dictionaries"),
        true => (Some(Icon::Translate), "Languages"),
    };
    widgets::segments_of(ui, height, &mut s.pair, &[false, true], 2, face);
}

/// How tall the choices' controls stand.
const CONTROL: f32 = 30.0;
/// The swap button's slot between the language pickers.
const SWAP_W: f32 = 28.0;

/// Mode, packs or languages, how the reply was reached, and settings.
fn choices(
    ui: &mut Ui,
    s: &mut DictionarySettings,
    ready: &[&'static Pack],
    reply: Option<&Reply>,
    t: &Tokens,
    out: &mut Commands,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.allocate_ui(vec2(260.0, CONTROL), |ui| mode(ui, s, CONTROL));
        ui.add_space(4.0);
        match s.pair {
            true => languages_row(ui, s, ready, 170.0, t),
            false => pack_box(ui, s, ready, 260.0, t),
        }
        if let Some((text, why)) = reply.and_then(|r| route_note(r, s)) {
            look::chip(ui, &text, t).on_hover_text(why);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| manage(ui, out));
    });
}

/// The window's choices: the mode and settings, then the picker across.
fn narrow_choices(
    ui: &mut Ui,
    s: &mut DictionarySettings,
    ready: &[&'static Pack],
    reply: Option<&Reply>,
    t: &Tokens,
    out: &mut Commands,
) {
    let gap = 6.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        let room = (ui.available_width() - GEAR_W - gap).max(120.0);
        ui.allocate_ui(vec2(room, CONTROL), |ui| mode(ui, s, CONTROL));
        manage(ui, out);
    });
    let width = ui.available_width();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        match s.pair {
            // Two equal pickers and the swap between fill the row exactly.
            true => {
                let each = ((width - SWAP_W - 2.0 * gap) / 2.0).floor().max(60.0);
                languages_row(ui, s, ready, each, t);
            }
            false => pack_box(ui, s, ready, width, t),
        }
    });
    if let Some((text, why)) = reply.and_then(|r| route_note(r, s)) {
        look::chip(ui, &text, t).on_hover_text(why);
    }
}

/// The settings button's slot, beside the mode.
const GEAR_W: f32 = 30.0;

fn manage(ui: &mut Ui, out: &mut Commands) {
    let size = vec2(GEAR_W, CONTROL);
    let manage = look::icon(ui, Icon::Settings, "Manage dictionaries…", size);
    if manage.clicked() {
        out.push(Sent::Engine(Command::OpenSettings(Tab::Dictionary)));
    }
}

fn pack_box(
    ui: &mut Ui,
    s: &mut DictionarySettings,
    ready: &[&'static Pack],
    width: f32,
    t: &Tokens,
) {
    let chosen = s
        .langs
        .first()
        .and_then(|id| ready.iter().find(|p| p.id == id));
    let shown = chosen.map_or("Every dictionary".to_owned(), |p| p.label());
    let id = (ui.make_persistent_id("dict-pack"), "Dictionary to search");
    look::picker(ui, id, vec2(width, CONTROL), &shown, t, |ui| {
        if look::option(ui, chosen.is_none(), "Every dictionary", t) {
            s.langs.clear();
        }
        for p in ready {
            let on = chosen.is_some_and(|c| c.id == p.id);
            if look::option(ui, on, &p.label(), t) {
                s.langs = vec![p.id.to_owned()];
            }
        }
    });
}

fn languages_row(
    ui: &mut Ui,
    s: &mut DictionarySettings,
    ready: &[&'static Pack],
    width: f32,
    t: &Tokens,
) {
    let langs = languages(ready);
    let names = ("dict-from", "Translate from");
    language_box(ui, names, width, (&mut s.from, &langs), t);
    let swap = look::icon(ui, Icon::Swap, "Swap languages", vec2(SWAP_W, CONTROL));
    if swap.clicked() {
        std::mem::swap(&mut s.from, &mut s.to);
    }
    let names = ("dict-to", "Translate to");
    language_box(ui, names, width, (&mut s.to, &langs), t);
}

/// A language picker; `id` is its salt and its spoken name.
fn language_box(
    ui: &mut Ui,
    (salt, name): (&str, &str),
    width: f32,
    (value, langs): (&mut Option<String>, &[&str]),
    t: &Tokens,
) {
    let shown = value.as_deref().map_or("Automatic".to_owned(), language);
    let id = (ui.make_persistent_id(salt), name);
    look::picker(ui, id, vec2(width, CONTROL), &shown, t, |ui| {
        if look::option(ui, value.is_none(), "Automatic", t) {
            *value = None;
        }
        for l in langs {
            let on = value.as_deref() == Some(*l);
            if look::option(ui, on, &language(l), t) {
                *value = Some((*l).to_owned());
            }
        }
    });
}

/// "via English" when a bridge answered, or the language detected.
fn route_note(reply: &Reply, s: &DictionarySettings) -> Option<(String, String)> {
    // The rows name each English word between; this names the language.
    if reply.entries.iter().any(|e| e.via.is_some()) {
        let via = language(HUB);
        let why = format!("No dictionary joins these two; answers pass through {via}.");
        return Some((format!("via {via}"), why));
    }
    let from = reply
        .from
        .as_deref()
        .filter(|_| s.pair && s.from.is_none())?;
    let why = "Automatic chose this language for the word.";
    Some((format!("Detected {}", language(from)), why.to_owned()))
}

/// The rows down the left: headword, its tags, the answer cut short.
fn results_list(
    ui: &mut Ui,
    t: &Tokens,
    list: &[Entry],
    asked: &str,
    chosen: usize,
    reveal: bool,
) -> Option<Act> {
    let mut act = None;
    let body = ScrollArea::vertical()
        .id_salt("dict-results")
        .auto_shrink(false)
        .show(ui, |ui| {
            clear_of_bar(ui);
            ui.spacing_mut().item_spacing.y = 2.0;
            let mut shown: Option<&str> = None;
            for (i, e) in list.iter().enumerate() {
                let near = entry::near(e, asked);
                if near.is_some()
                    && near != shown
                    && let Some(word) = near
                {
                    ui.add_space(8.0);
                    entry::heading(ui, word, t);
                }
                shown = near;
                let r = row(ui, e, i, i == chosen, t);
                if r.clicked() {
                    act = Some(Act::Choose(i));
                }
                if reveal && i == chosen {
                    r.scroll_to_me(None);
                }
            }
        });
    widgets::scroll_fades(ui, &body, Tokens::get(ui.ctx()).paper);
    act
}

/// One result: lit with an accent edge when chosen, the whole row a button.
fn row(ui: &mut Ui, e: &Entry, i: usize, on: bool, t: &Tokens) -> Response {
    let bg = ui.painter().add(Shape::Noop);
    let inner = Frame::new()
        .inner_margin(Margin {
            left: 14,
            right: 10,
            top: 7,
            bottom: 7,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 2.0;
            // The headword leads; its part of speech sits at the end.
            ui.horizontal(|ui| {
                let tags = e.tags();
                let first = tags.split(" · ").next().filter(|p| !p.is_empty());
                let tag = first.map(|p| RichText::new(p).italics().size(t.text_small));
                let room = ui.available_width() - if tag.is_some() { 72.0 } else { 0.0 };
                let lead = Layout::left_to_right(Align::Center);
                ui.allocate_ui_with_layout(vec2(room.max(0.0), 0.0), lead, |ui| {
                    let head = RichText::new(e.asked()).font(theme::medium(t.text));
                    shaped::line(ui, head, t.text, t.ink);
                });
                if let Some(tag) = tag {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(tag.color(t.muted));
                    });
                }
            });
            let size = entry::answer_size(e.answer(), t) * 0.9;
            shaped::line(ui, e.answer(), size, t.muted);
        });
    let rect = inner.response.rect;
    let resp = ui.interact(rect, ui.id().with(("dict-row", i)), Sense::click());
    let lift = ui
        .ctx()
        .animate_bool_with_time(resp.id, resp.hovered(), 0.1);
    let fill = match on {
        true => t.selected,
        false => t.hover.gamma_multiply(lift),
    };
    let mut shapes = vec![Shape::rect_filled(rect, t.radius, fill)];
    if on {
        // An accent edge marks the pick where its fill barely shows.
        let edge = Rect::from_min_size(rect.min + vec2(4.0, 8.0), vec2(3.0, rect.height() - 16.0));
        shapes.push(Shape::rect_filled(edge, 2.0, t.accent));
    }
    ui.painter().set(bg, Shape::Vec(shapes));
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let label = format!("{} — {}", e.asked(), e.answer());
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, on, &label));
    resp
}

/// The chosen entry large, its actions, then the word's other senses.
fn full_entry(
    ui: &mut Ui,
    t: &Tokens,
    h: &Answers<'_>,
    chosen: usize,
    asked: &str,
    out: &mut Commands,
) -> Option<Act> {
    let list = &h.reply.entries;
    let e = list.get(chosen)?;
    let mut act = None;
    let body = ScrollArea::vertical()
        .id_salt("dict-entry")
        .auto_shrink(false)
        .show(ui, |ui| {
            clear_of_bar(ui);
            ui.spacing_mut().item_spacing.y = 8.0;
            let word = if e.via.is_some() { asked } else { e.asked() };
            hero(ui, word, (h.voice, &h.asked_in(Some(e))), 30.0, out);
            if e.fit != WordMatch::Exact {
                let note = format!("The nearest entry to “{asked}”");
                ui.label(RichText::new(note).size(t.text_small).color(t.muted));
            }
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for tag in e.tags().split(" · ").filter(|s| !s.is_empty()) {
                    look::chip(ui, tag, t);
                }
                if let Some(way) = look::way(e) {
                    look::tag(ui, &way, t);
                }
                if let Some(via) = &e.via {
                    look::tag(ui, &format!("via {via}"), t);
                }
            });
            ui.add_space(2.0);
            answer_panel(ui, t, e, h, out);
            let languages = [h.asked_in(Some(e)), h.answer_in(e)];
            offers(ui, h.voice, languages.into_iter(), out);
            ui.add_space(2.0);
            ui.horizontal_wrapped(|ui| {
                let copy = Button::image_and_text(Icon::Copy.image(ui, 14.0), "Copy")
                    .image_tint_follows_text_color(true)
                    .corner_radius(t.radius);
                if ui.add(copy).clicked() {
                    ui.ctx().copy_text(e.answer().to_owned());
                    out.push(Sent::Ui(UiCommand::Notify(Toast::info("Copied"))));
                }
                act = look_again(ui, e, t);
            });
            let word = fold(e.asked());
            let others: Vec<(usize, &Entry)> = list
                .iter()
                .enumerate()
                .filter(|(i, o)| *i != chosen && fold(o.asked()) == word)
                .collect();
            if others.is_empty() {
                return;
            }
            ui.add_space(12.0);
            let title = format!("More for “{}”", e.asked());
            let title = RichText::new(title).font(theme::medium(t.text_small));
            ui.label(title.color(t.muted));
            for (i, o) in others {
                let said = look::Said {
                    voice: h.voice,
                    said_in: "",
                    way: h.packs,
                    click: true,
                    at: i,
                };
                if look::sense(ui, o, &said, t, out).clicked() {
                    act = Some(Act::Choose(i));
                }
            }
        });
    widgets::scroll_fades(ui, &body, Tokens::get(ui.ctx()).paper);
    act
}

/// The answer on a soft panel, with its voice, romanization and gloss.
fn answer_panel(ui: &mut Ui, t: &Tokens, e: &Entry, h: &Answers<'_>, out: &mut Commands) {
    Frame::new()
        .fill(t.ink.gamma_multiply(0.035))
        .corner_radius(t.radius_lg)
        .inner_margin(Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.horizontal(|ui| {
                let answer_in = h.answer_in(e);
                let speaks = h.voice.can_say(&answer_in) && !e.answer().trim().is_empty();
                let room = ui.available_width() - if speaks { 44.0 } else { 0.0 };
                let lead = Layout::left_to_right(Align::Center).with_main_wrap(true);
                ui.allocate_ui_with_layout(vec2(room.max(40.0), 0.0), lead, |ui| {
                    let size = entry::answer_size(e.answer(), t) * 1.5;
                    shaped::label(ui, e.answer(), size, t.ink);
                });
                if speaks {
                    let right = Layout::right_to_left(Align::Center);
                    let said = (e.answer(), answer_in.as_str());
                    ui.with_layout(right, |ui| look::say(ui, said, h.voice, 32.0, out));
                }
            });
            if let Some(r) = e.romanization.as_deref().filter(|r| !r.is_empty()) {
                shaped::label(ui, r, t.text, t.muted);
            }
            if let Some(s) = e.sense.as_deref().filter(|s| !s.is_empty()) {
                if shaped::needs_shaping(s) {
                    shaped::label(ui, s, t.text, t.muted);
                } else {
                    ui.label(RichText::new(s).italics().color(t.muted));
                }
            }
        });
}

/// "Look up ⟨answer⟩", offered when the answer is a word or two.
fn look_again(ui: &mut Ui, e: &Entry, t: &Tokens) -> Option<Act> {
    let answer = e.answer().trim();
    let short = answer.chars().count() <= SHORT && answer.split_whitespace().count() <= 3;
    if !short || answer.is_empty() {
        return None;
    }
    let text = RichText::new(format!("Look up “{answer}”"));
    let button = Button::image_and_text(Icon::Search.image(ui, 14.0), text)
        .image_tint_follows_text_color(true)
        .corner_radius(t.radius)
        .frame_when_inactive(false);
    ui.add(button)
        .clicked()
        .then(|| Act::Search(answer.to_owned()))
}

/// Keeps scrolled content off the floating bar, which paints over it.
pub fn clear_of_bar(ui: &mut Ui) {
    let bar = ui.spacing().scroll;
    let room = bar.bar_width + bar.bar_outer_margin + 2.0;
    ui.set_max_width((ui.available_width() - room).max(0.0));
}

/// Nothing typed: what is searched, how, and the recent searches.
fn home(
    ui: &mut Ui,
    t: &Tokens,
    ready: &[&'static Pack],
    s: &mut DictionarySettings,
) -> Option<Act> {
    let mut act = None;
    ui.vertical_centered(|ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.add_space((ui.available_height() * 0.08).max(8.0));
        look::disc(ui, Icon::Dictionary, 56.0, t);
        ui.add_space(4.0);
        let title = RichText::new("Look up any word").font(theme::medium(19.0));
        ui.label(title.color(t.ink));
        ui.label(RichText::new(scope(ready, s)).color(t.muted));
        ui.add_space(4.0);
        let keys: [(&[&str], &str); 3] = [
            (&["↑", "↓"], "choose"),
            (&["Enter"], "keep"),
            (&["Esc"], "clear"),
        ];
        look::keys(ui, &keys, t);
        if s.recent.is_empty() {
            return;
        }
        ui.add_space(20.0);
        let width = ui.available_width().min(440.0);
        ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
            act = recent(ui, t, s);
        });
    });
    act
}

/// The recent searches as chips, under a heading that clears them.
fn recent(ui: &mut Ui, t: &Tokens, s: &mut DictionarySettings) -> Option<Act> {
    let mut act = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.add(Icon::Clock.image(ui, 14.0).tint(t.muted));
        let head = RichText::new("Recent").font(theme::medium(t.text_small));
        ui.label(head.color(t.muted));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let clear = RichText::new("Clear").size(t.text_small).color(t.muted);
            let button = Button::new(clear).frame_when_inactive(false);
            if ui.add(button).clicked() {
                s.recent.clear();
            }
        });
    });
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        for word in &s.recent {
            let chip = Button::new(RichText::new(word).color(t.ink))
                .corner_radius(CornerRadius::same(255))
                .stroke(Stroke::new(1.0, t.line))
                .fill(t.surface)
                .min_size(vec2(0.0, 28.0));
            let r = ui.add(chip);
            if r.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if r.clicked() {
                act = Some(Act::Search(word.clone()));
            }
        }
    });
    act
}

/// "Searching English → Burmese and English → French", or the pair.
fn scope(ready: &[&'static Pack], s: &DictionarySettings) -> String {
    if s.pair {
        let from = s
            .from
            .as_deref()
            .map_or("any language".to_owned(), language);
        let to = s.to.as_deref().map_or("your language".to_owned(), language);
        return format!("Translating from {from} to {to}");
    }
    let chosen = s
        .langs
        .first()
        .and_then(|id| ready.iter().find(|p| p.id == id));
    let labels: Vec<String> = match chosen {
        Some(p) => vec![p.label()],
        None => ready.iter().map(|p| p.label()).collect(),
    };
    format!("Searching {}", labels.join(", "))
}

/// Nothing found: other source languages, or every dictionary.
fn nothing(
    ui: &mut Ui,
    t: &Tokens,
    asked: &str,
    draft: &mut DictionarySettings,
    ready: &[&'static Pack],
    reply: &Reply,
) {
    ui.vertical_centered(|ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.add_space((ui.available_height() * 0.08).max(8.0));
        look::disc(ui, Icon::Search, 48.0, t);
        let line = format!("Nothing found for “{asked}”.");
        shaped::label(ui, &line, t.text, t.ink);
        let hint = "Check the spelling, or search somewhere else:";
        let others = draft.pair || !draft.langs.is_empty();
        if others {
            ui.label(RichText::new(hint).size(t.text_small).color(t.muted));
        }
        if !draft.pair && !draft.langs.is_empty() && ui.link("Search every dictionary").clicked() {
            draft.langs.clear();
        }
        if !draft.pair {
            return;
        }
        let from = reply.from.as_deref().unwrap_or(HUB);
        let others: Vec<&str> = languages(ready)
            .into_iter()
            .filter(|l| *l != from)
            .collect();
        for l in others {
            if ui.link(format!("Search as {}", language(l))).clicked() {
                draft.from = Some(l.to_owned());
                if draft.to.as_deref() == Some(l) {
                    draft.to = None;
                }
            }
        }
    });
}

/// An empty page: icon, title, a line of why, maybe an action.
fn empty(ui: &mut Ui, t: &Tokens, title: &str, line: &str, action: Option<&str>) -> bool {
    let mut clicked = false;
    ui.vertical_centered(|ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.add_space((ui.available_height() * 0.25).max(24.0));
        look::disc(ui, Icon::Dictionary, 56.0, t);
        ui.add_space(4.0);
        let title = RichText::new(title).font(theme::medium(17.0));
        ui.label(title.color(t.ink));
        ui.label(RichText::new(line).color(t.muted));
        if let Some(action) = action {
            ui.add_space(6.0);
            let button = widgets::primary_button(ui, Some(Icon::Download), action, t.accent, t);
            clicked = ui.add(button.min_size(vec2(0.0, 32.0))).clicked();
        }
    });
    clicked
}
