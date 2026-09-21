import { envelope, packet, type Payload, type ReaderConfig } from "./protocol";
import type { ReaderRuntime } from "./lifecycle";
import { tauri, TauriScope } from "./tauri";

const READY_TIMEOUT_MS = 30_000;
const DISPOSE_TIMEOUT_MS = 5_000;
/** Frame ownership is centralized here. No other host code accesses its DOM. */
export class FrameRuntime implements ReaderRuntime {
  private readonly id: string;
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

  constructor(id: string, target: HTMLElement, config: ReaderConfig, event: (event: Payload) => void,
    closeWindow: () => Promise<void>, kind: "reader" | "library" = "reader") {
    this.id = id;
    this.event = event;
    this.scope = new TauriScope(config.path, (message) => this.command(message), closeWindow, kind);
    this.readyPromise = new Promise((resolve, reject) => { this.resolveReady = resolve; this.rejectReady = reject; });
    // Disposal can reject startup before the manager has attached its await.
    void this.readyPromise.catch(() => undefined);
    this.readyTimer = setTimeout(() => this.rejectReady(new Error("Reader startup timed out")), READY_TIMEOUT_MS);
    const frame = document.createElement("iframe");
    frame.className = `${kind}-frame`;
    frame.title = kind === "library" ? "Library" : `${config.format.toUpperCase()} reader`;
    // Same-origin is required for WASM/storage and Tauri's packaged protocol.
    // This is a lifecycle boundary, NOT a sandbox for untrusted code.
    frame.src = kind === "library" ? "/library/index.html" : `/reader-${config.format}/index.html`;
    this.frame = frame;
    frame.addEventListener("load", this.connect, { once: true });
    frame.addEventListener("error", this.loadError, { once: true });
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
    // Packaged tauri:// origins may serialize as "null". Only that case uses
    // *, and the child still verifies event.source === parent and version.
    const origin = location.origin === "null" ? "*" : location.origin;
    this.frame.contentWindow.postMessage(packet({ type: "connect", tauri: !!tauri() }), origin, [channel.port2]);
  };
  private async receive(value: unknown): Promise<void> {
    if (this.removed || !envelope(value)) return;
    const message = value.payload;
    if (message.type === "ready") {
      if (this.disposal) return;
      console.log("[runtime] READY", this.id);
      this.loaded = true;
      clearTimeout(this.readyTimer);
      this.resolveReady();
      for (const command of this.queued.splice(0)) this.command(command);
    } else if (message.type === "disposed" && value.requestId === this.disposeId) {
      this.finish();
    } else if (message.type === "rpc" && value.requestId !== undefined) {
      try {
        const result = await this.scope.call(message.method, message.args);
        this.port?.postMessage(packet({ type: "rpc-result", result }, value.requestId));
      } catch (error) {
        this.port?.postMessage(packet({ type: "rpc-result", error: String(error) }, value.requestId));
      }
    } else if (["progress", "metadata", "cover", "settings", "close-request", "reload-request", "open-path", "error", "title", "focus", "snapshot", "open-request", "library-ready", "paper-color", "thumbnail"].includes(message.type)) {
      this.event?.(message);
    }
  }
  ready(): Promise<void> { return this.readyPromise; }
  command(command: Payload): void {
    if (this.removed || this.disposal) return;
    if (!this.loaded) { this.queued.push(command); return; }
    this.port?.postMessage(packet(command, ++this.requestId));
  }
  dispose(): Promise<void> {
    if (this.disposal) return this.disposal;
    console.log("[runtime] DISPOSE", this.id);
    this.disposal = new Promise((resolve) => { this.resolveDisposed = resolve; });
    this.rejectReady(new Error("Reader startup cancelled"));
    clearTimeout(this.readyTimer);
    if (!this.loaded || !this.port) { this.finish(); return this.disposal; }
    this.disposeId = ++this.requestId;
    this.disposeTimer = setTimeout(() => {
      this.event?.({ type: "error", message: "Reader cleanup timed out; the frame was removed. The last confirmed position is kept." });
      this.finish();
    }, DISPOSE_TIMEOUT_MS);
    this.port.postMessage(packet({ type: "dispose" }, this.disposeId));
    return this.disposal;
  }
  private finish(): void {
    if (this.removed) return;
    this.removed = true;
    clearTimeout(this.readyTimer);
    clearTimeout(this.disposeTimer);
    console.log("[runtime] DISPOSED", this.id);
    this.scope.dispose();
    this.queued = [];
    if (this.port) { this.port.onmessage = null; this.port.onmessageerror = null; this.port.close(); this.port = null; }
    this.frame?.removeEventListener("load", this.connect);
    this.frame?.removeEventListener("error", this.loadError);
    // The iframe is the hard lifetime boundary for the WASM instance: the
    // sequence below (port close, removal, map delete) is the proof that the
    // instance actually died, logged so a leaked frame is visible in the
    // console rather than only in the heap profile.
    const frame = this.frame;
    if (frame) {
      console.log("[runtime] iframe still connected?", this.id, frame.isConnected);
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
