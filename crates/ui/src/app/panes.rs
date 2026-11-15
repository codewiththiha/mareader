use super::*;

impl MareaderApp {
    /// Splits `pane`; a refusal is explained in a toast.
    pub(super) fn split(&mut self, ctx: &Context, pane: PaneId, axis: SplitAxis) -> Option<PaneId> {
        let edge = match axis {
            SplitAxis::Horizontal => Edge::Right,
            SplitAxis::Vertical => Edge::Bottom,
        };
        self.split_at(ctx, pane, edge)
    }

    /// Every pane as the rail's Library panel lists it.
    pub(super) fn tabs(&self) -> Vec<PaneTab> {
        // The library's name tells twins apart; the document's title doesn't.
        let name = |pane, book: Option<mareader_library::BookId>| {
            let named = book.and_then(|id| self.library.model().book(id));
            let title = named.map(|b| b.title.value.as_str());
            let title = title.or_else(|| self.documents.title(pane));
            title.unwrap_or("Empty pane").to_owned()
        };
        let panes = self.tree.panes().into_iter();
        let tab = |pane| {
            let book = self.library.book_in(&self.documents, pane);
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
        let floating = self.settings.chrome.sidebar_overlay;
        let rail = (self.sidebar.offset()).filter(|_| floating);
        let edge = rail.map(|o| o + crate::sidebar::WIDTH);
        // Docked, the rail ends where the panes begin.
        let left = self
            .pane_rects
            .iter()
            .map(|(_, r)| r.left())
            .reduce(f32::min);
        let docked = left.filter(|_| self.sidebar.is_open() && !floating);
        let landing = library_panel::Landing {
            panes: &self.pane_rects,
            rail: edge.or(docked),
            full: self.tree.len() >= MAX_PANES,
        };
        let empty = |pane| self.documents.title(pane).is_none();
        let name = |pane| self.documents.title(pane).unwrap_or("this pane").to_owned();
        library_panel::drop_overlay(ui, &landing, empty, name, out);
    }

    /// Opens a library book: shown already, a click focuses its pane.
    pub(super) fn open_book(
        &mut self,
        ctx: &Context,
        id: mareader_library::BookId,
        pane: Option<PaneId>,
        placement: Placement,
    ) {
        // A lost book asks where it went instead of failing.
        if self.library.model().book(id).is_some_and(|b| b.missing()) {
            self.apply(ctx, Command::Library(ui_library::Action::FindAgain(id)));
            return;
        }
        let dropped = pane.is_some();
        let panes = self.tree.panes();
        let showing = panes
            .into_iter()
            .find(|&p| self.library.book_in(&self.documents, p) == Some(id));
        if let (false, Some(showing)) = (dropped, showing) {
            self.active = showing;
            self.go(Route::Reader);
            return;
        }
        // A library book beside an empty pane goes in it, as in mareader.
        let empty = self.documents.title(pane.unwrap_or(self.active)).is_none();
        let placement = match placement {
            Placement::Beside if empty => Placement::Into,
            other => other,
        };
        match self.library.open(id) {
            Ok(request) => {
                let request = OpenRequest {
                    pane,
                    placement,
                    ..request
                };
                self.apply(ctx, Command::Open(request));
            }
            Err(toast) => self.toasts.push(toast),
        }
    }

    /// Right of `pane`, else below it when the right has no room.
    pub(super) fn beside(&self, ctx: &Context, pane: PaneId) -> Edge {
        let mut probe = self.tree.clone();
        let area = Self::area(ctx);
        let (axis, side) = Edge::Right.placement();
        match probe.split(pane, axis, side, area) {
            Err(TreeError::NoRoom(_)) => Edge::Bottom,
            _ => Edge::Right,
        }
    }

    pub(super) fn area(ctx: &Context) -> Bounds {
        let r = ctx.content_rect();
        Bounds::new(r.left(), r.top(), r.width(), r.height())
    }

    /// The lifted pane follows the pointer; letting go moves it there.
    pub(super) fn lifted(&mut self, ctx: &Context) {
        let Some(lift) = self.lifting else {
            return;
        };
        let pane = lift.pane;
        if !self.tree.panes().contains(&pane) || self.tree.len() < 2 {
            self.lifting = None;
            return;
        }
        let title = self.documents.title(pane).unwrap_or("this pane").to_owned();
        let target = match crate::lift::show(ctx, &lift, &self.pane_rects, &title) {
            crate::lift::Lifted::Held => return,
            crate::lift::Lifted::Cancelled => None,
            crate::lift::Lifted::Dropped(target) => Some(target),
        };
        self.lifting = None;
        let moved = match target {
            None => return,
            Some(LiftTarget::Swap(other)) => self.tree.swap(pane, other),
            Some(LiftTarget::Dock(other, edge)) => {
                self.tree.move_to(pane, other, edge, Self::area(ctx))
            }
        };
        if moved.is_err() {
            let note = "There's no room for the pane there.";
            self.toasts.push(Toast::info(note));
        }
        ctx.request_repaint();
    }

    /// A new pane on `edge` of `pane`; a refusal is a note.
    pub(super) fn split_at(&mut self, ctx: &Context, pane: PaneId, edge: Edge) -> Option<PaneId> {
        let area = Self::area(ctx);
        let (axis, side) = edge.placement();
        match self.tree.split(pane, axis, side, area) {
            Ok(id) => Some(id),
            Err(TreeError::Full) => {
                let note = format!("A workspace holds {MAX_PANES} panes at most.");
                self.toasts.push(Toast::info(note));
                None
            }
            Err(_) => {
                let note = "This pane is too small to split.";
                self.toasts.push(Toast::info(note));
                None
            }
        }
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
                let to = self.library.file_target.or(self.library.at());
                let import = ui_library::BookAction::Import { paths, to };
                out.push(Command::Library(ui_library::Action::Book(import)));
            }
            return None;
        }
        let pointer = ctx.pointer_latest_pos();
        let under = pointer
            .and_then(|p| self.pane_rects.iter().find(|(_, r)| r.contains(p)))
            .map_or(self.active, |(id, _)| *id);
        let (hovering, dropped) = ctx.input(|i| {
            let first = i.raw.dropped_files.first().cloned();
            (!i.raw.hovered_files.is_empty(), first)
        });
        if let Some(file) = dropped {
            self.files.dropped(ctx, &file, under);
        }
        hovering.then_some(under)
    }

    /// Every document pane's counter, progress line and bottom bar.
    pub(super) fn page_bars(&mut self, ctx: &Context, backdrop: &Backdrop, out: &mut Commands) {
        let rects = &self.pane_rects;
        self.page_bars
            .retain(|id, _| rects.iter().any(|(p, _)| p == id));
        let label = LabelChrome {
            bar: self
                .titlebar
                .visible()
                .then(|| self.titlebar.band_bottom(ctx)),
        };
        let tokens = theme::Tokens::get(ctx);
        let toasts = self.toasts.covers();
        let rounded = rects.len() > 1 && self.settings.workspace.look.rounded();
        let corner = if rounded { tokens.radius_lg } else { 0.0 };
        // The title bar's glass takes slot 0.
        for (i, (id, rect)) in rects.iter().enumerate() {
            if let Some(reading) = self.documents.reading(*id) {
                // Distraction-free keeps the label, which has its own setting.
                if self.chromeless {
                    floating_label::show(ctx, *rect, &reading, &self.settings.layout, label);
                    continue;
                }
                let look = BarLook {
                    layout: &self.settings.layout,
                    backdrop,
                    slot: i + 1,
                    corner,
                    toasts,
                };
                let kept = bookmarks::on_pages(&self.study, &self.library, &self.documents, *id);
                let marked = kept.iter().any(|(_, p)| *p == reading.current);
                let bar = self.page_bars.entry(*id).or_default();
                page_bar::show(ctx, *rect, &reading, (&look, marked), bar, out);
                floating_label::show(ctx, *rect, &reading, &self.settings.layout, label);
            }
        }
    }
}
