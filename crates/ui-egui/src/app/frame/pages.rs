use super::*;
use crate::study_mode;

struct PageContext {
    view: PaneView,
    tokens: theme::Tokens,
    every: bool,
    source: Option<PaneId>,
    corner: f32,
    blend: bool,
    pinned_bottom: f32,
    lit: Option<(PaneId, Lit)>,
    shading: Option<crate::vocab::Look>,
}

impl MareaderApp {
    pub(super) fn rail(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &Context,
        route: Route,
        commands: &mut Commands,
    ) -> (f32, bool) {
        let overlay = self.settings().chrome.sidebar_overlay;
        let rail = RailContext {
            overlay,
            slide: self.settings().motion.sidebar_slide,
            platform: self.ui.platform,
            bar_bottom: self.ui.titlebar.band_bottom(ctx),
        };
        let sidebar_drawn =
            route.has_rail() && (self.ui.sidebar.is_open() || self.ui.sidebar.offset().is_some());
        let tabs = if sidebar_drawn && self.ui.sidebar.panel() == Panel::Library {
            self.tabs()
        } else {
            Vec::new()
        };
        if let Some(made) = self.session.library_take_made() {
            self.ui.shelf_tree.made(self.session.library_model(), made);
        }
        let notes = self.rail_notes(sidebar_drawn);
        let active = self.active();
        let ai_notes = self.rail_ai_notes(active);
        self.thumbs_news(active, sidebar_drawn);
        let reading = self.documents().reading(active);
        let thumb_look = PageLook::new(&self.settings().layout, self.session.text(active));
        let click = self.settings().workspace.library_click;
        let home = self.settings().library.home;
        let ui_state = &mut self.ui;
        let inset = self.session.with_rail(|lib_state, texts| {
            let paused = lib_state
                .organizer()
                .paused()
                .map(mareader_engine::library::Paused::short);
            let shelves = PanelContext {
                model: lib_state.model(),
                tabs: &tabs,
                active,
                click,
                home,
                held: lib_state.held(),
                organizer: ui_library::Organizer {
                    busy: lib_state.busy(),
                    paused: paused.as_deref(),
                },
            };
            let documents = &mut ui_state.documents;
            let shelf_tree = &mut ui_state.shelf_tree;
            let (outline, thumbs) = (&mut ui_state.outline, &mut ui_state.thumbs);
            let mut body =
                |ui: &mut egui::Ui, panel: Panel, recentre: bool, out: &mut Commands| match panel {
                    Panel::Outline => reading.as_ref().is_some_and(|r| {
                        outline_panel::show(ui, r, outline, recentre, out);
                        true
                    }),
                    Panel::Thumbnails => {
                        let target = reading.as_ref().map(|r| (r, thumb_look));
                        documents.thumbnails(ui, target, texts, thumbs, recentre, out)
                    }
                    Panel::Library => library_panel::show(ui, &shelves, shelf_tree, out),
                    Panel::Notes => reading.as_ref().is_some_and(|r| {
                        notes_panel::show(ui, r, &notes, &ai_notes, out);
                        true
                    }),
                };
            match (route.has_rail(), overlay) {
                (false, _) => 0.0,
                (true, true) => ui_state
                    .sidebar
                    .show_floating(ctx, &rail, &mut body, commands),
                (true, false) => ui_state.sidebar.show_docked(ui, &rail, &mut body, commands),
            }
        });
        (inset, overlay)
    }

    /// What the thumbnails show of the session: the AI's offer, and the
    /// pages its page AI writes now.
    fn thumbs_news(&mut self, active: PaneId, sidebar_drawn: bool) {
        let offer = crate::thumbnails::Offer::of(self.settings());
        self.ui.thumbs.set_offer(offer);
        if sidebar_drawn && self.ui.sidebar.panel() == Panel::Thumbnails {
            let writing = self.session.page_ai_writing(active);
            self.ui.thumbs.set_writing(writing);
        }
    }

    fn rail_ai_notes(&self, active: PaneId) -> Vec<crate::notes_panel::SelectionRow> {
        let mut ai_notes: Vec<crate::notes_panel::SelectionRow> = self
            .session
            .skills()
            .asks()
            .iter()
            .filter(|a| {
                a.pane == active
                    && self
                        .session
                        .documents()
                        .place(active)
                        .is_some_and(|p| p.doc.get() == a.doc)
            })
            .filter_map(|a| {
                let mareader_engine::skills::Anchor::Text(spans) = &a.anchor else {
                    return None;
                };
                Some(crate::notes_panel::SelectionRow {
                    id: a.id,
                    spot: a.spot,
                    name: a.name.clone(),
                    source: a.source().to_owned(),
                    pinned: a.pinned.is_some(),
                    pin_status: a.pin_status,
                    found: !spans.is_empty(),
                })
            })
            .collect();
        ai_notes.sort_by_key(|a| a.spot.map(|s| crate::notes_panel::order(&s)));
        ai_notes
    }

    fn rail_notes(&self, sidebar_drawn: bool) -> Arc<[crate::notes_panel::Row]> {
        if !(sidebar_drawn && self.ui.sidebar.panel() == Panel::Notes) {
            return Arc::from([]);
        }
        let book = self.book_in(self.active());
        let name = self
            .session
            .documents()
            .title(self.active())
            .unwrap_or_default();
        let look = &self.settings().study.highlights;
        self.ui.study.notes_of(
            self.session.study(),
            self.session.library_model(),
            book,
            name,
            look,
        )
    }

    pub(super) fn pages(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &Context,
        route: Route,
        drop_target: Option<PaneId>,
        commands: &mut Commands,
    ) -> PaneView {
        let context = self.page_context(ctx, drop_target);
        self.shots(ctx);
        match route {
            Route::Space => self.space_page(ui, ctx, commands),
            _ => self.central(ui, ctx, route, &context, commands),
        }
        context.view
    }

    fn page_context(&mut self, ctx: &Context, drop_target: Option<PaneId>) -> PageContext {
        let tokens = theme::Tokens::get(ctx);
        let has_empty = self
            .tree()
            .panes()
            .iter()
            .any(|&p| self.documents().reading(p).is_none());
        let recent = if has_empty {
            self.recent_books()
        } else {
            Vec::new()
        };
        let view = PaneView {
            active: self.active(),
            look: self.settings().workspace.look,
            chrome_bottom: self.ui.titlebar.band_bottom(ctx),
            drop_target,
            styles: self.pane_styles(),
            recent,
        };
        // Each pane reads its own paper while it has a colour of its own.
        let every = self.routing().colour();
        let source = self.paper_source();
        let corner = view.corner(self.tree().len() > 1, &tokens);
        let blend = self.settings().paper.blend;
        let pinned_bottom = if self.settings().chrome.pinned(self.route().bar()) {
            view.chrome_bottom
        } else {
            f32::NEG_INFINITY
        };
        let lit = found_in(self.session.search(), self.documents(), &self.ui.find).map(|f| Lit {
            hits: f.hits.clone(),
            active: self.ui.find.active(),
        });
        let lit = self.ui.find.pane().zip(lit);
        let cefr = &self.settings().study.cefr;
        let shading = cefr.enabled.then_some(crate::vocab::Look {
            level: cefr.level.band(),
            click: cefr.click_explain,
            hover: self.settings().dictionary.hover,
            ink: self.settings().study.highlights.cefr,
            opacity: self.settings().study.highlights.opacity,
        });
        PageContext {
            view,
            tokens,
            every,
            source,
            corner,
            blend,
            pinned_bottom,
            lit,
            shading,
        }
    }

    fn shots(&mut self, ctx: &Context) {
        if self.ui.read_aloud_shot && self.documents().reading(self.active()).is_some() {
            self.ui.read_aloud_shot = false;
            let cmd = Command::ReadAloud {
                pane: self.active(),
                from: ReadFrom::default(),
            };
            self.session.execute(cmd, ctx.input(|i| i.time));
            self.drain(ctx);
        }
        let shot = std::mem::take(&mut self.ui.transcript_shot);
        for (card, track) in shot {
            if !self
                .ui
                .documents
                .open_transcript(self.active(), card, track)
            {
                self.ui.transcript_shot.push((card, track));
            }
        }
        let shot = std::mem::take(&mut self.ui.trigger_shot);
        for (target, visible) in shot {
            if !self
                .ui
                .documents
                .set_trigger(self.active(), target, visible)
            {
                self.ui.trigger_shot.push((target, visible));
            }
        }
        // A screenshot's clicks wait for the scripts' first pages.
        if let Some(first) = self.ui.script_shot.first()
            && self.ui.documents.pick_script(self.active(), first)
        {
            for selector in self.ui.script_shot.split_off(1) {
                self.ui.documents.pick_script(self.active(), &selector);
            }
            self.ui.script_shot.clear();
        }
        let wake = self.session.waker();
        let docs = self.session.documents();
        for toast in self
            .ui
            .scripts
            .tick(ctx, &wake, docs, &mut self.ui.documents)
        {
            self.ui.toasts.push(toast);
        }
        for (doc, clip) in self.ui.documents.take_clips(self.session.documents()) {
            self.session.act_clip(&doc, clip);
        }
        let now = ctx.input(|i| i.time);
        let rise = self.ui.read_aloud.rise();
        if let Some(wait) = self
            .session
            .read_aloud_tick(now, rise, &mut self.ui.documents)
        {
            ctx.request_repaint_after(wait);
        }
        self.ui.documents.reveal_read_markers(&mut self.session);
    }

    fn central(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &Context,
        route: Route,
        context: &PageContext,
        commands: &mut Commands,
    ) {
        let view = &context.view;
        // Every setting is read before any field of `self` is borrowed below.
        let layout = self.settings().layout.clone();
        let (window, library_look) = (self.settings().text.clone(), self.settings().library);
        let highlight_look = self.settings().study.highlights.clone();
        let dictionary_look = self.settings().dictionary.clone();
        let lift = self.tree().len() > 1;
        let tree = self.tree().clone();
        let pane_texts = self.pane_texts(&tree.panes());
        let lifted = self.ui.lifting.map(|l| l.pane);
        let pinned = self.settings().chrome.pinned(route.bar());
        let page_top = self.ui.titlebar.page_top(ctx, route.bar(), pinned);
        let menu_shot = self.ui.menu_shot;
        let (highlights, dictionary) = (&highlight_look, &dictionary_look);
        let documents = &mut self.ui.documents;
        let (library, library_view) = (&mut self.ui.library, &library_look);
        let study = &mut self.ui.study;
        let page_ai = &mut self.ui.page_ai;
        study.begin_frame();
        let host = self.session.host().clone();
        let (layout, session) = (&layout, &mut self.session);
        self.ui.library_rect = None;
        // The dictionary page needs all of `self`: drawn outside the closure.
        if route == Route::Dictionary {
            self.ui.pane_rects.clear();
            self.ui
                .dict
                .page(ui, session, dictionary, page_top, commands);
            return;
        }
        // Copied out above: the closures below need `&mut self`.
        let mut library_rect = None;
        let rects = egui::CentralPanel::no_frame().show(ui, |ui| {
            if route == Route::Library {
                library_rect = Some(ui.max_rect());
                ui_library::view::show(ui, session, library, library_view, page_top, commands);
                return Vec::new();
            }
            ui_panes::show(ui, &tree, lifted, view, commands, |ui, id| {
                let style = view.style(id);
                let pane_text = text_for(&pane_texts, id, &window);
                let marks = pane_marks(session, study, id, highlights);
                let bookmarks = pane_bookmarks(session, study, id);
                let mut ai = page_ai.overlays(ctx, session, id);
                ai.top = ui_panes::clear_top(ui.max_rect(), view, lift, context.tokens.control);
                let cx = ViewContext {
                    active: id == view.active,
                    lift,
                    tokens: style.map_or(context.tokens, |s| s.tokens),
                    top_inset: (context.pinned_bottom - ui.max_rect().top()).max(0.0),
                    page: PageLook::new(layout, pane_text),
                    paper: style.and_then(|s| s.paper),
                    fade: style.and_then(|s| s.fade),
                    corner: context.corner,
                    sample: context.blend && (context.every || Some(id) == context.source),
                    marks,
                    bookmarks,
                    highlights: study_mode::bar(highlights, session.study_mode().on(id)),
                    card: study.card_mark(id),
                    picked: study.picked(id),
                    asking: session.study().asking_marks(),
                    shading: study_mode::shading(session, id, context.shading),
                    settled: study_mode::settled(session, id),
                    menu_shot: menu_shot && id == view.active,
                    found: pane_found(&context.lit, id),
                    aloud: session.read_aloud().lit(id),
                    ai,
                    host: host.clone(),
                    study_span: study_mode::span(session, id),
                };
                let drawn = documents.show(ui, session, id, &cx, true);
                let sheets = documents.as_view(session.documents()).sheets(id);
                let room = (id, ui.max_rect());
                crate::activity::seen::page_runs(ctx, session.page_ai(), room, sheets);
                drawn
            })
        });
        self.ui.pane_rects = rects.inner;
        self.ui.library_rect = library_rect;
    }

    /// Snapshots each pane's text settings before borrowing `session`.
    fn pane_texts(&self, panes: &[PaneId]) -> Vec<(PaneId, TextSettings)> {
        panes
            .iter()
            .map(|&id| (id, self.session.text(id).clone()))
            .collect()
    }
}

fn text_for<'a>(
    panes: &'a [(PaneId, TextSettings)],
    id: PaneId,
    fallback: &'a TextSettings,
) -> &'a TextSettings {
    panes
        .iter()
        .find(|(p, _)| *p == id)
        .map_or(fallback, |(_, t)| t)
}

/// The find bar's hit in this pane, and no other pane's.
fn pane_found(lit: &Option<(PaneId, Lit)>, pane: PaneId) -> Option<Lit> {
    lit.as_ref()
        .filter(|(p, _)| *p == pane)
        .map(|(_, l)| l.clone())
}

fn pane_marks(
    session: &Session,
    study: &StudyUi,
    pane: PaneId,
    look: &mareader_settings::HighlightSettings,
) -> Arc<[crate::study::marks::Shown]> {
    let book = session.book_in(pane);
    let name = session.documents().title(pane).unwrap_or_default();
    study.marks_of(session.study(), session.library_model(), book, name, look)
}

fn pane_bookmarks(session: &Session, study: &StudyUi, pane: PaneId) -> Vec<usize> {
    bookmarks::on_pages(session, study, pane)
        .into_iter()
        .map(|(_, page)| page)
        .collect()
}
