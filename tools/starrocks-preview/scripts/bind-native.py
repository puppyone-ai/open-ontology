#!/usr/bin/env python3
"""Build the sr_w45 native-import manifests and the operator registry.

The registry is not a client field. It names this capture's report, config,
decoded receipt, catalog, and Arrow digest. It does not name another source.
"""

from __future__ import annotations

import hashlib
import json
import re
import sys
from pathlib import Path

SOURCE_ID = "sr_w45"
ENGINE = "starrocks"
GENERATION = 1
NAMESPACE = "oo_w45_sr"
SNAPSHOT = {"kind": "token", "value": "sr-w45-c4-native-read"}
TABLES = (
    "pk_accounts",
    "agg_metrics",
    "dup_events",
    "uniq_dim",
    "dup_partition_sums",
)
DRIVER_DIGEST = "21bbdc81d436bbffb2951b2245068c1667dccd02ed32a6667b3770b4b7bd1aee"
DECIMAL = re.compile(r"^decimal128\((\d+),(\d+)\)$")


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def load(path: Path):
    return json.loads(path.read_text())


def column_from_field(field: dict) -> dict:
    token = field["type"]
    column = {
        "name": field["name"],
        "logical_name": field["name"],
        "arrow_type": token,
        "nullable": bool(field["nullable"]),
    }
    match = DECIMAL.fullmatch(token)
    if match:
        column["precision"] = int(match.group(1))
        column["scale"] = int(match.group(2))
    return column


def prepare(plan: Path, decoded_dir: Path, arrow_dir: Path) -> None:
    report_bytes = (plan / "report.json").read_bytes()
    config_bytes = (plan / "config.json").read_bytes()
    report = json.loads(report_bytes)
    config = json.loads(config_bytes)
    if report.get("source_id") != SOURCE_ID or report.get("compat_mode") != "starrocks-native":
        raise SystemExit("report is not the sr_w45 starrocks-native policy")
    driver = report.get("driver") or {}
    if driver.get("digest") != DRIVER_DIGEST or driver.get("name") != "starrocks-probe":
        raise SystemExit("report driver is not the StarRocks probe")
    if config.get("source_id") != SOURCE_ID or config.get("requested_write_class") != "R0":
        raise SystemExit("config is not the sr_w45 R0 policy")
    if config.get("r1w") is not None:
        raise SystemExit("config r1w is set")
    config_digest = sha256(config_bytes)
    tables = []
    for name in TABLES:
        decoded_path = decoded_dir / f"{name}.json"
        arrow_path = arrow_dir / f"{name}.arrow"
        decoded_bytes = decoded_path.read_bytes()
        arrow_bytes = arrow_path.read_bytes()
        decoded = json.loads(decoded_bytes)
        fields = decoded["fields"]
        if not fields or len(decoded["rows"]) == 0:
            raise SystemExit(f"{name} capture is empty")
        columns = [column_from_field(field) for field in fields]
        schema_preimage = "".join(
            f"{column['name']}\t{column['arrow_type']}\t{str(column['nullable']).lower()}\n"
            for column in columns
        )
        type_preimage = "".join(f"{column['arrow_type']}\n" for column in columns)
        digests = {
            "config_digest": config_digest,
            "schema_digest": sha256(schema_preimage.encode()),
            "type_digest": sha256(type_preimage.encode()),
            "input_digest": sha256(arrow_bytes),
            "report_sha256": sha256(report_bytes),
            "config_sha256": config_digest,
            "capture_receipt_sha256": sha256(decoded_bytes),
            "arrow_sha256": sha256(arrow_bytes),
            "schema_preimage": schema_preimage,
            "type_preimage": type_preimage,
        }
        table_dir = plan / name
        table_dir.mkdir(parents=True, exist_ok=True)
        catalog = {
            "protocol": "oo.pipeline.sealed-import-catalog/v1",
            "engine": ENGINE,
            "source_id": SOURCE_ID,
            "source_generation": GENERATION,
            "config_digest": digests["config_digest"],
            "schema_digest": digests["schema_digest"],
            "type_digest": digests["type_digest"],
            "input_digest": digests["input_digest"],
            "state": "not_sealed",
            "capture": "identity_only",
        }
        (table_dir / "catalog.json").write_text(json.dumps(catalog, indent=2) + "\n")
        digests["catalog_sha256"] = sha256((table_dir / "catalog.json").read_bytes())
        (table_dir / "digests.json").write_text(json.dumps(digests, indent=2) + "\n")
        manifest = {
            "protocol": "oo.pipeline.native-arrow-import/v1",
            "source_engine": ENGINE,
            "target_storage": "postgresql",
            "source_id": SOURCE_ID,
            "source_generation": GENERATION,
            "snapshot": SNAPSHOT,
            "catalog_identity": {
                "config_digest": digests["config_digest"],
                "schema_digest": digests["schema_digest"],
                "type_digest": digests["type_digest"],
                "input_digest": digests["input_digest"],
            },
            "tables": [
                {
                    "logical_schema": NAMESPACE,
                    "logical_table": name,
                    "row_count": len(decoded["rows"]),
                    "batch_sha256": digests["arrow_sha256"],
                    "arrow_path": f"/frozen/read/{name}.arrow",
                    "columns": columns,
                }
            ],
        }
        forbidden = {"admitted", "admission_bin", "registry_path", "registry_sha256", "approval"}
        if forbidden & set(manifest):
            raise SystemExit("manifest carries client authority")
        (table_dir / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
        tables.append(
            {
                "name": name,
                "row_count": len(decoded["rows"]),
                "capture_receipt_path": f"/frozen/decoded/{name}.json",
                "catalog_path": f"/evidence/plan/{name}/catalog.json",
                "columns": columns,
                "digests": {key: value for key, value in digests.items() if not key.endswith("preimage")},
            }
        )
    (plan / "tables.json").write_text(json.dumps({"tables": tables}, indent=2) + "\n")
    (plan / "digest-preimage.txt").write_text(
        "config_digest is sha256 of admission-config.json bytes.\n"
        "schema_digest is sha256 of name, captured type token, nullable lines.\n"
        "type_digest is sha256 of the captured type tokens, one per line.\n"
        "input_digest is sha256 of that table's native/read Arrow bytes.\n"
        "catalog state is not_sealed. The file is the identity lock, not a physical seal.\n"
        "snapshot token sr-w45-c4-native-read names native/read. The capture has no SCN.\n"
    )


def bind(plan: Path, evidence: Path) -> None:
    decision_bytes = (evidence / "decision.bin").read_bytes()
    decision = json.loads(decision_bytes)
    if decision.get("protocol") != "oo.source-admission.decision/v1":
        raise SystemExit("decision protocol")
    if decision.get("admitted") is not True or decision.get("blocks"):
        raise SystemExit("decision is not an empty admission")
    source_id = (decision.get("identity") or {}).get("source_id")
    if source_id != SOURCE_ID:
        raise SystemExit(f"decision source_id {source_id}")
    report = load(plan / "report.json")
    driver = report["driver"]
    decision_sha = sha256(decision_bytes)
    (evidence / "decision.sha256").write_text(decision_sha + "\n")
    bindings = []
    for table in load(plan / "tables.json")["tables"]:
        digests = table["digests"]
        fields = []
        for column in table["columns"]:
            field = {
                "name": column["name"],
                "arrow_type": column["arrow_type"],
                "nullable": column["nullable"],
                "precision": column.get("precision"),
                "scale": column.get("scale"),
            }
            fields.append(field)
        bindings.append(
            {
                "protocol": "oo.source-admission.capture-binding/v1",
                "source_id": SOURCE_ID,
                "source_engine": ENGINE,
                "source_generation": GENERATION,
                "snapshot": SNAPSHOT,
                "decision_sha256": decision_sha,
                "report_path": "/evidence/plan/report.json",
                "report_sha256": digests["report_sha256"],
                "config_path": "/evidence/plan/config.json",
                "config_sha256": digests["config_sha256"],
                "config_digest": digests["config_digest"],
                "schema_digest": digests["schema_digest"],
                "type_digest": digests["type_digest"],
                "input_digest": digests["input_digest"],
                "catalog_path": table["catalog_path"],
                "catalog_sha256": digests["catalog_sha256"],
                "capture_receipt_path": table["capture_receipt_path"],
                "capture_receipt_sha256": digests["capture_receipt_sha256"],
                "admission_receipt_path": "/frozen/native-readonly-receipt.json",
                "driver_kind": driver["kind"],
                "driver_name": driver["name"],
                "driver_version": driver["version"],
                "driver_digest": driver["digest"],
                "objects": [
                    {
                        "logical_schema": NAMESPACE,
                        "logical_table": table["name"],
                        "row_count": table["row_count"],
                        "arrow_sha256": digests["arrow_sha256"],
                    }
                ],
                "fields": fields,
            }
        )
    registry = {"protocol": "oo.source-admission.binding-registry/v1", "bindings": bindings}
    text = json.dumps(registry, indent=2) + "\n"
    if "secret://" in text or "secret_ref" in text or "clickhouse_268" in text:
        raise SystemExit("registry picked up a secret or another source identity")
    path = evidence / "registry.json"
    path.write_text(text)
    print(sha256(path.read_bytes()))


def main() -> int:
    command = sys.argv[1]
    if command == "prepare":
        prepare(Path(sys.argv[2]), Path(sys.argv[3]), Path(sys.argv[4]))
        return 0
    if command == "bind":
        bind(Path(sys.argv[2]), Path(sys.argv[3]))
        return 0
    raise SystemExit(f"unknown command {command}")


if __name__ == "__main__":
    raise SystemExit(main())
