const puppeteer = require('puppeteer-core');
const SAMPLE = 'samples/sample.pdf';
const sleep = ms => new Promise(r => setTimeout(r, ms));

(async () => {
  const browser = await puppeteer.launch({
    executablePath: '/usr/bin/chromium',
    headless: 'new',
    args: ['--no-sandbox', '--disable-gpu', '--disable-dev-shm-usage', '--window-size=1280,900'],
  });
  const page = await browser.newPage();
  await page.setViewport({ width: 1280, height: 900 });
  page.on('console', m => { const t = m.text(); if (t.includes('panicked')) console.log('PANIC:', t.slice(0, 220)); });
  await page.evaluateOnNewDocument((path) => {
    localStorage.setItem('pdfreader.library.v1', JSON.stringify([{ path, title: 'sample', page: 1, numPages: 0 }]));
  }, SAMPLE);
  await page.goto('http://127.0.0.1:1420/e2e-index.html', { waitUntil: 'networkidle0', timeout: 90000 });
  await sleep(5000);
  await page.evaluate(() => { const c = document.querySelector('.book[role="button"]'); if (c) c.click(); });
  await sleep(9000);

  const ftState = () => page.evaluate(() => {
    const n = [...document.querySelectorAll('.mix-blend-difference')][0];
    const span = n ? n.querySelector('span') : null;
    return {
      text: n ? n.innerText.trim().slice(0, 25) : null,
      opacity: span ? getComputedStyle(span).opacity : null,
      maxW: n ? n.style.maxWidth : null,
      pageGap: n ? (() => {
        const host = document.querySelector('.pdf-page');
        return host ? Math.round(host.getBoundingClientRect().left) : null;
      })() : null,
    };
  });

  const click = async (title) => {
    await page.evaluate((t) => {
      const b = [...document.querySelectorAll(`[title="${t}"]`)].find(e => e.tagName === 'BUTTON') || document.querySelector(`[title="${t}"]`);
      if (b) b.click();
    }, title);
    await sleep(4000);
  };

  console.log('-- close sidebar, title should show');
  await click('Close sidebar');
  console.log(JSON.stringify(await ftState()));

  console.log('-- zoom in x6 (page fills width -> title hides)');
  for (let i = 0; i < 6; i++) { await click('Zoom in (+)'); }
  console.log(JSON.stringify(await ftState()));

  console.log('-- zoom out x8 (page shrinks -> title returns)');
  for (let i = 0; i < 8; i++) { await click('Zoom out (-)'); }
  console.log(JSON.stringify(await ftState()));

  console.log('-- scroll to 50% (title re-measures on page change)');
  await page.evaluate(() => {
    const pl = document.getElementById('page-list');
    if (pl) { pl.scrollTop = Math.round(pl.scrollHeight * 0.5); pl.dispatchEvent(new Event('scroll')); }
  });
  await sleep(4500);
  console.log(JSON.stringify(await ftState()));

  console.log('-- resize window 900x700 (re-measure)');
  await page.setViewport({ width: 900, height: 700 });
  await sleep(4500);
  console.log(JSON.stringify(await ftState()));

  await browser.close();
})().catch(e => { console.error('HARNESS ERROR:', e); process.exit(1); });
