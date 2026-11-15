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
        self.library.end_frame(ctx);
        self.glosses(ctx, commands);
        if route == Route::Reader {
            let mut stage = ctx.content_rect();
            stage.min.x += inset;
            stage.min.y += crate::theme::TITLE_BAR_H;
            // Above the page bar too, which would cover it.
            stage.max.y -= crate::page_bar::BAR_H;
            let dictionary = &self.settings.dictionary;
            let lift = self.toast_lift(ctx, ctx.content_rect());
            let toasts = self.toasts.active_bed_for(
                ctx,
                ctx.content_rect(),
                lift,
                self.read_aloud.island_anchor(),
                self.dict.window_cover(),
            );
            let now = ctx.input(|i| i.time);
            let player = self.read_aloud.clearance_rect(now);
            if self.dict.window(
                ctx,
                (dictionary, self.settings.workspace.edge_parking),
                stage,
                toasts,
                player,
                commands,
            ) {
                self.read_aloud.yield_to_dictionary();
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
        ui_library::sheets::show(ctx, &self.library, commands);
        let backdrop = self.backdrop(ctx, view);
        self.page_bars(ctx, &backdrop, commands);
        let top = self.titlebar.band_bottom(ctx);
        let found = found_in(&self.search, &self.documents, &self.find);
        self.find.show(ctx, &self.pane_rects, top, found);
        self.player(ctx, route, commands);
        self.drive_search(ctx);
        // Typing, or filtered by what was typed, the field stays in view.
        let searching = route == Route::Library
            && (self.library.searching() || ui_library::search::focused(ctx));
        let band = ctx
            .content_rect()
            .with_max_y(self.titlebar.band_bottom(ctx));
        let tabbing = crate::focus::within(ctx, band);
        self.titlebar.hold = self.find.is_open() || searching || tabbing;
        let field = self
            .dict
            .page_field()
            .filter(|_| route == Route::Dictionary);
        self.titlebar.quiet = field;
        self.follow_focus(ctx, commands);
        self.documents.end_frame(ctx);
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
        let presets = self.settings.all_presets();
        let (title, look, page) = match route {
            Route::Reader => (
                self.documents.title(self.active).unwrap_or("Mareader"),
                self.look(self.active),
                BarPage::Reader(self.reader_bar(overlay)),
            ),
            Route::Library => {
                let width = self
                    .library_rect
                    .map_or(ctx.content_rect().width(), |r| r.width());
                let library = LibraryBar {
                    view: &self.settings.library,
                    auto_columns: ui_library::view::page_columns(width),
                    can_return: self.reading(),
                    trail: self.library.trail(),
                    renaming: self.library.renaming(),
                    search: self.library.field(),
                    organizing: self.library.organizer().busy(),
                    paused: (self.library.organizer().paused())
                        .map(ui_library::organizing::Paused::short),
                };
                // On a shelf, the breadcrumb names the page.
                let title = if library.trail.is_empty() {
                    "Library"
                } else {
                    ""
                };
                (title, self.settings.appearance, BarPage::Library(library))
            }
            Route::Dictionary => {
                let back = match self.dict_back() {
                    Route::Reader => "Back to reading",
                    _ => "Back to the library",
                };
                let page = BarPage::Dictionary(DictionaryBar { back });
                ("Dictionary", self.settings.appearance, page)
            }
        };
        let bar = BarContext {
            title,
            platform: self.platform,
            left_inset: inset,
            maximized: ctx.input(|i| i.viewport().maximized.unwrap_or(false)),
            fullscreen,
            // With no rail beside it, the bar hosts the lights.
            gutter: (!overlay || !route.has_rail()) && inset <= 0.0 && !fullscreen,
            appearance: &look,
            appearance_menu: self.settings.chrome.appearance_menu,
            presets: &presets,
            user_presets: &self.settings.user_presets,
            gpu: self.documents.gpu(),
            backdrop,
            page,
        };
        self.titlebar.show(ctx, &bar, commands);
        if route == Route::Library {
            ui_library::search::suggestions(ctx, &mut self.library, commands);
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
        self.lights = self.place_lights(ctx, overlay, route);
        // Shots and the preview simulate macOS; draw where the lights go.
        let simulated = self.platform == Platform::Mac && !cfg!(target_os = "macos");
        let spots = if simulated {
            Some(LightSpots::STAND_IN)
        } else {
            self.light_spots
        };
        if let Some(spots) = spots.filter(|_| self.platform == Platform::Mac) {
            back_lights(ctx, self.lights, spots, &theme::Tokens::get(ctx));
        }
        if simulated {
            stand_in_lights(ctx, self.lights);
        }
        let mut stage = ctx.content_rect();
        stage.min.x += inset;
        // Runs and toasts rise over the page bar while it shows.
        let lift = match route {
            Route::Reader => self.toast_lift(ctx, stage),
            Route::Library | Route::Dictionary => 0.0,
        };
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("toast-platform"), self.platform));
        self.toasts.show_with_player(
            ctx,
            stage,
            lift,
            self.read_aloud.island_anchor(),
            self.dict.window_cover(),
            commands,
        );
        // A paused run waits on the library page, off the pages read.
        let paused = route == Route::Library;
        let runs = &mut self.library.runs;
        // A run outlives the page that started it, so cards show on both.
        ui_library::runs::show(ctx, runs, (stage, lift), paused, commands);
    }

    pub(super) fn windows(&mut self, ctx: &Context, commands: &mut Commands) {
        let window = self.settings_scope();
        self.voices_shown(ctx);
        let voices = crate::settings_window::Voices {
            booth: &self.read_aloud.booth,
            shelf: &self.read_aloud.shelf,
            platform: self.platform,
            sounds: &self.sounds,
            english: self.english_voice(),
        };
        self.settings_window.show(
            ctx,
            &self.settings,
            &window,
            (&self.ai, &self.dict, &self.vocab, &voices),
            commands,
        );
        crate::shortcuts::show(ctx, self.platform);
        crate::focus::paint(ctx);
        self.film_grain(ctx);
        self.fade.paint(ctx);
    }

    pub(super) fn finish(&mut self, ctx: &Context, outside: bool, commands: Commands) {
        // Applied after drawing, so the next frame must show the result.
        if !commands.is_empty() {
            ctx.request_repaint();
            // A change still waiting on its snapshot lands first.
            if let Some(after) = self.fade.take() {
                self.restore(after);
            }
            let before = self.held();
            for command in commands {
                self.apply(ctx, command);
            }
            self.reconcile_looks(ctx);
            // Book a save; eframe autosaves only on a frame.
            if self.settings != before.settings {
                ctx.request_repaint_after(SAVE_EVERY + Duration::from_millis(50));
            }
            if !outside && self.may_fade(ctx) && self.leapt(&before) {
                let after = self.held();
                self.restore(before);
                self.fade.hold(ctx, after);
            }
        }
        // This frame drew with the old look; show the new one next frame.
        if self.restyle(ctx) {
            ctx.request_repaint();
        }
        self.machine_fonts.after_frame(ctx);
    }
}
