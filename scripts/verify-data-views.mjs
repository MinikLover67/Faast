import { chromium } from 'playwright';

const FAKE = [
  { id: '1', fileName: 'game-setup.exe', savePath: 'C:\\Users\\Me\\Downloads\\game-setup.exe', status: 'downloading', percent: 42, downloadedBytes: 4_200_000_000, totalBytes: 10_000_000_000, speedBps: 3_000_000, category: 'app', error: null },
  { id: '2', fileName: 'movie.mkv', savePath: 'C:\\Users\\Me\\Downloads\\Videos\\movie.mkv', status: 'completed', percent: 100, downloadedBytes: 2_000_000_000, totalBytes: 2_000_000_000, speedBps: 0, category: 'video', error: null },
  { id: '3', fileName: 'track.mp3', savePath: 'C:\\Users\\Me\\Downloads\\Videos\\track.mp3', status: 'paused', percent: 30, downloadedBytes: 3_000_000, totalBytes: 10_000_000, speedBps: 0, category: 'music', error: null },
  { id: '4', fileName: 'linux-distro.iso', savePath: 'C:\\Users\\Me\\Downloads\\linux-distro.iso', status: 'downloading', percent: 68, downloadedBytes: 3_400_000_000, totalBytes: 5_000_000_000, speedBps: 8_100_000, category: 'app', sourceKind: 'torrent', error: null },
];

const browser = await chromium.launch({ headless: true });
const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
const errors = [];
page.on('pageerror', (e) => errors.push('PAGEERROR: ' + e.message));
page.on('console', (m) => { if (m.type() === 'error') errors.push('CONSOLE: ' + m.text()); });
await page.addInitScript((fake) => {
  let id = 100;
  window.__FAKE__ = fake;
  window.__TAURI_INTERNALS__ = {
    invoke: async (cmd) => {
      if (cmd === 'list_downloads') return window.__FAKE__;
      if (cmd === 'get_speed_limit') return null;
      if (cmd === 'plugin:event|listen') return ++id;
      if (cmd === 'plugin:window|is_maximized') return false;
      if (cmd === 'plugin:window|on_resized') return ++id;
      return null;
    },
    transformCallback: () => ++id,
    unregisterCallback: () => {},
    unregisterListener: () => {},
    convertFileSrc: (p) => p,
    metadata: { currentWebview: { label: 'main' } },
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
}, FAKE);

await page.goto('http://localhost:4173', { waitUntil: 'networkidle' });
await page.waitForTimeout(300);

console.log('rows in list:', await page.locator('#downloadList .download-row').count());
console.log('counts:', await page.locator('#countAll').innerText(), await page.locator('#countDownloading').innerText(), await page.locator('#countCompleted').innerText());
await page.locator('#tabFiles').click();
console.log('folder rows:', await page.locator('#filesList .folder-row').count());
console.log('folder names:', await page.locator('#filesList .folder-row .file-name').allInnerTexts());
await page.locator('#filesList .folder-row').filter({ hasText: 'Videos' }).click();
console.log('files inside Videos:', await page.locator('#filesList .file-row:not(.folder-row) .file-name').allInnerTexts());
console.log('files views in DOM:', await page.locator('.files-view').count());

// Simulate a progress event through the mocked event bridge if possible: just
// verify repeated renders don't throw and the list stays intact.
await page.locator('#tabLibrary').click();
for (let i = 0; i < 5; i++) {
  await page.evaluate(() => document.getElementById('searchInput').dispatchEvent(new Event('input')));
}
console.log('rows after 5 renders:', await page.locator('#downloadList .download-row').count());
console.log('final state:', {
  listHidden: await page.locator('#downloadList').isHidden(),
  filesHidden: await page.locator('#filesView').isHidden(),
  libActive: await page.locator('#tabLibrary').evaluate((e) => e.classList.contains('active')),
  filesActive: await page.locator('#tabFiles').evaluate((e) => e.classList.contains('active')),
  sidebarActive: await page.locator('#navPrimary .nav-item.active').allInnerTexts(),
  styles: await page.evaluate(() => {
    const info = (sel) => {
      const e = document.querySelector(sel);
      return e ? { cls: e.className, bg: getComputedStyle(e).backgroundColor, color: getComputedStyle(e).color } : null;
    };
    return {
      all: info('#navPrimary .nav-item[data-filter="all"]'),
      filesNav: info('#navPrimary .nav-item[data-view="files"]'),
      libTab: info('#tabLibrary'),
      filesTab: info('#tabFiles'),
    };
  }),
});
console.log('errors:', errors.length ? errors : 'none');
await page.waitForTimeout(500);
await page.screenshot({ path: `scripts/redesign-preview-${Date.now()}.png`, fullPage: false });
await browser.close();
