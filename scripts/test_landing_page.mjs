// Browser checks for the generated landing page. Build docs/book first.
// Uses the Puppeteer already supplied by the site's diagram tooling.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { extname, resolve, sep } from 'node:path';
import { after, before, test } from 'node:test';
import { setTimeout as delay } from 'node:timers/promises';
import puppeteer from 'puppeteer';

const book = resolve(import.meta.dirname, '../docs/book');
const types = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css',
  '.svg': 'image/svg+xml', '.woff2': 'font/woff2' };
let browser, server, origin;

before(async () => {
  await readFile(resolve(book, 'index.html'));
  server = createServer(async (request, response) => {
    try {
      const url = new URL(request.url, 'http://localhost');
      const file = resolve(book, `.${url.pathname === '/' ? '/index.html' : url.pathname}`);
      if (!file.startsWith(book + sep)) { response.writeHead(403).end(); return; }
      // A slow script makes the initial layout observable even on a fast host.
      if (url.pathname === '/landing.js') await delay(600);
      const data = await readFile(file);
      response.writeHead(200, { 'Content-Type': types[extname(file)] || 'application/octet-stream',
        'Cache-Control': 'no-store' });
      response.end(data);
    } catch { response.writeHead(404).end(); }
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  origin = `http://127.0.0.1:${server.address().port}`;
  browser = await puppeteer.launch({ headless: true,
    ...(process.env.BROWSER_PATH ? { executablePath: process.env.BROWSER_PATH } : {}) });
});

after(async () => {
  await browser?.close();
  if (server) await new Promise(resolve => server.close(resolve));
});

const panels = ['panel-script', 'panel-cargo', 'panel-binstall', 'panel-deb', 'panel-rpm'];
async function visiblePanels(page) {
  return page.$$eval('.install-panel', elements => elements
    .filter(element => element.getBoundingClientRect().height > 0).map(element => element.id));
}

for (const [name, viewport] of [
  ['desktop', { width: 1350, height: 940, deviceScaleFactor: 1 }],
  ['mobile', { width: 412, height: 823, deviceScaleFactor: 1.75, isMobile: true, hasTouch: true }],
]) {
  test(`${name}: the installer is stable before and after script initialization`, async t => {
    const page = await browser.newPage();
    try {
      await page.setViewport(viewport);
      await page.evaluateOnNewDocument(() => {
        window.loading = { shifts: [], firstFrame: null, paints: [] };
        new PerformanceObserver(list => {
          for (const entry of list.getEntries()) {
            if (!entry.hadRecentInput) window.loading.shifts.push({ value: entry.value,
              time: entry.startTime, sources: entry.sources.map(source => ({
                element: source.node?.className || source.node?.id || source.node?.tagName,
                before: source.previousRect.toJSON(), after: source.currentRect.toJSON(),
              })) });
          }
        }).observe({ type: 'layout-shift', buffered: true });
        new PerformanceObserver(list => {
          for (const entry of list.getEntries()) {
            window.loading.paints.push({ name: entry.name, time: entry.startTime });
            if (entry.name === 'first-contentful-paint') {
              window.loading.firstFrame = {
                panels: [...document.querySelectorAll('.install-panel')]
                  .filter(panel => panel.getBoundingClientRect().height > 0).map(panel => panel.id),
                terminal: !!document.querySelector('.terminal'),
              };
            }
          }
        }).observe({ type: 'paint', buffered: true });
      });
      await page.goto(origin, { waitUntil: 'load' });
      // Include the one-time entrance motion after load and font readiness.
      await delay(2000);
      const loading = await page.evaluate(() => window.loading);
      t.diagnostic(JSON.stringify({ browser: await browser.version(), viewport, ...loading }));
      assert.deepEqual(loading.firstFrame?.panels, ['panel-script']);
      assert.equal(loading.firstFrame.terminal, true);
      assert.ok(loading.shifts.reduce((sum, shift) => sum + shift.value, 0) < 0.1,
        'initial loading must not cause a large layout shift');
    } finally { await page.close(); }
  });
}

test('every installation command remains visible without JavaScript', async () => {
  const page = await browser.newPage();
  try {
    await page.setJavaScriptEnabled(false);
    await page.goto(origin, { waitUntil: 'load' });
    assert.deepEqual(await visiblePanels(page), panels);
    const commands = await page.$$eval('.install-panel code', elements => elements.map(element => ({
      command: element.textContent, width: element.getBoundingClientRect().width,
      focusable: element.tabIndex === 0,
    })));
    assert.equal(commands.length, 5);
    assert.ok(commands.every(code => code.command.trim() && code.width > 0 && code.focusable));
    assert.equal(await page.$eval('.install-tabs', element => element.getBoundingClientRect().height), 0);
    assert.ok(await page.$$eval('.copy', elements => elements.every(element =>
      element.getBoundingClientRect().height === 0)));
  } finally { await page.close(); }
});

test('installation tabs retain click and keyboard selection', async () => {
  const page = await browser.newPage();
  try {
    await page.goto(origin, { waitUntil: 'load' });
    for (const panel of panels) {
      await page.click(`#tab-${panel.slice('panel-'.length)}`);
      assert.deepEqual(await visiblePanels(page), [panel]);
      const selected = await page.$$eval('.install-tabs [role="tab"]', tabs => tabs
        .filter(tab => tab.getAttribute('aria-selected') === 'true' && tab.tabIndex === 0)
        .map(tab => tab.getAttribute('aria-controls')));
      assert.deepEqual(selected, [panel]);
    }
    await page.focus('#tab-rpm');
    for (const [key, panel] of [
      ['ArrowRight', 'panel-script'], ['ArrowLeft', 'panel-rpm'],
      ['Home', 'panel-script'], ['End', 'panel-rpm'],
    ]) {
      await page.keyboard.press(key);
      assert.deepEqual(await visiblePanels(page), [panel]);
      assert.equal(await page.evaluate(() => document.activeElement.getAttribute('aria-controls')), panel);
    }
  } finally { await page.close(); }
});

test('copy controls send the selected command to the clipboard API', async () => {
  const page = await browser.newPage();
  try {
    // The platform clipboard is an external boundary; keep the test independent
    // of the desktop's clipboard service and permissions.
    await page.evaluateOnNewDocument(() => Object.defineProperty(navigator, 'clipboard', {
      value: { writeText(command) { window.copiedCommand = command; return Promise.resolve(); } },
    }));
    await page.goto(origin, { waitUntil: 'load' });
    await page.click('#tab-cargo');
    await page.click('#panel-cargo .copy');
    await page.waitForFunction(() => document.getElementById('copy-status').textContent === 'Copied to the clipboard');
    assert.equal(await page.evaluate(() => window.copiedCommand), 'cargo install --locked fastmash');
  } finally { await page.close(); }
});

test('copy controls select the command when clipboard access is unavailable', async () => {
  const page = await browser.newPage();
  try {
    await page.evaluateOnNewDocument(() => Object.defineProperty(navigator, 'clipboard', { value: undefined }));
    await page.goto(origin, { waitUntil: 'load' });
    await page.click('#panel-script .copy');
    assert.equal(await page.evaluate(() => window.getSelection().toString()),
      'curl -fsSL https://fastmash.io/install.sh | sh');
    assert.match(await page.$eval('#copy-status', element => element.textContent), /Command selected/);
  } finally { await page.close(); }
});

test('reduced motion leaves content and controls available without entrance animation', async () => {
  const page = await browser.newPage();
  try {
    await page.emulateMediaFeatures([{ name: 'prefers-reduced-motion', value: 'reduce' }]);
    await page.goto(origin, { waitUntil: 'load' });
    await delay(1000);
    assert.equal(await page.$$eval('.is-revealed', elements => elements.length), 0);
    assert.equal(await page.$eval('.hero-copy', element => getComputedStyle(element).animationName), 'none');
    await page.click('#tab-deb');
    assert.deepEqual(await visiblePanels(page), ['panel-deb']);
  } finally { await page.close(); }
});
