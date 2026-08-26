// Probe: record per-rAF DOM state during a zoom click to see whether the
// 200ms tween produces visible mid frames or an instant jump/flicker.
const puppeteer = require('puppeteer-core');

const URL = 'http://127.0.0.1:1420/e2e-index.html';
const SEED = 'pdfreader.library.v1';
const LIB = JSON.stringify([{ path: 'samples/sample.pdf', title: 'sample', page: 1, numPages: 0 }]);

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

(async () => {
  const browser = await puppeteer.launch({
    executablePath: '/usr/bin/chromium',
    headless: 'new',
    args: ['--no-sandbox', '--disable-dev-shm-usage', '--disable-gpu'],
  });
  const page = await browser.newPage();
  await page.setViewport({ width: 1280, height: 800 });
  const panics = [];
  page.on('pageerror', (e) => panics.push('PAGEERROR: ' + e.message));
  page.on('console', (m) => {
    const t = m.text();
    if (t.includes('[DBG')) console.log('DBG:', t);
    else if (m.type() === 'error') panics.push('CONSOLE: ' + t);
  });
  await page.goto(URL, { waitUntil: 'networkidle0' });
  await page.evaluate(([k, v]) => localStorage.setItem(k, v), [SEED, LIB]);
  await page.reload({ waitUntil: 'networkidle0' });
  await sleep(2000);
  // open doc
  await page.evaluate(() => { const c = document.querySelector('.book[role="button"]'); if (c) c.click(); });
  await sleep(9000);
  // make sure continuous + sidebar closed so we see pure zoom motion
  const closeSidebar = () => page.evaluate(() => {
    const b = [...document.querySelectorAll('[title="Close sidebar"]')].find((e) => e.tagName === 'BUTTON');
    if (b) b.click();
  });
  await closeSidebar();
  await sleep(2500);

  // Attach the rAF recorder BEFORE clicking zoom in.
  await page.evaluate(() => {
    window.__frames = [];
    window.__t0 = null;
    const rec = () => {
      const pl = document.getElementById('page-list');
      const host = document.querySelector('#page-list .page-host, #page-list canvas') || null;
      let rect = null;
      if (host) { const r = host.getBoundingClientRect(); rect = [Math.round(r.x), Math.round(r.y), Math.round(r.width), Math.round(r.height)]; }
      let transform = host ? getComputedStyle(host).transform : null;
      const readout = document.querySelector('.thumb-num') ? null : null;
      const zoomTxt = [...document.querySelectorAll('button span')].map((s) => s.textContent.trim()).find((t) => /^\d+%$/.test(t)) || null;
      window.__frames.push({
        t: window.__t0 === null ? 0 : Math.round((performance.now() - window.__t0) * 10) / 10,
        scrollTop: pl ? Math.round(pl.scrollTop) : null,
        scrollHeight: pl ? Math.round(pl.scrollHeight) : null,
        hostCount: document.querySelectorAll('#page-list canvas').length,
        rect,
        transform: transform === 'none' ? 'none' : (transform || '').slice(0, 40),
        zoomTxt,
      });
      requestAnimationFrame(rec);
    };
    requestAnimationFrame(rec);
  });

  const clickZoomIn = () => page.evaluate(() => {
    const b = [...document.querySelectorAll('[title="Zoom in (+)"]')].find((e) => e.tagName === 'BUTTON');
    if (b) b.click();
  });

  // Click and sample around it: start timestamp via evaluate right before click.
  await page.evaluate(() => { window.__frames = []; window.__t0 = null; });
  await clickZoomIn();
  await page.evaluate(() => { if (window.__t0 === null) window.__t0 = performance.now(); });
  await sleep(700);
  const frames = await page.evaluate(() => window.__frames.slice(0, 45));
  console.log(JSON.stringify(frames, null, 0));
  console.log('--- panics ---');
  panics.forEach((p) => console.log(p));
  await browser.close();
})().catch((e) => { console.error('HARNESS ERROR:', e); process.exit(1); });
