// Minimal static server for the built app (dist/), used by the browser-level
// lifecycle baseline in Deep CI. No dependencies: correct MIME for the
// artifacts Trunk emits — most importantly `application/wasm`, which the
// strict streaming compile wants — plus the SPA fallback the router expects.
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { extname, join, normalize, sep } from "node:path";
import { fileURLToPath } from "node:url";

const DIST = fileURLToPath(new URL("../../dist", import.meta.url));
const PORT = Number(process.env.PORT || 8123);

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".wasm": "application/wasm",
  ".pdf": "application/pdf",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".bcmap": "application/octet-stream",
};

/** Failure injection for the boot-contract stage: a request carrying
 *  `mareader_boot_fail=library|reader` gets that runtime's artifact 404'd, so
 *  the suite can drive the REAL missing-artifact path (the one that produced
 *  the blank window) instead of asserting it from the outside. Hard 404, and
 *  before the SPA fallback: a missing runtime artifact must look like a
 *  missing file, not like an HTML page served under a .js URL. */
function injectedFailure(req) {
  const cookies = req.headers.cookie ?? "";
  const match = /(?:^|;\s*)mareader_boot_fail=(library|reader)/.exec(cookies);
  return match ? match[1] : null;
}

const INJECTED_SUFFIXES = [".js", "_bg.wasm"];

createServer(async (req, res) => {
  try {
    let path = decodeURIComponent(new URL(req.url, "http://localhost").pathname);
    const fail = injectedFailure(req);
    if (fail && INJECTED_SUFFIXES.some((suffix) => path === `/${fail}${suffix}`)) {
      console.log(`[injected] 404 ${path} (mareader_boot_fail=${fail})`);
      res.writeHead(404, { "content-type": "text/plain; charset=utf-8" });
      res.end("injected missing runtime artifact\n");
      return;
    }
    if (path.endsWith("/")) path += "index.html";
    const file = normalize(join(DIST, path));
    if (!file.startsWith(DIST + sep) && file !== DIST) throw new Error("traversal");
    let data;
    let served = file;
    try {
      data = await readFile(file);
    } catch {
      // SPA fallback: unknown paths boot the app (the router owns routing).
      served = join(DIST, "index.html");
      data = await readFile(served);
    }
    // pdf.js Range-requests documents (disableStream + rangeChunkSize);
    // answer with real 206s so the open path exercises the same code the
    // packaged app's asset protocol does.
    const type = MIME[extname(served).toLowerCase()] ?? "application/octet-stream";
    const range = req.headers.range;
    if (range) {
      const match = /^bytes=(\d*)-(\d*)$/.exec(range);
      if (match) {
        const total = data.length;
        const start = match[1] ? Number(match[1]) : 0;
        const end = match[2] ? Math.min(Number(match[2]), total - 1) : total - 1;
        if (Number.isNaN(start) || start > end || start >= total) {
          res.writeHead(416, { "content-range": `bytes */${total}` });
          res.end();
          return;
        }
        res.writeHead(206, {
          "content-type": type,
          "content-range": `bytes ${start}-${end}/${total}`,
          "content-length": end - start + 1,
          "accept-ranges": "bytes",
          "cache-control": "no-store",
        });
        res.end(data.subarray(start, end + 1));
        return;
      }
    }
    res.writeHead(200, {
      "content-type": type,
      "accept-ranges": "bytes",
      "cache-control": "no-store",
    });
    res.end(data);
  } catch {
    res.writeHead(500);
    res.end("server error");
  }
}).listen(PORT, "127.0.0.1", () => {
  console.log(`serving ${DIST} on http://127.0.0.1:${PORT}`);
});
