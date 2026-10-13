#!/usr/bin/env python3
"""Import one real customer seal through import-jsonb-document.

The side table is the 0009 install proof. It is not the 16-row source.
"""

from __future__ import annotations

import hashlib
import json
import subprocess
import uuid
from pathlib import Path

ROOT = Path("/Users/redux/puppy-one/open-ontology")
CANON = ROOT / "evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-67-canonical"
SEAL = ROOT / "evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-68-seal"
OUT = ROOT / "evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-69-jsonb-one"
OLD = "pt_a1d0e63667e64ab1b7b9c250e46e7043a798de705406b292dd4b238c3254"


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def physical(prefix: str, level: int, config: str, table: str, column: str) -> str:
    out = bytearray(b"physical-mapping/v1\0")
    for part in ("oo_w141", config, "1"):
        out += part.encode() + b"\0"
    out.append(level)
    for part in ("oo_w141", table, column):
        out += part.encode() + b"\0"
    return prefix + sha256(bytes(out))[:60]


def psql(sql: str) -> str:
    proc = subprocess.run(
        [
            "docker", "exec", "-i", "oo-dl01w1-41-pg",
            "psql", "-v", "ON_ERROR_STOP=1", "-U", "oo_w141_admin", "-d", "oo_w141", "-At", "-c", sql,
        ],
        text=True,
        capture_output=True,
        check=False,
    )
    if proc.returncode != 0:
        raise SystemExit(proc.stderr.strip() or f"psql {proc.returncode}")
    return proc.stdout.strip()


def main() -> int:
    if OUT.exists():
        raise SystemExit("output dir exists")
    if psql(f"SELECT count(*) FROM _oo.{OLD}") != "16":
        raise SystemExit("retained table row count changed")
    if psql(
        "SELECT count(*) FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid "
        f"JOIN pg_type t ON t.oid=a.atttypid WHERE c.relname='{OLD}' AND t.typname='jsonb' AND a.attnum>0 AND NOT a.attisdropped"
    ) != "0":
        raise SystemExit("retained table gained a jsonb column")
    side = json.loads((CANON / "envelopes/side-00.json").read_text())
    if side["dataset"] != "oo_w141.shipment_doc" or side["column"] != "doc":
        raise SystemExit("side envelope identity")
    side_sha = sha256(bytes.fromhex(side["pg_jsonb_hex"]))
    config = sha256((CANON / "envelopes/side-00.json").read_bytes())
    namespace = physical("pn_", ord("n"), config, "", "")
    table = physical("pt_", ord("t"), config, "shipment_doc", "")
    id_column = physical("pc_", ord("c"), config, "shipment_doc", "id")
    doc_column = physical("pc_", ord("c"), config, "shipment_doc", "doc")
    if table == OLD or len({namespace, table, id_column, doc_column}) != 4:
        raise SystemExit("shadow name collided")
    existing_ns = psql("SELECT count(*) FROM pg_namespace WHERE nspname='" + namespace + "'")
    existing_rel = psql(
        "SELECT count(*) FROM pg_class WHERE relname IN ("
        + ",".join("'" + name + "'" for name in (table, id_column, doc_column))
        + ")"
    )
    if existing_ns != "0" or existing_rel != "0":
        raise SystemExit("shadow name already exists")
    preimage = "oo_w141.shipment_doc\nid\t23\t-1\t\ndoc\t3802\t-1\t\n"
    schema_digest = sha256(preimage.encode())
    catalog = {
        "protocol": "oo.pipeline.sealed-import-catalog/v1",
        "capture": "sealed_import",
        "engine": "mongodb",
        "source_id": "oo_w141",
        "config_digest": config,
        "input_digest": side_sha,
        "type_digest": sha256(b"int4\njsonb\n"),
        "schema_digest": schema_digest,
        "tool_hook_digest": sha256(b"oo-w141-shipment-doc-hook\n"),
        "run_id": str(uuid.uuid4()),
        "source_generation": 1,
        "target_generation": 1,
        "state": "awaiting_publish",
        "hook_checked": False,
        "shadows_queried": False,
        "source_introspection": "not_used",
        "tables": [
            {
                "logical_schema": "oo_w141",
                "logical_table": "shipment_doc",
                "accepted_rows": 1,
                "canonical_digest": side_sha,
                "stored_namespace": namespace,
                "stored_physical": table,
                "columns": [
                    {
                        "logical_name": "id",
                        "oid": 23,
                        "typmod": -1,
                        "caveat": None,
                        "source_type": "int4",
                        "key": True,
                        "stored_physical": id_column,
                    },
                    {
                        "logical_name": "doc",
                        "oid": 3802,
                        "typmod": -1,
                        "caveat": None,
                        "source_type": "jsonb",
                        "key": False,
                        "stored_physical": doc_column,
                    },
                ],
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
                    "physical_name": table,
                    "batch_id": "t0.b0",
                    "kind": "batch",
                    "accepted_rows": 1,
                    "rejected_rows": 0,
                }
            ],
        },
    }
    request = {
        "protocol": "oo.pipeline.jsonb-document-import/v1",
        "engine": "mongodb",
        "source_id": "oo_w141",
        "catalog": catalog,
        "document": side,
        "target": {
            "host": "oo-dl01w1-41-pg",
            "port": 5432,
            "database": "oo_w141",
            "user": "oo_w141_admin",
            "password_file": "/run/secrets/admin-password",
            "ca_file": "/run/secrets/ca.crt",
        },
    }
    OUT.mkdir()
    (OUT / "request.json").write_text(json.dumps(request) + "\n")
    (OUT / "shadows.json").write_text(json.dumps({
        "namespace": namespace,
        "table": table,
        "id_column": id_column,
        "doc_column": doc_column,
        "old_table_untouched_name": OLD,
    }, indent=2) + "\n")
    print("request-ready")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
