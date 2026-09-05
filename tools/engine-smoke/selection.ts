import { readFileSync } from "node:fs";
import vm from "node:vm";

// The READER bundle: the format-agnostic half of the browser side, today the
// selection tracker that answers "what did the reader select, and where?".
//
// This scenario deliberately does NOT import ./harness.js. The harness boots
// `public/pdfEngine.js` with a stubbed pdf.js, and the whole point here is the
// opposite: the reader bundle must install and report with no engine and no
// pdf.js in the sandbox at all, because a TXT or Markdown document selects
// through exactly this code and loads neither.

const readerSrc = readFileSync(
  new URL("../../public/readerEngine.js", import.meta.url),
  "utf8",
);

// ---------- the smallest DOM the tracker reads ----------

type Attrs = Record<string, string>;

class FakeText {
  readonly nodeType = 3;
  parentElement: FakeEl | null;
  constructor(readonly data: string, parent: FakeEl | null = null) {
    this.parentElement = parent;
  }
  get textContent(): string {
    return this.data;
  }
}

class FakeEl {
  readonly nodeType = 1;
  parentElement: FakeEl | null = null;
  readonly children: (FakeEl | FakeText)[] = [];
  constructor(
    readonly id: string = "",
    readonly attrs: Attrs = {},
    readonly classes: string[] = [],
  ) {}
  getAttribute(name: string): string | null {
    return Object.prototype.hasOwnProperty.call(this.attrs, name) ? this.attrs[name] : null;
  }
  get textContent(): string {
    return this.children.map((c) => c.textContent).join("");
  }
  matches(sel: string): boolean {
    return matchesSelector(this, sel);
  }
  closest(sel: string): FakeEl | null {
    // eslint-disable-next-line @typescript-eslint/no-this-alias
    let el: FakeEl | null = this;
    while (el) {
      if (el.matches(sel)) return el;
      el = el.parentElement;
    }
    return null;
  }
}

// The host protocol's selectors come in four shapes: a bare attribute
// (`[data-reader-host]`), an attribute compared with `=`, `^=` or `$=`
// (`[id^='cont-']`), and a class (`.textLayer`). Anything else is a bug in
// this stub, not a selector to support, so it throws instead of guessing.
function matchesSelector(el: FakeEl, sel: string): boolean {
  if (sel.startsWith(".")) return el.classes.includes(sel.slice(1));
  const attr = /^\[([\w-]+)([\^$]?=)?'?([^'\]]*)'?\]$/.exec(sel);
  if (!attr) throw new Error("selection smoke: unsupported selector " + sel);
  const name = attr[1];
  const op = attr[2];
  const value = attr[3] ?? "";
  const have = el.getAttribute(name);
  if (have === null) return false;
  if (!op) return true;
  if (op === "=") return have === value;
  if (op === "^=") return have.startsWith(value);
  return have.endsWith(value); // "$="
}

// One text node per row keeps the range arithmetic honest without modelling a
// tree: `toString()` is the row's text between the offsets, which is what a
// real Range returns when the row holds a single Text node.
class FakeRange {
  private row: FakeEl;
  startContainer: FakeText;
  startOffset: number;
  private endOffset: number;
  constructor(row: FakeEl, text: FakeText, start: number, end: number) {
    this.row = row;
    this.startContainer = text;
    this.startOffset = start;
    this.endOffset = end;
  }
  toString(): string {
    return this.row.textContent.slice(this.startOffset, this.endOffset);
  }
  cloneRange(): FakeRange {
    return new FakeRange(this.row, this.startContainer, this.startOffset, this.endOffset);
  }
  selectNodeContents(el: FakeEl): void {
    this.row = el;
    this.startOffset = 0;
    this.endOffset = el.textContent.length;
  }
  setEnd(container: FakeText, offset: number): void {
    // The real Range throws when the container is not in the range, and the
    // tracker catches exactly that; mirror it rather than clamp.
    if (container !== this.startContainer) throw new Error("container not in range");
    this.endOffset = offset;
  }
  getBoundingClientRect(): { left: number; top: number; width: number; height: number } {
    return { left: 120, top: 340, width: 44, height: 17 };
  }
  getClientRects(): unknown[] {
    return [];
  }
}

class FakeSelection {
  readonly rangeCount = 1;
  readonly isCollapsed = false;
  readonly anchorNode: FakeText;
  readonly focusNode: FakeText;
  private readonly range: FakeRange;
  constructor(row: FakeEl, text: FakeText, start: number, end: number) {
    this.anchorNode = text;
    this.focusNode = text;
    this.range = new FakeRange(row, text, start, end);
  }
  toString(): string {
    return this.range.toString();
  }
  getRangeAt(index: number): FakeRange {
    if (index !== 0) throw new Error("selection smoke: only range 0 exists");
    return this.range;
  }
}

// ---------- sandbox ----------

type Dispatched = { type: string; detail: unknown };

const dispatched: Dispatched[] = [];
const docListeners = new Map<string, (e: { target?: unknown }) => void>();
const winListeners = new Map<string, () => void>();
let currentSelection: FakeSelection | null = null;

const sandbox: Record<string, unknown> = {
  console,
  setTimeout,
  clearTimeout,
  Node: { ELEMENT_NODE: 1, TEXT_NODE: 3 },
  CustomEvent: class {
    readonly type: string;
    readonly detail: unknown;
    constructor(type: string, init?: { detail?: unknown }) {
      this.type = type;
      this.detail = init?.detail;
    }
  },
  document: {
    addEventListener(type: string, fn: (e: { target?: unknown }) => void) {
      docListeners.set(type, fn);
    },
    getSelection() {
      return currentSelection;
    },
  },
  dispatchEvent(e: Dispatched) {
    dispatched.push({ type: e.type, detail: e.detail });
    return true;
  },
  addEventListener(type: string, fn: () => void) {
    winListeners.set(type, fn);
  },
};
sandbox.window = sandbox;
sandbox.globalThis = sandbox;

function take(type: string): unknown {
  const at = dispatched.findIndex((e) => e.type === type);
  if (at === -1) throw new Error("selection smoke: no " + type + " event");
  return dispatched.splice(at, 1)[0].detail;
}

const PAGES = "pdfreader:selection-pages";
const DETAIL = "pdfreader:selection-detail";

function host(kind: string, page: number, blockIndex: string | null, text: string) {
  const node = new FakeText(text);
  const rowAttrs: Attrs = kind === "reflow" && blockIndex !== null
    ? { "data-block-index": blockIndex }
    : {};
  const row = new FakeEl("", rowAttrs, kind === "pdf" ? ["textLayer"] : []);
  row.children.push(node);
  node.parentElement = row;
  const hostEl = new FakeEl(
    kind === "reflow" ? `cont-${page}-pg` : `sp${page}-pg`,
    { "data-reader-host": kind, "data-host-page": String(page) },
  );
  hostEl.children.push(row);
  row.parentElement = hostEl;
  return { row, node };
}

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

export async function run(): Promise<void> {
  vm.createContext(sandbox);
  vm.runInContext(readerSrc, sandbox, { filename: "readerEngine.js" });

  // Installing is the bundle's whole job: three listeners, and nothing on the
  // window that belongs to the engine.
  for (const type of ["selectionchange", "mousedown"]) {
    if (!docListeners.has(type)) throw new Error("selection smoke: no document " + type);
  }
  if (!winListeners.has("mouseup")) throw new Error("selection smoke: no window mouseup");
  if (sandbox.PDFReader !== undefined) throw new Error("selection smoke: reader bundle set PDFReader");
  console.log("reader bundle ok: tracker installed with no engine present");

  // A reflowable selection reports its page range at once and, after the
  // debounce, its detail with the block spot the app persists.
  const sentence = "A sentence with a word worth explaining in it.";
  const start = sentence.indexOf("word");
  const reflow = host("reflow", 3, "7", sentence);
  currentSelection = new FakeSelection(reflow.row, reflow.node, start, start + 4);
  docListeners.get("selectionchange")!({});

  const pages = take(PAGES) as { first: number; last: number };
  if (pages.first !== 3 || pages.last !== 3) {
    throw new Error("selection smoke: wrong page range " + JSON.stringify(pages));
  }
  await wait(220);
  const detail = take(DETAIL) as {
    text: string;
    context: string;
    host: string | null;
    spot: { block: number; start: number; end: number } | null;
    rect: { x: number; y: number; width: number; height: number };
  };
  if (detail.text !== "word") throw new Error("selection smoke: wrong text " + detail.text);
  if (detail.host !== "reflow") throw new Error("selection smoke: wrong host " + detail.host);
  if (!detail.spot || detail.spot.block !== 7 || detail.spot.start !== start || detail.spot.end !== start + 4) {
    throw new Error("selection smoke: wrong spot " + JSON.stringify(detail.spot));
  }
  if (detail.context !== sentence) throw new Error("selection smoke: wrong context " + detail.context);
  if (detail.rect.x !== 120 || detail.rect.width !== 44) {
    throw new Error("selection smoke: wrong rect " + JSON.stringify(detail.rect));
  }
  console.log("reflow selection ok: page 3, block 7, spot", JSON.stringify(detail.spot));

  // The same drag inside a PDF's text layer reports the same shape with no
  // spot — a PDF anchors on the page-space rect the app derives itself.
  const pdf = host("pdf", 5, null, "Ink on a canvas, selectable through the text layer.");
  currentSelection = new FakeSelection(pdf.row, pdf.node, 0, 3);
  docListeners.get("selectionchange")!({});
  const pdfPages = take(PAGES) as { first: number; last: number };
  if (pdfPages.first !== 5 || pdfPages.last !== 5) {
    throw new Error("selection smoke: wrong pdf page range " + JSON.stringify(pdfPages));
  }
  await wait(220);
  const pdfDetail = take(DETAIL) as { text: string; host: string | null; spot: unknown };
  if (pdfDetail.text !== "Ink" || pdfDetail.host !== "pdf" || pdfDetail.spot !== null) {
    throw new Error("selection smoke: wrong pdf detail " + JSON.stringify(pdfDetail));
  }
  console.log("pdf selection ok: page 5, host pdf, no block spot");

  // Losing the selection clears the range once — the transition the sidebar's
  // pinning and the pill's dismissal both wait for.
  currentSelection = null;
  docListeners.get("selectionchange")!({});
  const cleared = take(PAGES);
  if (cleared !== null) throw new Error("selection smoke: clear sent " + JSON.stringify(cleared));
  // The debounced detail pass reports the collapse as a null detail, which is
  // what dismisses the pill; waiting for it here also drains the tracker's
  // timer so nothing fires during a later scenario.
  await wait(220);
  const clearedDetail = take(DETAIL);
  if (clearedDetail !== null) {
    throw new Error("selection smoke: clear sent detail " + JSON.stringify(clearedDetail));
  }
  console.log("selection clear ok: pages and detail reset to null");
}
