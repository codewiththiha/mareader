//! Service polling and the effective dictionary language.

use egui::Context;

use super::MareaderApp;

impl MareaderApp {
    pub(super) fn poll_services(&mut self, ctx: &Context) {
        let docs = self.ui.documents.as_view(self.session.documents());
        for (pane, _) in &self.ui.pane_rects {
            if docs.sheets(*pane).is_empty() {
                self.ui.study.hold(*pane);
            }
        }
        let ai_look = self.settings().ai.clone();
        crate::study::offer_skills(ctx, &ai_look.tasks.skills.custom);
        let available = self.session.dict().languages();
        let default = self
            .settings()
            .dictionary
            .default_lang
            .as_deref()
            .filter(|tag| available.iter().any(|lang| lang == tag))
            .map(str::to_owned);
        if let Some(answerers) = self.ui.ai.follow(&ai_look, default.as_deref()) {
            self.session.study_set_ai(answerers);
        }
        #[cfg(target_os = "macos")]
        self.ui.ai.probe_translation(&ai_look);
        #[cfg(not(target_arch = "wasm32"))]
        let apple = self.ui.ai.board().and_then(|b| match b.apple.ready() {
            Some(mareader_assist::AppleState::Ready(languages)) => Some(languages.as_slice()),
            _ => None,
        });
        #[cfg(target_arch = "wasm32")]
        let apple: Option<&[String]> = None;
        let dict = self.settings().dictionary.default_lang.as_deref();
        let packs = self.session.dict().pack_languages();
        let dictionary = dict.filter(|tag| packs.contains(tag));
        crate::language_menu::offer(
            ctx,
            crate::language_menu::choices(&ai_look, apple, dictionary),
        );
        self.take_organized();
        #[cfg(not(target_arch = "wasm32"))]
        for toast in self.ui.ai.waiting() {
            self.ui.toasts.push(toast);
        }
        self.tick_session(ctx);
    }
}
