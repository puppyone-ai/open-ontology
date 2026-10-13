#!/usr/bin/env python3
"""Fail if the driver lock is not a subset of the immutable base lock."""

from __future__ import annotations

import json
import sys
from pathlib import Path

REQUIRED = {
    "reqwest": ("0.13.5", "16a1cfa75cc186dd73d5818e510e042e40927bccc9c236b061cea97e1eb08029"),
    "arrow-array": ("60.0.0", "51dff6e4b9f158864a0aeb6a131ad53858ecbae7a2fc8989307b9b3c6b2f122e"),
    "arrow-ipc": ("60.0.0", "86c523472d22e31f984df7a44fc36e6323d3c9b05ffe53d267ee4c3aa7c128d2"),
    "arrow-schema": ("60.0.0", "f85a9a58cd03560526b0066487574aacb9008e413504fa0ee23d61141e96c338"),
    "arrow-buffer": ("60.0.0", "c7235b863533e9db3ab86905b4521251de11226275b436ff3cb2d0010558d1c5"),
    "serde": ("1.0.229", "4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba"),
    "serde_json": ("1.0.151", "c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14"),
    "tokio": ("1.53.1", "202caea871b69668250d242070849eb495be178ed697a3e98aebce5bc81a0bed"),
    "sha2": ("0.10.9", "a7507d819769d01a365ab707794a4084392c824f54a7a6a7862f8c3d0892b283"),
    "zeroize": ("1.9.0", "e13c156562582aa81c60cb29407084cdb54c4164760106ab78e6c5b0858cf64e"),
    "hyper-util": ("0.1.20", "96547c2556ec9d12fb1578c4eaf448b04993e7fb79cbaad930a656880a6bdfa0"),
}
OWN_PATH = {"ch-reader", "ch-decode"}


def packages(text: str) -> dict[tuple[str, str], dict[str, str]]:
    found: dict[tuple[str, str], dict[str, str]] = {}
    for block in text.split("[[package]]")[1:]:
        fields: dict[str, str] = {}
        for line in block.splitlines():
            if " = " not in line or line.startswith(" "):
                continue
            key, value = line.split(" = ", 1)
            if key in {"name", "version", "checksum", "source"}:
                fields[key] = value.strip().strip('"')
        name = fields.get("name")
        version = fields.get("version")
        if name and version:
            found[(name, version)] = fields
    return found


def main() -> int:
    own_path = Path(sys.argv[1])
    base_path = Path(sys.argv[2])
    out_path = Path(sys.argv[3])
    own = packages(own_path.read_text())
    base = packages(base_path.read_text())
    failures: list[str] = []
    for (name, version), fields in sorted(own.items()):
        if name in OWN_PATH and not fields.get("checksum"):
            continue
        base_fields = base.get((name, version))
        if base_fields is None:
            failures.append(f"{name} {version} is not in the base lock")
            continue
        if fields.get("checksum") != base_fields.get("checksum"):
            failures.append(f"{name} {version} checksum drifted")
    for name, (version, checksum) in REQUIRED.items():
        fields = own.get((name, version))
        if fields is None or fields.get("checksum") != checksum:
            failures.append(f"required pin {name} {version} is absent or has a different checksum")
    report = {"passed": not failures, "failures": failures, "own_packages": len(own)}
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(report, indent=2) + "\n")
    if failures:
        print("\n".join(failures), file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
