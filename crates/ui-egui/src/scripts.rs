//! A book's scripts: the pages they made in a pane, and the clicks sent.

use egui::{Context, Id, Margin, Pos2, Rect, RichText};
use mareader_engine::scripts::Field;

use mareader_engine::scripts::{Ask, Paged};

use crate::bubble::{Bubble, Side};
use crate::selection::Sheet;
use crate::theme::Tokens;
use crate::widgets;

const WIDTH: f32 = 320.0;
/// Characters of an alert shown in a toast.
const MAX_ALERT: usize = 300;

/// An alert's message as a toast says it.
pub fn alert_text(message: &str) -> String {
    let message = message.trim();
    match message.char_indices().nth(MAX_ALERT) {
        Some((cut, _)) => format!("{}…", message.get(..cut).unwrap_or_default()),
        None => message.to_owned(),
    }
}

/// The card a field is typed in, on its mark while its page shows.
pub fn edit(ctx: &Context, paged: &mut Paged, sheets: &[Sheet], room: Rect, t: &Tokens) {
    let Some(editing) = paged.editing.as_mut() else {
        return;
    };
    let Some(at) = (sheets.iter())
        .find(|s| s.page == editing.page)
        .map(|s| s.to_screen(editing.at))
    else {
        paged.editing = None;
        return;
    };
    let id = Id::new(("script field", editing.chapter, editing.node));
    let bubble = Bubble {
        id,
        at,
        room,
        width: WIDTH,
        margin: Margin::same(12),
        snug: true,
        prefer: Side::Below,
    };
    let mut done = false;
    let (card, ()) = bubble.show(ctx, t, |ui, _| {
        let input = match editing.field.multiline {
            true => widgets::input_multiline(&mut editing.text).rows(4),
            false => widgets::input(&mut editing.text),
        };
        let response = input
            .hint(&editing.field.hint)
            .password(editing.field.secret)
            .text_size(14.0)
            .show(ui)
            .response;
        if !editing.focused {
            response.request_focus();
            editing.focused = true;
        }
        let entered = ui.input(|i| i.key_pressed(egui::Key::Enter));
        done |= !editing.field.multiline && response.lost_focus() && entered;
        ui.add_space(8.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let button = egui::Button::new(RichText::new("Done").color(t.accent));
            done |= ui.add(button).clicked();
        });
    });
    let escape = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    let outside = ctx.input(|i| {
        let away = |p: Pos2| !card.contains(p) && !at.expand(4.0).contains(p);
        i.pointer.any_pressed() && i.pointer.press_origin().is_some_and(away)
    });
    if escape {
        paged.editing = None;
    } else if done || outside {
        // As a browser does when a field loses focus, what was typed counts.
        paged.commit();
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use running::Runtimes;

#[cfg(target_arch = "wasm32")]
pub use browser::Runtimes;

#[cfg(not(target_arch = "wasm32"))]
mod running {
    use std::collections::{HashMap, HashSet};
    use std::sync::Arc;
    use std::time::Duration;

    use egui::Context;
    use mareader_epub::Files;
    use mareader_script::{Page, Scripts};
    use mareader_session::Document;
    use mareader_workspace::PaneId;

    use super::{Ask, Field, Paged, alert_text};
    use crate::documents::DocumentsUi;
    use mareader_engine::documents::State as DocumentsState;
    use mareader_engine::session::Waker;
    use mareader_engine::toast::Toast;

    /// Script errors a pane keeps.
    const MAX_ERRORS: usize = 8;
    /// How often a busy runtime is looked at, to notice it stuck.
    const CHECK: Duration = Duration::from_millis(500);

    /// A pane's book and its scripts; none once stopped or if it has none.
    struct Running {
        /// The book's file: one across the twins each new insert makes.
        files: Option<Files>,
        scripts: Option<Scripts>,
    }

    /// The window's script runtimes, one per pane showing a scripted book.
    #[derive(Default)]
    pub struct Runtimes {
        running: HashMap<PaneId, Running>,
        /// Runtimes started so far.
        starts: usize,
    }

    impl Runtimes {
        /// Starts and stops runtimes with the panes; sends clicks, takes pages.
        pub fn tick(
            &mut self,
            ctx: &Context,
            wake: &Waker,
            state: &DocumentsState,
            documents: &mut DocumentsUi,
        ) -> Vec<Toast> {
            let mut toasts = Vec::new();
            let mut seen = HashSet::new();
            for (pane, doc, paged) in documents.scripted(state) {
                seen.insert(pane);
                let files = doc.files();
                let same = self
                    .running
                    .get(&pane)
                    .is_some_and(|r| match (&r.files, &files) {
                        (Some(was), Some(is)) => was.same(is),
                        (was, is) => was.is_none() && is.is_none(),
                    });
                if !same {
                    *paged = Paged::default();
                    let scripts = start(Arc::clone(wake), &doc);
                    self.starts += 1;
                    self.running.insert(pane, Running { files, scripts });
                }
                let Some(run) = self.running.get_mut(&pane) else {
                    continue;
                };
                let Some(scripts) = &run.scripts else {
                    paged.asks.clear();
                    continue;
                };
                for (chapter, ask) in paged.asks.drain(..) {
                    scripts.ask(chapter, to_script(ask));
                }
                for change in scripts.changes() {
                    paged.errors.extend(change.errors.iter().cloned());
                    let cut = paged.errors.len().saturating_sub(MAX_ERRORS);
                    paged.errors.drain(..cut);
                    let fields = change.fields.into_iter().map(|(n, f)| (n, field(f)));
                    paged.take(change.chapter, change.page, fields.collect());
                    let alerts = change.alerts.iter().map(|a| alert_text(a));
                    toasts.extend(alerts.filter(|a| !a.is_empty()).map(Toast::info));
                }
                if scripts.stuck() {
                    run.scripts = None;
                    let why = "This book's scripts stopped responding, so they were turned off.";
                    toasts.push(Toast::error(why));
                } else if scripts.busy() {
                    ctx.request_repaint_after(CHECK);
                }
            }
            self.running.retain(|pane, _| seen.contains(pane));
            toasts
        }

        /// The runtimes as screenshots report them.
        pub fn describe(&self) -> String {
            let all = self.running.values();
            let on = all.clone().filter(|r| r.scripts.is_some()).count();
            let busy = if self.idle() { "idle" } else { "busy" };
            let each = self.running.values().filter_map(|r| r.scripts.as_ref());
            let each: Vec<_> = each.map(Scripts::describe).collect();
            let (n, starts) = (self.running.len(), self.starts);
            format!("{n} runtimes, {on} running, {busy}, {starts} started; {each:?}")
        }

        /// Whether no runtime is in the middle of a call.
        pub fn idle(&self) -> bool {
            let mut scripts = self.running.values().filter_map(|r| r.scripts.as_ref());
            scripts.all(|s| !s.busy())
        }
    }

    /// `doc`'s scripts on their own thread; none if it has none.
    fn start(wake: Waker, doc: &Document) -> Option<Scripts> {
        let pages: Vec<Page> = (doc.scripts().into_iter())
            .map(|s| Page {
                chapter: s.chapter,
                xhtml: s.xhtml,
                scripts: s.code,
                styles: s.styles,
            })
            .collect();
        if pages.is_empty() {
            return None;
        }
        Scripts::start(pages, move || wake())
    }

    fn to_script(ask: Ask) -> mareader_script::Ask {
        match ask {
            Ask::Click(node) => mareader_script::Ask::Click(node),
            Ask::Value(node, value) => mareader_script::Ask::Value(node, value),
            Ask::Pick(selector) => mareader_script::Ask::Pick(selector),
        }
    }

    fn field(f: mareader_script::Field) -> Field {
        Field {
            value: f.value,
            hint: f.hint,
            multiline: f.multiline,
            secret: f.secret,
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod browser {
    use egui::Context;

    use crate::documents::DocumentsUi;
    use mareader_engine::documents::State as DocumentsState;
    use mareader_engine::session::Waker;
    use mareader_engine::toast::Toast;

    /// No scripts run in the browser build; their pages show as written.
    #[derive(Default)]
    pub struct Runtimes;

    impl Runtimes {
        pub fn tick(
            &mut self,
            _: &Context,
            _: &Waker,
            _: &DocumentsState,
            _: &mut DocumentsUi,
        ) -> Vec<Toast> {
            Vec::new()
        }

        pub fn idle(&self) -> bool {
            true
        }

        pub fn describe(&self) -> String {
            String::new()
        }
    }
}
