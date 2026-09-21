// CI only: exercise the shipped WASM entries, not mocked readers or model-only splits.
import { chromium } from "playwright";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
const server = spawn(process.execPath, ["tools/serve-runtimes.mjs"], { stdio: ["ignore", "pipe", "inherit"] });
await new Promise((resolve, reject) => { server.stdout.once("data", resolve); server.once("error", reject); server.once("exit", (code) => reject(new Error(`server exited: ${code}`))); });
const browser = await chromium.launch({ headless: true });
const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
const errors = [], logs = [];
page.on("pageerror", (error) => errors.push(error.message));
page.on("console", (message) => { logs.push(message.text()); });
// Only native transport is mocked. Every renderer, worker, WASM heap and bridge is real.
await page.addInitScript(() => {
  if (window !== window.top) return;
  const seed = sessionStorage.getItem('workspace-test-library');
  if (seed) { localStorage.setItem('mareader.library.v3', seed); sessionStorage.removeItem('workspace-test-library'); }
  window.__TAURI__ = {
    core: { invoke: async (command, args) => {
      if (command === "read_file_text") return args.path.endsWith('.md') ? '# Chapter One\n\nA **Markdown** document.\n\n## Chapter Two\n\nMore words.' : 'A plain text document.\n\nSecond paragraph.';
      if (command === "read_file_bytes") return (await fetch('/samples/Good Title Book.pdf')).arrayBuffer();
      if (command === "set_traffic_lights") return null;
      if (command === "verify_paths") return [];
      if (command === "take_pending_file") return null;
      throw new Error(`Unexpected native command: ${command}`);
    } },
    event: { listen: async () => () => {} },
    window: { getCurrentWindow: () => ({ isMaximized: async () => false, close: async () => {}, onCloseRequested: async () => () => {} }) },
    dialog: { open: async () => null },
  };
});
const count = async (n) => page.waitForFunction((n) => window.__MAREADER_DEBUG__?.panes.size === n && document.querySelectorAll('iframe.reader-frame').length === n, n);
const command = (detail) => page.evaluate((detail) => window.dispatchEvent(new CustomEvent('mareader:runtime-command', { detail })), detail);
async function open(path) {
  await command({ type: 'open-path', path });
  await count(1);
  const frame = page.frameLocator('iframe.reader-frame');
  if (path.endsWith('.pdf')) await frame.locator('canvas').first().waitFor({ timeout: 60_000 });
  else await frame.getByText(path.endsWith('.md') ? 'Chapter One' : 'A plain text document.', { exact: false }).first().waitFor({ timeout: 30_000 });
  assert.equal(await frame.locator('#toolbar-row, .sidebar-aside').count(), 0, 'reader must not mount window chrome');
  assert.equal(await page.locator('iframe.library-frame').count(), 0, 'the desktop never mounts the library frame');
  return frame;
}
async function closeAll() {
  await page.mouse.move(600, 10);
  await page.locator('button[title="Close this book and return to the library"]').click();
  await count(0);
  // Closing the last pane returns to the workspace home: no frames at all,
  // and no PDF engine in the persistent realm.
  assert.equal(await page.locator('iframe').count(), 0);
  assert.equal(await page.evaluate(() => typeof window.PDFReader), 'undefined');
}
async function split(bookSuffix, edge = 'right', targetId = null, commit = true) {
  const book = page.locator(`.workspace-sidebar .workspace-book[data-book-id]`).filter({ hasText: bookSuffix }).last();
  await book.scrollIntoViewIfNeeded();
  const from = await book.boundingBox(); assert(from);
  const id = targetId ?? await page.evaluate(() => window.__MAREADER_DEBUG__.activePane);
  const target = await page.locator(`[data-pane="${id}"]`).boundingBox(); assert(target);
  const x = target.x + target.width * (edge === 'left' ? .08 : edge === 'right' ? .92 : .5);
  const y = target.y + target.height * (edge === 'top' ? .08 : edge === 'bottom' ? .92 : .5);
  const before = await page.evaluate(() => ({ n: __MAREADER_DEBUG__.panes.size, tree: JSON.stringify(__MAREADER_DEBUG__.tree) }));
  await page.mouse.move(from.x+from.width/2, from.y+from.height/2);
  await page.mouse.down(); await page.mouse.move(x, y, { steps: 12 });
  const previews = await page.locator('.workspace-preview').count();
  assert.equal(await page.evaluate(() => JSON.stringify(__MAREADER_DEBUG__.tree)), before.tree, 'hover never mutates layout');
  assert.equal(await page.locator('iframe.reader-frame').count(), before.n, 'hover never creates runtimes');
  if (commit || before.n < 4) assert.equal(previews, before.n + (edge === 'center' ? 0 : 1), 'hover must show the complete preview');
  if (commit) {
    assert.equal(previews, before.n + (edge === 'center' ? 0 : 1), 'preview renders the full projected geometry');
    await page.mouse.up(); await count(before.n + (edge === 'center' ? 0 : 1));
    if (edge === 'center') await page.waitForFunction((id) => !__MAREADER_DEBUG__.panes.has(id), id);
  } else { await page.keyboard.press('Escape'); await page.mouse.up(); }
  return previews;
}
try {
  await page.goto('http://localhost:1420');
  // The workspace is the home: it boots into its own shell (sidebar tree and
  // all), with no library frame to wait for. The sidebar rail is mounted but
  // docked-closed at boot, so the wait is on attachment, not visibility.
  await page.locator('.workspace-sidebar').waitFor({ state: 'attached', timeout: 60_000 });
  assert.equal(await page.evaluate(() => __MAREADER_DEBUG__.workspace), true);
  assert.equal(await page.evaluate(() => typeof window.PDFReader), 'undefined');
  assert.equal(await page.locator('iframe').count(), 0, 'boot mounts the workspace, not a frame');
  // Repeated real PDF instances, followed by both reflow runtimes.
  for (let i=0; i<3; i++) { await open('/samples/Good Title Book.pdf'); await closeAll(); }
  await open('/books/example.txt'); await closeAll();
  await open('/books/example.md'); await closeAll();
  // Persist a nested shelf using the real blob shape, then boot its projection.
  await page.evaluate(() => {
    const key = 'mareader.library.v3', blob = JSON.parse(localStorage.getItem(key));
    blob.shelves = [
      {id:'s-test', name:'Reading', kind:{kind:'virtual'}, books:[], parent:null, manualParent:false},
      {id:'s-child', name:'Documents', kind:{kind:'virtual'}, books:blob.books.map(b=>b.id), parent:'s-test', manualParent:false},
    ];
    // Install after the old library page has flushed during navigation.
    sessionStorage.setItem('workspace-test-library', JSON.stringify(blob));
  });
  await page.reload();
  await page.locator('.workspace-sidebar').waitFor({ state: 'attached', timeout: 60_000 });
  await open('/samples/Good Title Book.pdf');
  await page.mouse.move(600, 10);
  await page.locator('button[title="Toggle sidebar"]').click();
  await page.locator('.workspace-sidebar').waitFor({state:'visible'});
  // The seeded nested shelf renders as real tree rows, open by default, with
  // the child shelf's books visible inside.
  assert.equal(await page.locator('.workspace-sidebar .workspace-shelf').filter({hasText:'Reading'}).count(), 1);
  assert.ok((await page.locator('.workspace-shelf').filter({hasText:'Documents'}).count()) >= 1);
  assert(await page.locator('.workspace-sidebar .format-badge').count() >= 3);
  // Cancel, split to four, reject a fifth, replace at the limit.
  await split('example', 'right', null, false);
  await count(1);
  await split('TXT', 'right');
  await split('MD', 'bottom');
  await split('PDF', 'left');
  await count(4);
  const noPreview = await split('TXT', 'top', null, false);
  assert.equal(noPreview, 0); await count(4);
  await split('MD', 'center'); await count(4);
  const ids = await page.evaluate(() => [...__MAREADER_DEBUG__.panes.keys()]);
  // Real iframe pointerdown is the focus authority.
  for (const id of ids) {
    const frame = page.frameLocator(`[data-pane="${id}"] iframe`);
    await frame.locator('#reader-app').click({position:{x:20,y:80},force:true});
    await page.waitForFunction((id) => __MAREADER_DEBUG__.activePane === id, id);
    assert.equal(await page.locator(`[data-pane="${id}"].active`).count(), 1);
  }
  const focusPane = async (id) => {
    await page.frameLocator(`[data-pane="${id}"] iframe`).locator('#reader-app').click({position:{x:20,y:80},force:true});
    await page.waitForFunction((id) => __MAREADER_DEBUG__.activePane === id, id);
  };
  const setBase = async (name) => {
    await page.mouse.move(1000,10);
    await page.getByTitle('Appearance', {exact:true}).click();
    await page.getByTitle(name, {exact:true}).click();
    await page.keyboard.press('Escape');
  };
  const bases = async () => {
    const result = [];
    for (const id of ids) result.push(await page.frameLocator(`[data-pane="${id}"] iframe`).locator('html').getAttribute('data-base'));
    return result;
  };
  await focusPane(ids[0]);
  await page.getByRole('button', {name:'Thumbnails',exact:true}).click();
  // Thumbnail cells render a placeholder until the real bitmap arrives, so the
  // <img> element itself only exists once a page has been rasterized.
  await page.locator('.workspace-thumb-cell img').first().waitFor({ timeout: 60_000 });
  await focusPane(ids[2]);
  await page.getByText('Thumbnails are available for PDF documents.', {exact:true}).waitFor();
  await page.getByRole('button', {name:'Outline',exact:true}).click();
  // The hidden library panel and the Active tabs also carry the md's
  // "Chapter One" title, so the wait targets the outline row itself.
  const outlineRow = page.locator('.sidebar-panel [data-outline-index="0"]');
  await outlineRow.waitFor();
  assert.equal((await outlineRow.textContent()).trim(), 'Chapter One');
  await page.getByRole('button', {name:'Library',exact:true}).click();
  await focusPane(ids[0]); await setBase('Dark');
  await page.frameLocator(`[data-pane="${ids[0]}"] iframe`).locator('html[data-base="dark"]').waitFor();
  await focusPane(ids[1]); await setBase('Light');
  await page.frameLocator(`[data-pane="${ids[1]}"] iframe`).locator('html[data-base="light"]').waitFor();
  const originalBases = await bases();
  const originalProfiles = await page.evaluate(() => __MAREADER_DEBUG__.profiles);
  const blend = async () => {
    await page.getByRole('button', {name:'Reader settings',exact:true}).click();
    await page.getByRole('dialog').getByRole('button', {name:'Theme',exact:true}).click();
    await page.getByRole('dialog').getByRole('switch', {name:/Paint the reader background/}).click();
    await page.keyboard.press('Escape');
  };
  await focusPane(ids[0]); await blend();
  for (const id of ids) await page.frameLocator(`[data-pane="${id}"] iframe`).locator('html[data-base="dark"]').waitFor();
  assert.equal(await page.evaluate(() => __MAREADER_DEBUG__.blend.source), ids[0]);
  await page.waitForFunction(() => {
    const paper = __MAREADER_DEBUG__.blend?.paper;
    if (!paper) return false;
    const backdrop = document.getElementById('workspace-layout').style.backgroundColor;
    return [...document.querySelectorAll('.reader-frame')].every(frame => {
      const doc = frame.contentDocument;
      const bg = doc?.querySelector('.reader-bg');
      return bg && frame.contentWindow.getComputedStyle(bg).backgroundColor === backdrop
        && (frame.title === 'PDF reader'
          || frame.contentWindow.getComputedStyle(doc.documentElement).getPropertyValue('--tx-paper').trim() === paper);
    });
  });
  await focusPane(ids[1]);
  assert.equal(await page.evaluate(() => __MAREADER_DEBUG__.blend.source), ids[0]);
  assert.deepEqual(await page.evaluate(() => __MAREADER_DEBUG__.profiles.map(p => p.base)), originalProfiles.map(p => p.base));
  assert.deepEqual(await bases(), ids.map(()=> 'dark'), 'Blend authority is independent of focus');
  await blend();
  for (let i=0;i<ids.length;i++) await page.frameLocator(`[data-pane="${ids[i]}"] iframe`).locator(`html[data-base="${originalBases[i]}"]`).waitFor();
  assert.deepEqual(await bases(), originalBases, 'leaving Blend restores every base profile');
  assert.deepEqual(await page.evaluate(() => __MAREADER_DEBUG__.profiles), originalProfiles,
    'leaving Blend restores complete appearance profiles, not only light/dark');
  // Close/collapse keeps the other frame instances intact.
  await page.locator(`[data-pane="${ids[0]}"] .pane-close`).click(); await count(3);
  const kept = await page.evaluate(() => [...__MAREADER_DEBUG__.panes.keys()]);
  assert.deepEqual(kept.sort(), ids.slice(1).sort());
  await closeAll();
  assert.equal(await page.locator('iframe').count(), 0, 'closing the last pane returns to the frameless workspace home');
  assert.deepEqual(errors, [], `uncaught browser errors: ${errors.join('\n')}`);
  console.log('Workspace browser acceptance: isolated routes, mixed-format four-pane split, preview, limit, replace, focus, close/collapse.');
} catch (error) {
  mkdirSync('test-artifacts', {recursive:true});
  await page.screenshot({path:'test-artifacts/runtime-failure.png',fullPage:true});
  writeFileSync('test-artifacts/browser.log', logs.join('\n')+'\n'+errors.join('\n'));
  writeFileSync('test-artifacts/page.html', await page.content());
  console.error('Browser errors:', errors); console.error('Recent console:', logs.slice(-40));
  console.error('Workspace state:', await page.evaluate(() => ({ tree: __MAREADER_DEBUG__.tree, active: __MAREADER_DEBUG__.activePane, dragging: __MAREADER_DEBUG__.dragging, drop: __MAREADER_DEBUG__.dropTarget })));
  throw error;
} finally { await browser.close(); server.kill(); }
