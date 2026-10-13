#!/usr/bin/env python3
"""Write the operator registry after the frozen validator prints a decision.

The registry is not a client field. It names this source's report, config,
capture receipt, catalog, and Arrow digest. It does not name a ClickHouse batch.
"""

from __future__ import annotations

import hashlib
import json
import sys
from pathlib import Path


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main() -> int:
    plan = Path(sys.argv[1])
    evidence = Path(sys.argv[2])
    decision_bytes = (evidence / "decision.bin").read_bytes()
    decision = json.loads(decision_bytes)
    if decision.get("protocol") != "oo.source-admission.decision/v1":
        raise SystemExit("decision protocol")
    if decision.get("admitted") is not True or decision.get("blocks"):
        raise SystemExit("decision is not an empty admission")
    source_id = decision.get("identity", {}).get("source_id")
    if source_id != "oo_w141":
        raise SystemExit(f"decision source_id {source_id}")
    decision_sha = hashlib.sha256(decision_bytes).hexdigest()
    (evidence / "decision.sha256").write_text(decision_sha + "\n")
    digests = json.loads((plan / "digests.json").read_text())
    columns = json.loads((plan / "columns.json").read_text())
    fields = []
    for column in columns:
        fields.append(
            {
                "name": column["name"],
                "arrow_type": column["arrow_type"],
                "nullable": column["nullable"],
                "precision": column["precision"],
                "scale": column["scale"],
            }
        )
    registry = {
        "protocol": "oo.source-admission.binding-registry/v1",
        "bindings": [
            {
                "protocol": "oo.source-admission.capture-binding/v1",
                "source_id": "oo_w141",
                "source_engine": "mongodb",
                "source_generation": 1,
                "snapshot": {"kind": "token", "value": "oo-dl01w1-41-frozen-batch"},
                "decision_sha256": decision_sha,
                "report_path": "/evidence/plan/report.json",
                "report_sha256": digests["report_sha256"],
                "config_path": "/evidence/plan/config.json",
                "config_sha256": digests["config_sha256"],
                "config_digest": digests["config_digest"],
                "schema_digest": digests["schema_digest"],
                "type_digest": digests["type_digest"],
                "input_digest": digests["input_digest"],
                "catalog_path": "/evidence/plan/catalog.json",
                "catalog_sha256": digests["catalog_sha256"],
                "capture_receipt_path": "/evidence/plan/capture-receipt.json",
                "capture_receipt_sha256": digests["capture_receipt_sha256"],
                "admission_receipt_path": "/frozen/native-readonly-receipt.json",
                "driver_kind": "native-rust",
                "driver_name": "mongodb",
                "driver_version": "3.3.0",
                "driver_digest": "622f272c59e54a3c85f5902c6b8e7b1653a6b6681f45e4c42d6581301119a4b8",
                "objects": [
                    {
                        "logical_schema": "oo_w141",
                        "logical_table": "shipments",
                        "row_count": 16,
                        "arrow_sha256": digests["arrow_sha256"],
                    }
                ],
                "fields": fields,
            }
        ],
    }
    text = json.dumps(registry, indent=2) + "\n"
    if "secret://" in text or "clickhouse_268" in text:
        raise SystemExit("registry picked up a secret or another source identity")
    path = evidence / "registry.json"
    path.write_text(text)
    print(hashlib.sha256(path.read_bytes()).hexdigest())
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
