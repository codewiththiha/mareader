//! Deck candidates from a batch's page text.

use std::collections::{HashMap, HashSet};

use mareader_workspace::PaneId;

use crate::session::Session;

const STOP: &[&str] = &[
    "the", "and", "that", "with", "from", "this", "were", "have", "been", "their", "which",
    "would", "there", "about", "could", "other", "these", "first", "after", "where", "those",
];

pub fn candidates(
    session: &Session,
    pane: PaneId,
    from: usize,
    to: usize,
) -> Vec<(String, String, usize)> {
    let known: HashSet<String> = session.study_memorized_keys().into_iter().collect();
    let mut counts: HashMap<String, (usize, String, usize)> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for page in from..=to {
        let Some(layer) = session.documents().layer(pane, page) else {
            continue;
        };
        let text = layer.text.text(0..layer.text.len());
        for raw in text.split_whitespace() {
            let word = raw
                .trim_matches(|c: char| !c.is_alphabetic())
                .to_lowercase();
            if word.chars().count() < 5 || STOP.contains(&word.as_str()) {
                continue;
            }
            if known.contains(&format!("{word}|other")) {
                continue;
            }
            match counts.get_mut(&word) {
                Some(hit) => hit.0 = hit.0.saturating_add(1),
                None => {
                    order.push(word.clone());
                    counts.insert(word, (1, text.chars().take(220).collect(), page));
                }
            }
        }
    }
    let mut out: Vec<(String, String, usize, usize)> = order
        .into_iter()
        .filter_map(|w| counts.get(&w).map(|(c, s, p)| (w, s.clone(), *p, *c)))
        .collect();
    out.sort_by(|a, b| b.3.cmp(&a.3).then_with(|| a.0.cmp(&b.0)));
    out.into_iter().map(|(w, s, p, _)| (w, s, p)).collect()
}
