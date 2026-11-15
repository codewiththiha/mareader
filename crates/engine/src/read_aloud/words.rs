//! Which words of the sentence said are lit: a few, a phrase, or all.

use std::ops::Range;
use std::time::{Duration, Instant};

use mareader_settings::Highlight;
use mareader_speech::Word;

/// Words in a phrase left whole; a longer run is cut where speech parts.
const PHRASE_MOST: usize = 6;
/// Words a cut phrase may run to, when that keeps words together.
const PHRASE_LONGEST: usize = 8;
/// Words that open a clause: a phrase starts well on one.
const CLAUSES: &[&str] = &[
    "and", "or", "but", "nor", "so", "yet", "because", "while", "when", "where", "whereas",
    "which", "who", "whom", "whose", "that", "if", "unless", "until", "since", "although",
    "though",
];
/// Words that open a phrase, prepositions mostly.
const OPENERS: &[&str] = &[
    "in", "on", "at", "to", "for", "from", "with", "by", "into", "about", "like", "than", "after",
    "before", "without", "through", "during", "as", "past", "toward", "towards", "across",
    "around", "between", "among", "against", "beyond", "within", "along", "behind", "despite",
    "except", "via",
];
/// Words that lean on the next: a phrase ends badly on one.
const LEANERS: &[&str] = &[
    "a", "an", "the", "of", "upon", "onto", "over", "under", "is", "are", "was", "were", "be",
    "been", "am", "has", "have", "had", "do", "does", "did", "will", "would", "can", "could",
    "shall", "should", "may", "might", "must", "my", "your", "his", "her", "its", "our", "their",
    "this", "these", "those", "not", "very",
];
/// Bytes said a second at normal speed, before a sentence is timed.
const PACE: f64 = 14.0;
/// Marks a voice pauses after, ending a phrase.
const PAUSES: [char; 13] = [
    ',', ';', ':', '—', '–', '.', '!', '?', '…', ')', '"', '”', '»',
];

/// The words lit while word `at` of `words` is heard.
pub fn group(
    text: &str,
    words: &[Word],
    at: usize,
    (mode, count): (Highlight, u8),
) -> Range<usize> {
    let len = words.len();
    let at = at.min(len.saturating_sub(1));
    match mode {
        Highlight::Sentence => 0..len,
        Highlight::Words => {
            let n = usize::from(count.max(1));
            let from = at / n * n;
            from..(from + n).min(len)
        }
        Highlight::Phrases => (phrases(text, words).into_iter())
            .find(|r| r.contains(&at))
            .unwrap_or(0..len),
    }
}

/// `words` cut where the voice pauses, long runs where speech parts.
fn phrases(text: &str, words: &[Word]) -> Vec<Range<usize>> {
    let said = |w: &Word| text.get(w.bytes.clone()).unwrap_or_default();
    let mut out = Vec::new();
    let mut from = 0;
    for (i, word) in words.iter().enumerate() {
        if said(word).ends_with(PAUSES) || i + 1 == words.len() {
            let run: Vec<&str> = words
                .get(from..=i)
                .unwrap_or_default()
                .iter()
                .map(said)
                .collect();
            out.extend(
                natural(&run)
                    .into_iter()
                    .map(|r| r.start + from..r.end + from),
            );
            from = i + 1;
        }
    }
    out
}

/// A run of words cut into phrases as speech parts them.
fn natural(run: &[&str]) -> Vec<Range<usize>> {
    let len = run.len();
    if len <= PHRASE_MOST {
        return std::iter::once(0..len).collect();
    }
    let bare = |i: usize| {
        let word = run.get(i).copied().unwrap_or_default();
        word.trim_matches(|c: char| !c.is_alphanumeric())
    };
    let is = |list: &[&str], i: usize| list.iter().any(|w| w.eq_ignore_ascii_case(bare(i)));
    // Three to five words cost least.
    let size = |n: usize| match n {
        1 => 6,
        2 => 2,
        3..=5 => 0,
        6 => 1,
        7 => 4,
        _ => 9,
    };
    // A cut before word `j`: dearest after "the" or "in".
    let cut = |j: usize| {
        let leans = is(LEANERS, j - 1) || is(OPENERS, j - 1) || is(CLAUSES, j - 1);
        let opens = match (is(CLAUSES, j), is(OPENERS, j)) {
            (true, _) => -3,
            (_, true) => -1,
            _ => 0,
        };
        opens + if leans { 8 } else { 0 }
    };
    // The least cost to cut the first `j` words, and where its last starts.
    let mut best: Vec<(i32, usize)> = vec![(i32::MAX, 0); len + 1];
    if let Some(first) = best.first_mut() {
        *first = (0, 0);
    }
    for j in 1..=len {
        let edge = if j < len { cut(j) } else { 0 };
        for i in j.saturating_sub(PHRASE_LONGEST)..j {
            let Some(&(before, _)) = best.get(i).filter(|b| b.0 < i32::MAX) else {
                continue;
            };
            let cost = before + size(j - i) + 2 + edge;
            if let Some(slot) = best.get_mut(j).filter(|s| cost < s.0) {
                *slot = (cost, i);
            }
        }
    }
    let mut out = Vec::new();
    let mut j = len;
    while j > 0 {
        let i = best.get(j).map_or(0, |b| b.1);
        out.push(i..j);
        j = i;
    }
    out.reverse();
    out
}

/// The word heard: at `byte` when the voice knows it, else at `guess`.
pub fn heard(words: &[Word], byte: Option<usize>, guess: f64) -> usize {
    let byte = byte.unwrap_or(guess.max(0.0) as usize);
    words
        .partition_point(|w| w.bytes.start <= byte)
        .saturating_sub(1)
}

/// How long the sentence said has been heard, pauses left out.
#[derive(Default)]
pub struct Clock {
    since: Option<Instant>,
    banked: Duration,
}

impl Clock {
    /// From nothing, now.
    pub fn start(&mut self) {
        *self = Self {
            since: Some(Instant::now()),
            banked: Duration::ZERO,
        };
    }

    pub fn stop(&mut self) {
        if let Some(since) = self.since.take() {
            self.banked += since.elapsed();
        }
    }

    pub fn go(&mut self) {
        self.since.get_or_insert_with(Instant::now);
    }

    pub fn seconds(&self) -> f64 {
        let running = self.since.map_or(Duration::ZERO, |s| s.elapsed());
        (self.banked + running).as_secs_f64()
    }
}

/// Bytes said a second at normal speed, learnt from sentences heard.
pub struct Pace(f64);

impl Default for Pace {
    fn default() -> Self {
        Self(PACE)
    }
}

impl Pace {
    /// `bytes` heard whole in `seconds` at `rate`.
    pub fn learn(&mut self, bytes: usize, seconds: f64, rate: f32) {
        let rate = f64::from(rate.max(0.1));
        if seconds < 0.5 || bytes < 8 {
            return;
        }
        let now = bytes as f64 / seconds / rate;
        self.0 = (self.0 * 0.7 + now * 0.3).clamp(4.0, 60.0);
    }

    /// Bytes likely heard after `seconds` at `rate`.
    pub fn bytes(&self, seconds: f64, rate: f32) -> f64 {
        seconds * self.0 * f64::from(rate.max(0.1))
    }
}

/// Seconds of the usual pace a time-left guess starts out trusting.
const PRIOR: f64 = 20.0;

/// The whole reading's pace, for time left; one odd sentence barely moves it.
#[derive(Debug, Default)]
pub struct Steady {
    bytes: f64,
    seconds: f64,
}

impl Steady {
    /// `bytes` heard whole in `seconds` at `rate`.
    pub fn learn(&mut self, bytes: usize, seconds: f64, rate: f32) {
        if seconds < 0.5 || bytes < 8 {
            return;
        }
        self.bytes += bytes as f64;
        self.seconds += seconds * f64::from(rate.max(0.1));
    }

    /// Bytes likely heard in `seconds` at `rate`.
    pub fn bytes(&self, seconds: f64, rate: f32) -> f64 {
        let pace = (PACE * PRIOR + self.bytes) / (PRIOR + self.seconds);
        seconds * pace.clamp(4.0, 60.0) * f64::from(rate.max(0.1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mareader_speech::Spot;

    fn words(text: &str) -> Vec<Word> {
        let mut out = Vec::new();
        let mut at = 0;
        for run in text.split(' ') {
            let spot = Spot { page: 0, char: at };
            out.push(Word {
                bytes: at..at + run.len(),
                start: spot,
                end: spot,
            });
            at += run.len() + 1;
        }
        out
    }

    #[test]
    fn a_few_words_light_in_steps_and_a_sentence_whole() {
        let text = "one two three four five";
        let w = words(text);
        let ask = |at, mode| group(text, &w, at, mode);
        assert_eq!(ask(3, (Highlight::Words, 1)), 3..4);
        assert_eq!(ask(3, (Highlight::Words, 2)), 2..4);
        assert_eq!(
            ask(4, (Highlight::Words, 2)),
            4..5,
            "the last step is short"
        );
        assert_eq!(ask(1, (Highlight::Words, 10)), 0..5);
        assert_eq!(ask(2, (Highlight::Sentence, 3)), 0..5);
        assert_eq!(ask(99, (Highlight::Words, 1)), 4..5, "held to the last");
    }

    #[test]
    fn phrases_end_where_the_voice_pauses_and_long_ones_halve() {
        let text = "Well, it was late; a b c d e f g h i j k l m n.";
        let w = words(text);
        let ask = |at| group(text, &w, at, (Highlight::Phrases, 1));
        assert_eq!(ask(0), 0..1, "\"Well,\"");
        assert_eq!(ask(2), 1..4, "\"it was late;\"");
        // Fourteen words without a pause: three even parts.
        assert_eq!(ask(4), 4..8);
        assert_eq!(ask(9), 8..13);
        assert_eq!(ask(17), 13..18);
    }

    /// The phrases of `text` as words.
    fn cut(text: &str) -> Vec<String> {
        let w = words(text);
        let pick = |r: Range<usize>| {
            let (Some(a), Some(b)) = (w.get(r.start), w.get(r.end - 1)) else {
                return String::new();
            };
            text.get(a.bytes.start..b.bytes.end)
                .unwrap_or_default()
                .to_owned()
        };
        phrases(text, &w).into_iter().map(pick).collect()
    }

    #[test]
    fn long_runs_part_as_speech_does_never_after_a_leaning_word() {
        assert_eq!(
            cut(
                "What is Crockford's view on the evolution of technologies like \
                 JavaScript and the importance of simplicity in design?"
            ),
            [
                "What is Crockford's view",
                "on the evolution of technologies",
                "like JavaScript and the importance",
                "of simplicity in design?",
            ]
        );
        assert_eq!(
            cut(
                "The river ran slow and brown past the old customs house where the \
                 boats unloaded rice and timber at dawn."
            ),
            [
                "The river ran slow and brown",
                "past the old customs house",
                "where the boats unloaded rice",
                "and timber at dawn.",
            ]
        );
        assert_eq!(
            cut("Nobody remembered who had built the jetty and nobody asked."),
            [
                "Nobody remembered",
                "who had built the jetty",
                "and nobody asked."
            ]
        );
        assert_eq!(
            cut("Six words stay together as one."),
            ["Six words stay together as one."]
        );
    }

    #[test]
    fn the_word_heard_is_found_by_byte_or_guessed() {
        let w = words("one two three");
        assert_eq!(heard(&w, Some(4), 0.0), 1);
        assert_eq!(heard(&w, Some(6), 0.0), 1, "inside a word");
        assert_eq!(heard(&w, None, 9.5), 2);
        assert_eq!(heard(&w, None, -3.0), 0);
        assert_eq!(heard(&[], Some(3), 0.0), 0);
    }

    #[test]
    fn the_pace_learns_from_sentences_heard_and_keeps_to_its_speed() {
        let mut pace = Pace::default();
        assert_eq!(pace.bytes(2.0, 1.0), 28.0);
        pace.learn(40, 1.0, 2.0);
        // Twenty a second at normal speed, a share of the way there.
        assert!((pace.0 - (14.0 * 0.7 + 20.0 * 0.3)).abs() < 1e-9);
        pace.learn(3, 9.0, 1.0);
        pace.learn(90, 0.1, 1.0);
        assert!((pace.0 - 15.8).abs() < 1e-9, "too short to trust");
        assert!((pace.bytes(1.0, 2.0) - 31.6).abs() < 1e-9);
    }

    #[test]
    fn the_steady_pace_shrugs_off_one_odd_sentence() {
        let mut steady = Steady::default();
        for _ in 0..40 {
            steady.learn(80, 5.0, 1.0);
        }
        let before = steady.bytes(1.0, 1.0);
        steady.learn(80, 12.0, 1.0);
        let after = steady.bytes(1.0, 1.0);
        assert!((before - 16.0).abs() < 0.2, "{before}");
        assert!((before - after) / before < 0.05, "{before} → {after}");
        assert!((steady.bytes(1.0, 2.0) - 2.0 * after).abs() < 1e-9);
    }
}
