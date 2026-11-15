use super::*;

pub(super) fn handles(command: &Command) -> bool {
    matches!(
        command,
        Command::ToggleSidebar
            | Command::OpenSidebar(..)
            | Command::CloseSidebar
            | Command::SetSidebarOverlay(..)
            | Command::Split(..)
            | Command::FocusPane(..)
            | Command::CyclePane { .. }
            | Command::ClosePane(..)
            | Command::ShowOpenDialog(..)
            | Command::Zoom(..)
            | Command::View(..)
            | Command::Arrow(..)
            | Command::Page { .. }
            | Command::Chromeless
            | Command::AutoScroll
            | Command::FirstPage
            | Command::LastPage
            | Command::GoTo { .. }
            | Command::CopyPages { .. }
            | Command::Turn { .. }
            | Command::Open(..)
            | Command::SetRatio(..)
            | Command::Window(..)
            | Command::Notify(..)
            | Command::Export { .. }
            | Command::CloseToast(..)
            | Command::ClearToasts
    )
}

pub(super) fn apply(app: &mut MareaderApp, ctx: &Context, command: Command) {
    if handles_1(&command) {
        apply_1(app, ctx, command);
        return;
    }
    if handles_2(&command) {
        apply_2(app, ctx, command);
        return;
    }
    if handles_3(&command) {
        apply_3(app, ctx, command);
    }
}

fn handles_1(command: &Command) -> bool {
    matches!(
        command,
        Command::ToggleSidebar
            | Command::OpenSidebar(..)
            | Command::CloseSidebar
            | Command::SetSidebarOverlay(..)
            | Command::Split(..)
            | Command::FocusPane(..)
            | Command::CyclePane { .. }
            | Command::ClosePane(..)
            | Command::ShowOpenDialog(..)
            | Command::Zoom(..)
    )
}

fn apply_1(app: &mut MareaderApp, ctx: &Context, command: Command) {
    match command {
        Command::ToggleSidebar => app.sidebar.toggle(),
        Command::OpenSidebar(panel) => app.sidebar.open(panel),
        Command::CloseSidebar => app.sidebar.close(),
        Command::SetSidebarOverlay(on) => app.settings.chrome.sidebar_overlay = on,
        Command::Split(axis) => {
            if let Some(id) = app.split(ctx, app.active, axis) {
                app.active = id;
            }
        }
        Command::FocusPane(id) => app.active = id,
        Command::CyclePane { forward } => {
            let panes = app.tree.panes();
            let at = panes.iter().position(|p| *p == app.active).unwrap_or(0);
            let n = panes.len().max(1);
            let tab = egui::Id::new("dict-edge-tab-button");
            let on_tab = ctx.memory(|m| m.has_focus(tab));
            let has_tab = app.settings.workspace.edge_parking
                == mareader_settings::EdgeParking::Tab
                && app.dict.edge_tab_active();
            let visit_tab =
                has_tab && !on_tab && ((forward && at + 1 == n) || (!forward && at == 0));
            if visit_tab {
                ctx.memory_mut(|m| m.request_focus(tab));
                crate::focus::by_key(ctx);
            } else {
                let next = if on_tab {
                    if forward { 0 } else { n - 1 }
                } else {
                    (if forward { at + 1 } else { at + n - 1 }) % n
                };
                if let Some(pane) = panes.get(next) {
                    app.active = *pane;
                }
                ctx.memory_mut(|m| m.focused().map(|id| m.surrender_focus(id)));
            }
        }
        Command::ClosePane(id) => {
            if app.tree.close(id).is_ok() {
                app.documents.close_pane(id);
                app.page_ai.close_pane(id);
                if app.active == id {
                    app.active = app.tree.panes().first().copied().unwrap_or(PaneId(1));
                }
            }
        }
        Command::ShowOpenDialog(placement) => app.files.pick(ctx, placement),
        Command::Zoom(zoom) => {
            let now = ctx.input(|i| i.time);
            app.documents.zoom(app.active, zoom, now);
        }
        _ => {}
    }
}

fn handles_2(command: &Command) -> bool {
    matches!(
        command,
        Command::View(..)
            | Command::Arrow(..)
            | Command::Page { .. }
            | Command::Chromeless
            | Command::AutoScroll
            | Command::FirstPage
            | Command::LastPage
            | Command::GoTo { .. }
            | Command::CopyPages { .. }
            | Command::Turn { .. }
    )
}

fn apply_2(app: &mut MareaderApp, ctx: &Context, command: Command) {
    match command {
        Command::View(mode) => {
            let now = ctx.input(|i| i.time);
            app.documents.set_mode(app.active, mode, now);
        }
        Command::Arrow(arrow) => {
            let now = ctx.input(|i| i.time);
            app.documents.arrow(app.active, arrow, now);
        }
        Command::Page { forward } => app.documents.page_key(app.active, forward),
        Command::Chromeless => {
            app.chromeless = !app.chromeless;
            if app.chromeless {
                app.sidebar.close();
                let keys = keymap::hint(app.platform, &Command::Chromeless);
                let keys = keys.map_or(String::new(), |k| format!(" or {k}"));
                let how = format!("Distraction-free. Press Esc{keys} to leave.");
                app.toasts.push(Toast::info(how));
            }
        }
        Command::AutoScroll => {
            let now = ctx.input(|i| i.time);
            if !app.documents.toggle_drift(app.active, now) {
                let why = "Auto-scroll runs in the scrolling views";
                app.toasts.push(Toast::info(why));
            }
        }
        Command::FirstPage => app.documents.go_to(app.active, 0),
        Command::LastPage => app.documents.go_to(app.active, usize::MAX),
        Command::GoTo { pane, page } => app.documents.go_to(pane, page),
        Command::CopyPages { pane, pages } => {
            if let Some(reading) = app.documents.reading(pane) {
                app.page_copy.start(ctx, reading.document, pages);
            }
        }
        Command::Turn { pane, forward } => app.documents.turn(pane, forward),
        _ => {}
    }
}

fn handles_3(command: &Command) -> bool {
    matches!(
        command,
        Command::Open(..)
            | Command::SetRatio(..)
            | Command::Window(..)
            | Command::Notify(..)
            | Command::Export { .. }
            | Command::CloseToast(..)
            | Command::ClearToasts
    )
}

fn apply_3(app: &mut MareaderApp, ctx: &Context, command: Command) {
    match command {
        Command::Open(request) => {
            // Held while the library loads, to open where it was left.
            let Some(request) = app.library.resume(request) else {
                return;
            };
            app.go(Route::Reader);
            let mut pane = request.pane.unwrap_or(app.active);
            let edge = match request.placement {
                Placement::Into => None,
                Placement::Beside => Some(app.beside(ctx, pane)),
                Placement::Edge(edge) => Some(edge),
            };
            if let Some(edge) = edge {
                match app.split_at(ctx, pane, edge) {
                    Some(new) => pane = new,
                    None => return,
                }
            }
            if app.tree.panes().contains(&pane) {
                app.active = pane;
                if let Some(note) = app.documents.open(ctx, pane, request) {
                    app.toasts.push(note);
                }
            }
        }
        Command::SetRatio(path, ratio) => {
            app.tree.set_ratio(&path, ratio);
        }
        Command::Window(w) => ctx.send_viewport_cmd(match w {
            WindowCommand::Minimize => ViewportCommand::Minimized(true),
            WindowCommand::ToggleMaximize => {
                let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                ViewportCommand::Maximized(!maximized)
            }
            WindowCommand::Close => ViewportCommand::Close,
            WindowCommand::StartDrag => ViewportCommand::StartDrag,
            WindowCommand::ToggleFullscreen => {
                let full = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
                ViewportCommand::Fullscreen(!full)
            }
        }),
        Command::Notify(toast) => app.toasts.push(toast),
        Command::Export {
            name,
            format,
            contents,
        } => app.files.save(ctx, name, format, contents),
        Command::CloseToast(id) => app.toasts.close(id, ctx.input(|i| i.time)),
        Command::ClearToasts => app.toasts.clear(ctx.input(|i| i.time)),
        _ => {}
    }
}
