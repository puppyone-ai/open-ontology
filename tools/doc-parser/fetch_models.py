#!/usr/bin/env python3
"""Build-time model fetch for the sidecar image (10 §2.6).

    fetch_models.py --dest /opt/models --lock models.lock.json --mode verify|write

Downloads each pinned source (Hugging Face repo at a full commit sha, only the
allowed files) into --dest, then either records every file's sha256 in the lock
(`write`) or requires them to equal the lock (`verify`, the image build).
Network is needed here and nowhere else; the runtime image is offline.
Layout, TableFormer and (B-S9-04) the RapidOCR PP-OCRv5 det/rec ONNX files are fetched; `kind: url`
sources are verified against their pinned sha256 while downloading. No picture, code/formula or VLM models.
"""
import argparse
import hashlib
import json
import shutil
import sys
import urllib.request
from pathlib import Path


def sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dest", required=True)
    ap.add_argument("--lock", required=True)
    ap.add_argument("--mode", choices=["verify", "write"], default="verify")
    ap.add_argument("--local-dir", default=".", help="where `kind: local` artifacts are")
    a = ap.parse_args()
    lock_path = Path(a.lock)
    lock = json.loads(lock_path.read_text())
    dest = Path(a.dest)
    from huggingface_hub import snapshot_download

    files = []
    for src in lock["sources"]:
        if src.get("kind") == "url":
            target = dest / src["local_dir"]
            target.mkdir(parents=True, exist_ok=True)
            for f in src["files"]:
                data = urllib.request.urlopen(f["url"], timeout=300).read()
                if hashlib.sha256(data).hexdigest() != f["sha256"]:
                    sys.exit(f"{f['url']}: sha256 differs from the lock")
                (target / f["local"]).write_bytes(data)
            for p in sorted(target.rglob("*")):
                if p.is_file():
                    files.append({"name": f"{src['local_dir']}/{p.relative_to(target).as_posix()}",
                                  "sha256": sha256_file(p), "size": p.stat().st_size})
            continue
        if src.get("kind") == "wheel-member":
            import io
            import zipfile

            target = dest / src["local_dir"]
            target.mkdir(parents=True, exist_ok=True)
            data = urllib.request.urlopen(src["url"], timeout=300).read()
            if hashlib.sha256(data).hexdigest() != src["wheel_sha256"]:
                sys.exit(f"{src['url']}: wheel sha256 differs from the lock")
            z = zipfile.ZipFile(io.BytesIO(data))
            for m in src["members"]:
                blob = z.read(m["member"])
                if hashlib.sha256(blob).hexdigest() != m["sha256"]:
                    sys.exit(f"{m['member']}: sha256 differs from the lock")
                (target / m["local"]).write_bytes(blob)
            for p in sorted(target.rglob("*")):
                if p.is_file():
                    files.append({"name": f"{src['local_dir']}/{p.relative_to(target).as_posix()}",
                                  "sha256": sha256_file(p), "size": p.stat().st_size})
            continue
        if src.get("kind") == "local":
            # An artifact we build ourselves (see `rebuild` in the source) and place next to the Dockerfile.
            target = dest / src["local_dir"]
            target.mkdir(parents=True, exist_ok=True)
            for f in src["files"]:
                data = (Path(a.local_dir) / f["local"]).read_bytes()
                if hashlib.sha256(data).hexdigest() != f["sha256"]:
                    sys.exit(f"{f['local']}: sha256 differs from the lock")
                (target / f["local"]).write_bytes(data)
            for p in sorted(target.rglob("*")):
                if p.is_file():
                    files.append({"name": f"{src['local_dir']}/{p.relative_to(target).as_posix()}",
                                  "sha256": sha256_file(p), "size": p.stat().st_size})
            continue
        rev = src["revision"]
        if len(rev) != 40 or any(c not in "0123456789abcdef" for c in rev):
            sys.exit(f"{src['repo_id']}: revision must be a full commit sha")
        target = dest / src["local_dir"]
        snapshot_download(repo_id=src["repo_id"], revision=rev, local_dir=str(target),
                          allow_patterns=src.get("allow_patterns"))
        shutil.rmtree(target / ".cache", ignore_errors=True)
        for p in sorted(target.rglob("*")):
            if p.is_file():
                files.append({"name": f"{src['local_dir']}/{p.relative_to(target).as_posix()}",
                              "sha256": sha256_file(p), "size": p.stat().st_size})
    if a.mode == "write":
        lock["files"] = files
        lock_path.write_text(json.dumps(lock, indent=2, ensure_ascii=False) + "\n")
        print(f"wrote {len(files)} files to {lock_path}")
        return
    want = {f["name"]: f["sha256"] for f in lock.get("files", [])}
    got = {f["name"]: f["sha256"] for f in files}
    if want != got:
        bad = sorted(set(want) ^ set(got)) + [n for n in want if n in got and want[n] != got[n]]
        sys.exit(f"model files differ from models.lock.json: {bad[:10]}")
    print(f"verified {len(got)} model files")


if __name__ == "__main__":
    main()
