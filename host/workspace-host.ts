import { FrameRuntime } from "./frame";
import { PaneRuntimeManager, panesPayload, type PaneMeta } from "./panes";
import { OUTPUT_EVENT, localCommand, payload, readerConfig, record, type Payload, type ReaderConfig } from "./protocol";
import { MIME, hit, internalDrop, leaves, place, rectangles, remove, type Target, type Tree } from "./layout";
import { tauri } from "./tauri";

interface Book { id: string; title: string; format: string; missing: boolean }
type TreeRoot =
  | { kind: "folder"; id: string; title: string; children: TreeRoot[] }
  | { kind: "book"; id: string; title: string; format: string; missing: boolean };
interface Meta { config: ReaderConfig; base: Record<string, unknown>; snapshot: Record<string, unknown>; paper?: string }
// getRandomValues also works on packaged origins without randomUUID support.
const dragToken = (): string => [...crypto.getRandomValues(new Uint32Array(4))].map((v) => v.toString(16).padStart(8, "0")).join("");
const clone = <T>(value: T): T => structuredClone(value);
const same = (a: unknown, b: unknown): boolean => JSON.stringify(a) === JSON.stringify(b);

export function bootWorkspace(): void {
  const root = document.getElementById("workspace-root")!;
  const status = document.getElementById("host-status")!;
  const metas = new Map<string, Meta>();
  const slots = new Map<string, HTMLElement>();
  let tree: Tree | null = null;
  let active: string | null = null;
  let nextId = 0;
  let books: Book[] = [];
  let drag: { token: string; bookId: string } | null = null;
  let drop: Target | null = null;
  let pendingDrop: { target: Target; bookId: string } | null = null;
  let overlay: HTMLElement | null = null;
  let chromeState: Payload = { type: "chrome-state", bar: false, rail: false };
  let shared: { appearance: unknown; source: string; paper?: string } | null = null;
  // Pane lifecycle bookkeeping for the payload: a pane is loading between
  // allocation and READY, closing between DISPOSE and the map delete.
  let loading = new Set<string>();
  let closing = new Set<string>();
  // A finished drag must not also fire the row's click open.
  let suppressClick = false;
  // Rows the drag session is already bound to: the workspace re-renders them
  // through Leptos, and a re-bind would stack a second capture per row.
  const boundRows = new WeakSet<HTMLElement>();
  let queue = Promise.resolve();
  let shuttingDown = false;
  let navigationEpoch = 0;
  const manager = new PaneRuntimeManager((id, config, event) => new FrameRuntime(id, slots.get(id)!, config, event, closeWindow), paneEvent);
  Object.defineProperty(window, "__MAREADER_DEBUG__", { configurable: true, value: {
    workspace: true, panes: manager.panes,
    get activePane() { return active; }, get tree() { return clone(tree); },
    get dragging() { return drag !== null; },
    get dropTarget() { return clone(drop); },
    // Value-only inspection; no second ownership map or frame references.
    get blend() { return clone(shared); },
    get profiles() { return [...metas].map(([id, meta]) => ({ id,
      base: clone(meta.base.appearance), effective: clone(effective(meta).appearance) })); },
  } });
  function run(work: () => Promise<void>): void {
    queue = queue.then(work).catch((error: unknown) => {
      if (String(error).includes("Reader startup cancelled")) return;
      status.hidden = false; status.textContent = String(error);
      localCommand({ type: "error", message: String(error) });
    });
  }
  function scheduleOpen(config: ReaderConfig, target: Target | null): void {
    const epoch = navigationEpoch;
    run(async () => { if (epoch === navigationEpoch) await open(config, target); });
  }
  function requestCloseAll(): void {
    navigationEpoch++;
    // Interrupt READY immediately; do not strand a close behind the startup
    // deadline in the serialized layout queue.
    void manager.closeAll();
    run(() => closeAll());
  }
  function layout(): HTMLElement { return document.getElementById("workspace-layout")!; }
  function paint(): void {
    const many = leaves(tree).length > 1;
    for (const [id, r] of rectangles(tree, { x: 0, y: 0, width: 100, height: 100 })) {
      const slot = slots.get(id);
      if (!slot) continue;
      Object.assign(slot.style, { left: `${r.x}%`, top: `${r.y}%`, width: `${r.width}%`, height: `${r.height}%` });
      slot.classList.toggle("active", id === active);
      slot.classList.toggle("split", many);
      const close = slot.querySelector<HTMLButtonElement>(".pane-close")!;
      close.hidden = !many;
      // Top-row close buttons must stay below the shared 48px titlebar;
      // otherwise entering its hover band covers the button before click.
      close.style.top = r.y === 0 ? "54px" : "6px";
    }
  }
  function effective(meta: Meta): Record<string, unknown> {
    if (!shared) return clone(meta.base);
    const settings = clone(meta.base);
    settings.appearance = clone(shared.appearance);
    settings.layout = { ...(record(settings.layout) ? settings.layout : {}), blend_mode: true };
    return settings;
  }
  // The workspace's sidebar derives from this one payload: the Active section
  // shows and hides on it, and its backdrop reads the shared Blend paper off
  // it. Published after every registry change: open, ready, close, failed,
  // dispose, and active-pane change.
  function publishPanes(): void {
    const entries: PaneMeta[] = [...metas].map(([id, meta]) => ({
      id,
      bookId: meta.config.bookId,
      title: meta.config.title,
      format: meta.config.format,
      loading: loading.has(id),
      closing: closing.has(id),
    }));
    localCommand(panesPayload(active, entries, shared?.paper ?? null));
  }
  function syncChrome(): void {
    const meta = active ? metas.get(active) : undefined;
    if (!meta || !active) { localCommand({ type: "active-state", paneId: null }); return; }
    localCommand({ type: "active-state", paneId: active, config: { ...meta.config, settings: effective(meta) }, snapshot: meta.snapshot });
  }
  function focus(id: string): void {
    if (!metas.has(id) || active === id) return;
    // Flush root slider commits synchronously before changing the host target.
    localCommand({ type: "flush-chrome" });
    active = id; paint(); syncChrome();
    localCommand({ type: "focus", paneId: id });
    publishPanes();
  }
  function updateBlend(): void {
    for (const [id, meta] of metas) {
      const settings = effective(meta);
      meta.config.settings = settings;
      manager.command(id, { type: "set-settings", settings });
      manager.command(id, { type: "set-blend", paper: shared?.paper ?? null });
    }
    // The workspace paints the shared paper itself; the host only publishes.
    publishPanes();
  }
  function chromeChange(message: Payload): void {
    const id = String(message.paneId), meta = metas.get(id);
    if (!meta || !record(message.settings)) return;
    const settings = message.settings;
    if (!same(settings, effective(meta))) {
      const blend = record(settings.layout) && settings.layout.blend_mode === true;
      if (blend && !shared) {
        shared = { appearance: clone(settings.appearance), source: id, paper: meta.paper };
      } else if (!blend && shared) {
        shared = null;
      } else if (shared) {
        shared.appearance = clone(settings.appearance);
        // Document-local controls still target the focused pane, not its peers.
        meta.base = { ...clone(settings), appearance: meta.base.appearance,
          layout: { ...(record(settings.layout) ? settings.layout : {}), blend_mode: false } };
      } else {
        meta.base = clone(settings);
        localCommand({ type: "settings", settings: meta.base });
      }
      updateBlend(); syncChrome();
    }
    if (record(message.controls)) {
      const changes: Payload = { type: "controls" };
      for (const key of ["mode", "fit", "page", "autoScroll", "search"]) {
        if (message.controls[key] !== undefined && !same(message.controls[key], meta.snapshot[key])) changes[key] = message.controls[key];
      }
      if (Object.keys(changes).length > 1) manager.command(id, changes);
    }
  }
  function paneEvent(id: string, message: Payload): void {
    const meta = metas.get(id);
    if (!meta) return;
    if (message.type === "focus") focus(id);
    else if (message.type === "snapshot" && record(message.snapshot)) {
      meta.snapshot = message.snapshot;
      if (active === id) {
        // The workspace mirrors chrome through active-state AND tracks the
        // live movement through the snapshot itself: its thumbnail
        // current-page highlight must not wait for a scroll or a focus hop.
        syncChrome();
        localCommand({ type: "snapshot", snapshot: message.snapshot });
      }
    } else if (message.type === "cover" && typeof message.dataUrl === "string") {
      meta.config.cover = message.dataUrl;
      if (active === id) syncChrome();
      localCommand(message);
    } else if (message.type === "settings" && record(message.settings)) {
      // Settings echo the effective view, never replace a Blend base profile.
      meta.config.settings = clone(message.settings);
    } else if (message.type === "paper-color" && typeof message.paper === "string" && /^#[a-f0-9]{6}$/i.test(message.paper)) {
      meta.paper = message.paper;
      if (shared?.source === id) {
        shared.paper = message.paper;
        updateBlend();
        // The blend source's paper is the workspace's backdrop: push it
        // through so the shell does not wait for the next registry publish.
        localCommand(message);
      }
    } else if (message.type === "thumbnail" && active === id && typeof message.dataUrl === "string") {
      // The workspace's grid is the consumer; the host only routes the bitmap.
      localCommand(message);
    } else if (message.type === "close-request") run(() => closePane(id));
    else if (message.type === "reload-request") run(async () => { await closeAll(); location.reload(); });
    else if (message.type === "open-path") localCommand(message);
    else if (message.type === "error") { localCommand(message); run(() => closePane(id)); }
    else localCommand(message);
  }
  async function open(config: ReaderConfig, target: Target | null): Promise<void> {
    if (shuttingDown) return;
    localCommand({ type: "flush-chrome" });
    if (tree && !target) target = { pane: active ?? leaves(tree)[0], edge: "center" };
    const id = `p${++nextId}`;
    const projected = place(tree, target, id); // validation before creating anything
    console.log("[runtime] OPEN", id, config.format);
    if (target?.edge === "center") {
      await manager.close(target.pane);
      slots.get(target.pane)?.remove(); slots.delete(target.pane); metas.delete(target.pane);
    }
    status.hidden = true;
    const slot = document.createElement("section"); slot.className = "workspace-pane"; slot.dataset.pane = id;
    const button = document.createElement("button"); button.className = "pane-close"; button.textContent = "×";
    button.title = "Close pane"; button.setAttribute("aria-label", "Close pane");
    button.onclick = () => { void manager.close(id); run(() => closePane(id)); }; slot.append(button);
    slot.onpointerdown = () => focus(id);
    slots.set(id, slot); layout().append(slot);
    const base = clone(config.settings);
    // A workspace Blend session is ephemeral; a persisted legacy flag cannot
    // silently erase a new pane's base profile.
    if (record(base.layout)) base.layout.blend_mode = false;
    metas.set(id, { config, base, snapshot: { page: config.resumePage, numPages: 1, title: config.title, outline: [] } });
    tree = projected; active = id; paint(); syncChrome();
    loading.add(id);
    publishPanes();
    try {
      console.log("[runtime] panes", metas.size);
      await manager.open(id, { ...config, settings: effective(metas.get(id)!) });
      loading.delete(id);
      if (shared && !metas.has(shared.source)) { shared.source = id; shared.paper = metas.get(id)?.paper; }
      updateBlend();
      manager.command(id, chromeState);
      publishPanes();
    } catch (error) { await closePane(id); throw error; }
  }
  async function closePane(id: string): Promise<void> {
    if (!metas.has(id)) return;
    closing.add(id);
    publishPanes();
    // DISPOSED/timeout fallback -> port close -> frame removal
    await manager.close(id);
    closing.delete(id);
    slots.get(id)?.remove(); slots.delete(id); metas.delete(id);
    tree = remove(tree, id); // collapse only after the runtime is unreachable
    if (active === id) active = leaves(tree)[0] ?? null;
    if (shared?.source === id) {
      const source = [...metas.keys()].find((key) => metas.get(key)?.config.format === "pdf") ?? active;
      if (source) { shared.source = source; shared.paper = metas.get(source)?.paper; } else shared = null;
      updateBlend();
    }
    paint(); syncChrome();
    publishPanes();
  }
  async function closeAll(): Promise<void> {
    endDrag();
    localCommand({ type: "flush-chrome" });
    await manager.closeAll();
    for (const slot of slots.values()) slot.remove();
    slots.clear(); metas.clear(); tree = null; active = null; shared = null;
    loading.clear(); closing.clear();
    // With no pane left, the chrome stops mirroring a document: the shell
    // reads as a home again instead of holding the last pane's title.
    syncChrome();
    publishPanes();
  }
  async function closeWindow(): Promise<void> {
    shuttingDown = true; await closeAll();
    allowClose = true; await tauri()?.window.getCurrentWindow().close();
  }
  // The workspace renders its own rows (Leptos, in this document); the host
  // only gives them a drag session, bound once per row node.
  function bindLibraryDrag(): void {
    for (const row of Array.from(document.querySelectorAll<HTMLButtonElement>(".workspace-book[data-book-id]"))) {
      if (row.disabled || boundRows.has(row)) continue;
      boundRows.add(row);
      let pointerMoved = false;
      row.onpointerdown = (down) => {
        // Tauri's native file-drop handler is kept enabled for OS imports. On
        // WebView2 it intercepts HTML DnD, so internal desktop gestures use
        // pointer capture and feed the SAME MIME/session validator and
        // overlay handlers.
        if (!tauri() || down.button !== 0) return;
        const book = books.find((b) => b.id === row.dataset.bookId);
        if (!book || book.missing) return;
        pointerMoved = false;
        const transfer = new DataTransfer();
        const move = (event: PointerEvent): void => {
          if (event.pointerId !== down.pointerId) return;
          if (!pointerMoved && Math.hypot(event.clientX-down.clientX, event.clientY-down.clientY) > 6) {
            pointerMoved = true;
            row.setPointerCapture(down.pointerId);
            drag = { token: dragToken(), bookId: book.id };
            transfer.setData(MIME, JSON.stringify({ version: 1, source: "library-sidebar", ...drag }));
            startOverlay();
          }
          if (pointerMoved) overlay?.dispatchEvent(new DragEvent("dragover", { dataTransfer: transfer, clientX: event.clientX, clientY: event.clientY, cancelable: true }));
        };
        const finish = (event: PointerEvent): void => {
          if (event.pointerId !== down.pointerId) return;
          window.removeEventListener("pointermove", move);
          window.removeEventListener("pointerup", finish);
          window.removeEventListener("pointercancel", cancel);
          if (row.hasPointerCapture(down.pointerId)) row.releasePointerCapture(down.pointerId);
          if (pointerMoved && event.type === "pointerup") {
            overlay?.dispatchEvent(new DragEvent("drop", { dataTransfer: transfer, clientX: event.clientX, clientY: event.clientY, cancelable: true }));
            // The pointer release lands a split, not an open: the click the
            // browser synthesises next must not also fire the row's open.
            suppressClick = true;
            setTimeout(() => { suppressClick = false; }, 0);
          }
          endDrag();
        };
        const cancel = (event: PointerEvent): void => finish(event);
        window.addEventListener("pointermove", move);
        window.addEventListener("pointerup", finish);
        window.addEventListener("pointercancel", cancel);
      };
      row.ondragstart = (event) => {
        if (tauri()) { event.preventDefault(); return; }
        const book = books.find((b) => b.id === row.dataset.bookId);
        if (!event.dataTransfer || !book || book.missing) return;
        drag = { token: dragToken(), bookId: book.id };
        event.dataTransfer.setData(MIME, JSON.stringify({ version: 1, source: "library-sidebar", ...drag }));
        event.dataTransfer.effectAllowed = "copy";
        // The browser must capture the source before inserting the hit surface.
        requestAnimationFrame(() => { if (drag) startOverlay(); });
      };
      row.ondragend = endDrag;
    }
  }
  function endDrag(): void { drag = null; drop = null; overlay?.remove(); overlay = null; }
  function startOverlay(): void {
    overlay?.remove();
    const surface = document.createElement("div"); overlay = surface; surface.className = "workspace-drop-overlay";
    surface.ondragover = (event) => {
      if (!drag || !event.dataTransfer?.types.includes(MIME) || event.dataTransfer.types.includes("Files")) return;
      event.preventDefault();
      const r = layout().getBoundingClientRect();
      const target = hit(tree, { x: 0, y: 0, width: r.width, height: r.height }, event.clientX-r.x, event.clientY-r.y);
      surface.replaceChildren(); drop = null;
      if (!target) return;
      try {
        const projected = place(tree, target, "preview");
        for (const [id, rect] of rectangles(projected, { x: 0, y: 0, width: 100, height: 100 })) {
          const box = document.createElement("div"); box.className = `workspace-preview${id === "preview" ? " incoming" : ""}`;
          Object.assign(box.style, { left: `${rect.x}%`, top: `${rect.y}%`, width: `${rect.width}%`, height: `${rect.height}%` });
          box.textContent = id === "preview" ? books.find((b) => b.id === drag?.bookId)?.title ?? "Open book" : metas.get(id)?.config.title ?? "";
          surface.append(box);
        }
        drop = target; event.dataTransfer.dropEffect = "copy";
      } catch { event.dataTransfer.dropEffect = "none"; }
    };
    surface.ondragleave = (event) => { if (!surface.contains(event.relatedTarget as Node | null)) { surface.replaceChildren(); drop = null; } };
    surface.ondrop = (event) => {
      event.preventDefault();
      let value: unknown;
      try { value = JSON.parse(event.dataTransfer?.getData(MIME) ?? "null"); } catch { endDrag(); return; }
      if (internalDrop(value, drag) && !event.dataTransfer?.types.includes("Files") && drop && drag) {
        pendingDrop = { target: drop, bookId: drag.bookId };
        localCommand({ type: "open-book", bookId: drag.bookId });
      }
      endDrag();
    };
    layout().append(surface);
  }
  window.addEventListener("dragend", endDrag);
  // DOM drops must not navigate the host away. Native file imports arrive
  // through the separate Tauri event; only the authenticated overlay splits.
  window.addEventListener("dragover", (event) => { if (event.dataTransfer?.types.includes("Files")) event.preventDefault(); });
  window.addEventListener("drop", (event) => event.preventDefault());
  window.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && drag) {
      event.preventDefault(); event.stopImmediatePropagation(); endDrag();
    }
  }, { capture: true });
  // A drag that ends on a split must swallow the row click it would
  // otherwise synthesise; the open belongs to the drop, not the release.
  window.addEventListener("click", (event) => {
    if (!suppressClick) return;
    event.stopPropagation();
    event.stopImmediatePropagation();
  }, { capture: true });
  window.addEventListener(OUTPUT_EVENT, (event) => {
    const message: unknown = (event as CustomEvent).detail;
    if (!payload(message)) return;
    if (message.type === "workspace-ready") {
      run(takePendingFile);
    } else if (message.type === "library-tree") {
      // The tree is hierarchical: root shelves with nested levels, then the
      // unfiled books. The host keeps the flat book list the drag session
      // and the drop preview look up, walked out of the same roots.
      const roots = Array.isArray(message.roots) ? message.roots as TreeRoot[] : [];
      const next: Book[] = [];
      const walk = (node: TreeRoot): void => {
        if (node.kind === "book") next.push({ id: node.id, title: node.title, format: node.format, missing: node.missing === true });
        else for (const child of node.children) walk(child);
      };
      for (const node of roots) walk(node);
      const changed = !same(books, next);
      books = next;
      if (changed && !drag) bindLibraryDrag();
    } else if (message.type === "open-request" && readerConfig(message.config)) {
      const config = message.config;
      const target = pendingDrop?.bookId === config.bookId ? pendingDrop.target : null;
      pendingDrop = null; scheduleOpen(config, target);
    } else if (message.type === "chrome-state") {
      chromeState = message;
      for (const id of manager.panes.keys()) manager.command(id, message);
    }
    else if (message.type === "chrome-change") chromeChange(message);
    else if (message.type === "pane-command" && typeof message.paneId === "string" && payload(message.command)) manager.command(message.paneId, message.command);
    else if (message.type === "thumbnail-request" && typeof message.page === "number") {
      // The workspace's grid asks; the host answers from the PDF engine the
      // active pane runs, and only while that pane is the one being shown.
      if (active && metas.get(active)?.config.format === "pdf") {
        manager.command(active, { type: "request-thumbnails", page: Number(message.page) });
      }
    } else if (message.type === "pane-focus" && typeof message.paneId === "string") {
      focus(String(message.paneId));
    } else if (message.type === "pane-close" && typeof message.paneId === "string") {
      const id = String(message.paneId);
      void manager.close(id);
      run(() => closePane(id));
    } else if (message.type === "close-all" || message.type === "close-request") requestCloseAll();
    else if (message.type === "reload-request") run(async () => { await closeAll(); location.reload(); });
    else if (message.type === "appearance-preview") {
      for (const id of shared ? metas.keys() : active ? [active] : []) manager.command(id, message);
    }
  });
  let allowClose = false;
  const stops: (() => void)[] = [];
  async function takePendingFile(): Promise<void> {
    const path = await tauri()?.core.invoke("take_pending_file");
    if (typeof path === "string" && path) localCommand({ type: "open-path", path });
  }
  function retain(promise: Promise<unknown> | undefined): void {
    void promise?.then((stop) => { if (typeof stop === "function") { if (shuttingDown) stop(); else stops.push(stop as () => void); } });
  }
  retain(tauri()?.event.listen("document-open-file", () => run(takePendingFile)));
  retain(tauri()?.window.getCurrentWindow().onCloseRequested?.((event: unknown) => {
    if (allowClose || !record(event) || typeof event.preventDefault !== "function") return;
    event.preventDefault(); shuttingDown = true; navigationEpoch++; void manager.closeAll(); run(closeWindow);
  }));
  // Native file drops are ordinary opens, never split input. Desktop book
  // drags use pointer capture so native interception stays enabled.
  retain(tauri()?.event.listen("tauri://drag-drop", (event: unknown) => {
    if (!record(event) || !record(event.payload) || !Array.isArray(event.payload.paths)) return;
    const path = event.payload.paths[0];
    if (typeof path === "string") localCommand({ type: "open-path", path });
  }));
  window.addEventListener("popstate", requestCloseAll);
  window.addEventListener("pagehide", () => { shuttingDown = true; stops.forEach((stop) => stop()); void closeAll(); }, { once: true });
  // The workspace re-renders its sidebar through Leptos; the host only binds
  // its drag session to whatever rows just appeared.
  const observer = new MutationObserver((changes) => {
    if (changes.some((change) => [...change.addedNodes].some((node) => node instanceof Element && (node.matches(".workspace-book") || node.querySelector(".workspace-book"))))) bindLibraryDrag();
  });
  observer.observe(root, { childList: true, subtree: true });
  window.addEventListener("pagehide", () => observer.disconnect(), { once: true });
}
