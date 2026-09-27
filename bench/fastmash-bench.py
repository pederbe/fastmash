#!/usr/bin/env python3
"""Compare Fastmash with GNU datamash on your own machine.

Runs a subset of the jobs from https://fastmash.io/benchmarks/ with both
programs, checks that they print the same output, and prints a Markdown table
of median elapsed times you can paste into an issue or discussion.

usage:
  fastmash-bench.py [--runs N] [--data DIR] [--locale LOCALE]
                    [--datamash PATH] [--fastmash PATH] [--no-hyperfine]
                    [--job NAME]... [--list]
  fastmash-bench.py --custom 'ARGS' --input FILE [options]

The public datasets (UCSC RefGene hg19, about 8 MB compressed, and the UCI
Wine Quality data, about 350 kB) are downloaded into --data on first use;
the synthetic inputs are generated there. Needs Python 3.8+ and nothing else.
If hyperfine is installed it does the timing; otherwise each program runs
--runs times after one warm-up, alternating the two, and the median is kept.
"""

import argparse
import gzip
import hashlib
import os
import platform
import shlex
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path

REFGENE_URL = 'https://hgdownload.soe.ucsc.edu/goldenPath/hg19/database/refGene.txt.gz'
WINE_URL = 'https://archive.ics.uci.edu/ml/machine-learning-databases/wine-quality/'
# The published results used this snapshot. UCSC updates the table, so a
# different hash only means your numbers use slightly different data.
REFGENE_SHA256 = '1c361d4ed4566c6f05a097c6c4a9fd0e67c72ef07ad018dff7cbcaa761209a02'
MAX_DOWNLOAD = 64 * 1024 * 1024

# name, input file, arguments (as on the benchmarks page)
JOBS = [
    ('refgene-quantiles', 'refGene.txt', 'q1 9 median 9 q3 9 iqr 9'),
    ('refgene-transcripts', 'refGene.txt', '-s -g 13 count 2 collapse 2'),
    ('refgene-exons', 'refGene.txt', '-s -g 13 count 9 min 9 max 9 mean 9 median 9'),
    ('decimal-1000000', 'synthetic-1000000.tsv', '-H sum 3 mean 3'),
    ('decimal-100000', 'synthetic-100000.tsv', '-H sum 3 mean 3'),
    ('grouped-decimal-100000', 'synthetic-100000.tsv', '-H -s -g 2 count 3 mean 3 median 3'),
    ('wine-by-quality', 'winequality-white.csv', "-t ';' -H -s -g 12 count 1 mean 11 median 11"),
    ('wine-white-summary', 'winequality-white.csv', "-t ';' -H mean 9 median 9 min 9 max 9"),
]


def say(*args):
    print(*args, file=sys.stderr, flush=True)


def download(url, path):
    say(f'downloading {url}')
    with urllib.request.urlopen(url, timeout=60) as response:
        data = response.read(MAX_DOWNLOAD + 1)
    if len(data) > MAX_DOWNLOAD:
        sys.exit(f'unexpectedly large download: {url}')
    tmp = path.with_suffix(path.suffix + '.part')
    tmp.write_bytes(data)
    tmp.replace(path)


def sha256(path):
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1 << 20), b''):
            digest.update(block)
    return digest.hexdigest()


def synthetic(path, count, groups):
    """The generator from the Fastmash repository (data/workloads/prepare.py)."""
    tmp = path.with_suffix('.part')
    with tmp.open('w', encoding='ascii', newline='\n') as stream:
        stream.write('sample\tgroup\tsignal\tfactor\n')
        for index in range(count):
            mixed = ((index + 20260921) * 1103515245 + 12345) & 0x7fffffff
            group = (index * 4051) % groups
            value = mixed % 2000001 - 1000000
            signal = ('-' if value < 0 else '') + f'{abs(value) // 1000}.{abs(value) % 1000:03d}'
            stream.write(f'{index}\tg{group:05d}\t{signal}\t{mixed % 10000}e-3\n')
    tmp.replace(path)


def prepare(data, needed):
    data.mkdir(parents=True, exist_ok=True)
    notes = []
    if 'refGene.txt' in needed and not (data / 'refGene.txt').exists():
        gz = data / 'refGene.txt.gz'
        if not gz.exists():
            download(REFGENE_URL, gz)
        with gzip.open(gz, 'rb') as src, (data / 'refGene.txt.part').open('wb') as dst:
            shutil.copyfileobj(src, dst)
        (data / 'refGene.txt.part').replace(data / 'refGene.txt')
    if 'refGene.txt' in needed and sha256(data / 'refGene.txt') != REFGENE_SHA256:
        notes.append('RefGene differs from the snapshot used for the published results '
                     '(UCSC has updated it); compare the two programs, not with our table.')
    if 'winequality-white.csv' in needed and not (data / 'winequality-white.csv').exists():
        download(WINE_URL + 'winequality-white.csv', data / 'winequality-white.csv')
    for count, groups in ((100000, 4096), (1000000, 65536)):
        name = f'synthetic-{count}.tsv'
        if name in needed and not (data / name).exists():
            say(f'generating {name}')
            synthetic(data / name, count, groups)
    return notes


def run_once(program, args, path, env, stdout):
    with path.open('rb') as stdin:
        start = time.perf_counter()
        proc = subprocess.run([program] + args, stdin=stdin, stdout=stdout,
                              stderr=subprocess.PIPE, env=env)
        elapsed = time.perf_counter() - start
    return proc, elapsed


def check_same(programs, args, path, env):
    outputs = []
    for program in programs:
        proc, _ = run_once(program, args, path, env, subprocess.PIPE)
        if proc.returncode != 0:
            message = proc.stderr.decode(errors='replace').strip().splitlines()
            detail = message[0].replace('|', '/') if message else 'no message'
            return f'{Path(program).name} failed (exit {proc.returncode}): {detail}'
        outputs.append(proc.stdout)
    if outputs[0] != outputs[1]:
        return 'outputs differ'
    return None


def time_builtin(programs, args, path, env, runs):
    times = {p: [] for p in programs}
    for p in programs:
        run_once(p, args, path, env, subprocess.DEVNULL)
    for i in range(runs):
        order = programs if i % 2 == 0 else programs[::-1]
        for p in order:
            _, elapsed = run_once(p, args, path, env, subprocess.DEVNULL)
            times[p].append(elapsed * 1000)
    return {p: statistics.median(t) for p, t in times.items()}


def time_hyperfine(hyperfine, programs, args, path, env, runs):
    import json
    commands = [' '.join(shlex.quote(a) for a in [p] + args) + ' < ' + shlex.quote(str(path))
                for p in programs]
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp) / 'result.json'
        subprocess.run([hyperfine, '--warmup', '1', '--runs', str(runs), '--style', 'none',
                        '--output', 'null', '--export-json', str(out)] + commands,
                       env=env, check=True, stdout=subprocess.DEVNULL)
        results = json.loads(out.read_text())['results']
    return {p: r['median'] * 1000 for p, r in zip(programs, results)}


def version(program):
    try:
        out = subprocess.run([program, '--version'], capture_output=True, text=True).stdout
        return out.splitlines()[0] if out else '?'
    except OSError:
        return 'not found'


def cpu_model():
    try:
        for line in open('/proc/cpuinfo'):
            if line.startswith('model name'):
                return line.split(':', 1)[1].strip()
    except OSError:
        pass
    return platform.processor() or '?'


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    parser.add_argument('--runs', type=int, default=10, help='timed runs per program (default 10)')
    parser.add_argument('--data', type=Path, default=Path('fastmash-bench-data'),
                        help='where to keep the downloaded and generated inputs')
    parser.add_argument('--locale', default='C', help='LC_ALL for both programs (default C)')
    parser.add_argument('--datamash', default='datamash')
    parser.add_argument('--fastmash', default='fastmash')
    parser.add_argument('--no-hyperfine', action='store_true', help='use the built-in timer')
    parser.add_argument('--job', action='append', help='run only this job (repeatable)')
    parser.add_argument('--list', action='store_true', help='list the jobs and exit')
    parser.add_argument('--custom', help='your own arguments, for example "-s -g 1 median 2"')
    parser.add_argument('--input', type=Path, help='input file for --custom')
    opts = parser.parse_args()

    if opts.list:
        for name, file, args in JOBS:
            print(f'{name:24} {args}  < {file}')
        return
    if opts.runs < 1:
        sys.exit('--runs must be at least 1')
    if opts.custom:
        if not opts.input or not opts.input.is_file():
            sys.exit('--custom needs --input FILE')
        jobs = [('custom', opts.input, opts.custom)]
        notes = []
    else:
        selected = [j for j in JOBS if not opts.job or j[0] in opts.job]
        unknown = set(opts.job or []) - {j[0] for j in JOBS}
        if unknown:
            sys.exit('unknown job: ' + ', '.join(sorted(unknown)) + ' (see --list)')
    programs = [shutil.which(opts.datamash), shutil.which(opts.fastmash)]
    for want, found in zip((opts.datamash, opts.fastmash), programs):
        if not found:
            sys.exit(f'{want}: not found on PATH (use --datamash/--fastmash)')
    if not opts.custom:
        notes = prepare(opts.data, {j[1] for j in selected})
        jobs = [(name, opts.data / file, args) for name, file, args in selected]

    env = dict(os.environ, LC_ALL=opts.locale)
    hyperfine = None if opts.no_hyperfine else shutil.which('hyperfine')
    rows = []
    for name, path, args in jobs:
        argv = shlex.split(args)
        say(f'{name}: checking output')
        problem = check_same(programs, argv, path, env)
        if problem:
            rows.append((name, args, None, None, problem))
            continue
        say(f'{name}: timing {opts.runs} runs each')
        if hyperfine:
            medians = time_hyperfine(hyperfine, programs, argv, path, env, opts.runs)
        else:
            medians = time_builtin(programs, argv, path, env, opts.runs)
        rows.append((name, args, medians[programs[0]], medians[programs[1]], None))

    print(f'Fastmash benchmark kit: {version(programs[1])} vs {version(programs[0])}')
    print(f'CPU: {cpu_model()}; {platform.system()} {platform.release()}; '
          f'LC_ALL={opts.locale}; median of {opts.runs} runs '
          f'({"hyperfine" if hyperfine else "built-in timer"})')
    print()
    print('| Job | Arguments | datamash (ms) | fastmash (ms) | datamash / fastmash |')
    print('| --- | --- | ---: | ---: | ---: |')
    for name, args, gnu, fast, problem in rows:
        if problem:
            print(f'| {name} | `{args}` | | | {problem} |')
        else:
            print(f'| {name} | `{args}` | {gnu:.1f} | {fast:.1f} | {gnu / fast:.2f}x |')
    for note in notes:
        print(f'\nNote: {note}')
    if any(problem for *_, problem in rows):
        sys.exit(1)


if __name__ == '__main__':
    main()
