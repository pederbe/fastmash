// Render the terminal demo from the published benchmark table.
// See README.md for setup, previews and output formats.
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const flags = new Set(process.argv.slice(2));
for (const flag of flags) {
  if (!['--preview', '--og'].includes(flag)) throw new Error(`Unknown option: ${flag}`);
}
const FPS = 12;
const root = resolve(import.meta.dirname, '..', '..');
const ffmpeg = process.env.FFMPEG_PATH || 'ffmpeg';
const module = process.env.PUPPETEER_MODULE;
const { default: puppeteer } = await import(module ? pathToFileURL(resolve(module)).href : 'puppeteer');
const rows = readFileSync(join(root, 'docs/src/benchmarks/core-jobs.tsv'), 'utf8')
  .split(/\r?\n/).filter(line => line && !line.startsWith('#')).map(line => line.split('\t'));
const headers = rows.shift();
const matches = rows.map(row => Object.fromEntries(headers.map((key, i) => [key, row[i]])))
  .filter(row => row.job === 'refgene-quantiles' && row.host === 'laptop');
if (matches.length !== 1) throw new Error('Expected exactly one laptop RefGene quartile result');
const data = { gnu_ms: Number(matches[0].gnu_ms), fastmash_ms: Number(matches[0].fastmash_ms) };
if (!Object.values(data).every(value => Number.isFinite(value) && value > 0)) {
  throw new Error('Benchmark times must be finite and positive');
}
const template = readFileSync(join(import.meta.dirname, 'demo.html'), 'utf8');
const marker = '/* BENCHMARK_DATA */ null';
if (template.split(marker).length !== 2) throw new Error('Missing or ambiguous benchmark marker');
const html = template.replace(marker, JSON.stringify(data));
const frames = mkdtempSync(join(tmpdir(), 'fastmash-demo-'));
writeFileSync(join(frames, 'preview.html'), html.replace('</body>', '<script>window.play();</script></body>'));
console.log(`Render files: ${frames}`);

let count;
const browser = await puppeteer.launch({
  headless: true,
  ...(process.env.BROWSER_PATH ? { executablePath: process.env.BROWSER_PATH } : {}),
});
try {
  const page = await browser.newPage();
  const pageErrors = [];
  page.on('pageerror', error => pageErrors.push(error.message));
  await page.setViewport({ width: 1280, height: 720, deviceScaleFactor: 1 });
  await page.setContent(html);
  await page.evaluate(() => document.fonts.ready);
  if (pageErrors.length) throw new Error(pageErrors.join('\n'));
  const demo = await page.evaluate(() => ({ ...window.DEMO, duration: window.DURATION }));
  if (!Number.isFinite(demo.duration) || demo.duration <= 0) throw new Error('Invalid demo duration');
  writeFileSync(join(frames, 'timing.json'), JSON.stringify(demo, null, 2) + '\n');
  for (const [name, time] of [
    ['install', demo.helpStart - 1],
    ['help', demo.compareStart - 1],
    ['running', demo.runStart + demo.timings.fastmash * demo.playbackScale + 0.25],
    ['result', demo.duration - 1],
  ]) {
    await page.evaluate(t => window.render(t), time);
    const overflow = await page.evaluate(() => [...document.querySelectorAll('.scene:not(.hidden) pre')]
      .filter(el => el.scrollWidth > el.clientWidth || el.getBoundingClientRect().bottom > el.parentElement.getBoundingClientRect().bottom)
      .map(el => el.id));
    if (overflow.length) throw new Error(`Clipped terminal content: ${overflow.join(', ')}`);
    await page.screenshot({ path: join(frames, `${name}.png`) });
  }
  if (!flags.has('--preview')) {
    count = Math.ceil(demo.duration * FPS);
    for (let frame = 0; frame < count; frame++) {
      await page.evaluate(t => window.render(t), frame / FPS);
      await page.screenshot({ path: join(frames, String(frame).padStart(4, '0') + '.png') });
    }
    console.log(`${count} frames, ${demo.duration.toFixed(1)} s`);
  }
  if (flags.has('--og')) {
    await page.setViewport({ width: 1200, height: 630, deviceScaleFactor: 1 });
    await page.goto(pathToFileURL(join(import.meta.dirname, 'og.html')).href);
    await page.evaluate(() => document.fonts.ready);
    await page.screenshot({ path: join(frames, 'og.png') });
  }
  if (pageErrors.length) throw new Error(pageErrors.join('\n'));
} finally {
  await browser.close();
}

if (!flags.has('--preview')) {
  const input = ['-framerate', String(FPS), '-i', join(frames, '%04d.png')];
  function encode(args) {
    execFileSync(ffmpeg, ['-hide_banner', '-loglevel', 'warning', '-y', ...args], { stdio: 'inherit' });
  }
  encode([...input, '-vf', 'palettegen=max_colors=128:stats_mode=diff', '-frames:v', '1', '-update', '1', join(frames, 'palette.png')]);
  encode([...input, '-i', join(frames, 'palette.png'), '-lavfi', 'paletteuse=dither=none', '-loop', '0', join(frames, 'demo.gif')]);
  encode([...input, '-an', '-c:v', 'libx264', '-preset', 'medium',
    '-b:v', '512k', '-minrate', '512k', '-maxrate', '512k', '-bufsize', '1024k',
    '-x264-params', 'nal-hrd=cbr:force-cfr=1',
    '-pix_fmt', 'yuv420p', '-r', '24', '-movflags', '+faststart', join(frames, 'demo.mp4')]);
  encode([...input, '-an', '-c:v', 'libaom-av1', '-cpu-used', '6', '-row-mt', '1', '-threads', '4',
    '-crf', '24', '-b:v', '0', '-pix_fmt', 'yuv420p', '-r', '24', join(frames, 'demo.webm')]);
  for (const name of ['demo.gif', 'demo.mp4', 'demo.webm']) {
    copyFileSync(join(frames, name), join(root, 'site', name));
    console.log(`site/${name}`);
  }
  copyFileSync(join(frames, 'result.png'), join(root, 'site/demo-poster.png'));
}
if (flags.has('--og')) {
  copyFileSync(join(frames, 'og.png'), join(root, 'site/og.png'));
  console.log('site/og.png');
}
