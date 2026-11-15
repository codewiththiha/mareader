//! The machine's five answers, and the places they imply.

use std::sync::Arc;

use super::*;
use crate::session::Session;

/// A directory that goes when the test does.
fn dir() -> Result<tempfile::TempDir, String> {
    tempfile::tempdir().map_err(|e| e.to_string())
}

#[test]
fn a_machine_takes_its_places_from_its_host() {
    let machine = Machine::under(&Fake::under(Path::new("/here")));

    assert_eq!(machine.library.as_deref(), Some(Path::new("/here/library")));
    assert_eq!(machine.cache.as_deref(), Some(Path::new("/here/cache")));
    assert_eq!(
        machine.dictionaries.as_deref(),
        Some(Path::new("/here/dictionaries"))
    );
    assert_eq!(
        machine.vocabulary.as_deref(),
        Some(Path::new("/here/vocabulary"))
    );
    assert_eq!(
        machine.voices.as_deref(),
        Some(Path::new("/here/voice-models"))
    );
    assert_eq!(machine.ai.as_deref(), Some(Path::new("/here/ai")));
    assert_eq!(machine.ai_helper, None, "a helper is a launch's own");
    assert_eq!(machine.offered, None, "so is a catalogue");
}

#[test]
fn a_host_with_no_folders_names_no_places() {
    let machine = Machine::under(&Fake::default());

    assert_eq!(machine, Machine::default());
}

#[test]
fn a_browser_host_names_a_cache_and_nothing_else() {
    let host = Fake {
        cache: Some(PathBuf::from("/cache")),
        ..Fake::default()
    };
    let machine = Machine::under(&host);

    assert_eq!(machine.cache.as_deref(), Some(Path::new("/cache")));
    assert_eq!(machine.library, None, "no folder, no library");
    assert_eq!(machine.ai, None);
}

#[test]
fn a_fake_keeps_what_the_machine_was_asked_to_do() {
    let host = Arc::new(Fake::default());
    let machine: Shared = host.clone();

    machine.set_clipboard("the page's text".to_owned());
    assert!(machine.reveal(Path::new("/books/a.epub")));
    assert!(machine.open_external(Path::new("/cache/clip.mp4")));

    assert_eq!(host.copied(), ["the page's text".to_owned()]);
    assert_eq!(host.shown(), [PathBuf::from("/books/a.epub")]);
    assert_eq!(host.opened(), [PathBuf::from("/cache/clip.mp4")]);
}

#[test]
fn a_session_keeps_its_files_where_its_host_says() -> Result<(), String> {
    let dir = dir()?;
    let host: Shared = Arc::new(Fake::under(dir.path()));
    let mut session = Session::new(host, Arc::new(|| {}));

    assert!(session.persists(), "the host named a folder");
    session.settle(None);
    assert!(dir.path().join(crate::save::SETTINGS_FILE).is_file());
    assert!(dir.path().join(crate::save::SESSION_FILE).is_file());
    Ok(())
}

#[test]
fn a_session_wires_its_services_from_its_host() -> Result<(), String> {
    let dir = dir()?;
    let host: Shared = Arc::new(Fake::under(dir.path()));
    let session = Session::headless(host);

    assert!(session.persists());
    assert!(session.library().cache.is_some());
    assert!(!session.library().loading());
    #[cfg(not(target_arch = "wasm32"))]
    {
        assert!(session.dict().attached());
        assert!(session.vocab().attached());
        assert!(session.read_aloud().shelf.attached());
    }
    Ok(())
}
