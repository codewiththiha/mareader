//! Where reading is on a page, to find again once a book is set anew.

use mareader_pdf::{TextChar, TextLayer};
use mareader_session::Anchor;

/// Letters that end a sentence when a gap follows.
const STOPS: &[char] = &['.', '!', '?', '…', '。', '！', '？'];
/// What may close a sentence after its stop: quotes and brackets.
const CLOSERS: &[char] = &['"', '\'', '”', '’', ')', ']', '»', '」', '』'];
/// How many lines above the reading line its sentence may start.
const BACK_LINES: u32 = 3;

/// Where reading goes on at height `y`: its sentence's start, if near.
pub fn line_at(layer: &TextLayer, y: f32) -> Option<Anchor> {
    let chars = layer.text.chars();
    // The first line at least half in view.
    let first = chars
        .iter()
        .position(|c| (c.bounds.y0 + c.bounds.y1) / 2.0 > y)?;
    let line = chars.get(first)?.line;
    let mut i = first;
    while i > 0 && !starts(layer, i) {
        let above = chars.get(i - 1).map_or(line, |c| c.line);
        // A long sentence: the line itself is the better place.
        if line.saturating_sub(above) > BACK_LINES {
            i = first;
            break;
        }
        i -= 1;
    }
    (i..chars.len()).find_map(|k| anchor(layer, k))
}

/// The top of the line showing `at`, if this page shows it.
pub fn top_of(layer: &TextLayer, at: Anchor) -> Option<f32> {
    let chars = layer.text.chars();
    let k = (0..chars.len()).find(|&k| anchor(layer, k).is_some_and(|a| a >= at))?;
    let line = chars.get(k)?.line;
    // The line's top, not the letter's: tall letters set it.
    let tops = chars.iter().filter(|c| c.line == line);
    tops.map(|c| c.bounds.y0).reduce(f32::min)
}

/// Character `k` begins a sentence or a paragraph.
fn starts(layer: &TextLayer, k: usize) -> bool {
    let chars = layer.text.chars();
    let (Some(c), Some(prev)) = (chars.get(k), k.checked_sub(1).and_then(|p| chars.get(p))) else {
        return true;
    };
    if c.ch.is_whitespace() {
        return false;
    }
    let new_line = prev.line != c.line;
    if new_line && !layer.text.wraps(c.line) {
        return true;
    }
    let gap = c.space_before || new_line || prev.ch.is_whitespace();
    gap && ends(chars, k)
}

/// The letters before `k`, gaps skipped, close a sentence.
fn ends(chars: &[TextChar], k: usize) -> bool {
    let mut before = chars.get(..k).unwrap_or_default().iter().rev();
    let mut last = before.find(|c| !c.ch.is_whitespace()).map(|c| c.ch);
    while last.is_some_and(|ch| CLOSERS.contains(&ch)) {
        last = before.next().map(|c| c.ch);
    }
    last.is_some_and(|ch| STOPS.contains(&ch))
}

fn anchor(layer: &TextLayer, k: usize) -> Option<Anchor> {
    let (block, offset) = layer.place(k)?;
    Some(Anchor { block, offset })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mareader_pdf::{Bounds, PageText};

    /// A page of `lines`, 10 high each, every letter in block 0.
    fn page(lines: &[(bool, &str)]) -> TextLayer {
        let mut byte = 0u32;
        let mut places = Vec::new();
        let rows = lines.iter().enumerate().map(|(n, (wraps, text))| {
            let y = n as f32 * 10.0;
            let row: Vec<(char, Bounds)> = (text.chars().enumerate())
                .map(|(x, ch)| {
                    let x = x as f32;
                    (
                        ch,
                        Bounds {
                            x0: x,
                            y0: y,
                            x1: x + 1.0,
                            y1: y + 8.0,
                        },
                    )
                })
                .collect();
            places.extend(row.iter().map(|_| {
                byte += 1;
                Some((0, byte - 1))
            }));
            (*wraps, row)
        });
        let text = PageText::from_lines(rows.collect::<Vec<_>>());
        TextLayer {
            text,
            links: Vec::new(),
            places,
            ..TextLayer::default()
        }
    }

    fn at(offset: usize) -> Anchor {
        Anchor { block: 0, offset }
    }

    #[test]
    fn the_reading_line_backs_up_to_its_sentence() {
        let layer = page(&[
            (false, "One. Two goes"),
            (true, "on here."),
            (true, "Three."),
        ]);
        // The second line opens mid-sentence: "Two" starts it.
        assert_eq!(line_at(&layer, 12.0), Some(at(5)));
        // The third opens a sentence of its own.
        assert_eq!(line_at(&layer, 22.0), Some(at(21)));
    }

    #[test]
    fn a_paragraph_starts_a_sentence_and_quotes_close_one() {
        let layer = page(&[
            (false, "He said \"No.\""),
            (true, "Then left"),
            (false, "New"),
        ]);
        assert_eq!(line_at(&layer, 12.0), Some(at(13)));
        assert_eq!(line_at(&layer, 22.0), Some(at(22)));
    }

    #[test]
    fn a_long_sentence_keeps_the_line() {
        let lines = [
            (false, "a"),
            (true, "b"),
            (true, "c"),
            (true, "d"),
            (true, "e"),
            (true, "f"),
        ];
        let layer = page(&lines);
        assert_eq!(line_at(&layer, 42.0), Some(at(4)));
        assert_eq!(line_at(&layer, 32.0), Some(at(0)));
    }

    #[test]
    fn an_anchor_is_found_at_its_lines_top() {
        let layer = page(&[(false, "One. Two"), (true, "three.")]);
        assert_eq!(top_of(&layer, at(9)), Some(10.0));
        assert_eq!(top_of(&layer, at(2)), Some(0.0));
        assert_eq!(top_of(&layer, at(99)), None);
    }
}
