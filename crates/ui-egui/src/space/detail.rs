//! A vocabulary row growing into its details, while its dictionary answers.

use egui::{
    Align, Color32, Context, Id, Label, Layout, Rect, RichText, Sense, Stroke, Ui, UiBuilder, vec2,
};
use mareader_dict::{Entry, Reply, Ticket};
use mareader_engine::space::{self, Row};

use super::{look, sentence};
use crate::icons::Icon;
use crate::theme::{self, Tokens};
use crate::widgets;
use crate::z_order::{self, Tier};

const DURATION: f64 = 0.24;

pub(super) struct Detail {
    row: Row,
    source: Rect,
    started: f64,
    closing: bool,
    close_from: f32,
    ticket: Option<Ticket>,
    entries: Option<Vec<Entry>>,
    failure: Option<String>,
}

impl Detail {
    pub fn new(row: Row, source: Rect, now: f64) -> Self {
        Self {
            row,
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
            (screen.width() - 32.0).clamp(0.0, 620.0),
            (screen.height() - 32.0).clamp(0.0, 590.0),
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
            ui.painter().rect_filled(rect, t.radius_lg, t.surface);
            ui.painter().rect_stroke(
                rect,
                t.radius_lg,
                Stroke::new(1.0, t.line),
                egui::StrokeKind::Inside,
            );
            if progress > 0.88 && !self.closing {
                let inner = rect.shrink(20.0);
                ui.scope_builder(
                    UiBuilder::new()
                        .max_rect(inner)
                        .layout(Layout::top_down(Align::Min)),
                    |ui| {
                        ui.set_clip_rect(inner);
                        close |= self.content(ui, t, inner.height());
                    },
                );
            } else {
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    &self.row.lemma,
                    theme::medium(t.text + 4.0),
                    t.ink,
                );
            }
        });
        if close {
            self.close(ctx);
        }
        true
    }

    fn content(&self, ui: &mut Ui, t: &Tokens, height: f32) -> bool {
        let mut close = false;
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.add(Label::new(
                RichText::new(&self.row.lemma)
                    .font(theme::semibold(t.text + 10.0))
                    .color(t.ink),
            ));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                close = widgets::small_icon_button(ui, Icon::Close, "Close word details").clicked();
            });
        });
        let remaining = (height - 46.0).max(0.0);
        egui::ScrollArea::vertical()
            .id_salt("word-detail-scroll")
            .max_height(remaining)
            .show(ui, |ui| {
                self.body(ui, t);
            });
        close
    }

    fn body(&self, ui: &mut Ui, t: &Tokens) {
        ui.horizontal_wrapped(|ui| {
            if let Some(pos) = &self.row.pos {
                look::quiet(ui, pos, t.text, t);
            }
            look::level(ui, self.row.band, t);
            look::tag(
                ui,
                space::status_label(self.row.status),
                look::status(self.row.status, t),
                6.0,
            );
        });
        section(ui, "Meaning", t);
        let meaning = if self.row.meaning.is_empty() {
            "No saved meaning."
        } else {
            &self.row.meaning
        };
        ui.label(RichText::new(meaning).size(t.text + 2.0).color(t.ink));
        if !self.row.context.is_empty() {
            section(ui, "From the book", t);
            ui.add(Label::new(sentence::highlight(&self.row.context, &self.row.form, t)).wrap());
        }
        section(ui, "Books", t);
        ui.label(self.row.books.join(", "));
        self.extra(ui, t);
        self.history(ui, t);
    }

    fn extra(&self, ui: &mut Ui, t: &Tokens) {
        let mut synonyms = self.row.synonyms.clone();
        let mut examples = self.row.usages.clone();
        let mut definitions = Vec::new();
        if let Some(entries) = &self.entries {
            for entry in entries {
                if !entry.definition.is_empty()
                    && entry.definition != self.row.meaning
                    && !definitions.contains(&entry.definition)
                {
                    definitions.push(entry.definition.clone());
                }
                if let Some(ai) = &entry.ai {
                    for synonym in &ai.synonyms {
                        if !synonyms.contains(synonym) {
                            synonyms.push(synonym.clone());
                        }
                    }
                    for usage in &ai.usages {
                        if !examples.contains(usage) {
                            examples.push(usage.clone());
                        }
                    }
                }
            }
        }
        if !definitions.is_empty() {
            section(ui, "Dictionary", t);
            for item in definitions {
                ui.label(item);
            }
        }
        section(ui, "Synonyms", t);
        if synonyms.is_empty() {
            look::quiet(ui, "None available", t.text, t);
        } else {
            ui.label(synonyms.join(", "));
        }
        section(ui, "Examples", t);
        if examples.is_empty() {
            look::quiet(ui, "None available", t.text, t);
        } else {
            for item in examples {
                ui.label(item);
            }
        }
        if self.entries.is_none() {
            look::quiet(ui, "Looking up dictionary details…", t.text_small, t);
        } else if let Some(why) = &self.failure {
            look::quiet(ui, &format!("Dictionary lookup: {why}"), t.text_small, t);
        }
    }

    fn history(&self, ui: &mut Ui, t: &Tokens) {
        if self.row.history.is_empty() && self.row.by != mareader_library::LearnedBy::Reactive {
            return;
        }
        section(ui, "Your recall answers", t);
        if self.row.history.is_empty() {
            look::quiet(
                ui,
                "No answer text was saved for earlier recalls.",
                t.text,
                t,
            );
        }
        for run in &self.row.history {
            let title = format!("{} · {}", run.book, space::date(run.when));
            ui.label(RichText::new(title).color(t.muted));
            for (i, answer) in run.answers.iter().enumerate() {
                let title = format!("{}. {}  ·  {}", i + 1, answer.text, answer.verdict);
                ui.label(RichText::new(title).color(t.ink));
                if !answer.feedback.is_empty() {
                    look::quiet(ui, &answer.feedback, t.text_small, t);
                }
            }
            ui.add_space(6.0);
        }
    }
}

fn section(ui: &mut Ui, title: &str, t: &Tokens) {
    ui.add_space(12.0);
    look::caption(ui, title, t);
}
