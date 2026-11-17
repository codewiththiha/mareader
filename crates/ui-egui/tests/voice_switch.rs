//! Switching Read Aloud engines keeps Voice and Sound steady on click frames.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use mareader_engine::command::Command;
use mareader_engine::host::{Fake, Machine};
use mareader_engine::settings::Tab;
use mareader_speech::{Build, Gender, Model, ModelFile, ModelVoice};
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::ui_command::Sent;

const NO_SUM: &str = "0000000000000000000000000000000000000000000000000000000000000000";

const fn file(path: &'static str, bytes: u64) -> ModelFile {
    ModelFile {
        path,
        bytes,
        sha256: NO_SUM,
        hosts: None,
        from: None,
    }
}

const CONFIG: &[ModelFile] = &[file("config.json", 4)];

const fn stub_model(
    (id, name, first_voice): (&'static str, &'static str, &'static str),
    builds: &'static [Build],
    voices: &'static [ModelVoice],
) -> Model {
    Model {
        id,
        name,
        about: "Test model",
        license: "MIT",
        hosts: &["http://127.0.0.1:9/"],
        files: CONFIG,
        builds,
        voices,
        first_voice,
        knobs: &[],
    }
}

const RUNNABLE_PAIR: &[Model] = &[
    stub_model(
        ("kokoro", "Kokoro", "ada"),
        &[Build {
            id: "big",
            name: "Big",
            about: "Full",
            file: file("weights.bin", 5),
            extra: None,
        }],
        &[ModelVoice {
            id: "ada",
            name: "Ada",
            language: "en-US",
            gender: Gender::Female,
            file: Some(file("voices/ada.pt", 2)),
            extra: None,
            shared: &[],
            note: "",
        }],
    ),
    stub_model(
        ("kitten", "Kitten", "kit"),
        &[Build {
            id: "mini",
            name: "Mini",
            about: "Small",
            file: file("weights.bin", 5),
            extra: Some(file("voices.npz", 3)),
        }],
        &[ModelVoice {
            id: "kit",
            name: "Kit",
            language: "en-GB",
            gender: Gender::Female,
            file: None,
            extra: None,
            shared: &[],
            note: "",
        }],
    ),
];

#[test]
fn switching_voice_models_keeps_voice_section_steady() {
    let dir = tempfile::tempdir().unwrap();
    for (rel, len) in [
        ("kokoro/config.json", 4),
        ("kokoro/weights.bin", 5),
        ("kokoro/voices/ada.pt", 2),
        ("kitten/config.json", 4),
        ("kitten/weights.bin", 5),
        ("kitten/voices.npz", 3),
    ] {
        let p = dir.path().join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, vec![0; len]).unwrap();
    }
    let root = dir.path().to_path_buf();
    let mut h = Harness::builder()
        .with_size(egui::vec2(960.0, 900.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(400)
        .build_eframe(move |cc| {
            let mut app = MareaderApp::on(
                &cc.egui_ctx,
                std::sync::Arc::new(Fake::default()),
                Machine::voices(root, RUNNABLE_PAIR),
            );
            app.set_option("set", "read_aloud.engine:\"kokoro\"").ok();
            app
        });
    h.state_mut()
        .queue(Sent::Engine(Command::OpenSettings(Tab::ReadAloud)));
    h.run_steps(4);
    let steady = |h: &Harness<'static, MareaderApp>| {
        assert!(h.query_by_label("Language").is_some());
        assert!(h.query_by_label("Voices").is_some());
        let p = h.get_by_role_and_label(Role::Button, "Preview");
        assert!(!p.accesskit_node().is_disabled());
        h.get_by_role_and_label(Role::Slider, "Speed").rect().top()
    };
    let before = steady(&h);
    // On the click frame, Voice must not collapse or disable Preview.
    h.get_by_role_and_label(Role::RadioButton, "Kitten").click();
    h.step();
    assert_eq!(h.state().settings().read_aloud.engine, "kitten");
    assert!((steady(&h) - before).abs() < 1.0);
    h.run_steps(2);
    assert!((steady(&h) - before).abs() < 1.0);
}
