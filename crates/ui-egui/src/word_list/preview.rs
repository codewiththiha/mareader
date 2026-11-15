//! The word list's Markdown drawn as it will read: only what `build` writes.

use egui::text::LayoutJob;
use egui::{Color32, FontId, Grid, RichText, TextFormat, Ui};

use crate::theme::{self, Tokens};

/// The preview stops after this many rows, so huge lists stay smooth.
pub const ROWS: usize = 400;

/// A block of the list, parsed once per list.
#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Title(String),
    Heading(String),
    Paragraph(String),
    /// A bullet and how deep it sits.
    Item(String, u8),
    /// Header cells, then the rows.
    Table(Vec<String>, Vec<Vec<String>>),
}

/// The blocks of `markdown`, and whether rows were left out.
pub fn parse(markdown: &str) -> (Vec<Block>, bool) {
    let mut out = Vec::new();
    let mut rows = 0;
    let mut lines = markdown.lines().peekable();
    while let Some(line) = lines.next() {
        if rows >= ROWS {
            return (out, true);
        }
        if let Some(rest) = line.strip_prefix("# ") {
            out.push(Block::Title(rest.into()));
        } else if let Some(rest) = line.strip_prefix("## ") {
            out.push(Block::Heading(rest.into()));
        } else if let Some(rest) = line.strip_prefix("  - ") {
            out.push(Block::Item(rest.into(), 1));
            rows += 1;
        } else if let Some(rest) = line.strip_prefix("- ") {
            out.push(Block::Item(rest.into(), 0));
            rows += 1;
        } else if line.starts_with('|') {
            let head = cells(line);
            let mut body = Vec::new();
            lines.next_if(|l| l.starts_with("| ---") || l.starts_with("|---"));
            while let Some(row) = lines.next_if(|l| l.starts_with('|')) {
                if rows < ROWS {
                    body.push(cells(row));
                }
                rows += 1;
            }
            out.push(Block::Table(head, body));
        } else if !line.trim().is_empty() {
            out.push(Block::Paragraph(line.into()));
            rows += 1;
        }
    }
    (out, rows > ROWS)
}

/// A table row's cells, with escaped bars kept as text.
fn cells(line: &str) -> Vec<String> {
    let inner = line.trim().trim_start_matches('|').trim_end_matches('|');
    let marked = inner.replace("\\|", "\u{0}");
    let cells = marked.split('|').map(|c| c.trim().replace('\u{0}', "|"));
    cells.collect()
}

/// Draws the blocks.
pub fn show(ui: &mut Ui, blocks: &[Block]) {
    let t = Tokens::get(ui.ctx());
    let size = t.text;
    for (i, block) in blocks.iter().enumerate() {
        match block {
            Block::Title(s) => {
                ui.label(
                    RichText::new(s)
                        .font(theme::medium(size * 1.45))
                        .color(t.ink),
                );
                ui.add_space(4.0);
            }
            Block::Heading(s) => {
                ui.add_space(10.0);
                ui.label(
                    RichText::new(s)
                        .font(theme::medium(size * 1.15))
                        .color(t.ink),
                );
                ui.add_space(2.0);
            }
            Block::Paragraph(s) => {
                ui.label(inline(s, size, &t));
                ui.add_space(4.0);
            }
            Block::Item(s, depth) => {
                ui.horizontal_wrapped(|ui| {
                    ui.add_space(8.0 + f32::from(*depth) * 16.0);
                    ui.label(RichText::new("•").color(t.muted));
                    ui.label(inline(s, size, &t));
                });
            }
            Block::Table(head, rows) => {
                Grid::new(("word-table", i))
                    .striped(true)
                    .spacing([14.0, 6.0])
                    .show(ui, |ui| {
                        for h in head {
                            let font = theme::medium(t.text_small);
                            ui.label(RichText::new(h).font(font).color(t.muted));
                        }
                        ui.end_row();
                        for row in rows {
                            for c in row {
                                ui.label(inline(c, size, &t));
                            }
                            ui.end_row();
                        }
                    });
                ui.add_space(8.0);
            }
        }
    }
}

/// A line with **bold** and *italic* runs, wrapping in the space given.
fn inline(s: &str, size: f32, t: &Tokens) -> LayoutJob {
    let mut job = LayoutJob::default();
    let (mut bold, mut italic) = (false, false);
    let mut rest = s;
    let push = |job: &mut LayoutJob, text: &str, bold: bool, italic: bool| {
        if text.is_empty() {
            return;
        }
        let font = if bold {
            theme::medium(size)
        } else {
            FontId::proportional(size)
        };
        let color = if italic && !bold { t.muted } else { t.ink };
        let format = TextFormat {
            font_id: font,
            color,
            italics: italic,
            background: Color32::TRANSPARENT,
            ..Default::default()
        };
        job.append(text, 0.0, format);
    };
    while !rest.is_empty() {
        let next = rest.find('*').unwrap_or(rest.len());
        let (text, tail) = rest.split_at(next);
        push(&mut job, text, bold, italic);
        if let Some(after) = tail.strip_prefix("**") {
            bold = !bold;
            rest = after;
        } else if let Some(after) = tail.strip_prefix('*') {
            italic = !italic;
            rest = after;
        } else {
            rest = tail;
        }
    }
    job.wrap.max_width = f32::INFINITY;
    job
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn the_lists_markdown_parses_into_blocks() {
        let md = "# T · words\n\n*2 words above B2 · page 1*\n\n## One\n\n\
                  | Word | Burmese |\n| --- | --- |\n| **a** | x \\| y |\n\n\
                  **b** · C1\n\n- Burmese: z\n- Near *b*:\n  - **bb**\n";
        let (blocks, cut) = parse(md);
        assert!(!cut);
        assert_eq!(blocks.first(), Some(&Block::Title("T · words".into())));
        assert!(blocks.contains(&Block::Heading("One".into())));
        let table = Block::Table(
            vec!["Word".into(), "Burmese".into()],
            vec![vec!["**a**".into(), "x | y".into()]],
        );
        assert!(blocks.contains(&table), "{blocks:?}");
        assert!(blocks.contains(&Block::Item("**bb**".into(), 1)));
    }

    #[test]
    fn a_long_list_is_cut_for_the_preview() {
        let mut md = "| Word |\n| --- |\n".to_owned();
        for i in 0..ROWS + 5 {
            md.push_str(&format!("| w{i} |\n"));
        }
        let (blocks, cut) = parse(&md);
        assert!(cut);
        let rows = match blocks.first() {
            Some(Block::Table(_, rows)) => rows.len(),
            _ => 0,
        };
        assert_eq!(rows, ROWS);
    }
}
