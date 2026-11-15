//! A page's text as pieces for the AI, and a result as blocks to set.

use std::hash::{Hash, Hasher};
use std::ops::Range;

use mareader_pdf::{Bounds, PageText};
use mareader_reflow::{Block, Content, Emphasis, Frame, Role as BlockRole, Text, clip};
use mareader_study::page::{Kept, Out, PdfLine, Piece, Rewritten, Role, pdf_pieces};

/// Where a piece came from: a block's bytes or rows, or nowhere.
#[derive(Clone, Debug, PartialEq)]
enum Origin {
    Block { index: usize, range: Range<usize> },
    Loose,
}

/// A page's pieces, with what each was cut from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Source {
    pub pieces: Vec<Piece>,
    origins: Vec<Origin>,
}

impl Source {
    /// Nothing for the AI to read: no text, only kept parts.
    pub fn is_blank(&self) -> bool {
        !self.pieces.iter().any(|p| match p {
            Piece::Text { text, .. } => !text.trim().is_empty(),
            Piece::Kept(_) => false,
        })
    }

    /// A fingerprint of the text, to tell when the page has changed.
    pub fn key(&self) -> String {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for p in &self.pieces {
            if let Piece::Text { text, .. } = p {
                text.hash(&mut h);
            }
        }
        format!("{:016x}", h.finish())
    }

    fn push(&mut self, piece: Piece, origin: Origin) {
        self.pieces.push(piece);
        self.origins.push(origin);
    }
}

/// Pages' sources as one, in order; `true` marks a page that runs on.
pub fn join(pages: Vec<(Source, bool)>) -> Source {
    let mut out = Source::default();
    for (page, runs_on) in pages {
        let mut pieces = page.pieces.into_iter().zip(page.origins).peekable();
        let first_para = |p: &(Piece, Origin)| {
            matches!(
                &p.0,
                Piece::Text {
                    role: Role::Para,
                    ..
                }
            )
        };
        // A paragraph cut by the page break is whole again.
        if runs_on
            && pieces.peek().is_some_and(first_para)
            && let Some(Piece::Text {
                role: Role::Para,
                text,
                code,
            }) = out.pieces.last_mut()
            && let Some((
                Piece::Text {
                    text: more,
                    code: more_code,
                    ..
                },
                _,
            )) = pieces.next()
        {
            if text.ends_with('-') && more.starts_with(char::is_lowercase) {
                text.pop();
            } else {
                text.push(' ');
            }
            let at = text.len();
            text.push_str(&more);
            code.extend(more_code.into_iter().map(|r| r.start + at..r.end + at));
        }
        for (piece, origin) in pieces {
            out.push(piece, origin);
        }
    }
    out
}

/// The span a book's page shows: its first and last `[block, byte]`.
pub fn span(excerpts: &[(usize, Range<usize>)], blocks: &[Block]) -> Option<[[usize; 2]; 2]> {
    let (first, last) = (excerpts.first()?, excerpts.last()?);
    let end = match blocks.get(last.0).map(|b| &b.content) {
        Some(Content::Text(t)) => last.1.end.min(t.text.len()),
        Some(Content::Table(t)) => last.1.end.min(t.rows.len()),
        _ => last.1.end.min(1),
    };
    Some([[first.0, first.1.start], [last.0, end]])
}

/// A book's text from the first `[block, byte]` to the last.
pub fn book(blocks: &[Block], [[first, first_byte], [last, last_byte]]: [[usize; 2]; 2]) -> Source {
    let mut out = Source::default();
    for index in first..=last {
        let Some(block) = blocks.get(index).filter(|b| !b.is_blank()) else {
            continue;
        };
        let start = if index == first { first_byte } else { 0 };
        let end = if index == last { last_byte } else { usize::MAX };
        let piece = match &block.content {
            Content::Text(t) => {
                let range = clip(&t.text, start..end);
                let Some(piece) = text_piece(t, &block.frame, range.clone()) else {
                    continue;
                };
                out.push(piece, Origin::Block { index, range });
                continue;
            }
            Content::Table(t) => {
                let rows = start.min(t.rows.len())..end.min(t.rows.len());
                let kept = Kept::Table { rows: rows.len() };
                out.push(Piece::Kept(kept), Origin::Block { index, range: rows });
                continue;
            }
            Content::Image { .. } => Kept::Picture { alt: String::new() },
            Content::Rule | Content::Space(_) => Kept::Other,
        };
        out.push(Piece::Kept(piece), Origin::Block { index, range: 0..1 });
    }
    out
}

fn text_piece(t: &Text, frame: &Frame, range: Range<usize>) -> Option<Piece> {
    let text = t.text.get(range.clone())?;
    if text.trim().is_empty() {
        return None;
    }
    let role = match t.role {
        BlockRole::Heading(level) => Role::Heading(level),
        BlockRole::Preformatted if looks_like_code(text) => {
            let lines = text.lines().count().max(1);
            return Some(Piece::Kept(Kept::Code { lines }));
        }
        _ if frame.marker.is_some() => Role::Item,
        _ if !frame.bars.is_empty() => Role::Quote,
        _ => Role::Para,
    };
    let code = t
        .spans
        .iter()
        .filter(|s| s.emphasis.code)
        .filter_map(|s| {
            let start = s.range.start.max(range.start);
            let end = s.range.end.min(range.end);
            (start < end).then(|| start - range.start..end - range.start)
        })
        .collect();
    Some(Piece::Text {
        role,
        text: text.to_owned(),
        code,
    })
}

/// Code, not verse: both keep their lines, but only verse is translated.
pub fn looks_like_code(text: &str) -> bool {
    const LEADS: [&str; 16] = [
        "fn ", "def ", "let ", "var ", "const ", "import ", "return", "class ", "#include",
        "function", "public ", "private ", "if (", "for (", "while (", "SELECT ",
    ];
    let marks = text.chars().filter(|c| "{}[]();=<>/\\|&*#$_`".contains(*c));
    let ink = text.chars().filter(|c| !c.is_whitespace()).count().max(1);
    let lead = text
        .lines()
        .any(|l| LEADS.iter().any(|k| l.trim_start().starts_with(k)));
    lead || marks.count() * 100 / ink >= 6
}

/// A PDF page as paragraphs; `near` are the pages beside it, if loaded.
pub fn pdf(text: &PageText, near: &[&PageText]) -> Source {
    let rows: Vec<(String, Bounds, bool)> = text.rows().collect();
    let lines = pdf_lines(&rows);
    // Running heads repeat at the edges of the pages around.
    let mut running = Vec::new();
    for page in near {
        let rows: Vec<String> = page.rows().map(|r| r.0).collect();
        let edge = rows.iter().take(2).chain(rows.iter().rev().take(2));
        running.extend(edge.cloned());
    }
    let mut out = Source::default();
    for piece in pdf_pieces(&lines, &running) {
        out.push(piece, Origin::Loose);
    }
    out
}

/// Each row, marked where a gap, an indent or a short line breaks it.
fn pdf_lines(rows: &[(String, Bounds, bool)]) -> Vec<PdfLine> {
    let mut heights: Vec<f32> = rows.iter().map(|r| r.1.height()).collect();
    heights.sort_by(f32::total_cmp);
    let h = heights
        .get(heights.len() / 2)
        .copied()
        .unwrap_or(10.0)
        .max(1.0);
    let left = rows.iter().map(|r| r.1.x0).fold(f32::INFINITY, f32::min);
    let right = rows
        .iter()
        .map(|r| r.1.x1)
        .fold(f32::NEG_INFINITY, f32::max);
    let mut out = Vec::with_capacity(rows.len());
    let mut prev: Option<&(String, Bounds, bool)> = None;
    for row in rows {
        let (text, b, wraps) = row;
        let starts = match prev {
            None => true,
            Some(_) if *wraps => false,
            Some((p_text, p, _)) => {
                let gap = b.y0 - p.y1;
                let ended = p_text.trim_end().ends_with(['.', '!', '?', ':', '"', '”']);
                b.y0 < p.y0 - h
                    || gap > 0.8 * h
                    || b.x0 > left + 1.2 * h
                    || (ended && p.x1 < right - 3.0 * h)
            }
        };
        out.push(PdfLine {
            text: text.clone(),
            starts,
        });
        prev = Some(row);
    }
    out
}

/// The result as blocks; kept parts are cut from `original` again.
pub fn blocks(result: &Rewritten, source: &Source, original: &[Block]) -> Vec<Block> {
    let origin = |i: usize| match source.origins.get(i) {
        Some(Origin::Block { index, range }) => Some((original.get(*index)?, range.clone())),
        _ => None,
    };
    let mut out = Vec::with_capacity(result.outs.len());
    for o in &result.outs {
        match o {
            Out::Kept(i) => {
                if let Some((block, range)) = origin(*i) {
                    out.push(block.cut(range));
                }
            }
            Out::Text {
                role,
                text,
                code,
                from,
            } => {
                let from = from.and_then(origin).map(|(b, _)| b);
                out.push(text_block(*role, text, code, from));
            }
        }
    }
    out
}

fn text_block(role: Role, text: &str, code: &[Range<usize>], from: Option<&Block>) -> Block {
    let mut frame = from.map(|b| b.frame.clone()).unwrap_or_default();
    // A summary's points and quotes have no block to take a frame from.
    if from.is_none() {
        match role {
            Role::Item => {
                frame.marker = Some("•".into());
                frame.inset = 1.5;
                frame.tight = true;
            }
            Role::Quote => {
                frame.bars = mareader_reflow::thin_vec![0.5];
                frame.inset = 1.5;
                frame.soft = true;
            }
            Role::Heading(_) | Role::Para => {}
        }
    }
    let block_role = match role {
        Role::Heading(level) => BlockRole::Heading(level.clamp(1, 6)),
        _ => BlockRole::Paragraph,
    };
    let mut t = Text::new(block_role);
    let mut at = 0;
    let mut runs: Vec<Range<usize>> = code.iter().map(|r| clip(text, r.clone())).collect();
    runs.sort_by_key(|r| r.start);
    for run in runs {
        if run.start < at {
            continue;
        }
        t.push(
            text.get(at..run.start).unwrap_or_default(),
            Emphasis::default(),
        );
        let mono = Emphasis {
            code: true,
            ..Emphasis::default()
        };
        t.push(text.get(run.clone()).unwrap_or_default(), mono);
        at = run.end;
    }
    t.push(text.get(at..).unwrap_or_default(), Emphasis::default());
    Block {
        content: Content::Text(t),
        frame,
        break_before: false,
    }
}

/// The result as plain text, for the clipboard.
pub fn plain(result: &Rewritten) -> String {
    let mut out = String::new();
    for o in &result.outs {
        if let Out::Text { role, text, .. } = o {
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            if *role == Role::Item {
                out.push_str("• ");
            }
            out.push_str(text.trim());
        }
    }
    out
}

#[cfg(test)]
#[path = "extract_tests.rs"]
mod tests;
