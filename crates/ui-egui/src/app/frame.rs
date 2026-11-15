use super::appearance::{back_lights, stand_in_lights};
use super::navigation::found_in;
use super::*;

mod chrome;
mod input;
mod pages;

impl eframe::App for MareaderApp {
    /// The remembered paper from the very first frame, never a dark flash.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        let paper = theme::Tokens::new(&self.settings().appearance).paper;
        paper.to_normalized_gamma_f32()
    }

    fn logic(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        if self.ui.fonts == FontState::Missing {
            theme::install_fonts(ctx);
            egui_extras::install_image_loaders(ctx);
            // Zoom keys zoom the document, not the interface.
            ctx.options_mut(|o| o.zoom_with_keyboard = false);
            // Nothing zooms the interface now: a size saved before goes.
            ctx.set_zoom_factor(1.0);
            self.ui.fonts = FontState::Installed;
            self.ui.wake.bind(ctx);
        }
        self.restyle(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if !self.prepare(ui, &ctx, frame) {
            return;
        }
        let (mut commands, route, outside) = self.commands(&ctx);
        let drop_target = self.escape(&ctx, route, &mut commands);
        let (inset, overlay) = self.rail(ui, &ctx, route, &mut commands);
        let view = self.pages(ui, &ctx, route, drop_target, &mut commands);
        self.reader_overlay(ui, &ctx, route, inset, &mut commands);
        let backdrop = self.bars(&ctx, route, &view, &mut commands);
        self.show_titlebar(&ctx, route, inset, overlay, &backdrop, &mut commands);
        self.lights(&ctx, route, inset, overlay, &mut commands);
        self.windows(&ctx, &mut commands);
        self.finish(&ctx, outside, commands);
    }

    /// Writes happen on eframe's persist thread, and only when changed.
    #[cfg(target_arch = "wasm32")]
    fn auto_save_interval(&self) -> Duration {
        SAVE_EVERY
    }

    /// The engine saves itself; a page keeps the two files it hands over.
    fn save(&mut self, _storage: &mut crate::import::Store<'_>) {
        self.session.flush();
        crate::import::keep(self);
    }

    /// Writes what is owed before the window goes.
    fn on_exit(&mut self) {
        self.flush();
    }
}

impl MareaderApp {
    /// Advances engine background jobs and drains their events.
    pub(super) fn tick_session(&mut self, ctx: &Context) {
        self.session.set_area(Self::area(ctx));
        let tick = self.session.tick(ctx.input(|i| i.time));
        if tick.repaint {
            ctx.request_repaint();
        }
        if let Some(wait) = tick.wait {
            ctx.request_repaint_after(Duration::from_secs_f64(wait));
        }
        self.drain(ctx);
    }

    /// Writes the engine's files when due, and books the frame that pays.
    fn keep(&mut self, ctx: &Context, now: f64) {
        // A launch that writes somewhere needs to know what was read.
        let last = if self.session.persists() || crate::import::keeps() {
            self.last_document()
        } else {
            None
        };
        if let Some(wait) = self.session.keep(now, last) {
            ctx.request_repaint_after(Duration::from_secs_f64(wait));
        }
        self.drain(ctx);
    }

    /// Writes everything owed: the library's rows and the engine's files.
    pub fn flush(&mut self) {
        let last = self.last_document();
        self.session.settle(last);
    }
}
