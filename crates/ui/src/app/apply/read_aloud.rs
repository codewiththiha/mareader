use super::*;

pub(super) fn handles(command: &Command) -> bool {
    matches!(
        command,
        Command::PlaceReadMarker { .. }
            | Command::ReadAloud { .. }
            | Command::ToggleReadAloud
            | Command::StopReadAloud
            | Command::TryVoice
            | Command::TryVoiceOf(..)
            | Command::StopVoiceTry
            | Command::SayWord(..)
            | Command::SayWordWith { .. }
            | Command::RevealVoice(..)
            | Command::StopSaying
            | Command::VoiceModel(..)
            | Command::ShowReading
    )
}

pub(super) fn apply(app: &mut MareaderApp, ctx: &Context, command: Command) {
    super::two_stage(app, ctx, command, handles_1, apply_1, handles_2, apply_2);
}

fn handles_1(command: &Command) -> bool {
    matches!(
        command,
        Command::PlaceReadMarker { .. }
            | Command::ReadAloud { .. }
            | Command::ToggleReadAloud
            | Command::StopReadAloud
            | Command::TryVoice
            | Command::TryVoiceOf(..)
            | Command::StopVoiceTry
            | Command::SayWord(..)
            | Command::SayWordWith { .. }
    )
}

fn apply_1(app: &mut MareaderApp, ctx: &Context, command: Command) {
    match command {
        Command::PlaceReadMarker { pane, spot } => {
            let now = ctx.input(|i| i.time);
            app.documents.place_read_marker(pane, spot, now);
            ctx.request_repaint();
        }
        Command::ReadAloud { pane, from } => {
            // Reading from elsewhere sends a waiting pin off.
            app.documents.dismiss_read_markers(ctx.input(|i| i.time));
            if let Some(reading) = app.documents.reading(pane) {
                app.clips.stop();
                let settings = &app.settings.read_aloud;
                app.read_aloud.start(ctx, &reading, from, settings);
            }
        }
        Command::ToggleReadAloud => {
            app.documents.dismiss_read_markers(ctx.input(|i| i.time));
            if !app.read_aloud.toggle()
                && let Some(reading) = app.documents.reading(app.active)
            {
                app.clips.stop();
                let settings = &app.settings.read_aloud;
                let from = ReadFrom::default();
                app.read_aloud.start(ctx, &reading, from, settings);
            }
        }
        Command::StopReadAloud => app.read_aloud.stop(),
        Command::TryVoice => {
            app.hush();
            let settings = &app.settings.read_aloud;
            app.read_aloud.booth.try_voice(settings);
        }
        Command::TryVoiceOf(voice) => {
            app.hush();
            let (booth, shelf) = (&mut app.read_aloud.booth, &app.read_aloud.shelf);
            booth.try_voice_of(ctx, &app.settings.read_aloud, shelf, &voice);
        }
        Command::StopVoiceTry => app.read_aloud.booth.stop_trying(),
        Command::SayWord(word) => {
            app.hush();
            let settings = &app.settings.read_aloud;
            if let Err(why) = app.read_aloud.say_word(ctx, settings, (&word, None)) {
                app.toasts.push(Toast::error(why));
            }
        }
        Command::SayWordWith { word, voice } => {
            app.hush();
            let settings = &app.settings.read_aloud;
            let said = (word.as_str(), Some(voice.as_str()));
            if let Err(why) = app.read_aloud.say_word(ctx, settings, said) {
                app.toasts.push(Toast::error(why));
            }
        }
        _ => {}
    }
}

fn handles_2(command: &Command) -> bool {
    matches!(
        command,
        Command::RevealVoice(..)
            | Command::StopSaying
            | Command::VoiceModel(..)
            | Command::ShowReading
    )
}

fn apply_2(app: &mut MareaderApp, ctx: &Context, command: Command) {
    match command {
        Command::RevealVoice(model, language) => {
            app.open_settings(Some(Tab::Model(model)));
            let fold = crate::settings_window::voice_fold(model, language);
            crate::settings_window::reveal(ctx, fold);
        }
        Command::StopSaying => app.read_aloud.stop_word(),
        Command::VoiceModel(item, action) => {
            let settings = &mut app.settings.read_aloud;
            if let Err(why) = app.read_aloud.act((item, action), settings) {
                app.toasts.push(Toast::error(why));
            }
        }
        Command::ShowReading => {
            if let Some((pane, page, _)) = app.read_aloud.spot() {
                if app.route() != Route::Reader {
                    app.go(Route::Reader);
                }
                app.active = pane;
                app.documents.go_to(pane, page);
            } else if let Some(origin) = app.read_aloud.away().cloned() {
                app.reopen_reading(ctx, origin);
            }
        }
        _ => {}
    }
}
