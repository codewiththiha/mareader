use super::*;

impl MareaderApp {
    pub(super) fn reader_overlay(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &Context,
        route: Route,
        inset: f32,
        commands: &mut Commands,
    ) {
        let cache = self.session.library().cache.clone();
        self.ui.library.end_frame(ctx, cache.as_deref());
        self.glosses(ctx, commands);
        if route == Route::Reader {
            let mut stage = ctx.content_rect();
            stage.min.x += inset;
            stage.min.y += crate::theme::TITLE_BAR_H;
            // Above the page bar too, which would cover it.
            stage.max.y -= crate::page_bar::BAR_H;
            let dictionary = self.settings().dictionary.clone();
            let lift = self.toast_lift(ctx, ctx.content_rect());
            let active = self.session.read_aloud().active();
            let toasts = self.ui.toasts.active_bed_for(
                ctx,
                ctx.content_rect(),
                lift,
                self.ui.read_aloud.island_anchor(active),
                self.ui.dict.window_cover(),
            );
            let now = ctx.input(|i| i.time);
            let player = self.ui.read_aloud.clearance_rect(active, now);
            let edge_parking = self.settings().workspace.edge_parking;
            if self.ui.dict.window(
                ctx,
                &mut self.session,
                (&dictionary, edge_parking),
                (stage, toasts, player),
                commands,
            ) {
                self.ui.read_aloud.yield_to_dictionary();
            }
        }
        self.lifted(ctx);
        self.drops_from_rail(ui, commands);
    }

    pub(super) fn bars(
        &mut self,
        ctx: &Context,
        route: Route,
        view: &PaneView,
        commands: &mut Commands,
    ) -> Backdrop {
        ui_library::sheets::show(ctx, self.session.library(), commands);
        let backdrop = self.backdrop(ctx, view);
        self.page_bars(ctx, &backdrop, commands);
        let top = self.ui.titlebar.band_bottom(ctx);
        let docs = self.ui.documents.as_view(self.session.documents());
        let found = found_in(self.session.search(), docs, &self.ui.find);
        self.ui.find.show(ctx, &self.ui.pane_rects, top, found);
        self.player(ctx, route, commands);
        self.drive_search(ctx);
        // Typing, or filtered by what was typed, the field stays in view.
        let searching = route == Route::Library
            && (self.session.library().searching() || ui_library::search::focused(ctx));
        let band = ctx
            .content_rect()
            .with_max_y(self.ui.titlebar.band_bottom(ctx));
        let tabbing = crate::focus::within(ctx, band);
        self.ui.titlebar.hold = self.ui.find.is_open() || searching || tabbing;
        let field = self
            .ui
            .dict
            .page_field()
            .filter(|_| route == Route::Dictionary);
        self.ui.titlebar.quiet = field;
        self.follow_focus(ctx, commands);
        self.ui.documents.end_frame(ctx, &mut self.session);
        backdrop
    }

    pub(super) fn show_titlebar(
        &mut self,
        ctx: &Context,
        route: Route,
        inset: f32,
        overlay: bool,
        backdrop: &Backdrop,
        commands: &mut Commands,
    ) {
        let fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
        let presets = self.settings().all_presets();
        let user_presets = self.settings().user_presets.clone();
        let library_view = self.settings().library;
        let (title, look, page) = match route {
            Route::Reader => {
                let active = self.active();
                let title = self.session.documents().title(active).unwrap_or("Mareader");
                let page = BarPage::Reader(self.reader_bar(overlay));
                (title, self.look(active), page)
            }
            Route::Library => {
                let rect = self.ui.library_rect.unwrap_or_else(|| ctx.content_rect());
                let state = self.session.library();
                let library = LibraryBar {
                    view: &library_view,
                    auto_columns: ui_library::view::page_columns(rect.width()),
                    can_return: self.reading(),
                    trail: state.trail(),
                    renaming: state.renaming(),
                    search: state.field(),
                    organizing: state.busy(),
                    paused: (state.organizer().paused())
                        .map(mareader_engine::library::Paused::short),
                };
                // On a shelf, the breadcrumb names the page.
                let title = if library.trail.is_empty() {
                    "Library"
                } else {
                    ""
                };
                (title, self.settings().appearance, BarPage::Library(library))
            }
            Route::Dictionary => {
                let back = match self.dict_back() {
                    Route::Reader => "Back to reading",
                    _ => "Back to the library",
                };
                let page = BarPage::Dictionary(DictionaryBar { back });
                ("Dictionary", self.settings().appearance, page)
            }
        };
        let bar = BarContext {
            title,
            pinned: self.settings().chrome.pinned(route.bar()),
            platform: self.ui.platform,
            left_inset: inset,
            maximized: ctx.input(|i| i.viewport().maximized.unwrap_or(false)),
            fullscreen,
            // With no rail beside it, the bar hosts the lights.
            gutter: (!overlay || !route.has_rail()) && inset <= 0.0 && !fullscreen,
            appearance: &look,
            appearance_menu: self.settings().chrome.appearance_menu,
            presets: &presets,
            user_presets: &user_presets,
            gpu: self.ui.documents.gpu(),
            backdrop,
            page,
        };
        self.ui.titlebar.show(ctx, &bar, commands);
        if route == Route::Library {
            ui_library::search::suggestions(
                ctx,
                &mut self.session,
                self.ui.library.covers_mut(),
                commands,
            );
        }
    }

    pub(super) fn lights(
        &mut self,
        ctx: &Context,
        route: Route,
        inset: f32,
        overlay: bool,
        commands: &mut Commands,
    ) {
        self.ui.lights = self.place_lights(ctx, overlay, route);
        // Shots and the preview simulate macOS; draw where the lights go.
        let simulated = self.ui.platform == Platform::Mac && !cfg!(target_os = "macos");
        let spots = if simulated {
            Some(LightSpots::STAND_IN)
        } else {
            self.ui.light_spots
        };
        if let Some(spots) = spots.filter(|_| self.ui.platform == Platform::Mac) {
            back_lights(ctx, self.ui.lights, spots, &theme::Tokens::get(ctx));
        }
        if simulated {
            stand_in_lights(ctx, self.ui.lights);
        }
        let mut stage = ctx.content_rect();
        stage.min.x += inset;
        // Runs and toasts rise over the page bar while it shows.
        let lift = match route {
            Route::Reader => self.toast_lift(ctx, stage),
            Route::Library | Route::Dictionary => 0.0,
        };
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("toast-platform"), self.ui.platform));
        let active = self.session.read_aloud().active();
        self.ui.toasts.show_with_player(
            ctx,
            stage,
            lift,
            self.ui.read_aloud.island_anchor(active),
            self.ui.dict.window_cover(),
            commands,
        );
        // A paused run waits on the library page, off the pages read.
        let paused = route == Route::Library;
        // A run outlives the page that started it, so cards show on both.
        ui_library::runs::show(ctx, &mut self.session, (stage, lift), paused, commands);
    }

    pub(super) fn windows(&mut self, ctx: &Context, commands: &mut Commands) {
        let window = self.settings_scope();
        self.voices_shown(ctx);
        let settings = self.settings().clone();
        let voices = crate::settings_window::Voices {
            booth: &self.session.read_aloud().booth,
            shelf: &self.session.read_aloud().shelf,
            platform: self.ui.platform,
            sounds: self.session.sounds(),
            english: self.english_voice(),
        };
        self.ui.settings_window.show(
            ctx,
            &settings,
            &window,
            (
                &self.ui.ai,
                self.session.dict(),
                self.session.vocab(),
                &voices,
            ),
            commands,
        );
        crate::shortcuts::show(ctx, self.ui.platform);
        crate::focus::paint(ctx);
        self.film_grain(ctx);
        self.ui.fade.paint(ctx);
    }

    pub(super) fn finish(&mut self, ctx: &Context, outside: bool, commands: Commands) {
        // Applied after drawing, so the next frame must show the result.
        if !commands.is_empty() {
            ctx.request_repaint();
            // A change still waiting on its snapshot lands first.
            if let Some(after) = self.ui.fade.take() {
                self.restore(after);
            }
            let before = self.held();
            for command in commands {
                self.apply(ctx, command);
            }
            self.reconcile_looks(ctx);
            // A change owes a write; book the frame that carries it.
            if *self.settings() != *before.settings() {
                ctx.request_repaint_after(SAVE_EVERY + Duration::from_millis(50));
            }
            if !outside && self.may_fade(ctx) && self.leapt(&before) {
                let after = self.held();
                self.restore(before);
                self.ui.fade.hold(ctx, after);
            }
        }
        // This frame drew with the old look; show the new one next frame.
        if self.restyle(ctx) {
            ctx.request_repaint();
        }
        self.ui.machine_fonts.after_frame(ctx);
    }
}
