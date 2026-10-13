#!/usr/bin/env python3
"""Derive the generation-5 C5 full-snapshot consumer input.

The generation-4 request stays the recorded expired child. This script only
reads it. A parented non-snapshot request whose applied_pos is 0 is expired.
A full snapshot uses position codec snapshot-zero/v1: the request origin stays
0, and each stored row position is that row's index. A snapshot cannot also
carry tombstones. Generation 5 names parent 4. Generations 1-4 are not rebuilt.
"""

import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
WITNESS = (
    ROOT
    / "evidence/bc6cf794-2d14-4e98-bcb6-f7a9b3533a88/DL01/opensearch-driver-preflight/witness-20261012"
)
OUT = (
    ROOT
    / "evidence/bc6cf794-2d14-4e98-bcb6-f7a9b3533a88/DL01/opensearch-driver-preflight/recovery-20261012"
)
ARROW_ORDER = ["led-03", "led-01", "led-02", "led-04", "led-rm"]


def main() -> None:
    source = WITNESS / "import-request.json"
    raw = source.read_bytes()
    document = json.loads(raw)
    if document.get("source_generation") != 4:
        raise SystemExit("recorded child is not generation 4")
    if document.get("parent_source_generation") != 3:
        raise SystemExit("recorded parent is not 3")
    if document.get("applied_pos") != "0":
        raise SystemExit("recorded applied_pos is not 0")
    if "full_snapshot" in document:
        raise SystemExit("recorded request already carries full_snapshot")
    if document.get("tombstones") != ["led-c5"]:
        raise SystemExit("recorded tombstone is not led-c5")
    if document.get("position_codec") != "snapshot-zero/v1":
        raise SystemExit("recorded position codec changed")
    if document.get("lease_id") or document.get("admission_path") or document.get("source_bound"):
        raise SystemExit("recorded request still carries trust fields")
    recovery = dict(document)
    recovery["source_generation"] = 5
    recovery["parent_source_generation"] = 4
    recovery["full_snapshot"] = True
    recovery["tombstones"] = []
    recovery["batch_id"] = "os44-fast-8-g5"
    recovery["applied_pos"] = "0"
    OUT.mkdir(parents=True, exist_ok=True)
    encoded = json.dumps(recovery, separators=(", ", ": "), ensure_ascii=False).encode() + b"\n"
    (OUT / "consumer-input.json").write_bytes(encoded)
    chain = {
        "protocol": "oo.opensearch.c5-recovery/v1",
        "recorded_fault": {
            "path": "witness-20261012/import-request.json",
            "sha256": hashlib.sha256(raw).hexdigest(),
            "source_generation": 4,
            "parent_source_generation": 3,
            "applied_pos": "0",
            "full_snapshot": False,
            "shape": "unique",
            "tombstones": ["led-c5"],
            "reason": "expired",
            "bytes_modified": False,
        },
        "rules": [
            "docs/database-support/03-change-capture-and-consistency.md section 4: POSITION_EXPIRED starts a new generation resnapshot and does not continue from the nearest position",
            "docs/database-support/03 section 2: C5 is a full-diff cycle, position order is none, consistency ceiling is B",
            "docs/database-support/04 section 10: OpenSearch uses PIT plus search_after; deletes are reconcile; consistency is B",
            "classify refuses a parented non-snapshot request whose applied_pos is 0 before connect",
            "full_snapshot true is the flag that forces shape snapshot",
            "a snapshot with tombstones is conflict, so led-c5 is absence from the five live rows",
            "a generation other than 1 requires parent_source_generation == generation - 1",
            "snapshot-zero/v1 stores each row position as its index in the Arrow stream",
        ],
        "next_input": {
            "path": "recovery-20261012/consumer-input.json",
            "source_generation": 5,
            "parent_source_generation": 4,
            "full_snapshot": True,
            "applied_pos": "0",
            "position_codec": "snapshot-zero/v1",
            "tombstones": [],
            "absent_key": "led-c5",
            "arrow_order": ARROW_ORDER,
            "row_position": [str(index) for index in range(len(ARROW_ORDER))],
            "arrow_sha256": document["input_digest"],
            "generations_1_to_4_reimported": False,
            "lease_id": None,
            "admission_path": None,
        },
    }
    (OUT / "chain.json").write_text(json.dumps(chain, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"wrote": str(OUT), "bytes": len(encoded)}))


if __name__ == "__main__":
    main()
