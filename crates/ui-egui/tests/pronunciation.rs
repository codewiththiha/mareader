//! The pronunciation pack: a download of its own, or a voice model's files.

use std::path::Path;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_engine::command::Command;
use mareader_engine::host::{Fake, Machine};
use mareader_engine::settings::Tab;
use mareader_settings::{Accent, Notation};
use mareader_speech::{MODELS, PRONUNCIATION};
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::ui_command::Sent;

type H = Harness<'static, MareaderApp>;

/// Kokoro's copy of the files, empty but each its listed size.
fn kokoro_holds_them(dir: &Path) {
    for f in PRONUNCIATION.files {
        let path = dir.join("kokoro").join(f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file = std::fs::File::create(path).unwrap();
        file.set_len(f.bytes).unwrap();
    }
}

/// The app over `dir`, the Vocabulary tab open.
fn settings(dir: &Path) -> H {
    reading_with(dir, "Your_system_voice:en-US")
}

/// [`settings`], English said by `voice`, as `Karen:en-AU`.
fn reading_with(dir: &Path, voice: &'static str) -> H {
    let dir = dir.to_owned();
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 1200.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(400)
        .build_eframe(move |cc| {
            let mut app = MareaderApp::on(
                &cc.egui_ctx,
                std::sync::Arc::new(Fake::default()),
                Machine::voices(dir, MODELS),
            );
            app.set_option("animations", "false").ok();
            app.set_option("english_voice", voice).ok();
            app
        });
    h.state_mut()
        .queue(Sent::Engine(Command::OpenSettings(Tab::Vocabulary)));
    h.run_steps(4);
    h
}

/// Steps until `label` shows, as linking finishes on its thread.
fn until(h: &mut H, label: &str) {
    for _ in 0..600 {
        h.run_steps(1);
        if h.query_by_label(label).is_some() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let _ = h.get_by_label(label);
}

fn press(h: &mut H, label: &str) {
    h.get_by_label(label).scroll_to_me();
    h.run_steps(10);
    h.get_by_label(label).click();
    h.run_steps(2);
}

/// Flips the switch named `label`.
fn flip(h: &mut H, label: &str) {
    let switch = egui::accesskit::Role::CheckBox;
    h.get_by_role_and_label(switch, label).scroll_to_me();
    h.run_steps(10);
    h.get_by_role_and_label(switch, label).click();
    h.run_steps(2);
}

#[test]
fn a_voice_model_holding_the_files_makes_the_pack_downloaded() {
    let dir = tempfile::tempdir().unwrap();
    kokoro_holds_them(dir.path());
    let mut h = settings(dir.path());
    until(&mut h, "Shared with Kokoro");
    // Removing it would free nothing: Kokoro reads the same files.
    assert!(h.query_by_label("Remove pronunciation").is_none());
    assert!(h.query_by_label("Download pronunciation").is_none());
    let first = PRONUNCIATION.files.first().unwrap().path;
    let kept = dir.path().join("pronunciation").join(first);
    assert!(kept.exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let inode = |p: &Path| std::fs::metadata(p).unwrap().ino();
        let theirs = dir.path().join("kokoro").join(first);
        assert_eq!(inode(&kept), inode(&theirs), "linked, not copied");
    }
    let switch = egui::accesskit::Role::CheckBox;
    let _ = h.get_by_role_and_label(switch, "Show on word cards");
    flip(&mut h, "Match the reading voice");
    press(&mut h, "British");
    press(&mut h, "Easy to read");
    let p = h.state().settings().study.pronunciation;
    assert_eq!((p.accent, p.notation), (Accent::British, Notation::Easy));
    assert!(p.shown);
}

#[test]
fn the_reading_voice_picks_the_accent_until_told_not_to() {
    let dir = tempfile::tempdir().unwrap();
    kokoro_holds_them(dir.path());
    let mut h = reading_with(dir.path(), "Karen:en-AU");
    until(&mut h, "Shared with Kokoro");
    // Greyed while the voice decides: a press changes nothing.
    press(&mut h, "Both");
    let p = h.state().settings().study.pronunciation;
    assert!(p.follow_voice);
    assert_eq!(p.accent, Accent::Both);
    flip(&mut h, "Match the reading voice");
    press(&mut h, "American");
    let p = h.state().settings().study.pronunciation;
    assert!(!p.follow_voice);
    assert_eq!(p.accent, Accent::American);
}

#[test]
fn without_a_voice_model_the_pack_is_a_download_of_its_own() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = settings(dir.path());
    h.run_steps(4);
    let _ = h.get_by_label("Download pronunciation");
    assert!(h.query_by_label("Show on word cards").is_none());
    assert!(!dir.path().join("pronunciation").exists());
}
