import { chromium } from 'playwright';

const browser = await chromium.connectOverCDP('http://127.0.0.1:9222');
const contexts = browser.contexts();
const page = contexts[0].pages().find((p) => p.url().includes('tauri.localhost')) || contexts[0].pages()[0];
const errors = [];
page.on('pageerror', (e) => errors.push('PAGEERROR: ' + e.message));
page.on('console', (m) => { if (m.type() === 'error') errors.push('CONSOLE: ' + m.text()); });

const check = (name, ok, extra = '') => console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${extra ? ' — ' + extra : ''}`);

console.log('URL:', page.url());
check('dark theme active', await page.evaluate(() => document.documentElement.dataset.theme === 'dark'));
check('titlebar renders', await page.locator('.titlebar').count() === 1);
check('sidebar renders', await page.locator('.sidebar').count() === 1);
check('new download button visible', await page.locator('#btnNew').isVisible());

// Add-download modal
await page.locator('#btnNew').click();
await page.waitForTimeout(350);
check('modal opens', await page.locator('#modalBackdrop').isVisible());
check('modal input focusable', await page.locator('#downloadUrl').isEditable());
await page.locator('#downloadUrl').fill('https://example.com/file.zip');
check('media format picker stays hidden for non-media URL', await page.locator('#formatRow').isHidden());
await page.locator('#modalUrl').count(); // no-op guard
await page.keyboard.press('Escape');
await page.waitForTimeout(350);
check('Escape closes modal', await page.locator('#modalBackdrop').isHidden());

// View switching
await page.locator('#tabFiles').click();
await page.waitForTimeout(250);
check('Files tab shows files view', await page.locator('#filesView').isVisible() && await page.locator('#downloadList').isHidden());
await page.locator('#tabLibrary').click();
await page.waitForTimeout(250);
check('Library tab shows list', await page.locator('#downloadList').isVisible() && await page.locator('#filesView').isHidden());

// Search + toolbar
await page.locator('#searchInput').fill('nothing-matches');
await page.waitForTimeout(150);
check('search filters without error', await page.locator('#downloadList').count() === 1);
await page.locator('#searchInput').fill('');

// Theme toggle
const before = await page.evaluate(() => document.documentElement.dataset.theme);
await page.locator('#btnThemeToggle').click();
await page.waitForTimeout(150);
const after = await page.evaluate(() => document.documentElement.dataset.theme);
check('theme toggle switches theme', before !== after, `${before} -> ${after}`);
// restore dark
if (after !== 'dark') await page.locator('#btnThemeToggle').click();
await page.waitForTimeout(150);

check('no console/page errors', errors.length === 0, errors.join(' | ') || 'none');
await browser.close();
