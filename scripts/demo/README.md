# Terminal demo

The 28-second demo shows the install command and a colored help excerpt,
then replays the same RefGene quartile job in GNU datamash and Fastmash. It is a silent animation with no
external player or network requests.

`demo.html` is the editable layout. `record.mjs` reads the Intel laptop
`refgene-quantiles` row from
[`core-jobs.tsv`](../../docs/src/benchmarks/core-jobs.tsv), then captures
deterministic frames at 12 fps. Both video exports repeat frames at 24 fps.
The clocks display the published rounded times. The job's matching output is
`4\t8\t14\t10` for 81,407 records. Playback is slowed 20 times so the
difference is visible, as labeled in the animation. The
[benchmark method](../../docs/src/benchmarks/method.md) provides the full context.

The opening scene teaches the installation command. The help scene uses
the release CLI's bold headings and cyan syntax; calculation output remains
plain. It does not simulate a successful download or installation. The timing scene replays recorded
measurements; rendering does not execute or remeasure either program.

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

`--preview` saves an animated HTML preview and four milestone PNGs in a
temporary directory, without replacing the demo assets. A full render writes:

| Asset | Format | Use |
| --- | --- | --- |
| `site/demo.mp4` | H.264, 1280x720, 24 fps | Social upload copy |
| `site/demo.webm` | AV1, 1280x720, 24 fps | Compact web video and compatible uploaders |
| `site/demo.gif` | 1280x720, 12 fps, looping | Markdown and image-only embeds |
| `site/demo-poster.png` | 1280x720 | Static result frame |

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

## Upload formats

MP4 and WebM are containers; H.264 and AV1 are video codecs. An uploader
listing WebM does not by itself establish support for every WebM codec.

Checked against platform documentation on 2 October 2026:

- [LinkedIn's native uploader](https://www.linkedin.com/help/linkedin/answer/a548372)
  lists MP4 and WebM. Its
  [Videos API](https://learn.microsoft.com/en-us/linkedin/marketing/community-management/shares/videos-api)
  specifies MP4.
- [Bluesky's upload API](https://github.com/bluesky-social/atproto/blob/main/lexicons/app/bsky/video/uploadVideo.json)
  specifies `video/mp4`.
- [Mastodon](https://docs.joinmastodon.org/user/posting/#attachments)
  accepts MP4 and WebM and transcodes video to H.264 MP4.

Use the MP4 when one file needs to work across these upload paths. The AV1
copy is also available for web playback; platform acceptance of that exact
encoding has not been tested. This compact WebM is below LinkedIn's published
bitrate range; use the MP4 there. No platform upload is part of rendering.

## Description for posts

Fastmash terminal demo. Install with one command, read a colored help
excerpt, then calculate quartiles from 81,407 RefGene records using the same command in both tools. Both return
4, 8, 14 and 10. GNU datamash takes 112.8 ms; Fastmash takes 26.6 ms, or
4.2 times as fast on this job on the Intel laptop running native Linux.
The animation replays the measured times at 20 times slower speed. fastmash.io.
