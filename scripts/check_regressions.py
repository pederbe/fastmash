#!/usr/bin/env python3
"""Run the release regression cases against any fastmash build.

This does not build, pin or qualify the binary: it checks behavior only, so
contributors and CI can run it on supported Linux and native macOS hosts.
It uses the retained invocation and comparison (regression_cases.py). By default it runs the
newest fixture, tests/cli/cases-v3.jsonl.gz; --fixture selects another.
The cases give their input through a pipe; --stdin-file gives it as a regular
file instead, to the cases whose input is ordinary, with the same expectations,
so that the routes only input from a file takes (hash grouping) are checked too.
For a development build, --expected-version verifies its explicit CLI version
in the documented successful version cases; all other frozen expectations
remain unchanged, including version-option errors and transport failures.

Requirements: Python 3.10+ and an unblocked signal mask. Linux needs /usr/bin/sort
and fastmash-sort-supervisor beside the binary. macOS uses the native sorter and
requires the calibrated Rust fault helper named by FASTMASH_TEST_FULL_OUTPUT_LIBRARY
for full-output cases. Frozen expectations remain unchanged on disk; the explicit
native diagnostic and observation dispositions are recorded by --evidence-dir.
Run it serially: concurrent
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
FIXTURE = ROOT / 'tests/cli/cases-v3.jsonl.gz'

# These cases print the successful version action, including option precedence.
# Required-value, sentinel and transport-failure cases retain frozen outcomes.
VERSION_CASES = frozenset({
    'version', 'version-locale', 'version-first', 'version-before-unknown',
    'output-header-reference:version-precedence', 'header-aliases:alias-then-version',
    'named-fields:version', 'whitespace:version', 'skip-comments:version',
    'narm:version', 'count:version',
})

# These two frozen Linux diagnostics come from its external sort process.
# The native route diagnoses the same missing header itself, with status 1.
NATIVE_SORT_DIAGNOSTICS = frozenset({
    'sorting/header-empty', 'sorting/header-only-comments',
})

# Only these held pipe observations cross the native output buffer boundary.
# All completed output, status, terminal line buffering and EOF behavior retain
# their frozen expectations. Derive the prefix from fixture bytes and fstat facts,
# never from the candidate's observed output.
NATIVE_BUFFER_OBSERVATIONS = frozenset({
    'grouping-reference-r2:held-flush-pipe',
    'grouping-reference-r2:held-flush-pipe-header',
    'output-header-lifecycle:held-8192',
    'header-writer-reference:pipe-large',
})


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


def expected(case, expected_version=None, *, platform=None, destination=None):
    """The case's expectation, with file-backed output resolved to bytes."""
    result = dict(case['expected'])
    if expected_version is not None and case['id'] in VERSION_CASES:
        result['stdout_hex'] = f'fastmash {expected_version}\n'.encode().hex()
    if 'stdout_file' in result:
        result['stdout_hex'] = (ROOT / result.pop('stdout_file')).read_bytes().hex()
    if (platform or sys.platform) == 'darwin':
        if case['id'] in NATIVE_SORT_DIAGNOSTICS:
            result['stderr_hex'] = b'fastmash: missing input header for named grouping key\n'.hex()
        if case['id'] in NATIVE_BUFFER_OBSERVATIONS and destination is not None:
            size = destination.get('block_size')
            if destination.get('isatty') is not False or type(size) is not int or size < 0:
                raise ValueError('Native held output requires observed nonterminal fstat facts')
            capacity = size if 0 < size < 8192 else 8192
            output = bytes.fromhex(result['stdout_hex'])
            if case['id'].startswith('grouping-reference-r2:'):
                # The last group is pending until input EOF; earlier groups
                # and any output header have already reached the buffer.
                pending = output.rsplit(b'\n', 2)[-2] + b'\n'
                before_eof = len(output) - len(pending)
            else:
                # Only the input header is supplied; the output header is
                # available before EOF, with no numerical result to follow.
                before_eof = len(output)
            size = before_eof // capacity * capacity
            held = dict(result['held'], bytes=size)
            if 'stdout_hex' in held:
                held['stdout_hex'] = output[:size].hex()
            result['held'] = held
    return result


def matches(case, observed, expected_version=None):
    return regression_cases.matches(dict(case, expected=expected(case, expected_version,
        destination=observed.get('destination'))), observed)


def native_dispositions(case):
    """Per-case mechanisms or native expectations, without dropping a case."""
    result = []
    if case['id'] in NATIVE_SORT_DIAGNOSTICS:
        result.append('native missing-header diagnostic replaces Linux system-sort stderr')
    if case['id'] in NATIVE_BUFFER_OBSERVATIONS:
        result.append('held prefix uses native destination block size; completed stdout and status frozen')
    if case['io'] == 'full':
        result.append('calibrated native write ENOSPC replaces Linux /dev/full')
    if case['io'] == 'read-error':
        result.append('native late input EIO transport; completed stdout and status frozen')
    return result


invoke = regression_cases.invoke


def file_input(case):
    """Whether the case's input can come from a regular file instead: ordinary
    input and output, and no input held open."""
    return (case['io'] == 'normal' and not case.get('observe_held')
            and not case.get('hold_stdin'))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True,
                        help='fastmash executable; Linux requires its sibling sort supervisor')
    parser.add_argument('--fixture', type=Path, default=FIXTURE)
    parser.add_argument('--match', default='', help='only run cases whose id contains this text')
    parser.add_argument('--keep-going', action='store_true',
                        help='run every case instead of stopping at the first mismatch')
    parser.add_argument('--expected-version',
                        help='verify this explicit version in successful version cases only; '
                             'default: the frozen fixture expectation')
    parser.add_argument('--stdin-file', action='store_true',
                        help='give ordinary input as a regular file instead of a pipe; '
                             'cases with other input transports are left out')
    parser.add_argument('--evidence-dir', type=Path,
                        help='retain identities, per-case dispositions and failure observations here')
    args = parser.parse_args()
    binary = args.binary.resolve()
    if not binary.is_file():
        parser.exit(1, f'{binary} is required\n')
    if sys.platform == 'linux' and not binary.with_name('fastmash-sort-supervisor').is_file():
        parser.exit(1, f'{binary} requires fastmash-sort-supervisor beside it on Linux\n')
    if sys.platform not in ('linux', 'darwin'):
        parser.exit(1, f'Unsupported regression test platform: {sys.platform}\n')
    if args.evidence_dir and args.evidence_dir.exists() and any(args.evidence_dir.iterdir()):
        parser.exit(1, f'{args.evidence_dir} must be empty so observations retain one run identity\n')
    cases = [case for case in load_fixture(args.fixture) if args.match in case['id']]
    excluded = []
    if args.stdin_file:
        excluded = [{'id': case['id'], 'reason': 'non-regular input transport; covered by the piped corpus run'}
                    for case in cases if not file_input(case)]
        cases = [dict(case, io='file') for case in cases if file_input(case)]
    calibration = None
    if sys.platform == 'darwin' and any(case['io'] == 'full' for case in cases):
        regression_cases.cli_streams.native_full_environment(binary, {'LC_ALL': 'C'})
        calibration = dict(regression_cases.cli_streams.native_fault_identity(),
                           mechanism='native-dyld-write-ENOSPC', candidate_calibrated=True)
    evidence = {'platform': sys.platform, 'binary': str(binary),
                'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
                'fixture': str(args.fixture.resolve()),
                'fixture_sha256': hashlib.sha256(args.fixture.read_bytes()).hexdigest(),
                'expected_version': args.expected_version, 'stdin_file': args.stdin_file,
                'full_output_calibration': calibration, 'selected_cases': len(cases),
                'transport_dispositions': excluded,
                'native_dispositions': [{'id': case['id'], 'reasons': native_dispositions(case)}
                                        for case in cases if sys.platform == 'darwin'
                                        and native_dispositions(case)]}
    if args.evidence_dir:
        args.evidence_dir.mkdir(parents=True, exist_ok=True)
        (args.evidence_dir / 'identity.json').write_text(json.dumps(evidence, indent=2) + '\n')
    failed = []
    attempted = 0
    with tempfile.TemporaryDirectory(prefix='fastmash-regressions-') as directory:
        for case in cases:
            attempted += 1
            # Diagnostics print argv[0]; the v2 expectations use fastmash.
            observed = invoke({'name': 'fastmash', **case}, binary, Path(directory))
            passed = matches(case, observed, args.expected_version)
            if args.evidence_dir and ((sys.platform == 'darwin' and native_dispositions(case))
                                      or not passed):
                with (args.evidence_dir / 'observations.jsonl').open('a') as output:
                    output.write(json.dumps({'id': case['id'], 'passed': passed, 'observed': observed,
                        'expected': expected(case, args.expected_version,
                                             destination=observed.get('destination'))}) + '\n')
            if passed:
                continue
            failed.append(case["id"])
            # A supervisor can outlive a failed invocation; each owns a group.
            try:
                os.killpg(observed["process_group"], signal.SIGKILL)
            except (KeyError, ProcessLookupError):
                pass
            print(f"FAIL {case['id']}: {' '.join(observed['argv'][1:])}")
            for key, value in expected(case, args.expected_version,
                                       destination=observed.get('destination')).items():
                if observed.get(key) != value:
                    print(f'  {key}: expected {value!r}, observed {observed.get(key)!r}')
            if not args.keep_going:
                break
    print(f"{attempted - len(failed)} of {len(cases)} cases passed"
          + ("" if attempted == len(cases) else f"; stopped after {attempted}"))
    if excluded:
        print(f'{len(excluded)} transport cases apply to the piped corpus run only')
    if args.evidence_dir:
        (args.evidence_dir / 'result.json').write_text(json.dumps({
            'attempted': attempted, 'passed': attempted - len(failed),
            'failed': failed, 'selected_cases': len(cases)}, indent=2) + '\n')
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main())
