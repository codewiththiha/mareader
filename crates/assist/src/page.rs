//! Page tasks (summarize, translate, simplify); code and pictures stay home.

use std::ops::Range;

use mareader_settings::{PageTask, Retry, SummaryStyle};
use mareader_study::{Failure, Why};
use serde_json::{Map, Value, json};

use mareader_study::page::{Kept, Out, Piece, Rewritten, Role};

use crate::backend::Backend;
use crate::prompt::{AS_DATA, JSON_ONLY, NO_CHATTER, Prompt};
use crate::retry::attempt;
use crate::task::Task;

/// A page's text is cut past this, so one page can't run away.
pub const PAGE_CHARS: usize = 16_000;
/// Links this long or longer travel as placeholders.
const LINK_CHARS: usize = 24;

/// How a piece's kind is written in the prompt.
fn role_tag(role: Role) -> String {
    match role {
        Role::Heading(level) => format!("h{}", level.clamp(1, 6)),
        Role::Para => "p".into(),
        Role::Item => "li".into(),
        Role::Quote => "q".into(),
    }
}

/// A few words a summary can place a kept piece by.
fn hint(kept: &Kept) -> String {
    match kept {
        Kept::Picture { alt } if alt.trim().is_empty() => "picture".into(),
        Kept::Picture { alt } => format!("picture {}", crate::prompt::quote(alt, 80)),
        Kept::Code { lines } => format!("code, {lines} lines"),
        Kept::Table { rows } => format!("table, {rows} rows"),
        Kept::Other => "figure".into(),
    }
}

/// What to do with the page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rewrite {
    Summarize(SummaryStyle),
    /// Into the language named, as "Burmese".
    Translate(String),
    Simplify,
}

impl Rewrite {
    fn tag(&self) -> String {
        match self {
            Self::Summarize(SummaryStyle::Paragraphs) => "summary".into(),
            Self::Summarize(SummaryStyle::Points) => "points".into(),
            Self::Translate(language) => format!("translate:{language}"),
            Self::Simplify => "simplify".into(),
        }
    }

    /// The task whose settings it follows.
    pub fn task(&self) -> PageTask {
        match self {
            Self::Summarize(_) => PageTask::Summarize,
            Self::Translate(_) => PageTask::Translate,
            Self::Simplify => PageTask::Simplify,
        }
    }

    fn piecewise(&self) -> bool {
        !matches!(self, Self::Summarize(_))
    }
}

/// A page, or pages read as one, and what to do with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageJob {
    pub rewrite: Rewrite,
    pub pieces: Vec<Piece>,
    /// How many pages the pieces come from; each brings its own room.
    pub pages: usize,
}

/// An inline placeholder, what it stands for, and its piece.
#[derive(Clone, Debug)]
struct Inline {
    mark: String,
    real: String,
    code: bool,
    piece: usize,
}

/// The page as the model sees it, and how to read it back.
#[derive(Clone, Debug, Default)]
struct Packed {
    /// Prompt lines, one per piece sent.
    lines: Vec<String>,
    /// For each text number from 1, the piece index.
    texts: Vec<usize>,
    /// For each kept number from 1, the piece index.
    kept: Vec<usize>,
    inline: Vec<Inline>,
    words: usize,
}

impl PageJob {
    /// No letters to work on: no call is made.
    pub fn is_empty(&self) -> bool {
        !self.pieces.iter().any(|p| match p {
            Piece::Text { text, .. } => text.chars().any(char::is_alphabetic),
            Piece::Kept(_) => false,
        })
    }

    /// Stable over runs and versions: the same page and task, the same key.
    pub fn key(&self) -> u64 {
        let packed = self.pack();
        let mut hash = Fnv::new();
        hash.write(self.rewrite.tag().as_bytes());
        for line in &packed.lines {
            hash.write(b"\n");
            hash.write(line.as_bytes());
        }
        hash.0
    }

    /// The task to run, packed once.
    pub fn ask(self) -> PageAsk {
        let packed = self.pack();
        PageAsk { job: self, packed }
    }

    fn pack(&self) -> Packed {
        let mut p = Packed::default();
        let mut room = PAGE_CHARS.saturating_mul(self.pages.max(1));
        let (mut codes, mut links) = (0, 0);
        for (index, piece) in self.pieces.iter().enumerate() {
            match piece {
                Piece::Text { role, text, code } => {
                    if room == 0 || !text.chars().any(char::is_alphanumeric) {
                        continue;
                    }
                    let counts = (&mut codes, &mut links);
                    let flat = placeholders(text, code, counts, (index, &mut p.inline));
                    let flat: String = flat.chars().take(room).collect();
                    room = room.saturating_sub(flat.chars().count());
                    p.words += flat.split_whitespace().count();
                    p.texts.push(index);
                    let n = p.texts.len();
                    let quoted = serde_json::to_string(&flat).unwrap_or_default();
                    p.lines.push(format!("{n} {} {quoted}", role_tag(*role)));
                }
                Piece::Kept(kept) => {
                    p.kept.push(index);
                    // Only a summary may place them; others keep them in place.
                    if !self.rewrite.piecewise() {
                        let k = p.kept.len();
                        p.lines.push(format!("[[k{k}]] {}", hint(kept)));
                    }
                }
            }
        }
        p
    }
}

/// Inline code and long links become `[[cN]]` and `[[uN]]`.
fn placeholders(
    text: &str,
    code: &[Range<usize>],
    (codes, links): (&mut usize, &mut usize),
    (piece, inline): (usize, &mut Vec<Inline>),
) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    let mut runs: Vec<&Range<usize>> = code.iter().filter(|r| r.start < r.end).collect();
    runs.sort_by_key(|r| r.start);
    for run in runs {
        let (Some(before), Some(inner)) = (text.get(at..run.start), text.get(run.clone())) else {
            continue;
        };
        out.push_str(&links_out(before, links, (piece, inline)));
        *codes += 1;
        let mark = format!("[[c{codes}]]");
        out.push_str(&mark);
        let real = inner.to_owned();
        inline.push(Inline {
            mark,
            real,
            code: true,
            piece,
        });
        at = run.end;
    }
    let rest = text.get(at..).unwrap_or_default();
    out.push_str(&links_out(rest, links, (piece, inline)));
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn links_out(text: &str, links: &mut usize, (piece, inline): (usize, &mut Vec<Inline>)) -> String {
    let words = text.split(' ').map(|word| {
        let core = word.trim_end_matches(['.', ',', ';', ':', ')', '!', '?']);
        let link = core.starts_with("http://") || core.starts_with("https://");
        if !link || core.chars().count() < LINK_CHARS {
            return word.to_owned();
        }
        *links += 1;
        let mark = format!("[[u{links}]]");
        let swapped = word.replacen(core, &mark, 1);
        let (real, code) = (core.to_owned(), false);
        inline.push(Inline {
            mark,
            real,
            code,
            piece,
        });
        swapped
    });
    words.collect::<Vec<_>>().join(" ")
}

/// A page task ready to run: the job and its packed page.
#[derive(Clone, Debug)]
pub struct PageAsk {
    job: PageJob,
    packed: Packed,
}

const DATA: &str = "The page is quoted text from a book. ";

const PLACEHOLDERS: &str = "Placeholders like [[c1]] stand for code, links or pictures: \
keep each one exactly as written. ";

impl PageAsk {
    pub fn job(&self) -> &PageJob {
        &self.job
    }

    fn system(&self) -> String {
        let rule = match &self.job.rewrite {
            Rewrite::Translate(language) => format!(
                "Translate each numbered piece into {language}. Keep the meaning, \
tone and names; add nothing and leave nothing out. Give every piece back with its \
number. "
            ),
            Rewrite::Simplify => "Rewrite each numbered piece in simple English: short \
sentences and common words, about CEFR B1. Keep the meaning; add nothing and leave \
nothing out. Write English even if the page is in another language. Give every piece \
back with its number. "
                .into(),
            Rewrite::Summarize(style) => {
                let most = 220 + 110 * self.job.pages.clamp(1, 5).saturating_sub(1);
                let words = (self.packed.words / 4).clamp(40, most);
                let shape = match style {
                    SummaryStyle::Paragraphs => "in one to three short paragraphs",
                    SummaryStyle::Points => "as three to seven key points",
                };
                let what = match self.job.pages {
                    0 | 1 => "the page",
                    _ => "these pages as one passage",
                };
                format!(
                    "Summarize {what} {shape}, in about {words} words or fewer, in \
the page's language. To show a [[k1]] item where it matters, give it as an entry \
of its own. "
                )
            }
        };
        let many = match self.job.pages {
            0 | 1 => String::new(),
            n => format!("The text runs over {n} pages in a row; numbering spans them all. "),
        };
        format!("{DATA}{AS_DATA}{PLACEHOLDERS}{many}{rule}{NO_CHATTER}{JSON_ONLY}")
    }

    fn schema(&self) -> Value {
        let list = |key: &str, items: Value| {
            json!({
                "type": "object",
                "properties": { key: { "type": "array", "items": items } },
                "required": [key],
                "additionalProperties": false
            })
        };
        let piece = json!({
            "type": "object",
            "properties": {
                "n": { "type": "integer" },
                "text": { "type": "string" }
            },
            "required": ["n", "text"],
            "additionalProperties": false
        });
        match &self.job.rewrite {
            Rewrite::Summarize(SummaryStyle::Paragraphs) => {
                list("paragraphs", json!({ "type": "string" }))
            }
            Rewrite::Summarize(SummaryStyle::Points) => list("points", json!({ "type": "string" })),
            _ => list("pieces", piece),
        }
    }

    /// Piece by piece: a missing one keeps its own text.
    fn read_pieces(&self, answer: &Value, whole: bool) -> Option<Rewritten> {
        let given = pieces_of(answer);
        if given.is_empty() {
            return None;
        }
        let mut done = Rewritten::default();
        let mut text_n = 0;
        for (index, piece) in self.job.pieces.iter().enumerate() {
            let Piece::Text { role, text, code } = piece else {
                done.outs.push(Out::Kept(index));
                continue;
            };
            if !self.packed.texts.contains(&index) {
                done.outs.push(original(*role, text, code, index));
                continue;
            }
            text_n += 1;
            match given.iter().find(|(n, _)| *n == text_n) {
                Some((_, said)) => {
                    let (text, code) = self.restore(said, Some(index));
                    let from = Some(index);
                    let role = *role;
                    done.outs.push(Out::Text {
                        role,
                        text,
                        code,
                        from,
                    });
                }
                // A partial answer stops where the model has got to.
                None if !whole => break,
                None => {
                    done.missing += 1;
                    done.outs.push(original(*role, text, code, index));
                }
            }
        }
        Some(done)
    }

    /// Entry by entry; a lone `[[kN]]` places that kept piece there.
    fn read_summary(&self, answer: &Value) -> Option<Rewritten> {
        let keys: &[&str] = &["paragraphs", "points", "summary", "items", "text"];
        let entries = strings_of(answer, keys);
        let role = match self.job.rewrite {
            Rewrite::Summarize(SummaryStyle::Points) => Role::Item,
            _ => Role::Para,
        };
        let mut done = Rewritten::default();
        let mut placed = Vec::new();
        for entry in entries {
            let kept_here = kept_marks(&entry, &self.packed.kept);
            let (text, code) = self.restore(&entry, None);
            if text.chars().any(char::is_alphanumeric) {
                let from = None;
                done.outs.push(Out::Text {
                    role,
                    text,
                    code,
                    from,
                });
            }
            for index in kept_here {
                if !placed.contains(&index) {
                    placed.push(index);
                    done.outs.push(Out::Kept(index));
                }
            }
        }
        let wrote = done.outs.iter().any(|o| matches!(o, Out::Text { .. }));
        wrote.then_some(done)
    }

    /// Puts inline placeholders back; missing ones go where they were.
    fn restore(&self, said: &str, from: Option<usize>) -> (String, Vec<Range<usize>>) {
        let mut text = String::new();
        let mut code = Vec::new();
        let mut used: Vec<&str> = Vec::new();
        let mut rest = said;
        while let Some(open) = rest.find("[[") {
            let (before, after) = rest.split_at(open);
            text.push_str(before);
            let Some(close) = after.find("]]") else {
                text.push_str(after);
                rest = "";
                break;
            };
            let inner = after.get(2..close).unwrap_or_default();
            let mark = format!("[[{}]]", inner.trim().to_lowercase());
            let found = self.packed.inline.iter().find(|i| i.mark == mark);
            if let Some(found) = found
                && !used.contains(&found.mark.as_str())
            {
                used.push(&found.mark);
                let start = text.len();
                text.push_str(&found.real);
                if found.code {
                    code.push(start..text.len());
                }
            }
            rest = after.get(close + 2..).unwrap_or_default();
        }
        text.push_str(rest);
        // A model that dropped one: back where it stood in the source.
        let source = match from.and_then(|i| self.job.pieces.get(i)) {
            Some(Piece::Text { text, .. }) => text.as_str(),
            _ => "",
        };
        let mine = self.packed.inline.iter().filter(|i| Some(i.piece) == from);
        for lost in mine.filter(|i| !used.contains(&i.mark.as_str())) {
            let found = source.find(lost.real.as_str()).unwrap_or(0);
            let at = word_break(&text, found as f32 / source.len().max(1) as f32);
            let piece = format!(" {} ", lost.real);
            text.insert_str(at, &piece);
            shift(&mut code, at, piece.len());
            if lost.code {
                code.push(at + 1..at + 1 + lost.real.len());
            }
        }
        tidy(text, code)
    }
}

fn original(role: Role, text: &str, code: &[Range<usize>], index: usize) -> Out {
    Out::Text {
        role,
        text: text.to_owned(),
        code: code.to_vec(),
        from: Some(index),
    }
}

/// The kept pieces an entry names, by `[[kN]]`.
fn kept_marks(entry: &str, kept: &[usize]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut rest = entry;
    while let Some(open) = rest.find("[[") {
        let after = rest.get(open + 2..).unwrap_or_default();
        let Some(close) = after.find("]]") else {
            break;
        };
        let inner = after.get(..close).unwrap_or_default().trim().to_lowercase();
        let n = inner
            .strip_prefix('k')
            .and_then(|n| n.parse::<usize>().ok());
        if let Some(index) = n.and_then(|n| kept.get(n.wrapping_sub(1))) {
            out.push(*index);
        }
        rest = after.get(close + 2..).unwrap_or_default();
    }
    out
}

/// The byte nearest `share` of the way through, at a space.
fn word_break(text: &str, share: f32) -> usize {
    let goal = (text.len() as f32 * share.clamp(0.0, 1.0)) as usize;
    if goal == 0 {
        return 0;
    }
    let spaces = text.match_indices(' ').map(|(i, _)| i).chain([text.len()]);
    spaces
        .min_by_key(|i| i.abs_diff(goal))
        .unwrap_or(text.len())
}

fn shift(code: &mut [Range<usize>], at: usize, by: usize) {
    for r in code.iter_mut().filter(|r| r.start >= at) {
        *r = r.start + by..r.end + by;
    }
}

/// One space between words, ranges kept on their characters.
fn tidy(text: String, code: Vec<Range<usize>>) -> (String, Vec<Range<usize>>) {
    let mut out = String::with_capacity(text.len());
    let mut map = vec![0usize; text.len() + 1];
    let mut space = false;
    for (i, ch) in text.char_indices() {
        if let Some(slot) = map.get_mut(i) {
            *slot = out.len();
        }
        if ch.is_whitespace() {
            space = !out.is_empty();
            continue;
        }
        if space {
            out.push(' ');
            space = false;
        }
        if let Some(slot) = map.get_mut(i) {
            *slot = out.len();
        }
        out.push(ch);
    }
    if let Some(last) = map.last_mut() {
        *last = out.len();
    }
    let at = |i: usize| map.get(i).copied().unwrap_or(out.len()).min(out.len());
    let code = code
        .into_iter()
        .map(|r| at(r.start)..at(r.end))
        .filter(|r| r.start < r.end);
    let code = code.collect();
    (out, code)
}

/// `n` and text pairs, from the shapes models answer in.
fn pieces_of(answer: &Value) -> Vec<(usize, String)> {
    let list = match answer {
        Value::Array(items) => Some(items),
        Value::Object(o) => field(o, &["pieces", "items", "translations", "result"])
            .and_then(Value::as_array)
            .or_else(|| o.values().find_map(Value::as_array)),
        _ => None,
    };
    let mut out = Vec::new();
    for (i, item) in list.map_or(&[][..], Vec::as_slice).iter().enumerate() {
        match item {
            Value::Object(o) => {
                let n = field(o, &["n", "number", "id", "index"]).and_then(number);
                let text = field(o, &["text", "translation", "rewrite", "content"]);
                if let Some(text) = text.and_then(Value::as_str) {
                    out.push((n.unwrap_or(i + 1), text.trim().to_owned()));
                }
            }
            Value::String(s) => out.push((i + 1, s.trim().to_owned())),
            _ => {}
        }
    }
    out
}

/// A list of strings under one of `keys`, or a lone string split by lines.
fn strings_of(answer: &Value, keys: &[&str]) -> Vec<String> {
    let found = match answer {
        Value::Object(o) => field(o, keys).or_else(|| o.values().next()),
        other => Some(other),
    };
    let items: Vec<String> = match found {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        Some(Value::String(s)) => s.split("\n\n").map(str::to_owned).collect(),
        _ => Vec::new(),
    };
    let items = items.into_iter().map(|s| s.trim().to_owned());
    items.filter(|s| !s.is_empty()).collect()
}

fn number(v: &Value) -> Option<usize> {
    match v {
        Value::Number(n) => n.as_u64().and_then(|n| usize::try_from(n).ok()),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn field<'a>(o: &'a Map<String, Value>, names: &[&str]) -> Option<&'a Value> {
    names.iter().find_map(|name| {
        o.iter()
            .find(|(k, _)| k.trim().eq_ignore_ascii_case(name))
            .map(|(_, v)| v)
    })
}

impl Task for PageAsk {
    type Output = Rewritten;

    fn prompt(&self) -> Prompt {
        let head = match self.job.rewrite.piecewise() {
            true => {
                "Pieces, one per line: number, kind (h1 heading, p paragraph, li list \
item, q quote), then the text."
            }
            false => {
                "Pieces, one per line: number, kind, then the text; [[k]] lines are \
kept items."
            }
        };
        Prompt {
            name: match &self.job.rewrite {
                Rewrite::Summarize(SummaryStyle::Paragraphs) => "page_summary",
                Rewrite::Summarize(SummaryStyle::Points) => "page_points",
                _ => "page_pieces",
            },
            system: self.system(),
            user: format!("{head}\n{}", self.packed.lines.join("\n")),
            schema: self.schema(),
        }
    }

    fn read(&self, answer: &Value, whole: bool) -> Option<Rewritten> {
        match self.job.rewrite.piecewise() {
            true => self.read_pieces(answer, whole),
            false => self.read_summary(answer),
        }
    }
}

/// The result, asked again after unreadable answers and brief outages.
pub async fn rewrite(
    backend: &dyn Backend,
    ask: &PageAsk,
    partial: &(dyn Fn(Rewritten) + Sync),
    retry: Retry,
) -> Result<Rewritten, Failure> {
    if ask.job.is_empty() {
        return Err(Failure::detailed(Why::NotHere, "this page has no text"));
    }
    attempt(backend, ask, partial, retry).await
}

/// FNV-1a: tiny, stable across builds, enough to spot the same page.
struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 ^= u64::from(*b);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }
}

#[cfg(test)]
#[path = "page_tests.rs"]
mod tests;
