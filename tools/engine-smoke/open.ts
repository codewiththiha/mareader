import { fakeWindow, R, openDoc } from "./harness.js";

export async function run(): Promise<void> {
  const opened = await openDoc("/fake/book.pdf");
  if (!opened.ok) throw new Error("open failed: " + JSON.stringify(opened));
  console.log("open ok:", opened.numPages, "pages");
  // The open payload carries the fingerprint the search index caches under.
  if (opened.fingerprint !== "smoke-permanent") {
    throw new Error("open must report the permanent fingerprint, got " + opened.fingerprint);
  }
  // The chapter tree arrives on its own call once the reader is up.
  if (opened.outline.length !== 0) {
    throw new Error("open must not resolve the outline, got " + opened.outline.length);
  }
  const outline = await R.resolveOutline();
  if (!outline.ok || outline.outline.length !== 0) {
    throw new Error("resolveOutline failed: " + JSON.stringify(outline));
  }
  console.log("resolveOutline ok");

  // The OS handoff: the shell invokes the queued-path command directly.
  const invoke = (cmd: string): Promise<unknown> => fakeWindow.__TAURI__!.core.invoke(cmd);
  const none = await invoke("take_pending_file");
  if (none !== null) throw new Error("take_pending_file should resolve null, got " + none);
  let queuedPath: string | null = null;
  const realInvoke = fakeWindow.__TAURI__!.core.invoke;
  fakeWindow.__TAURI__!.core.invoke = async (cmd: string): Promise<unknown> =>
    cmd === "take_pending_file" ? queuedPath : realInvoke(cmd);
  queuedPath = "C:/Users/reader/Documents/book.pdf";
  const taken = await invoke("take_pending_file");
  if (taken !== queuedPath) throw new Error("take_pending_file did not return the path: " + taken);
  queuedPath = null;
  fakeWindow.__TAURI__!.core.invoke = realInvoke;
  console.log("take_pending_file ok");

}
