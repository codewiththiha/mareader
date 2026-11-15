//! Read pages' hard words, each once, set out as Markdown.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

use mareader_cefr::{MAX_WORD_CHARS, candidates, normalize, tokenize};
use mareader_dict::{Entry, Found, fold};
use mareader_session::Document;
use mareader_settings::{WordFields, WordGroup, WordLayout, WordList, WordSort};

use crate::session::Waker;

/// A cell keeps at most this many rows; a word this many near words.
const CELL_ROWS: usize = 4;
const NEAR_WORDS: usize = 4;

/// One picked page as read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Page {
    /// The page, counted from zero, as its number shows.
    pub page: usize,
    /// The outline chapter it falls in, if the book has an outline.
    pub chapter: Option<String>,
    pub text: String,
}

/// A hard word where it first shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub key: String,
    pub page: usize,
    pub chapter: Option<String>,
    pub band: u8,
    /// Its place in reading order.
    pub order: usize,
}

/// Every key the pages' words may be known by, each once.
pub fn keys(pages: &[Page]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for word in pages.iter().flat_map(|p| words_of(&p.text)) {
        for key in candidates(&word) {
            if seen.insert(key.clone()) {
                out.push(key);
            }
        }
    }
    out
}

/// The picked pages' text, each own page once, with its chapter.
fn read(document: &Document, pages: &[usize]) -> Vec<Page> {
    let outline = document.outline();
    let top = outline.iter().map(|e| e.depth).min();
    let chapters: Vec<_> = outline.iter().filter(|e| Some(e.depth) == top).collect();
    let mut out: Vec<Page> = Vec::new();
    for &shown in pages {
        let page = document.to_base(shown);
        if out.iter().any(|p| p.page == page) {
            continue;
        }
        let Ok(layer) = document.text_layer(shown) else {
            continue;
        };
        let (text, _) = crate::study::words::read(&layer);
        let mut before = chapters.iter().filter(|e| e.page <= shown);
        let chapter = before.next_back().map(|e| e.title.trim().to_owned());
        let text = text.into_iter().collect();
        out.push(Page {
            page,
            chapter,
            text,
        });
    }
    out
}

/// Reads `pages` on a thread, so the frame never waits; `wake` asks for one.
pub fn spawn(
    wake: Waker,
    document: Arc<Document>,
    pages: Vec<usize>,
    inline: bool,
) -> Receiver<Vec<Page>> {
    let (tx, rx) = channel();
    let work = move || {
        let _ = tx.send(read(&document, &pages));
        wake();
    };
    if inline || cfg!(target_arch = "wasm32") {
        work();
    } else {
        let named = std::thread::Builder::new().name("word-list".into());
        // Unspawned, the list never comes; the reader still works.
        let _ = named.spawn(work);
    }
    rx
}

fn words_of(text: &str) -> impl Iterator<Item = String> + '_ {
    let chars: Vec<char> = text.chars().collect();
    let spans = tokenize(text).into_iter();
    let spans = spans.filter(|s| s.len() <= MAX_WORD_CHARS);
    spans.filter_map(move |s| Some(chars.get(s.start..s.end)?.iter().collect()))
}

/// The words above `level`, each once, on the first page it shows.
pub fn hard(pages: &[Page], bands: &HashMap<String, u8>, level: u8) -> Vec<Word> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for page in pages {
        for word in words_of(&page.text) {
            let key = normalize(&word);
            if !seen.insert(key.clone()) {
                continue;
            }
            let band = (candidates(&word).iter())
                .filter_map(|k| bands.get(k).copied())
                .max()
                .unwrap_or(0);
            if band > level {
                let order = out.len();
                let chapter = page.chapter.clone();
                out.push(Word {
                    key,
                    page: page.page,
                    chapter,
                    band,
                    order,
                });
            }
        }
    }
    out
}

/// What a list is made from.
#[derive(Debug)]
pub struct Sheet<'a> {
    pub title: &'a str,
    /// The reader's level: the list holds words above it.
    pub level: u8,
    pub words: &'a [Word],
    /// Dictionary answers by word and language.
    pub found: &'a HashMap<(String, String), Found>,
    /// The asked languages that have a dictionary, in column order.
    pub languages: &'a [String],
    /// The first and last picked page.
    pub pages: (usize, usize),
}

/// The list as Markdown.
pub fn render(sheet: &Sheet<'_>, list: &WordList) -> String {
    let mut out = String::new();
    let title = match list.book && !sheet.title.trim().is_empty() {
        true => format!("{} · words", one_line(sheet.title)),
        false => "Word list".to_owned(),
    };
    let _ = writeln!(out, "# {title}\n");
    let (a, b) = (sheet.pages.0 + 1, sheet.pages.1 + 1);
    let pages = match a == b {
        true => format!("page {a}"),
        false => format!("pages {a}–{b}"),
    };
    let n = sheet.words.len();
    let level = mareader_cefr::label(sheet.level);
    let words = if n == 1 { "word" } else { "words" };
    let _ = writeln!(out, "*{n} {words} above {level} · {pages}*\n");
    let mut shown = Shown::new(sheet.words);
    let mut near_rows = false;
    for (name, mut words) in groups(sheet.words, list.group) {
        sort(&mut words, list.sort);
        if let Some(name) = name {
            let _ = writeln!(out, "## {}\n", one_line(&name));
        }
        let body = match list.layout {
            WordLayout::Table => table(sheet, list, &words, &mut shown),
            WordLayout::List => listed(sheet, list, &words, &mut shown),
        };
        near_rows |= body.1;
        out.push_str(&body.0);
    }
    if near_rows && list.layout == WordLayout::Table {
        out.push_str("*↳ marks a dictionary word near the book word above it.*\n");
    }
    out
}

/// Groups in reading order; names are chapters or pages.
fn groups(words: &[Word], group: WordGroup) -> Vec<(Option<String>, Vec<&Word>)> {
    let mut out: Vec<(Option<String>, Vec<&Word>)> = Vec::new();
    let chapters = words.iter().any(|w| w.chapter.is_some());
    for word in words {
        let name = match group {
            WordGroup::Chapter if chapters => Some(
                word.chapter
                    .clone()
                    .unwrap_or_else(|| "Opening pages".into()),
            ),
            WordGroup::Page => Some(format!("Page {}", word.page + 1)),
            _ => None,
        };
        match out.iter_mut().find(|(n, _)| *n == name) {
            Some((_, list)) => list.push(word),
            None => out.push((name, vec![word])),
        }
    }
    out
}

fn sort(words: &mut [&Word], sort: WordSort) {
    match sort {
        WordSort::Reading => words.sort_by_key(|w| w.order),
        WordSort::AToZ => words.sort_by(|a, b| a.key.cmp(&b.key).then(a.order.cmp(&b.order))),
        WordSort::ZToA => words.sort_by(|a, b| b.key.cmp(&a.key).then(a.order.cmp(&b.order))),
        WordSort::Level => words.sort_by(|a, b| b.band.cmp(&a.band).then(a.order.cmp(&b.order))),
    }
}

/// Near words already listed: the book's own words, then each shown one.
struct Shown(HashSet<String>);

impl Shown {
    fn new(words: &[Word]) -> Self {
        Self(words.iter().map(|w| fold(&w.key)).collect())
    }

    /// `word`'s near headwords not shown yet, with their rows per language.
    fn near(&mut self, sheet: &Sheet<'_>, word: &Word) -> Vec<(String, Vec<Vec<Entry>>)> {
        let mut out: Vec<(String, Vec<Vec<Entry>>)> = Vec::new();
        for (i, lang) in sheet.languages.iter().enumerate() {
            let Some(found) = sheet.found.get(&(word.key.clone(), lang.clone())) else {
                continue;
            };
            for entry in &found.near {
                let head = fold(&entry.word);
                if self.0.contains(&head) && !out.iter().any(|(h, _)| *h == head) {
                    continue;
                }
                let at = match out.iter().position(|(h, _)| *h == head) {
                    Some(at) => at,
                    None if out.len() < NEAR_WORDS => {
                        out.push((head, vec![Vec::new(); sheet.languages.len()]));
                        out.len() - 1
                    }
                    None => continue,
                };
                if let Some(rows) = out.get_mut(at).and_then(|(_, r)| r.get_mut(i)) {
                    rows.push(entry.clone());
                }
            }
        }
        self.0.extend(out.iter().map(|(h, _)| h.clone()));
        out
    }
}

/// `word`'s own rows, one list per language.
fn own(sheet: &Sheet<'_>, word: &Word) -> Vec<Vec<Entry>> {
    let rows = sheet.languages.iter().map(|lang| {
        let found = sheet.found.get(&(word.key.clone(), lang.clone()));
        found.map(|f| f.own.clone()).unwrap_or_default()
    });
    rows.collect()
}

/// The parts of speech across rows, each once: "noun, verb".
fn kinds(rows: &[Vec<Entry>]) -> String {
    let mut out: Vec<String> = Vec::new();
    for tags in rows.iter().flatten().map(Entry::tags) {
        for tag in tags.split(" · ").filter(|t| !t.is_empty()) {
            if !out.iter().any(|t| t == tag) {
                out.push(tag.to_owned());
            }
        }
    }
    out.join(", ")
}

/// One language's rows as text, each kept part once.
fn cell(rows: &[Entry], fields: WordFields) -> String {
    let mut out: Vec<String> = Vec::new();
    for e in rows {
        let mut s = String::new();
        if fields.definition {
            s.push_str(&one_line(&e.definition));
        }
        if let (true, Some(r)) = (fields.romanization, &e.romanization) {
            let _ = write!(s, " ({})", one_line(r));
        }
        if let (true, Some(sense)) = (fields.sense, &e.sense) {
            let _ = write!(s, " — *{}*", one_line(sense));
        }
        let s = s.trim().to_owned();
        if !s.is_empty() && !out.contains(&s) && out.len() < CELL_ROWS {
            out.push(s);
        }
    }
    out.join("; ")
}

/// Whether any row part shows, so language columns mean something.
fn meanings(fields: WordFields) -> bool {
    fields.definition || fields.romanization || fields.sense
}

fn table(sheet: &Sheet<'_>, list: &WordList, words: &[&Word], shown: &mut Shown) -> (String, bool) {
    let f = list.fields;
    let langs: &[String] = if meanings(f) { sheet.languages } else { &[] };
    let mut head = vec!["Word".to_owned()];
    if f.level {
        head.push("Level".into());
    }
    if f.part_of_speech && !sheet.languages.is_empty() {
        head.push("Part of speech".into());
    }
    head.extend(langs.iter().map(|l| mareader_dict::language(l)));
    if list.pages {
        head.push("Page".into());
    }
    let mut out = format!("| {} |\n", head.join(" | "));
    let _ = writeln!(out, "|{}", " --- |".repeat(head.len()));
    let mut any_near = false;
    let row = |out: &mut String, word: String, level: String, rows: &[Vec<Entry>], page| {
        let mut cells = vec![word];
        if f.level {
            cells.push(level);
        }
        if f.part_of_speech && !sheet.languages.is_empty() {
            cells.push(kinds(rows));
        }
        for i in 0..langs.len() {
            let text = rows.get(i).map(|r| cell(r, f)).unwrap_or_default();
            cells.push(if text.is_empty() {
                "—".into()
            } else {
                escape(&text)
            });
        }
        if list.pages {
            cells.push(page);
        }
        let _ = writeln!(out, "| {} |", cells.join(" | "));
    };
    for word in words {
        let level = mareader_cefr::label(word.band).to_owned();
        let page = (word.page + 1).to_string();
        row(
            &mut out,
            format!("**{}**", word.key),
            level,
            &own(sheet, word),
            page,
        );
        if !list.similar {
            continue;
        }
        for (head, rows) in shown.near(sheet, word) {
            any_near = true;
            let name = format!("↳ {} *(like {})*", escape(&head), word.key);
            row(&mut out, name, String::new(), &rows, String::new());
        }
    }
    out.push('\n');
    (out, any_near)
}

fn listed(
    sheet: &Sheet<'_>,
    list: &WordList,
    words: &[&Word],
    shown: &mut Shown,
) -> (String, bool) {
    let f = list.fields;
    let mut out = String::new();
    let mut any_near = false;
    let lines = |rows: &[Vec<Entry>]| -> Vec<String> {
        if !meanings(f) {
            return Vec::new();
        }
        let named = sheet.languages.iter().zip(rows);
        let named = named.map(|(l, r)| (mareader_dict::language(l), cell(r, f)));
        let named = named.filter(|(_, c)| !c.is_empty());
        named.map(|(l, c)| format!("{l}: {c}")).collect()
    };
    for word in words {
        let rows = own(sheet, word);
        let mut head = vec![format!("**{}**", word.key)];
        if f.level {
            head.push(mareader_cefr::label(word.band).to_owned());
        }
        let said = kinds(&rows);
        if f.part_of_speech && !said.is_empty() {
            head.push(format!("*{said}*"));
        }
        if list.pages {
            head.push(format!("p. {}", word.page + 1));
        }
        let _ = writeln!(out, "{}\n", head.join(" · "));
        let mut items = lines(&rows);
        if items.is_empty() && meanings(f) && !sheet.languages.is_empty() {
            items.push("*Not in the dictionary*".into());
        }
        for item in items {
            let _ = writeln!(out, "- {item}");
        }
        let near = if list.similar {
            shown.near(sheet, word)
        } else {
            Vec::new()
        };
        if !near.is_empty() {
            any_near = true;
            let _ = writeln!(out, "- Near *{}*:", word.key);
            for (head, rows) in near {
                let kinds = kinds(&rows);
                let kinds = if kinds.is_empty() {
                    String::new()
                } else {
                    format!(" *({kinds})*")
                };
                let said = lines(&rows).join("; ");
                let said = if said.is_empty() {
                    String::new()
                } else {
                    format!(" — {said}")
                };
                let _ = writeln!(out, "  - **{head}**{kinds}{said}");
            }
        }
        out.push('\n');
    }
    (out, any_near)
}

/// Text on one line, as a table cell or heading needs.
fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A table cell's text: its own bars kept as text.
fn escape(s: &str) -> String {
    s.replace('|', "\\|")
}

#[cfg(test)]
#[path = "word_marks_tests.rs"]
mod tests;
