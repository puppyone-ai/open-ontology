#!/usr/bin/env python3
"""Build only the owner-authorised DM8 package, locally, without downloading."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import zipfile

ROOT = Path(__file__).resolve().parents[3]
ARCHIVE_SHA256 = "18103060e2223ea5054a8f82fea14f5289fa3ce994a1bb4b6461b175bc687bca"
ISO_SHA256 = "fb631279a60fa2fb814179fa712d4847aef32c9a2e694da5f93241d0fdef9d54"
INSTALLER_SHA256 = "71d0eef76796360bbc9a5ddad39c697f3fd63706f83af02c043e9b7167117c49"
ISO_NAME = "dm8_20260817_x86_Ubuntu22_64.iso"


def verify(path, expected):
    with path.open("rb") as source:
        actual = hashlib.file_digest(source, "sha256").hexdigest()
    if actual != expected:
        raise ValueError("package checksum mismatch; installation refused")


def prepare(archive, out):
    verify(archive, ARCHIVE_SHA256)
    out.mkdir(parents=True, exist_ok=False)
    context = out / "context"
    context.mkdir()
    iso = out / "dm8.iso"
    # Read one exact member, never extract vendor-supplied paths.
    with zipfile.ZipFile(archive) as source:
        with source.open(ISO_NAME) as src, iso.open("wb") as dst:
            shutil.copyfileobj(src, dst)
    verify(iso, ISO_SHA256)
    installer = context / "DMInstall.bin"
    with installer.open("wb") as dst:
        subprocess.run(["tar", "-xOf", str(iso), "DMInstall.bin"],
                       stdout=dst, check=True)
    verify(installer, INSTALLER_SHA256)
    for name in ("Dockerfile", "install.xml"):
        shutil.copyfile(Path(__file__).with_name(name), context / name)
    return context


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--owner-authorised-e340", action="store_true")
    args = parser.parse_args()
    if not re.fullmatch(r"[a-z0-9][a-z0-9-]{0,80}", args.run_id):
        parser.error("invalid run-id")
    if not args.owner_authorised_e340:
        parser.error("explicit E340 licence authorisation required")
    archive = args.archive.resolve()
    if not archive.is_relative_to(ROOT / "evidence"):
        parser.error("archive must be inside this worktree's evidence directory")
    out = ROOT / "evidence" / args.run_id / "dm-build"
    context = prepare(archive, out)
    image = "oo-e-dm8:" + args.run_id
    with (out / "build.log").open("w") as log:
        result = subprocess.run(
            ["docker", "build", "--platform", "linux/amd64",
             "--label", "oo.lane=e", "--label", "oo.run_id=" + args.run_id,
             "-t", image, str(context)], stdout=log, stderr=subprocess.STDOUT,
        )
    receipt = {
        "archive_sha256": ARCHIVE_SHA256, "iso_sha256": ISO_SHA256,
        "installer_sha256": INSTALLER_SHA256, "authorisation": "E340",
        "distribution": "local testing only; never push",
        "build_exit": result.returncode,
    }
    if result.returncode == 0:
        receipt["image_id"] = subprocess.check_output(
            ["docker", "image", "inspect", "--format", "{{.Id}}", image],
            text=True,
        ).strip()
    (out / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    return result.returncode


if __name__ == "__main__":
    raise SystemExit(main())
