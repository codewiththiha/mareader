use super::*;
use crate::ui_command::UiCommand;
use mareader_engine::command::Command;

impl MareaderApp {
    pub(super) fn glosses(&mut self, ctx: &Context, out: &mut Commands) {
        self.staged(ctx, out);
        self.seeds(ctx, out);
        self.gloss_events(ctx, out);
        self.poll_services(ctx);
        let sounds = self.show_study(ctx, out);
        self.show_page_ai(ctx, out, &sounds);
    }

    fn staged(&mut self, ctx: &Context, out: &mut Commands) {
        let pane = self.active();
        let (has_sheets, front_shown) = {
            let sheets = self.documents().sheets(pane);
            let front = self.ui.staged.front().map(|s| s.page);
            (
                !sheets.is_empty(),
                front.is_some_and(|p| sheets.iter().any(|s| s.page == p)),
            )
        };
        match self.ui.staged.pop_front() {
            // Its page is turned to first, then selected once laid out.
            Some(mut staged) if has_sheets && !front_shown => {
                if !staged.turned {
                    out.push(Sent::Engine(Command::GoTo {
                        pane,
                        page: staged.page,
                    }));
                }
                staged.turned = true;
                self.ui.staged.push_front(staged);
                ctx.request_repaint();
            }
            Some(Staged {
                page,
                text,
                explain,
                look_up,
                pen,
                note,
                bookmark,
                pick,
                ..
            }) if has_sheets => {
                let first = self.marks(pane).first().map(|m| m.id);
                match (bookmark, first) {
                    (true, _) => out.push(Sent::Engine(Command::Bookmark(Some(pane)))),
                    // Picking starts, then a later `pick` takes all in.
                    (false, Some(id)) if text.is_empty() && !self.ui.study.picking() => {
                        let start = mareader_engine::study::Action::SelectSeveral(pane, id);
                        out.push(Sent::Engine(Command::Study(start)));
                    }
                    (false, Some(_)) if text.is_empty() => {
                        out.push(Sent::Engine(Command::Study(
                            mareader_engine::study::Action::PickAll,
                        )));
                        if pick {
                            egui::Popup::open_id(ctx, egui::Id::new(crate::study::PICK_STYLE));
                        }
                    }
                    _ => out.push(Sent::Engine(Command::SelectText { pane, page, text })),
                }
                if explain {
                    out.push(Sent::Engine(Command::Explain(pane)));
                }
                if look_up {
                    out.push(Sent::Engine(Command::LookUp(pane)));
                }
                if let Some(pen) = pen {
                    let color = None;
                    out.push(Sent::Engine(Command::Highlight { pane, pen, color }));
                }
                if let Some((text, save)) = note {
                    out.push(Sent::Engine(Command::Note(pane)));
                    out.push(Sent::Engine(Command::Study(
                        mareader_engine::study::Action::TypeNote(text, save),
                    )));
                }
                // The next waits a frame, as each selection replaces the last.
                if !self.ui.staged.is_empty() {
                    ctx.request_repaint();
                }
            }
            Some(staged) => self.ui.staged.push_front(staged),
            None => {}
        }
    }

    fn seeds(&mut self, ctx: &Context, out: &mut Commands) {
        let pane = self.active();
        let (word_ready, ai_ready) = {
            let sheets = self.documents().sheets(pane);
            let shown = |page| sheets.iter().any(|s| s.page == page);
            let word = self
                .ui
                .word_window_seed
                .as_ref()
                .is_some_and(|p| p.first().is_some_and(|&p| shown(p)));
            let ai = self
                .ui
                .page_ai_seed
                .as_ref()
                .is_some_and(|(page, ..)| shown(*page));
            (word, ai)
        };
        if word_ready && let Some(pages) = self.ui.word_window_seed.take() {
            out.push(Sent::Engine(Command::WordListWindow { pane, pages }));
        }
        if ai_ready && let Some((page, task, follow, last)) = self.ui.page_ai_seed.clone() {
            let now = ctx.input(|i| i.time);
            let done = match follow {
                false if last > page => {
                    let pages: Vec<usize> = (page..=last).collect();
                    self.session
                        .page_ai_run_pages(pane, page, &self.ui.ai, &pages, task);
                    true
                }
                true if self.session.page_ai_can_read(pane, page) => {
                    self.session
                        .page_ai_follow(pane, page, &self.ui.ai, task, true, now);
                    true
                }
                true => false,
                false => self
                    .session
                    .page_ai_seed(pane, page, &self.ui.ai, task, now),
            };
            if done {
                self.ui.page_ai_seed = None;
            }
            self.drain(ctx);
            ctx.request_repaint();
        }
    }

    fn gloss_events(&mut self, ctx: &Context, out: &mut Commands) {
        for (pane, gloss) in self.ui.documents.take_gloss() {
            if study_gloss(&gloss) {
                self.study_gloss(ctx, out, pane, gloss);
            } else if page_gloss(&gloss) {
                self.page_gloss(ctx, out, pane, gloss);
            } else {
                self.pill_gloss(ctx, out, pane, gloss);
            }
        }
    }

    fn study_gloss(&mut self, ctx: &Context, out: &mut Commands, pane: PaneId, gloss: Gloss) {
        match gloss {
            Gloss::Explain => out.push(Sent::Engine(Command::Explain(pane))),
            Gloss::LookUp => out.push(Sent::Engine(Command::LookUp(pane))),
            Gloss::Note => out.push(Sent::Engine(Command::Note(pane))),
            Gloss::ReadAloud => {
                if let Some(sel) = self.session.documents().selection(pane) {
                    let (a, b) = sel.ordered();
                    let spot = |c: mareader_session::Caret| mareader_speech::Spot {
                        page: c.page,
                        char: c.index,
                    };
                    let from = ReadFrom {
                        anchor: None,
                        at: Some(spot(a)),
                        until: Some(spot(b)),
                    };
                    self.session.execute(Command::ClearSelection(pane), 0.0);
                    out.push(Sent::Engine(Command::ReadAloud { pane, from }));
                }
            }
            Gloss::ReadFrom(from) => out.push(Sent::Engine(Command::ReadAloud { pane, from })),
            Gloss::Highlight(pen, color) => {
                out.push(Sent::Engine(Command::Highlight { pane, pen, color }))
            }
            Gloss::Copied => out.push(Sent::Ui(UiCommand::Notify(Toast::info("Copied")))),
            Gloss::Open { mark, at } => {
                self.ui
                    .study
                    .open_mark(ctx, &mut self.session, pane, mark, at);
                self.ask_role();
            }
            Gloss::Shaded { page, chars, at } => self.explain_shaded(pane, page, chars, at),
            Gloss::Rest { word, context, at } => self.rest(pane, (word, context), at),
            Gloss::Menu { mark, at, words } => {
                self.ui.menu_shot = false;
                let state = self.session.study();
                let model = self.session.library_model();
                self.ui.study.open_menu(state, model, pane, mark, at, words);
            }
            Gloss::Anchor(at) => self.ui.study.anchor(pane, at),
            Gloss::Pick(mark) => self.ui.study.pick(pane, mark),
            Gloss::Toggle(mark) => self.ui.study.toggle(pane, mark),
            Gloss::Seen { page, all } => self.ui.study.seen(pane, page, all),
            _ => {}
        }
    }

    fn page_gloss(&mut self, ctx: &Context, _out: &mut Commands, pane: PaneId, gloss: Gloss) {
        match gloss {
            Gloss::Lift(origin) if self.tree().len() > 1 => {
                let home = self.ui.pane_rects.iter().find(|(id, _)| *id == pane);
                if let Some(&(_, home)) = home {
                    let since = ctx.input(|i| i.time);
                    let lift = crate::lift::Lift {
                        pane,
                        origin,
                        home,
                        since,
                    };
                    self.ui.lifting = Some(lift);
                    self.focus(pane);
                    ctx.request_repaint();
                }
            }
            Gloss::Lift(_) => {}
            Gloss::PageMenu {
                page,
                at,
                place,
                caret,
            } => {
                let aloud = self.session.documents().place(pane).map(|_| ReadFrom {
                    anchor: place,
                    at: caret.map(|char| mareader_speech::Spot { page, char }),
                    until: None,
                });
                self.ui.page_ai.open_menu(pane, page, at, aloud);
                if let Some(pin) = self.documents().menu_pin(pane, page, at, caret) {
                    self.ui.page_ai.pin_menu(pin);
                }
            }
            Gloss::Bubble { page, index, at } => {
                let now = ctx.input(|i| i.time);
                self.ui
                    .page_ai
                    .pick(&mut self.session, pane, page, index, at, now);
            }
            Gloss::BubbleMenu { page, index, at } => {
                self.ui
                    .page_ai
                    .open_bubble_menu(self.session.page_ai(), pane, page, index, at);
            }
            _ => {}
        }
    }

    fn pill_gloss(&mut self, ctx: &Context, _out: &mut Commands, pane: PaneId, gloss: Gloss) {
        match gloss {
            Gloss::Pill(mareader_engine::page_ai::view::PillPress::Stop) => {
                let stop = mareader_engine::page_ai::Action::StopPill(pane);
                let now = ctx.input(|i| i.time);
                self.session.execute(Command::PageAi(stop), now);
            }
            Gloss::Pill(mareader_engine::page_ai::view::PillPress::Continue) => {
                let Some(page) = self.session.documents().current(pane) else {
                    return;
                };
                let now = ctx.input(|i| i.time);
                self.session.page_ai_resume(pane, page, &self.ui.ai, now);
                self.drain(ctx);
            }

            _ => {}
        }
    }

    fn poll_services(&mut self, ctx: &Context) {
        let docs = self.ui.documents.as_view(self.session.documents());
        for (pane, _) in &self.ui.pane_rects {
            if docs.sheets(*pane).is_empty() {
                self.ui.study.hold(*pane);
            }
        }
        let ai_look = self.settings().ai.clone();
        if let Some(explainer) = self.ui.ai.follow(&ai_look) {
            self.session.study_set_explainer(explainer);
        }
        self.take_organized();
        self.tick_session(ctx);
    }

    fn show_study(
        &mut self,
        ctx: &Context,
        out: &mut Commands,
    ) -> Vec<(String, Vec<mareader_engine::sounds::Line>)> {
        let sounds = self.card_sounds(ctx.input(|i| i.time));
        // Read before the mutable borrows below.
        let (gloss_look, highlight_look) = (
            self.settings().study.gloss.clone(),
            self.settings().study.highlights.clone(),
        );
        let dictionary_look = self.settings().dictionary.clone();
        let aloud_look = self.settings().read_aloud.clone();
        let looks = (&gloss_look, &highlight_look);
        let others = dictionary_look.other_voices;
        let dict = &dictionary_look;
        let looking = self.ui.dict.card_word().is_some() || dict.window.open;
        if others && (looking || self.route() == Route::Dictionary) {
            self.session.read_aloud_list_voices();
        }
        let model = self.session.library_model();
        let saying = self.session.read_aloud().saying();
        let (speakers, offers) = self.session.read_aloud().speakers(&aloud_look, others);
        self.ui.dict.set_voice(crate::dict::Voice {
            saying: saying.map(str::to_owned),
            sounds: sounds.clone(),
            speakers,
            offers,
        });
        let heard = mareader_engine::sounds::Heard {
            saying,
            sounds: &sounds,
        };
        let panes = (&self.ui.pane_rects[..], heard);
        let study_state = (self.session.study(), model);
        self.ui.study.show(ctx, study_state, panes, looks, out);
        sounds
    }

    fn show_page_ai(
        &mut self,
        ctx: &Context,
        out: &mut Commands,
        sounds: &[(String, Vec<mareader_engine::sounds::Line>)],
    ) {
        let now = ctx.input(|i| i.time);
        self.ui.page_ai.receive_cards(ctx, self.ui.documents.gpu());
        if let Some(wait) = self.session.page_ai_tick(&self.ui.ai, now) {
            ctx.request_repaint_after(wait);
        }
        // A book lays its pane's shown results and transcripts into its pages.
        let ai_look = self.settings().ai.clone();
        let wanted = mareader_engine::transcript::wanted(&ai_look);
        for pane in self.tree().panes() {
            let ai = self.session.page_ai_inserts(pane);
            let own = self
                .ui
                .documents
                .transcripts(self.session.documents(), pane, &wanted);
            let shown = self.ui.documents.triggered(self.session.documents(), pane);
            let ai = mareader_engine::transcript::combine(ai, own);
            if let Some(inserts) = mareader_engine::transcript::combine(ai, shown) {
                self.ui
                    .documents
                    .set_inserts(&mut self.session, pane, inserts);
            }
        }
        let state = (self.session.page_ai(), self.session.library_model());
        let docs = self.ui.documents.as_view(self.session.documents());
        self.ui
            .page_ai
            .show(ctx, state, &ai_look, (&self.ui.pane_rects, docs), out);
        self.drain(ctx);
        self.ui
            .dict
            .set_exact_only(!self.settings().dictionary.near);
        let settings = self.settings().clone();
        self.ui
            .word_window
            .show(ctx, &settings, (&mut self.session, &self.ui.files), out);
        let docs = self.ui.documents.as_view(self.session.documents());
        let offset = |pane| docs.offset(pane);
        let saying = self.session.read_aloud().saying();
        let heard = mareader_engine::sounds::Heard { saying, sounds };
        let attached = self.session.dict().attached();
        self.ui
            .dict
            .show(ctx, attached, (&self.ui.pane_rects, heard), offset, out);
    }
}

fn study_gloss(gloss: &Gloss) -> bool {
    matches!(
        gloss,
        Gloss::Explain
            | Gloss::LookUp
            | Gloss::Note
            | Gloss::ReadAloud
            | Gloss::ReadFrom(..)
            | Gloss::Highlight(..)
            | Gloss::Copied
            | Gloss::Open { .. }
            | Gloss::Shaded { .. }
            | Gloss::Rest { .. }
            | Gloss::Menu { .. }
            | Gloss::Anchor(..)
            | Gloss::Pick(..)
            | Gloss::Toggle(..)
            | Gloss::Seen { .. }
    )
}

fn page_gloss(gloss: &Gloss) -> bool {
    matches!(
        gloss,
        Gloss::Lift(..) | Gloss::PageMenu { .. } | Gloss::Bubble { .. } | Gloss::BubbleMenu { .. }
    )
}
