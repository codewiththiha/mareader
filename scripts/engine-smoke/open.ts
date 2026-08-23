import {
  EngineResult,
  OpenPayload,
  PDFReader,
  fakeWindow,
} from "./harness.js";

export async function run(): Promise<void> {
  // 1. open
  const opened = await PDFReader.open("/fake/book.pdf");
  if (!opened.ok) throw new Error("open failed: " + JSON.stringify(opened));
  console.log("open ok:", opened.numPages, "pages");

  // 10. OS file handoff wrapper
  const none = await PDFReader.takePendingFile();
  if (none !== null) throw new Error("takePendingFile should resolve null, got " + none);
  let queuedPath: string | null = null;
  const realInvoke = fakeWindow.__TAURI__!.core.invoke;
  fakeWindow.__TAURI__!.core.invoke = async (cmd: string): Promise<unknown> =>
    cmd === "take_pending_file" ? queuedPath : realInvoke(cmd);
  queuedPath = "C:/Users/reader/Documents/book.pdf";
  const taken = await PDFReader.takePendingFile();
  if (taken !== queuedPath) throw new Error("takePendingFile did not return the path: " + taken);
  queuedPath = null;
  fakeWindow.__TAURI__!.core.invoke = realInvoke;
  console.log("takePendingFile ok");

}
