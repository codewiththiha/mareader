use super::*;

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
        let overlay = self.settings.chrome.sidebar_overlay;
        let rail = RailContext {
            overlay,
            slide: self.settings.motion.sidebar_slide,
            platform: self.platform,
            bar_bottom: self.titlebar.band_bottom(ctx),
        };
        let tabs = self.tabs();
        if let Some(made) = self.library.take_made() {
            self.shelf_tree.made(self.library.model(), made);
        }
        let organizer = self.library.organizer();
        let paused = organizer
            .paused()
            .map(ui_library::organizing::Paused::short);
        let shelves = PanelContext {
            model: self.library.model(),
            tabs: &tabs,
            active: self.active,
            click: self.settings.workspace.library_click,
            home: self.settings.library.home,
            held: organizer.held(),
            organizer: ui_library::Organizer {
                busy: organizer.busy(),
                paused: paused.as_deref(),
            },
        };
        // Gathered only while the panel shows.
        let notes = match self.sidebar.is_open() && self.sidebar.panel() == Panel::Notes {
            true => {
                let book = self.library.book_in(&self.documents, self.active);
                let name = self.documents.title(self.active).unwrap_or_default();
                let look = &self.settings.study.highlights;
                self.study.notes_of(self.library.model(), book, name, look)
            }
            false => Vec::new(),
        };
        let shelf_tree = &mut self.shelf_tree;
        self.thumbs
            .set_offer(crate::thumbnails::Offer::of(&self.settings));
        let (documents, active) = (&mut self.documents, self.active);
        let (outline, thumbs) = (&mut self.outline, &mut self.thumbs);
        // The rail's panels serve the focused pane's document.
        let mut body =
            |ui: &mut egui::Ui, panel: Panel, recentre: bool, out: &mut Commands| match panel {
                Panel::Outline => documents.reading(active).is_some_and(|r| {
                    outline_panel::show(ui, &r, outline, recentre, out);
                    true
                }),
                Panel::Thumbnails => documents.thumbnails(ui, active, thumbs, recentre, out),
                Panel::Library => library_panel::show(ui, &shelves, shelf_tree, out),
                Panel::Notes => documents.reading(active).is_some_and(|r| {
                    notes_panel::show(ui, &r, &notes, out);
                    true
                }),
            };
        // The rail serves reading; the library keeps it, unseen, for return.
        let inset = match (route.has_rail(), overlay) {
            (false, _) => 0.0,
            (true, true) => self.sidebar.show_floating(ctx, &rail, &mut body, commands),
            (true, false) => self.sidebar.show_docked(ui, &rail, &mut body, commands),
        };
        (inset, overlay)
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
        self.central(ui, ctx, route, &context, commands);
        context.view
    }

    fn page_context(&mut self, ctx: &Context, drop_target: Option<PaneId>) -> PageContext {
        let tokens = theme::Tokens::get(ctx);
        let view = PaneView {
            active: self.active,
            look: self.settings.workspace.look,
            chrome_bottom: self.titlebar.band_bottom(ctx),
            drop_target,
            styles: self.pane_styles(&tokens),
            recent: self.recent_books(),
        };
        // Each pane reads its own paper while it has a colour of its own.
        let every = self.routing().colour();
        let source = self.paper_source();
        let corner = view.corner(self.tree.len() > 1, &tokens);
        let blend = self.settings.paper.blend;
        let pinned_bottom = if self.titlebar.pinned {
            view.chrome_bottom
        } else {
            f32::NEG_INFINITY
        };
        let lit = found_in(&self.search, &self.documents, &self.find).map(|f| Lit {
            hits: f.hits.clone(),
            active: self.find.active(),
        });
        let lit = self.find.pane().zip(lit);
        let cefr = &self.settings.study.cefr;
        let shading = cefr.enabled.then_some(crate::vocab::Look {
            level: cefr.level.band(),
            mark: cefr.mark,
            click: cefr.click_explain,
            hover: self.settings.dictionary.hover,
            ink: self.settings.study.highlights.cefr,
            opacity: self.settings.study.highlights.opacity,
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
        if self.read_aloud_shot
            && let Some(reading) = self.documents.reading(self.active)
        {
            self.read_aloud_shot = false;
            let settings = &self.settings.read_aloud;
            let from = ReadFrom::default();
            (self.read_aloud).start(ctx, &reading, from, settings);
        }
        let shot = std::mem::take(&mut self.transcript_shot);
        for (card, track) in shot {
            if !self.documents.open_transcript(self.active, card, track) {
                self.transcript_shot.push((card, track));
            }
        }
        let shot = std::mem::take(&mut self.trigger_shot);
        for (target, visible) in shot {
            if !self.documents.set_trigger(self.active, target, visible) {
                self.trigger_shot.push((target, visible));
            }
        }
        // A screenshot's clicks wait for the scripts' first pages.
        if let Some(first) = self.script_shot.first()
            && self.documents.pick_script(self.active, first)
        {
            for selector in self.script_shot.split_off(1) {
                self.documents.pick_script(self.active, &selector);
            }
            self.script_shot.clear();
        }
        for toast in self.scripts.tick(ctx, &mut self.documents) {
            self.toasts.push(toast);
        }
        for (doc, clip) in self.documents.take_clips() {
            self.clips.act(&doc, clip, &mut self.read_aloud);
        }
        self.clips.tick();
        self.find_reading();
        let (library, documents) = (&self.library, &self.documents);
        self.read_aloud.remember(|pane| crate::read_aloud::Origin {
            book: library.book_in(documents, pane),
            path: documents.path(pane).map(std::path::Path::to_path_buf),
            name: documents.title(pane).unwrap_or_default().to_owned(),
        });
        self.read_aloud
            .tick(ctx, &mut self.documents, &self.settings.read_aloud);
        self.documents.reveal_read_markers();
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
        let tokens = context.tokens;
        let every = context.every;
        let source = context.source;
        let corner = context.corner;
        let blend = context.blend;
        let pinned_bottom = context.pinned_bottom;
        let lit = &context.lit;
        let read_aloud = &self.read_aloud;
        let documents = &mut self.documents;
        let (layout, texts, window) = (&self.settings.layout, &self.texts, &self.settings.text);
        let (library, library_view) = (&mut self.library, &self.settings.library);
        let study = &mut self.study;
        let page_ai = &mut self.page_ai;
        study.begin_frame();
        let highlights = &self.settings.study.highlights;
        let vocab = &mut self.vocab;
        let shading = context.shading;
        self.library_rect = None;
        let menu_shot = self.menu_shot;
        let lift = self.tree.len() > 1;
        let dictionary_page = (route == Route::Dictionary).then_some(&mut self.dict);
        let dictionary = &self.settings.dictionary;
        let page_top = self.titlebar.page_top(ctx, route.bar());
        egui::CentralPanel::no_frame().show(ui, |ui| {
            if let Some(dict) = dictionary_page {
                self.pane_rects.clear();
                dict.page(ui, dictionary, page_top, commands);
                return;
            }
            if library.showing {
                self.pane_rects.clear();
                self.library_rect = Some(ui.max_rect());
                ui_library::view::show(ui, library, library_view, page_top, commands);
                return;
            }
            let lifted = self.lifting.map(|l| l.pane);
            let tree = &self.tree;
            self.pane_rects = ui_panes::show(ui, tree, lifted, view, commands, |ui, id| {
                let style = view.style(id);
                let cx = ViewContext {
                    active: id == view.active,
                    lift,
                    tokens: style.map_or(tokens, |s| s.tokens),
                    top_inset: (pinned_bottom - ui.max_rect().top()).max(0.0),
                    page: PageLook::new(layout, texts.of(id, window)),
                    paper: style.and_then(|s| s.paper),
                    corner,
                    sample: blend && (every || Some(id) == source),
                    marks: pane_marks(study, library, documents, id, highlights),
                    bookmarks: pane_bookmarks(study, library, documents, id),
                    highlights: highlights.clone(),
                    card: study.card_mark(id),
                    picked: study.picked(id),
                    shading,
                    menu_shot: menu_shot && id == view.active,
                    found: lit
                        .as_ref()
                        .filter(|(p, _)| *p == id)
                        .map(|(_, l)| l.clone()),
                    aloud: read_aloud.lit(id),
                    ai: pane_ai(page_ai, ctx, id, library, documents),
                };
                documents.show(ui, id, &cx, Some(&mut *vocab))
            });
        });
    }
}

fn pane_marks(
    study: &Study,
    library: &LibraryState,
    documents: &Documents,
    pane: PaneId,
    look: &mareader_settings::HighlightSettings,
) -> Arc<[crate::study::marks::Shown]> {
    let book = library.book_in(documents, pane);
    let name = documents.title(pane).unwrap_or_default();
    study.marks_of(library.model(), book, name, look)
}

fn pane_bookmarks(
    study: &Study,
    library: &LibraryState,
    documents: &Documents,
    pane: PaneId,
) -> Vec<usize> {
    bookmarks::on_pages(study, library, documents, pane)
        .into_iter()
        .map(|(_, page)| page)
        .collect()
}

fn pane_ai(
    page_ai: &mut crate::page_ai::PageAi,
    ctx: &Context,
    pane: PaneId,
    library: &LibraryState,
    documents: &Documents,
) -> crate::page_ai::PaneAi {
    let book = library.book_in(documents, pane);
    let name = documents.title(pane).unwrap_or_default();
    page_ai.overlays(ctx, pane, library.model(), documents, (book, name))
}
