// Verify: zoom-out glide is smooth too, and NORMAL scrolling still updates the
// virtualizer (echo must not be suppressed outside a gesture).
const puppeteer = require('puppeteer-core');

const URL = 'http://127.0.0.1:1420/e2e-index.html';
const SEED = 'pdfreader.library.v1';
const LIB = JSON.stringify([{ path: 'samples/sample.pdf', title: 'sample', page: 1, numPages: 0 }]);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

(async () => {
  const browser = await puppeteer.launch({
    executablePath: '/usr/bin/chromium', headless: 'new',
    args: ['--no-sandbox', '--disable-dev-shm-usage', '--disable-gpu'],
  });
  const page = await browser.newPage();
  await page.setViewport({ width: 1280, height: 800 });
  const panics = [];
  page.on('pageerror', (e) => panics.push('PAGEERROR: ' + e.message));
  await page.goto(URL, { waitUntil: 'networkidle0' });
  await page.evaluate(([k, v]) => localStorage.setItem(k, v), [SEED, LIB]);
  await page.reload({ waitUntil: 'networkidle0' });
  await sleep(2000);
  await page.evaluate(() => { const c = document.querySelector('.book[role="button"]'); if (c) c.click(); });
  await sleep(9000);

  // Normal scroll check FIRST: user scroll must move the view (echo active).
  await page.evaluate(() => {
    const pl = document.getElementById('page-list');
    pl.scrollTop = 5000;
    pl.dispatchEvent(new Event('scroll'));
  });
  await sleep(1200);
  const afterUserScroll = await page.evaluate(() => document.getElementById('page-list').scrollTop);
  console.log('user scroll 5000 ->', Math.round(afterUserScroll), '(expect ~5000: echo alive)');

  // Attach recorder, then zoom OUT.
  await page.evaluate(() => {
    window.__frames = [];
    window.__t0 = null;
    const rec = () => {
      const pl = document.getElementById('page-list');
      const host = document.querySelector('#page-list .page-host, #page-list canvas') || null;
      let rect = null;
      if (host) { const r = host.getBoundingClientRect(); rect = [Math.round(r.x), Math.round(r.y), Math.round(r.width), Math.round(r.height)]; }
      window.__frames.push({
        t: window.__t0 === null ? 0 : Math.round((performance.now() - window.__t0) * 10) / 10,
        scrollTop: pl ? Math.round(pl.scrollTop) : null,
        scrollHeight: pl ? Math.round(pl.scrollHeight) : null,
        rect,
      });
      requestAnimationFrame(rec);
    };
    requestAnimationFrame(rec);
  });
  await page.evaluate(() => {
    const b = [...document.querySelectorAll('[title="Zoom out (-)"]')].find((e) => e.tagName === 'BUTTON');
    if (b) b.click();
  });
  await page.evaluate(() => { if (window.__t0 === null) window.__t0 = performance.now(); });
  await sleep(700);
  const frames = await page.evaluate(() => window.__frames.slice(0, 16));
  console.log('zoom-out frames:');
  frames.forEach((f) => console.log(`  t=${f.t} scrollTop=${f.scrollTop} h=${f.scrollHeight} rect=${f.rect ? f.rect.join(',') : 'null'}`));

  // And a normal scroll AFTER the gesture must still work.
  await sleep(300);
  await page.evaluate(() => {
    const pl = document.getElementById('page-list');
    pl.scrollTop = 1000;
    pl.dispatchEvent(new Event('scroll'));
  });
  await sleep(1200);
  const afterZoomScroll = await page.evaluate(() => document.getElementById('page-list').scrollTop);
  console.log('user scroll 1000 after zoom ->', Math.round(afterZoomScroll), '(expect ~1000: echo alive)');

  console.log('--- panics ---');
  panics.forEach((p) => console.log(p));
  await browser.close();
})().catch((e) => { console.error('HARNESS ERROR:', e); process.exit(1); });
