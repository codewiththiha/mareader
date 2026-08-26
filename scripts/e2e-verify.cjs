const puppeteer = require('puppeteer-core');

const SAMPLE = process.env.SAMPLE || 'samples/sample.pdf';
const sleep = ms => new Promise(r => setTimeout(r, ms));

(async () => {
  const browser = await puppeteer.launch({
    executablePath: '/usr/bin/chromium',
    headless: 'new',
    args: ['--no-sandbox', '--disable-gpu', '--disable-dev-shm-usage', '--window-size=1280,900'],
  });
  const page = await browser.newPage();
  await page.setViewport({ width: 1280, height: 900 });
  const panics = [];
  page.on('console', m => {
    const t = m.text();
    if (t.includes('panicked')) panics.push(t.slice(0, 200));
    else if (m.type() === 'error' && !t.includes('favicon')) panics.push('ERR: ' + t.slice(0, 150));
  });
  page.on('pageerror', e => panics.push('PAGEERROR: ' + (e.message || '').slice(0, 120)));

  await page.evaluateOnNewDocument((path) => {
    try {
      localStorage.setItem('pdfreader.library.v1', JSON.stringify([{ path, title: 'sample', page: 1, numPages: 0 }]));
    } catch (e) {}
  }, SAMPLE);

  await page.goto('http://127.0.0.1:1420/e2e-index.html', { waitUntil: 'networkidle0', timeout: 90000 });
  await sleep(5000);
  await page.evaluate(() => { const c = document.querySelector('.book[role="button"]'); if (c) c.click(); });
  await sleep(9000);

  const clickTitle = async (title, tag = '*') => {
    await page.evaluate((t, tg) => {
      const b = [...document.querySelectorAll(`[title="${t}"]`)].find(e => e.tagName === tg || (tg === '*' && e.tagName === 'BUTTON')) || document.querySelector(`[title="${t}"]`);
      if (b) b.click();
    }, title, tag);
    await sleep(4000);
  };

  const state = () => page.evaluate(() => {
    const currentBadge = document.querySelector('.thumb-num.is-current span');
    const pl = document.getElementById('page-list');
    const thumbs = document.getElementById('thumb-scroll');
    const ft = [...document.querySelectorAll('.mix-blend-difference')][0] || null;
    const ftSpan = ft ? ft.querySelector('span') : null;
    const aside = document.querySelector('aside.sidebar-aside');
    return {
      currentThumb: currentBadge ? currentBadge.textContent.trim() : null,
      contCanvases: [...document.querySelectorAll('#page-list canvas')].length,
      thumbScrollTop: thumbs ? Math.round(thumbs.scrollTop) : -1,
      pageListScrollTop: pl ? Math.round(pl.scrollTop) : -1,
      floatingTitleText: ft ? ft.innerText.trim().slice(0, 30) : null,
      floatingTitleOpacity: ftSpan ? getComputedStyle(ftSpan).opacity : null,
      sidebarWidth: aside ? Math.round(aside.getBoundingClientRect().width) : null,
    };
  });

  console.log('== 1) open thumbs sidebar ==');
  await clickTitle('Thumbnails');
  console.log(JSON.stringify(await state()));

  console.log('== 2) scroll to ~65% (thumb should follow) ==');
  await page.evaluate(() => {
    const pl = document.getElementById('page-list');
    if (pl) { pl.scrollTop = Math.round(pl.scrollHeight * 0.65); pl.dispatchEvent(new Event('scroll')); }
  });
  await sleep(4500);
  console.log(JSON.stringify(await state()));

  console.log('== 3) click thumb page 12 ==');
  await page.evaluate(() => {
    const badges = [...document.querySelectorAll('#thumb-scroll .thumb-num')];
    const b12 = badges.find(b => b.textContent.trim() === '12');
    if (b12) b12.closest('button').click();
  });
  await sleep(4500);
  console.log(JSON.stringify(await state()));

  console.log('== 4) single -> continuous round trip ==');
  await clickTitle('Single page view');
  const s1 = await state();
  await clickTitle('Continuous scroll view');
  const s2 = await state();
  console.log('single:', JSON.stringify(s1));
  console.log('back-to-continuous:', JSON.stringify(s2));

  console.log('== 5) close sidebar -> floating title should appear ==');
  await clickTitle('Close sidebar', 'BUTTON');
  await sleep(1500); // let the sidebar slide finish; title fades in after it
  const s5 = await state();
  console.log(JSON.stringify(s5));

  console.log('== 6) scroll in continuous (no sidebar): title follows page ==');
  await page.evaluate(() => {
    const pl = document.getElementById('page-list');
    if (pl) { pl.scrollTop = Math.round(pl.scrollHeight * 0.45); pl.dispatchEvent(new Event('scroll')); }
  });
  await sleep(4500);
  console.log(JSON.stringify(await state()));

  console.log('== 7) resize window (title should re-measure, stay sane) ==');
  await page.setViewport({ width: 1000, height: 800 });
  await sleep(4500);
  console.log(JSON.stringify(await state()));

  console.log('=== PANICS (' + panics.length + ') ===');
  panics.forEach(p => console.log(p));
  await browser.close();
})().catch(e => { console.error('HARNESS ERROR:', e); process.exit(1); });
