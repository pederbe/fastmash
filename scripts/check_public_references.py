"""Check tracked public files for private references before they ship."""
import argparse
import gzip
from pathlib import Path, PurePosixPath
import re
import subprocess

# (reason, pattern); every tracked file is checked, a gzip file decompressed.
PRIVATE = [
    ('host name', re.compile(r'cachyc[a]t', re.I)),
    ('personal path', re.compile(r'/home/' + r'peder|C:\\Users|/mnt/[c]/|Documents/C[o]dex', re.I)),
    ('email address', re.compile(r'(?<![\w.+-])[\w.+-]{1,64}@(?:gmail|outlook|hotmail)\.com', re.I)),
    ('lab repository', re.compile(r'fastmash' + r'-lab')),
    ('lab directory', re.compile(r'\.scr[a]tch/|\barchiv[e]s/|\beviden[c]e/|\bexperim[e]nts?/')),
    # Only the built-in locale directory belongs in public source.
    ('lab data path', re.compile(r'(?<![\w./-])d[a]ta/(?!locales\b)')),
    ('lab document', re.compile(r'(?<![\w$/])docs/(?!src/|book\b|book/)[\w./-]+\.md')),
]
# A delimiter immediately after a Rust format placeholder is fixture data.
# Keep scanning the rest of the line, including comments after that string.
ISSUE = re.compile(r'(?<![\w&/#"-])(?<!\{\})#[0-9]{1,3}\b')


def scan(out, names):
    hits = []
    for name in names:
        data = (out / name).read_bytes()
        if name.endswith('.gz'):
            data = gzip.decompress(data)
        text = data.decode('utf-8', errors='replace')
        for number, line in enumerate(text.splitlines(), 1):
            for reason, pattern in PRIVATE:
                if pattern.search(line):
                    hits.append(f'{name}:{number}: {reason}: {line.strip()[:120]}')
            code = PurePosixPath(name).suffix in ('.rs', '.py', '.sh', '.toml', '.tsv')
            if code and ISSUE.search(line):
                hits.append(f'{name}:{number}: private issue number: {line.strip()[:120]}')
    return hits


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()
    names = subprocess.check_output(["git", "-C", str(args.root), "ls-files", "-z"]).decode().split("\0")
    hits = scan(args.root, [name for name in names if name])
    for hit in hits:
        print(hit)
    print(f"{len(hits)} private references")
    return bool(hits)


if __name__ == "__main__":
    raise SystemExit(main())
