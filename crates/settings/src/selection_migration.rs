//! Selection-menu migration for the newly available highlighter.

use serde_json::{Map, Value, json};

pub(crate) fn v9_to_v10(root: &mut Map<String, Value>) {
    let bar = root
        .get_mut("study")
        .and_then(|s| s.get_mut("highlights"))
        .and_then(|h| h.get_mut("bar"))
        .and_then(Value::as_array_mut);
    let Some(bar) = bar else { return };
    let old = json!([
        "copy", "explain", "skills", "look_up", "read_aloud", "note",
        {"highlight": "full"}, {"highlight": "underline"}
    ]);
    if old.as_array().is_some_and(|items| items == bar) {
        bar.push(json!({"highlight": "half"}));
    }
}

#[cfg(test)]
mod tests {
    use crate::{BarItem, Pen, Settings, load, to_json};

    #[test]
    fn only_the_old_default_gets_the_extra_pen() {
        let mut old = Settings {
            version: 9,
            ..Settings::default()
        };
        old.study
            .highlights
            .bar
            .retain(|i| *i != BarItem::Highlight(Pen::Half));
        let upgraded = load(&to_json(&old)).settings;
        assert_eq!(
            upgraded.study.highlights.bar,
            Settings::default().study.highlights.bar
        );
        old.study.highlights.bar.swap(0, 1);
        assert_eq!(
            load(&to_json(&old)).settings.study.highlights.bar,
            old.study.highlights.bar
        );
        let mut current = upgraded;
        current
            .study
            .highlights
            .bar
            .retain(|i| *i != BarItem::Highlight(Pen::Half));
        assert_eq!(
            load(&to_json(&current)).settings.study.highlights.bar,
            current.study.highlights.bar
        );
    }
}
