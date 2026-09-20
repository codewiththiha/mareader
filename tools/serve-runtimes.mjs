import { createServer } from "node:http";
import { readFile, stat } from "node:fs/promises";
import { resolve, extname, sep } from "node:path";
const root = resolve("dist");
const types = { ".html": "text/html", ".js": "text/javascript", ".mjs": "text/javascript", ".wasm": "application/wasm", ".css": "text/css", ".pdf": "application/pdf" };
createServer(async (req, res) => {
  try {
    const url = new URL(req.url, "http://localhost");
    const path = resolve(root, "." + decodeURIComponent(url.pathname));
    if (path !== root && !path.startsWith(root + sep)) { res.writeHead(403).end(); return; }
    const file = (await stat(path)).isDirectory() ? resolve(path, "index.html") : path;
    const data = await readFile(file);
    res.writeHead(200, { "Content-Type": types[extname(file)] ?? "application/octet-stream", "Cache-Control": "no-store" });
    res.end(data);
  } catch { res.writeHead(404).end("Not found"); }
}).listen(1420, "0.0.0.0", () => console.log("Mareader: http://localhost:1420"));
