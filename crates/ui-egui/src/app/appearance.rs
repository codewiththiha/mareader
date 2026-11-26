use super::*;

impl MareaderApp {
    /// Current window tokens from settings, plus debug overrides if active.
    pub(super) fn target_tokens(&self) -> theme::Tokens {
        #[allow(unused_mut)]
        let mut tokens = theme::Tokens::new(&self.settings().appearance);
        #[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
        self.ui.live.overlay(&mut tokens);
        tokens
    }

    /// Restyles egui when the look changed or is fading; true if it did.
    pub(super) fn restyle(&mut self, ctx: &Context) -> bool {
        #[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
        if self.ui.live.poll(ctx) {
            self.ui.styled = None;
        }
        let fading = self.ui.fade.advance(ctx);
        let reduced = self.ui.reduced_motion.load(Ordering::Relaxed);
        let want = (
            self.settings().appearance,
            self.settings().motion.clone(),
            reduced,
        );
        if !fading && self.ui.styled.as_ref() == Some(&want) {
            return false;
        }
        let target = self.target_tokens();
        let tokens = self.ui.fade.tokens(&target);
        theme::apply(ctx, &tokens);
        motion::set(ctx, &self.settings().motion, reduced);
        self.ui.styled = (!fading).then_some(want);
        true
    }

    /// What the focused pane and the workspace show: which settings apply.
    pub fn settings_scope(&self) -> Scope {
        let docs = self.documents();
        let reading = docs.reading(self.active());
        let focused = match &reading {
            None => Shown::Nothing,
            Some(r) if r.document.is_reflowable() => Shown::Book {
                stream: r.mode == ViewMode::Vertical,
            },
            Some(_) => Shown::Pdf,
        };
        // An AI page on screen is set in the pane's type, like a book.
        let ai = |pane: PaneId| self.session.page_ai().on_screen(pane);
        let text = |p: PaneId| docs.reflowable(p) || ai(p);
        let book_design = match focused {
            Shown::Book { .. } => self.book_in(self.active()).and_then(|id| {
                let book = self.session.library_model().book(id)?;
                Some((id, book.design.value))
            }),
            _ => None,
        };
        Scope {
            book_design,
            focused,
            split: self.tree().len() > 1,
            any_text: self.tree().panes().into_iter().any(text),
            ai_pages: focused == Shown::Pdf && ai(self.active()),
            sideways: reading.is_some_and(|r| r.mode == ViewMode::Horizontal),
            explains: self.session.study().explains(),
        }
    }

    /// How far toasts rise: over the page bar in the corner while it shows.
    pub(super) fn toast_lift(&self, ctx: &Context, stage: Rect) -> f32 {
        let corner = stage.max - egui::vec2(24.0, 24.0);
        let under = |(id, rect): &(PaneId, Rect)| {
            rect.contains(corner) && self.ui.page_bars.get(id).is_some_and(PageBar::shown)
        };
        let up = !self.ui.chromeless && self.ui.pane_rects.iter().any(under);
        let target = if up { page_bar::BAR_H } else { 0.0 };
        let time = crate::motion::secs(ctx, crate::toast::FADE);
        ctx.animate_value_with_time(egui::Id::new("toast-lift"), target, time)
    }

    /// What the bars show behind them: pane fills, then this frame's pages.
    pub(super) fn backdrop(&mut self, ctx: &Context, view: &PaneView) -> Backdrop {
        let drawn = self.ui.documents.take_drawn();
        if !self.settings().chrome.glass {
            return Backdrop::Solid;
        }
        let Some(gpu) = self.ui.documents.gpu() else {
            return Backdrop::Tint;
        };
        let t = theme::Tokens::get(ctx);
        let fill = |id| view.style(id).map_or(t.paper, |s| s.fill);
        let fills = self
            .ui
            .pane_rects
            .iter()
            .map(|(id, r)| Quad::fill(*r, fill(*id)));
        let mut quads: Vec<Quad> = fills.collect();
        // The library page's paper stands in for the panes.
        quads.extend(self.ui.library_rect.map(|r| Quad::fill(r, t.paper)));
        quads.extend(drawn);
        Backdrop::Glass(Glass {
            pass: gpu.pass.clone(),
            canvas: t.surface,
            quads,
        })
    }

    /// Blend's raw paper: each pane's own, or else shared by every pane.
    pub fn blend_paper(&self, pane: PaneId) -> Option<Rgb> {
        let paper = &self.settings().paper;
        if !paper.blend {
            return None;
        }
        let docs = self.documents();
        if self.routing().colour() {
            return docs.paper(pane, paper.area);
        }
        // The latest focused pane with a paper: a book's has none, as mareader.
        self.recent()
            .iter()
            .find_map(|&p| docs.paper(p, paper.area))
    }

    /// The latest focused pane showing a PDF: shared blend reads its paper.
    pub(super) fn paper_source(&self) -> Option<PaneId> {
        let d = self.session.documents();
        self.recent()
            .iter()
            .copied()
            .find(|&p| d.page_count(p).is_some() && !d.reflowable(p))
    }

    /// Which families split panes own right now.
    pub(super) fn routing(&self) -> Routing {
        self.session.routing()
    }

    pub(super) fn held(&self) -> Held {
        self.session.appearance_snapshot()
    }

    /// Captures the current displayed look across window and panes for a fade.
    pub(super) fn look_snap(&self, ctx: &Context) -> LookSnap {
        let tokens = theme::Tokens::get(ctx);
        let grain = FilmGrain::of(&self.settings().appearance);
        let panes = self
            .pane_styles()
            .into_iter()
            .map(|(id, style)| {
                let snap = PaneSnap {
                    tokens: style.tokens,
                    fill: style.fill,
                    paper: style.paper,
                };
                (id, snap)
            })
            .collect();
        LookSnap {
            tokens,
            grain,
            panes,
        }
    }

    /// Whether the window or a pane jumped to another look since `before`.
    pub(super) fn leapt(&self, before: &Held) -> bool {
        before.leapt(&self.session)
    }

    /// A fade needs motion and no slider held.
    pub(super) fn may_fade(&self, ctx: &Context) -> bool {
        let held = ctx.input(|i| i.pointer.any_down());
        motion::enabled(ctx) && !held
    }

    /// `pane`'s look as drawn: the window's, or its own while split.
    pub fn look(&self, pane: PaneId) -> Appearance {
        self.session.look(pane)
    }

    /// Fits the panes' own text to the panes and the focused one.
    pub(super) fn reconcile_texts(&mut self) {
        self.session.reconcile_texts();
    }

    /// `pane`'s text settings: the window's, or its own while independent.
    pub fn text(&self, pane: PaneId) -> &mareader_settings::TextSettings {
        self.session.text(pane)
    }

    /// Fits the panes' looks to this frame's routing and panes.
    pub(super) fn reconcile_looks(&mut self, ctx: &Context) {
        self.session.reconcile_looks(ctx.input(|i| i.time));
    }

    /// Each pane's theme this frame, and its blend paper.
    pub(super) fn pane_styles(&self) -> Vec<(PaneId, PaneStyle)> {
        let own = self.routing().any();
        let window = self.target_tokens();
        let style = |id: PaneId| {
            let target_tokens = if own {
                theme::Tokens::new(&self.look(id))
            } else {
                window
            };
            let paper = self.blend_paper(id);
            // A book's pane takes its page's paper, as mareader's text panes.
            let plain = match self.session.documents().reflowable(id) {
                true => theme::color(Rgb::from_unit(target_tokens.book.paper)),
                false => target_tokens.paper,
            };
            let target_fill = paper.map_or(plain, |raw| baked(&target_tokens, raw));
            let (tokens, fill, fade) = self.ui.fade.pane(id, &target_tokens, target_fill);
            let style = PaneStyle {
                tokens,
                paper,
                fill,
                own,
                fade,
            };
            (id, style)
        };
        self.tree().panes().into_iter().map(style).collect()
    }

    /// The grain over every layer, menus and windows included, as mareader's.
    pub(super) fn film_grain(&self, ctx: &Context) {
        let Some(gpu) = self.ui.documents.gpu() else {
            return;
        };
        let target = FilmGrain::of(&self.settings().appearance);
        let (old, new) = self.ui.fade.grain(target);
        if old.is_none() && new.is_none() {
            return;
        }
        let painter = crate::z_order::painter(
            ctx,
            crate::z_order::Tier::PostEffect,
            egui::Id::new("film-grain"),
        );
        let rect = ctx.content_rect();
        for grain in [old, new].into_iter().flatten() {
            let offset = crawl_offset(ctx, grain);
            let shape = GrainCallback::window(gpu.grain.clone(), rect, grain, offset);
            painter.add(shape);
        }
    }

    /// The reader's half of the bar: rail toggle, tools and the menu's state.
    pub(super) fn reader_bar(&self, overlay: bool) -> ReaderBar {
        let routing = self.routing();
        let docs = self.documents();
        ReaderBar {
            sidebar_toggle: !self.ui.sidebar.is_open(),
            rail_docked: !overlay && self.ui.sidebar.is_open(),
            independence: routing.split.then_some(Independence {
                themes: routing.themes,
                textures: routing.textures,
            }),
            zoom: docs.zoom_state(self.active()),
            mode: docs.mode(self.active()),
            auto_scroll: docs.drifting(self.active()),
            aloud: (docs.reading(self.active())).map(|_| self.session.read_aloud().active()),
            ink: (docs.reflowable(self.active())).then_some(self.settings().text.ink_contrast),
            text: (docs.reflowable(self.active()))
                .then(|| mareader_engine::appearance_menu::QuickText::of(&self.settings().text)),
            dictionary: {
                let w = &self.settings().dictionary.window;
                w.open && !w.minimized
            },
        }
    }

    /// The rail hosts the lights while drawn; otherwise the bar does.
    pub(super) fn place_lights(&self, ctx: &Context, route: Route) -> NativeLights {
        let rail = self.ui.sidebar.offset().filter(|_| route.has_rail());
        let bar_hosts = rail.is_none();
        // Handing over from a rail, they fade in rather than pop.
        let secs = motion::secs(ctx, titlebar::FADE);
        let handoff = ctx.animate_bool_with_time(egui::Id::new("lights-bar"), bar_hosts, secs);
        match rail {
            Some(shift) => NativeLights {
                shift,
                opacity: 1.0,
            },
            None => NativeLights {
                shift: 0.0,
                opacity: self.ui.titlebar.opacity().min(handoff),
            },
        }
    }
}

/// `raw` paper themed as the pages are, so the backdrop matches them.
fn baked(t: &theme::Tokens, raw: Rgb) -> egui::Color32 {
    theme::color(Rgb::from_unit(t.page.apply(raw.to_unit())))
}

/// The lights' layer: backings, then the stand-ins drawn off a Mac.
fn lights_layer(ctx: &Context) -> egui::Painter {
    crate::z_order::painter(
        ctx,
        crate::z_order::Tier::WindowLights,
        egui::Id::new("lights"),
    )
}

/// A dark disc just inside each glass light on light themes, edging its
/// sheen.
pub(super) fn back_lights(
    ctx: &Context,
    lights: NativeLights,
    spots: LightSpots,
    t: &theme::Tokens,
) {
    if t.dark || !spots.glass || lights.opacity <= 0.0 {
        return;
    }
    let painter = lights_layer(ctx);
    let top = ctx.content_rect().left_top().to_vec2();
    let fill = t.ink.gamma_multiply(0.9 * lights.opacity);
    let radius = (spots.size / 2.0 - 0.5).max(0.0);
    for at in spots.centres(lights.shift) {
        painter.circle_filled(at + top, radius, fill);
    }
}

/// Three dots where AppKit would put its buttons, for checking off a Mac.
pub(super) fn stand_in_lights(ctx: &Context, lights: NativeLights) {
    if lights.opacity <= 0.0 {
        return;
    }
    let painter = lights_layer(ctx);
    let top = ctx.content_rect().left_top().to_vec2();
    let colours = [(255, 95, 87), (254, 188, 46), (40, 200, 64)];
    let spots = LightSpots::STAND_IN;
    for ((r, g, b), at) in colours.into_iter().zip(spots.centres(lights.shift)) {
        let fill = egui::Color32::from_rgb(r, g, b).gamma_multiply(lights.opacity);
        painter.circle_filled(at + top, spots.size / 2.0, fill);
    }
}

/// The looks' own keys: what reads this window's own state.
///
/// `None` for a key that is not one of them, so the next family is asked.
pub(super) fn own_key(app: &mut MareaderApp, key: &str, value: &str) -> Option<Result<(), String>> {
    match key {
        "chromeless" => Some(app.option_chromeless(key, value)),
        "menu" => Some(app.option_menu(key, value)),
        "reduced" => Some(app.option_reduced(key, value)),
        _ => None,
    }
}

impl MareaderApp {
    fn option_chromeless(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.ui.chromeless = value == "true";
        Ok(())
    }

    fn option_menu(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.ui.menu_shot = value == "true";
        Ok(())
    }

    fn option_reduced(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.ui
            .reduced_motion
            .store(value == "true", Ordering::Relaxed);
        Ok(())
    }
}
