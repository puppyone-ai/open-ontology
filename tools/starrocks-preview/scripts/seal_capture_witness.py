#!/usr/bin/env python3
"""Seal the existing StarRocks catchup capture with the fixed 02 validator.

The catchup tick, Arrow streams, reader audit, and probe binary stay where
they were written. This script does not start StarRocks, does not import the
twenty native generations again, and does not insert a jobs lease.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path

REPO = Path("/Users/redux/puppy-one/open-ontology")
PRODUCT = REPO / "evidence/2c023948-0862-4dae-ae8f-6fc30241db62/DL01/starrocks-product"
CATCHUP = PRODUCT / "raw/capture-schedule"
OUT = PRODUCT / "raw/seal-witness"
DRIVER = (
    REPO
    / "evidence/2c023948-0862-4dae-ae8f-6fc30241db62/DL01/starrocks-driver-preflight/build/bin/starrocks-probe"
)
VALIDATOR = (
    REPO
    / "evidence/5983d507-bd78-474b-9417-32909ed054eb/DL01/admission/linux/seal-194/bin/oo-source-admission"
)
PIPELINE = (
    REPO
    / "evidence/b5ac4bc8-1872-4fe4-a56a-04a70f7103a2/DL01/native-physical-import/trust-registration/raw/194c/oo-pipeline"
)
IMAGE = "docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
VALIDATOR_SHA = "e465241fc1292b6eb5a13cf12efbffe360a9fbf1cbb03c02affb0241a4e58e1f"
PIPELINE_SHA = "56514ae7e734ab211365c37b8889644f93a979dc82326708a7cfc3a7012e9866"
SOURCE_ID = "sr_w45"
SCHEMA = "oo_w45_sr"
GENERATION = 1
SNAPSHOT = "t2"
DELETE_OK = {"reconcile", "partition_replace", "unknown", "none", "key_absent"}
LOADER_METHOD = {"C4": "c4_partition", "C5": "c5_diff"}


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def reader_audit(path: Path) -> dict:
    """Verify this driver's actual SQL journey, including attempted writes."""
    rows = [json.loads(line) for line in path.read_text().splitlines() if line.strip()]
    allowed = {"handshake": {""}, "cleanup": {""}, "session": {"SET", "USE"}, "read": {"SELECT", "SHOW"}}
    counts = {}
    for row in rows:
        kind = row.get("class")
        sql = (row.get("sql") or "").strip()
        if ";" in sql.rstrip(";"):
            raise SystemExit("source reader audit contains multiple statements")
        words = sql.split()
        verb = words[0].upper() if words else ""
        if row.get("role") != "business" or kind not in allowed or verb not in allowed[kind]:
            raise SystemExit("source reader audit has an unapproved statement")
        counts[kind] = counts.get(kind, 0) + 1
    if not counts.get("handshake") or not counts.get("read"):
        raise SystemExit("source reader audit lacks a real read session")
    return {"path": str(path), "sha256": sha256(path), "records": len(rows),
            "classes": counts, "write_attempts": 0,
            "verified_by": "StarRocks source consumer SQL allowlist; admission binds capturer bytes"}


def cell(value: object) -> str:
    if value is None:
        return r"\N"
    return str(value)


def arrow_column(field: dict) -> dict:
    spelled = field["type"]
    column = {
        "name": field["name"],
        "logical_name": field["name"],
        "nullable": field["nullable"],
    }
    if spelled == "int64":
        column["arrow_type"] = "int64"
        column["precision"] = 64
    elif spelled == "utf8":
        column["arrow_type"] = "utf8"
    elif spelled == "date32":
        column["arrow_type"] = "date32"
    elif spelled.startswith("decimal128(") and spelled.endswith(")"):
        precision, scale = spelled[len("decimal128(") : -1].split(",")
        column["arrow_type"] = "decimal128"
        column["precision"] = int(precision)
        column["scale"] = int(scale)
    else:
        column["arrow_type"] = spelled
    return column


def type_row(table: str, column: dict) -> dict:
    row = {"table": table, "name": column["name"], "arrow_type": column["arrow_type"]}
    if "precision" in column:
        row["precision"] = column["precision"]
    if "scale" in column:
        row["scale"] = column["scale"]
    return row


def binary_mounts() -> list[str]:
    mounts = []
    for path in (VALIDATOR, PIPELINE):
        if REPO.resolve() not in path.parents:
            mounts.extend(["-v", f"{path}:{path}:ro"])
    return mounts


def docker(args: list[str], stdin: bytes | None = None) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(
        [
            "docker",
            "run",
            "--rm",
            "--name",
            "oo-dl01w1-45-seal-witness",
            "--platform",
            "linux/arm64",
            "--pull",
            "never",
            "-i",
            "-v",
            f"{REPO}:{REPO}:ro",
            "-v",
            f"{OUT}:{OUT}",
            "-v",
            "/tmp/oo-w45-native-pg.pw:/tmp/oo-w45-native-pg.pw:ro",
            *binary_mounts(),
            IMAGE,
            *args,
        ],
        input=stdin,
        capture_output=True,
        check=False,
    )


def run_phase(name: str, args: list[str], stdin: bytes | None = None) -> dict:
    phase = OUT / "phases" / name
    phase.mkdir(parents=True, exist_ok=True)
    result = docker(args, stdin)
    (phase / "stdout").write_bytes(result.stdout)
    (phase / "stderr").write_bytes(result.stderr)
    (phase / "exit").write_text(str(result.returncode) + "\n", encoding="utf-8")
    return {
        "exit": result.returncode,
        "stdout": result.stdout.decode("utf-8", "replace").strip(),
        "stderr": result.stderr.decode("utf-8", "replace").strip(),
    }


def main() -> int:
    global OUT, VALIDATOR, PIPELINE, VALIDATOR_SHA, PIPELINE_SHA, GENERATION, SNAPSHOT
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--capture-stage", choices=("baseline", "catchup", "slow", "expired"), default="catchup")
    parser.add_argument("--source-generation", type=int, default=GENERATION)
    parser.add_argument("--snapshot-label", default=SNAPSHOT)
    parser.add_argument("--seal-only", action="store_true", help="seal source witnesses without invoking the importer")
    parser.add_argument("--out", type=Path, default=OUT)
    parser.add_argument("--validator", type=Path, default=VALIDATOR)
    parser.add_argument("--validator-sha256", default=VALIDATOR_SHA)
    parser.add_argument("--pipeline", type=Path, default=PIPELINE)
    parser.add_argument("--pipeline-sha256", default=PIPELINE_SHA)
    args = parser.parse_args()
    GENERATION = args.source_generation
    SNAPSHOT = args.snapshot_label
    if GENERATION < 1 or not SNAPSHOT or len(SNAPSHOT) > 128:
        raise SystemExit("invalid source generation or snapshot label")
    stage = args.capture_stage
    OUT = args.out.resolve()
    VALIDATOR = args.validator.resolve()
    PIPELINE = args.pipeline.resolve()
    VALIDATOR_SHA = args.validator_sha256
    PIPELINE_SHA = args.pipeline_sha256
    if PRODUCT.resolve() not in OUT.parents:
        raise SystemExit("output must remain in the owned StarRocks product evidence")
    if OUT == CATCHUP.resolve() or CATCHUP.resolve() in OUT.parents:
        raise SystemExit("output must not overwrite the immutable capture")
    if OUT.exists():
        raise SystemExit(f"output exists: {OUT}; choose a fresh run directory")
    for name, digest in (("validator", VALIDATOR_SHA), ("pipeline", PIPELINE_SHA)):
        if len(digest) != 64 or any(c not in "0123456789abcdef" for c in digest):
            raise SystemExit(f"invalid {name} sha256")
    OUT.mkdir(parents=True, exist_ok=True)
    tick = json.loads((CATCHUP / f"ticks/{stage}.json").read_text(encoding="utf-8"))
    audit_path = CATCHUP / f"reads/{stage}/audit.jsonl"
    source_audit = reader_audit(audit_path)
    driver_sha = sha256(DRIVER)
    validator_sha = sha256(VALIDATOR)
    pipeline_sha = sha256(PIPELINE)
    if validator_sha != VALIDATOR_SHA or pipeline_sha != PIPELINE_SHA:
        raise SystemExit("fixed ELF sha changed")
    tables = []
    schema_preimage = []
    type_preimage = []
    capturer_tables = []
    for name, fact in tick["tables"].items():
        decoded_path = CATCHUP / f"decoded/{stage}" / f"{name}.json"
        arrow_path = CATCHUP / f"reads/{stage}" / f"{name}.arrow"
        decoded = json.loads(decoded_path.read_text(encoding="utf-8"))
        columns = [arrow_column(field) for field in decoded["fields"]]
        schema_preimage.append({"table": name, "columns": columns})
        type_preimage.extend(type_row(name, column) for column in columns)
        quality = fact["quality"]
        token = quality[0] if isinstance(quality, list) and quality else None
        if fact["delete_detection"] not in DELETE_OK:
            raise SystemExit(f"{name} delete token is not sealable")
        capturer_tables.append(
            {
                "name": name,
                "arrow": arrow_path.name,
                "route": fact["method"],
                "consistency": fact["grade"],
                "delete_mode": fact["delete_detection"],
                "cursor_safety": "no_cursor",
                "quality": token,
                "quality_recorded": quality,
                "key_model": fact["model"],
                "rows": [
                    "\x1f".join(cell(row[field["name"]]) for field in decoded["fields"])
                    for row in decoded["rows"]
                ],
            }
        )
        tables.append(
            {
                "name": name,
                "method": fact["method"],
                "grade": fact["grade"],
                "delete_detection": fact["delete_detection"],
                "quality": token,
                "model": fact["model"],
                "arrow": str(arrow_path),
                "arrow_magic": arrow_path.read_bytes()[:6].hex(),
                "decoded_rows": len(decoded["rows"]),
            }
        )
    inputs = OUT / "inputs"
    capturer = {
        "protocol": "oo.starrocks.capture-tick/v1",
        "source_engine": "starrocks",
        "source_id": SOURCE_ID,
        "source_generation": GENERATION,
        "label": SNAPSHOT,
        "database": SCHEMA,
        "product_reader_write_attempts": 0,
        "write_observation": "source reader audit independently checked by the source consumer",
        "source_reader_audit": source_audit,
        "snapshot_token": False,
        "snapshot_token_basis": f"{stage} finished read has no cross-table snapshot token",
        "source_tick": f"raw/capture-schedule/ticks/{stage}.json",
        "tables": capturer_tables,
    }
    write_json(inputs / "capturer.json", capturer)
    write_json(inputs / "schema-preimage.json", schema_preimage)
    write_json(inputs / "type-preimage.json", type_preimage)
    config = {
        "protocol": "oo.source-admission.config/v1",
        "target_id": SOURCE_ID,
        "source_id": SOURCE_ID,
        "secret_ref": "secret://oo-dl01w1-45/reader",
        "compat_mode": "starrocks-native",
        "permissions": [{"name": "SELECT", "status": "known", "granted": True}],
        "budget": {"unit": "Rows", "hard_limit": 1000, "confidence": "unknown"},
        "write_options": {
            "signal_table": False,
            "heartbeat_write": False,
            "create_stream": False,
            "create_trigger": False,
        },
        "debezium": {
            "signal_data_collection": None,
            "heartbeat_action_query": None,
            "snapshot_mode": "never",
            "create_stream": False,
            "create_trigger": False,
        },
        "requested_write_class": "R0",
        "r1w": None,
    }
    write_json(inputs / "config.json", config)
    for table in tables:
        witness = {
            "protocol": "oo.source-admission.capture-witness/v1",
            "source_engine": "starrocks",
            "source_id": SOURCE_ID,
            "source_generation": GENERATION,
            "snapshot": {"kind": "capture-tick", "value": SNAPSHOT},
            "run_id": f"sr_w45-{stage}-{SNAPSHOT}",
            "logical_schema": SCHEMA,
            "logical_table": table["name"],
            "capturer_path": str(inputs / "capturer.json"),
            "arrow_path": table["arrow"],
            "driver_path": str(DRIVER),
            "schema_preimage_path": str(inputs / "schema-preimage.json"),
            "type_preimage_path": str(inputs / "type-preimage.json"),
        }
        report = {
            "protocol": "oo.source-admission.report/v1",
            "target_id": SOURCE_ID,
            "source_id": SOURCE_ID,
            "secret_ref": "secret://oo-dl01w1-45/reader",
            "driver": {
                "kind": "native",
                "name": "starrocks-probe",
                "version": "0.1.0",
                "digest": driver_sha,
            },
            "compat_mode": "starrocks-native",
            "license_flags": ["Apache-2.0", "MIT"],
            "permissions": [{"name": "SELECT", "status": "known", "granted": True}],
            "capabilities": [
                {
                    "name": "BatchFull",
                    "cursor": None,
                    "feed": None,
                    "cdc": None,
                    "token_order": None,
                    "budget_unit": None,
                    "level": "experimental",
                    "write_class": "R0",
                    "consistency": table["grade"],
                }
            ],
            "capture_methods": [],
            "cost_model": {
                "unit": "Rows",
                "confidence": "unknown",
                "estimate": "catchup tick t2 uses the existing Arrow row count. StarRocks published no billing unit.",
            },
            "r1w": None,
            "capture_witness": str(inputs / f"witness-{table['name']}.json"),
        }
        write_json(inputs / f"witness-{table['name']}.json", witness)
        write_json(inputs / f"report-{table['name']}.json", report)
        table["witness"] = str(inputs / f"witness-{table['name']}.json")
        table["report"] = str(inputs / f"report-{table['name']}.json")

    image = subprocess.run(
        ["docker", "image", "inspect", "--format", "{{.Id}} {{.Os}}/{{.Architecture}}", IMAGE],
        capture_output=True,
        text=True,
        check=False,
    )
    (OUT / "image.txt").write_text(image.stdout + image.stderr, encoding="utf-8")
    if image.returncode != 0:
        raise SystemExit("pinned rust image is absent")
    # seal-native requires the parent of --out to exist and refuses an existing out dir.
    (OUT / "sealed").mkdir(parents=True, exist_ok=True)
    sealed = []
    for table in tables:
        destination = OUT / "sealed" / table["name"]
        if destination.exists():
            raise SystemExit(f"output exists: {destination}")
        phase = run_phase(
            f"seal-{table['name']}",
            [
                str(VALIDATOR),
                "seal-native",
                "--report",
                table["report"],
                "--config",
                str(inputs / "config.json"),
                "--out",
                str(destination),
            ],
        )
        decision = None
        decision_path = destination / "decision.json"
        if decision_path.is_file():
            decision = json.loads(decision_path.read_text(encoding="utf-8"))
        elif phase["stdout"]:
            try:
                parsed = json.loads(phase["stdout"])
            except json.JSONDecodeError:
                parsed = None
            if isinstance(parsed, dict) and "admitted" in parsed:
                decision = parsed
        sealed.append(
            {
                "table": table["name"],
                "exit": phase["exit"],
                "stderr": phase["stderr"],
                "admitted": bool(decision and decision.get("admitted") is True),
                "blocks": [] if decision is None else decision.get("blocks", []),
                "out": str(destination) if destination.is_dir() else None,
            }
        )
    admitted = [item for item in sealed if item["admitted"]]
    chosen = next((item for item in admitted if item["table"] == "uniq_dim"), None)
    if chosen is None and admitted:
        chosen = admitted[0]
    calls = {}
    if chosen is not None and not args.seal_only:
        receipt_path = Path(chosen["out"]) / "admission-receipt.json"
        registry_path = Path(chosen["out"]) / "registry.json"
        receipt = json.loads(receipt_path.read_text(encoding="utf-8"))
        request = {
            "protocol": "oo.pipeline.native-update/v1",
            "admission_path": str(receipt_path),
            "registry_path": str(registry_path),
            "arrow_path": next(item["arrow"] for item in tables if item["name"] == chosen["table"]),
            "applied_pos": "0",
            # The importer binds this field to the validated producer receipt,
            # not to the separate source SQL audit stream.
            "audit_digest": sha256(inputs / "capturer.json"),
            "batch_id": f"{stage}-{SNAPSHOT}",
            "capture_method": LOADER_METHOD[receipt["capture_method"]],
            "columns": next(
                row["columns"] for row in schema_preimage if row["table"] == chosen["table"]
            ),
            "config_digest": receipt["config_digest"],
            "consistency_grade": receipt["consistency"],
            "cross_table_consistent": False,
            "delete_detection": receipt["delete_detection"],
            "full_snapshot": True,
            "input_digest": receipt["input_digest"],
            "key_column": "dim_id" if chosen["table"] == "uniq_dim" else None,
            "logical_schema": SCHEMA,
            "logical_table": chosen["table"],
            "mirror": False,
            "parent_source_generation": None,
            "position_codec": "snapshot-zero/v1",
            "position_lineage": "native-update.batch",
            "schema_digest": receipt["schema_digest"],
            "snapshot": receipt["snapshot"],
            "source_engine": "starrocks",
            "source_generation": GENERATION,
            "source_id": SOURCE_ID,
            "target": {
                "host": "127.0.0.1",
                "port": 15446,
                "database": "oo_w45_seal",
                "runtime_principal": "postgres",
                "password_file": "/tmp/oo-w45-native-pg.pw",
                "ssl": "disable",
                "shard": "shard_w45",
                "owner": "6813d023-51e7-6725-14b3-6cb93a9b24cf",
            },
            "tombstones": [],
            "type_digest": receipt["type_digest"],
            "upsert_supported": False,
            "window_closed": True,
        }
        if request["key_column"] is None:
            del request["key_column"]
        request_path = OUT / "calls/import-native-update.json"
        write_json(request_path, request)
        calls["import-native-update"] = run_phase(
            "import-native-update",
            [str(PIPELINE), "import-native-update"],
            stdin=(request_path.read_text(encoding="utf-8")).encode("utf-8"),
        )
        catalog_request = {
            "protocol": "oo.pipeline.native-sealed-catalog/v1",
            # closed_uuid requires a lowercase hex UUID. This identifies the
            # catalog call. The capture run id remains sr_w45-catchup-t2.
            "run_id": "f082d8e8-c487-48de-b44c-ea3fc9cdcc9f",
            "source_id": SOURCE_ID,
            "owner": "6813d023-51e7-6725-14b3-6cb93a9b24cf",
            "source_generation": GENERATION,
            "target_generation": 1,
            "config_digest": receipt["config_digest"],
            "schema_digest": receipt["schema_digest"],
            "type_digest": receipt["type_digest"],
            "input_digest": receipt["input_digest"],
            "snapshot": receipt["snapshot"],
            "target": {
                "host": request["target"]["host"],
                "port": request["target"]["port"],
                "database": request["target"]["database"],
                "runtime_principal": request["target"]["runtime_principal"],
                "password_file": request["target"]["password_file"],
                "ca_file": str(OUT / "calls/unused-ca.pem"),
            },
        }
        # resolve_sealed reads the validator from the environment. The CA file
        # is only opened after that check. A missing CA must not be created
        # as a certificate.
        catalog_path = OUT / "calls/catalog-import-native.json"
        write_json(catalog_path, catalog_request)
        phase = OUT / "phases/catalog-import-native"
        phase.mkdir(parents=True, exist_ok=True)
        env_run = subprocess.run(
            [
                "docker",
                "run",
                "--rm",
                "--name",
                "oo-dl01w1-45-seal-witness",
                "--platform",
                "linux/arm64",
                "--pull",
                "never",
                "-i",
                "-e",
                f"OO_NATIVE_ADMISSION_BIN={VALIDATOR}",
                "-e",
                f"OO_NATIVE_BINDING_REGISTRY={registry_path}",
                "-e",
                f"OO_NATIVE_BINDING_REGISTRY_SHA256={sha256(registry_path)}",
                "-v",
                f"{REPO}:{REPO}:ro",
                "-v",
                f"{OUT}:{OUT}",
                "-v",
                "/tmp/oo-w45-native-pg.pw:/tmp/oo-w45-native-pg.pw:ro",
                *binary_mounts(),
                IMAGE,
                str(PIPELINE),
                "catalog-import-native",
            ],
            input=catalog_path.read_bytes(),
            capture_output=True,
            check=False,
        )
        (phase / "stdout").write_bytes(env_run.stdout)
        (phase / "stderr").write_bytes(env_run.stderr)
        (phase / "exit").write_text(str(env_run.returncode) + "\n", encoding="utf-8")
        calls["catalog-import-native"] = {
            "exit": env_run.returncode,
            "stdout": env_run.stdout.decode("utf-8", "replace").strip(),
            "stderr": env_run.stderr.decode("utf-8", "replace").strip(),
        }
    summary = {
        "protocol": "oo.starrocks.seal-witness/v1",
        "seal_only": args.seal_only,
        "source_reader_audit": source_audit,
        "driver_sha256": driver_sha,
        "driver_bytes": DRIVER.stat().st_size,
        "validator_sha256": validator_sha,
        "pipeline_sha256": pipeline_sha,
        "audit_sha256": sha256(audit_path),
        "tables": tables,
        "sealed": sealed,
        "calls": calls,
        "paused_container_started": False,
        "green_imports_rerun": False,
        "lease_inserted": False,
    }
    write_json(OUT / "summary.json", summary)
    print(json.dumps({"sealed": sealed, "calls": calls}, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
