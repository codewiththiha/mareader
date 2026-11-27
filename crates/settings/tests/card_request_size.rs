use mareader_settings::{AiSettings, Provider};

#[test]
fn request_size_survives_and_old_out_of_range_values_are_clamped() {
    let mut ai = AiSettings::default();
    let first = ai.add_preset(Provider::Ollama, "one");
    let second = ai.add_preset(Provider::Ollama, "two");
    ai.preset_mut(first).unwrap().cards_per_request = 30;
    ai.preset_mut(second).unwrap().cards_per_request = 7;
    let json = serde_json::to_string(&ai).unwrap();
    let mut loaded: AiSettings = serde_json::from_str(&json).unwrap();
    loaded.sanitize();
    assert_eq!(loaded.preset(first).unwrap().cards_per_request, 20);
    assert_eq!(loaded.preset(second).unwrap().cards_per_request, 7);
}
