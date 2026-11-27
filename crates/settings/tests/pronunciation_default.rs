use mareader_settings::{Notation, PronunciationSettings};

#[test]
fn defaults_to_easy_but_keeps_an_explicit_ipa_choice() {
    let fresh: PronunciationSettings = serde_json::from_str("{}").unwrap();
    assert_eq!(fresh.notation, Notation::Easy);
    let saved: PronunciationSettings = serde_json::from_str(r#"{"notation":"ipa"}"#).unwrap();
    assert_eq!(saved.notation, Notation::Ipa);
}
