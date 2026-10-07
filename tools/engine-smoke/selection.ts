import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

import { exportedStrings } from "../repo.js";

// The reader bundle alone: it must install with no engine and no pdf.js.

const readerSrc = readFileSync(
  new URL("../../public/readerEngine.js", import.meta.url),
  "utf8",
);

// the smallest DOM the tracker reads

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
  readonly isConnected = true;
  getAttribute(name: string): string | null {
    return Object.prototype.hasOwnProperty.call(this.attrs, name) ? this.attrs[name] : null;
  }
  /** Raised here, the event bubbles to the window: recorded there. */
  dispatchEvent(e: Dispatched): boolean {
    dispatched.push({ type: e.type, detail: e.detail, bubbles: e.bubbles, target: this });
    return true;
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

// Host selectors come in four shapes; anything else throws.
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

// One text node per row keeps the range arithmetic honest.
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
    // The real Range throws when the container is outside it; do not clamp.
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

// sandbox

type Dispatched = { type: string; detail: unknown; bubbles?: boolean; target?: unknown };

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
    readonly bubbles: boolean;
    constructor(type: string, init?: { detail?: unknown; bubbles?: boolean }) {
      this.type = type;
      this.detail = init?.detail;
      this.bubbles = init?.bubbles ?? false;
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
    dispatched.push({ type: e.type, detail: e.detail, bubbles: e.bubbles, target: sandbox });
    return true;
  },
  addEventListener(type: string, fn: () => void) {
    winListeners.set(type, fn);
  },
};
sandbox.window = sandbox;
sandbox.globalThis = sandbox;

function takeEvent(type: string): Dispatched {
  const at = dispatched.findIndex((e) => e.type === type);
  if (at === -1) throw new Error("selection smoke: no " + type + " event");
  return dispatched.splice(at, 1)[0];
}

// Several panes listen; each keeps only the events raised inside its own root.
function takeFrom(type: string, origin: unknown): unknown {
  const e = takeEvent(type);
  if (e.target !== origin) {
    throw new Error(`selection smoke: ${type} raised on the wrong target`);
  }
  if (origin !== sandbox && !e.bubbles) {
    throw new Error(`selection smoke: ${type} raised on its host must bubble to the window`);
  }
  return e.detail;
}

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
  return { row, node, hostEl };
}

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

export async function run(): Promise<void> {
  vm.createContext(sandbox);
  vm.runInContext(readerSrc, sandbox, { filename: "readerEngine.js" });

  // Event names come from the engine's table, read by check-events' helper.
  const events = exportedStrings(
    fileURLToPath(new URL("../../public/engine/events.ts", import.meta.url)),
  );
  const PAGES = events.get("SELECTION_PAGES_EVENT");
  const DETAIL = events.get("SELECTION_DETAIL_EVENT");
  if (!PAGES || !DETAIL) {
    throw new Error("selection smoke: public/engine/events.ts no longer declares both events");
  }

  // Installing is the bundle's whole job: three listeners, none the engine's.
  for (const type of ["selectionchange", "mousedown"]) {
    if (!docListeners.has(type)) throw new Error("selection smoke: no document " + type);
  }
  if (!winListeners.has("mouseup")) throw new Error("selection smoke: no window mouseup");
  if (sandbox.PDFReader !== undefined) throw new Error("selection smoke: reader bundle set PDFReader");
  console.log("reader bundle ok: tracker installed with no engine present");

  // A reflowable selection reports its range at once, then its detail.
  const sentence = "A sentence with a word worth explaining in it.";
  const start = sentence.indexOf("word");
  const reflow = host("reflow", 3, "7", sentence);
  currentSelection = new FakeSelection(reflow.row, reflow.node, start, start + 4);
  docListeners.get("selectionchange")!({});

  const pages = takeFrom(PAGES, reflow.hostEl) as { first: number; last: number };
  if (pages.first !== 3 || pages.last !== 3) {
    throw new Error("selection smoke: wrong page range " + JSON.stringify(pages));
  }
  await wait(220);
  const detail = takeFrom(DETAIL, reflow.hostEl) as {
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

  // A PDF drag reports the same shape with no spot: the app derives it.
  const pdf = host("pdf", 5, null, "Ink on a canvas, selectable through the text layer.");
  currentSelection = new FakeSelection(pdf.row, pdf.node, 0, 3);
  docListeners.get("selectionchange")!({});
  const pdfPages = takeFrom(PAGES, pdf.hostEl) as { first: number; last: number };
  if (pdfPages.first !== 5 || pdfPages.last !== 5) {
    throw new Error("selection smoke: wrong pdf page range " + JSON.stringify(pdfPages));
  }
  await wait(220);
  const pdfDetail = takeFrom(DETAIL, pdf.hostEl) as { text: string; host: string | null; spot: unknown };
  if (pdfDetail.text !== "Ink" || pdfDetail.host !== "pdf" || pdfDetail.spot !== null) {
    throw new Error("selection smoke: wrong pdf detail " + JSON.stringify(pdfDetail));
  }
  console.log("pdf selection ok: page 5, host pdf, no block spot");

  // Losing the selection clears the range once: the sidebar's cue.
  currentSelection = null;
  docListeners.get("selectionchange")!({});
  const cleared = takeFrom(PAGES, sandbox);
  if (cleared !== null) throw new Error("selection smoke: clear sent " + JSON.stringify(cleared));
  // The debounced detail pass reports a null detail, which dismisses the pill.
  await wait(220);
  const clearedDetail = takeFrom(DETAIL, sandbox);
  if (clearedDetail !== null) {
    throw new Error("selection smoke: clear sent detail " + JSON.stringify(clearedDetail));
  }
  console.log("selection clear ok: pages and detail reset to null, on the window");
}
