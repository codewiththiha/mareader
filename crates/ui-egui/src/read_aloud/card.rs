//! The player as a card: cover, place, and words streaming like lyrics.

use std::ops::Range;
use std::sync::Arc;

use egui::epaint::TextShape;
use egui::text::{LayoutJob, TextFormat};
use egui::{
    Color32, CornerRadius, CursorIcon, FontId, Galley, Mesh, Pos2, Rect, Sense, Shadow, Stroke, Ui,
    pos2, vec2,
};
use mareader_engine::read_aloud::player::Lyrics;

use crate::icons::Icon;
use crate::theme::{self, Tokens};
use crate::toast::IslandNote;
use crate::widgets;

pub(super) const HEIGHT: f32 = 224.0;
/// The full card's least width; it keeps the bar's if wider.
pub(super) const FULL_LEAST: f32 = 380.0;
/// The compact card: the bar, with one streaming line above it.
pub(super) const COMPACT_HEIGHT: f32 = 94.0;
const PAD: f32 = 14.0;
const COVER: egui::Vec2 = vec2(70.0, 105.0);
/// Narrower than this, the cover gives the words its room.
const COVERLESS: f32 = 340.0;
/// The lit words sit this far down the lines shown.
const EYE: f32 = 0.4;
/// The words' scroll closes this share of its gap each second, roughly.
const GLIDE: f32 = 10.0;
pub(super) const NOW_SIZE: f32 = 16.0;
const NEAR_SIZE: f32 = 13.0;
pub(super) const LABEL: &str = "Show where it's reading";
/// The compact line's type size, and seconds a new sentence takes to rise.
const COMPACT_SIZE: f32 = 15.0;
const ENTER: f64 = 0.3;

/// What the card says about the book.
pub(super) use mareader_engine::read_aloud::state::About;

/// The words' scroll, kept between frames.
#[derive(Default)]
pub(super) struct Scroll {
    at: f32,
    key: Option<u64>,
    /// Where the sentence said starts in the lines, last frame.
    now_top: f32,
    time: Option<f64>,
}

/// The card's fill, edge and shadow; `k` is how far it has opened.
///
/// A `note` grows as a tab out of the pill's top, one body with it.
pub(super) fn frame(ui: &Ui, rect: Rect, k: f32, note: Option<&IslandNote>) {
    let t = Tokens::get(ui.ctx());
    let radius = lerp(super::control::HEIGHT / 2.0, 18.0, k);
    let round = CornerRadius::same(radius as u8);
    let base = ui.ctx().global_style().visuals.window_shadow;
    let shadow = Shadow {
        offset: [0, (base.offset[1] as f32 + 6.0 * k) as i8],
        blur: (f32::from(base.blur) + 18.0 * k) as u8,
        ..base
    };
    let tab = note.and_then(|n| Some((super::tab::rect(rect, n)?, n)));
    ui.painter().add(shadow.as_shape(rect, round));
    if let Some((at, _)) = tab {
        super::tab::shadow(ui.painter(), at, shadow);
    }
    ui.painter().rect_filled(rect, round, t.surface);
    let edge = Stroke::new(1.0, t.line);
    (ui.painter()).rect_stroke(rect, round, edge, egui::StrokeKind::Inside);
    if let Some((at, note)) = tab {
        super::tab::body(ui.painter(), at, note, &t);
    }
}

/// The full card, faded in by `show`; true when a `live` one is clicked.
pub(super) fn show(
    ui: &mut Ui,
    (rect, show, live): (Rect, f32, bool),
    (about, lyrics): (&About<'_>, Option<&Lyrics>),
    scroll: &mut Scroll,
    cover: &mut dyn FnMut(&Ui, Rect) -> bool,
) -> bool {
    let t = Tokens::get(ui.ctx());
    let inner = rect.shrink(PAD);
    if inner.width() < 40.0 || inner.height() < 40.0 {
        return false;
    }
    let faded = |c: Color32| c.gamma_multiply(show);
    // The cover rises a little as the card opens.
    let lift = (1.0 - show) * 8.0;
    let with_cover = rect.width() >= COVERLESS;
    let art = Rect::from_min_size(inner.min + vec2(0.0, lift), COVER);
    let text_left = if with_cover {
        art.right() + PAD
    } else {
        inner.left()
    };
    let column = Rect::from_min_max(pos2(text_left, inner.top() + lift), inner.max);
    let mut clicked = false;
    if with_cover {
        let r = live.then(|| reveal(ui, art, "read-aloud-cover"));
        let hot = (r.as_ref()).map_or(0.0, |r| {
            ui.ctx().animate_bool_with_time(r.id, r.hovered(), 0.12)
        });
        let art = art.expand(2.0 * hot);
        ui.scope(|ui| {
            ui.set_opacity(show);
            if !cover(ui, art) {
                drawn_cover(ui, art, about.title);
            }
        });
        clicked |= r.is_some_and(|r| r.clicked());
    }
    // The title, with bars that dance while it plays.
    let bars_w = 18.0;
    let font = theme::medium(15.0);
    let title_w = (column.width() - bars_w - 8.0).max(10.0);
    let text = truncated(ui, about.title, font.clone(), title_w);
    let galley = ui.painter().layout_no_wrap(text, font, faded(t.ink));
    let title_h = galley.size().y.max(18.0);
    let title_rect = Rect::from_min_size(column.min, vec2(galley.size().x, title_h));
    ui.painter().galley(title_rect.min, galley, faded(t.ink));
    if live {
        let r = ui.interact(title_rect, ui.id().with("read-aloud-title"), Sense::click());
        if r.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        clicked |= r.clicked();
    }
    let bars = Rect::from_min_size(
        pos2(column.right() - bars_w, column.top() + 2.0),
        vec2(bars_w, title_h - 4.0),
    );
    dance(ui, bars, about.playing, faded(t.accent));
    let mut y = title_rect.bottom() + 2.0;
    let line = match (&about.place, about.narrator) {
        (Some(place), Some(who)) => format!("{place} · read by {who}"),
        (Some(place), None) => place.clone(),
        (None, Some(who)) => format!("Read by {who}"),
        (None, None) => String::new(),
    };
    if !line.is_empty() {
        let small = FontId::proportional(t.text_small);
        let text = truncated(ui, &line, small.clone(), column.width());
        let g = ui.painter().layout_no_wrap(text, small, faded(t.muted));
        let h = g.size().y;
        ui.painter()
            .galley(pos2(column.left(), y), g, faded(t.muted));
        y += h;
    }
    let words = Rect::from_min_max(pos2(column.left(), y + 8.0), column.max);
    // Mid-morph, the words wait until a line fits.
    if words.height() < NOW_SIZE {
        return clicked;
    }
    match lyrics {
        Some(lyrics) => stream(ui, words, show, lyrics, scroll),
        None => quiet(ui, words, show, about),
    }
    clicked
}

/// Where a click goes back to the words, named for screen readers.
fn reveal(ui: &Ui, rect: Rect, id: &str) -> egui::Response {
    let r = ui.interact(rect, ui.id().with(id), Sense::click());
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, LABEL));
    if r.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    widgets::tip(&r, LABEL, None);
    r
}

/// The compact card: where it is, then one gliding line; true when clicked.
pub(super) fn compact(
    ui: &mut Ui,
    (rect, show, live): (Rect, f32, bool),
    (about, lyrics): (&About<'_>, Option<&Lyrics>),
    ticker: &mut Ticker,
) -> bool {
    let t = Tokens::get(ui.ctx());
    let inner = rect.shrink2(vec2(16.0, 0.0)).with_min_y(rect.top() + 9.0);
    if inner.width() < 40.0 || inner.height() < 30.0 {
        return false;
    }
    let lift = (1.0 - show) * 6.0;
    let faded = |c: Color32| c.gamma_multiply(show);
    // "Georgia · Page 22 · History", with bars that dance while it plays.
    let bars_w = 14.0;
    let small = FontId::proportional(t.text_small);
    let line = match &about.place {
        Some(place) => format!("{} · {place}", about.title),
        None => about.title.to_owned(),
    };
    let text = truncated(ui, &line, small.clone(), inner.width() - bars_w - 10.0);
    let g = ui.painter().layout_no_wrap(text, small, faded(t.muted));
    let top = inner.top() + lift;
    let head_h = g.size().y;
    ui.painter()
        .galley(pos2(inner.left(), top), g, faded(t.muted));
    let bars = Rect::from_min_size(
        pos2(inner.right() - bars_w, top + 2.0),
        vec2(bars_w, head_h - 4.0),
    );
    dance(ui, bars, about.playing, faded(t.accent));
    let words = Rect::from_min_max(
        pos2(inner.left(), top + head_h + 2.0),
        pos2(inner.right(), rect.bottom() - 2.0),
    );
    match lyrics {
        Some(lyrics) => ticker.show(ui, words, show, lyrics),
        None => {
            let said = match (about.playing, about.narrator) {
                (true, Some(who)) => format!("Read by {who}"),
                (true, None) => "Listening to the narrator".to_owned(),
                (false, _) => "Paused".to_owned(),
            };
            let font = theme::medium(COMPACT_SIZE);
            let g = ui.painter().layout_no_wrap(said, font, faded(t.ink));
            let at = pos2(words.left(), words.center().y - g.size().y / 2.0);
            ui.painter().galley(at, g, faded(t.ink));
        }
    }
    live && reveal(ui, rect, "read-aloud-line").clicked()
}

/// The sentence said on one line, kept moving with the words lit.
#[derive(Default)]
pub(super) struct Ticker {
    /// How far along the line is scrolled, gliding to its goal.
    x: f32,
    key: Option<u64>,
    time: Option<f64>,
    /// When the sentence said arrived, rising into place.
    since: f64,
    /// The last sentence, lifting away, and its scroll.
    gone: Option<(Arc<Galley>, f32, f64)>,
    last: Option<Arc<Galley>>,
}

impl Ticker {
    fn show(&mut self, ui: &Ui, area: Rect, show: f32, lyrics: &Lyrics) {
        let t = Tokens::get(ui.ctx());
        let (job, lit) = line(lyrics, &t);
        let galley = ui.painter().layout_job(job);
        let x_of = |c: usize| galley.pos_from_cursor(egui::text::CCursor::new(c)).left();
        let (a, b) = (x_of(lit.start), x_of(lit.end));
        let room = area.width();
        let most = (galley.size().x - room).max(0.0);
        // The lit words sit a little left of the middle, as eyes lead.
        let goal = ((a + b) / 2.0 - room * 0.42).clamp(0.0, most);
        let now = ui.input(|i| i.time);
        let dt = self.time.map_or(0.0, |was| (now - was).clamp(0.0, 0.1)) as f32;
        self.time = Some(now);
        if self.key != Some(lyrics.key) {
            if let (Some(_), Some(old)) = (self.key, self.last.take()) {
                self.gone = Some((old, self.x, now));
            }
            (self.key, self.x, self.since) = (Some(lyrics.key), goal, now);
        }
        let calm = !crate::motion::enabled(ui.ctx());
        self.x += (goal - self.x) * (1.0 - (-dt * 8.0).exp());
        if calm {
            self.x = goal;
        }
        let rise = |since: f64| match calm {
            true => 1.0,
            false => ((now - since) / ENTER).clamp(0.0, 1.0) as f32,
        };
        let painter = ui.painter().with_clip_rect(area.expand2(vec2(0.0, 2.0)));
        let y = area.center().y - galley.size().y / 2.0;
        if let Some((old, x, since)) = &self.gone {
            let k = ease(rise(*since));
            let at = pos2(area.left() - x, y - 10.0 * k);
            let fade = show * (1.0 - k);
            painter.add(TextShape::new(at, old.clone(), t.ink).with_opacity_factor(fade));
            if k >= 1.0 {
                self.gone = None;
            }
        }
        let k = ease(rise(self.since));
        let at = pos2(area.left() - self.x, y + 10.0 * (1.0 - k));
        painter.add(TextShape::new(at, galley.clone(), t.ink).with_opacity_factor(show * k));
        // Words beyond the edges melt into the card.
        let edge = 22.0;
        if self.x > 0.5 {
            side_fade(ui, area.with_max_x(area.left() + edge), t.surface, true);
        }
        if galley.size().x - self.x > room + 0.5 {
            side_fade(ui, area.with_min_x(area.right() - edge), t.surface, false);
        }
        if (goal - self.x).abs() > 0.5 || k < 1.0 || self.gone.is_some() {
            ui.ctx().request_repaint();
        }
        self.last = Some(galley);
    }
}

/// The sentence said as one coloured line, and its lit words' characters.
fn line(lyrics: &Lyrics, t: &Tokens) -> (LayoutJob, Range<usize>) {
    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    let format = |c: Color32| TextFormat {
        font_id: theme::medium(COMPACT_SIZE),
        color: c,
        ..Default::default()
    };
    let chars = |byte: usize| lyrics.now.get(..byte).map_or(0, |s| s.chars().count());
    let mut lit = 0..0;
    let mut from = 0;
    for (i, word) in lyrics.words.iter().enumerate() {
        let colour = match i {
            _ if i < lyrics.lit.start => t.ink,
            _ if lyrics.lit.contains(&i) => t.accent,
            _ => t.muted,
        };
        if i == lyrics.lit.start {
            lit.start = chars(word.start);
        }
        if i + 1 == lyrics.lit.end {
            lit.end = chars(word.end);
        }
        let end = (lyrics.words.get(i + 1)).map_or(lyrics.now.len(), |next| next.start);
        if let Some(text) = lyrics.now.get(from..end) {
            job.append(text, 0.0, format(colour));
        }
        from = end;
    }
    if lyrics.words.is_empty() {
        job.append(&lyrics.now, 0.0, format(t.ink));
    }
    lit.end = lit.end.max(lit.start);
    (job, lit)
}

/// From the card's fill to clear across `rect`; `left` fades rightward.
fn side_fade(ui: &Ui, rect: Rect, fill: Color32, left: bool) {
    let clear = Color32::TRANSPARENT;
    let (near, far) = if left { (fill, clear) } else { (clear, fill) };
    let mut mesh = Mesh::default();
    let corners: [(Pos2, Color32); 4] = [
        (rect.left_top(), near),
        (rect.right_top(), far),
        (rect.right_bottom(), far),
        (rect.left_bottom(), near),
    ];
    for (pos, colour) in corners {
        mesh.colored_vertex(pos, colour);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    ui.painter().add(mesh);
}

/// Slow, quick, then slow.
fn ease(k: f32) -> f32 {
    let k = k.clamp(0.0, 1.0);
    k * k * (3.0 - 2.0 * k)
}

/// A cover drawn for a book with none: the look's wash and headphones.
fn drawn_cover(ui: &Ui, rect: Rect, title: &str) {
    let t = Tokens::get(ui.ctx());
    let round = CornerRadius::same(6);
    ui.painter().rect_filled(rect, round, t.accent_soft);
    let edge = Stroke::new(1.0, t.line);
    (ui.painter()).rect_stroke(rect, round, edge, egui::StrokeKind::Inside);
    let icon = Rect::from_center_size(rect.center() - vec2(0.0, 10.0), egui::Vec2::splat(28.0));
    Icon::ReadAloud.paint(ui, icon, t.accent);
    let initial: String = title.chars().take(1).collect();
    let g = (ui.painter()).layout_no_wrap(initial, theme::medium(14.0), t.accent);
    let at = pos2(rect.center().x - g.size().x / 2.0, icon.bottom() + 8.0);
    ui.painter().galley(at, g, t.accent);
}

/// Three bars rising and falling while it plays, resting low when not.
fn dance(ui: &Ui, rect: Rect, playing: bool, colour: Color32) {
    let time = ui.input(|i| i.time) as f32;
    let calm = ui
        .ctx()
        .animate_bool_with_time(ui.id().with("bars"), playing, 0.25);
    for (i, speed) in [7.3f32, 9.1, 5.7].into_iter().enumerate() {
        let wave = 0.5 + 0.5 * (time * speed + i as f32 * 1.7).sin();
        let h = rect.height() * (0.25 + 0.75 * wave * calm);
        let x = rect.left() + i as f32 * 6.0;
        let bar = Rect::from_min_max(pos2(x, rect.bottom() - h), pos2(x + 3.5, rect.bottom()));
        ui.painter().rect_filled(bar, CornerRadius::same(2), colour);
    }
    if playing {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(33));
    }
}

/// The sentence said amid its neighbours, gliding to keep lit words seen.
fn stream(ui: &mut Ui, area: Rect, show: f32, lyrics: &Lyrics, scroll: &mut Scroll) {
    let t = Tokens::get(ui.ctx());
    let (job, now_at, lit_at) = layout(lyrics, area.width(), &t, show);
    let galley = ui.painter().layout_job(job);
    let y_of = |byte: usize| {
        let cursor = egui::text::CCursor::new(byte);
        galley.pos_from_cursor(cursor).center().y
    };
    let now_top = galley
        .pos_from_cursor(egui::text::CCursor::new(now_at))
        .top();
    let lit_y = y_of(lit_at);
    // A new sentence: the lines move up by what went above, so it glides.
    if scroll.key != Some(lyrics.key) {
        if scroll.key.is_some() {
            scroll.at -= scroll.now_top;
        } else {
            scroll.at = lit_y - area.height() * EYE;
        }
        scroll.key = Some(lyrics.key);
    }
    scroll.now_top = now_top;
    let now = ui.input(|i| i.time);
    let dt = scroll.time.map_or(0.0, |was| (now - was).clamp(0.0, 0.1)) as f32;
    scroll.time = Some(now);
    let target = lit_y - area.height() * EYE;
    // Racing or sought sentences never leave the words out of sight.
    let reach = (area.height() * 0.25).max(0.0);
    scroll.at = scroll.at.clamp(target - reach, target + reach);
    scroll.at += (target - scroll.at) * (1.0 - (-dt * GLIDE).exp());
    if (target - scroll.at).abs() > 0.5 {
        ui.ctx().request_repaint();
    }
    let painter = ui.painter().with_clip_rect(area);
    painter.galley(area.min - vec2(0.0, scroll.at), galley, t.ink);
    let fill = t.surface;
    fade_edge(ui, area.with_max_y(area.top() + 14.0), fill, true);
    fade_edge(ui, area.with_min_y(area.bottom() - 22.0), fill, false);
    // Read out by screen readers as the sentence said.
    let r = ui.interact(area, ui.id().with("read-aloud-words"), Sense::hover());
    let said = lyrics.now.clone();
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &said));
}

/// The lines as one job; where the sentence and its lit words start.
fn layout(lyrics: &Lyrics, width: f32, t: &Tokens, show: f32) -> (LayoutJob, usize, usize) {
    let mut job = LayoutJob::default();
    job.wrap.max_width = width;
    let near = |c: Color32| TextFormat {
        font_id: FontId::proportional(NEAR_SIZE),
        color: c.gamma_multiply(show),
        line_height: Some(NEAR_SIZE * 1.35),
        ..Default::default()
    };
    let main = |c: Color32| TextFormat {
        font_id: theme::medium(NOW_SIZE),
        color: c.gamma_multiply(show),
        line_height: Some(NOW_SIZE * 1.3),
        ..Default::default()
    };
    if let Some(before) = &lyrics.before {
        job.append(before, 0.0, near(t.muted.gamma_multiply(0.7)));
        job.append("\n\n", 0.0, near(t.muted));
    }
    let now_at = job.text.chars().count();
    let mut lit_at = now_at;
    let mut from = 0;
    for (i, word) in lyrics.words.iter().enumerate() {
        let colour = match i {
            _ if i < lyrics.lit.start => t.ink,
            _ if lyrics.lit.contains(&i) => t.accent,
            _ => t.muted,
        };
        if i == lyrics.lit.start {
            let before: usize = (lyrics.now.get(..word.start)).map_or(0, |s| s.chars().count());
            lit_at = now_at + before;
        }
        // Spaces go with the word before them.
        let end = (lyrics.words.get(i + 1)).map_or(lyrics.now.len(), |next| next.start);
        if let Some(text) = lyrics.now.get(from..end) {
            job.append(text, 0.0, main(colour));
        }
        from = end;
    }
    if lyrics.words.is_empty() {
        job.append(&lyrics.now, 0.0, main(t.ink));
    }
    if let Some(after) = &lyrics.after {
        job.append("\n\n", 0.0, near(t.muted));
        job.append(after, 0.0, near(t.muted.gamma_multiply(0.7)));
    }
    (job, now_at, lit_at)
}

/// A narrated book's card: its chapter and narrator, the words its own.
fn quiet(ui: &Ui, area: Rect, show: f32, about: &About<'_>) {
    let t = Tokens::get(ui.ctx());
    let text = match about.playing {
        true => "Listening to the narrator",
        false => "Paused",
    };
    let g = ui.painter().layout(
        text.to_owned(),
        theme::medium(NOW_SIZE),
        t.ink.gamma_multiply(show),
        area.width(),
    );
    let at = pos2(area.left(), area.center().y - g.size().y / 2.0);
    ui.painter().galley(at, g, t.ink);
}

/// From the card's fill to clear, over `rect`; `top` fades downward.
fn fade_edge(ui: &Ui, rect: Rect, fill: Color32, top: bool) {
    let clear = Color32::from_rgba_unmultiplied(fill.r(), fill.g(), fill.b(), 0);
    let (upper, lower) = if top { (fill, clear) } else { (clear, fill) };
    let mut mesh = Mesh::default();
    let corners: [(Pos2, Color32); 4] = [
        (rect.left_top(), upper),
        (rect.right_top(), upper),
        (rect.right_bottom(), lower),
        (rect.left_bottom(), lower),
    ];
    for (pos, colour) in corners {
        mesh.colored_vertex(pos, colour);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    ui.painter().add(mesh);
}

/// `text` cut with an ellipsis to fit `width`.
fn truncated(ui: &Ui, text: &str, font: FontId, width: f32) -> String {
    let fits = |s: &str| {
        ui.painter()
            .layout_no_wrap(s.to_owned(), font.clone(), Color32::WHITE)
            .size()
            .x
            <= width
    };
    if fits(text) {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let cut: String = chars.iter().take(mid).collect::<String>() + "…";
        if fits(&cut) { lo = mid } else { hi = mid - 1 }
    }
    chars
        .iter()
        .take(lo)
        .collect::<String>()
        .trim_end()
        .to_owned()
        + "…"
}

pub(super) fn lerp(a: f32, b: f32, k: f32) -> f32 {
    a + (b - a) * k
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lyrics() -> Lyrics {
        Lyrics {
            before: Some("It was late.".into()),
            now: "The mill stood still".into(),
            words: vec![0..3, 4..8, 9..14, 15..20],
            lit: 1..3,
            preferred: 1..3,
            heard: 1,
            after: None,
            key: 7,
        }
    }

    #[test]
    fn said_words_are_ink_lit_ones_accent_and_the_rest_muted() {
        let t = Tokens::new(&Default::default());
        let (job, now_at, lit_at) = layout(&lyrics(), 300.0, &t, 1.0);
        assert_eq!(now_at, "It was late.\n\n".chars().count());
        assert_eq!(lit_at, now_at + "The ".len());
        let colours: Vec<(String, Color32)> = (job.sections.iter())
            .map(|s| {
                (
                    job.text
                        .get(s.byte_range.start.0..s.byte_range.end.0)
                        .unwrap_or_default()
                        .to_owned(),
                    s.format.color,
                )
            })
            .collect();
        // Neighbours of one colour share a section.
        let of = |w: &str| {
            let has = |s: &String| s.split_whitespace().any(|x| x == w);
            colours.iter().find(|(s, _)| has(s)).map(|(_, c)| *c)
        };
        assert_eq!(of("The"), Some(t.ink));
        assert_eq!(of("mill"), Some(t.accent));
        assert_eq!(of("stood"), Some(t.accent));
        assert_eq!(of("still"), Some(t.muted));
    }

    #[test]
    fn every_byte_of_the_sentence_is_shown_once() {
        let t = Tokens::new(&Default::default());
        let l = lyrics();
        let (job, _, _) = layout(&l, 300.0, &t, 1.0);
        assert!(job.text.contains(&l.now));
        assert_eq!(job.text.matches("still").count(), 1);
    }
}
