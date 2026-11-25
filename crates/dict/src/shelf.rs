//! The packs on disk, asked in place: a card's lookup and a search.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::ai::Saved;
use crate::ai_store::{AiStore, file};
use crate::entry::{Entry, order};
use crate::lang;
use crate::packs::{PACKS, Pack};
use crate::route::{HUB, Pair, Plan, languages, plans, settle};
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
        }
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
        reply
    }

    /// The AI meanings kept for `words` in `lang`.
    fn remembered(&mut self, words: &[String], lang: &str) -> Reply {
        let mut reply = Reply::new(0);
        let found = self.ai().and_then(|ai| {
            let each = words.iter().map(|w| ai.word(w, lang));
            each.collect::<Result<Vec<_>, _>>()
        });
        match found {
            Ok(lists) => reply.saved = lists.into_iter().flatten().collect(),
            Err(why) => reply.failed = Some(why),
        }
        reply
    }

    /// A card's ask: from the word's language to `to`, else another.
    pub fn look_up(&mut self, word: &str, to: Option<&str>) -> Reply {
        let ready = self.ready();
        let langs = languages(&ready);
        let from = lang::detect(word, &langs).unwrap_or(HUB);
        let to = to
            .filter(|t| *t != from && langs.contains(t))
            .or_else(|| langs.iter().copied().find(|l| *l != from));
        let mut reply = Reply::new(ready.len());
        let Some(to) = to else {
            return reply;
        };
        (reply.from, reply.to) = (Some(from.to_owned()), Some(to.to_owned()));
        for plan in plans(from, to, &ready) {
            let rows = self.run(&plan, word, Depth::Card, CARD_ROWS, &mut reply.failed);
            if !rows.is_empty() {
                reply.entries = rows;
                break;
            }
        }
        order(&mut reply.entries, CARD_ROWS);
        reply
    }

    /// Each word's rows in each language of `to` a pack has.
    pub fn words(&mut self, words: &[String], to: &[String], near: bool) -> Reply {
        let ready = self.ready();
        let mut reply = Reply::new(ready.len());
        for lang in to {
            let pack = ready.iter().find(|p| p.source == HUB && p.target == lang);
            let Some(pack) = pack.copied() else {
                continue;
            };
            let Some(store) = self.store(pack) else {
                continue;
            };
            for word in words {
                let rows = |r: Result<Vec<Row>, String>, failed: &mut Option<String>| {
                    let mut entries: Vec<Entry> = r
                        .unwrap_or_else(|e| {
                            *failed = Some(e);
                            Vec::new()
                        })
                        .into_iter()
                        .map(|r| entry(r, pack, false, None))
                        .collect();
                    order(&mut entries, WORD_ROWS);
                    entries
                };
                let asks = std::slice::from_ref(word);
                let own = rows(
                    store.exact_any(Side::Word, asks, WORD_ROWS),
                    &mut reply.failed,
                );
                let near = match near {
                    true => rows(store.near(word, WORD_ROWS * 2), &mut reply.failed),
                    false => Vec::new(),
                };
                reply.found.push(crate::Found {
                    word: word.clone(),
                    to: lang.clone(),
                    own,
                    near,
                });
            }
        }
        reply
    }

    /// The panel's ask: between a pair of languages, or across packs.
    pub fn search(&mut self, text: &str, pair: Option<&Pair>, pack: Option<&str>) -> Reply {
        let ready = self.ready();
        let mut reply = Reply::new(ready.len());
        if fold(text).is_empty() {
            return reply;
        }
        if let Some(pair) = pair {
            let Some((from, to)) = settle(pair, text, &ready) else {
                return reply;
            };
            for plan in plans(&from, &to, &ready) {
                let rows = self.run(&plan, text, Depth::Search, SEARCH_ROWS, &mut reply.failed);
                if !rows.is_empty() {
                    reply.entries = rows;
                    break;
                }
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
        }
        order(&mut reply.entries, SEARCH_ROWS);
        reply
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
    }
}
