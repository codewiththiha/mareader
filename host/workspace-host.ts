import { FrameRuntime } from "./frame";
import { PaneRuntimeManager } from "./panes";
import { COMMAND_EVENT, OUTPUT_EVENT, localCommand, payload, readerConfig, record, type Payload, type ReaderConfig } from "./protocol";
import { MIME, hit, internalDrop, leaves, place, rectangles, remove, type Target, type Tree } from "./layout";
import { tauri } from "./tauri";

interface Book { id: string; title: string; path: string; format: string; missing: boolean }
interface Shelf { id: string; name: string; parent: string | null; books: string[] }
interface Meta { config: ReaderConfig; base: Record<string, unknown>; snapshot: Record<string, unknown>; paper?: string }
// getRandomValues also works on packaged origins without randomUUID support.
const dragToken = (): string => [...crypto.getRandomValues(new Uint32Array(4))].map((v) => v.toString(16).padStart(8, "0")).join("");
const clone = <T>(value: T): T => structuredClone(value);
const same = (a: unknown, b: unknown): boolean => JSON.stringify(a) === JSON.stringify(b);

export function bootWorkspace(): void {
  const root = document.getElementById("workspace-root")!;
  const libraryRoot = document.getElementById("library-root")!;
  const status = document.getElementById("host-status")!;
  const metas = new Map<string, Meta>();
  const slots = new Map<string, HTMLElement>();
  let library: FrameRuntime | null = null;
  let tree: Tree | null = null;
  let active: string | null = null;
  let nextId = 0;
  let books: Book[] = [], shelves: Shelf[] = [];
  let tab = "library";
  const expanded = new Set<string>();
  let drag: { token: string; bookId: string } | null = null;
  let drop: Target | null = null;
  let pendingDrop: { target: Target; bookId: string } | null = null;
  let overlay: HTMLElement | null = null;
  let chromeState: Payload = { type: "chrome-state", bar: false, rail: false };
  let shared: { appearance: unknown; source: string; paper?: string } | null = null;
  let queue = Promise.resolve();
  let shuttingDown = false;
  let navigationEpoch = 0;
  const manager = new PaneRuntimeManager((id, config, event) => new FrameRuntime(slots.get(id)!, config, event, closeWindow), paneEvent);
  Object.defineProperty(window, "__MAREADER_DEBUG__", { configurable: true, value: {
    workspace: true, panes: manager.panes,
    get activePane() { return active; }, get tree() { return clone(tree); },
    get library() { return library !== null; },
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
  function syncChrome(): void {
    const meta = active ? metas.get(active) : undefined;
    if (!meta || !active) { localCommand({ type: "active-state", paneId: null }); return; }
    localCommand({ type: "active-state", paneId: active, config: { ...meta.config, settings: effective(meta) }, snapshot: meta.snapshot });
  }
  function focus(id: string): void {
    if (!metas.has(id) || active === id) return;
    // Flush root slider commits synchronously before changing the host target.
    localCommand({ type: "flush-chrome" });
    active = id; paint(); syncChrome(); renderSidebar();
  }
  function updateBlend(): void {
    for (const [id, meta] of metas) {
      const settings = effective(meta);
      meta.config.settings = settings;
      manager.command(id, { type: "set-settings", settings });
      manager.command(id, { type: "set-blend", paper: shared?.paper ?? null });
    }
    layout().style.background = shared?.paper ?? "";
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
      const structural = !same(meta.snapshot.outline, message.snapshot.outline) || meta.snapshot.numPages !== message.snapshot.numPages;
      meta.snapshot = message.snapshot;
      if (active === id) { syncChrome(); if (structural && tab !== "library") renderSidebar(); }
    } else if (message.type === "cover" && typeof message.dataUrl === "string") {
      meta.config.cover = message.dataUrl;
      if (active === id) syncChrome();
      localCommand(message);
    } else if (message.type === "settings" && record(message.settings)) {
      // Settings echo the effective view, never replace a Blend base profile.
      meta.config.settings = clone(message.settings);
    } else if (message.type === "paper-color" && typeof message.paper === "string" && /^#[a-f0-9]{6}$/i.test(message.paper)) {
      meta.paper = message.paper;
      if (shared?.source === id) { shared.paper = message.paper; updateBlend(); }
    } else if (message.type === "thumbnail" && active === id && tab === "thumbnails" && typeof message.dataUrl === "string") {
      document.querySelectorAll<HTMLImageElement>(`.workspace-sidebar img[data-page="${Number(message.page)}"]`).forEach((image) => { image.src = String(message.dataUrl); });
      const next = document.querySelector<HTMLImageElement>(".workspace-sidebar img[data-page]:not([src])");
      if (next) manager.command(id, { type: "request-thumbnails", page: Number(next.dataset.page) });
    } else if (message.type === "close-request") run(() => closePane(id));
    else if (message.type === "reload-request") run(async () => { await closeAll(false); location.reload(); });
    else if (message.type === "open-path") localCommand(message);
    else if (message.type === "error") { localCommand(message); run(() => closePane(id)); }
    else localCommand(message);
  }
  async function showLibrary(): Promise<void> {
    active = null; shared = null; syncChrome();
    root.hidden = true; libraryRoot.hidden = false;
    if (library || shuttingDown) return;
    const config: ReaderConfig = { bookId: "library", path: "library", format: "txt", title: null, cover: null, resumePage: 1, resumeFraction: null, settings: {} };
    const frame = new FrameRuntime(libraryRoot, config, libraryEvent, closeWindow, "library");
    library = frame;
    try { await frame.ready(); }
    catch (error) { await frame.dispose(); if (library === frame) library = null; throw error; }
    status.hidden = true;
  }
  function libraryEvent(message: Payload): void {
    if (message.type === "open-request" && readerConfig(message.config)) {
      const config = message.config;
      scheduleOpen(config, null);
    } else if (message.type === "open-path") localCommand(message);
    else if (message.type === "reload-request") run(async () => { await closeAll(false); location.reload(); });
    else if (message.type === "error") localCommand(message);
  }
  async function open(config: ReaderConfig, target: Target | null): Promise<void> {
    if (shuttingDown) return;
    localCommand({ type: "flush-chrome" });
    if (library) { await library.dispose(); library = null; localCommand({ type: "reload-library" }); }
    if (tree && !target) target = { pane: active ?? leaves(tree)[0], edge: "center" };
    const id = `p${++nextId}`;
    const projected = place(tree, target, id); // validation before creating anything
    if (target?.edge === "center") {
      await manager.close(target.pane);
      slots.get(target.pane)?.remove(); slots.delete(target.pane); metas.delete(target.pane);
    }
    libraryRoot.hidden = true; root.hidden = false; status.hidden = true;
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
    tree = projected; active = id; paint(); syncChrome(); renderSidebar();
    try {
      await manager.open(id, { ...config, settings: effective(metas.get(id)!) });
      if (shared && !metas.has(shared.source)) { shared.source = id; shared.paper = metas.get(id)?.paper; }
      updateBlend();
      manager.command(id, chromeState);
    } catch (error) { await closePane(id); throw error; }
  }
  async function closePane(id: string): Promise<void> {
    if (!metas.has(id)) return;
    await manager.close(id); // DISPOSED/timeout fallback -> port close -> frame removal
    slots.get(id)?.remove(); slots.delete(id); metas.delete(id);
    tree = remove(tree, id); // collapse only after the runtime is unreachable
    if (active === id) active = leaves(tree)[0] ?? null;
    if (shared?.source === id) {
      const source = [...metas.keys()].find((key) => metas.get(key)?.config.format === "pdf") ?? active;
      if (source) { shared.source = source; shared.paper = metas.get(source)?.paper; } else shared = null;
      updateBlend();
    }
    paint(); syncChrome(); renderSidebar();
    if (!tree) await showLibrary();
  }
  async function closeAll(returnToLibrary = true): Promise<void> {
    endDrag();
    localCommand({ type: "flush-chrome" });
    await manager.closeAll();
    for (const slot of slots.values()) slot.remove();
    slots.clear(); metas.clear(); tree = null; active = null; shared = null;
    if (library && !returnToLibrary) { await library.dispose(); library = null; }
    if (returnToLibrary) await showLibrary();
  }
  async function closeWindow(): Promise<void> {
    shuttingDown = true; await closeAll(false);
    allowClose = true; await tauri()?.window.getCurrentWindow().close();
  }
  function bookRow(book: Book): HTMLElement {
    const row = document.createElement("button"); row.className = "workspace-book"; row.dataset.bookId = book.id;
    row.disabled = book.missing; row.draggable = !book.missing;
    const badge = document.createElement("span"); badge.className = "format-badge"; badge.textContent = book.format.toUpperCase();
    const title = document.createElement("span"); title.textContent = book.title; title.className = "truncate";
    row.append(badge, title);
    let pointerMoved = false;
    row.onclick = () => { if (pointerMoved) { pointerMoved = false; return; } localCommand({ type: "open-book", bookId: book.id }); };
    // Tauri's native file-drop handler is kept enabled for OS imports. On
    // WebView2 it intercepts HTML DnD, so internal desktop gestures use pointer
    // capture and feed the SAME MIME/session validator and overlay handlers.
    row.onpointerdown = (down) => {
      if (!tauri() || book.missing || down.button !== 0) return;
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
        if (pointerMoved && event.type === "pointerup") overlay?.dispatchEvent(new DragEvent("drop", { dataTransfer: transfer, clientX: event.clientX, clientY: event.clientY, cancelable: true }));
        endDrag();
      };
      const cancel = (event: PointerEvent): void => finish(event);
      window.addEventListener("pointermove", move);
      window.addEventListener("pointerup", finish);
      window.addEventListener("pointercancel", cancel);
    };
    row.ondragstart = (event) => {
      if (tauri()) { event.preventDefault(); return; }
      if (!event.dataTransfer || book.missing) return;
      drag = { token: dragToken(), bookId: book.id };
      event.dataTransfer.setData(MIME, JSON.stringify({ version: 1, source: "library-sidebar", ...drag }));
      event.dataTransfer.effectAllowed = "copy";
      // The browser must capture the source before inserting the hit surface.
      requestAnimationFrame(() => { if (drag) startOverlay(); });
    };
    row.ondragend = endDrag;
    return row;
  }
  function renderSidebar(): void {
    document.querySelectorAll<HTMLElement>(".workspace-sidebar").forEach((panel) => {
      panel.replaceChildren(); panel.dataset.tab = tab;
      if (panel.hidden) return;
      if (tab === "library") {
        const appendShelf = (shelf: Shelf, parent: HTMLElement, ancestors: Set<string>): void => {
          if (ancestors.has(shelf.id)) return;
          const seen = new Set(ancestors); seen.add(shelf.id);
          const group = document.createElement("details"); group.open = expanded.has(shelf.id);
          const heading = document.createElement("summary"); heading.textContent = shelf.name; group.append(heading);
          group.ontoggle = () => { if (group.open) expanded.add(shelf.id); else expanded.delete(shelf.id); };
          for (const child of shelves.filter((s) => s.parent === shelf.id)) appendShelf(child, group, seen);
          for (const id of shelf.books) { const book = books.find((b) => b.id === id); if (book) group.append(bookRow(book)); }
          parent.append(group);
        };
        for (const shelf of shelves.filter((s) => !s.parent || !shelves.some((p) => p.id === s.parent))) appendShelf(shelf, panel, new Set());
        const all = document.createElement("details"); all.open = true;
        const heading = document.createElement("summary"); heading.textContent = "All books"; all.append(heading);
        for (const book of books) all.append(bookRow(book));
        panel.append(all);
      } else {
        const meta = active ? metas.get(active) : undefined;
        if (!meta || !active) { panel.textContent = "Open a book to view this panel."; return; }
        const id = active;
        if (tab === "outline") {
          // The original Rust OutlinePanel renders the focused snapshot.
        } else if (meta.config.format !== "pdf") panel.textContent = "Thumbnails are available for PDF documents.";
        else {
          // Bounded window around the current page, not one raster per page.
          const current = Number(meta.snapshot.page ?? 1), total = Number(meta.snapshot.numPages ?? 1);
          const first = Math.max(1, current - 4), last = Math.min(total, first + 11);
          const previous = document.createElement("button"); previous.textContent = "Previous pages";
          previous.onclick = () => { meta.snapshot.page = Math.max(1, first - 12); renderSidebar(); };
          panel.append(previous);
          for (let page = first; page <= last; page++) {
            const row = document.createElement("button"); row.className = "workspace-thumbnail thumb-card";
            const image = document.createElement("img"); image.className = "thumb-canvas"; image.dataset.page = String(page); image.alt = `Page ${page}`;
            const badge = document.createElement("div"); badge.className = `thumb-num${page === current ? " is-current" : ""}`;
            const label = document.createElement("span"); label.textContent = String(page); badge.append(label);
            row.append(image, badge);
            row.onclick = () => manager.command(id, { type: "controls", page }); panel.append(row);
          }
          const next = document.createElement("button"); next.textContent = "Next pages";
          next.onclick = () => { meta.snapshot.page = Math.min(total, last + 5); renderSidebar(); };
          panel.append(next);
          manager.command(id, { type: "request-thumbnails", page: first });
        }
      }
    });
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
  // The library frame is the only writer while its page is mounted. Route
  // external opens there so its final flush cannot overwrite a newer root row.
  window.addEventListener(COMMAND_EVENT, (event) => {
    const value: unknown = (event as CustomEvent).detail;
    if (library && payload(value) && ["open-path", "open-book"].includes(value.type)) {
      event.stopImmediatePropagation(); library.command(value);
    }
  }, { capture: true });
  window.addEventListener(OUTPUT_EVENT, (event) => {
    const message: unknown = (event as CustomEvent).detail;
    if (!payload(message)) return;
    if (message.type === "workspace-ready") {
      requestAnimationFrame(() => {
        localCommand({ type: "reload-library" });
        run(async () => { await showLibrary(); await takePendingFile(); });
      });
    } else if (message.type === "library-tree") {
      const newBooks = Array.isArray(message.books) ? message.books as Book[] : [];
      const newShelves = Array.isArray(message.shelves) ? message.shelves as Shelf[] : [];
      const changed = !same(books, newBooks) || !same(shelves, newShelves);
      books = newBooks; shelves = newShelves;
      if (changed && !drag) renderSidebar();
    } else if (message.type === "open-request" && readerConfig(message.config)) {
      const config = message.config;
      const target = pendingDrop?.bookId === config.bookId ? pendingDrop.target : null;
      pendingDrop = null; scheduleOpen(config, target);
    } else if (message.type === "sidebar-tab") { tab = String(message.tab); renderSidebar(); }
    else if (message.type === "chrome-state") {
      chromeState = message;
      for (const id of manager.panes.keys()) manager.command(id, message);
    }
    else if (message.type === "chrome-change") chromeChange(message);
    else if (message.type === "pane-command" && typeof message.paneId === "string" && payload(message.command)) manager.command(message.paneId, message.command);
    else if (message.type === "close-all" || message.type === "close-request") requestCloseAll();
    else if (message.type === "reload-request") run(async () => { await closeAll(false); location.reload(); });
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
    // The library page owns import-to-shelf (including multi-file drops).
    // Do not simultaneously open its first file from the persistent host.
    if (library) return;
    if (!record(event) || !record(event.payload) || !Array.isArray(event.payload.paths)) return;
    const path = event.payload.paths[0]; if (typeof path === "string") localCommand({ type: "open-path", path });
  }));
  window.addEventListener("popstate", requestCloseAll);
  window.addEventListener("pagehide", () => { shuttingDown = true; stops.forEach((stop) => stop()); void closeAll(false); }, { once: true });
  // Rail placement can remount its panel without remounting the workspace.
  const observer = new MutationObserver((changes) => {
    if (changes.some((change) => [...change.addedNodes].some((node) => node instanceof Element && (node.matches(".workspace-sidebar") || node.querySelector(".workspace-sidebar"))))) renderSidebar();
  });
  observer.observe(root, { childList: true, subtree: true });
  window.addEventListener("pagehide", () => observer.disconnect(), { once: true });
}
