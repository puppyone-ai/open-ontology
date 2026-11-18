#!/usr/bin/env python3
"""Super Ontology L1 cells `docs-rustfs` and `docs-minio` (B-S4-04, 10 section 12.3).

Each cell starts one labelled container (RustFS is the default S3 test store,
D9; MinIO is the compatibility store), makes a bucket, runs the real-store
tests of the S3 backend and the remote download against it, and removes the
container and its volume. Images are pinned by digest (below); nothing but
these tests talks to them.

  tools/oo-test/super_ontology_s3.py --cells docs-rustfs,docs-minio [--run-id ID]
      [--evidence-dir DIR] [--keep]

`tools/oo-test/oo-test fast --layer L1 --cells docs-rustfs,docs-minio --run-id ID`
calls this for cells that start with `docs-`.

Evidence: <evidence-dir>/s3-<cell>.txt (commands, tails, the store's version
line) and s3-cells.json. Credentials are throwaway values made here for the run
and never written to the evidence.

MinIO: the official `minio/minio` repository left Docker Hub (September 2026);
the cell runs the drop-in community fork `pgsty/minio` pinned by digest, which
keeps the same environment, command and on-disk format.
"""

import argparse
import datetime
import hashlib
import hmac
import json
import os
import re
import secrets
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
LANE = os.environ.get("OO_TEST_LANE", "super-ontology-be1")
BUCKET = "oo-test"

CELLS = {
    "docs-rustfs": {
        "image": "rustfs/rustfs@sha256:ee717072290bec9b11abfc0365f8310b00df1cd9b3639943edae47bdef05044a",
        "note": "rustfs/rustfs 1.0.6 (arm64 and amd64 index digest)",
        "env": lambda k, s: {"RUSTFS_ACCESS_KEY": k, "RUSTFS_SECRET_KEY": s},
        "args": [],
        "mounts": ["/data", "/logs"],
        "user": None,
    },
    "docs-minio": {
        "image": "pgsty/minio@sha256:b6bfe7239bfc83fb90d31612d9704d86039dd714f7904b3f1ad68f211e602372",
        "note": "pgsty/minio RELEASE.2026-08-04T00-00-00Z (community fork of MinIO)",
        "env": lambda k, s: {"MINIO_ROOT_USER": k, "MINIO_ROOT_PASSWORD": s},
        "args": ["server", "/data"],
        "mounts": ["/data"],
        "user": None,
    },
}


def sh(argv, check=True, timeout=600, env=None, cwd=None):
    r = subprocess.run(argv, capture_output=True, text=True, timeout=timeout, env=env, cwd=cwd)
    if check and r.returncode != 0:
        raise RuntimeError(f"{argv[:3]} failed: {r.stderr.strip()[-400:]}")
    return r


def hmac_sha(key, msg):
    return hmac.new(key, msg.encode(), hashlib.sha256).digest()


def make_bucket(endpoint, access, secret, bucket, region="us-east-1"):
    """PUT /<bucket> signed with SigV4 (stdlib only)."""
    host = endpoint.split("//", 1)[1]
    now = datetime.datetime.now(datetime.timezone.utc)
    amz = now.strftime("%Y%m%dT%H%M%SZ")
    day = now.strftime("%Y%m%d")
    payload = hashlib.sha256(b"").hexdigest()
    headers = {"host": host, "x-amz-content-sha256": payload, "x-amz-date": amz}
    signed = ";".join(sorted(headers))
    canon = "\n".join(["PUT", f"/{bucket}", "", *[f"{k}:{headers[k]}" for k in sorted(headers)], "", signed, payload])
    scope = f"{day}/{region}/s3/aws4_request"
    to_sign = "\n".join(["AWS4-HMAC-SHA256", amz, scope, hashlib.sha256(canon.encode()).hexdigest()])
    k = hmac_sha(("AWS4" + secret).encode(), day)
    for part in (region, "s3", "aws4_request"):
        k = hmac_sha(k, part)
    sig = hmac.new(k, to_sign.encode(), hashlib.sha256).hexdigest()
    auth = f"AWS4-HMAC-SHA256 Credential={access}/{scope}, SignedHeaders={signed}, Signature={sig}"
    req = urllib.request.Request(f"{endpoint}/{bucket}", method="PUT", data=b"",
                                 headers={**headers, "Authorization": auth})
    try:
        urllib.request.urlopen(req, timeout=15).read()
    except urllib.error.HTTPError as e:
        if e.code != 409:
            raise


def wait_ready(endpoint, seconds=60):
    end = time.time() + seconds
    while time.time() < end:
        try:
            urllib.request.urlopen(endpoint + "/", timeout=2).read()
            return True
        except urllib.error.HTTPError as e:
            if e.code < 500:
                return True  # 403 without a signature: the server is up
            time.sleep(0.5)
        except Exception:
            time.sleep(0.5)
    return False


def run_cell(name, run_id, evidence, keep):
    cell = CELLS[name]
    cname = f"oo-be1-{name}-{run_id}"[:60]
    vols = [f"{cname}-v{i}" for i, _ in enumerate(cell["mounts"])]
    access = "ootest" + secrets.token_hex(4)
    secret = secrets.token_urlsafe(24)
    log = []
    started = time.time()
    status = "failed"
    port = None
    try:
        for v in vols:
            sh(["docker", "volume", "create", "--label", f"oo.lane={LANE}", v])
        cmd = ["docker", "run", "-d", "--name", cname, "--label", f"oo.lane={LANE}", "-p", "127.0.0.1::9000"]
        for k, v in cell["env"](access, secret).items():
            cmd += ["-e", f"{k}={v}"]
        for v, m in zip(vols, cell["mounts"]):
            cmd += ["-v", f"{v}:{m}"]
        cmd += [cell["image"], *cell["args"]]
        sh(cmd)
        port = sh(["docker", "port", cname, "9000/tcp"]).stdout.strip().splitlines()[0].rsplit(":", 1)[1]
        endpoint = f"http://127.0.0.1:{port}"
        if not wait_ready(endpoint):
            raise RuntimeError("store did not come up: " + sh(["docker", "logs", "--tail", "20", cname], check=False).stderr[-300:])
        for attempt in range(40):
            try:
                make_bucket(endpoint, access, secret, BUCKET)
                break
            except urllib.error.HTTPError as e:
                if e.code < 500 or attempt == 39:
                    raise
                time.sleep(1)
        log.append(f"# cell {name}: {cell['note']}\n# image {cell['image']}\n# started {datetime.datetime.now().isoformat(timespec='seconds')}\n")
        env = dict(os.environ, OO_S3_ENDPOINT=endpoint, OO_S3_ACCESS=access, OO_S3_SECRET=secret,
                   OO_S3_BUCKET=BUCKET, CARGO_BUILD_JOBS=os.environ.get("CARGO_BUILD_JOBS", "6"))
        runs = [
            ["cargo", "test", "-p", "oo-connectors", "--features", "objectstore-s3", "--locked",
             "--test", "s3_rustfs", "--", "--ignored", "--test-threads=1", "--nocapture"],
            ["cargo", "test", "-p", "oo-docs", "--features", "objectstore-s3", "--locked",
             "--test", "download", "--", "--ignored", "--test-threads=1", "--nocapture"],
        ]
        ok = True
        for argv in runs:
            r = sh(argv, check=False, timeout=1500, env=env, cwd=REPO)
            out = (r.stdout + r.stderr)
            for secret_value in (access, secret):
                out = out.replace(secret_value, "<redacted>")
            out = out.replace(endpoint, "<endpoint>")
            lines = [l for l in out.splitlines() if re.search(r"^test |test result|PROBE|DOWNLOAD|etag|panicked|error", l)]
            log.append(f"$ {' '.join(argv)}\n" + "\n".join(lines) + f"\n(exit {r.returncode})\n")
            ok = ok and r.returncode == 0
        status = "passed" if ok else "failed"
    except Exception as e:  # keep the run's error in the evidence
        log.append(f"ERROR {e}\n")
    finally:
        if not keep:
            sh(["docker", "rm", "-f", cname], check=False)
            for v in vols:
                sh(["docker", "volume", "rm", "-f", v], check=False)
    (evidence / f"s3-{name}.txt").write_text("".join(log) + f"# status {status} in {time.time() - started:.0f}s\n")
    return {"cell": name, "status": status, "seconds": round(time.time() - started), "image": cell["image"]}


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--cells", default="docs-rustfs,docs-minio")
    ap.add_argument("--run-id", default=time.strftime("s3-%Y%m%d-%H%M%S"))
    ap.add_argument("--evidence-dir", default=str(REPO / "testing/super-ontology/evidence/L1"))
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args(argv)
    if not re.fullmatch(r"[a-z0-9][a-z0-9-]{0,40}", args.run_id):
        ap.error("invalid run id")
    names = [c for c in args.cells.split(",") if c]
    unknown = [c for c in names if c not in CELLS]
    if unknown:
        ap.error(f"unknown cells {unknown}")
    evidence = Path(args.evidence_dir)
    evidence.mkdir(parents=True, exist_ok=True)
    rows = [run_cell(n, args.run_id, evidence, args.keep) for n in names]
    (evidence / "s3-cells.json").write_text(json.dumps({"run_id": args.run_id, "cells": rows}, indent=2) + "\n")
    for r in rows:
        print(json.dumps(r), flush=True)
    return 0 if all(r["status"] == "passed" for r in rows) else 1


if __name__ == "__main__":
    sys.exit(main())
