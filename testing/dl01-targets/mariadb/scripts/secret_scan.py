#!/usr/bin/env python3
"""Exit 1 when a file contains a generated secret. Prints only a count."""

import pathlib
import sys


def secrets(directory: pathlib.Path) -> list[bytes]:
    found = []
    for path in sorted(directory.iterdir()):
        if path.suffix != ".pw":
            continue
        text = path.read_text(encoding="utf-8").strip()
        if text:
            found.append(text.encode("utf-8"))
    return found


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: secret_scan.py SECRET_DIR FILE", file=sys.stderr)
        return 2
    directory = pathlib.Path(sys.argv[1])
    target = pathlib.Path(sys.argv[2])
    data = target.read_bytes()
    hits = 0
    for secret in secrets(directory):
        hits += data.count(secret)
    print(f"secret_hits={hits} file={target.name}")
    return 1 if hits else 0


if __name__ == "__main__":
    sys.exit(main())
