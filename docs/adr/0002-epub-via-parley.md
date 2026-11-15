# ADR 0002: EPUB via rbook + an XHTML subset + parley

- **Status:** accepted; parley confirmed by the owner, 2026-10-13. The M4 spike
  answers the open questions below, not the choice
- **Context:** EPUB replaces txt/md. mareader's text formats give full
  typography control:
  - families, size and weight;
  - line height and paragraph margin;
  - word and letter spacing;
  - indent, justify and hyphenation;
  - book layout and column alignment;
  - ink contrast.

  Every view mode must work, and the page must take appearance, textures and
  grain. EPUB content is XHTML + CSS.
- **Options considered:**
  - **A browser engine** (webview, Servo, Blitz). Blitz is 0.3 alpha with
    immature inline layout, and all of them are heavy. User typography must also
    override publisher CSS, which fights the cascade. Rejected.
  - **egui's own text** (`LayoutJob`). epaint 0.36 shapes with harfrust and has
    justify, letter spacing and line height. It lacks word spacing, hyphenation,
    bidi and pagination control. Rejected for book text; fine for chrome.
  - **parley 0.12 + fontique.** Rich text layout from Linebender: bidi,
    shaping, line breaking, alignment including justify, letter and word spacing,
    line height and font fallback. Chosen.
- **Decision:**
  - `rbook` (Apache-2.0) reads the container: OPF, spine, NCX/nav ToC and lazy
    resources.
  - `epub` converts each spine XHTML document, through `html5ever`, into
    `reflow` blocks: headings, paragraphs, lists, quotes, pre, tables (basic) and
    images. A small CSS subset gives emphasis, alignment and margins.
  - User typography wins over publisher CSS for every knob it sets.
  - `reflow` lays out with parley, paginates by real line heights (never
    splitting a heading from its next line), and rasterises with `vello_cpu` 0.3,
    the rasteriser hayro already uses.
  - Hyphenation uses `hypher` (pattern-based, embedded, many languages).
- **Initial open questions:**
  1. Does parley break at U+00AD and draw the hyphen? If not, break lines
     ourselves at hypher points.
  2. Glyph-run drawing in vello_cpu 0.3 for parley output (font data handoff).
  3. Bundled reading faces: which OFL families, with ATTRIBUTION entries.
- **Resolved design choices (2026-10-14):**
  1. parley breaks at U+00AD but draws no hyphen: the soft hyphen shapes to a
     zero-width blank. The typography pass draws the hyphen at lines that end
     in one.
  2. The handoff is direct: parley's `FontData` is peniko's, and each glyph
     run goes to `RenderContext::glyph_run` with its size and variation
     coordinates (`NormalizedCoord::to_bits`).
  3. Literata (variable weight and optical size, roman and italic) for
     reading, Inter for headings, both OFL and in ATTRIBUTION.toml. Only
     bundled faces load for now, so layout is identical everywhere.
- **Consequences:** EPUB pages are textures like PDF pages (ADR 0003). Search,
  selection and gloss use parley cluster geometry. The CSS subset is ours to
  grow; anything outside it degrades to plain flow and never fails.
