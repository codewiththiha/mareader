//! Read-aloud playback, voice booths, and model shelf owned by [`Session`].

use std::ops::Range;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

use mareader_session::Entry;
use mareader_settings::ReadAloudSettings;
use mareader_speech::Item;
use mareader_workspace::PaneId;

use crate::dict::Download;
use crate::documents::PaneShow;
use crate::read_aloud::ReadFrom;
use crate::read_aloud::parts;
use crate::read_aloud::place::{Panes, Stand};
use crate::read_aloud::player::Act;
use crate::read_aloud::state::{Origin, State};
use crate::reading::Place;
use crate::texts::TextLayers;

use super::Session;

/// Front-end pane driver used while advancing read-aloud inside [`Session`].
pub trait PaneDriver {
    fn go_to(&mut self, pane: PaneId, place: &mut Place, page: usize, count: usize);

    #[allow(clippy::too_many_arguments)]
    fn reveal_said(
        &mut self,
        pane: PaneId,
        place: &mut Place,
        entry: &Entry,
        texts: &TextLayers,
        page: usize,
        chars: Range<usize>,
        clear: f32,
    ) -> bool;
}

impl PaneDriver for () {
    fn go_to(&mut self, _: PaneId, place: &mut Place, page: usize, count: usize) {
        place.go_to(page, count);
    }

    fn reveal_said(
        &mut self,
        _: PaneId,
        _: &mut Place,
        _: &Entry,
        _: &TextLayers,
        _: usize,
        _: Range<usize>,
        _: f32,
    ) -> bool {
        false
    }
}

struct Drive<'a> {
    documents: &'a mut crate::documents::State,
    driver: &'a mut dyn PaneDriver,
}

impl Panes for Drive<'_> {
    fn go_to(&mut self, pane: PaneId, page: usize) {
        let count = self.documents.page_count(pane).unwrap_or(0);
        if let Some(place) = self.documents.place_mut(pane) {
            self.driver.go_to(pane, place, page, count);
        }
    }

    fn reveal_said(&mut self, pane: PaneId, page: usize, chars: Range<usize>, clear: f32) -> bool {
        match self.documents.split_pane(pane) {
            PaneShow::Open {
                entry,
                place,
                texts,
                ..
            } => self
                .driver
                .reveal_said(pane, place, entry, texts, page, chars, clear),
            _ => false,
        }
    }
}

impl Session {
    /// Engine-owned read-aloud state, read-only.
    pub fn read_aloud(&self) -> &State {
        &self.read_aloud
    }

    /// Wires voice models and the pronunciation pack sharing `dir`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn wire_voices(
        &mut self,
        dir: PathBuf,
        catalogue: &'static [mareader_speech::Model],
    ) -> Result<(), String> {
        let waker = Arc::clone(&self.waker);
        let shelf = &mut self.read_aloud.shelf;
        shelf.attach(dir.clone(), catalogue, Arc::clone(&waker))?;
        self.sounds.attach(dir, waker, shelf)?;
        self.sounds.set_inline(self.inline);
        Ok(())
    }

    /// Polls voice model downloads and updates shelf memory/idle limits.
    pub(crate) fn poll_voice_shelf(&mut self) -> bool {
        let changed = self.read_aloud.shelf.poll();
        if changed {
            self.sounds.voices_changed();
        }
        let memory = self.settings.read_aloud.speech_memory_mb as usize;
        let idle = self.settings.read_aloud.model_idle();
        self.read_aloud.shelf.set_speech_memory(memory << 20);
        self.read_aloud.shelf.set_model_idle(idle);
        changed
    }

    /// Keeps the settings voice booth open for `engine`, or closes it.
    pub fn read_aloud_voices_shown(&mut self, engine: Option<&str>) {
        let settings = &self.settings.read_aloud;
        self.read_aloud.voices_shown(engine, settings);
    }

    /// Opens a system engine once to list its voices while none are known.
    pub fn read_aloud_list_voices(&mut self) {
        self.read_aloud.list_voices(&self.settings.read_aloud);
    }

    /// One voice at a time: pauses playback and silences clips and samples.
    pub fn hush(&mut self) {
        self.clips.stop();
        self.read_aloud.hush();
    }

    pub(crate) fn read_aloud_toggle(&mut self) -> bool {
        self.read_aloud.toggle()
    }

    pub(crate) fn read_aloud_stop(&mut self) {
        self.read_aloud.stop();
    }

    /// Starts reading from `stand`; returns whether playback was inactive.
    pub(crate) fn read_aloud_start(&mut self, stand: &Stand, from: ReadFrom) -> bool {
        self.clips.stop();
        let settings = &self.settings.read_aloud;
        self.read_aloud.start(stand, from, settings)
    }

    /// Rehomes playback if the book read aloud is open in some pane again.
    pub(crate) fn read_aloud_find(&mut self) {
        let Some(origin) = self.read_aloud.away() else {
            return;
        };
        let pane = self.tree.panes().into_iter().find(|&p| {
            let own = self.documents.book(p);
            let path = self.documents.path(p);
            let book = own.or_else(|| self.library.model().book_at(path?));
            origin.is(book, path)
        });
        if let Some(stand) = pane.and_then(|p| self.documents.stand(p)) {
            self.read_aloud.rehome(&stand);
        }
    }

    /// Remembers the currently read-aloud pane's book/path/name.
    pub(crate) fn read_aloud_remember(&mut self) {
        let (library, documents) = (&self.library, &self.documents);
        self.read_aloud.remember(|pane| {
            let own = documents.book(pane);
            let path = documents.path(pane);
            Origin {
                book: own.or_else(|| library.model().book_at(path?)),
                path: path.map(std::path::Path::to_path_buf),
                name: documents.title(pane).unwrap_or_default().to_owned(),
            }
        });
    }

    /// Advances playback and word-card booth; drives panes while following.
    pub fn read_aloud_tick(&mut self, now: f64, rise: f32, driver: &mut dyn PaneDriver) {
        let stand = self.read_aloud.pane().and_then(|p| self.documents.stand(p));
        let mut panes = Drive {
            documents: &mut self.documents,
            driver,
        };
        let settings = &self.settings.read_aloud;
        self.read_aloud
            .tick(now, rise, &mut panes, stand.as_ref(), settings);
    }

    /// Brings `chars` on `pane`'s `page` into view using `driver`.
    pub fn reveal_pane_said(
        &mut self,
        pane: PaneId,
        page: usize,
        chars: Range<usize>,
        clear: f32,
        driver: &mut dyn PaneDriver,
    ) -> bool {
        let mut panes = Drive {
            documents: &mut self.documents,
            driver,
        };
        panes.reveal_said(pane, page, chars, clear)
    }

    pub(crate) fn read_aloud_act(&mut self, act: Act) {
        self.read_aloud.act_player(act);
    }

    pub fn read_aloud_answer(
        &mut self,
        read: bool,
        remember: Option<parts::Kind>,
        settings: &mut ReadAloudSettings,
    ) -> bool {
        self.read_aloud.answer_question(read, remember, settings)
    }

    pub(crate) fn try_voice(&mut self) {
        self.hush();
        self.read_aloud.booth.try_voice(&self.settings.read_aloud);
    }

    pub(crate) fn try_voice_of(&mut self, voice: &str) {
        self.hush();
        let settings = &self.settings.read_aloud;
        let (booth, shelf) = (&mut self.read_aloud.booth, &self.read_aloud.shelf);
        booth.try_voice_of(settings, shelf, voice);
    }

    pub(crate) fn stop_voice_try(&mut self) {
        self.read_aloud.booth.stop_trying();
    }

    pub(crate) fn say_word(
        &mut self,
        now: f64,
        word: &str,
        voice: Option<&str>,
    ) -> Result<(), String> {
        self.hush();
        let settings = &self.settings.read_aloud;
        self.read_aloud.say_word(now, settings, (word, voice))
    }

    pub(crate) fn stop_saying(&mut self) {
        self.read_aloud.stop_word();
    }

    pub(crate) fn voice_model(
        &mut self,
        item: Item,
        action: Download,
        settings: &mut ReadAloudSettings,
    ) -> Result<(), String> {
        self.read_aloud.act_model(item, action, settings)
    }
}
