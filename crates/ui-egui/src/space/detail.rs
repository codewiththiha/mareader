//! A vocabulary row growing into its details, while its dictionary answers.

mod content;

use egui::{Align, Color32, Context, Id, Layout, Rect, Sense, Stroke, UiBuilder, vec2};
use mareader_dict::{Entry, Reply, Ticket};
use mareader_engine::space::{History, Row};

use crate::theme::{self, Tokens};
use crate::z_order::{self, Tier};

const DURATION: f64 = 0.24;

pub(super) struct Detail {
    row: Row,
    history: Vec<History>,
    source: Rect,
    started: f64,
    closing: bool,
    close_from: f32,
    ticket: Option<Ticket>,
    entries: Option<Vec<Entry>>,
    failure: Option<String>,
}

impl Detail {
    pub fn new(row: Row, history: Vec<History>, source: Rect, now: f64) -> Self {
        Self {
            row,
            history,
            source,
            started: now,
            closing: false,
            close_from: 1.0,
            ticket: None,
            entries: None,
            failure: None,
        }
    }

    pub fn word(&self) -> &str {
        &self.row.lemma
    }

    pub fn ticket(&mut self, ticket: Ticket) {
        self.ticket = Some(ticket);
    }

    pub fn receive(&mut self, reply: &Reply) {
        if self.ticket != Some(reply.ticket) {
            return;
        }
        self.ticket = None;
        self.failure = reply.failed.clone();
        self.entries = Some(
            reply
                .entries
                .iter()
                .filter(|entry| entry.word.to_lowercase() == self.row.lemma.to_lowercase())
                .cloned()
                .collect(),
        );
    }

    fn progress(&self, ctx: &Context, now: f64) -> f32 {
        if !crate::motion::enabled(ctx) {
            return if self.closing { 0.0 } else { 1.0 };
        }
        let elapsed = ((now - self.started) / DURATION).clamp(0.0, 1.0) as f32;
        if self.closing {
            (self.close_from - elapsed).max(0.0)
        } else {
            elapsed
        }
    }

    pub fn close(&mut self, ctx: &Context) {
        if !self.closing {
            let now = ctx.input(|i| i.time);
            self.close_from = self.progress(ctx, now);
            self.started = now;
            self.closing = true;
            ctx.request_repaint();
        }
    }

    /// Returns false after the closing animation has finished.
    pub fn show(&mut self, ctx: &Context, t: &Tokens) -> bool {
        let now = ctx.input(|i| i.time);
        let progress = self.progress(ctx, now);
        if self.closing && progress <= 0.0 {
            return false;
        }
        if progress < 1.0 || self.closing {
            ctx.request_repaint();
        }
        let eased = progress * progress * (3.0 - 2.0 * progress);
        let screen = ctx.content_rect();
        let size = vec2(
            (screen.width() - 32.0).clamp(0.0, 680.0),
            (screen.height() - 32.0).clamp(0.0, 520.0),
        );
        let target = Rect::from_center_size(screen.center(), size);
        let rect = Rect::from_min_max(
            self.source.min.lerp(target.min, eased),
            self.source.max.lerp(target.max, eased),
        );
        let area =
            z_order::area(ctx, Tier::Modal, Id::new("space-word-detail")).fixed_pos(screen.min);
        ctx.memory_mut(|m| m.set_modal_layer(area.layer()));
        let mut close = false;
        area.show(ctx, |ui| {
            let (back, response) = ui.allocate_exact_size(screen.size(), Sense::click());
            let alpha = (if t.dark { 90.0 } else { 48.0 }) * eased;
            ui.painter()
                .rect_filled(back, 0.0, Color32::from_black_alpha(alpha as u8));
            if response.clicked()
                && !rect.contains(response.interact_pointer_pos().unwrap_or_default())
            {
                close = true;
            }
            ui.painter().rect_filled(rect, t.radius_lg, t.card);
            ui.painter().rect_stroke(
                rect,
                t.radius_lg,
                Stroke::new(1.0, t.line),
                egui::StrokeKind::Inside,
            );
            let content = ((progress - 0.45) / 0.4).clamp(0.0, 1.0);
            if content > 0.0 && rect.width() > 40.0 && rect.height() > 40.0 {
                let inner = rect.shrink(20.0);
                ui.scope_builder(
                    UiBuilder::new()
                        .max_rect(inner)
                        .layout(Layout::top_down(Align::Min)),
                    |ui| {
                        ui.set_clip_rect(inner);
                        ui.set_opacity(content);
                        let pressed = content::show(ui, self, t, inner.height());
                        close |= pressed && progress >= 0.95 && !self.closing;
                    },
                );
            }
            if content < 1.0 {
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    &self.row.lemma,
                    theme::medium(t.text + 4.0),
                    t.ink.gamma_multiply(1.0 - content),
                );
            }
        });
        if close {
            self.close(ctx);
        }
        true
    }

}

#[cfg(test)]
mod tests {
    use mareader_library::{LearnedBy, WordId, WordStatus};

    use super::*;

    fn row() -> Row {
        Row {
            key: 1,
            word: WordId(1),
            lemma: "quick".into(),
            pos: None,
            band: 4,
            status: WordStatus::Learning,
            meaning: "fast".into(),
            context: "A quick walk.".into(),
            form: "quick".into(),
            books: Vec::new(),
            book_ids: Vec::new(),
            met_at: None,
            seen: 0,
            tries: 1,
            by: LearnedBy::Reactive,
            first_try: true,
            synonyms: Vec::new(),
            usages: Vec::new(),
        }
    }

    #[test]
    fn history_repeats_book_and_date_only_when_needed() {
        let mut word = row();
        word.books = vec!["One".into()];
        word.seen = 1_792_225_800_000;
        let run = History { book: "One".into(), when: word.seen, answers: Vec::new() };
        let mut detail = Detail::new(word, vec![run.clone()], Rect::NOTHING, 0.0);
        assert_eq!(content::session_title(&detail, &run), "");
        detail.history.push(run.clone());
        assert_eq!(content::session_title(&detail, &run), "Oct 17, 2026");
        detail.row.books.push("Two".into());
        assert_eq!(content::session_title(&detail, &run), "One · Oct 17, 2026");
    }

    #[test]
    fn motion_advances_without_a_dictionary_reply_and_ignores_stale_replies() {
        let ctx = Context::default();
        let source = Rect::from_min_size(egui::Pos2::ZERO, vec2(100.0, 40.0));
        let mut detail = Detail::new(row(), Vec::new(), source, 0.0);
        detail.ticket(Ticket(7));
        assert!((detail.progress(&ctx, DURATION / 2.0) - 0.5).abs() < 0.001);
        detail.receive(&Reply {
            ticket: Ticket(6),
            ..Reply::default()
        });
        assert!(detail.entries.is_none());
        assert_eq!(detail.progress(&ctx, DURATION), 1.0);
        detail.receive(&Reply {
            ticket: Ticket(7),
            ..Reply::default()
        });
        assert!(detail.entries.is_some());
        detail.closing = true;
        detail.started = DURATION;
        assert!((detail.progress(&ctx, DURATION * 1.5) - 0.5).abs() < 0.001);
        crate::motion::set(&ctx, &mareader_settings::MotionSettings::default(), true);
        assert_eq!(detail.progress(&ctx, DURATION), 0.0);
    }
}
