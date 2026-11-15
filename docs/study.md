# Study tools (M6)

Reading aids: explain a word in place, keep it marked, look words up in
offline dictionaries, and shade words above a chosen CEFR level. The parity
entries are `study.*` in `parity/mareader-features.toml`.

## What mareader does

- **Selection menu.** Any non-empty selection gets a small icon bar
  centred 10 px under it: Copy, Explain with AI, Look up. It hides while a
  card is open and when the selection scrolls away. A right-click on a live
  selection belongs to the bar, not the system menu.
- **Explain.** Only one word: trimmed, no inner whitespace, at most 60
  characters; anything else gets a toast saying so. The word and about 60
  characters either side go to the model, which answers part of speech,
  meaning in context, 2–5 synonyms and 2–3 usages, as JSON.
- **Marks.** Explaining a word marks it where it stands. The same word in
  the same spot reuses its mark. A click on a mark opens its card again; a
  right-click offers "Remove highlight", with Undo in the toast. Long press
  selects many marks to remove at once.
- **Card.** The word, then Meaning, Synonyms (chips) and Usages; a shimmer
  while waiting; an error with Retry when retrying can help. Escape or a
  press outside closes it. Answers are saved with their mark (see "Saved
  answers" below); failures are not kept.
- **Answers.** Apple Intelligence through the `fm-bridge` helper on macOS
  with Apple silicon; elsewhere the card says why it can't answer.
- **Dictionaries.** Parquet packs downloaded with mirrors and resume,
  converted to SQLite, looked up on click or hover.
- **CEFR.** A tagger and a word-level table shade words at or above a
  chosen level; a click explains.

## Where it lives

| Piece | Home | Why |
|---|---|---|
| The one-word rule, context, the answer's shape and limits, the `Explainer` contract | `crates/study` (L1) | pure rules, no egui, tested alone |
| Mark records | `crates/library` | every synced row lives in one store, merged and absorbed alike (`docs/sync.md`) |
| Bar, card, mark layer | `crates/ui-egui/src/study/` | egui; one place hooks into the page view |
| Packs, downloads, conversion, lookups | `crates/dict` (L1) | threads and SQLite, no egui; the browser lists packs only |
| Lookup card, Dictionary panel, Settings › Dictionary | `crates/ui-egui/src/dict/`, `settings_window/dictionary.rs` | egui |

The `study` crate is the "gloss requests and marks" row of
`docs/architecture.md`; the marks' records join the library's rows so a
sync carries them with the books they belong to.

## Marks

A mark is a synced record: an id, its book (stamped, so merging two
copies of one book can move marks), the word, its context, where it is,
when it was made, and `alive` (stamped, so a removal and an Undo merge
like a membership). Unknown fields from newer builds are kept.

Where a mark is depends on the page:

- **A PDF** keeps the page and the word's box in page units. Text read
  again lands in the same place, whatever the zoom.
- **A book** keeps the block and the byte range in its text, the same
  terms as a position. Re-typesetting moves the word to another page; the
  mark follows, because the page is found from the block each time.

Every character of a book's page knows its block and byte, so a selection
turns into a range and a range back into boxes on whichever page shows it.
Table cells have no such place; a word in one is explained, not marked.

## The answer

`Explainer::explain(word, context)` answers once, on a worker thread:
`Ok(WordInfo)` or an error with a kind, a message for the card and whether
Retry can help. `WordInfo` is clamped as mareader clamps it: meaning 1200
characters, 16 synonyms of 80, 8 usages of 400, cut text ending in "…".

- `Unavailable` explains nothing and says why. It is what every build has
  until the Apple bridge lands.
- `Canned` answers from a small table, for tests and shots
  (`explain=canned`); the word `fail` gives a retryable error.

## Slices

| # | Slice | Check |
|---|---|---|
| 1 | Selection bar (Copy, Explain) on PDFs and books; Explain marks the word and opens its card; marks drawn in the gloss colour and opacity, kept per book as synced records; a click opens a mark's card, a right-click removes it with Undo | select a word, click Explain: it's shaded and a card opens; click elsewhere, click the shading: the card is back; right-click it, Remove highlight, then Undo; `pdf=sample:5 explain=canned selection=0:quick gloss=open` in shots |
| 2 | Apple Intelligence through `fm-bridge` (macOS, Apple silicon, feature `ai`), the card filling as the answer streams, Retry | on a Mac with Apple Intelligence, Explain answers in place |
| 3 | Highlighters (owner, 2026-10-16; see "Highlighters" below): full, half and underline looks; your own highlights from the selection menu on PDFs and books; Settings › Highlights for each highlighter's colour, the menu's buttons and the colour-on-click choice; AI marks on their own reserved look; density; selecting many marks to remove at once | select text, click Highlight or Underline: it stays after a relaunch; add Half in Settings and it joins the menu; change a colour and the highlights follow while the slider moves |
| 4 | Dictionary packs: download with mirrors and resume, convert, Look up in the bar, a lookup card | download a pack, select a word, Look up |
| 5 | CEFR shading: the dataset and tagger, a level setting, click to explain; drawn in its own reserved highlighter and colour (red, as mareader's, by default) | turn CEFR on at B2: harder words are shaded, and that look is no longer offered for your own highlights; click one to explain it, rest on one to look it up; `pdf=sample:5 cefr=<dir> workers=0` in shots |

Look up stays out of the bar until slice 4 gives it something to do.

## Slice 1 behaviour

- **The bar** sits under the selection's last line, centred on it, or above
  its first line when there's no room below. It shows once the button is
  up, for any non-empty selection in the focused pane, and goes with the
  selection. Copy writes the clipboard and toasts "Copied".
- **Explain** checks the one-word rule (surrounding punctuation trimmed),
  marks the word unless the same word in the same spot is marked, clears
  the selection and opens the card. With no library (a browser build),
  the card opens without a mark.
- **The card** opens under the mark (above when there's no room), 320
  points wide, and closes on Escape or a press outside it. A mark's answer
  is asked once, then saved with it; a failed answer is asked again next
  time.
- **Marks** paint as rounded fills under the word in the gloss colour at
  the gloss opacity. A click opens the card; a right-click opens a menu
  with Remove highlight; the toast offers Undo for six seconds.

## Slice 2 behaviour

The owner widened this slice (2026-10-16): answers come from Apple
Intelligence on a Mac or, on every desktop, from an OpenAI-compatible
server, with no `ai` feature to turn on. How tasks, prompts and providers
fit together is in `docs/assist.md`.

- **Settings › AI** (between Dictionary and AI models) picks who explains:
  Automatic, Apple Intelligence (Macs only), OpenAI, Gemini, OpenRouter,
  Ollama, LM Studio or a custom address. Off a Mac, Automatic reads None
  and Explain says to choose one. Apple Intelligence's line says whether
  the model is ready, downloading, switched off or missing.
- **A server** shows its model, with the preset's default as the hint and
  a Choose menu listing the server's models, and its address: in the main
  group for local and custom servers, under Advanced for the cloud
  presets. Try it explains a sample word and says how long it took, or why
  it failed.
- **Model list.** A refresh button beside Choose asks again; the old list
  stays shown meanwhile. Ollama and LM Studio are asked each time the tab
  opens. The chosen model is saved; one the server no longer lists is
  dimmed, with a note to choose another ("x" matches "x:latest").
- **API keys** (cloud presets; optional for Custom) are listed by their
  ends with their state: ready, resting after a rate limit, or set aside
  with the provider's reason. Each can be set aside, brought back or
  removed. A password field adds one on Add or Enter. Keys are encrypted
  on this computer and requests take turns among them.
- **The card fills as the answer streams**: the meaning appears while the
  synonyms and usages are still coming. A failure names the reason
  (no key, keys turned away, offline, a model to check, a locked
  keychain) with the provider's own words underneath, and Try again shows
  only where asking again can help.
- **Changing the AI settings** takes effect on the next new word; saved
  answers stay, and Ask again on a card reaches the new provider.
- The browser preview hides the AI tab; `Unavailable` still answers there.
- **AI Dictionary (2026-10-22).** Explain is now the AI Dictionary, on the
  selection bar and everywhere else. Its model, mark, colour and card
  spacing are set in Settings › AI; Highlights shows its mark with a link
  there. Models and keys live in Settings › AI models (`docs/assist.md`).

## Saved answers

The owner asked for this (2026-10-16): a click on a mark showed a new
answer each launch, and a settings change dropped every answer.

- **With the mark.** A finished answer is saved in the mark's library
  record as its `gloss` field: part of speech, meaning, synonyms, usages
  and who answered. It survives restarts and, once sync exists, travels
  with the mark, so another machine doesn't pay to ask again. Answers
  that merge keep the later one.
- **Who answered.** The card's small print names the provider and model,
  e.g. "Ollama · gemma4:31b-cloud", or "Apple Intelligence".
- **Ask again** on a finished card asks the AI chosen now. The saved
  answer stays until the new one arrives and replaces it, so a failed
  retry loses nothing.
- **Removing a mark** hides its answer with it; Undo brings both back.
  Explaining the same spot afresh makes a new mark and asks again.
- **Failures are never saved.** A book outside the library keeps its marks
  and answers for the session only, as before.

## Highlighters

Asked for by the owner (2026-10-16) for slice 3; the CEFR half lands with
slice 5.

- **Three looks:** **full**, a square fill per
  line of text at half opacity; **half**, the same fill over the lower half
  of the line only; **underline**, a thin line under the text. No
  strikethrough (owner). They replace today's rounded mark fills.
- **Your own highlights.** The selection menu offers Highlight and
  Underline by default; Half can be added. A highlight covers any
  selection, not just one word, on PDFs and books alike, and is a synced
  record like a mark.
- **Settings › Highlights.** Each highlighter's default colour; which
  buttons the selection menu shows and in what order (Copy and Explain
  too); and what a click does: use the default colour, or show a colour
  row each time.
- **Reserved looks.** AI marks (owner) and, from slice 5, CEFR each pick
  a highlighter and a colour. That pair is then not offered for your own
  highlights, so nothing you highlight can pass for an AI mark or a CEFR
  word. AI marks default to a full violet highlight.


## Slice 3 behaviour

- **Records.** A highlight is a mark with `kind: highlight`, its `pen` and
  maybe a `color`; marks saved before keep reading as Explain marks. Its
  spot spans pages: `Chars` keeps a PDF's `[page, char]` from and to,
  `Blocks` a book's `[block, byte]`, so type changes find it again. The
  same text with the same pen is one highlight; another pen is another.
- **Colours.** A highlight made with "Use its colour" keeps none and
  follows its pen's colour in Settings. One picked from the colour row
  keeps it. A kept colour that later meets a reserved look is drawn in
  its pen's colour instead.
- **Drawing.** Fills are square, at Fill strength (50% by default, 20 to
  80%); half fills the lower half of each line; underlines are solid, about
  8% of the line high. Highlights are drawn first and AI marks over them.
- **Under the ink (2026-10-22).** Fills, CEFR shading included, lie under
  the page's text as a highlighter's do: the page pass multiplies them on
  light paper and screens them on dark, so marked words keep their own
  ink. On dark pages a fill starts at 80% of Fill strength and is capped
  so the ink keeps 4.5:1 against it, or 60% of the page's own contrast
  when that is lower; on light pages black ink stays black. The settings
  preview uses the same rule (`PageTone::mark`). Without a GPU, fills fall
  back to a translucent box on top.
- **Lifted on dim pages (2026-10-22).** Where a page's ink is darker than
  its dark paper, as in Dim, no blend can make marked text readable, so a
  highlight redraws the page inside its box: the paper becomes the pen's
  wash at Fill strength and the ink near-white, the wash darkened until
  that ink keeps 5:1. PDFs, books and CEFR shading share it.
- **Clicks.** A click on an AI mark opens its card, as before; a click on
  your highlight opens its menu with Remove highlight, with Undo.
- **The bar** shows the menu's buttons in order, each pen's button with
  its colour underneath. With every button hidden no bar shows; Copy's
  shortcut still works. With "Choose a colour each time" a pen opens a
  colour row: its own colour, then the palette, without reserved pairs.
- **Settings › Highlights.** Each pen's colour (eight preset colours or
  a custom one), Fill strength, the click choice, the menu's buttons with
  show switches and up and down, then the AI's pen, colour and the card's
  spacing. Reserved pairs show crossed out. The AI cannot take a pen's own
  colour; if the AI moves to a pen whose colour it holds, that pen moves
  to the next free colour.
- **Left for later.** Selecting many marks to remove at once moves to the
  next study slice.
- **Checks.** Unit tests cover the records, old rows, spans and reserved
  pairs; kittests highlight and underline from the bar, remove with a
  click, pick from the colour row, set the tab, and stage highlights with
  the `highlight=full|half|underline` option after `selection=`.

## Slice 4 behaviour

- **Packs.** Eight, English to Burmese, Chinese, French, German,
  Japanese, Korean, Spanish and Thai, built in
  [wikidict](https://github.com/codewiththiha/wikidict) (its docs name each
  source and licence). Each comes from GitHub, then two jsDelivr mirrors,
  all pinned to one wikidict commit, two tries each. A download resumes from its part file
  when the server honours the range and its start matches; anything that
  isn't a parquet at both ends is dropped and the next mirror tried.
  Pause keeps the part, Cancel and Remove delete it.
- **Conversion.** The parquet is streamed row by row, by column
  name (word and definition required; part of speech, romanization and
  sense if present), into `<id>.db` beside it, then the parquet goes.
  Empty words or definitions are skipped and cells cut at 2,000 chars.
  A whole download found at launch converts then; a leftover table half
  built is deleted.
- **Look up** joins the selection menu after Explain, saved menus too
  (settings v3). It takes the selection, one line, edge punctuation
  trimmed, up to 60 characters, and opens a card beside it; nothing is
  marked. The card answers in Default language, else the first other
  language a pack holds. The word's scripts name its language: Burmese,
  Korean, Thai, kana for Japanese, Han alone for Chinese (Japanese when
  only it is downloaded); Latin is read as English.
- **Routes.** One pack either way round, else two packs meeting at
  English, so Burmese reaches French; such answers say "via" the English
  word. A card takes exact entries, then prefixes, then near misses (two
  edits at most), marked "≈" with the entry found. Repeats show once.
- **The card** closes on Escape, a press outside, or a scroll. Without
  packs it offers Get dictionaries…, which opens Settings › Dictionary.
- **The Dictionary panel** searches as you type (180 ms after the last
  key): every pack or one, both sides, with substrings; or between two
  languages, each Automatic or chosen, with a swap. When nothing is found
  between languages it offers "Search as" each other language.
- **Settings › Dictionary** lists packs on this device, downloading ones
  included, by language name; the rest wait in a More languages fold,
  open while nothing is downloaded. Rows offer Download, progress with
  Pause and Cancel, Resume, Remove and Try again, then Default language,
  offered only for downloaded packs.
- **Picking several marks**, left from slice 3. A long press on a mark
  (half a second) or Select several in its menu starts it: picked marks
  get a ring, a click toggles one, and a bar offers This page, All,
  Remove (N) with Undo, and Done. Escape stops picking.
- **Left for later.** Lookup on hover over CEFR words comes with slice 5;
  a browser lists packs but can't download them.
- **Checks.** The dict crate's tests convert both fixture packs, serve
  downloads from a local server (mirrors, error pages, a dropped
  connection resumed, pause and cancel), and ask cards, bridges and
  searches; kittests look up from the bar, follow Get dictionaries…,
  remove a pack, search in the panel, and pick and remove marks.

## Slice 5 behaviour

- **The dataset.** Settings › Vocabulary downloads mareader's word levels
  (about 2.9 MB, some 248,000 senses) and the role tagger (about 6.7 MB)
  from the cefr-rs repository, then two jsDelivr mirrors, through the
  same resumable fetch as the dictionary packs. The parquet becomes
  `cefr.db` in `<data>/vocabulary/`; a whole download found at launch
  converts then. Pause, Cancel, Resume, Remove and Try again act on both.
- **One download.** The tab shows both files as one Word data row at its
  top, with one progress bar over both. The shading options appear only
  once the levels are in, and go again when they are removed. Levels kept
  from before the roles model existed say "Word roles missing" and offer
  Download for the rest.
- **Shading.** With the switch on, a word whose level is above My level
  (A2 to C2, B2 by default; C2 shades nothing) is shaded on PDFs and
  books alike. New settings use a soft, level-graded wash; underline and
  dot styles offer a non-colour cue by weight. Existing settings keep their
  custom pen and colour, which remains available. A word counts as hard when it, a
  contraction's base or a part of a hyphenated compound is.
  Pages are planned once per text layer, level and dataset, and capped
  at 5,000 words; levels not yet known are asked off the frame and the
  page repaints when they land.
- **Reserved.** Like the AI's look, the CEFR look can't be taken by a
  highlighter, and the two can't share one. Settings › Vocabulary picks
  its pen and colour, offering neither a pen's own colour nor the AI's.
- **Under your marks.** Shading is drawn beneath highlights and AI marks;
  a hover or click on a word under a mark goes to the mark.
- **Click to explain** (on by default): a click on a shaded word asks the
  AI as Explain does and marks the word, the AI mark drawn over the
  shade. The card shows the word's class from the tagger until the AI's
  arrives, then that sense's CEFR level beside it.
- **Rest to look up** (`dictionary.hover`, on by default): resting on a
  shaded word for 160 ms opens its dictionary card, once per rest. It
  never replaces a Look up or Explain card and waits for a downloaded
  pack; leaving the word and its card (8 points of slack) or a press
  outside closes it.
- **Left for later.** A browser shows the tab but can't download; word
  roles for books in other languages.
- **Checks.** The cefr crate converts a fixture parquet, plans hard words
  and drives the service end to end; kittests shade a fixture page,
  explain on a click, open the dictionary card on a rest, and set the
  level, the look and Remove in the tab.

## Notes

A note is the reader's own words on a highlight. It is one more stamped
field of the mark (`note`), so it saves, merges and syncs with it, and it
hides and returns with the mark.

- **Note** in the selection bar highlights the words in the bar's first
  pen, then opens a draft beside them, with a caret pointing at them.
  The draft quotes the words under a rule in the highlight's colour.
  Save, or Ctrl+Enter (⌘Enter on a Mac), keeps it.
- Cancel and Escape drop the draft. If Note made the highlight, the
  highlight goes too; one made before stays.
- A press outside the draft keeps it: typed words are never lost by a
  stray click. While a draft is open it stays, even if its words scroll
  away.
- A noted highlight shows a small badge in its colour in the margin beside
  its last line, never over text; with no margin it sits raised and small
  over the line's end.
  Clicking the highlight or the badge shows the note, with Edit and Delete.
  Delete offers Undo.
- The mark menu offers Add note or Edit note; AI marks take no notes.
- The **Notes** panel lists the book's highlights in reading order, each
  with its page and note. All and Notes filter the list. A click goes to
  the page; the right-click menu edits, deletes or removes.
- Settings v4 adds Note to saved selection bars after Look up.

## Restyling a highlight

Right-click a highlight to change its look at any time. The menu opens on
its pen and colour; each choice applies at once, and the menu stays open so
looks can be compared.
It opens under the highlight, or above it near the window's foot, so it
never hides the words being restyled.

- **Pens.** Highlight, Half and Underline sit in one segmented row, named
  under their icons. Each shows the colour the highlight would take.
- **Colours.** The pen's own colour comes first, then the palette. The
  colour shown is ringed. Choosing the pen's own colour stores none, so the
  highlight follows the pen if Settings change it later.
- **Reserved looks.** A pen and colour pair kept for AI marks or CEFR
  shading is shown struck out and can't be chosen; its tooltip says what
  keeps it. A new pen keeps the colour unless that pair is reserved; then
  it takes the pen's own colour.
- AI marks keep the AI's look: their menu has no pens or colours.
- The look is a stamped field of the mark (`style`), so the latest restyle
  wins when libraries merge.

## Exporting highlights and notes

The Notes panel's header has **Copy all** and **Export…**. Both take the
list as shown: with the Notes filter on, only noted highlights go.

- **Copy all** puts Markdown on the clipboard and says how much it copied.
- **Export…** offers Markdown (.md), Plain text (.txt) and JSON (.json),
  then the system save dialog, named after the book. A toast says where
  it went, or why it couldn't be written.
- Highlights follow reading order, under their chapter from the outline.
  Each has its words on one line (a PDF's line breaks are layout), its
  note, and its page. JSON adds the pen, the colour and when it was made.
- The browser preview can't write files; it says so.

## Bookmarks

A bookmark keeps a page to come back to.

- **Bookmark this page** is the ribbon at the end of the page bar, or
  Cmd+D (Ctrl+D). On a bookmarked page the button shows pressed, and
  the same button or key takes the bookmark back, with Undo.
- A bookmarked page hangs a ribbon from its top edge, in the accent.
- A PDF bookmark keeps its page. A book's keeps the text its page
  started with, so it stays with that text when the type changes.
- The Notes panel lists bookmarks among the highlights in reading order,
  each with its page and chapter. A **Bookmarks** filter appears once
  there is one. Click goes to the page; right-click adds a note or
  removes the bookmark.
- Exports and Copy all carry highlights only.
- Bookmarks are stored as marks and sync like them. An older build
  skips them rather than failing to load.

## Restyling several highlights

Select several's bar has **Style…** beside Remove. It opens the same
pens and colours as a highlight's menu, for every highlight picked.

- A pen restyles each highlight, keeping its colour unless that look
  is reserved; then it takes the pen's own colour.
- A colour goes to each highlight whose pen may take it; the rest stay.
  The toast says so: "Restyled 1 of 2 highlights".
- A colour is struck through only when every pen in the group keeps it
  for AI marks or CEFR shading. With one pen in common, that pen and the
  shared colour show as current; mixed, none does.
- AI marks in the selection keep their look. Style… is off when no
  highlight is picked.
- Undo in the toast puts every look back. Picking goes on after either.

## Finding notes

The Notes panel reads like a notebook as a book's highlights grow.

- **Chapters.** Rows sit under their chapter from the outline, each heading
  with how many rows follow. A book with no outline lists them unheaded.
- **Search.** The field under the filters keeps rows whose words, note or
  chapter hold every word typed, in any case; matches are washed in the
  accent. Escape or × clears it. It narrows within the filter chosen.
- Copy all and Export take what the search leaves, like the filters.

## Meaning in another language (2026-10-19)

Settings › AI › Tasks › **Meaning in** picks the language of Explain's
meaning: the book's language (the default) or one of 40 listed ones.

- Only the meaning changes. The part of speech, synonyms and usages stay in
  the language of the text.
- In another language, the meaning reads like a bilingual dictionary: the
  closest equivalent of the word in that sense, with a short explanation
  only when no equivalent fits exactly. The schema's description of
  `meaning` says the same, since field descriptions steer models too.
- Apple Intelligence writes only the languages it reports
  (`supportedLanguages`, matched on the primary subtag). The others are
  greyed out in the list, and a language chosen earlier that it can't
  write gets a note under Tasks.

## Pronunciation (2026-10-24)

Settings › Vocabulary › **Pronunciation** is a download of its own, about
20 MB: the English pronunciation data the voice models read, American and
British. Once it is in, the dictionary and AI cards show how an English
word sounds under the word.

- **Sharing.** Kokoro and Piper keep the same eight files, and Kitten keeps
  the five American ones. When a voice model already holds every file, the
  pack counts as downloaded: its files are hard links to the model's, so
  nothing is fetched and no space is used twice. The row then reads
  "Shared with Kokoro" and has no Remove button, because removing it would
  free nothing. A model downloaded later links the pack's files the same
  way. Removing one side leaves the other's links working.
- **Match the reading voice** (on by default). Cards write the English
  of the voice that says English words: the voice picked in Read aloud,
  or the engine's English voice that stands in for it. An `en-US` voice
  gives American, `en-GB` British, each untagged. Other Englishes have no
  lexicon, so the nearer one shows tagged ≈ US or ≈ UK, with a tip saying
  why: Canadian and Philippine take American; Australian, New Zealand,
  Irish, South African, Indian and the rest take British. No English
  voice, or one without a region (plain `en`), shows both. The row's
  second line names the voice and its English, amber with an alert when
  the cards show a stand-in.
- **Accent.** Both (tagged US and UK), American or British; greyed while
  the voice decides, showing the accent it picked.
- **Written as.** IPA as dictionaries write it (/jɒt/, /jɑːt/), Easy to
  read (yot, yaat: syllables split with dots, the stressed one in bold),
  or Raw, the phonemes exactly as the voices read them.
- Words are looked up on a thread, and the data loads only for the accent
  shown. It is freed two minutes after the last lookup, or shared with a
  voice that is already holding it. Only short English selections (up to
  three words) are sounded out, and a word that can't be sounded out
  shows nothing.
