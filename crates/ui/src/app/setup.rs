use super::*;

impl MareaderApp {
    /// Reopens what was read at exit, if its file is still there.
    pub fn resume(&mut self) -> bool {
        let Some(last) = self.last.take().filter(|l| l.path.is_file()) else {
            return false;
        };
        let place = last.place.map(|(block, offset)| Anchor { block, offset });
        let spine = last.spine.map(|place| mareader_session::SpinePlace {
            href: place.href,
            item: place.item,
            block: place.block,
            offset: place.offset,
        });
        let request = OpenRequest {
            page: last.page,
            place,
            spine,
            pages: last.pages,
            book: last.book,
            ..OpenRequest::path(last.path)
        };
        self.queue(Command::Open(request));
        true
    }

    /// Keeps the library in `root`; without one it lives in memory.
    pub fn attach_library(&mut self, root: PathBuf) {
        self.library.attach(root);
    }

    /// Starts the AI service, its keys kept in `root`; `helper` as in `Ai`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn attach_ai(&mut self, ctx: &Context, root: PathBuf, helper: Option<PathBuf>) {
        self.ai.attach(ctx, root, helper);
        // A run stopped before quitting waits on its card.
        self.library.set_paused(self.ai.paused());
    }

    /// Keeps language packs in `dir`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn attach_dictionaries(&mut self, ctx: &Context, dir: PathBuf) {
        let ctx = ctx.clone();
        let waker = Arc::new(move || ctx.request_repaint());
        if let Err(e) = self.dict.attach(dir, waker) {
            self.toasts
                .push(Toast::info(format!("Dictionaries are off: {e}")));
        }
    }

    /// Keeps the word-level dataset in `dir`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn attach_vocabulary(&mut self, ctx: &Context, dir: PathBuf) {
        let ctx = ctx.clone();
        let waker = Arc::new(move || ctx.request_repaint());
        if let Err(e) = self.vocab.attach(dir, waker) {
            self.toasts
                .push(Toast::info(format!("Word levels are off: {e}")));
        }
    }

    /// Keeps downloaded voice models in `dir`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn attach_voice_models(&mut self, ctx: &Context, dir: PathBuf) {
        self.attach_voice_models_from(ctx, dir, mareader_speech::MODELS);
    }

    /// As [`Self::attach_voice_models`], offering `catalogue` instead.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn attach_voice_models_from(
        &mut self,
        ctx: &Context,
        dir: PathBuf,
        catalogue: &'static [mareader_speech::Model],
    ) {
        let ctx = ctx.clone();
        let waker = Arc::new(move || ctx.request_repaint());
        if let Err(e) = self.attach_voices(dir, catalogue, waker) {
            self.toasts
                .push(Toast::info(format!("Voice models are off: {e}")));
        }
    }

    /// Voice models and the pronunciation pack, sharing files in `dir`.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn attach_voices(
        &mut self,
        dir: PathBuf,
        catalogue: &'static [mareader_speech::Model],
        waker: mareader_speech::Waker,
    ) -> Result<(), String> {
        let shelf = &mut self.read_aloud.shelf;
        shelf.attach(dir.clone(), catalogue, waker.clone())?;
        self.sounds.attach(dir, waker, shelf)
    }

    /// Keeps covers and other rebuildable files in `dir`.
    pub fn attach_cache(&mut self, dir: PathBuf) {
        self.library.attach_cache(dir);
    }
}
