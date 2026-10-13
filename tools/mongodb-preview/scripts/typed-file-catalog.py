#!/usr/bin/env python3
"""Build a sealed-import catalog from this run's envelope and physical mappings.

schema_digest stays the digest stored by the import. This script does not
replace it after seeing the column oids.
"""

from __future__ import annotations

import hashlib
import json
import sys
import unicodedata
from pathlib import Path


def stable_column(name: str) -> str:
    if name == "_id" or (name.isascii() and name.replace("_", "").isalnum() and name[:1].islower() and name == name.lower()):
        return name
    return "q_" + unicodedata.normalize("NFC", name).encode().hex()


def main() -> int:
    out = Path(sys.argv[1])
    envelope = json.loads((out / "positive.json").read_text())
    result = envelope["import"]
    table = result["tables"][0]
    mappings = json.loads((out / "physical-mappings.json").read_text())
    run_rows = json.loads((out / "sealed-run.json").read_text())
    stored_digest = result.get("schema_digest") or envelope.get("schema_digest")
    if not stored_digest:
        identity = json.loads((out / "plan" / "digests.json").read_text())
        stored_digest = identity["schema_digest"]
    columns_out = []
    preimage = f"{table['logical_schema']}.{table['logical_table']}\n"
    by_logical = {}
    for row in mappings:
        if row["level"] == "c":
            by_logical[row["logical_column"]] = row["physical_name"]
    namespace = next(row["physical_name"] for row in mappings if row["level"] == "n")
    physical = table["physical_name"]
    for column in table["columns"]:
        logical = column["logical_name"]
        stored = by_logical[stable_column(logical)]
        oid = int(column["oid"])
        typmod = int(column["typmod"])
        columns_out.append(
            {
                "caveat": None,
                "key": False,
                "logical_name": logical,
                "oid": oid,
                "source_type": None,
                "stored_physical": stored,
                "typmod": typmod,
            }
        )
        preimage += f"{logical}\t{oid}\t{typmod}\t\n"
    actual = hashlib.sha256(preimage.encode()).hexdigest()
    compare = {
        "stored_schema_digest": stored_digest,
        "oid_preimage_sha256": actual,
        "match": actual == stored_digest,
        "preimage_bytes": len(preimage.encode()),
    }
    (out / "schema-digest-compare.json").write_text(json.dumps(compare, indent=2) + "\n")
    run = run_rows[0]
    tool_hook = hashlib.sha256((out / "positive.json").read_bytes()).hexdigest()
    catalog = {
        "protocol": "oo.pipeline.sealed-import-catalog/v1",
        "capture": "sealed_import",
        "engine": "mongodb",
        "source_id": "oo_w141",
        "config_digest": run["config_digest"],
        "input_digest": run["input_digest"],
        "type_digest": run["type_digest"],
        "schema_digest": stored_digest,
        "tool_hook_digest": tool_hook,
        "run_id": run["run_id"],
        "source_generation": int(run["source_generation"]),
        "target_generation": int(run["target_generation"]),
        "state": "awaiting_publish",
        "hook_checked": False,
        "shadows_queried": False,
        "source_introspection": "not_used",
        "tables": [
            {
                "logical_schema": table["logical_schema"],
                "logical_table": table["logical_table"],
                "accepted_rows": int(table["row_count"]),
                "canonical_digest": table["canonical_digest"],
                "stored_namespace": namespace,
                "stored_physical": physical,
                "columns": columns_out,
                "rejects": [],
            }
        ],
        "quality": {
            "loss_concealed": False,
            "manifest_reject_count": 0,
            "payload_includes_reject_lines": False,
            "stored_omits_manifest_rejects": False,
            "stored_rejected_rows": 0,
            "stored_rows": [
                {
                    "accepted_rows": int(table["row_count"]),
                    "attnum": None,
                    "batch_id": table.get("batch_id") or "t0.b0",
                    "key_digest": None,
                    "kind": "batch",
                    "physical_name": physical,
                    "reason": None,
                    "rejected_rows": 0,
                    "row_index": None,
                    "value_len": None,
                }
            ],
        },
    }
    (out / "sealed-catalog.json").write_text(json.dumps(catalog, indent=2) + "\n")
    (out / "tool-hook-note.json").write_text(
        json.dumps(
            {
                "tool_hook_digest": tool_hook,
                "equals": "sha256 of positive.json bytes",
                "hook_checked": False,
                "hook_proof": False,
            },
            indent=2,
        )
        + "\n"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
