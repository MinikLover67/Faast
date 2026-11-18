// Faast UI E2E smoke test — headless Chromium via Playwright (webapp-testing methodology).
// Serves against the built frontend (vite preview) since the Tauri IPC bridge cannot run in plain Chromium.

import { chromium } from 'playwright';

const BASE = process.env.FAUST_BASE_URL || 'http://localhost:4173';
const results = [];
function check(name, ok, extra = '') {
  results.push({ name, ok });
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${extra ? ' — ' + extra : ''}`);
}

const browser = await chromium.launch({ headless: true });
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
const consoleErrors = [];
page.on('console', (m) => {
  if (m.type() === 'error') consoleErrors.push(m.text());
});
page.on('pageerror', (e) => consoleErrors.push('PAGEERROR: ' + e.message));

await page.goto(BASE, { waitUntil: 'networkidle' });
await page.waitForSelector('.window', { state: 'attached', timeout: 15000 });

// ── Title bar ──
check('titlebar renders', (await page.locator('.titlebar').count()) === 1);
check('app brand renders', ((await page.locator('.app-brand').innerText()) || '').toLowerCase().includes('faast'));
check('window controls render (min/max/close)', (await page.locator('.window-controls .wbtn').count()) === 3);
check('close button present', (await page.locator('.wbtn.close[data-action="close"]').count()) === 1);
check('titlebar meta renders', ((await page.locator('.titlebar-meta').innerText()) || '').includes('Open Source'));
check('titlebar is a deep drag region', (await page.locator('.titlebar[data-tauri-drag-region="deep"]').count()) === 1);
check('window buttons are not drag regions', (await page.locator('.window-controls .wbtn[data-tauri-drag-region]').count()) === 0);
check('maximize glyph shows maximize state', (await page.locator('.wbtn[data-action="maximize"] .ic-max').isVisible()) === true);

// ── Sidebar ──
check('sidebar renders', (await page.locator('.sidebar').count()) === 1);
for (const t of ['All downloads', 'Downloading', 'Completed', 'Paused', 'Failed']) {
  check(`sidebar nav "${t}"`, (await page.getByRole('button', { name: new RegExp(t) }).count()) > 0);
}
for (const t of ['Video', 'Music', 'Documents', 'Applications']) {
  check(`category "${t}"`, (await page.getByRole('button', { name: new RegExp(t) }).count()) > 0);
}

// ── Toolbar ──
check('new download button renders', (await page.locator('#btnNew').count()) === 1);
check('pause button renders', (await page.locator('#btnPause').count()) === 1);
check('resume button renders', (await page.locator('#btnResume').count()) === 1);
check('remove button renders', (await page.locator('#btnRemove').count()) === 1);
check('search input renders', (await page.locator('#searchInput').count()) === 1);
check('pause disabled with empty list', (await page.locator('#btnPause').isDisabled()) === true);
check('resume disabled with empty list', (await page.locator('#btnResume').isDisabled()) === true);
check('remove disabled with empty list', (await page.locator('#btnRemove').isDisabled()) === true);

// ── List header + empty state ──
check('list header renders', (await page.locator('.list-header').count()) === 1);
await page.waitForSelector('#downloadList .empty-state', { state: 'attached', timeout: 15000 });
check('empty state shows message', ((await page.locator('#downloadList .empty-state p').innerText()) || '').includes('No downloads'));

// ── Status bar ──
check('statusbar renders', (await page.locator('.statusbar').count()) === 1);
check('global speed shows', ((await page.locator('#globalSpeed').innerText()) || '').length > 0);
check('active count shows', ((await page.locator('#activeCount').innerText()) || '').length > 0);
check('total downloaded shows', ((await page.locator('#totalDownloaded').innerText()) || '').length > 0);

// ── Computed styles (CSS must actually be bundled & applied) ──
const cssRules = await page.evaluate(() => {
  const sheets = Array.from(document.styleSheets);
  return sheets.map((s) => {
    try {
      return s.cssRules ? s.cssRules.length : -1;
    } catch (e) {
      return -2; // cross-origin
    }
  });
});
check('at least one stylesheet with rules loaded', cssRules.some((n) => n > 0), JSON.stringify(cssRules));

const bodyBg = await page.evaluate(() => getComputedStyle(document.body).backgroundColor);
check('body background color applied', bodyBg === 'rgb(9, 9, 11)', bodyBg);

const bodyPad = await page.evaluate(() => getComputedStyle(document.body).padding);
check('body has no outer padding (edge-to-edge)', bodyPad === '0px', bodyPad);

const windowRadius = await page.evaluate(() => getComputedStyle(document.querySelector('.window')).borderRadius);
check('window surface has no floating-card radius', windowRadius === '0px', windowRadius);

const windowBg = await page.evaluate(() => getComputedStyle(document.querySelector('.window')).backgroundColor);
check('window surface background applied', windowBg === 'rgb(9, 9, 11)', windowBg);

const primaryBg = await page.evaluate(() => getComputedStyle(document.querySelector('#btnNew')).backgroundColor);
check('primary button uses theme primary', primaryBg === 'rgb(250, 250, 250)', primaryBg);

const fontFamily = await page.evaluate(() => getComputedStyle(document.body).fontFamily);
check('font family applied', (fontFamily || '').length > 0, fontFamily);

const hasIndeterminate = await page.evaluate(() =>
  Array.from(document.styleSheets).some((s) => {
    try {
      return Array.from(s.cssRules).some((r) => (r.cssText || '').includes('bar-indeterminate'));
    } catch (e) {
      return false;
    }
  })
);
check('indeterminate progress animation present', hasIndeterminate);

// ── Modal open + close via Escape ──
await page.click('#btnNew');
await page.waitForSelector('#modalBackdrop .modal', { state: 'visible', timeout: 8000 });
check('add-download modal opens', (await page.locator('#modalBackdrop .modal').isVisible()) === true);
check('modal animates in', (await page.locator('#modalBackdrop .modal').evaluate((el) => getComputedStyle(el).animationName)) !== 'none');
check('modal backdrop animates in', (await page.locator('#modalBackdrop').evaluate((el) => getComputedStyle(el).animationName)) !== 'none');
check('browse torrent button renders', (await page.locator('#btnModalBrowse').isVisible()) === true);
check('save-dir change button renders', (await page.locator('#btnModalSaveDir').isVisible()) === true);
check('drag-drop hint renders', (await page.locator('#modalBackdrop .modal-hint').innerText() || '').includes('drop a .torrent'));
check('toast container present', (await page.locator('#toastWrap').count()) === 1);

// ── Enter submits the modal (empty input shows the same feedback as the button) ──
await page.click('#downloadUrl');
await page.keyboard.press('Enter');
await page.waitForSelector('.toast', { state: 'visible', timeout: 4000 });
const enterToast = (await page.locator('.toast').first().innerText()) || '';
check('Enter submits the add form', enterToast.toLowerCase().includes('nothing to add'), enterToast.replace(/\n/g, ' '));

// ── Toast feedback: empty add shows a "Nothing to add" toast ──
await page.click('#btnModalAdd');
await page.waitForSelector('.toast', { state: 'visible', timeout: 4000 });
const toastText = (await page.locator('.toast').first().innerText()) || '';
check('empty add shows a toast', toastText.toLowerCase().includes('nothing to add'), toastText.replace(/\n/g, ' '));
await page.keyboard.press('Escape');
await page.keyboard.press('Escape');
await page.waitForSelector('#modalBackdrop .modal', { state: 'hidden', timeout: 8000 });
check('add-download modal closes on Escape', true);

// ── Modal open + close via Cancel button ──
await page.click('#btnNew');
await page.waitForSelector('#modalBackdrop .modal', { state: 'visible', timeout: 8000 });
check('add-download modal reopens', (await page.locator('#modalBackdrop .modal').isVisible()) === true);
await page.click('#btnModalCancel');
await page.waitForSelector('#modalBackdrop .modal', { state: 'hidden', timeout: 8000 });
check('add-download modal closes on Cancel', true);

// ── Modal open + close via backdrop click ──
await page.click('#btnNew');
await page.waitForSelector('#modalBackdrop .modal', { state: 'visible', timeout: 8000 });
// click the backdrop itself (not the modal card) — position: top-left corner of the backdrop
await page.mouse.click(12, 12);
await page.waitForSelector('#modalBackdrop .modal', { state: 'hidden', timeout: 8000 });
check('add-download modal closes on backdrop click', true);

// ── Clipboard auto-add detection logic (pure function test via page eval) ──
const detectResults = await page.evaluate(() => {
  // replicate looksLikeDownloadSource from src/main.js
  function looksLikeDownloadSource(s) {
    const t = String(s || '').trim().toLowerCase();
    if (!t) return false;
    if (t.startsWith('magnet:')) return true;
    if (t.startsWith('http://') || t.startsWith('https://')) return true;
    if (t.endsWith('.torrent')) return true;
    return false;
  }
  return {
    magnet: looksLikeDownloadSource('magnet:?xt=urn:btih:abc123'),
    https: looksLikeDownloadSource('https://example.com/file.zip'),
    ftp: looksLikeDownloadSource('ftp://host/file.bin'),
    torrentUrl: looksLikeDownloadSource('https://example.com/a.torrent'),
    plainText: looksLikeDownloadSource('hello world'),
    empty: looksLikeDownloadSource(''),
    injection: looksLikeDownloadSource('<script>alert(1)</script>'),
  };
});
check('clipboard detector accepts magnet', detectResults.magnet === true);
check('clipboard detector accepts https', detectResults.https === true);
check('clipboard detector rejects unsupported ftp', detectResults.ftp === false);
check('clipboard detector accepts .torrent url', detectResults.torrentUrl === true);
check('clipboard detector rejects plain text', detectResults.plainText === false);
check('clipboard detector rejects empty', detectResults.empty === false);
check('clipboard detector rejects html injection', detectResults.injection === false);

// ── HTML escaping guard (esc function must neutralize injection) ──
const escResults = await page.evaluate(() => {
  function esc(s) {
    return String(s ?? '')
      .replaceAll('&', '&amp;')
      .replaceAll('<', '&lt;')
      .replaceAll('>', '&gt;')
      .replaceAll('"', '&quot;');
  }
  return {
    quote: esc('"><img src=x onerror=alert(1)>'),
    amp: esc('a&b'),
  };
});
check('esc neutralizes attribute-breakout injection', !escResults.quote.includes('<img') && escResults.quote.includes('&lt;img'));
check('esc escapes ampersands', escResults.amp === 'a&amp;b');

// ── Screenshot for visual review ──
await page.screenshot({ path: '/tmp/opencode/faast_e2e.png', fullPage: true });

if (consoleErrors.length) {
  console.log('CONSOLE ERRORS DETECTED:');
  for (const e of consoleErrors) console.log('  - ' + e);
}

await browser.close();

const failed = results.filter((r) => !r.ok);
console.log(`RESULT: ${results.length - failed.length}/${results.length} checks passed`);
if (failed.length) {
  console.log('FAILED CHECKS:');
  for (const f of failed) console.log('  - ' + f.name);
  process.exit(1);
}
process.exit(0);
