use super::*;

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
        let pane = self.active;
        let sheets = self.documents.sheets(pane);
        let shown = |page| sheets.iter().any(|s| s.page == page);
        match self.staged.pop_front() {
            // Its page is turned to first, then selected once laid out.
            Some(mut staged) if !sheets.is_empty() && !shown(staged.page) => {
                if !staged.turned {
                    out.push(Command::GoTo {
                        pane,
                        page: staged.page,
                    });
                }
                staged.turned = true;
                self.staged.push_front(staged);
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
            }) if !sheets.is_empty() => {
                let first = self.marks(pane).first().map(|m| m.id);
                match (bookmark, first) {
                    (true, _) => out.push(Command::Bookmark(Some(pane))),
                    // Picking starts, then a later `pick` takes all in.
                    (false, Some(id)) if text.is_empty() && !self.study.picking() => {
                        let start = crate::study::Action::SelectSeveral(pane, id);
                        out.push(Command::Study(start));
                    }
                    (false, Some(_)) if text.is_empty() => {
                        out.push(Command::Study(crate::study::Action::PickAll));
                        if pick {
                            egui::Popup::open_id(ctx, egui::Id::new(crate::study::PICK_STYLE));
                        }
                    }
                    _ => out.push(Command::SelectText { pane, page, text }),
                }
                if explain {
                    out.push(Command::Explain(pane));
                }
                if look_up {
                    out.push(Command::LookUp(pane));
                }
                if let Some(pen) = pen {
                    let color = None;
                    out.push(Command::Highlight { pane, pen, color });
                }
                if let Some((text, save)) = note {
                    out.push(Command::Note(pane));
                    out.push(Command::Study(crate::study::Action::TypeNote(text, save)));
                }
                // The next waits a frame, as each selection replaces the last.
                if !self.staged.is_empty() {
                    ctx.request_repaint();
                }
            }
            Some(staged) => self.staged.push_front(staged),
            None => {}
        }
    }

    fn seeds(&mut self, ctx: &Context, out: &mut Commands) {
        let pane = self.active;
        let sheets = self.documents.sheets(pane);
        let shown = |page| sheets.iter().any(|s| s.page == page);
        if let Some(pages) = self
            .word_window_seed
            .take_if(|p| p.first().is_some_and(|&p| shown(p)))
        {
            out.push(Command::WordListWindow { pane, pages });
        }
        if let Some((page, task, follow, last)) = self.page_ai_seed.clone()
            && shown(page)
        {
            let book = self.library.book_in(&self.documents, pane);
            let name = self.documents.title(pane).unwrap_or_default().to_owned();
            let asked = crate::page_ai::Asked {
                pane,
                page,
                book,
                name: &name,
                documents: &self.documents,
                ai: &self.ai,
                settings: &self.settings.ai,
            };
            let now = ctx.input(|i| i.time);
            let done = match follow {
                false if last > page => {
                    let pages: Vec<usize> = (page..=last).collect();
                    self.page_ai.run_pages(asked, &pages, task);
                    true
                }
                true if self.page_ai.can_read(&asked) => {
                    let model = self.library.model();
                    self.page_ai.follow(asked, model, task, true, now, out);
                    true
                }
                true => false,
                false => self
                    .page_ai
                    .seed(asked, self.library.model_mut(), task, now),
            };
            if done {
                self.page_ai_seed = None;
            }
            ctx.request_repaint();
        }
    }

    fn gloss_events(&mut self, ctx: &Context, out: &mut Commands) {
        for (pane, gloss) in self.documents.take_gloss() {
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
            Gloss::Explain => out.push(Command::Explain(pane)),
            Gloss::LookUp => out.push(Command::LookUp(pane)),
            Gloss::Note => out.push(Command::Note(pane)),
            Gloss::ReadAloud => {
                if let Some(sel) = self.documents.selection(pane) {
                    let (a, b) = sel.ordered();
                    let spot = |c: crate::selection::Caret| mareader_speech::Spot {
                        page: c.page,
                        char: c.index,
                    };
                    let from = ReadFrom {
                        anchor: None,
                        at: Some(spot(a)),
                        until: Some(spot(b)),
                    };
                    self.documents.clear_selection(pane);
                    out.push(Command::ReadAloud { pane, from });
                }
            }
            Gloss::ReadFrom(from) => out.push(Command::ReadAloud { pane, from }),
            Gloss::Highlight(pen, color) => out.push(Command::Highlight { pane, pen, color }),
            Gloss::Copied => out.push(Command::Notify(Toast::info("Copied"))),
            Gloss::Open { mark, at } => {
                self.study
                    .open_mark(ctx, self.library.model(), pane, mark, at);
                self.ask_role();
            }
            Gloss::Shaded { page, chars, at } => self.explain_shaded(ctx, pane, page, chars, at),
            Gloss::Rest { word, context, at } => self.rest(pane, (word, context), at),
            Gloss::Menu { mark, at, words } => {
                self.menu_shot = false;
                let model = self.library.model();
                self.study.open_menu(model, pane, mark, at, words);
            }
            Gloss::Anchor(at) => self.study.anchor(pane, at),
            Gloss::Pick(mark) => self.study.pick(pane, mark),
            Gloss::Toggle(mark) => self.study.toggle(pane, mark),
            Gloss::Seen { page, all } => self.study.seen(pane, page, all),
            _ => {}
        }
    }

    fn page_gloss(&mut self, ctx: &Context, _out: &mut Commands, pane: PaneId, gloss: Gloss) {
        match gloss {
            Gloss::Lift(origin) if self.tree.len() > 1 => {
                let home = self.pane_rects.iter().find(|(id, _)| *id == pane);
                if let Some(&(_, home)) = home {
                    let since = ctx.input(|i| i.time);
                    let lift = crate::lift::Lift {
                        pane,
                        origin,
                        home,
                        since,
                    };
                    self.lifting = Some(lift);
                    self.active = pane;
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
                let aloud = self.documents.reading(pane).map(|_| ReadFrom {
                    anchor: place,
                    at: caret.map(|char| mareader_speech::Spot { page, char }),
                    until: None,
                });
                self.page_ai.open_menu(pane, page, at, aloud);
                if let Some(pin) = self.documents.menu_pin(pane, page, at, caret) {
                    self.page_ai.pin_menu(pin);
                }
            }
            Gloss::Bubble { page, index, at } => {
                let now = ctx.input(|i| i.time);
                self.page_ai.pick(pane, page, index, at, now);
            }
            Gloss::BubbleMenu { page, index, at } => {
                self.page_ai.open_bubble_menu(pane, page, index, at);
            }
            _ => {}
        }
    }

    fn pill_gloss(&mut self, ctx: &Context, out: &mut Commands, pane: PaneId, gloss: Gloss) {
        match gloss {
            Gloss::Pill(crate::page_ai::PillPress::Stop) => self.page_ai.stop_pill(pane),
            Gloss::Pill(crate::page_ai::PillPress::Continue) => {
                let Some(reading) = self.documents.reading(pane) else {
                    return;
                };
                let book = self.library.book_in(&self.documents, pane);
                let name = self.documents.title(pane).unwrap_or_default().to_owned();
                let asked = crate::page_ai::Asked {
                    pane,
                    page: reading.current,
                    book,
                    name: &name,
                    documents: &self.documents,
                    ai: &self.ai,
                    settings: &self.settings.ai,
                };
                let now = ctx.input(|i| i.time);
                let model = self.library.model();
                self.page_ai.resume(asked, model, now, out);
            }

            _ => {}
        }
    }

    fn poll_services(&mut self, ctx: &Context) {
        for (pane, _) in &self.pane_rects {
            if self.documents.sheets(*pane).is_empty() {
                self.study.hold(*pane);
            }
        }
        if let Some(explainer) = self.ai.follow(&self.settings.ai) {
            self.study.set_explainer(explainer);
        }
        self.take_organized();
        self.study.receive(self.library.model_mut());
        let (changed, role) = self.vocab.poll();
        if changed {
            // Answers came: the pages plan their shading again.
            ctx.request_repaint();
        }
        if self.read_aloud.shelf.poll() {
            // A voice model came or went: the pronunciation pack follows.
            self.sounds.voices_changed();
            ctx.request_repaint();
        }
        if self.sounds.poll(ctx.input(|i| i.time)) {
            ctx.request_repaint();
        }
        let memory = self.settings.read_aloud.speech_memory_mb as usize;
        self.read_aloud.shelf.set_speech_memory(memory << 20);
        (self.read_aloud.shelf).set_model_idle(self.settings.read_aloud.model_idle());
        // Whichever card asked takes it; each checks the word is its own.
        if let Some((word, role)) = role {
            self.dict.set_role(&word, role.clone());
            self.study.set_role(&word, role);
        }
    }

    fn show_study(
        &mut self,
        ctx: &Context,
        out: &mut Commands,
    ) -> Vec<(String, Vec<crate::sounds::Line>)> {
        let sounds = self.card_sounds(ctx.input(|i| i.time));
        let looks = (&self.settings.study.gloss, &self.settings.study.highlights);
        let model = self.library.model();
        let others = self.settings.dictionary.other_voices;
        let dict = &self.settings.dictionary;
        let looking = self.dict.card_word().is_some() || dict.window.open;
        if others && (looking || self.route() == Route::Dictionary) {
            self.read_aloud.list_voices(ctx, &self.settings.read_aloud);
        }
        let saying = self.read_aloud.saying();
        let (speakers, offers) = self.read_aloud.speakers(&self.settings.read_aloud, others);
        self.dict.set_voice(crate::dict::Voice {
            saying: saying.map(str::to_owned),
            sounds: sounds.clone(),
            speakers,
            offers,
        });
        let heard = crate::sounds::Heard {
            saying,
            sounds: &sounds,
        };
        let panes = (&self.pane_rects[..], heard);
        self.study.show(ctx, model, panes, looks, out);
        sounds
    }

    fn show_page_ai(
        &mut self,
        ctx: &Context,
        out: &mut Commands,
        sounds: &[(String, Vec<crate::sounds::Line>)],
    ) {
        let gpu = self.documents.gpu();
        self.page_ai
            .receive(ctx, self.library.model_mut(), gpu, out);
        let around = crate::page_ai::Around {
            main: self.library.model(),
            documents: &self.documents,
            ai: &self.ai,
            settings: &self.settings.ai,
        };
        self.page_ai.tick(ctx, &around, out);
        // A book lays its pane's shown results and transcripts into its pages.
        let wanted = crate::transcript::wanted(&self.settings.ai);
        for pane in self.tree.panes() {
            let book = self.library.book_in(&self.documents, pane);
            let name = self.documents.title(pane).unwrap_or_default().to_owned();
            let model = self.library.model();
            let who = (book, name.as_str());
            let ai = self.page_ai.inserts(model, pane, &self.documents, who);
            let own = self.documents.transcripts(pane, &wanted);
            let shown = self.documents.triggered(pane);
            let ai = crate::transcript::combine(ai, own);
            if let Some(inserts) = crate::transcript::combine(ai, shown) {
                self.documents.set_inserts(pane, inserts);
            }
        }
        let model = self.library.model();
        self.page_ai.show(
            ctx,
            &self.settings.ai,
            model,
            (&self.pane_rects, &self.documents),
            out,
        );
        if let Some(toast) = self.page_copy.poll(ctx) {
            self.toasts.push(toast);
        }
        self.dict.set_exact_only(!self.settings.dictionary.near);
        self.dict.poll();
        self.poll_word_list(ctx);
        let services = (&mut self.vocab, &mut self.dict, &self.files);
        self.word_window.show(ctx, &self.settings, services, out);
        let documents = &self.documents;
        let offset = |pane| documents.offset(pane);
        let saying = self.read_aloud.saying();
        let heard = crate::sounds::Heard { saying, sounds };
        self.dict.show(ctx, (&self.pane_rects, heard), offset, out);
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
