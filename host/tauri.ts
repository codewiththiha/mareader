import { record, type Payload } from "./protocol";

export interface TauriApi {
  core: { invoke(command: string, args?: unknown): Promise<unknown> };
  event: { listen(event: string, callback: (event: unknown) => void): Promise<() => void> };
  dialog: { open(options: unknown): Promise<unknown> };
  window: { getCurrentWindow(): Record<string, (...args: unknown[]) => Promise<unknown>> };
}
// A local cast rather than another global declaration: the PDF bundle owns
// its narrower __TAURI__ type and is compiled in the same TypeScript project.
export function tauri(): TauriApi | undefined {
  return (window as unknown as { __TAURI__?: TauriApi }).__TAURI__;
}

/** One RPC scope per reader. A late listen resolution unlistens immediately;
 * no Tauri callback is allowed to retain a dead child browsing context. */
export class TauriScope {
  private disposed = false;
  private nextSubscription = 0;
  private readonly subscriptions = new Map<number, () => void>();
  constructor(private readonly path: string, private readonly send: (event: Payload) => void,
    private readonly closeWindow: () => Promise<void>) {}
  async call(method: unknown, args: unknown): Promise<unknown> {
    if (this.disposed) throw new Error("Reader is disposed");
    const api = tauri();
    if (!api) throw new Error("This operation requires the desktop app");
    if (method === "invoke" && record(args) && typeof args.command === "string") {
      if (!["read_file_text", "read_file_bytes", "explain_word", "set_traffic_lights"].includes(args.command)) {
        throw new Error("Reader command is not allowed");
      }
      if (args.command.startsWith("read_file_") && (!record(args.args) || args.args.path !== this.path)) {
        throw new Error("Reader may only read its configured document");
      }
      return api.core.invoke(args.command, args.args);
    }
    if (method === "dialog") return api.dialog.open(args);
    if (method === "window" && record(args) && typeof args.name === "string") {
      const name = args.name;
      if (!["minimize", "toggleMaximize", "isMaximized", "startDragging", "close"].includes(name)) {
        throw new Error("Window command is not allowed");
      }
      if (name === "close") { await this.closeWindow(); return null; }
      const handle = api.window.getCurrentWindow();
      return handle[name].call(handle);
    }
    if (method === "listen" && typeof args === "string") {
      if (!["ai-stream-chunk", "tauri://resize"].includes(args)) throw new Error("Reader event is not allowed");
      const subscription = ++this.nextSubscription;
      const unlisten = await api.event.listen(args, (event) => {
        if (!this.disposed) this.send({ type: "tauri-event", subscription, event });
      });
      if (this.disposed) unlisten();
      else this.subscriptions.set(subscription, unlisten);
      return subscription;
    }
    if (method === "unlisten" && typeof args === "number") {
      this.subscriptions.get(args)?.();
      this.subscriptions.delete(args);
      return null;
    }
    throw new Error("Unknown reader RPC");
  }
  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    for (const unlisten of this.subscriptions.values()) unlisten();
    this.subscriptions.clear();
  }
}
