#!/usr/bin/env python3
"""Checks for the StarRocks trusted-receipt producer."""

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import native_producer as producer


def main() -> int:
    producer.assert_schema_oracle()
    out = Path("/tmp/oo-w45-producer-test")
    if out.exists():
        for path in sorted(out.rglob("*"), reverse=True):
            if path.is_file():
                path.unlink()
            elif path.is_dir():
                path.rmdir()
    manifest = producer.emit(out)
    if len(manifest["loads"]) != 20:
        raise SystemExit(f"expected 20 loads, got {len(manifest['loads'])} skipped={manifest['skipped']}")
    if manifest["owner"] == "11111111-1111-1111-1111-111111111111":
        raise SystemExit("owner collided with the shared fixture owner")
    pk = json.loads(Path(out / "g2/pk_accounts/request.json").read_text())
    ids = [row[0] for row in pk["truth"]]
    if "302" in ids or "300" not in ids:
        raise SystemExit(f"catchup pk keys {ids}")
    edited = [row for row in pk["truth"] if row[0] == "300"][0]
    if edited[2] != "edited":
        raise SystemExit("nickname was not the captured edit")
    if pk["full_snapshot"] is not True or pk["upsert_supported"] is not False or pk["window_closed"] is not True:
        raise SystemExit("window flags")
    if pk["parent_source_generation"] != 1:
        raise SystemExit("parent")
    slow_uniq = json.loads(Path(out / "g3/uniq_dim/request.json").read_text())
    if any(row[0] == "18" for row in slow_uniq["truth"]):
        raise SystemExit("dim 18 resurrected")
    if any(row[1] != "second" for row in slow_uniq["truth"]):
        raise SystemExit("failed unique update was rewritten")
    expired_pk = json.loads(Path(out / "g4/pk_accounts/request.json").read_text())
    if any(row[0] == "302" for row in expired_pk["truth"]):
        raise SystemExit("302 resurrected after expiry")
    dup = json.loads(Path(out / "g1/dup_events/request.json").read_text())
    if len(dup["truth"]) != 6:
        raise SystemExit("duplicate rows dropped")
    blob = Path(out / "g1/pk_accounts/receipt.json").read_text()
    if "secret://" in blob or "secret_ref" in blob:
        raise SystemExit("secret in receipt")
    receipt = json.loads(blob)
    request = json.loads(Path(out / "g1/pk_accounts/request.json").read_text())
    for name in ("schema_digest", "type_digest", "config_digest", "input_digest", "audit_digest"):
        if receipt[name] != request[name]:
            raise SystemExit(f"digest drift {name}")
    if request["admission_expect"]["declared_receipt_sha256"] == receipt["schema_digest"]:
        raise SystemExit("declared hash copies schema digest")
    print("native_producer tests ok", len(manifest["loads"]))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
