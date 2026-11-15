# Design

> **Status:** M0 · **Last updated:** 2026-10-13

mareader2's design system. It starts from mareader's: calm, paper-first, chrome
that gets out of the way. But it is ours to improve (ADR 0005): where egui offers a
better native answer, or a mareader detail reads poorly, change it here, then in
the code.

Every UI change is looked at with the shot tool (`CLAUDE.md` § The look loop)
in light, dark and one tint, at desktop and narrow sizes.

## Principles

- **The page is the app.** Chrome is translucent, auto-hiding and quiet; the
  pane's paper colour fills everything the page doesn't.
- **One theme source.** Colours and shape come from the appearance
  (`crates/appearance` → `Tokens` → egui `Visuals` and `Style`). Stock widgets
  get them from the theme; custom widgets read `Tokens`. No colour or size
  literals in views except platform caption conventions.
- **Instant.** Moving an appearance control restyles the whole window, pages
  included, on the next frame.
- **Native over imitation.** Use egui's widgets, containers and interaction
  states. Hover responds at once, as egui does; animate only motion that
  explains something (the bar fading, the sidebar sliding, a zoom easing).
- **Nothing interactive under the hover band.** The top 48 pt reveals the title
  bar, so pane controls sit below it.

## How the theme works

Several looks, plus accent, contrast, brightness or text-size knobs, all
come from one small mechanism:

1. **Tokens are the only source.** `theme::Tokens` holds colours *and* shape:
   radii, control and icon size, text sizes, gap. Views and widgets read
   `Tokens::get(ctx)`; a theme can change shape, not just colour.
2. **Tokens are a pure function of settings:** `Tokens::new(&Appearance)`.
   A new knob is a small pure modifier on top (a `with_brightness`, a
   `with_appearance(accent, contrast)`), with a test.
3. **One apply.** `theme::apply(ctx, &tokens)` writes egui's `Visuals` and
   `Style`, covering all five widget states (noninteractive, inactive, hovered,
   active, open), and stores the tokens in egui's context data. Stock widgets
   follow with no per-call styling.
4. **Applied on change, at once.** `MareaderApp::restyle` compares the settings
   with what it last applied. An appearance `Command` restyles at the end of
   the frame it lands in and asks for one repaint, so the next frame shows it.
   A slider sends `Command::SetAppearance` on every tick; only disk writes are
   debounced. Test: `an_appearance_change_restyles_on_the_frame_it_lands`.
5. **Pages follow without re-rendering.** Filter, blend, texture, grain and the
   EPUB ink and paper colours are uniforms of the page pass (ADR 0003). A tint
   or brightness drag re-rasterises nothing; only typography (font, size,
   spacing) re-lays out EPUB text.
6. **Tune it live.** Debug builds read `MAREADER_TOKENS=tokens.json` (keys are
   token field names; values `"#rrggbb[aa]"` or numbers) and re-apply when the
   file changes. Unset, it costs nothing.

**Knobs this makes cheap** (not mareader features; the owner decides): interface
text size, a contrast boost, roundness, density. Each is one modifier.

**Not done:** per-app grammar flags (`pro`, `bevel`); mareader2 has one
design. A brightness knob would go through OKLCH like the tints, not per sRGB
channel.

## Colour

Seven tokens per base mode, from `crates/appearance/src/tokens.rs` (the
authority; fix this table if they disagree).

| Token | Light | Dark | Dim | Black | Used for |
|---|---|---|---|---|---|
| paper | `#ffffff` | `#131316` | `#1a1c1f` | `#000000` | page background, panes |
| ink | `#1f2937` | `#e5e7eb` | `#c3c6cb` | `#d4d4d8` | text and icons |
| muted | `#6b7280` | `#9ca3af` | `#8b8f96` | `#8a8a93` | secondary text, hints |
| surface | `#ebedef` | `#16161a` | `#202328` | `#000000` | chrome, the title bar |
| line | `#dadce1` | `#2b2b31` | `#2e3238` | `#27272a` | separators, hover fill |
| accent | `#2563eb` | `#60a5fa` | `#7a9bd4` | `#60a5fa` | selection, the active pane, toggles |
| accent-soft | `#dbeafe` | `#1d2b3a` | `#232b36` | `#0c1e33` | selection wash |

**Tint.** A tint colours the whole palette in its hue. Each token takes the
tint's hue outright (in OKLCH), keeps its lightness, and gains chroma toward a
per-token ceiling as the strength rises; strength 50 gives the full effect.
Colours sRGB cannot show are gamut-mapped by lowering chroma; near-white tokens
first darken up to 0.06 L so a sepia paper is cream, not white. Taking the hue
outright means no tint passes through other hues on the way (mareader's sepia
went magenta).

**Black.** Paper and chrome are pitch black (`#000`) for OLED screens and dark
rooms; ink is a softer `#d4d4d8` to spare glare. A tint cannot colour black, so
in Black it colours the ink (and the book's ink, up to 0.07 chroma) instead of
the paper. A PDF page is fully inverted, then a black point at 15 % turns white
and a scan's grey paper alike to pure black, and a 180° hue turn brings colours
back (a "smart invert"); it composites by screen over the black paper, which
changes nothing. A reflowed book draws straight in its palette: black paper,
the ink at 0.86 L, book colours mapped between them.

**Contrast (2026-10-22).** Chrome keeps at least 0.035 OKLCH lightness from a
book's page, and a separator at least 0.05 from the chrome it divides, on
whichever side each already lies (`SURFACE_GAP`, `LINE_GAP`). The bases meet
both; a tint that would close a gap moves the token's lightness only, so a
sepia sidebar stays sepia but no longer melts into its page. Dim stays a dimmed
page on purpose: a book's ink sits darker than its 0.22 paper and a PDF is only
dimmed, so marked text is lifted instead (`docs/study.md`). Settings groups
fill with `card`, the paper washed 18 % toward accent-soft (dark looks: halfway
from surface to line, then 25 % accent-soft), and an off switch's track is the
line 22 % toward the ink (18 % in dark looks), its knob edged in light looks.

**One pass, fewer steps.** The page chain is folded before it reaches the
shader: a step joins the one before it whenever that one keeps every colour in
range, as then the CSS clamp between them changes nothing. Dim runs as one
matrix, Dark and Black as two, so every pixel does less work in every mode.

**Book paper.** A reflowed page's paper follows the same rule as the tokens: its
target chroma is capped at what sRGB holds within 0.06 L of the mode's paper,
then scaled by the strength, and near white it darkens rather than losing
chroma. So every step of the slider shows in every mode and hue (light paper
used to stop at `#fff7f0` by strength 10). The popover shows strength as a share
of the full effect, 0–100 % over stored strengths 0–50.

**Appearance popover.** Groups (Presets, each preset section, Mode & colour,
Page texture, Film grain) fold from their header; a folded header reads out its
setting, and folds persist. Presets are five-across cards with "Save current
look" as the header's +. Choices are segmented tracks; texture and grain dials
show only when they act. While split, the per-pane switches sit pinned under
the scroll, together.

It is anchored as a **menu, not a card** (2026-11-06), which is the one place
the "popovers point at their trigger" rule above does not apply: it is a modeless
control panel rather than something explaining its anchor. Its frame touches the
title bar's underside, with no gap of its own, and stands 8 pt off the window's
right edge (`titlebar::FLYOUT_MARGIN`): enough to read as a panel, too little to
be a gap. It carries no shadow, since there is no room above it for one to fall
into and the theme's 24 pt blur would only smear the bar. Both styles,
Classic and Dial deck, share the anchor.

**Derived in `theme.rs`:**
- hover fill = `line`; selected = `accent-soft` with an accent icon;
- glass (the title bar) = surface at 96 % until the blur pass (M2 slice 3) gives
  surface at 72 % with a 20 pt backdrop blur.
- glass, retuned (2026-10-22): surface at 59 % (`FROST` 150) over a
  12 pt blur. At 72 % and 20 pt, shots of glass on and off differed by at
  most 6 levels, so the switch looked dead; text under the bar now shows.

**Switches show what they do (2026-10-22).** A sweep of shots turned each
switch on and off and kept the ones that changed nothing visible:
- **Corners and pane shadow** need room: touching panes hide both. Picking
  rounded corners or the shadow opens an 8 pt gap (`PaneLook::make_room`)
  when the gap is zero; a chosen gap stays. The row hint says so.
- **Progress bar** draws a faint track under the accent line, so turning it
  on shows even on the first page. In a rounded split the track stops short
  of the pane's corners.
- **Appearance popover** sizes to the room left above the window's foot,
  less a 16 pt margin, so its last group (Film grain) is never cut off in a
  split.

**One source per control (2026-10-22).** Shots showed the same control drawn
several ways; each now has one helper in `widgets.rs`:
- **Dropdowns** come from `widgets::combo`, with the Lucide chevron. Clippy's
  `disallowed-methods` refuses a raw `egui::ComboBox`, so egui's triangle
  can't come back.
- **Filled buttons** (accent or danger) come from `primary_button`. The
  label takes black or white, whichever reads better on the fill
  (`theme::ink_on`, 4.58:1 at worst): black on the dark looks' light blue,
  white on light mode's deep blue. Disabled, a filled button fades like the
  buttons beside it rather than swapping to a pale fill.
- **Menu notes and headings** (`menu_note`) indent by the button padding, so
  they line up with the rows' icons, never under the pointer at the menu's
  corner.
- **Empty sidebar panels** (`panel_empty`): icon, title and a wrapped hint,
  centred a third of the way down, inset 28 pt from the sides.
- **Bar rules** (`vrule`): one hairline between a bar's groups.

The shortcuts sheet takes the window's height, less 32 pt above and below,
before it scrolls, so a laptop-sized window shows every row.

**Contrast floor.** Muted and accent ink are small text, so after any tint
`readable` moves their OKLCH lightness just far enough for 4.5:1 (WCAG AA)
on paper, surface and accent-soft. Light muted and light accent darken a
little; dark mode already passes. The table holds the inputs.

**Preset names show whole.** A centred name may run into the gaps beside
its card, then shrink by up to a point; only then do cards drop below five
across ("Parchment" was "Parchme…"). Past three it is elided, whole on hover.

## Type

- Inter (Regular, Medium, SemiBold), embedded once from `assets/fonts` by
  `mareader-reflow`; the interface borrows its bytes (`reflow::inter`).
- Body 14 pt, small 12 pt, title 13 pt Medium.
- Reading fonts for EPUB arrive with M4 (ADR 0002).
- egui's named families exist only from the second frame; `MareaderApp::ui`
  skips frame one for that reason.

## Metrics

| Thing | Value | Where |
|---|---|---|
| Title bar | 48 pt | `theme::TITLE_BAR_H` |
| Toolbar button | 36 pt square, 18 pt icon, radius 8 | `Tokens::{control, icon, radius}` |
| Popover, window | radius 12, shadow 0 8 24 (black 24 % light, 47 % dark) | `Tokens::{radius_lg, shadow}` |
| Spacing | gap 8: items 8 × 6, button padding 10 × 6; toolbar clusters 4 | `Tokens::gap`, `titlebar.rs` |
| Body, small text | 14 pt, 12 pt | `Tokens::{text, text_small}` |
| Sidebar (M1) | ~288 pt, slides 300 ms | — |
| Toast | 3.5 s, errors 6 s, with an action 6 s; 360 pt wide | `toast.rs` |

## Title bar

`crates/ui/src/titlebar.rs`. An `egui::Area` at `Order::Foreground` floating
over the panes, so content never moves when it appears.

- **Reveal.** The top 48 pt (after a docked sidebar) shows it at once.
- **Hide.** 400 ms after the pointer leaves, then a 200 ms fade.
- **Held** while any popup is open, or while a press that began on it is down.
- **Pinned** by the pin button; persisted. Each page keeps its own pin through
  `ChromeSettings::pinned`/`pin` and `Bar` (`Route::bar` maps a page to it):
  the reader starts unpinned, the library and the dictionary pinned.
- **Page room.** `TitleBar::page_top` is where the library and the dictionary
  start: under the bar while pinned, at the window's top once unpinned, with
  a short slide. The dictionary's field sits in the band then, so it is the
  bar's `quiet` rect: the pointer on it never calls the bar.
- **Pin, since the UX audit.** The bar has no pin button now: View ▸ "Keep
  title bar shown" (also in Settings) pins it. The pin icon means one thing
  only, docking the sidebar from its header.
- **Chromeless** mode never shows it.
- **Sidebar edge.** In overlay mode the window's left 6 pt opens the rail
  at once, but within the bar's band only after resting there 0.5 s, so
  overshooting Library onto the edge doesn't move it.
- **Fading out, it ignores clicks**, so a click meant for the page lands there.
- **Window moves.** Dragging the empty bar moves the window; double-click
  toggles maximise.
- **Layout.** `egui::Sides`: leading cluster (sidebar, library) and trailing
  cluster (search, appearance, settings, pin, then the captions). No split
  buttons (owner, 2026-10-17): panes split by dragging a book from the
  sidebar's Library panel onto a pane edge. The title is centred on the window when it fits, else in the gap,
  truncated with an ellipsis, and hidden when the gap is under 48 pt.
- **Document controls.** When documents open (M2), the trailing cluster gains
  the pane's own controls (zoom, view mode).

### Window controls

| Platform | Look |
|---|---|
| macOS | Native traffic lights at x 20, centred in the 48 pt row; the bar leaves 80 pt for them when it hosts them |
| Windows | 46 pt full-height squares, flush right; hover `line`; close hover `#e81123`, pressed `#c50f1f`, white glyph; 14 pt glyphs |
| GNOME/Linux | 24 pt circles, 8 pt apart; ink at 12 / 24 / 34 % for rest / hover / pressed; close `#e0524f` / `#c53a37`, white glyph; 12 pt glyphs |

**macOS traffic lights** (owner decision, 2026-10-13). AppKit puts them at
the top of a 28 pt strip; `apps/mareader/src/traffic_lights.rs` moves them to
the middle of the 48 pt row; `NativeLights` (`crates/ui/src/platform.rs`)
says where, each frame:

- **A drawn sidebar hosts them**, docked or floating: they sit in its header
  at full opacity and slide with it, leaving past the window's left edge.
- **Otherwise the bar does**, fading with it (and in after a rail's exit).
- **Overlay mode, no rail:** hidden, and the bar keeps no gutter (mareader).
- **Zoomed interface:** nothing zooms the interface now, and a zoom egui
  saved from older builds is put back to 100% at launch. Were it zoomed, the
  binary still centres the lights in the taller row and scales the slide.
- **Glass backing:** macOS 26 draws the lights as glass whose white sheen
  fades into a light bar. On light themes the app paints a dark disc half a
  point inside each, where the binary measured it (`LightSpots`), fading and
  sliding with them.

Hidden lights can't take clicks. Full screen is left to AppKit. Off a Mac,
shots and the preview draw three stand-in dots in their place. It is the
workspace's one `unsafe` module and the `traffic-lights` feature (on by
default); `--no-default-features` keeps AppKit's position.

**Frameless edges.** On Windows and Linux the outer 5 pt resize the window
(`crates/ui/src/resize.rs`); corners win over edges.

## Panes

Model `crates/workspace`, view `crates/ui/src/panes.rs`. Each pane is drawn in
a scoped `Ui` region (`Ui::scope_builder` with a `max_rect`), the way
egui_tiles lays out tiles. That is layout only: panes share the app's
documents, caches and workers (ADR 0006).

- Up to 4 panes; a split refuses below a 0.15 ratio or 200 pt per side.
- Dividers: an 8 pt drag band with resize cursors.
- The active pane (when split) has an accent outline, 2 pt by default.
- Workspace look settings: gap, outline width, rounded corners
  (`Tokens::radius_lg`), shadow (0 5 18, `Tokens::shadow`).
- Dragging a divider sends `Command::SetRatio` every frame it moves.
- A close button appears at the top right of a hovered pane, below the hover
  band.

### Lifting a pane (owner request, 2026-10-18)

In a split, pressing empty space in a pane (off the pages, or away from
their words) and holding still for a second lifts the pane; moving more
than 4 pt first makes the press a pan instead. A ring fills round the
press from a fifth of a second on, so plain clicks never flash it.

- While lifted, the pane dims where it was, a chip "Moving <title>"
  follows the pointer, and the box it would get lights in the accent
  with the move in words.
- Over another pane's middle third it swaps with that pane; elsewhere it
  docks beside the nearest edge with room for two 200 pt halves. A pane
  too small to halve either way swaps.
- Letting go moves it (`PaneTree::swap`, `PaneTree::move_to`); the pane
  keeps its id, so its document, page bar and look go with it. Letting
  go off any pane, or Escape, leaves everything as it was.
- One pane alone never lifts; a move that would not fit is refused
  whole, with a note.

## Settings that follow the focused pane

The settings window shows only what reaches the focused pane, as mareader
does, but from one rule instead of a condition at every row.
`settings_window::Scope` says what the focused pane shows (nothing, a PDF,
or a book read as a stream or as pages), whether the window is split,
whether any pane shows a book, and whether pages scroll sideways. Each tab,
section and row names an `Applies`, and `Scope::applies` is the only place
that decides. A section's rows go through `Rows`, which draws a divider only
between rows it drew, so hiding a row never leaves a double line.

| Applies | Shown when | Used by |
|---|---|---|
| `Always` | always | most rows |
| `Book` | a book is focused | the Fonts tab, Column width included |
| `NotBook` | a PDF or nothing is focused | Scale my zoom with the pane, the Paper section |
| `Pages` | not a book's vertical stream | No gap, Page shadow |
| `Split` | the window is split | Split pane appearance |
| `SplitWithBook` | split, with a book in any pane | Pane text |

A new setting picks its `Applies` once; nothing else changes.

Away from the books, the library's and the dictionary's bars carry a
Settings button (and Cmd/Ctrl+, works there). It lists only the tabs those
pages use: Read aloud with its voice models, Highlights, Vocabulary,
Dictionary, AI and AI models (`Tab::beyond_books`). The dictionary's page
opens it on Dictionary. A link to any other tab opens the full list.

## Folds

`fold` is the fold that keeps its chosen rows while shut: `fold::title`
draws the name, an optional count pill and a chevron that turns as it
opens; `fold::rows` lays the rows, kept ones always, the rest growing in,
clipped and fading, and taking clicks only once wholly shown. A fold
starts shut and remembers itself for the session under its id. Settings'
Voice engines (keeping the engine in use and the tab shown) and the
library menu's Sort by (keeping the order in use, its direction below)
use it. Settings' two-line folds stay on `fold_row`, which can be revealed.

## Tooltips, toasts and the menu

The reader's View & tools menu presents title-bar pinning and distraction-free
as switch rows with accessible on/off state. Distraction-free hides the bar and
its menu when enabled; Escape or its shortcut returns it. Settings and Keyboard
shortcuts share a two-up footer. The version remains factual: no update status is
shown without an update check.

- **Tooltips.** `widgets::tip`: a chip on `surface` with a `line` edge,
  radius 6, 12 pt text, the shortcut beside it in `muted`. 350 ms delay.
- **Toasts.** `crates/ui/src/toast.rs` and `toast/`. They stack in the
  bottom-right corner of the panes, 16 pt in, above the page bar in the
  reader; 360 pt wide.
  - **Card.** The tone icon and edge mark each card; the original gradient
    wash drains across its surface to show the time left. An action such as
    Undo shows its shortcut; a dismiss button remains available.
  - **Pile and fan.** The newest is in front, with older cards peeking behind
    and a count badge. Hover fans the pile into a paused list; click the pile
    or badge to keep it open for keyboard and touch. The header clears all,
    and the list scrolls past its height cap. Repeats deduplicate and at most
    12 are kept.
  - **Player island.** Short player confirmations rise from the player when
    they fit and their space is clear. Long messages, a compact player, other
    notifications and conflicts use the regular corner stack instead.
  - **Swipe.** A card dragged right past 96 pt, or flicked, flies off;
    shorter, it springs back. A pull left barely moves it.
  - **Never over what was summoned.** When an open menu, a word or
    dictionary card, a note, the highlight bar, the selection bar, the
    page's right-click menu or the "Table ahead" prompt would sit under the
    stack, the toasts leave at once with their usual fade, their time cut
    short (`toast::keep_clear`). Toasts that come meanwhile wait and come in
    once the corner is clear, with their full time; one kept waiting longer
    than it would have shown is dropped.
  - **Corner bed and conflicts.** Toasts sit at their usual spot in the
    corner (`toast::bed`). When chrome is idling in the toast bed, it eases
    aside or narrows automatically (the player shrinks; the dictionary
    window shifts left), easing back to its place once toasts leave. When a
    user manually widens the player or drags the dictionary into the toast,
    the toast plays its outro and disappears immediately.
  - **Above what was summoned.** Whatever asks to stay clear of toasts
    (`toast::keep_clear`) also lifts the player: over the selection bar on
    the library page it glides up and glides back down once it's gone.
  - **Out of the way.** The stack takes the pointer only over its cards and
    badge. While something on the page is carried near it, it fades to 30 %
    and lets the pointer through, so drops beneath it still work.
  - **Time.** Up 3.5 s, 6 s for errors and for cards with an action.
    Hovering, the open list or a drag stops every clock, and at least
    1.5 s is left after. They come in with a 200 ms rise and fade.
- **Read-aloud question.** `read_aloud/ask.rs`. When Skip tables, lists,
  figures or asides is set to Ask me, reading pauses at one and a 44 pt
  pill rises 8 pt into place 10 pt above the player: the part's mark in a
  soft accent round, "List ahead" (or Table, Figure, Aside), a quiet Read
  and a filled accent Skip. Play or Next mean Read, Esc means Skip, and a
  changed setting answers it. Inside both a list and a figure, the outer
  one is asked about first.
- **Read-aloud player width.** It starts as narrow as it goes: 300 pt, or
  the steps, play, a short track and stop if wider; the full card opens to
  380 pt. Dragging a side sets a width (300–960 pt) that every shape keeps,
  and the time left and speed come back as room allows. A double-click on
  a side resets it to the narrowest.
- **View & tools menu.** The last button in the bar (`view_menu.rs`). It
  holds full screen, the keyboard shortcut list and the version; Open…
  is left to Ctrl+O (⌘O) and the empty pane. When the
  bar is narrow, Settings, Search and the splits fold into it, leftmost
  first, so the title keeps 160 pt.
- **Shortcuts.** `keymap.rs` is the only table: the handler, tooltips and
  the list all read it, so they cannot disagree. macOS shows symbols (⌘,),
  other desktops words (Ctrl+,).
- **Shortcut sheet.** "Keyboard shortcuts" in either View menu, or Ctrl+/
  (⌘/), opens a centred modal (`shortcuts.rs`) instead of unfolding a list in
  the menu. Groups (Moving around, View, Find, General, Panes, Controls) sit
  in two balanced columns, one when narrow; every key for an action shows as
  a key cap. The filter takes focus, matching action names, group names or a
  whole key. Esc, the backdrop, ✕ or its own key close it; page keys stay
  inert behind it.
- **Selection bar icons.** The bar over selected text shows icons alone at
  every width; each name is its tooltip and accessible label. Words made it
  far too wide.
- **Modals.** Every sheet, alert and window-sized panel is a
  `modal::Dialog`: centred, on `surface` with a `line` edge, the large radius
  and the window shadow, over one backdrop (black at 48, 90 in dark). Its
  parts are shared too: the heading with an optional mark, the switch card,
  and the filled and quiet buttons. A test refuses `Modal::new` anywhere else.
- **Remove alert.** Removing asks in a small centred alert
  (`library/remove_alert.rs`), fixed at 440 px so it never jumps. It holds the
  question by name ("Remove “Dune”?"), one line on what happens to the files,
  and switches only where they mean something: "Remove everything inside" for
  a shelf with contents, and "Erase highlights and reading position" when
  there is any. Both start off each time; the erase switch turns its card red
  and the button to "Erase & remove". No counts, sizes or shelf lists. Enter
  removes, Esc or Cancel keeps, and a short toast follows.
- **Import sheet.** A short form (`library/import_sheet.rs`, 460 px): the
  folder as a field the whole row of which picks another; format checkboxes,
  ticked ones counted; "File size larger than" with its dial on one line;
  "Read books in place" with "Check for new books" nested under it; then the
  folder structure as two radios. Each answer has one small line under it,
  no paragraphs. Import stays off until a format is ticked; Enter imports.
- **Dictionary card, compact.** 300 px wide with tighter padding; the
  languages sit on the word's line before ✕. Burmese and other tall scripts
  draw answers at 1.05× the text size, not 1.15×: they run taller at a size.
- **Long highlights.** A notes-panel quote stops at three lines and a note at
  four; a cut row ends in "Show more", which opens it in place, then "Show
  less". The row's accessible label always carries the whole text.
- **Shaped text in panels.** Burmese, Thai, Arabic and Indic text egui would
  garble goes through `shaped.rs`: notes, chapter titles in Contents and the
  notes headings, Library titles, and the AI card's word, meaning and usages.
  Heights come from a cheap measure; a texture is drawn only once on screen.
  Search does not wash words in shaped text.

## Dictionary page

A route of its own (`dict/page.rs`), like the library, in place of mareader's
rail panel. The reader's and library's bars carry a Dictionary button, and
Cmd/Ctrl+Shift+D toggles it from anywhere. On the page the button is pressed
and returns where it came from; Library sits beside it.

- **Layout.** A centred column up to 1080 px: a pill search field (50 px on
  the page, 40 in the window) whose accent ring glows in while it has the
  keys, the text and hint centred in its full height so tall scripts never
  clip, and a ✕ that fades in. Then the choices: a sliding Dictionaries |
  Languages switch, the pack or From ⇄ To as pickers that cut their label
  short instead of overflowing (in the window the two pickers and ⇄ fill
  the row exactly), a "via English" or "Detected …" chip, and Manage. From
  640 px the results list (300 px; the pick lit with an accent edge) sits
  beside the chosen entry; narrower, one column of cards.
- **Entry.** The headword large with its speak button and IPA, tags and the
  pack as chips, the answer at 1.5× on a soft panel with its own speak
  button, romanization and sense, Copy, "Look up ⟨answer⟩" when the answer
  is a word or two, and the word's other senses as cards. The window's
  column leads with the same headword, then each sense as a card (tags, the
  answer and its speak button, gloss); near matches follow under "≈ word"
  headings that carry their own speak button.
- **Speaking.** A word or answer gets a speak button only when the voice in
  use speaks its language: a model's picked voice, the system voice's
  picked language, else the system's locale. The dictionary knows each
  entry's language from its pack (flipped for reverse lookups; a bridge's
  answer is in the language asked for). The hover card follows the same rule.
- **Other voices.** Settings › Dictionary › Saying words › "Use other
  languages' voices" (on by default) lends a word the first downloaded voice
  of the engine in use for its language. The system engine is opened once
  to list its voices when a dictionary first shows, and its last listing
  is kept after its booths close. A language the engine's model has but none
  downloaded shows a soft "Get a French voice to hear these" row (two at
  most) under the window's headword and the page's answer panel; it opens
  the model's Settings tab, where `settings_window::reveal` asks the one
  fold primitive (`fold_row`) to open, scroll into view only if needed, and
  glow in the accent twice as it fades over 1.6 s.
- **Keys.** The field has focus on arrival; ↑ ↓ pick a result, Enter keeps
  the search in Recent, Esc clears and then goes back. Cmd+F refocuses.
- **Empty.** An accent disc, what is searched, the keys drawn as keycaps,
  and recent searches as chips under a Recent heading with Clear.
- **Floating window.** In the reader the button and shortcut float the same
  search over the page instead (`dict/window.rs`); Settings › Dictionary ›
  In the reader can ask for the page. It starts at 380 × 520, top right;
  the head drags, each edge and corner resizes down to 300 × 240. − folds
  it into a pill naming the last word; ⤢ opens the page with search kept,
  and ✕ closes. Esc clears its field, then closes. Open, folded and the
  restored rect are saved. It grows from 96 % and rises 8 px as it fades
  in, and leaves the same way. Folding morphs the window into its pill;
  motion settings turn the animations off.
- **Feeding the window.** While it is open, Look up fills its search
  instead of a card (a folded window comes back) and the selection stays.
  A card's book button carries its word into the window, or the page. The
  shortcut gives an idle window the keys; pressed while typing, it closes.
- **Edge parking.** Settings › Panes offers Off, Window edge, and Edge tab.
  Off keeps the dictionary on screen; Window edge keeps the original 20 pt
  sliver and magnetic pull. Edge tab replaces the sliver with a labeled tab,
  search count and a delayed peek; dragging toward an edge shows the threshold.
  Clicking or keyboard-activating the tab restores the original window.
  A covered read-aloud player uses the old sliver in Window edge mode and a
  playable progress orb in Edge tab mode. Its words follow the current speech:
  the chosen group is shown if it fits, otherwise a fitting phrase or the
  current word. This does not change page highlights. The orb grows and moves
  back into the player when uncovered; motion settings turn that off. In Off
  mode it moves above the dictionary when possible. Toast avoidance and the
  player's priority remain.
- **Bridges.** The route chip names the language between ("via English");
  each answer names its English word ("via light"), which keeps its room
  while the tags give way. Scrolled lists stay clear of the floating bar.

## Book covers (2026-10-28)

A cover should look like a well-made book: quiet, still, never a sticker.

- **Finish.** Every cover gets a tight contact shadow under a soft one,
  a hairline edge (dark on light looks, light on dark), and a hinge: a
  groove pressed in near the spine, lit on its far side. Corners are
  tight at the spine and softer at the fore-edge.
- **Cloth covers.** A book without art is bound in one of ten deep
  tones (navy, forest, oxblood, ochre, slate, plum, teal, olive, rust,
  indigo), lit from above. Its initial is set in Literata in a gilt
  frame, with a short rule and its format spaced at the foot. Gilt type
  reads at 4.5:1 or better on every tone (a unit test checks it). Dark
  looks deepen the tones.
- **Hover.** The cover lifts 3 points and its shadow lengthens; there's
  no grey box. Picks and drop targets keep their ground and ring.
- **Progress.** A silk bookmark in the accent, tucked under the cover's
  top edge near its fore-edge, says how far as "89%" (at least 1%). It
  has a swallowtail end, a sheen down its middle, a soft shadow, and
  grows to fit "100%". An unopened book has none, and a missing one
  wears its Missing pill there instead. The card loses its progress
  row: 44 points of text. (A bar in the cover's foot came first, and
  looked like a scrubber, not a book.)
- **Organizing.** No badge. While a run works, a soft band of accent-
  tinted light crosses each held cover every 2.6 seconds, card after
  card in reading order, and the caption reads "Finding a shelf" with
  three dots that breathe in turn. The rail's tree shows the same dots.
  When the plan lands, one last pass crosses every held cover at once,
  and each caption names the shelf the plan files it on, or "Stays
  where it is". With Animations off, nothing moves and the words stay.

Code: `crates/ui/src/library/view/face.rs` (the finish, cloth, the ribbon
and the band) and `view/held.rs` (the pass, captions and tips).

## Icons

Lucide 1.34.0 geometry is compiled from build-only declarations behind
`crates/ui/src/icons/` (ADR 0004).

- Code asks for an `Icon` by meaning; `icons::pack` is the only place that names
  a generated glyph. Add a variant when a feature needs one.
- **Resting glyphs (2026-11-07).** Every stock glyph is the canonical first
  sample of the same typed scene that animates. `raster_resting` composites only
  that sample on the 24-unit grid, tightly crops its neutral-white pixels, and
  the UI caches the texture by pack and physical side under a 4 MiB/512-entry
  context-local bound. One image quad uses virtual-canvas UVs to preserve the
  original margins, then the button or `Icon::paint` applies the current tint.
  No animation atlas, XML parser or `egui-lucide` dependency is involved.
- **Provider SVGs (2026-10-22).** `Icon::svg` serves providers' logos from
  `crates/ui/assets/logos` (Simple Icons under CC0), white and tinted like the
  rest. They sit in ink on a quiet tile; a custom server shows the generated
  server glyph. The AI mark and all six voice marks use generated packs.
- **The voices' own marks (2026-11-10).** `Icon::engine` maps an engine id to
  its generated Kokoro, Piper, Kitten, Phonon, system or silent pack. They use
  the same 24-unit grid and stroke 2 as the other glyphs.
- **Icons that move (2026-11-10).** `mareader-animated-icons` holds 104 generated
  typed scenes and finite timelines. On the first interaction for a pack and
  physical side, one bounded worker rasterises tightly cropped, deduplicated
  layer-state variants and packs them into one neutral-white texture. The UI
  selects the current sample's cells and appends their quads in source order to
  one egui mesh. No runtime SVG string or XML tree exists, no atlas exceeds
  8 MiB, and the context-local animated cache is also capped at 8 MiB and 512
  entries. While generation is pending or cannot be admitted, the generated
  resting glyph remains visible. A run plays forwards and returns to that same
  resting source.
- Animated frames use a padded canvas so authored overshoot is not clipped, but
  the padding costs no layout. Icon-only buttons centre it on their response;
  image-and-text buttons use their exact leading-image box.
- **When an icon moves is its surroundings' business, not its own.** A region
  `announce`s an `Act`, and the nearest announcement wins: `Hover` for
  furniture or a menu that may disappear, `Click` for a control that stays put,
  and `Still` for a mark that only names something. A widget may `assume` an
  action only where no enclosing region has spoken. Runs use stable widget
  names, not labels or layout positions, and a press arriving mid-run queues one
  replay rather than snapping the glyph.

## Motion

- **Animations** is the master switch (`crates/ui/src/motion.rs`). Off zeroes
  egui's `animation_time` and every motion duration; end states still arrive in
  the same frame. The system reduced-motion preference has the same authority.
- Per-motion switches (M2): icon animations, sidebar slide, canvas follows the
  window, zoom ease and scroll to page. Each preference persists separately;
  icon animations default on to preserve existing behaviour.
- A loading indicator fades rather than freezes when animations are off.
- Icon runs (`icons::anim`) read `motion::icon_secs`, which combines their own
  switch with the master and reduced-motion gates. Off, a glyph is its generated
  resting drawing, no run starts or survives, and no repaint is asked for on its
  account. A press that arrives mid-run is queued rather than cut short, so a
  glyph is never caught bent.
- An active icon run asks for the next 60 Hz sample boundary; it does not request
  60-plus FPS, and it stops asking when the 0.42–0.84 s timeline lands. The
  context-local animated atlas cache holds at most 8 MiB of RGBA pixels or 512
  textures and uses the generated resting glyph instead of exceeding the bound.
  A cold run prepares one complete pack-and-side atlas on its bounded worker;
  warmed samples only select ordered cells from that texture. Frames use the
  moving glyph's padded canvas (normally 26–62 device pixels for the 15–18 pt
  controls at 1–2× scale, hard-capped at 192 px), and a run no longer schedules
  icon repaints once its widget is not built. Ordinary UI fades, slides and
  morphs also keep egui repainting while active, but normally do only
  interpolation, layout and painter work. The measured optimization plan and
  source audit live in `docs/animation-performance.md`.

## Layers

| What | egui |
|---|---|
| Panes and pages | the root `Ui` |
| Title bar | `Area`, `Order::Foreground` |
| Popovers, menus | `Popup` |
| Settings, confirmations | `modal::Dialog` (over egui's `Modal`) |
| Toasts | `Area`, `Order::Tooltip` |

## Revision history

| Date | Summary |
|---|---|
| 2026-10-13 | Created with the M0 foundation as `look.md` |
| 2026-10-13 | Became `design.md`: our own design system (ADR 0005); one-hue tints; egui-native controls |
| 2026-10-13 | How the theme works: shape tokens, one apply, instant restyle, live token file |
| 2026-10-15 | Settings that follow the focused pane: `Scope`, `Applies` and `Rows` |
| 2026-10-18 | Lifting a pane to move or swap it |
| 2026-10-28 | Book covers: bound finish, cloth covers, progress on the cover, organizing without a badge |
| 2026-11-07 | Generated resting glyphs replace `egui-lucide`; icon motion gains its own persisted switch |

### Page marks and reading questions

CEFR shading defaults to a quiet graded wash, with underline and dot styles
that differ in weight as well as colour; existing custom pens stay available.
Highlights use the page's contrast-aware multiply or screen blend. A noted
passage has a small end tab; its margin badge appears on hover.

A read-aloud question shows Space and Escape as answers and estimates duration
when buffered text permits; a partial buffer is labelled as a lower bound. An
optional skip rule is kept only for the current library book and can be removed
from the player's options. Unsaved documents do not offer a persistent rule.
