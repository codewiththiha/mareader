//! The packs on disk, asked in place: a card's lookup and a search.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::ai::{AI_PACK, Saved};
use crate::ai_store::{AiStore, file};
use crate::entry::{Entry, order};
use crate::lang;
use crate::packs::{PACKS, Pack};
use crate::route::{HUB, Pair, Plan, languages, languages_with, plans, settle};
use crate::search::{WordMatch, fold};
use crate::store::{Depth, Row, Side, Store};
use crate::{Query, Reply};

/// A card shows at most this many rows; a search lists more.
pub const CARD_ROWS: usize = 24;
pub const SEARCH_ROWS: usize = 60;
/// A word list keeps at most this many rows per word and language.
pub const WORD_ROWS: usize = 8;

/// The converted pack's file.
pub fn db_file(dir: &Path, pack: &Pack) -> PathBuf {
    dir.join(format!("{}.db", pack.id))
}

/// The download, while it comes and once whole.
pub fn part_file(dir: &Path, pack: &Pack) -> PathBuf {
    dir.join(format!("{}.parquet.part", pack.id))
}

/// The table while it is built, renamed when whole.
pub fn building_file(dir: &Path, pack: &Pack) -> PathBuf {
    dir.join(format!("{}.db.part", pack.id))
}

pub struct Shelf {
    dir: PathBuf,
    open: HashMap<&'static str, Store>,
    ai: Option<AiStore>,
}

impl Shelf {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            open: HashMap::new(),
            ai: None,
        }
    }

    /// The packs converted and readable, in offer order.
    pub fn ready(&mut self) -> Vec<&'static Pack> {
        PACKS.iter().filter(|p| self.store(p).is_some()).collect()
    }

    fn store(&mut self, pack: &'static Pack) -> Option<&Store> {
        if !self.open.contains_key(pack.id) {
            let path = db_file(&self.dir, pack);
            let store = path.is_file().then(|| Store::open(&path).ok()).flatten()?;
            self.open.insert(pack.id, store);
        }
        self.open.get(pack.id)
    }

    /// Lets go of a pack's file, so it can be deleted.
    pub fn forget(&mut self, pack: &Pack) {
        self.open.remove(pack.id);
    }

    pub fn answer(&mut self, query: &Query) -> Reply {
        match query {
            Query::LookUp { word, to } => self.look_up(word, to.as_deref()),
            Query::Search { text, pair, pack } => self.search(text, pair.as_ref(), pack.as_deref()),
            Query::Words { words, to, near } => self.words(words, to, *near),
            Query::Remember { saved, replace } => self.remember(saved, *replace),
            Query::Remembered { words, lang } => self.remembered(words, lang),
            Query::Forget { saved } => self.forget_ai(saved),
            Query::Edit { old, new, replace } => self.edit_ai(old, new, *replace),
        }
    }

    /// Drops an AI meaning; the reply names the languages left.
    fn forget_ai(&mut self, saved: &Saved) -> Reply {
        let mut reply = Reply::new(0);
        if let Err(e) = self.ai().and_then(|ai| ai.forget(saved)) {
            reply.failed = Some(e);
        }
        reply.ai_langs = Some(self.ai_langs());
        reply
    }

    /// Writes an edit; a meaning already under the new key comes back.
    fn edit_ai(&mut self, old: &Saved, new: &Saved, replace: bool) -> Reply {
        let mut reply = Reply::new(0);
        match self.ai().and_then(|ai| ai.edit(old, new, replace)) {
            Ok(kept) => reply.saved.extend(kept),
            Err(e) => reply.failed = Some(e),
        }
        reply.ai_langs = Some(self.ai_langs());
        reply
    }

    /// The AI dictionary, opened or made on first use.
    fn ai(&mut self) -> Result<&AiStore, String> {
        if self.ai.is_none() {
            self.ai = Some(AiStore::open(&file(&self.dir))?);
        }
        let ai = self.ai.as_ref();
        ai.ok_or_else(|| "no AI dictionary".to_owned())
    }

    /// Keeps each meaning; the reply lists differing ones left in place.
    fn remember(&mut self, saved: &[Saved], replace: bool) -> Reply {
        let mut reply = Reply::new(0);
        let kept = self.ai().and_then(|ai| {
            let mut stayed = Vec::new();
            for one in saved {
                if ai.put(one, replace)? {
                    continue;
                }
                let old = ai.get(&one.word, &one.pos, &one.lang)?;
                stayed.extend(old.filter(|o| !o.same_as(one)));
            }
            Ok(stayed)
        });
        match kept {
            Ok(stayed) => reply.saved = stayed,
            Err(why) => reply.failed = Some(why),
        }
        reply.ai_langs = Some(self.ai_langs());
        reply
    }

    /// The AI meanings kept for `words` in `lang`.
    fn remembered(&mut self, words: &[String], lang: &str) -> Reply {
        let mut reply = Reply::new(0);
        let found = self.ai().and_then(|ai| {
            let each = words.iter().map(|w| ai.word(w, Some(lang)));
            each.collect::<Result<Vec<_>, _>>()
        });
        match found {
            Ok(lists) => reply.saved = lists.into_iter().flatten().collect(),
            Err(why) => reply.failed = Some(why),
        }
        reply
    }

    /// A card's ask: from the word's language to `to`, else another.
    ///
    /// For an English word the AI dictionary is one more dictionary: its
    /// entries in `to` follow the packs' rows, and a language only it
    /// keeps, English included, is asked like a pack's.
    pub fn look_up(&mut self, word: &str, to: Option<&str>) -> Reply {
        let ready = self.ready();
        let langs = languages(&ready);
        let from = lang::detect(word, &langs).unwrap_or(HUB);
        let ai = if from == HUB {
            self.ai_langs()
        } else {
            Vec::new()
        };
        let in_ai = |t: &str| ai.iter().any(|l| l == t);
        let to = to
            .filter(|t| (*t != from && langs.contains(t)) || in_ai(t))
            .or_else(|| langs.iter().copied().find(|l| *l != from))
            .or_else(|| ai.first().map(String::as_str))
            .map(str::to_owned);
        let mut reply = Reply::new(ready.len() + usize::from(!ai.is_empty()));
        let Some(to) = to else {
            return reply;
        };
        for plan in plans(from, &to, &ready) {
            let rows = self.run(&plan, word, Depth::Card, CARD_ROWS, &mut reply.failed);
            if !rows.is_empty() {
                reply.entries = rows;
                break;
            }
        }
        order(&mut reply.entries, CARD_ROWS);
        if in_ai(&to) {
            let kept = self.ai().and_then(|s| s.word(word, Some(to.as_str())));
            let kept = kept.unwrap_or_default();
            reply.entries.extend(kept.iter().map(ai_entry));
        }
        reply.ai_langs = (from == HUB).then_some(ai);
        (reply.from, reply.to) = (Some(from.to_owned()), Some(to));
        reply
    }

    /// The AI dictionary's languages; none before it was first written to.
    fn ai_langs(&mut self) -> Vec<String> {
        if self.ai.is_none() && !file(&self.dir).is_file() {
            return Vec::new();
        }
        let langs = self.ai().and_then(AiStore::langs);
        langs.unwrap_or_default()
    }

    /// Each word's rows in each language of `to` a pack or the AI
    /// dictionary has; a pack's rows come first.
    pub fn words(&mut self, words: &[String], to: &[String], near: bool) -> Reply {
        let ready = self.ready();
        let ai = self.ai_langs();
        let mut reply = Reply::new(ready.len() + usize::from(!ai.is_empty()));
        for lang in to {
            let pack = ready.iter().find(|p| p.source == HUB && p.target == lang);
            let pack = pack.copied();
            let kept = ai.contains(lang);
            if pack.is_none() && !kept {
                continue;
            }
            for word in words {
                let (mut own, close) = match pack {
                    Some(pack) => self.pack_words(pack, word, near, &mut reply.failed),
                    None => (Vec::new(), Vec::new()),
                };
                if kept {
                    let said = self.ai().and_then(|s| s.word(word, Some(lang.as_str())));
                    own.extend(said.unwrap_or_default().iter().map(ai_entry));
                }
                reply.found.push(crate::Found {
                    word: word.clone(),
                    to: lang.clone(),
                    own,
                    near: close,
                });
            }
        }
        reply.ai_langs = Some(ai);
        reply
    }

    /// A word's rows in `pack`, and with `near` those of words near it.
    fn pack_words(
        &mut self,
        pack: &'static Pack,
        word: &str,
        near: bool,
        failed: &mut Option<String>,
    ) -> (Vec<Entry>, Vec<Entry>) {
        let Some(store) = self.store(pack) else {
            return (Vec::new(), Vec::new());
        };
        let mut rows = |r: Result<Vec<Row>, String>| {
            let rows = r.unwrap_or_else(|e| {
                *failed = Some(e);
                Vec::new()
            });
            let mut entries: Vec<Entry> = rows
                .into_iter()
                .map(|r| entry(r, pack, false, None))
                .collect();
            order(&mut entries, WORD_ROWS);
            entries
        };
        let own = rows(store.exact_any(Side::Word, &[word.to_owned()], WORD_ROWS));
        let near = match near {
            true => rows(store.near(word, WORD_ROWS * 2)),
            false => Vec::new(),
        };
        (own, near)
    }

    /// The panel's ask: between a pair of languages, or across packs; the
    /// AI dictionary answers after them.
    pub fn search(&mut self, text: &str, pair: Option<&Pair>, pack: Option<&str>) -> Reply {
        let ready = self.ready();
        let ai = self.ai_langs();
        let mut reply = Reply::new(ready.len() + usize::from(!ai.is_empty()));
        if fold(text).is_empty() {
            return reply;
        }
        if let Some(pair) = pair {
            let langs = languages_with(&ready, &ai);
            let Some((from, to)) = settle(pair, text, &langs) else {
                return reply;
            };
            for plan in plans(&from, &to, &ready) {
                let rows = self.run(&plan, text, Depth::Search, SEARCH_ROWS, &mut reply.failed);
                if !rows.is_empty() {
                    reply.entries = rows;
                    break;
                }
            }
            let side = ai_side(&from, &to).filter(|(l, _)| ai.iter().any(|a| a == l));
            if let Some((lang, meaning)) = side {
                let rows = self.ai_found(text, Some(lang), meaning, &mut reply.failed);
                reply.entries.extend(rows);
            }
            (reply.from, reply.to) = (Some(from), Some(to));
        } else {
            let wanted = ready.iter().filter(|p| pack.is_none_or(|id| p.id == id));
            for p in wanted.copied().collect::<Vec<_>>() {
                for (side, flipped) in [(Side::Word, false), (Side::Definition, true)] {
                    let rows =
                        self.ask(p, side, text, Depth::Search, SEARCH_ROWS, &mut reply.failed);
                    reply
                        .entries
                        .extend(rows.into_iter().map(|r| entry(r, p, flipped, None)));
                }
            }
            let asks_ai = pack.is_none_or(|id| id == AI_PACK) && !ai.is_empty();
            for meaning in [false, true].into_iter().filter(|_| asks_ai) {
                let rows = self.ai_found(text, None, meaning, &mut reply.failed);
                reply.entries.extend(rows);
            }
        }
        order(&mut reply.entries, SEARCH_ROWS);
        reply
    }

    /// AI rows whose word, or with `meaning` meaning, holds `text`.
    fn ai_found(
        &mut self,
        text: &str,
        lang: Option<&str>,
        meaning: bool,
        failed: &mut Option<String>,
    ) -> Vec<Entry> {
        let found = self
            .ai()
            .and_then(|ai| ai.search(text, lang, meaning, SEARCH_ROWS));
        let found = found.unwrap_or_else(|e| {
            *failed = Some(e);
            Vec::new()
        });
        let row = |s: &Saved| Entry {
            fit: ai_fit(s, text, meaning),
            flipped: meaning,
            ..ai_entry(s)
        };
        found.iter().map(row).collect()
    }

    fn ask(
        &mut self,
        pack: &'static Pack,
        side: Side,
        text: &str,
        depth: Depth,
        limit: usize,
        failed: &mut Option<String>,
    ) -> Vec<Row> {
        let Some(store) = self.store(pack) else {
            return Vec::new();
        };
        store.ask(side, text, depth, limit).unwrap_or_else(|e| {
            *failed = Some(e);
            Vec::new()
        })
    }

    /// One plan's answers; a bridge asks for the first hop's English.
    fn run(
        &mut self,
        plan: &Plan,
        text: &str,
        depth: Depth,
        limit: usize,
        failed: &mut Option<String>,
    ) -> Vec<Entry> {
        let side = |reverse| {
            if reverse {
                Side::Definition
            } else {
                Side::Word
            }
        };
        let mut hops = plan.hops.iter();
        let Some(first) = hops.next() else {
            return Vec::new();
        };
        let rows = self.ask(
            first.pack,
            side(first.reverse),
            text,
            depth,
            limit * 2,
            failed,
        );
        let Some(second) = hops.next() else {
            return rows
                .into_iter()
                .map(|r| entry(r, first.pack, first.reverse, None))
                .collect();
        };
        // The English side of the first hop's rows.
        let far = side(!first.reverse);
        let mids: Vec<(String, WordMatch)> =
            rows.iter().map(|r| (fold(r.side(far)), r.fit)).collect();
        let asks: Vec<String> = mids.iter().map(|(m, _)| m.clone()).collect();
        let Some(store) = self.store(second.pack) else {
            return Vec::new();
        };
        let near = side(second.reverse);
        let finals = store.exact_any(near, &asks, limit).unwrap_or_else(|e| {
            *failed = Some(e);
            Vec::new()
        });
        finals
            .into_iter()
            .map(|mut r| {
                let mid = r.side(near).to_owned();
                r.fit = mids
                    .iter()
                    .find(|(m, _)| *m == fold(&mid))
                    .map_or(WordMatch::Fuzzy, |(_, f)| *f);
                entry(r, second.pack, second.reverse, Some(mid))
            })
            .collect()
    }
}

fn entry(r: Row, pack: &'static Pack, flipped: bool, via: Option<String>) -> Entry {
    Entry {
        word: r.word,
        pos: r.pos,
        definition: r.definition,
        romanization: r.romanization,
        sense: r.sense,
        pack: pack.id,
        via,
        fit: r.fit,
        flipped,
        ai: None,
    }
}

/// The side of the AI dictionary a pair asks: words from English, else
/// meanings back to it.
fn ai_side<'a>(from: &'a str, to: &'a str) -> Option<(&'a str, bool)> {
    match (from == HUB, to == HUB) {
        (true, false) => Some((to, false)),
        (false, true) => Some((from, true)),
        _ => None,
    }
}

/// How near `text` an AI row's word, or with `meaning` its meaning, comes.
fn ai_fit(saved: &Saved, text: &str, meaning: bool) -> WordMatch {
    let side = fold(if meaning { &saved.meaning } else { &saved.word });
    let ask = fold(text);
    if side == ask {
        WordMatch::Exact
    } else if side.starts_with(&ask) {
        WordMatch::Prefix
    } else {
        WordMatch::Substring
    }
}

/// An AI dictionary entry as a card row.
fn ai_entry(saved: &Saved) -> Entry {
    Entry {
        word: saved.word.clone(),
        pos: Some(saved.pos.clone()),
        definition: saved.meaning.clone(),
        romanization: None,
        sense: None,
        pack: AI_PACK,
        via: None,
        fit: WordMatch::Exact,
        flipped: false,
        ai: Some(Box::new(saved.clone())),
    }
}
