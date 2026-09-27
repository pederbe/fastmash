#!/usr/bin/env python3
"""Run the release regression cases against any fastmash build.

Unlike check_cli_regressions.py, this does not build, pin or admit the binary:
it checks behavior only, so contributors and CI can run it on any Linux host.
It uses the retained invocation and comparison (regression_cases.py). By default it runs the
newest fixture, tests/cli/cases-v2.jsonl.gz; --fixture selects another.

Requirements: Linux, Python 3.10+, /usr/bin/sort, an unblocked signal mask,
and fastmash-sort-supervisor beside the binary. Run it serially: concurrent
runs can make external-sort cases exceed their 8-second limit.
"""
import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path
import signal
import sys
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
import regression_cases  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / 'tests/cli/cases-v2.jsonl.gz'


def load_fixture(path):
    """Cases from a fixture, verified against its provenance record."""
    path = Path(path).resolve()
    provenance = json.loads(path.with_name(path.name.replace('.jsonl.gz', '-provenance.json')).read_text())
    compressed = path.read_bytes()
    data = gzip.decompress(compressed)
    if (hashlib.sha256(compressed).hexdigest() != provenance['fixture_sha256']
            or hashlib.sha256(data).hexdigest() != provenance['content_sha256']):
        raise ValueError(f'{path} differs from its provenance')
    cases = [json.loads(line) for line in data.splitlines()]
    if len(cases) != provenance['cases'] or len({case['id'] for case in cases}) != len(cases):
        raise ValueError(f'{path} has an incomplete or duplicated inventory')
    return cases


def expected(case):
    """The case's expectation, with file-backed output resolved to bytes."""
    result = dict(case['expected'])
    if 'stdout_file' in result:
        result['stdout_hex'] = (ROOT / result.pop('stdout_file')).read_bytes().hex()
    return result


def matches(case, observed):
    return regression_cases.matches(dict(case, expected=expected(case)), observed)


invoke = regression_cases.invoke


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True,
                        help='fastmash executable; fastmash-sort-supervisor must be beside it')
    parser.add_argument('--fixture', type=Path, default=FIXTURE)
    parser.add_argument('--match', default='', help='only run cases whose id contains this text')
    parser.add_argument('--keep-going', action='store_true',
                        help='run every case instead of stopping at the first mismatch')
    args = parser.parse_args()
    binary = args.binary.resolve()
    if not binary.is_file() or not binary.with_name('fastmash-sort-supervisor').is_file():
        parser.exit(1, f'{binary} and fastmash-sort-supervisor beside it are required\n')
    cases = [case for case in load_fixture(args.fixture) if args.match in case['id']]
    failed = []
    attempted = 0
    with tempfile.TemporaryDirectory(prefix='fastmash-regressions-') as directory:
        for case in cases:
            attempted += 1
            # Diagnostics print argv[0]; the v2 expectations use fastmash.
            observed = invoke({'name': 'fastmash', **case}, binary, Path(directory))
            if matches(case, observed):
                continue
            failed.append(case["id"])
            # A supervisor can outlive a failed invocation; each owns a group.
            try:
                os.killpg(observed["process_group"], signal.SIGKILL)
            except (KeyError, ProcessLookupError):
                pass
            print(f"FAIL {case['id']}: {' '.join(observed['argv'][1:])}")
            for key, value in expected(case).items():
                if observed.get(key) != value:
                    print(f'  {key}: expected {value!r}, observed {observed.get(key)!r}')
            if not args.keep_going:
                break
    print(f"{attempted - len(failed)} of {len(cases)} cases passed"
          + ("" if attempted == len(cases) else f"; stopped after {attempted}"))
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main())
