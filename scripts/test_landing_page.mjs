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
      // A slow script shows that the first paint waits for prepared entrances.
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

const panels = ['panel-script', 'panel-brew', 'panel-deb', 'panel-rpm', 'panel-binstall', 'panel-cargo'];
async function visiblePanels(page) {
  return page.$$eval('.install-panel', elements => elements
    .filter(element => element.getBoundingClientRect().height > 0).map(element => element.id));
}

const desktop = { width: 1350, height: 940, deviceScaleFactor: 1 };
for (const [name, viewport] of [
  ['desktop', desktop],
  ['mobile', { width: 412, height: 823, deviceScaleFactor: 1.75, isMobile: true, hasTouch: true }],
]) {
  test(`${name}: entrances are prepared before the first paint and loading stays stable`, async t => {
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
                entrancesPrepared: [...document.querySelectorAll('[data-reveal]:not([data-revealed])')]
                  .every(element => getComputedStyle(element).opacity === '0'),
              };
            }
          }
        }).observe({ type: 'paint', buffered: true });
      });
      await page.goto(origin, { waitUntil: 'load' });
      // Include the staggered entrances of the first screen.
      await delay(2000);
      const loading = await page.evaluate(() => window.loading);
      t.diagnostic(JSON.stringify({ browser: await browser.version(), viewport, ...loading }));
      assert.deepEqual(loading.firstFrame?.panels, ['panel-script']);
      assert.equal(loading.firstFrame.terminal, true);
      assert.equal(loading.firstFrame.entrancesPrepared, true, 'entrances are prepared before the first paint');
      assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), 'no page overflow');
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
    assert.equal(commands.length, panels.length);
    assert.ok(commands.every(code => code.command.trim() && code.width > 0 && code.focusable));
    assert.equal(await page.$eval('.install-tabs', element => element.getBoundingClientRect().height), 0);
    assert.ok(await page.$$eval('.copy', elements => elements.every(element =>
      element.getBoundingClientRect().height === 0)));
    assert.ok(await page.$$eval('[data-reveal]', elements => elements.every(element =>
      getComputedStyle(element).opacity === '1')));
    assert.equal(await page.$eval('#motion-toggle', element => element.getBoundingClientRect().height), 0);
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
    await page.focus('#tab-cargo');
    for (const [key, panel] of [
      ['ArrowRight', 'panel-script'], ['ArrowLeft', 'panel-cargo'],
      ['Home', 'panel-script'], ['End', 'panel-cargo'],
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

test('reduced motion disables every decorative loop and entrance', async () => {
  const page = await browser.newPage();
  try {
    await page.emulateMediaFeatures([{ name: 'prefers-reduced-motion', value: 'reduce' }]);
    await page.goto(origin, { waitUntil: 'load' });
    assert.ok(await page.$$eval('[data-motion], [data-reveal]', elements =>
      elements.every(element => element.getAnimations().length === 0)));
    assert.equal(await page.$eval('[data-aura]', element => getComputedStyle(element, '::before').animationName), 'none');
    assert.ok(await page.$$eval('[data-reveal]', elements => elements.every(element =>
      getComputedStyle(element).opacity === '1')));
    assert.deepEqual(await page.$eval('#motion-toggle', element => [element.disabled, element.textContent]),
      [true, 'Motion off']);
    await page.click('#tab-deb');
    assert.deepEqual(await visiblePanels(page), ['panel-deb']);
  } finally { await page.close(); }
});

test('decorative motion runs in view and pauses for the page control', async () => {
  const page = await browser.newPage();
  const time = () => page.$eval('[data-motion="breathe"]', element => element.getAnimations()[0]?.currentTime);
  const aura = () => page.$eval('[data-aura]', element => getComputedStyle(element, '::before').animationPlayState);
  try {
    // The desktop layout keeps the hero glow and the terminal aura in view together.
    await page.setViewport(desktop);
    await page.goto(origin, { waitUntil: 'load' });
    await page.waitForFunction(() => document.querySelector('[data-aura]').hasAttribute('data-motion-active'));
    await page.waitForFunction(() => document.querySelector('[data-motion="breathe"]').hasAttribute('data-motion-active'));
    const properties = await page.$eval('[data-motion="breathe"]', element => element.getAnimations()
      .flatMap(animation => animation.effect.getKeyframes().flatMap(frame => Object.keys(frame))));
    assert.deepEqual([...new Set(properties.filter(key =>
      !['offset', 'computedOffset', 'easing', 'composite'].includes(key)))].sort(), ['opacity', 'transform']);
    const before = await time();
    await delay(240);
    assert.ok(await time() > before + 100, 'the hero glow keeps breathing');
    assert.equal(await aura(), 'running');

    await page.click('#motion-toggle');
    assert.equal(await page.$eval('#motion-toggle', element => element.textContent), 'Resume motion');
    await page.waitForFunction(() => document.querySelector('[data-motion="breathe"]').getAnimations()[0].playState === 'paused');
    const paused = await time();
    await delay(200);
    assert.ok(Math.abs(await time() - paused) < 5, 'animation time stays fixed while paused');
    assert.equal(await aura(), 'paused');
    assert.ok(await page.$$eval('[data-reveal]', elements => elements.every(element =>
      element.hasAttribute('data-revealed') && getComputedStyle(element).opacity === '1')),
    'pausing shows every section at once');

    await page.click('#motion-toggle');
    await page.waitForFunction(() => document.querySelector('[data-motion="breathe"]').hasAttribute('data-motion-active'));
    await page.evaluate(() => scrollTo(0, document.body.scrollHeight));
    await page.waitForFunction(() => !document.querySelector('[data-motion="breathe"]').hasAttribute('data-motion-active')
      && !document.querySelector('[data-aura]').hasAttribute('data-motion-active'));
  } finally { await page.close(); }
});

test('scroll entrances wait for a visible position and unfold in sequence', async t => {
  const page = await browser.newPage();
  const state = () => page.$$eval('.better [data-reveal]', elements => elements.map(element => ({
    opacity: Number(getComputedStyle(element).opacity),
    animations: element.getAnimations().length,
  })));
  try {
    // The whole comparison section fits in a desktop viewport.
    await page.setViewport(desktop);
    await page.goto(origin, { waitUntil: 'load' });
    const offscreen = await state();
    await page.$$eval('.better [data-reveal]', elements => scrollTo(0,
      scrollY + Math.min(...elements.map(element => element.getBoundingClientRect().top)) - innerHeight + 8));
    await delay(160);
    const edge = await state();
    t.diagnostic(JSON.stringify({ offscreen, edge }));
    assert.ok(offscreen.every(element => element.opacity === 0), 'offscreen content is prepared before its entrance');
    assert.ok(edge.every(element => element.animations === 0), 'eight visible pixels do not start the entrance');
    await page.$eval('.better', element => element.scrollIntoView({ block: 'center' }));
    await page.waitForFunction(() => document.querySelector('.better [data-reveal]').getAnimations().length > 0);
    await delay(400);
    const entering = await state();
    assert.ok(entering.every(element => element.opacity > 0 && element.opacity < 0.95), 'entrance is still unfolding after 400 ms');
    assert.ok(entering[0].opacity > entering[1].opacity + 0.05, 'related content enters in sequence');
    await page.waitForFunction(() => [...document.querySelectorAll('.better [data-reveal]')].every(element =>
      element.getAnimations().length === 0 && getComputedStyle(element).opacity === '1'));
  } finally { await page.close(); }
});

test('keyboard focus shows content without waiting for its entrance', async () => {
  const page = await browser.newPage();
  try {
    await page.goto(origin, { waitUntil: 'load' });
    assert.equal(await page.$eval('.switch-copy', element => getComputedStyle(element).opacity), '0');
    await page.focus('.switch-copy a');
    assert.deepEqual(await page.$eval('.switch-copy', element => [element.hasAttribute('data-revealed'),
      getComputedStyle(element).opacity, element.getAnimations().length]), [true, '1', 0]);
  } finally { await page.close(); }
});

test('keyboard focus finishes an entrance that is already under way', async () => {
  const page = await browser.newPage();
  try {
    await page.setViewport(desktop);
    await page.goto(origin, { waitUntil: 'load' });
    await page.$eval('.better', element => element.scrollIntoView({ block: 'center' }));
    await page.waitForFunction(() => document.querySelector('.better .more').getAnimations().length > 0);
    await page.focus('.better .more a');
    assert.deepEqual(await page.$eval('.better .more', element =>
      [getComputedStyle(element).opacity, element.getAnimations().length]), ['1', 0]);
  } finally { await page.close(); }
});
