//! Resolving a preferred language without changing the saved preference.

use mareader_dict::LANGUAGES;

/// A usable catalog tag: preference, dictionary default, then English.
/// Confirmed unsupported languages fall back to English, then the first
/// supported catalog entry. Unknown capability leaves the preference alone.
pub fn effective_language(
    preferred: Option<&str>,
    dictionary: Option<&str>,
    speaks: &dyn Fn(&str) -> Option<bool>,
) -> Option<&'static str> {
    let in_catalog = |tag: &str| LANGUAGES.iter().find(|l| l.tag == tag).map(|l| l.tag);
    let wanted = preferred
        .and_then(in_catalog)
        .or_else(|| dictionary.and_then(in_catalog))
        .unwrap_or("en");
    if speaks(wanted) != Some(false) {
        return Some(wanted);
    }
    if speaks("en") == Some(true) {
        return Some("en");
    }
    LANGUAGES
        .iter()
        .find(|l| speaks(l.tag) == Some(true))
        .map(|l| l.tag)
}

impl crate::AiSettings {
    /// Resolves the two dictionary-following language choices for a job
    /// snapshot, without changing the preferences saved in Settings.
    pub fn with_dictionary_language(&self, dictionary: Option<&str>) -> Self {
        let mut resolved = self.clone();
        let unknown = |_: &str| None;
        resolved.tasks.explain.language =
            effective_language(self.tasks.explain.language.as_deref(), dictionary, &unknown)
                .map(str::to_owned);
        resolved.tasks.study.language =
            effective_language(self.tasks.study.language.as_deref(), dictionary, &unknown)
                .map(str::to_owned);
        resolved
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dictionary_followers_are_independent_and_preferences_survive() {
        let mut ai = crate::AiSettings::default();
        ai.tasks.explain.language = Some("my".into());
        let snapshot = ai.with_dictionary_language(Some("fr"));
        assert_eq!(snapshot.tasks.explain.language.as_deref(), Some("my"));
        assert_eq!(snapshot.tasks.study.language.as_deref(), Some("fr"));
        assert_eq!(snapshot.study_language(), Some("French"));
        assert_eq!(ai.tasks.study.language, None);
        let empty = ai.with_dictionary_language(None);
        assert_eq!(empty.tasks.study.language.as_deref(), Some("en"));
        ai.tasks.explain.language = None;
        let snapshot = ai.with_dictionary_language(Some("zz"));
        assert_eq!(snapshot.tasks.explain.language.as_deref(), Some("en"));
        assert_eq!(snapshot.tasks.study.language.as_deref(), Some("en"));
        assert_eq!(ai.tasks.explain.language, None);
    }

    #[test]
    fn follows_dictionary_and_never_discards_the_preference() {
        let unknown = |_: &str| None;
        let english = |tag: &str| Some(tag == "en");
        let japanese = |tag: &str| Some(tag == "ja");
        assert_eq!(effective_language(None, Some("fr"), &unknown), Some("fr"));
        assert_eq!(effective_language(None, None, &unknown), Some("en"));
        assert_eq!(
            effective_language(Some("my"), Some("fr"), &unknown),
            Some("my")
        );
        assert_eq!(
            effective_language(Some("my"), Some("fr"), &english),
            Some("en")
        );
        assert_eq!(
            effective_language(Some("my"), Some("fr"), &japanese),
            Some("ja")
        );
        assert_eq!(effective_language(Some("my"), None, &|_| Some(false)), None);
    }
}
