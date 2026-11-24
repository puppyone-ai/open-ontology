#!/usr/bin/env python3
"""Digest summary of a docs-images.yml run (SO-F-S23-30). Standard library only.

    release_summary.py --merged DIR --lock deploy/docs/images.lock.json --version 0.2.1 --revision <sha> --out DIR

DIR holds the merged-<image>.json files the merge jobs uploaded. Writes, into --out:
  digests.json       one entry per image: repository, version, index digest, per-architecture digests
  digests.md         the same as a table (appended to the run summary)
  images.lock.json   a copy of the lock with a `registry` section; the owner reviews it and commits it
Nothing is pushed and no pull request is opened: a pull request made with the workflow token starts no checks.
"""
import argparse
import json
import re
import sys
from pathlib import Path

DIGEST = re.compile(r"sha256:[0-9a-f]{64}")


def load(merged):
    entries = {}
    for path in sorted(Path(merged).rglob("merged-*.json")):
        e = json.loads(path.read_text())
        for key in ("index", "linux/amd64", "linux/arm64"):
            if not DIGEST.fullmatch(e.get(key, "")):
                sys.exit(f"{path}: {key} is not a sha256 digest")
        if e["image"] in entries:
            sys.exit(f"{path}: duplicate image {e['image']}")
        entries[e["image"]] = e
    if not entries:
        sys.exit("no merged-*.json found")
    return entries


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--merged", required=True)
    ap.add_argument("--lock", required=True)
    ap.add_argument("--version", required=True)
    ap.add_argument("--revision", required=True)
    ap.add_argument("--out", required=True)
    a = ap.parse_args()
    entries = load(a.merged)
    for e in entries.values():
        if e["version"] != a.version:
            sys.exit(f"{e['image']}: version {e['version']} differs from {a.version}")
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    release = {"protocol": "oo.docs-images-release/v1", "version": a.version, "revision": a.revision,
               "images": {n: {k: e[k] for k in ("repository", "index", "linux/amd64", "linux/arm64")}
                          for n, e in sorted(entries.items())}}
    (out / "digests.json").write_text(json.dumps(release, indent=2) + "\n")
    lock = json.loads(Path(a.lock).read_text())
    lock["registry"] = release
    (out / "images.lock.json").write_text(json.dumps(lock, indent=2, ensure_ascii=False) + "\n")
    rows = ["| image | index digest | linux/amd64 | linux/arm64 |", "|---|---|---|---|"]
    for n, e in sorted(entries.items()):
        rows.append(f"| `{e['repository']}:{a.version}` | `{e['index']}` | `{e['linux/amd64']}` | `{e['linux/arm64']}` |")
    (out / "digests.md").write_text(f"### Docs runtime images {a.version} ({a.revision})\n\n" + "\n".join(rows) + "\n")
    print(f"{len(entries)} images")


if __name__ == "__main__":
    main()
