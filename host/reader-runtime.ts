import { COMMAND_EVENT, OUTPUT_EVENT, CONNECTED_EVENT, envelope, packet, payload, localCommand, type Payload } from "./protocol";

let port: MessagePort | null = null;
let ready: Payload | null = null;
let disposed = false;
let disposeId: number | undefined;
let nextId = 0;
const pending = new Map<number, { resolve: (value: unknown) => void; reject: (error: Error) => void; timer: ReturnType<typeof setTimeout> }>();
const subscriptions = new Map<number, (event: unknown) => void>();
function send(value: Payload, requestId?: number): void { port?.postMessage(packet(value, requestId)); }
function rpc(method: string, args?: unknown): Promise<unknown> {
  if (!port || disposed) return Promise.reject(new Error("Reader bridge is closed"));
  const requestId = ++nextId;
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => { pending.delete(requestId); reject(new Error("Desktop operation timed out")); }, 60_000);
    pending.set(requestId, { resolve, reject, timer });
    send({ type: "rpc", method, args }, requestId);
  });
}
function close(): void {
  if (disposed) return;
  disposed = true;
  window.removeEventListener(OUTPUT_EVENT, output);
  window.removeEventListener("message", connect);
  document.removeEventListener("pointerdown", drag);
  document.removeEventListener("dblclick", doubleClick);
  for (const item of pending.values()) { clearTimeout(item.timer); item.reject(new Error("Reader disposed")); }
  pending.clear(); subscriptions.clear();
  if (port) { port.onmessage = null; port.close(); port = null; }
  ready = null;
}
function output(event: Event): void {
  const value: unknown = (event as CustomEvent).detail;
  if (!payload(value) || disposed) return;
  if (value.type === "ready") ready = value;
  send(value, value.type === "disposed" ? disposeId : undefined);
  if (value.type === "disposed") close();
}
function installTauriProxy(): void {
  const proxy = {
    core: { invoke: (command: string, args?: unknown) => rpc("invoke", { command, args }) },
    dialog: { open: (options: unknown) => rpc("dialog", options) },
    event: { listen: async (event: string, callback: (event: unknown) => void) => {
      const id = Number(await rpc("listen", event));
      if (disposed) return () => undefined;
      subscriptions.set(id, callback);
      return () => { subscriptions.delete(id); if (!disposed) void rpc("unlisten", id).catch(() => undefined); };
    } },
    window: { getCurrentWindow: () => Object.fromEntries(
      ["minimize", "toggleMaximize", "isMaximized", "startDragging", "close"].map((name) => [name, () => rpc("window", { name })]),
    ) },
  };
  Object.defineProperty(window, "__TAURI__", { configurable: true, value: proxy });
}
function connect(event: MessageEvent<unknown>): void {
  if (port || disposed || event.source !== parent || event.origin !== location.origin
    || !envelope(event.data) || event.data.payload.type !== "connect" || event.ports.length !== 1) return;
  port = event.ports[0];
  if (event.data.payload.tauri === true) installTauriProxy();
  port.onmessage = (event: MessageEvent<unknown>) => {
    if (!envelope(event.data) || disposed) return;
    const { payload: message, requestId } = event.data;
    if (message.type === "rpc-result" && requestId !== undefined) {
      const call = pending.get(requestId);
      if (!call) return;
      pending.delete(requestId); clearTimeout(call.timer);
      if (typeof message.error === "string") call.reject(new Error(message.error));
      else call.resolve(message.result);
    } else if (message.type === "tauri-event") {
      subscriptions.get(Number(message.subscription))?.(message.event);
    } else if (message.type === "focus") window.focus();
    else if (message.type === "blur") (document.activeElement as HTMLElement | null)?.blur();
    else if (["open", "set-settings", "resize", "dispose"].includes(message.type)) {
      if (message.type === "dispose") disposeId = requestId;
      localCommand(message);
    }
  };
  port.start();
  if (ready) send(ready);
  // Rust must mount AFTER this point: its initial effects probe Tauri and
  // register listeners. The runtime entry script waits for this event.
  // Set a flag so a late WASM init can mount immediately if it missed the event.
  (window as any).__MAREADER_CONNECTED__ = true;
  window.dispatchEvent(new Event(CONNECTED_EVENT));
}
function drag(event: PointerEvent): void {
  if (event.button !== 0 || !(event.target instanceof Element)
    || event.target.getAttribute("data-tauri-drag-region") !== "true") return;
  void rpc("window", { name: "startDragging" }).catch(() => undefined);
}
function doubleClick(event: MouseEvent): void {
  if (!(event.target instanceof Element) || event.target.getAttribute("data-tauri-drag-region") !== "true") return;
  void rpc("window", { name: "toggleMaximize" }).catch(() => undefined);
}
window.addEventListener(OUTPUT_EVENT, output);
window.addEventListener("message", connect);
window.addEventListener("pagehide", () => {
  // Best effort only; normal close awaits the acknowledged flush instead.
  if (!disposed) window.dispatchEvent(new CustomEvent(COMMAND_EVENT, { detail: { type: "dispose" } }));
  close();
}, { once: true });
document.addEventListener("pointerdown", drag);
document.addEventListener("dblclick", doubleClick);
