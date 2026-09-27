#!/usr/bin/env python3
"""Write the third-party license notices for the fastmash binaries.

Collects every crate the `fastmash` package links (normal dependencies on
x86_64 Linux) from `cargo metadata`, with each crate's license expression and
the license, copyright and notice files it ships. Works with vendored and
crates.io sources alike. Usage:

    python3 scripts/third_party_licenses.py > THIRD-PARTY-LICENSES.md
"""
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
PREFIXES = ('license', 'licence', 'copying', 'copyright', 'notice', 'unicode', 'authors')
MIT = """Permission is hereby granted, free of charge, to any person obtaining a copy of this
software and associated documentation files (the "Software"), to deal in the Software
without restriction, including without limitation the rights to use, copy, modify,
merge, publish, distribute, sublicense, and/or sell copies of the Software, and to
permit persons to whom the Software is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice shall be included in all copies
or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED,
INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT
HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF
CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE
OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
"""
RUST_STD = """The Rust standard library (std, core, alloc, and their dependencies) is
linked into the binaries under the MIT and Apache-2.0 licenses:
<https://github.com/rust-lang/rust/blob/master/COPYRIGHT>.
"""


def linked_packages():
    metadata = json.loads(subprocess.check_output(
        ['cargo', 'metadata', '--format-version', '1', '--locked',
         '--filter-platform', 'x86_64-unknown-linux-gnu'], cwd=ROOT))
    packages = {package['id']: package for package in metadata['packages']}
    nodes = {node['id']: node for node in metadata['resolve']['nodes']}
    root = next(p['id'] for p in metadata['packages']
                if p['name'] == 'fastmash' and p['source'] is None)
    linked, pending = set(), [root]
    while pending:
        current = pending.pop()
        if current in linked:
            continue
        linked.add(current)
        for dependency in nodes[current]['deps']:
            if any(kind['kind'] is None for kind in dependency['dep_kinds']):
                pending.append(dependency['pkg'])
    return sorted((packages[i] for i in linked if packages[i]['source'] is not None),
                  key=lambda p: (p['name'], p['version']))


def main():
    out = sys.stdout
    out.write('# Third-party licenses\n\nThe fastmash and fastmash-sort-supervisor binaries '
              'include the following third-party code.\n\n' + RUST_STD + '\n')
    for package in linked_packages():
        out.write(f"## {package['name']} {package['version']}\n\n"
                  f"License: {package['license']}\n\n")
        directory = Path(package['manifest_path']).parent
        files = sorted(path for path in directory.iterdir()
                       if path.is_file() and path.name.lower().startswith(PREFIXES))
        if not files:
            # The crate ships no license file; state its terms from its metadata.
            if package['license'] != 'MIT':
                raise SystemExit(f"{package['name']}: no license file found in {directory}")
            authors = ', '.join(package['authors']) or 'the authors'
            out.write(f"The crate ships no license file. Source: {package['repository']}\n\n"
                      f"```text\nCopyright (c) {authors}\n\n{MIT}```\n\n")
        for path in files:
            text = path.read_text(encoding='utf-8', errors='replace').strip()
            out.write(f'### {path.name}\n\n```text\n{text}\n```\n\n')


if __name__ == '__main__':
    main()
