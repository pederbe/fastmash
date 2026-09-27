#!/usr/bin/env python3
"""Draw the core-job benchmark chart for the website from measured results.

usage:
  benchmark_chart.py results --source TEXT NATIVE_SUMMARY WSL_SUMMARY OUT_TSV
  benchmark_chart.py svg RESULTS_TSV OUT_SVG

`results` reads the per-host checkpoint summaries (one row per catalog set,
comparison and job; the last two columns are `cand=S1/S2` and `gnu=S1/S2`,
the median elapsed milliseconds of the two sessions) and writes
docs/src/benchmarks/core-jobs.tsv: for each charted core job and host, the
mean of the two session medians for GNU datamash and Fastmash. `--source`
names the measured build; it is printed under the chart.

`svg` draws a horizontal bar chart of how many times faster Fastmash is than
GNU datamash (GNU median / Fastmash median) per job and host, with the 1x line
marking parity, as an SVG to include inline in the page. Colors come from CSS
variables in docs/theme/fastmash.css, so the chart follows the site theme.
"""
import csv
import html
import sys

# Core jobs measured in milliseconds rather than fractions of one: tiny jobs
# are reported separately as within about 0.1 ms of GNU datamash.
JOBS = [
    ('refgene-quantiles', 'Exon-count quantiles (RefGene)'),
    ('refgene-transcripts', 'Transcripts per gene (RefGene)'),
    ('refgene-exons', 'Exon statistics per gene (RefGene)'),
    ('gene-many', 'Many small groups (genes)'),
    ('genes-example', 'Gene example (datamash manual)'),
    ('decimal-1000000', 'Sum and mean, 1M decimals'),
    ('decimal-100000', 'Sum and mean, 100k decimals'),
    ('grouped-decimal-100000', 'Grouped decimals, 100k rows'),
    ('dominant-group', 'One dominant group'),
    ('wine-by-quality', 'Wine quality by grade'),
    ('disk-spill', 'Large sort with disk spill'),
]
HOSTS = [('native', 'Intel laptop, native Linux'), ('wsl', 'AMD desktop, WSL2')]


def session_mean(field):
    values = [float(v) for v in field.split('=', 1)[1].split('/')]
    return sum(values) / len(values)


def results(source, native, wsl, out):
    rows = []
    for host, path in (('native', native), ('wsl', wsl)):
        found = {}
        for row in csv.reader(open(path), delimiter='\t'):
            if row[0].startswith('core') and row[1] == 'gnu':
                found[row[2]] = (session_mean(row[-1]), session_mean(row[-2]))
        for job, _ in JOBS:
            gnu, fastmash = found[job]
            rows.append((job, host, f'{gnu:.1f}', f'{fastmash:.1f}'))
    with open(out, 'w', newline='') as f:
        f.write(f'# source\t{source}\n')
        writer = csv.writer(f, delimiter='\t', lineterminator='\n')
        writer.writerow(['job', 'host', 'gnu_ms', 'fastmash_ms'])
        writer.writerows(rows)


def svg(results_tsv, out):
    lines = open(results_tsv).read().splitlines()
    source = lines[0].split('\t', 1)[1] if lines[0].startswith('# source') else ''
    data = {(r['job'], r['host']): r for r in csv.DictReader(lines[1:], delimiter='\t')}
    ratio = {key: float(r['gnu_ms']) / float(r['fastmash_ms']) for key, r in data.items()}
    top = max(4.0, max(ratio.values()))
    top = float(int(top) + 1)
    label_w, chart_w, bar_h, gap, row_gap = 250, 420, 11, 3, 14
    row_h = len(HOSTS) * (bar_h + gap) + row_gap
    head, foot = 46, 66
    width = label_w + chart_w + 60
    height = head + row_h * len(JOBS) + foot
    x = lambda v: label_w + chart_w * v / top
    out_lines = [
        f'<svg class="fm-chart" viewBox="0 0 {width} {height}" role="img" '
        f'xmlns="http://www.w3.org/2000/svg" aria-labelledby="fm-chart-title">',
        '<title id="fm-chart-title">How many times faster Fastmash is than GNU datamash '
        'on the core jobs, per host</title>',
    ]
    for i, (_, name) in enumerate(HOSTS):
        lx = label_w + i * 210
        out_lines.append(f'<rect class="fm-bar-{i}" x="{lx}" y="8" width="12" height="12" rx="2"/>')
        out_lines.append(f'<text class="fm-text" x="{lx + 18}" y="18">{html.escape(name)}</text>')
    for tick in range(0, int(top) + 1):
        tx = x(tick)
        cls = 'fm-parity' if tick == 1 else 'fm-grid'
        out_lines.append(f'<line class="{cls}" x1="{tx:.1f}" y1="{head - 8}" x2="{tx:.1f}" '
                         f'y2="{head + row_h * len(JOBS) - row_gap + 4}"/>')
        out_lines.append(f'<text class="fm-muted" x="{tx:.1f}" y="{head + row_h * len(JOBS) + 8}" '
                         f'text-anchor="middle">{tick}×</text>')
    for j, (job, name) in enumerate(JOBS):
        y0 = head + j * row_h
        mid = y0 + (len(HOSTS) * (bar_h + gap) - gap) / 2
        out_lines.append(f'<text class="fm-text" x="{label_w - 10}" y="{mid + 4:.1f}" '
                         f'text-anchor="end">{html.escape(name)}</text>')
        for i, (host, _) in enumerate(HOSTS):
            r = ratio[(job, host)]
            by = y0 + i * (bar_h + gap)
            out_lines.append(f'<rect class="fm-bar-{i}" x="{label_w}" y="{by}" '
                             f'width="{x(r) - label_w:.1f}" height="{bar_h}" rx="2">'
                             f'<title>{html.escape(name)}, {HOSTS[i][1]}: GNU {data[(job, host)]["gnu_ms"]} ms, '
                             f'Fastmash {data[(job, host)]["fastmash_ms"]} ms</title></rect>')
            out_lines.append(f'<text class="fm-value" x="{x(r) + 4:.1f}" y="{by + bar_h - 2}">'
                             f'{r:.1f}×</text>')
    base = head + row_h * len(JOBS) + 26
    out_lines.append(f'<text class="fm-muted" x="{label_w}" y="{base}">Times faster than GNU '
                     'datamash 1.9, by median elapsed time.</text>')
    out_lines.append(f'<text class="fm-muted" x="{label_w}" y="{base + 14}">The dashed line is '
                     'the same speed; shorter bars are slower.</text>')
    base += 14
    if source:
        out_lines.append(f'<text class="fm-muted" x="{label_w}" y="{base + 16}">'
                         f'{html.escape(source)}</text>')
    out_lines.append('</svg>')
    open(out, 'w', encoding='utf-8').write('\n'.join(out_lines) + '\n')


if __name__ == '__main__':
    args = sys.argv[1:]
    if args[:1] == ['results'] and len(args) == 6 and args[1] == '--source':
        results(args[2], args[3], args[4], args[5])
    elif args[:1] == ['svg'] and len(args) == 3:
        svg(args[1], args[2])
    else:
        sys.exit(__doc__)
