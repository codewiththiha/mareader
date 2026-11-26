//! Reading settings saved while the selection menu was resizable.

#[test]
fn old_menu_sizes_are_ignored_without_resetting_other_settings() {
    let saved = r##"{
        "version": 10,
        "study": {
            "selection_menu": {"width": 340, "height": 480},
            "highlights": {"opacity": 0.7}
        },
        "motion": {"icon_animations": false}
    }"##;
    let loaded = mareader_settings::load(saved);
    assert!(loaded.notes.is_empty(), "{:?}", loaded.notes);
    assert!(!loaded.settings.motion.icon_animations);
    assert!((loaded.settings.study.highlights.opacity - 0.7).abs() < 0.001);
    let json = mareader_settings::to_json(&loaded.settings);
    assert!(!json.contains("selection_menu"));
    let old_bad = saved.replace(r#"{"width": 340, "height": 480}"#, "null");
    let bad = mareader_settings::load(&old_bad);
    assert!(bad.notes.is_empty(), "{:?}", bad.notes);
    assert_eq!(
        bad.settings.study.highlights,
        loaded.settings.study.highlights
    );
}
