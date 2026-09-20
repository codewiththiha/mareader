// Runs only on GitHub's frontend lane, against the actual four WASM artifacts.
import { chromium } from "playwright";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
const server = spawn(process.execPath, ["tools/serve-runtimes.mjs"], { stdio: ["ignore", "pipe", "inherit"] });
await new Promise((resolve, reject) => { server.stdout.once("data", resolve); server.once("error", reject); server.once("exit", (code) => reject(new Error(`server exited: ${code}`))); });
const browser = await chromium.launch({ headless: true });
const page = await browser.newPage();
const errors = [];
page.on("pageerror", (error) => errors.push(error.message));
try {
  await page.goto("http://localhost:1420");
  await page.waitForFunction(() => document.querySelector("#library-root")?.childElementCount > 0);
  assert.equal(await page.evaluate(() => typeof window.PDFReader), "undefined");
  assert.equal(await page.locator("iframe").count(), 0);
  const libraryIdentity = await page.evaluate(() => {
    const root = document.querySelector("#library-root").firstElementChild;
    root.dataset.persistenceProbe = "persistent";
    return root.tagName;
  });
  const open = async (path) => {
    await page.evaluate((path) => window.dispatchEvent(new CustomEvent("mareader:runtime-command", { detail: { type: "open-path", path } })), path);
    await page.waitForFunction(() => document.querySelectorAll("iframe.reader-frame").length === 1);
    const frame = page.frameLocator("iframe.reader-frame");
    await frame.locator("button[title='Close this book and return to the library']").waitFor({ timeout: 60_000 });
    return frame;
  };
  const close = async (frame) => {
    await frame.locator("button[title='Close this book and return to the library']").click();
    await page.waitForFunction(() => !document.querySelector("iframe.reader-frame"));
    await page.locator("#library-root").waitFor({ state: "visible" });
    assert.equal(await page.locator("[data-persistence-probe='persistent']").count(), 1);
    assert.equal(await page.evaluate(() => typeof window.PDFReader), "undefined");
  };
  // PDF: real pdf.js, a real worker, actual raster surfaces; the library's
  // DOM is unchanged after each realm is destroyed.
  for (let i = 0; i < 10; i++) {
    const frame = await open("/samples/Good Title Book.pdf");
    await frame.locator("canvas").first().waitFor({ timeout: 60_000 });
    await close(frame);
  }
  // Mock only the native filesystem transport, not the WASM or renderers.
  await page.evaluate(() => {
    window.__TAURI__ = {
      core: { invoke: async (command, args) => {
        if (command === "read_file_text") return args.path.endsWith('.md') ? '# Chapter One\n\nA **Markdown** document.\n\n## Chapter Two\n\nMore words.' : 'A plain text document.\n\nSecond paragraph.';
        if (command === "set_traffic_lights") return null;
        if (command === "verify_paths") return [];
        if (command === "take_pending_file") return null;
        throw new Error(`Unexpected native command: ${command}`);
      } },
      event: { listen: async () => () => {} },
      window: { getCurrentWindow: () => ({ isMaximized: async () => false, close: async () => {} }) },
      dialog: { open: async () => null },
    };
  });
  for (const [extension, expected] of [["txt", "A plain text document."], ["md", "Chapter One"]]) {
    const frame = await open(`/books/example.${extension}`);
    await frame.getByText(expected, { exact: false }).first().waitFor({ timeout: 30_000 });
    const iframe = page.frames().find((frame) => frame.url().includes(`/reader-${extension}/`));
    assert(iframe);
    assert.equal(await iframe.evaluate(() => typeof window.PDFReader), "undefined");
    await close(frame);
  }
  assert.equal(await page.locator("#library-root > :first-child").evaluate((e) => e.tagName), libraryIdentity);
  assert.deepEqual(errors, [], `uncaught browser errors: ${errors.join('\n')}`);
  console.log("Browser lifecycle: PDF, TXT and Markdown opened and disposed; library identity preserved.");
} catch (error) {
  mkdirSync("test-artifacts", { recursive: true });
  await page.screenshot({ path: "test-artifacts/runtime-failure.png", fullPage: true });
  console.error("Browser errors:", errors);
  throw error;
} finally {
  await browser.close();
  server.kill();
}
