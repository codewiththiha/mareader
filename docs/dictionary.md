# The dictionary system

A word-level dictionary that ships as downloads, not as data in the binary.
Each language pair is one Parquet pack from
[`codewiththiha/wikidict`](https://github.com/codewiththiha/wikidict); the app
downloads a pack through `download-core`, rebuilds it into SQLite beside the
dataset database, and deletes the Parquet. The CEFR highlighter decides *which*
words are worth a lookup; the dictionary answers *what they mean*.

This document records the data, the part-of-speech problem across two different
tag vocabularies, the store, the English bridge, and the surfaces. Sections 1
and 2 are the shipped core, `crates/dictionary-core`; 3 to 5 are what it feeds.

## 1. The packs

Every pack has English in `word` and the target-language word in `definition`,
so one code path reads them all. Two schemas coexist:

| pack | pair | rows | bytes | columns |
| --- | --- | --- | --- | --- |
| `mcfnlp-en-my.parquet` | en → my | 110,640 | 2,875,539 | `word, pos, definition` |
| `jmdict-en-jp.parquet` | en → jp | 441,348 | 7,026,188 | `word, pos, definition` |
| `wiktionary-en-fr.parquet` | en → fr | 128,263 | 3,218,737 | `word, pos, definition, romanization, sense, lang_code, source` |

Mirrors, in order, exactly as the CEFR dataset does it — raw first (strong
ETag, ranges), jsDelivr second, `github.com/.../raw` third:

```
https://raw.githubusercontent.com/codewiththiha/wikidict/main/output/curated/<pack>.parquet
https://cdn.jsdelivr.net/gh/codewiththiha/wikidict@main/output/curated/<pack>.parquet
https://github.com/codewiththiha/wikidict/raw/main/output/curated/<pack>.parquet
```

All three hosts answer `206` to a byte range; only raw and jsDelivr are
byte-stable enough to resume against, and the transport already replaces a
partial whose validator changed, so a resume that crosses mirrors restarts
rather than splices.

`mcfnlp-en-my` carries no `romanization` (Myanmar transliteration is not in the
source), `jmdict-en-jp` carries none either (JMdict kana is in the definition
column), and `wiktionary-en-fr` carries the words without transliteration
(Latin script). Romanization is therefore shown when a pack has it, never
required.

Licences: Wiktionary-derived packs are CC BY-SA / GFDL, `jmdict-en-jp` is
JMdict (CC BY-SA, EDRDG), `mcfnlp-en-my` is the MCF NLP dictionary from
HuggingFace. The Settings section states the attribution; nothing is shipped in
the binary, so nothing is redistributed by the app.

Later pairs are the same shape: `wiktionary-en-jp`, `wiktionary-en-my`,
`wordnet-en-fr`, and the `reverse/eng-*-words` tables. The registry is a list,
not a match statement.

## 2. The part-of-speech problem

Two vocabularies meet here and neither is the other's.

### 2.1 What our NLP can identify

The tagger is `cefr-rs`'s (`nlprule`, LanguageTool's English model). It emits
Penn-style tags with suffixes stripped (`NN:U` → `NN`, `IN/that` → `IN`), and
`kind_of` folds them into **eleven readable kinds**:

| kind | tags it answers for |
| --- | --- |
| `verb` | `VB`, `VBD`, `VBG`, `VBN`, `VBP`, `VBZ` |
| `noun` | `NN`, `NNS`, `NNP`, `NNPS`, `NP*` |
| `adjective` | `JJ`, `JJR`, `JJS` |
| `adverb` | `RB`, `RBR`, `RBS` |
| `pronoun` | `PRP`, `PRP$`, `WP`, `WP$` |
| `preposition` | `IN`, `TO` |
| `conjunction` | `CC` |
| `number` | `CD` |
| `modal verb` | `MD` |
| `determiner` | `DT`, `PDT`, `WDT` |
| `other` | everything else (`SYM`, `UH`, `RP`, `FW`, `EX`, `LS`, `POS`, …) |

So the accurate answer to "what can ours detect": eleven kinds, out of the
thirty-six Penn tags the model can emit. The raw tag is kept beside the kind,
because it distinguishes what the kinds collapse (`NNS` against `NNP`,
`VBD` against `VBG`) and the dictionary card can show it.

### 2.2 What the packs carry

Fifty-four distinct `pos` values across the whole wikidict repo; the three app
packs use twenty-five, sixteen and twenty-one of them:

| pack | distinct tags | tags |
| --- | --- | --- |
| `mcfnlp-en-my` | 25 | `n v adj adv (empty) prep pron interj det advpart conj indefpron adjs ns indefdet indefadv suff symb pers pref abbr possess vs advs indefart` |
| `jmdict-en-jp` | 16 | `noun verb phrase adj adv intj suffix conj particle prefix pron aux counter num other cop` |
| `wiktionary-en-fr` | 21 | `noun adj verb name adv phrase prep_phrase intj prep proverb prefix pron suffix conj num det contraction particle article postp symbol` |

Three shapes of tag live in that list:

1. **Short forms** — `n`, `v`, `adj`, `adv`, `prep`, `pron`, `conj`, `det`,
   `intj`, `interj`, `num`, `suff`, `pref`, `symb`, `abbr`.
2. **Long forms** — `noun`, `verb`, `adjective`(never used, but the alias is
   kept), `adverb`(same), `suffix`, `prefix`, `symbol`, `particle`, `phrase`,
   `name`, `article`, `contraction`, `proverb`, `counter`, `aux`, `cop`.
3. **Subtypes and leftovers** — `adjs`, `advs`, `advpart`, `ns`, `vs`, `pers`,
   `possess`, `indefpron`, `indefdet`, `indefadv`, `indefart`, `prep_phrase`,
   `postp`, `adnominal`, `affix`, `combining_form`, `infix`, `interfix`,
   `root`, `syllable`, `character`, `punct`, `other` (used by the wider repo;
   the app's three packs use the subset above).

Nothing in the packs is authoritative about what `ns` or `vs` mean — the
sources disagree and one of them (MCF NLP) publishes no legend. So the mapping
is **ours, explicit, and total**: every observed tag answers a canonical kind,
and a tag nobody has seen still lands somewhere by morphology rather than
falling through to "unknown".

### 2.3 The alias table (hardcoded, both directions)

`dictionary-core::pos` owns one table. Left: every tag we have seen or expect
(short and long spelling of the same name, so a pack that switches spellings
does not change the answer). Right: the canonical kind it folds to.

```
n noun ns common noun n-adv n-pr n-t n-suf n-pref n-adj   -> noun
v verb vs vs-i vs-s vt vi phrasal verb infinitive         -> verb
aux auxiliary modal modal verb                            -> aux
cop copula                                                -> cop
adj adjective adjs adnominal adj-na adj-no adj-i adj-ix   -> adjective
adv adverb advs advpart adv-to                            -> adverb
pron pronoun pers indefpron pn                            -> pronoun
possess possessive                                        -> possessive
det determiner indefdet                                   -> determiner
article indefart                                          -> article
prep preposition                                          -> preposition
postp postposition                                        -> postposition
conj conjunction                                          -> conjunction
intj interj interjection int                              -> interjection
num number numeral                                        -> number
particle prt                                              -> particle
symb symbol punct punctuation                             -> symbol
abbr abbreviation                                         -> abbreviation
suff suffix                                               -> suffix
pref prefix                                               -> prefix
name proper noun                                          -> name
ctr counter classifier                                    -> counter
phrase prep_phrase proverb contraction idiom exp          -> phrase
character syllabogram syllable combining_form radical root -> character
unc unknown, and any literal "other"                      -> other
```

The rule for a tag nobody wrote down is morphology, never guesswork:
`adv*`/`*adv` → `adverb`, `adj*`/`*adj` → `adjective`, `int*` →
`interjection`, `*pron` → `pronoun`, `*det` → `determiner`, `*art` →
`article`, `*suff` → `suffix`, `*pref` → `prefix`, `*symb` → `symbol`,
`*abbr` → `abbreviation`; anything that still matches nothing folds to `other`
and keeps its raw string for display. That fallback is already proven on the
data: every tag in all six curated packs folds to a real kind, and the only
`other` rows are the 33 jmdict rows that literally say `other` and the 1,170
mcfnlp rows whose field is empty.

A tag may also be **several tags in one string** — `"v, n"`, `"n; adj"`,
`"verb/noun"`. Splitting happens on `,;|/` and each piece goes through the same
table, so a row that is both a verb and a noun is found under either, and the
card can name the one that matched.

### 2.4 Matching rules

The lookup is deliberately not strict about part of speech. Given a word and
(optionally) the NLP's kind for it in this sentence, each row is one of:

1. **Strong** — some tag of the row folds to a kind that *agrees* with the
   NLP's kind. Agreement is a set, not equality:

   | our kind | agrees with |
   | --- | --- |
   | `verb`, `modal verb` | `verb`, `aux`, `cop` |
   | `noun` | `noun`, `name`, `counter` |
   | `adjective` | `adjective`, `adnominal`→`adjective` |
   | `adverb` | `adverb` |
   | `pronoun` | `pronoun`, `possessive` |
   | `determiner` | `determiner`, `article`, `possessive` |
   | `preposition` | `preposition`, `postposition` |
   | `conjunction` | `conjunction` |
   | `number` | `number` |
   | `other` | nothing (it is the absence of a verdict) |

   The row's matching tag is what the card shows: a row tagged `v, n` shown
   for a verb displays **v**, and never "verb, noun" as a mush.
2. **Weak** — the word matches and no tag agrees, or there is no NLP kind at
   all (no POS model downloaded, or the tagger had no verdict for that word).
   Weak rows are shown *after* the strong ones, tagged with their own `pos`
   verbatim, and never hidden. This is what makes the dictionary useful with
   only a level pack installed: the CEFR dataset answers without the model.
3. **Fuzzy** — a search that finds no exact key falls back to the trigram
   index (substring and near miss), then to a prefix scan. The route searches
   that way; the hover card asks for the exact key and its near neighbours.

Order is: strong before weak, then exact key before fuzzy, then the pack's own
row order. Nothing is ever *rejected* for a POS mismatch — a wrong guess about
`ns` costs a rank, not an entry.

## 3. The store

One SQLite file per installed set of packs, built by the same shape of code as
the CEFR dataset: verify the Parquet (`PAR1` at both ends), rebuild on a
blocking worker into a temporary file, adopt it by rename, delete the Parquet.

```sql
CREATE TABLE entry (            -- one row per (word, pos, sense) of a pack
  id       INTEGER PRIMARY KEY,
  pack     TEXT NOT NULL,       -- 'en-my', 'en-jp', 'en-fr'
  word     TEXT NOT NULL,       -- the pack's key column (English)
  key      TEXT NOT NULL,       -- folded key: lowercased, trimmed, ASCII-folded
  pos      TEXT NOT NULL,       -- raw, as the pack wrote it
  kind     TEXT NOT NULL,       -- canonical kinds, space-joined
  gloss    TEXT NOT NULL,       -- the target-language word
  roman    TEXT, sense TEXT, note TEXT
);
CREATE TABLE rev (              -- the same rows, keyed the other way
  id       INTEGER PRIMARY KEY,
  entry    INTEGER NOT NULL REFERENCES entry(id),
  pack     TEXT NOT NULL,
  key      TEXT NOT NULL,       -- folded target-language word
  kind     TEXT NOT NULL
);
CREATE VIRTUAL TABLE search USING fts5(
  key, gloss, content='entry', content_rowid='id', tokenize='trigram');
```

* `rev` is what makes a Myanmar word findable at all — the packs are English →
  target, so a reverse row per entry (target → entry) is built during the same
  pass, with no extra download. It is slim (key, kind, id) so it costs about a
  quarter of `entry`.
* `search` is the fuzzy half: a trigram index over both sides, used when a
  query is longer than two characters and exact keys miss.
* `PRAGMA user_version` carries the builder's revision, exactly as the CEFR
  database does, so a stale file is rebuilt instead of read.

## 4. The English bridge

The user picks an input language and an output language. When a direct pack
exists, one table answers it. When it does not — Myanmar → Japanese, with only
`en-my` and `en-jp` installed — the query goes through English, in three steps
that each stay inside the store:

1. `rev` for the input word in the input language → the English key(s) whose
   gloss is that word. (English is in `word`, the pair's own column.)
2. `entry` for those English keys in the output language → the target words.
3. Rank: the same `kind` on both hops first, then the sense/romanization the
   middle row carried, then the input's own position.

The bridge is **conditional and honest**: it runs only when both `en-<input>`
and `en-<output>` are installed, and when it runs, the card says so ("via
English") with the middle word shown. When a hop is missing the route says
which pack is missing and offers the download, rather than silently answering
nothing.

## 5. The surfaces

| surface | what it is |
| --- | --- |
| Settings → Dictionary | one row per pack: bytes, live progress, pause / resume / cancel / remove (the CEFR dataset row, reused), plus the display-language choice, "look up on hover", and attribution |
| Hover card | the AI card's sibling, on the CEFR mark's hover: word, POS, gloss, romanization, paged through every match; a language switch in its title |
| Dictionary route | the library's shape for words: quick search, fuzzy search, language and POS filters, results in the chosen output language |
| Floating overlay | the same search in a resizable panel across every pane, collapsible to a bubble |

The hover card is the only dictionary surface that reads the CEFR marks; it is
also the only one that never fires without them. Hovering a mark whose pack is
not installed does nothing at all — no fetch, no spinner, no placeholder.

## 6. Rules the rest of the app already sets

* `dictionary-core` is pure and wasm-safe (POS table, keys, bridge plan, result
  shapes); `dictionary-db` reads Parquet and writes SQLite and is **excluded
  from the wasm lane by name**, the way `download-core` is.
* Every cache is bounded and cleared by its owner: the lookup memo is per
  document and clears whole at its cap, the card holds only the hits it shows.
* Nothing is fetched without the Settings row that names the pack and its size;
  a hover never starts a download.
* The route is its own realm and unmounts with it; the overlay holds no store
  and drops its results when it closes.

## 7. Open questions

* Hover delay and dismissal: a short delay before the card opens, and closing
  on pointer-leave-after-grace, so a sweep across a page does not flash twenty
  cards.
* Whether the overlay and the route share one result component or two.
* Attribution copy for the Settings section, once the packs' licences are
  pinned in the pack registry.
