#!/usr/bin/env python3
"""Exit 1 when a file contains a plaintext secret. Prints a count, never the secret."""

import pathlib
import sys


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: secret_scan.py SECRET_DIR FILE", file=sys.stderr)
        return 2
    directory = pathlib.Path(sys.argv[1])
    target = pathlib.Path(sys.argv[2])
    data = target.read_bytes()
    hits = 0
    for path in sorted(directory.iterdir()):
        if path.suffix != ".pw":
            continue
        secret = path.read_text(encoding="utf-8").strip().encode("utf-8")
        if secret:
            hits += data.count(secret)
    print(f"secret_hits={hits} file={target.name}")
    return 1 if hits else 0


if __name__ == "__main__":
    raise SystemExit(main())
