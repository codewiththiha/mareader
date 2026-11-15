use super::*;
use crate::ui_command::UiCommand;

impl MareaderApp {
    /// Every pane as the rail's Library panel lists it.
    pub(super) fn tabs(&self) -> Vec<PaneTab> {
        // The library's name tells twins apart; the document's title doesn't.
        let name = |pane, book: Option<mareader_library::BookId>| {
            let named = book.and_then(|id| self.session.library_model().book(id));
            let title = named.map(|b| b.title.value.as_str());
            let title = title.or_else(|| self.session.documents().title(pane));
            title.unwrap_or("Empty pane").to_owned()
        };
        let panes = self.tree().panes().into_iter();
        let tab = |pane| {
            let book = self.book_in(pane);
            PaneTab {
                pane,
                name: name(pane, book),
                book,
            }
        };
        panes.map(tab).collect()
    }

    /// A book carried from the rail lands where it's let go.
    pub(super) fn drops_from_rail(&self, ui: &egui::Ui, out: &mut Commands) {
        if self.route() != Route::Reader {
            return;
        }
        let floating = self.settings().chrome.sidebar_overlay;
        let rail = (self.ui.sidebar.offset()).filter(|_| floating);
        let edge = rail.map(|o| o + crate::sidebar::WIDTH);
        // Docked, the rail ends where the panes begin.
        let left = self
            .ui
            .pane_rects
            .iter()
            .map(|(_, r)| r.left())
            .reduce(f32::min);
        let docked = left.filter(|_| self.ui.sidebar.is_open() && !floating);
        let landing = library_panel::Landing {
            panes: &self.ui.pane_rects,
            rail: edge.or(docked),
            full: self.tree().len() >= MAX_PANES,
        };
        let empty = |pane| self.session.documents().title(pane).is_none();
        let name = |pane| {
            self.session
                .documents()
                .title(pane)
                .unwrap_or("this pane")
                .to_owned()
        };
        library_panel::drop_overlay(ui, &landing, empty, name, out);
    }

    pub(super) fn area(ctx: &Context) -> Bounds {
        let r = ctx.content_rect();
        Bounds::new(r.left(), r.top(), r.width(), r.height())
    }

    /// The lifted pane follows the pointer; letting go moves it there.
    pub(super) fn lifted(&mut self, ctx: &Context) {
        let Some(lift) = self.ui.lifting else {
            return;
        };
        let pane = lift.pane;
        if !self.tree().panes().contains(&pane) || self.tree().len() < 2 {
            self.ui.lifting = None;
            return;
        }
        let title = self
            .session
            .documents()
            .title(pane)
            .unwrap_or("this pane")
            .to_owned();
        let target = match crate::lift::show(ctx, &lift, &self.ui.pane_rects, &title) {
            crate::lift::Lifted::Held => return,
            crate::lift::Lifted::Cancelled => None,
            crate::lift::Lifted::Dropped(target) => Some(target),
        };
        self.ui.lifting = None;
        let moved = match target {
            None => return,
            Some(LiftTarget::Swap(other)) => Command::SwapPanes { pane, other },
            Some(LiftTarget::Dock(other, edge)) => Command::MovePane {
                pane,
                other,
                edge,
                area: Self::area(ctx),
            },
        };
        self.send(moved);
        ctx.request_repaint();
    }

    /// Drops open under the pointer, returning that pane; the library imports.
    pub(super) fn drops(&self, ctx: &Context, out: &mut Commands) -> Option<PaneId> {
        let desktop = cfg!(not(target_arch = "wasm32"));
        if desktop && self.route() == Route::Library {
            let dropped = ctx.input(|i| i.raw.dropped_files.clone());
            let paths = dropped.iter().map(|f| f.path().to_path_buf());
            let paths: Vec<PathBuf> = paths.collect();
            // Folders ask how they're kept; files are copied in.
            if !paths.is_empty() {
                let to = self.ui.library.file_target.or(self.session.library().at());
                let import = mareader_engine::library::BookAction::Import { paths, to };
                out.push(Sent::Engine(Command::Library(
                    mareader_engine::library::Action::Book(import),
                )));
            }
            return None;
        }
        let pointer = ctx.pointer_latest_pos();
        let under = pointer
            .and_then(|p| self.ui.pane_rects.iter().find(|(_, r)| r.contains(p)))
            .map_or(self.active(), |(id, _)| *id);
        let (hovering, dropped) = ctx.input(|i| {
            let first = i.raw.dropped_files.first().cloned();
            (!i.raw.hovered_files.is_empty(), first)
        });
        if let Some(file) = dropped {
            self.ui.files.dropped(ctx, &file, under);
        }
        hovering.then_some(under)
    }

    /// Every document pane's counter, progress line and bottom bar.
    pub(super) fn page_bars(&mut self, ctx: &Context, backdrop: &Backdrop, out: &mut Commands) {
        let rects = &self.ui.pane_rects;
        self.ui
            .page_bars
            .retain(|id, _| rects.iter().any(|(p, _)| p == id));
        let pinned = self.settings().chrome.pinned(self.route().bar());
        let label = LabelChrome {
            bar: self
                .ui
                .titlebar
                .visible(pinned)
                .then(|| self.ui.titlebar.band_bottom(ctx)),
        };
        let tokens = theme::Tokens::get(ctx);
        let toasts = self.ui.toasts.covers();
        let rounded = rects.len() > 1 && self.settings().workspace.look.rounded();
        let layout = self.settings().layout.clone();
        let corner = if rounded { tokens.radius_lg } else { 0.0 };
        // The title bar's glass takes slot 0.
        for (i, (id, rect)) in rects.iter().enumerate() {
            let reading = self
                .ui
                .documents
                .as_view(self.session.documents())
                .reading(*id);
            if let Some(reading) = reading {
                // Distraction-free keeps the label, which has its own setting.
                if self.ui.chromeless {
                    floating_label::show(ctx, *rect, &reading, &layout, label);
                    continue;
                }
                let look = BarLook {
                    layout: &layout,
                    backdrop,
                    slot: i + 1,
                    corner,
                    toasts,
                };
                let kept = bookmarks::on_pages(&self.session, &self.ui.study, *id);
                let marked = kept.iter().any(|(_, p)| *p == reading.current);
                let bar = self.ui.page_bars.entry(*id).or_default();
                page_bar::show(ctx, *rect, &reading, (&look, marked), bar, out);
                floating_label::show(ctx, *rect, &reading, &layout, label);
            }
        }
    }
}

/// The window's own keys: what reads this window's own state.
///
/// `None` for a key that is not one of them, so the next family is asked.
pub(super) fn own_key(app: &mut MareaderApp, key: &str, value: &str) -> Option<Result<(), String>> {
    match key {
        "platform" => Some(app.option_platform(key, value)),
        "toast" => Some(app.option_toast(key, value)),
        "toasts" => Some(app.option_toasts(key, value)),
        "workers" => Some(app.option_workers(key, value)),
        "sidebar" => Some(app.option_sidebar(key, value)),
        "find" => Some(app.option_find(key, value)),
        "open" => Some(app.option_open(key, value)),
        _ => None,
    }
}

impl MareaderApp {
    fn option_platform(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.ui.platform = Platform::parse(value).ok_or("platform: mac, windows or gnome")?;

        Ok(())
    }

    fn option_toast(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.ui.toasts.push(shot_toast(value));
        Ok(())
    }

    fn option_toasts(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.ui.toasts.set_open(value == "open");
        Ok(())
    }

    fn option_workers(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let n = value
            .parse()
            .map_err(|_| "workers: a count, 0 runs inline")?;
        self.session.set_document_workers(n);
        self.ui.library.set_inline(n == 0);
        self.ui.page_ai.set_inline(n == 0);
        self.session.set_inline(n == 0);

        Ok(())
    }

    fn option_sidebar(&mut self, _key: &str, value: &str) -> Result<(), String> {
        match Panel::parse(value) {
            Some(p) => self.ui.sidebar.open(Some(p)),
            None if value == "closed" => self.ui.sidebar.close(),
            None => return Err("sidebar: thumbnails, outline, library or closed".into()),
        };
        Ok(())
    }

    // The bar's query, landing on its first hit once `open=find`.
    fn option_find(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.ui.find.seed(value);
        Ok(())
    }

    /// The popovers a shot brings up; the engine answers `open`'s own values.
    fn option_open(&mut self, _key: &str, value: &str) -> Result<(), String> {
        match value {
            "appearance" => self.queue(Sent::Ui(UiCommand::ShowAppearance)),
            "settings" => self.queue(Sent::Ui(UiCommand::ShowSettings)),
            "shortcuts" => self.queue(Sent::Ui(UiCommand::ShowShortcuts)),
            "palette" => self.queue(Sent::Ui(UiCommand::ShowPalette)),
            "menu" => self.queue(Sent::Ui(UiCommand::ShowViewMenu)),
            "find" => self.queue(Sent::Ui(UiCommand::ShowFind)),
            _ => {
                return Err(
                    "open: appearance, settings, menu, find, folders, find_again, remove \
                     or rename"
                        .into(),
                );
            }
        };
        Ok(())
    }
}

/// A toast from a shot's words: `_` for spaces, a prefix for its tone.
fn shot_toast(value: &str) -> Toast {
    let words = value.replace('_', " ");
    let (kind, message) = words.split_once(':').unwrap_or(("", &words));
    match kind {
        "error" => Toast::error(message),
        "warning" => Toast::warning(message),
        "player" => Toast::info(message).player(),
        "undo" => Toast::info(message).undo(Command::Study(StudyAction::Restore(Vec::new()))),
        "action" => {
            Toast::info(message).action("Resume", Command::Study(StudyAction::Restore(Vec::new())))
        }
        _ => Toast::info(words.as_str()),
    }
}
