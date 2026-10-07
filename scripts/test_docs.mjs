// Browser checks for the generated mdBook. Build docs/book first.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { extname, resolve, sep } from 'node:path';
import { after, before, test } from 'node:test';
import puppeteer from 'puppeteer';

const book = resolve(import.meta.dirname, '../docs/book');
const types = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css',
  '.svg': 'image/svg+xml', '.woff2': 'font/woff2' };
const chapters = ['/introduction', '/guide/operations', '/guide/csv-and-result-names',
  '/contributing/architecture', '/adr/0003-portable-results'];
let browser, server, origin;

before(async () => {
  await readFile(resolve(book, 'introduction.html'));
  server = createServer(async (request, response) => {
    try {
      const url = new URL(request.url, 'http://localhost');
      const pathname = url.pathname.endsWith('/') ? `${url.pathname}index.html`
        : extname(url.pathname) ? url.pathname : `${url.pathname}.html`;
      const file = resolve(book, `.${pathname}`);
      if (!file.startsWith(book + sep)) { response.writeHead(403).end(); return; }
      const data = await readFile(file);
      response.writeHead(200, { 'Content-Type': types[extname(file)] || 'application/octet-stream' });
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

for (const width of [320, 640, 1280]) {
  test(`docs at ${width}px: themes keep content and wide panels within the page`, async () => {
    const context = await browser.createBrowserContext();
    const page = await context.newPage();
    try {
      await page.setViewport({ width, height: 900 });
      for (const chapter of chapters) {
        await page.goto(origin + chapter, { waitUntil: 'load' });
        for (const theme of ['navy', 'light', 'rust', 'coal', 'ayu']) {
          await page.evaluate(theme => {
            document.documentElement.classList.remove('navy', 'light', 'rust', 'coal', 'ayu');
            document.documentElement.classList.add(theme);
          }, theme);
          const layout = await page.evaluate(() => {
            const main = document.querySelector('main');
            const bounds = main.getBoundingClientRect();
            return {
              page: document.documentElement.scrollWidth,
              viewport: innerWidth,
              main: { width: main.clientWidth, scroll: main.scrollWidth },
              panels: [...main.querySelectorAll('pre, .table-wrapper, .diagram')].map(panel => {
                const rect = panel.getBoundingClientRect();
                return { left: rect.left, right: rect.right };
              }),
              left: bounds.left, right: bounds.right,
            };
          });
          const label = `${chapter}, ${theme}, ${width}px`;
          assert.ok(layout.page <= layout.viewport, `page overflow: ${label}`);
          assert.ok(layout.main.scroll <= layout.main.width + 1, `content overflow: ${label}`);
          assert.ok(layout.panels.every(panel => panel.left >= layout.left - 1
            && panel.right <= layout.right + 1), `panel overflow: ${label}`);
        }
      }
    } finally { await context.close(); }
  });
}

test('introduction cards keep every original getting-started link', async () => {
  const context = await browser.createBrowserContext();
  const page = await context.newPage();
  try {
    await page.goto(origin + '/introduction');
    assert.deepEqual(await page.$$eval('#get-started + ul a', links => links.map(link =>
      new URL(link.href).pathname)), ['/install', '/quick-start', '/guide/operations',
      '/guide/csv-and-result-names', '/guide/top-n-records', '/guide/table-health',
      '/guide/dataset-comparison']);
    assert.equal(await page.$$eval('#get-started + ul > li', items => items.length), 6);
  } finally { await context.close(); }
});

test('search and native theme selection remain usable', async () => {
  const context = await browser.createBrowserContext();
  const page = await context.newPage();
  try {
    await page.goto(origin + '/guide/operations');
    await page.click('#mdbook-search-toggle');
    await page.type('#mdbook-searchbar', 'weighted');
    await page.waitForSelector('#mdbook-searchresults a');
    assert.ok(await page.$$eval('#mdbook-searchresults a', links => links.some(link =>
      link.href.includes('weighted-mean'))));
    await page.click('#mdbook-theme-toggle');
    await page.click('#mdbook-theme-light');
    assert.ok(await page.$eval('html', html => html.classList.contains('light')));
    await page.reload();
    assert.ok(await page.$eval('html', html => html.classList.contains('light')));
  } finally { await context.close(); }
});

test('mobile sidebar, decision folding and skip link support keyboard use', async () => {
  const context = await browser.createBrowserContext();
  const page = await context.newPage();
  try {
    await page.setViewport({ width: 640, height: 900 });
    await page.goto(origin + '/introduction');
    await page.focus('#mdbook-sidebar-toggle');
    await page.keyboard.down('Shift');
    await page.keyboard.press('Tab');
    await page.keyboard.up('Shift');
    assert.equal(await page.$eval(':focus', element => element.className), 'skip-link');
    await page.keyboard.press('Enter');
    assert.equal(await page.$eval(':focus', element => element.id), 'main-content');
    await page.focus('#mdbook-sidebar-toggle');
    await page.keyboard.press('Enter');
    assert.ok(await page.$eval('html', html => html.classList.contains('sidebar-visible')));
    const decisionLink = await page.$('#mdbook-sidebar a[href="adr/"]');
    assert.ok(decisionLink);
    const toggle = await decisionLink.evaluateHandle(link => link.nextElementSibling);
    assert.equal(await toggle.evaluate(element => element.getAttribute('aria-expanded')), 'false');
    await toggle.asElement().focus();
    await page.keyboard.press('Enter');
    assert.equal(await toggle.evaluate(element => element.getAttribute('aria-expanded')), 'true');
    await page.keyboard.press('Space');
    assert.equal(await toggle.evaluate(element => element.getAttribute('aria-expanded')), 'false');
  } finally { await context.close(); }
});

test('wide examples scroll by keyboard and expose the copy control on focus', async () => {
  const context = await browser.createBrowserContext();
  const page = await context.newPage();
  try {
    await page.setViewport({ width: 320, height: 900 });
    await page.goto(origin + '/guide/csv-and-result-names');
    const example = await page.$('pre > code');
    await example.focus();
    assert.equal(await example.evaluate(code => getComputedStyle(code).overflowX), 'auto');
    const button = await example.evaluateHandle(code => code.parentElement.querySelector('.clip-button'));
    await page.waitForFunction(() => {
      const buttons = document.querySelector('pre:focus-within > .buttons');
      return buttons && getComputedStyle(buttons).visibility === 'visible';
    });
    await button.asElement().click();
    await page.waitForFunction(() => [...document.querySelectorAll('.tooltiptext')].some(tooltip =>
      tooltip.textContent === 'Copied!'));
  } finally { await context.close(); }
});

for (const scheme of ['light', 'dark']) {
  test(`${scheme} reading and chapter navigation remain available without JavaScript`, async () => {
    const context = await browser.createBrowserContext();
    const page = await context.newPage();
    try {
      await page.setViewport({ width: 320, height: 900 });
      await page.setJavaScriptEnabled(false);
      await page.emulateMediaFeatures([{ name: 'prefers-color-scheme', value: scheme }]);
      await page.goto(origin + '/introduction');
      assert.equal(await page.$eval('main h1', heading => heading.textContent), 'Fastmash');
      assert.equal(await page.$$eval('#get-started + ul a', links => links.length), 7);
      assert.equal(await page.$eval('main a:not(.header)', link => getComputedStyle(link).color),
        scheme === 'dark' ? 'rgb(45, 212, 191)' : 'rgb(15, 118, 110)');
      const contrast = await page.$eval('pre > code', code => {
        function luminance(color) {
          const channels = color.match(/[\d.]+/g).slice(0, 3).map(Number).map(value => {
            const channel = value / 255;
            return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
          });
          return channels[0] * 0.2126 + channels[1] * 0.7152 + channels[2] * 0.0722;
        }
        const foreground = getComputedStyle(code).color;
        let surface = code;
        while (getComputedStyle(surface).backgroundColor === 'rgba(0, 0, 0, 0)') {
          surface = surface.parentElement;
        }
        const background = getComputedStyle(surface).backgroundColor;
        const values = [luminance(foreground), luminance(background)].sort((a, b) => a - b);
        return { foreground, background, ratio: (values[1] + 0.05) / (values[0] + 0.05) };
      });
      assert.ok(contrast.ratio >= 4.5, `unreadable ${scheme} code example: ${JSON.stringify(contrast)}`);
      assert.equal(await page.$eval('html', html => getComputedStyle(html).colorScheme), scheme);
      await page.goto(origin + '/contributing/architecture');
      assert.ok(await page.$$eval(`.diagram-${scheme}`, images => images.length > 0 && images.every(image =>
        getComputedStyle(image).display === 'block')));
      assert.ok(await page.$$eval(`.diagram-${scheme === 'dark' ? 'light' : 'dark'}`, images =>
        images.length > 0 && images.every(image => getComputedStyle(image).display === 'none')));
      await Promise.all([page.waitForNavigation(), page.click('.sidebar-contents-link')]);
      assert.equal(new URL(page.url()).pathname, '/toc');
      assert.ok(await page.$('a[href="guide/operations"]'));
    } finally { await context.close(); }
  });
}

test('reduced motion and print keep the content readable', async () => {
  const context = await browser.createBrowserContext();
  const page = await context.newPage();
  try {
    await page.emulateMediaFeatures([{ name: 'prefers-reduced-motion', value: 'reduce' }]);
    await page.goto(origin + '/introduction');
    assert.equal(await page.$eval('.page-wrapper', element => getComputedStyle(element).transitionDuration), '0s');
    await page.emulateMediaType('print');
    assert.equal(await page.$eval('#get-started + ul', element => getComputedStyle(element).display), 'block');
    assert.equal(await page.$eval('pre', element => getComputedStyle(element).boxShadow), 'none');
    assert.equal(await page.$eval('.skip-link', element => getComputedStyle(element).display), 'none');
    assert.equal(await page.$eval('html', element => getComputedStyle(element).backgroundColor), 'rgb(255, 255, 255)');
    assert.equal(await page.$eval('main', element => getComputedStyle(element).color), 'rgb(17, 17, 17)');
    assert.equal(await page.$eval('pre code', element => getComputedStyle(element).color), 'rgb(17, 17, 17)');
    assert.equal(await page.$$eval('#get-started + ul a', links => links.length), 7);
  } finally { await context.close(); }
});
