//! A book's highlights and notes as Markdown, plain text or JSON.

use mareader_appearance::Rgb;
use mareader_library::Pen;
use serde_json::{Value, json};

/// What an export is written as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Markdown,
    Text,
    Json,
}

impl Format {
    pub const ALL: [Format; 3] = [Format::Markdown, Format::Text, Format::Json];

    pub fn label(self) -> &'static str {
        match self {
            Self::Markdown => "Markdown",
            Self::Text => "Plain text",
            Self::Json => "JSON",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Text => "txt",
            Self::Json => "json",
        }
    }
}

/// One highlight as exported.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub quote: String,
    pub note: Option<String>,
    /// From 0; the reader sees it from 1.
    pub page: Option<usize>,
    pub chapter: Option<String>,
    pub pen: Pen,
    pub color: Rgb,
    /// When it was made, in wall milliseconds.
    pub added: u64,
}

/// A book's entries, in reading order, and when they were exported.
pub struct Sheet<'a> {
    pub title: &'a str,
    pub entries: &'a [Entry],
    pub now_ms: u64,
}

/// The sheet written as `format`.
pub fn render(sheet: &Sheet<'_>, format: Format) -> String {
    match format {
        Format::Markdown => markdown(sheet),
        Format::Text => text(sheet),
        Format::Json => json(sheet),
    }
}

/// A file name for the export: the title, tidied, and the extension.
pub fn file_name(title: &str, format: Format) -> String {
    let bad = |c: char| c.is_control() || r#"/\:*?"<>|"#.contains(c);
    let clean: String = title
        .chars()
        .map(|c| if bad(c) { ' ' } else { c })
        .collect();
    let clean = one_line(&clean);
    let stem = if clean.is_empty() { "Book" } else { &clean };
    format!("{stem} - highlights.{}", format.extension())
}

/// "12 highlights · 3 notes", in words for one of each.
pub fn tally(highlights: usize, notes: usize) -> String {
    let plural = |n: usize, one: &str| match n {
        1 => format!("1 {one}"),
        n => format!("{n} {one}s"),
    };
    format!(
        "{} · {}",
        plural(highlights, "highlight"),
        plural(notes, "note")
    )
}

fn tally_of(entries: &[Entry]) -> String {
    let notes = entries.iter().filter(|e| e.note.is_some()).count();
    tally(entries.len(), notes)
}

fn markdown(sheet: &Sheet<'_>) -> String {
    let mut out = format!("# {}\n\n", one_line(sheet.title));
    out += &format!(
        "*{} · exported {}*\n",
        tally_of(sheet.entries),
        date(sheet.now_ms)
    );
    let mut chapter = None;
    for e in sheet.entries {
        if e.chapter.is_some() && e.chapter != chapter {
            chapter.clone_from(&e.chapter);
            out += &format!("\n## {}\n", one_line(e.chapter.as_deref().unwrap_or("")));
        }
        out += &format!("\n> {}\n", one_line(&e.quote));
        if let Some(note) = &e.note {
            out += &format!("\n**Note:** {}\n", note.trim().replace('\n', "  \n"));
        }
        if let Some(p) = e.page {
            out += &format!("\n*Page {}*\n", p + 1);
        }
    }
    out
}

fn text(sheet: &Sheet<'_>) -> String {
    let mut out = format!("{}\n", one_line(sheet.title));
    out += &format!(
        "{}, exported {}\n",
        tally_of(sheet.entries).replace(" · ", ", "),
        date(sheet.now_ms)
    );
    let mut chapter = None;
    for e in sheet.entries {
        if e.chapter.is_some() && e.chapter != chapter {
            chapter.clone_from(&e.chapter);
            let name = one_line(e.chapter.as_deref().unwrap_or(""));
            out += &format!("\n== {name} ==\n");
        }
        out += &format!("\n\"{}\"\n", one_line(&e.quote));
        if let Some(note) = &e.note {
            out += &format!("Note: {}\n", note.trim());
        }
        if let Some(p) = e.page {
            out += &format!("Page {}\n", p + 1);
        }
    }
    out
}

fn json(sheet: &Sheet<'_>) -> String {
    let pen = |p: Pen| serde_json::to_value(p).unwrap_or(Value::Null);
    let entries: Vec<Value> = sheet
        .entries
        .iter()
        .map(|e| {
            json!({
                "text": one_line(&e.quote),
                "note": e.note.as_deref().map(str::trim),
                "page": e.page.map(|p| p + 1),
                "chapter": e.chapter,
                "style": pen(e.pen),
                "color": e.color.to_hex(),
                "created": stamp(e.added),
            })
        })
        .collect();
    let doc = json!({
        "title": one_line(sheet.title),
        "exported": stamp(sheet.now_ms),
        "highlights": entries,
    });
    serde_json::to_string_pretty(&doc).unwrap_or_default() + "\n"
}

/// Words on one line: a PDF's line breaks are layout, not meaning.
fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The UTC date of `ms`, as 2026-10-17.
pub fn date(ms: u64) -> String {
    let (y, m, d) = civil(ms / 86_400_000);
    format!("{y:04}-{m:02}-{d:02}")
}

/// The UTC time of `ms`, as 2026-10-17T08:30:00Z.
fn stamp(ms: u64) -> String {
    let secs = ms / 1000;
    let (h, min, s) = (secs / 3600 % 24, secs / 60 % 60, secs % 60);
    format!("{}T{h:02}:{min:02}:{s:02}Z", date(ms))
}

/// Days since 1970-01-01 as a calendar date (Howard Hinnant's method).
fn civil(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + u64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-10-17 08:30:00 UTC.
    const NOW: u64 = 1_792_225_800_000;

    fn entries() -> Vec<Entry> {
        let entry = |quote: &str, note: Option<&str>, page, chapter: Option<&str>| Entry {
            quote: quote.into(),
            note: note.map(Into::into),
            page: Some(page),
            chapter: chapter.map(Into::into),
            pen: Pen::Full,
            color: Rgb(0xff, 0xff, 0x00),
            added: NOW,
        };
        vec![
            entry(
                "The quick\nbrown fox",
                Some("Check the fox."),
                0,
                Some("One"),
            ),
            entry("lazy", None, 2, Some("One")),
            entry("Sphinx", Some("Line one\nline two"), 4, Some("Two")),
        ]
    }

    fn sheet(entries: &[Entry]) -> Sheet<'_> {
        Sheet {
            title: "A/B: book",
            entries,
            now_ms: NOW,
        }
    }

    #[test]
    fn dates_are_read_from_the_epoch() {
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(stamp(NOW), "2026-10-17T08:30:00Z");
        assert_eq!(date(951_782_400_000), "2000-02-29");
    }

    #[test]
    fn markdown_groups_by_chapter_with_quotes_notes_and_pages() {
        let entries = entries();
        let md = render(&sheet(&entries), Format::Markdown);
        assert!(md.starts_with("# A/B: book\n\n*3 highlights · 2 notes · exported 2026-10-17*\n"));
        assert_eq!(md.matches("## One").count(), 1, "{md}");
        assert!(md.contains("> The quick brown fox\n\n**Note:** Check the fox.\n\n*Page 1*"));
        assert!(
            md.contains("**Note:** Line one  \nline two"),
            "a hard break keeps lines"
        );
        assert!(md.find("## Two") > md.find("lazy"));
    }

    #[test]
    fn plain_text_and_json_carry_the_same_highlights() {
        let entries = entries();
        let txt = render(&sheet(&entries), Format::Text);
        assert!(txt.contains("3 highlights, 2 notes, exported 2026-10-17"));
        assert!(
            txt.contains("== One ==\n\n\"The quick brown fox\"\nNote: Check the fox.\nPage 1\n")
        );
        let doc: Value = serde_json::from_str(&render(&sheet(&entries), Format::Json)).unwrap();
        let first = &doc["highlights"][0];
        assert_eq!(first["text"], "The quick brown fox");
        assert_eq!(
            (first["page"].as_u64(), first["style"].as_str()),
            (Some(1), Some("full"))
        );
        assert_eq!(first["color"], "#ffff00");
        assert_eq!(doc["highlights"][1]["note"], Value::Null);
        assert_eq!(doc["exported"], "2026-10-17T08:30:00Z");
    }

    #[test]
    fn a_file_name_never_holds_a_path() {
        assert_eq!(
            file_name("A/B: book", Format::Markdown),
            "A B book - highlights.md"
        );
        assert_eq!(file_name(" \n", Format::Json), "Book - highlights.json");
    }
}
