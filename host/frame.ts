import { envelope, packet, type Payload, type ReaderConfig } from "./protocol";
import type { ReaderRuntime } from "./lifecycle";
import { tauri, TauriScope } from "./tauri";

const READY_TIMEOUT_MS = 30_000;
const DISPOSE_TIMEOUT_MS = 5_000;
/** Frame ownership is centralized here. No other host code accesses its DOM.
 *  One instance owns exactly one iframe + MessagePort + timers + listeners.
 *  Generation IDs guard stale async messages after a pane is closing.
 */
export class FrameRuntime implements ReaderRuntime {
  private readonly id: string;
  private readonly generation: string;
  private frame: HTMLIFrameElement | null;
  private port: MessagePort | null = null;
  private scope: TauriScope;
  private readyPromise: Promise<void>;
  private resolveReady!: () => void;
  private rejectReady!: (reason: Error) => void;
  private readyTimer: ReturnType<typeof setTimeout>;
  private disposeTimer: ReturnType<typeof setTimeout> | undefined;
  private disposal: Promise<void> | null = null;
  private resolveDisposed: (() => void) | null = null;
  private loaded = false;
  private queued: Payload[] = [];
  private removed = false;
  private requestId = 0;
  private disposeId = 0;
  private event: ((event: Payload) => void) | null;
  private loadAbort: AbortController | null = null;
  private listeners: Array<() => void> = [];
  private disposed = false;

  constructor(id: string, target: HTMLElement, config: ReaderConfig, event: (event: Payload) => void,
    closeWindow: () => Promise<void>, kind: "reader" | "library" = "reader") {
    this.id = id;
    this.generation = (globalThis.crypto as unknown as { randomUUID?: () => string })?.randomUUID?.() ??
      [...crypto.getRandomValues(new Uint32Array(4))].map(v => v.toString(16).padStart(8, "0")).join("");
    this.event = event;
    this.scope = new TauriScope(config.path, (message) => this.command(message), closeWindow, kind);
    this.readyPromise = new Promise((resolve, reject) => { this.resolveReady = resolve; this.rejectReady = reject; });
    void this.readyPromise.catch(() => undefined);
    this.readyTimer = setTimeout(() => this.rejectReady(new Error("Reader startup timed out")), READY_TIMEOUT_MS);
    this.loadAbort = new AbortController();
    const frame = document.createElement("iframe");
    frame.className = `${kind}-frame`;
    // Stable selectors for tests and CSS — plan §11 expects data-runtime-kind.
    frame.dataset.runtimeKind = kind;
    frame.dataset.generation = this.generation;
    frame.title = kind === "library" ? "Library" : `${config.format.toUpperCase()} reader`;
    frame.src = kind === "library" ? "/library/index.html" : `/reader-${config.format}/index.html`;
    // Keep a null src before removal to help the browser release the WASM realm eagerly.
    frame.setAttribute("loading", "eager");
    this.frame = frame;
    const onLoad = () => this.connect();
    const onError = () => this.loadError();
    frame.addEventListener("load", onLoad, { once: true, signal: this.loadAbort.signal });
    frame.addEventListener("error", onError, { once: true, signal: this.loadAbort.signal });
    this.listeners.push(() => frame.removeEventListener("load", onLoad));
    this.listeners.push(() => frame.removeEventListener("error", onError));
    target.append(frame);
  }
  private loadError = (): void => { this.rejectReady(new Error("Could not load reader frame")); };
  private connect = (): void => {
    if (this.removed || this.disposal || !this.frame?.contentWindow) return;
    const channel = new MessageChannel();
    this.port = channel.port1;
    this.port.onmessage = (event: MessageEvent<unknown>) => { void this.receive(event.data); };
    this.port.onmessageerror = () => { this.rejectReady(new Error("Invalid reader bridge message")); };
    this.port.start();
    const origin = location.origin === "null" ? "*" : location.origin;
    // Send generation so the child can tag all future messages.
    this.frame.contentWindow.postMessage(packet({ type: "connect", generation: this.generation, tauri: !!tauri() } as unknown as Payload), origin, [channel.port2]);
  };
  private async receive(value: unknown): Promise<void> {
    if (this.removed || !envelope(value)) return;
    const message = value.payload as Payload & { generation?: string };
    // Reject stale generation — an old async render that outlived its pane.
    if (message.generation && message.generation !== this.generation) return;
    if (message.type === "ready") {
      if (this.disposal) return;
      console.log("[runtime] READY", this.id, this.generation);
      this.loaded = true;
      clearTimeout(this.readyTimer);
      this.resolveReady();
      for (const command of this.queued.splice(0)) this.command(command);
    } else if (message.type === "disposed" && value.requestId === this.disposeId) {
      this.finish();
    } else if (message.type === "rpc" && value.requestId !== undefined) {
      try {
        const result = await this.scope.call(message.method as string, message.args);
        this.port?.postMessage(packet({ type: "rpc-result", result } as Payload, value.requestId));
      } catch (error) {
        this.port?.postMessage(packet({ type: "rpc-result", error: String(error) } as Payload, value.requestId));
      }
    } else if (["progress", "metadata", "cover", "settings", "close-request", "reload-request", "open-path", "error", "title", "focus", "snapshot", "open-request", "library-ready", "paper-color", "thumbnail", "thumbnail-request", "workspace-ready", "library-tree", "chrome-state", "chrome-change", "pane-command", "pane-focus", "pane-close", "panes", "active-state", "controls", "snapshot"].includes(message.type)) {
      this.event?.(message);
    }
  }
  ready(): Promise<void> { return this.readyPromise; }
  command(command: Payload): void {
    if (this.removed || this.disposal) return;
    // Tag host->child commands with generation so child can confirm.
    const tagged = { ...command, generation: this.generation } as Payload;
    if (!this.loaded) { this.queued.push(tagged); return; }
    this.port?.postMessage(packet(tagged, ++this.requestId));
  }
  dispose(): Promise<void> {
    if (this.disposal) return this.disposal;
    if (this.disposed) return Promise.resolve();
    this.disposed = true;
    console.log("[runtime] DISPOSE", this.id, this.generation);
    this.disposal = new Promise((resolve) => { this.resolveDisposed = resolve; });
    this.rejectReady(new Error("Reader startup cancelled"));
    clearTimeout(this.readyTimer);
    this.loadAbort?.abort();
    this.loadAbort = null;
    for (const remove of this.listeners.splice(0)) remove();
    if (!this.loaded || !this.port) { this.finish(); return this.disposal; }
    this.disposeId = ++this.requestId;
    this.disposeTimer = setTimeout(() => {
      this.event?.({ type: "error", message: "Reader cleanup timed out; the frame was removed. The last confirmed position is kept." });
      this.finish();
    }, DISPOSE_TIMEOUT_MS);
    this.port.postMessage(packet({ type: "dispose", generation: this.generation } as Payload, this.disposeId));
    return this.disposal;
  }
  private finish(): void {
    if (this.removed) return;
    this.removed = true;
    clearTimeout(this.readyTimer);
    clearTimeout(this.disposeTimer);
    console.log("[runtime] DISPOSED", this.id, this.generation);
    this.scope.dispose();
    this.queued = [];
    for (const remove of this.listeners.splice(0)) remove();
    this.loadAbort?.abort();
    this.loadAbort = null;
    if (this.port) { this.port.onmessage = null; this.port.onmessageerror = null; this.port.close(); this.port = null; }
    // Hard lifetime boundary for the WASM instance — port close, src blank, removal, null.
    const frame = this.frame;
    if (frame) {
      console.log("[runtime] iframe still connected?", this.id, frame.isConnected);
      try { frame.src = "about:blank"; } catch {}
      frame.remove();
      console.log("[runtime] AFTER REMOVE", this.id, frame.isConnected);
    }
    this.frame = null;
    this.event = null;
    const resolve = this.resolveDisposed;
    this.resolveDisposed = null;
    resolve?.();
  }
}
