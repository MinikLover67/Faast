import { chromium } from 'playwright';

const browser = await chromium.launch({ headless: true });
const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
const errors = [];
page.on('pageerror', (e) => errors.push('PAGEERROR: ' + e.message));
page.on('console', (m) => { if (m.type() === 'error') errors.push('CONSOLE: ' + m.text()); });
await page.goto('http://localhost:4173', { waitUntil: 'networkidle' });

const state = async () => ({
  listHidden: await page.locator('#downloadList').isHidden(),
  filesHidden: await page.locator('#filesView').isHidden(),
  libActive: await page.locator('#tabLibrary').evaluate((e) => e.classList.contains('active')),
  filesActive: await page.locator('#tabFiles').evaluate((e) => e.classList.contains('active')),
  extraViews: await page.locator('.files-view').count(),
});
console.log('initial', await state());
await page.locator('#tabFiles').click();
console.log('after Files tab', await state());
await page.locator('#tabLibrary').click();
console.log('after Library tab', await state());
await page.keyboard.press('Escape');
console.log('after Escape (no modal)', 'page closed?', page.isClosed());
await page.screenshot({ path: 'scripts/verify-frontend.png' });
console.log('errors:', errors.length ? errors : 'none');
await browser.close();
