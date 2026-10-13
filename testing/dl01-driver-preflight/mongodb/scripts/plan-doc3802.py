#!/usr/bin/env python3
"""Write the generation-2 native import plan for the doc Arrow.

The frozen report, config, and admission decision stay the validator inputs.
This script does not open a database and does not rewrite the frozen batch.
"""

from __future__ import annotations

import hashlib
import json
import sys
from pathlib import Path

ROOT = Path("/Users/redux/puppy-one/open-ontology")
FROZEN_PLAN = ROOT / "evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/native-bind/plan"
ARROW_DIR = ROOT / "evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-72-doc-arrow"
OUT = ROOT / "evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-71-native-import"
CONFIG_DIGEST = "37cf3b3429abf94912befc96f28040b70405419e73698994dbc5e9ae4d4d960a"
REPORT_SHA = "48c6ad5c6b403aa9b4884bdf0878d5553e4491246a710e14748aa55b6fa73203"
DECISION_SHA = "56eb2fd4348bedaadcc9d82fc1d2688f80a6d7600b8a8e18c9441c7271967e84"
FROZEN_ARROW = "4764e423c574ab54043873d93250596c290ed08ebb93048bee1e9cc64c4c96c8"
EXPECTED_TABLE = "pt_4d11431451ccacc3837c1cbd40cc4f0e28844c70b465650f63aef51b9898"
RETAINED_TABLE = "pt_a1d0e63667e64ab1b7b9c250e46e7043a798de705406b292dd4b238c3254"
SNAPSHOT = "oo-dl01w1-41-doc3802"
GENERATION = 2


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def physical_name(generation: int, level: str, namespace: str, table: str, column: str) -> str:
    out = bytearray(b"physical-mapping/v1\0")
    for part in (
        b"oo_w141",
        CONFIG_DIGEST.encode(),
        str(generation).encode(),
    ):
        out.extend(part)
        out.append(0)
    out.extend(level.encode())
    for part in (namespace.encode(), table.encode(), column.encode()):
        out.extend(part)
        out.append(0)
    prefix = {"n": "pn_", "t": "pt_", "c": "pc_"}[level]
    return prefix + sha256(bytes(out))[:60]


def main() -> int:
    if OUT.exists():
        raise SystemExit("evidence dir already exists")
    arrow = (ARROW_DIR / "shipments-doc.arrow").read_bytes()
    arrow_sha = sha256(arrow)
    recorded = (ARROW_DIR / "arrow.sha256").read_text().strip()
    if arrow_sha != recorded or arrow_sha == FROZEN_ARROW:
        raise SystemExit("arrow identity")
    identity = json.loads((ARROW_DIR / "arrow-identity.json").read_text())
    if identity.get("rows") != 16 or identity.get("columns") != 20 or identity.get("frozen_arrow_replaced") is not False:
        raise SystemExit("arrow identity json")
    report = (FROZEN_PLAN / "report.json").read_bytes()
    config = (FROZEN_PLAN / "config.json").read_bytes()
    if sha256(report) != REPORT_SHA or sha256(config) != CONFIG_DIGEST:
        raise SystemExit("frozen report or config changed")
    schema = json.loads((FROZEN_PLAN / "schema.json").read_text())["schema"]
    columns = json.loads((FROZEN_PLAN / "columns.json").read_text())
    if len(schema) != 19 or len(columns) != 19:
        raise SystemExit("frozen schema width")
    schema = schema + [{"name": "doc", "type": "utf8", "nullable": False}]
    schema_preimage = "".join(
        f"{item['name']}\t{item['type']}\t{str(item['nullable']).lower()}\n" for item in schema
    )
    type_preimage = "".join(f"{item['type']}\n" for item in schema)
    digests = {
        "config_digest": CONFIG_DIGEST,
        "schema_digest": sha256(schema_preimage.encode()),
        "type_digest": sha256(type_preimage.encode()),
        "input_digest": arrow_sha,
        "report_sha256": REPORT_SHA,
        "config_sha256": CONFIG_DIGEST,
        "arrow_sha256": arrow_sha,
        "frozen_arrow_sha256": FROZEN_ARROW,
    }
    table = physical_name(GENERATION, "t", "oo_w141", "shipments", "")
    retained = physical_name(1, "t", "oo_w141", "shipments", "")
    if table != EXPECTED_TABLE or retained != RETAINED_TABLE:
        raise SystemExit(f"physical name {table} retained {retained}")
    if table == retained:
        raise SystemExit("generation 2 adopted the retained table")
    fields = []
    manifest_columns = []
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
        manifest_columns.append(
            {
                "arrow_type": column["arrow_type"],
                "logical_name": column["name"],
                "name": column["name"],
                "nullable": column["nullable"],
                "precision": column["precision"],
                "scale": column["scale"],
            }
        )
    doc_field = {"name": "doc", "arrow_type": "utf8", "nullable": False, "document": True}
    doc_column = {
        "arrow_type": "utf8",
        "logical_name": "doc",
        "name": "doc",
        "nullable": False,
        "document": True,
    }
    if any(key in doc_field for key in ("precision", "scale", "char_length")):
        raise SystemExit("doc field carries a numeric key")
    fields.append(doc_field)
    manifest_columns.append(doc_column)
    OUT.mkdir(parents=True)
    plan = OUT / "plan"
    plan.mkdir()
    (plan / "report.json").write_bytes(report)
    (plan / "config.json").write_bytes(config)
    (plan / "schema-preimage.txt").write_text(schema_preimage)
    (plan / "type-preimage.txt").write_text(type_preimage)
    receipt = {
        "ready": False,
        "decoder": "oo-w141-doc-arrow",
        "driver": "none",
        "rows": 16,
        "metadata": {
            "encoder": "oo-w141-doc-arrow",
            "json_document_column": "doc",
            "document_scope": "seal-scope",
            "weight_in_document": False,
            "typed_weight_column": "weight",
            "frozen_arrow_sha256": FROZEN_ARROW,
            "frozen_arrow_replaced": False,
        },
    }
    receipt_bytes = (json.dumps(receipt, indent=2) + "\n").encode()
    (plan / "capture-receipt.json").write_bytes(receipt_bytes)
    catalog = {
        "protocol": "oo.pipeline.sealed-import-catalog/v1",
        "engine": "mongodb",
        "source_id": "oo_w141",
        "source_generation": GENERATION,
        "config_digest": digests["config_digest"],
        "schema_digest": digests["schema_digest"],
        "type_digest": digests["type_digest"],
        "input_digest": digests["input_digest"],
        "state": "not_sealed",
        "capture": "identity_only",
    }
    catalog_bytes = (json.dumps(catalog, indent=2) + "\n").encode()
    (plan / "catalog.json").write_bytes(catalog_bytes)
    digests["capture_receipt_sha256"] = sha256(receipt_bytes)
    digests["catalog_sha256"] = sha256(catalog_bytes)
    (plan / "digests.json").write_text(json.dumps(digests, indent=2) + "\n")
    registry = {
        "protocol": "oo.source-admission.binding-registry/v1",
        "bindings": [
            {
                "protocol": "oo.source-admission.capture-binding/v1",
                "source_id": "oo_w141",
                "source_engine": "mongodb",
                "source_generation": GENERATION,
                "snapshot": {"kind": "token", "value": SNAPSHOT},
                "decision_sha256": DECISION_SHA,
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
                        "arrow_sha256": arrow_sha,
                    }
                ],
                "fields": fields,
            }
        ],
    }
    registry_text = json.dumps(registry, indent=2) + "\n"
    if "secret://" in registry_text:
        raise SystemExit("registry contains a secret ref")
    (OUT / "registry.json").write_text(registry_text)
    registry_sha = sha256((OUT / "registry.json").read_bytes())
    (OUT / "registry.sha256").write_text(registry_sha + "\n")
    request = {
        "catalog_identity": {
            "config_digest": digests["config_digest"],
            "input_digest": digests["input_digest"],
            "schema_digest": digests["schema_digest"],
            "type_digest": digests["type_digest"],
        },
        "protocol": "oo.pipeline.native-arrow-import/v1",
        "snapshot": {"kind": "token", "value": SNAPSHOT},
        "source_engine": "mongodb",
        "source_generation": GENERATION,
        "source_id": "oo_w141",
        "tables": [
            {
                "arrow_path": "/evidence/shipments-doc.arrow",
                "batch_sha256": arrow_sha,
                "columns": manifest_columns,
                "logical_schema": "oo_w141",
                "logical_table": "shipments",
                "row_count": 16,
            }
        ],
        "target": {
            "ca_file": "/run/secrets/ca.crt",
            "database": "oo_w141",
            "host": "oo-dl01w1-41-pg",
            "owner": "23df6145-4592-4dc0-82df-fd226dd020fd",
            "password_file": "/run/secrets/admin-password",
            "port": 5432,
            "runtime_principal": "oo_w141_admin",
            "shard": "oo_w141",
        },
        "target_storage": "postgresql",
    }
    request_text = json.dumps(request, indent=2) + "\n"
    forbidden = {"admitted", "admission_bin", "registry_path", "registry_sha256", "approval"}
    if forbidden & set(json.loads(request_text)):
        raise SystemExit("request carries client authority")
    if "secret://" in request_text or "clickhouse" in request_text:
        raise SystemExit("request identity")
    (OUT / "request.json").write_text(request_text)
    (plan / "manifest.json").write_text(request_text)
    shadows = {
        "algorithm": "physical-mapping/v1",
        "config_digest": CONFIG_DIGEST,
        "source_generation": GENERATION,
        "expected_table": table,
        "retained_table": retained,
        "retained_must_stay_rows": 16,
        "retained_jsonb_columns": 0,
    }
    (OUT / "shadows.json").write_text(json.dumps(shadows, indent=2) + "\n")
    (OUT / "shipments-doc.arrow").write_bytes(arrow)
    if sha256((OUT / "shipments-doc.arrow").read_bytes()) != arrow_sha:
        raise SystemExit("copied arrow digest")
    print(registry_sha)
    return 0


if __name__ == "__main__":
    sys.exit(main())
