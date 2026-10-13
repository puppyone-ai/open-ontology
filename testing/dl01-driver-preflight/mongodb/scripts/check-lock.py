#!/usr/bin/env python3
"""Verify Cargo.lock pins. This does not update the lock."""

from __future__ import annotations

import pathlib
import sys

EXPECT = {
    "mongodb": ("3.3.0", "622f272c59e54a3c85f5902c6b8e7b1653a6b6681f45e4c42d6581301119a4b8"),
    "mongodb-internal-macros": (
        "3.3.0",
        "63981427a0f26b89632fd2574280e069d09fb2912a3138da15de0174d11dd077",
    ),
    "serde_json": ("1.0.151", "c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14"),
    "serde": ("1.0.229", "4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba"),
    "arrow-array": ("60.0.0", "51dff6e4b9f158864a0aeb6a131ad53858ecbae7a2fc8989307b9b3c6b2f122e"),
    "arrow-ipc": ("60.0.0", "86c523472d22e31f984df7a44fc36e6323d3c9b05ffe53d267ee4c3aa7c128d2"),
    "arrow-schema": ("60.0.0", "f85a9a58cd03560526b0066487574aacb9008e413504fa0ee23d61141e96c338"),
    "arrow-buffer": ("60.0.0", "c7235b863533e9db3ab86905b4521251de11226275b436ff3cb2d0010558d1c5"),
    "arrow-data": ("60.0.0", "5aa06d5e22786c4966ffc4d2f171d0cb65d95f6bdea108a14e8c335e9faa437f"),
    "tokio": ("1.53.2", "e95f91fcc7a621e8b030f6aa23c71fe9838ae2fb4d8118b75602a328f5144044"),
    "bson": ("2.15.0", "7969a9ba84b0ff843813e7249eed1678d9b6607ce5a3b8f0a47af3fcf7978e6e"),
    "mongocrypt": ("0.3.2", "8da0cd419a51a5fb44819e290fbdb0665a54f21dead8923446a799c7f4d26ad9"),
    "mongocrypt-sys": (
        "0.1.6+1.18.2",
        "851fac73f7fe22f6a3ab87f720ce509cae7c9fd08e7dd27866cc232dee07ccf4",
    ),
}


def packages(text: str) -> dict[str, list[tuple[str, str]]]:
    found: dict[str, list[tuple[str, str]]] = {}
    for block in text.split("[[package]]"):
        version = ""
        checksum = ""
        name = ""
        for line in block.strip().splitlines():
            line = line.strip()
            if line.startswith("name = "):
                name = line.split("=", 1)[1].strip().strip('"')
            elif line.startswith("version = ") and not version:
                version = line.split("=", 1)[1].strip().strip('"')
            elif line.startswith("checksum = "):
                checksum = line.split("=", 1)[1].strip().strip('"')
        if name:
            found.setdefault(name, []).append((version, checksum))
    return found


def main() -> int:
    lock_path = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "/src/Cargo.lock")
    out_path = pathlib.Path(sys.argv[2] if len(sys.argv) > 2 else "/evidence/lock-checksums.txt")
    previous = pathlib.Path(sys.argv[3]) if len(sys.argv) > 3 else None
    found = packages(lock_path.read_text())
    lines = []
    failed = False
    for name, (version, checksum) in EXPECT.items():
        rows = found.get(name, [])
        match = any(row == (version, checksum) for row in rows)
        rendered = ",".join(f"{ver}:{sum_}" for ver, sum_ in rows) or "missing"
        lines.append(f"{name} expect={version}:{checksum} found={rendered} match={str(match).lower()}")
        if not match:
            failed = True
    bson_rows = found.get("bson", [])
    extra_bson = [row for row in bson_rows if row[0] != "2.15.0"]
    if extra_bson:
        lines.append("bson_extra " + ",".join(f"{ver}:{sum_}" for ver, sum_ in extra_bson))
    if previous is not None:
        old = packages(previous.read_text())
        for name, rows in sorted(old.items()):
            for version, checksum in rows:
                if not checksum:
                    continue
                if (version, checksum) not in found.get(name, []):
                    lines.append(f"drift {name} {version} {checksum}")
                    failed = True
    text = "\n".join(lines) + "\n"
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(text)
    sys.stdout.write(text)
    return 2 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
