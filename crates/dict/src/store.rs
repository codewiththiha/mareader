//! One converted pack: an SQLite table asked by headword or definition.

use std::path::Path;

use rusqlite::{Connection, OpenFlags, params_from_iter};

use crate::search::{WordMatch, edit_distance, fold};

/// The table every pack becomes; the layout's version is kept in `meta`.
pub(crate) const SCHEMA: &str = "
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE entries (
    id INTEGER PRIMARY KEY,
    word TEXT NOT NULL,
    word_fold TEXT NOT NULL,
    pos TEXT,
    definition TEXT NOT NULL,
    def_fold TEXT NOT NULL,
    romanization TEXT,
    sense TEXT
);";
pub(crate) const INDEXES: &str = "
CREATE INDEX entries_word ON entries(word_fold);
CREATE INDEX entries_def ON entries(def_fold);";
pub(crate) const LAYOUT: &str = "1";

/// Near spellings are sought among this many same-letter rows at most.
const FUZZY_POOL: usize = 20_000;
/// Above every character, to bound a prefix range on the index.
const TOP: char = '\u{10FFFF}';

/// Which side of a row is asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Word,
    Definition,
}

impl Side {
    fn column(self) -> &'static str {
        match self {
            Self::Word => "word_fold",
            Self::Definition => "def_fold",
        }
    }
}

/// How far an ask reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Depth {
    /// A card: exact rows, else ones starting so, else near spellings.
    Card,
    /// A search: exact, starting so and containing, then near ones.
    Search,
}

/// A row as read, with how the asked side fit.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub word: String,
    pub pos: Option<String>,
    pub definition: String,
    pub romanization: Option<String>,
    pub sense: Option<String>,
    pub fit: WordMatch,
}

impl Row {
    /// The side `side` names.
    pub fn side(&self, side: Side) -> &str {
        match side {
            Side::Word => &self.word,
            Side::Definition => &self.definition,
        }
    }
}

pub struct Store {
    conn: Connection,
}

/// Whether `path` holds a finished pack in this layout.
pub fn usable(path: &Path) -> bool {
    Store::open(path).is_ok_and(|s| s.rows().is_ok_and(|n| n > 0))
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, String> {
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let conn = Connection::open_with_flags(path, flags).map_err(|e| e.to_string())?;
        let store = Self { conn };
        match store.meta("layout").as_deref() {
            Some(LAYOUT) => Ok(store),
            _ => Err("not a dictionary in this layout".into()),
        }
    }

    fn meta(&self, key: &str) -> Option<String> {
        let sql = "SELECT value FROM meta WHERE key = ?1";
        self.conn.query_row(sql, [key], |r| r.get(0)).ok()
    }

    /// The rows the pack holds, as counted when it was built.
    pub fn rows(&self) -> Result<u64, String> {
        let n = self.meta("rows").ok_or("no row count")?;
        n.parse().map_err(|_| "a bad row count".into())
    }

    /// Rows whose `side` fits `ask`, tightest first, at most `limit`.
    pub fn ask(
        &self,
        side: Side,
        ask: &str,
        depth: Depth,
        limit: usize,
    ) -> Result<Vec<Row>, String> {
        let ask = fold(ask);
        let mut out = Vec::new();
        if ask.is_empty() || limit == 0 {
            return Ok(out);
        }
        let col = side.column();
        let upper = format!("{ask}{TOP}");
        let exact = format!("{col} = ?1");
        self.collect(&exact, &[&ask], WordMatch::Exact, limit, &mut out)?;
        let tight = depth == Depth::Search || out.is_empty();
        if tight {
            let prefix = format!("{col} > ?1 AND {col} < ?2");
            self.collect(&prefix, &[&ask, &upper], WordMatch::Prefix, limit, &mut out)?;
        }
        if depth == Depth::Search && out.len() < limit {
            let like = format!("%{}%", escape(&ask));
            let inner = format!("{col} LIKE ?1 ESCAPE '\\' AND NOT ({col} >= ?2 AND {col} < ?3)");
            let args: [&str; 3] = [&like, &ask, &upper];
            self.collect(&inner, &args, WordMatch::Substring, limit, &mut out)?;
        }
        if out.is_empty() {
            self.fuzzy(side, &ask, limit, &mut out)?;
        }
        Ok(out)
    }

    /// Stems and family words of `ask`; near spellings when neither shows.
    pub fn near(&self, ask: &str, limit: usize) -> Result<Vec<Row>, String> {
        let ask = fold(ask);
        let mut out = Vec::new();
        if ask.chars().count() < 3 || limit == 0 {
            return Ok(out);
        }
        let mut stems = self.exact_any(Side::Word, &stems(&ask), limit)?;
        stems.iter_mut().for_each(|r| r.fit = WordMatch::Fuzzy);
        out.append(&mut stems);
        let col = Side::Word.column();
        let upper = format!("{ask}{TOP}");
        let longest = ask.chars().count() + FAMILY_REACH;
        let family = format!("{col} > ?1 AND {col} < ?2 AND length({col}) <= {longest}");
        self.collect(&family, &[&ask, &upper], WordMatch::Prefix, limit, &mut out)?;
        if out.is_empty() && ask.chars().count() >= 5 {
            self.fuzzy(Side::Word, &ask, limit, &mut out)?;
            out.retain(|r| fold(&r.word) != ask);
        }
        Ok(out)
    }

    /// Rows whose `side` is exactly one of `asks`: a bridge's second hop.
    pub fn exact_any(&self, side: Side, asks: &[String], limit: usize) -> Result<Vec<Row>, String> {
        let folds: Vec<String> = asks
            .iter()
            .map(|a| fold(a))
            .filter(|a| !a.is_empty())
            .collect();
        let mut out = Vec::new();
        // SQLite allows 32,766 parameters; a bridge never needs many.
        for chunk in folds.chunks(500) {
            let marks = vec!["?"; chunk.len()].join(", ");
            let filter = format!("{} IN ({marks})", side.column());
            let args: Vec<&str> = chunk.iter().map(String::as_str).collect();
            self.collect(&filter, &args, WordMatch::Exact, limit, &mut out)?;
        }
        Ok(out)
    }

    fn collect(
        &self,
        filter: &str,
        args: &[&str],
        fit: WordMatch,
        limit: usize,
        out: &mut Vec<Row>,
    ) -> Result<(), String> {
        let room = limit.saturating_sub(out.len());
        if room == 0 {
            return Ok(());
        }
        let sql = format!(
            "SELECT word, pos, definition, romanization, sense FROM entries \
             WHERE {filter} ORDER BY id LIMIT {room}"
        );
        let mut stmt = self.conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params_from_iter(args), |r| {
                Ok(Row {
                    word: r.get(0)?,
                    pos: r.get::<_, Option<String>>(1)?.filter(|s| !s.is_empty()),
                    definition: r.get(2)?,
                    romanization: r.get::<_, Option<String>>(3)?.filter(|s| !s.is_empty()),
                    sense: r.get::<_, Option<String>>(4)?.filter(|s| !s.is_empty()),
                    fit,
                })
            })
            .map_err(|e| e.to_string())?;
        for row in rows {
            out.push(row.map_err(|e| e.to_string())?);
        }
        Ok(())
    }

    /// Near spellings: same first letter, length within two, two edits.
    fn fuzzy(&self, side: Side, ask: &str, limit: usize, out: &mut Vec<Row>) -> Result<(), String> {
        let len = ask.chars().count();
        let Some(first) = ask.chars().next().filter(|_| len >= 3) else {
            return Ok(());
        };
        let col = side.column();
        let (lo, hi) = (first.to_string(), format!("{first}{TOP}"));
        // Lengths go in as numbers: bound as text they never compare equal.
        let (short, long) = (len - 2, len + 2);
        let filter =
            format!("{col} >= ?1 AND {col} < ?2 AND length({col}) BETWEEN {short} AND {long}");
        let mut pool = Vec::new();
        let args: [&str; 2] = [&lo, &hi];
        self.collect(&filter, &args, WordMatch::Fuzzy, FUZZY_POOL, &mut pool)?;
        let mut near: Vec<(usize, Row)> = pool
            .into_iter()
            .map(|r| (edit_distance(ask, &fold(r.side(side)), 2), r))
            .filter(|(d, _)| *d <= 2)
            .collect();
        near.sort_by_key(|(d, _)| *d);
        out.extend(near.into_iter().map(|(_, r)| r).take(limit));
        Ok(())
    }
}

/// A family word is at most this many letters longer than the word.
const FAMILY_REACH: usize = 4;

/// What an English word may come from: "running" from "run".
pub fn stems(word: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |s: String| {
        if s.chars().count() >= 2 && s != word && !out.contains(&s) {
            out.push(s);
        }
    };
    for (suffix, with) in [("ies", "y"), ("ied", "y"), ("ier", "y"), ("iest", "y")] {
        if let Some(base) = word.strip_suffix(suffix) {
            add(format!("{base}{with}"));
        }
    }
    for suffix in [
        "s", "es", "ed", "d", "ing", "ly", "er", "est", "ness", "ment",
    ] {
        let Some(base) = word.strip_suffix(suffix) else {
            continue;
        };
        add(base.to_owned());
        if matches!(suffix, "ed" | "ing" | "er" | "est") {
            add(format!("{base}e"));
            // "running" is "run": a doubled last letter is one.
            let mut chars = base.chars().rev();
            if let (Some(a), Some(b)) = (chars.next(), chars.next())
                && a == b
            {
                add(base.chars().take(base.chars().count() - 1).collect());
            }
        }
    }
    out
}

/// `ask` with LIKE's wildcards and the escape itself taken literally.
pub(crate) fn escape(ask: &str) -> String {
    let mut out = String::with_capacity(ask.len());
    for c in ask.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::build::build;

    fn fixture(name: &str) -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("t.db");
        let src = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        build(&src, &db).unwrap();
        let store = Store::open(&db).unwrap();
        (dir, store)
    }

    #[test]
    fn a_card_takes_exact_rows_and_only_then_reaches_further() {
        let (_d, s) = fixture("en-my.parquet");
        let run = s.ask(Side::Word, "RUN", Depth::Card, 10).unwrap();
        assert!(
            run.iter()
                .all(|r| r.fit == WordMatch::Exact && fold(&r.word) == "run")
        );
        assert_eq!(run.len(), 4);
        let runn = s.ask(Side::Word, "runn", Depth::Card, 10).unwrap();
        assert_eq!(
            runn.iter().map(|r| r.word.as_str()).collect::<Vec<_>>(),
            ["runner"]
        );
        let typo = s.ask(Side::Word, "colur", Depth::Card, 10).unwrap();
        assert_eq!(typo.first().map(|r| r.fit), Some(WordMatch::Fuzzy));
        assert!(
            s.ask(Side::Word, "zzzz", Depth::Card, 10)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_search_reaches_inside_words_and_wildcards_stay_text() {
        let (_d, s) = fixture("en-fr.parquet");
        let got = s.ask(Side::Word, "ight", Depth::Search, 10).unwrap();
        assert!(
            got.iter()
                .all(|r| r.word == "light" && r.fit == WordMatch::Substring)
        );
        let pct = s.ask(Side::Word, "0%", Depth::Search, 10).unwrap();
        assert_eq!(pct.len(), 1);
        assert_eq!(s.ask(Side::Word, "%", Depth::Search, 10).unwrap().len(), 1);
        let back = s.ask(Side::Definition, "Lumière", Depth::Card, 10).unwrap();
        assert_eq!(back.first().map(|r| r.word.as_str()), Some("light"));
        let any = s
            .exact_any(Side::Word, &["quick".into(), "run".into()], 10)
            .unwrap();
        assert_eq!(any.len(), 3);
    }

    #[test]
    fn stems_undo_common_endings() {
        assert_eq!(stems("running"), ["runn", "runne", "run"]);
        assert!(stems("obfuscated").contains(&"obfuscate".to_owned()));
        assert!(stems("studies").contains(&"study".to_owned()));
        assert!(stems("quickly").contains(&"quick".to_owned()));
        assert!(stems("is").is_empty());
    }

    #[test]
    fn near_rows_are_stems_and_family_never_the_word() {
        let (_d, s) = fixture("en-my.parquet");
        let near = s.near("running", 10).unwrap();
        assert!(near.iter().any(|r| r.word == "run"), "{near:?}");
        let family = s.near("run", 10).unwrap();
        assert!(family.iter().any(|r| r.word == "runner"), "{family:?}");
        assert!(family.iter().all(|r| fold(&r.word) != "run"));
        let typo = s.near("colours", 10).unwrap();
        assert!(!typo.is_empty(), "a stem or a near spelling");
    }

    #[test]
    fn a_file_of_another_kind_is_not_usable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.db");
        assert!(!usable(&path));
        std::fs::write(&path, b"not sqlite").unwrap();
        assert!(!usable(&path));
    }
}
