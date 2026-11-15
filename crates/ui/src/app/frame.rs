use super::appearance::{back_lights, stand_in_lights};
use super::navigation::found_in;
use super::*;

mod chrome;
mod input;
mod pages;

impl eframe::App for MareaderApp {
    /// The remembered paper from the very first frame, never a dark flash.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        let paper = theme::Tokens::new(&self.settings.appearance).paper;
        paper.to_normalized_gamma_f32()
    }

    fn logic(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        if self.fonts == FontState::Missing {
            theme::install_fonts(ctx);
            egui_extras::install_image_loaders(ctx);
            // Zoom keys zoom the document, not the interface.
            ctx.options_mut(|o| o.zoom_with_keyboard = false);
            // Nothing zooms the interface now: a size saved before goes.
            ctx.set_zoom_factor(1.0);
            self.fonts = FontState::Installed;
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
    fn auto_save_interval(&self) -> Duration {
        SAVE_EVERY
    }

    fn save(&mut self, storage: &mut dyn Storage) {
        self.library.flush();
        storage.set_string(SETTINGS_KEY, mareader_settings::to_json(&self.settings));
        // Nothing open saves `null`: the next launch starts empty.
        let last = serde_json::to_string(&self.last_document());
        storage.set_string(LAST_DOCUMENT_KEY, last.unwrap_or_else(|_| "null".into()));
    }
}
