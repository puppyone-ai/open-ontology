#!/usr/bin/env python3
"""Build a local-only thick worker without putting Oracle binaries in git."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
from urllib.parse import urlparse
import zipfile

ROOT = Path(__file__).resolve().parents[4]
URL = ("https://download.oracle.com/otn_software/linux/instantclient/1930000/"
       "instantclient-basic-linux.x64-19.30.0.0.0dbru.zip")
SHA256 = "4711cab0a1891ea5c6c4498aa70f5b0934308e5c144e03a755d86da9b4eaaf12"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--run-id", required=True)
    args = parser.parse_args()
    if not args.run_id or not all(c.isalnum() or c in "-_" for c in args.run_id):
        parser.error("invalid run-id")
    out = ROOT / "evidence" / args.run_id / "worker-build"
    out.mkdir(parents=True, exist_ok=False)
    context = out / "context"
    context.mkdir()
    archive = context / "instantclient.zip"
    result = subprocess.run(
        ["curl", "--fail", "--location", "--retry", "3", "--retry-all-errors",
         "--max-time", "180",
         "--output", str(archive), "--write-out", "%{url_effective}", URL],
        capture_output=True, text=True,
    )
    (out / "download.log").write_text(result.stderr)
    if result.returncode or urlparse(result.stdout).hostname != "download.oracle.com":
        raise RuntimeError("public download failed or redirected; do not sign in")
    if hashlib.sha256(archive.read_bytes()).hexdigest() != SHA256:
        raise RuntimeError("client checksum mismatch; do not use or redistribute")
    with zipfile.ZipFile(archive) as source:
        (out / "BASIC_LICENSE").write_bytes(source.read("instantclient_19_30/BASIC_LICENSE"))
    for name in ("Dockerfile", "worker.py"):
        shutil.copyfile(Path(__file__).with_name(name), context / name)
    image = "oo-e-oracle-worker:" + args.run_id
    with (out / "build.log").open("w") as log:
        subprocess.run(["docker", "build", "--platform", "linux/amd64",
                        "--label", "oo.lane=e", "--label", "oo.run_id=" + args.run_id,
                        "-t", image, str(context)], stdout=log, stderr=subprocess.STDOUT,
                       check=True)
    inspection = subprocess.check_output(["docker", "image", "inspect", image], text=True)
    (out / "image.json").write_text(inspection)
    (out / "provenance.json").write_text(json.dumps({
        "url": URL, "sha256": SHA256, "license": "Oracle Free Use Terms and Conditions",
        "distribution": "local test image only; never publish",
    }, indent=2) + "\n")
    print(image)


if __name__ == "__main__":
    main()
