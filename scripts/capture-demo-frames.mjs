import { chromium } from 'playwright';
import fs from 'node:fs';

const FAKE = [
  { id: '1', fileName: 'game-setup.exe', savePath: 'C:\\Users\\Me\\Downloads\\game-setup.exe', status: 'downloading', percent: 42, downloadedBytes: 4_200_000_000, totalBytes: 10_000_000_000, speedBps: 3_000_000, category: 'app', sourceKind: 'http', error: null },
  { id: '2', fileName: 'movie.mkv', savePath: 'C:\\Users\\Me\\Downloads\\Videos\\movie.mkv', status: 'completed', percent: 100, downloadedBytes: 2_000_000_000, totalBytes: 2_000_000_000, speedBps: 0, category: 'video', sourceKind: 'http', error: null },
  { id: '3', fileName: 'linux-distro.iso', savePath: 'C:\\Users\\Me\\Downloads\\linux-distro.iso', status: 'downloading', percent: 68, downloadedBytes: 3_400_000_000, totalBytes: 5_000_000_000, speedBps: 8_100_000, category: 'app', sourceKind: 'torrent', error: null },
];

fs.mkdirSync('scripts/gif', { recursive: true });
const browser = await chromium.launch({ headless: true });
const page = await browser.newPage({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 1 });
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
  document.documentElement.dataset.theme = 'dark';
}, FAKE);

await page.goto('http://localhost:4173', { waitUntil: 'networkidle' });
await page.waitForTimeout(600);
await page.screenshot({ path: 'scripts/gif/01-list.png' });

await page.locator('#btnNew').click();
await page.waitForTimeout(400);
await page.locator('#downloadUrl').fill('https://example.com/linux-distro.iso');
await page.screenshot({ path: 'scripts/gif/02-modal.png' });

await page.keyboard.press('Escape');
await page.waitForTimeout(400);
await page.locator('#tabFiles').click();
await page.waitForTimeout(400);
await page.screenshot({ path: 'scripts/gif/03-files.png' });

console.log('captured 3 frames');
await browser.close();
