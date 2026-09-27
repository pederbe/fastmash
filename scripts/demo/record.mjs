// Render scripts/demo/demo.html to site/demo.gif, and scripts/demo/og.html to
// site/og.png (the 1200x630 link-preview image).
//
// usage: node scripts/demo/record.mjs            (from the repository root)
//
// Needs the site tools (npm ci --ignore-scripts; npx puppeteer browsers install
// chrome) and ImageMagick's `convert`. Frames are captured at 10 per second by
// stepping the page's render(t), so timing does not depend on this machine.
import { execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import puppeteer from 'puppeteer';

const FPS = 10;
const root = resolve(import.meta.dirname, '..', '..');
const frames = mkdtempSync(join(tmpdir(), 'fastmash-demo-'));
const browser = await puppeteer.launch({ headless: true, args: ['--no-sandbox', '--disable-gpu'] });
try {
  const page = await browser.newPage();
  await page.setViewport({ width: 960, height: 540, deviceScaleFactor: 1 });
  await page.goto('file://' + join(root, 'scripts', 'demo', 'demo.html'));
  await page.evaluate(() => document.fonts.ready);
  const duration = await page.evaluate(() => window.DURATION);
  const count = Math.ceil(duration * FPS);
  for (let frame = 0; frame < count; frame++) {
    await page.evaluate((t) => window.render(t), frame / FPS);
    await page.screenshot({ path: join(frames, String(frame).padStart(4, '0') + '.png') });
  }
  console.log(`${count} frames, ${duration.toFixed(1)} s`);
  await page.setViewport({ width: 1200, height: 630, deviceScaleFactor: 1 });
  await page.goto('file://' + join(root, 'scripts', 'demo', 'og.html'));
  await page.evaluate(() => document.fonts.ready);
  await page.screenshot({ path: join(root, 'site', 'og.png') });
  console.log('site/og.png');
} finally {
  await browser.close();
}
execFileSync('convert', ['-delay', String(100 / FPS), '-loop', '0', join(frames, '*.png'),
  '-colors', '64', '-layers', 'Optimize', join(root, 'site', 'demo.gif')], { stdio: 'inherit' });
rmSync(frames, { recursive: true, force: true });
console.log('site/demo.gif');
