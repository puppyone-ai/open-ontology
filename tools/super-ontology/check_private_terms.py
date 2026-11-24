#!/usr/bin/env python3
"""Private-term scanner (Super Ontology, card B-S1-08).

The term list is never stored in the repository. It is read from the file named
by $OO_PRIVATE_TERMS_FILE, or ~/.config/open-ontology/private-terms.txt.
One term per line; blank lines and lines starting with '#' are ignored.
Matching is case-insensitive substring matching.

Output: only the number of hits and `path:line` locations. A term is never
printed. Exit codes: 0 no hits, 1 hits, 2 usage/config error.
"""
import os
import sys
from pathlib import Path

ENV_VAR = "OO_PRIVATE_TERMS_FILE"
DEFAULT_PATH = Path("~/.config/open-ontology/private-terms.txt")
SKIP_DIRS = {".git", "target", "node_modules", "__pycache__"}


def terms_path():
    env = os.environ.get(ENV_VAR)
    return Path(env).expanduser() if env else DEFAULT_PATH.expanduser()


def load_terms(path):
    text = Path(path).read_text(encoding="utf-8")
    return [t.strip().lower() for t in text.splitlines() if t.strip() and not t.lstrip().startswith("#")]


def iter_files(roots):
    for root in roots:
        p = Path(root)
        if p.is_file():
            yield p
            continue
        for dirpath, dirnames, filenames in os.walk(p):
            dirnames[:] = sorted(d for d in dirnames if d not in SKIP_DIRS)
            for name in sorted(filenames):
                yield Path(dirpath) / name


def scan_file(path, terms):
    """Return line numbers (1-based) with a hit. Binary files are scanned as
    raw bytes (terms encoded as UTF-8) and reported as line 0 if hit."""
    try:
        data = path.read_bytes()
    except OSError:
        return []
    hits = []
    lowered = data.lower()
    enc = [t.encode("utf-8") for t in terms]
    if b"\0" in data[:8192]:
        # binary (zip/pdf): also look inside nothing else; raw match only
        return [0] if any(t in lowered for t in enc) else []
    for i, line in enumerate(lowered.split(b"\n"), 1):
        if any(t in line for t in enc):
            hits.append(i)
    return hits


def scan(roots, terms):
    results = []
    for f in iter_files(roots):
        for ln in scan_file(f, terms):
            results.append((str(f), ln))
    return results


def main(argv=None):
    argv = sys.argv[1:] if argv is None else argv
    if not argv:
        print("usage: check_private_terms.py PATH [PATH...]", file=sys.stderr)
        return 2
    tp = terms_path()
    if not tp.is_file():
        print(f"private-terms file not found (set {ENV_VAR})", file=sys.stderr)
        return 2
    terms = load_terms(tp)
    if not terms:
        print("private-terms file is empty", file=sys.stderr)
        return 2
    hits = scan(argv, terms)
    for path, ln in hits:
        print(f"{path}:{ln}")
    print(f"private-term hits: {len(hits)}")
    return 1 if hits else 0


if __name__ == "__main__":
    sys.exit(main())
