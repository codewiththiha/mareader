//! The desktop's file dialogs: what the front end asks, this thread runs.
//!
//! A dialog belongs to the machine, so it lives here and not in the front end:
//! `files` says what it wants in words, and this answers over `rfd`, on a
//! thread that keeps the window drawing while a picker is open. What comes back
//! is a path, which the front end turns into the engine command that asked.

use mareader_ui_egui::files::{Ask, Dialogs, Done, Picked};

/// The desktop's dialogs: one helper thread per pick.
///
/// The front end keeps one picker open at a time, so no thread outlives the
/// answer it was spawned for.
pub struct Shell;

impl Dialogs for Shell {
    fn ask(&self, ask: Ask, done: Done<Picked>) -> bool {
        let name = match &ask {
            Ask::Book { .. } => "mareader-open-dialog",
            Ask::Folder { .. } | Ask::File { .. } | Ask::Books { .. } => "mareader-path-dialog",
            Ask::Save { .. } => "mareader-save-dialog",
        };
        std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || done(pick(ask)))
            .is_ok()
    }
}

/// Runs `ask`'s dialog to its end, blocking this helper thread.
fn pick(ask: Ask) -> Picked {
    let picked = match ask {
        Ask::Book { title, filter } => {
            let dialog = dialog(title, Some(filter.0), Some(filter.1), None);
            pollster::block_on(dialog.pick_file()).map(|file| vec![file])
        }
        Ask::Folder { title } => {
            let dialog = dialog(&title, None, None, None);
            pollster::block_on(dialog.pick_folder()).map(|file| vec![file])
        }
        Ask::File { title, filter } => {
            let dialog = dialog(title, Some(filter.0), Some(filter.1), None);
            pollster::block_on(dialog.pick_file()).map(|file| vec![file])
        }
        Ask::Books {
            title,
            filter,
            from,
        } => {
            let dialog = dialog(title, Some(filter.0), Some(filter.1), from);
            pollster::block_on(dialog.pick_files())
        }
        Ask::Save { title, name, kind } => {
            let dialog = dialog(title, Some(kind.0), Some(&[kind.1]), None).set_file_name(name);
            pollster::block_on(dialog.save_file()).map(|file| vec![file])
        }
    };
    let paths = picked
        .unwrap_or_default()
        .iter()
        .map(|file| file.path().to_path_buf())
        .collect();
    Picked::Paths(paths)
}

/// An `rfd` dialog: its `title`, one filter, and the folder it opens in.
fn dialog(
    title: &str,
    label: Option<&str>,
    extensions: Option<&[&str]>,
    from: Option<std::path::PathBuf>,
) -> rfd::AsyncFileDialog {
    let dialog = rfd::AsyncFileDialog::new().set_title(title);
    let dialog = match (label, extensions) {
        (Some(label), Some(extensions)) => dialog.add_filter(label, extensions),
        _ => dialog,
    };
    match from {
        Some(folder) => dialog.set_directory(folder),
        None => dialog,
    }
}
