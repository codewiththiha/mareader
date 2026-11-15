use super::*;

pub(super) fn handles(command: &Command) -> bool {
    matches!(
        command,
        Command::SetAppearance(..)
            | Command::SetInkContrast(..)
            | Command::SetQuickText(..)
            | Command::SetIndependence(..)
            | Command::SavePreset { .. }
            | Command::DeletePreset(..)
            | Command::SetTitlebarPinned(..)
            | Command::ShowAppearance
            | Command::ShowViewMenu
            | Command::ShowSettings
            | Command::ShowShortcuts
            | Command::OpenSettings(..)
            | Command::CloseSettings
            | Command::SetSettings(..)
    )
}

pub(super) fn apply(app: &mut MareaderApp, ctx: &Context, command: Command) {
    super::two_stage(app, ctx, command, handles_1, apply_1, handles_2, apply_2);
}

fn handles_1(command: &Command) -> bool {
    matches!(
        command,
        Command::SetAppearance(..)
            | Command::SetInkContrast(..)
            | Command::SetQuickText(..)
            | Command::SetIndependence(..)
            | Command::SavePreset { .. }
            | Command::DeletePreset(..)
            | Command::SetTitlebarPinned(..)
            | Command::ShowAppearance
            | Command::ShowViewMenu
            | Command::ShowSettings
            | Command::ShowShortcuts
            | Command::OpenSettings(..)
            | Command::CloseSettings
    )
}

fn apply_1(app: &mut MareaderApp, ctx: &Context, command: Command) {
    match command {
        // The shelf wears the window's look, the crowned pane's if split.
        Command::SetAppearance(a) if app.route() != Route::Reader => {
            let routing = app.routing();
            let global = &mut app.settings.appearance;
            app.looks.edit_window(global, routing, a);
        }
        Command::SetAppearance(a) => {
            let routing = app.routing();
            let global = &mut app.settings.appearance;
            app.looks.edit(app.active, global, routing, a);
        }
        Command::SetInkContrast(ink) => {
            app.settings.text.ink_contrast = ink;
            app.settings.text.sanitize();
        }
        Command::SetQuickText(q) => q.apply(&mut app.settings.text),
        Command::SetIndependence(i) => {
            app.settings.workspace.independent_themes = i.themes;
            app.settings.workspace.independent_textures = i.textures;
        }
        Command::SavePreset { name, group, look } => {
            app.settings.save_preset(&name, &group, look);
        }
        Command::DeletePreset(id) => app.settings.delete_preset(&id),
        Command::SetTitlebarPinned(p) => {
            app.titlebar.pinned = p;
            app.settings.chrome.pin(app.route().bar(), p);
        }
        Command::ShowAppearance => {
            app.titlebar.reveal();
            egui::Popup::open_id(ctx, egui::Id::new(crate::titlebar::APPEARANCE_POPOVER));
        }
        Command::ShowViewMenu => {
            app.titlebar.reveal();
            let menu = match app.route() {
                Route::Reader => crate::view_menu::VIEW_MENU,
                Route::Library => crate::library::menu::LIBRARY_VIEW_MENU,
                Route::Dictionary => crate::titlebar::DICTIONARY_MENU,
            };
            egui::Popup::open_id(ctx, egui::Id::new(menu));
        }
        Command::ShowSettings => app.open_settings(None),
        Command::ShowShortcuts => crate::shortcuts::toggle(ctx),
        Command::OpenSettings(tab) => app.open_settings(Some(tab)),
        Command::CloseSettings => app.settings_window.close(),
        _ => {}
    }
}

fn handles_2(command: &Command) -> bool {
    matches!(command, Command::SetSettings(..))
}

fn apply_2(app: &mut MareaderApp, _ctx: &Context, command: Command) {
    if let Command::SetSettings(s) = command {
        let mut s = *s;
        s.sanitize();
        app.titlebar.pinned = s.chrome.pinned(app.route().bar());
        app.settings = s;
    }
}
