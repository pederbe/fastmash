# Terminal demo

The 48-second demo shows installation, colored help, regular terminal results
and a health report excerpt, then replays the same RefGene quartile job in GNU
datamash and Fastmash. It is a silent animation with no external player or
network requests.

`demo.html` is the editable layout. `record.mjs` reads the Intel laptop
`refgene-quantiles` row from
[`core-jobs.tsv`](../../docs/src/benchmarks/core-jobs.tsv), then captures
deterministic frames at 12 fps. Both video exports repeat frames at 24 fps.
The clocks display the published rounded times. The job's matching output is
`4\t8\t14\t10` for 81,407 records. Playback is slowed 20 times so the
difference is visible, as labeled in the animation. The
[benchmark method](../../docs/src/benchmarks/method.md) provides the full context.

The opening scene teaches the installation command without simulating a
successful download or installation. The help and result scenes use the CLI's
purple headings, blue syntax and result values, green completion marks and gray
secondary text. Results show a quick sum and mean, followed by grouped team scores.
Files and pipes retain their plain data format.

The health excerpt shows mixed-type advisories in amber and a declared numerical
type violation in red. Ellipses mark omitted report sections. The displayed
results and report text match actual terminal captures. The timing scene replays
recorded measurements and their captured plain output. Rendering does not execute
or remeasure either program.

## Render

Use Node.js 22 or later, Puppeteer and a Chromium browser from the site's
existing development tools, plus FFmpeg with `libx264` and `libaom-av1`.
From the repository root:

```sh
npm ci --ignore-scripts
npx puppeteer browsers install chrome
node scripts/demo/record.mjs --preview
node scripts/demo/record.mjs
```

`--preview` saves an animated HTML preview and six milestone PNGs in a
temporary directory, without replacing the demo assets. A full render writes:

| Asset | Format | Use |
| --- | --- | --- |
| `site/demo.mp4` | H.264, 1280x720, 24 fps | Social upload copy |
| `site/demo.webm` | AV1, 1280x720, 24 fps | Compact web video and compatible uploaders |
| `site/demo.gif` | 1280x720, 12 fps, looping | Markdown and image-only embeds |
| `site/demo-poster.png` | 1280x720 | Styled calculation frame |

The MP4 uses 512 kbit/s constant bitrate, above LinkedIn's documented
192 kbit/s minimum. The WebM uses constant-quality AV1 encoding. Each export
is encoded directly from the captured PNGs, avoiding conversion losses
between video formats. Inspect the terminal text after rendering.

Temporary frames and preview files are kept for inspection. Published demo
files are replaced only after all three encodes succeed. The separate link
preview image is changed only when `--og` is supplied.

Optional environment variables reuse an existing setup:

- `BROWSER_PATH`: Chromium, Chrome or Edge executable.
- `PUPPETEER_MODULE`: absolute path to a Puppeteer or Puppeteer Core module's
  JavaScript entry point.
- `FFMPEG_PATH`: FFmpeg executable, if it is not on `PATH`.

The shared link-preview image uses `scripts/demo/og.html`. Render it with:

```sh
node scripts/demo/record.mjs --og
```

This also refreshes the demo assets. To inspect a preview and image without
replacing committed assets, use `node scripts/demo/record.mjs --preview --og`.
Review the image and its alternative text together.
