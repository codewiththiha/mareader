use super::*;
use crate::app::navigation::found_in;

pub(super) fn handles(command: &Command) -> bool {
    matches!(
        command,
        Command::WordList { .. }
            | Command::WordListWindow { .. }
            | Command::Study(..)
            | Command::Explain(..)
            | Command::LookUp(..)
            | Command::Dict(..)
            | Command::Vocab(..)
            | Command::Pronunciation(..)
            | Command::SetDictionary(..)
            | Command::Highlight { .. }
            | Command::Note(..)
            | Command::Bookmark(..)
            | Command::SelectText { .. }
            | Command::FindStep { .. }
            | Command::ShowFind
            | Command::ShowSearch
            | Command::ShowDictionary
    )
}

pub(super) fn apply(app: &mut MareaderApp, ctx: &Context, command: Command) {
    super::two_stage(app, ctx, command, handles_1, apply_1, handles_2, apply_2);
}

fn handles_1(command: &Command) -> bool {
    matches!(
        command,
        Command::WordList { .. }
            | Command::WordListWindow { .. }
            | Command::Study(..)
            | Command::Explain(..)
            | Command::LookUp(..)
            | Command::Dict(..)
            | Command::Vocab(..)
            | Command::Pronunciation(..)
            | Command::SetDictionary(..)
            | Command::Highlight { .. }
            | Command::Note(..)
            | Command::Bookmark(..)
            | Command::SelectText { .. }
    )
}

fn apply_1(app: &mut MareaderApp, ctx: &Context, command: Command) {
    match command {
        Command::WordList { pane, pages } => {
            let list = app.settings.study.words.instant().clone();
            if let Some(ask) = app.word_ask(pane, pages, list) {
                app.word_lists.start(ctx, ask);
            }
        }
        Command::WordListWindow { pane, pages } => {
            let list = app.settings.study.words.last.clone();
            if let Some(ask) = app.word_ask(pane, pages, list) {
                let chapters = !ask.document.outline().is_empty();
                app.word_window.open(ctx, pane, ask, chapters);
            }
        }
        Command::Study(action) => {
            let mut out = Commands::new();
            let model = app.library.model_mut();
            app.study.act(ctx, model, action, &mut out);
            app.queued.extend(out);
        }
        Command::Explain(pane) => app.explain(ctx, pane),
        Command::LookUp(pane) => app.look_up(pane),
        Command::Dict(crate::dict::Action::Window(w)) => app.dict_window(w),
        Command::Dict(crate::dict::Action::Pursue(word)) => app.pursue(&word),
        Command::Dict(action) => app.dict.act(action),
        Command::Vocab(action) => app.vocab.act(action),
        Command::Pronunciation(action) => {
            if let Err(why) = app.sounds.act(action) {
                app.toasts.push(Toast::error(why));
            }
        }
        Command::SetDictionary(d) => {
            app.settings.dictionary = *d;
            app.settings.dictionary.sanitize();
        }
        Command::Highlight { pane, pen, color } => app.highlight(pane, pen, color),
        Command::Note(pane) => app.note(pane),
        Command::Bookmark(pane) => app.bookmark(pane.unwrap_or(app.active)),
        Command::SelectText { pane, page, text } => {
            app.documents.select(pane, page, &text);
        }
        _ => {}
    }
}

fn handles_2(command: &Command) -> bool {
    matches!(
        command,
        Command::FindStep { .. }
            | Command::ShowFind
            | Command::ShowSearch
            | Command::ShowDictionary
    )
}

fn apply_2(app: &mut MareaderApp, ctx: &Context, command: Command) {
    match command {
        // With nothing to search in, find searches the library.
        Command::FindStep { forward } if app.find.is_open() => {
            let found = found_in(&app.search, &app.documents, &app.find);
            app.find.key_step(found, forward, ctx.input(|i| i.time));
        }
        Command::FindStep { .. } => app.apply(ctx, Command::ShowFind),
        Command::ShowFind => match (app.route(), app.documents.reading(app.active)) {
            // The page's own field is its find.
            (Route::Dictionary, _) => app.dict.arrive(),
            (Route::Reader, Some(_)) => {
                app.find.open(app.active);
                app.titlebar.reveal();
            }
            _ => {
                app.go(Route::Library);
                let focus = ui_library::SearchAction::Focus;
                app.apply(ctx, Command::Library(ui_library::Action::Search(focus)));
            }
        },
        Command::ShowSearch if app.find.is_open() => app.find.close(),
        Command::ShowSearch => app.apply(ctx, Command::ShowFind),
        Command::ShowDictionary => match app.route() {
            Route::Dictionary => app.go(app.dict_back()),
            // Over the page, unless Settings ask for the page.
            Route::Reader if app.settings.dictionary.opens == DictionaryOpens::Window => {
                app.dict_window(crate::dict::Window::Toggle);
            }
            _ => app.go(Route::Dictionary),
        },
        _ => {}
    }
}
