import { FrameRuntime } from "./frame";
import { HostRuntimeManager } from "./lifecycle";
import { OUTPUT_EVENT, payload, record, readerConfig, localCommand } from "./protocol";
import { tauri } from "./tauri";

const library = document.getElementById("library-root")!;
const reader = document.getElementById("reader-root")!;
const status = document.getElementById("host-status")!;
let returnFocus: HTMLElement | null = null;
let libraryReady = false;
let pendingPath: string | null = null;
const manager = new HostRuntimeManager(
  (config, event) => new FrameRuntime(reader, config, event, async () => {
    await manager.close();
    await tauri()?.window.getCurrentWindow().close();
  }),
  (event) => {
    if (event.type === "close-request") void manager.close();
    else if (event.type === "reload-request") void manager.close().then(() => location.reload());
    else if (event.type === "open-path" && typeof event.path === "string") {
      // Flush the old book BEFORE resolving the next resume point, including
      // a fast reopen of the same book through the OS or dialog.
      void manager.close().then(() => localCommand(event));
    } else if (event.type === "error") {
      localCommand(event);
      void manager.close();
    } else localCommand(event);
  },
  (state) => {
    const active = !["disposed", "failed"].includes(state);
    library.hidden = active;
    library.inert = active;
    reader.hidden = !active;
    status.hidden = state !== "loading" && state !== "creating";
    status.textContent = "Opening document…";
    if (!active) {
      history.replaceState(null, "", "/");
      returnFocus?.focus();
      returnFocus = null;
    } else if (state === "ready") {
      history.replaceState(null, "", "/#reader");
      manager.command({ type: "focus" });
    }
  },
);
function output(event: Event): void {
  const value: unknown = (event as CustomEvent).detail;
  if (!payload(value)) return;
  if (value.type === "open-request" && readerConfig(value.config)) {
    if (!returnFocus && document.activeElement instanceof HTMLElement) returnFocus = document.activeElement;
    void manager.open(value.config);
  } else if (value.type === "open-path" && typeof value.path === "string") {
    void manager.close().then(() => localCommand(value));
  } else if (value.type === "library-ready") {
    libraryReady = true;
    if (pendingPath) { localCommand({ type: "open-path", path: pendingPath }); pendingPath = null; }
    void takePendingFile();
  }
}
async function takePendingFile(): Promise<void> {
  try {
    const path = await tauri()?.core.invoke("take_pending_file");
    if (typeof path !== "string" || !path) return;
    if (!libraryReady) { pendingPath = path; return; }
    await manager.close();
    localCommand({ type: "open-path", path });
  } catch (error) { localCommand({ type: "error", message: String(error) }); }
}
window.addEventListener(OUTPUT_EVENT, output);
window.addEventListener("popstate", () => { void manager.close(); });
window.addEventListener("focus", () => manager.command({ type: "focus" }));
window.addEventListener("blur", () => manager.command({ type: "blur" }));
const resize = new ResizeObserver(() => manager.command({ type: "resize", width: reader.clientWidth, height: reader.clientHeight }));
resize.observe(reader);
let unlisten: (() => void) | undefined;
let unlistenClose: (() => void) | undefined;
let allowNativeClose = false;
let leaving = false;
void tauri()?.event.listen("document-open-file", () => { void takePendingFile(); }).then((handle) => {
  if (leaving) handle(); else unlisten = handle;
});

const nativeWindow = tauri()?.window.getCurrentWindow();
if (nativeWindow?.onCloseRequested) {
  void nativeWindow.onCloseRequested((event: unknown) => {
    if (allowNativeClose || !record(event) || typeof event.preventDefault !== "function") return;
    event.preventDefault();
    void manager.close().then(async () => {
      allowNativeClose = true;
      await nativeWindow.close();
    });
  }).then((handle) => {
    if (typeof handle !== "function") return;
    const stop = handle as () => void;
    if (leaving) stop(); else unlistenClose = stop;
  });
}

window.addEventListener("pagehide", () => {
  leaving = true; unlisten?.(); unlistenClose?.(); resize.disconnect();
  void manager.close();
}, { once: true });
