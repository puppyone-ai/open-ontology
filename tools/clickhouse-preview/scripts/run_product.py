#!/usr/bin/env python3
"""ClickHouse controlled-preview orchestration for the current sealed API.

Calls admission, `pg-builder import-seal`, one `pg-builder sealed` on the
faithful catalog, and one `pg-builder import` probe. It does not call
`generate`, `import-cards`, or `compile_captured_pg_catalog`. It does not
invent semantic decisions, a role pack, or an HTTP status.
The declared price is the e4f MySQL native-loop approved test config, used
as a declared estimate. Exit 0 means admission, the faithful compile, a
produced pack, and the physical import all returned 0, and the typed
DateTime64 and array catalogs were refused.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
from pathlib import Path

PROTOCOL = "oo.pipeline.sealed-import-catalog/v1"
RUN_ID = "4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de"
SOURCE_ID = "clickhouse_268"
ENGINE = "clickhouse"


def sha256_text(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def shadow(prefix: str, label: str) -> str:
    return prefix + sha256_text(label)[:60]


def numeric_typmod(precision: int, scale: int) -> int:
    return ((precision << 16) | scale) + 4


def column(name: str, oid: int, typmod: int, source_type: str, key: bool, caveat: str | None) -> dict:
    body = {
        "caveat": caveat,
        "key": key,
        "logical_name": name,
        "oid": oid,
        "source_type": source_type,
        "stored_physical": shadow("pc_", f"col:{name}"),
        "typmod": typmod,
    }
    return body


def schema_preimage(columns: list[dict]) -> str:
    lines = ["readings.final\n"]
    for item in columns:
        caveat = item["caveat"] or ""
        lines.append(f"{item['logical_name']}\t{item['oid']}\t{item['typmod']}\t{caveat}\n")
    return "".join(lines)


# Source identity recorded on the attempt-06 capture. schema_digest is not one
# of these: it is the sealed oid preimage and changes when a target oid changes.
CONFIG_DIGEST = "435ac4a5d24920afab3343cb9de50a7e71d53ad107b35205cd686b596ea3b64a"
INPUT_DIGEST = "007be53b723c87abb6594871ae082fe14d67c5b01aa192ff26c2c2c6371865cd"
TYPE_DIGEST = "b935b6a9eb5f3384060ddaae7c588b6718933b644c0b8814d7deb467fe39bbb1"


def catalog(columns: list[dict], accepted_rows: int) -> dict:
    table_shadow = shadow("pt_", "table:readings.final")
    return {
        "capture": "sealed_import",
        "config_digest": CONFIG_DIGEST,
        "engine": ENGINE,
        "hook_checked": False,
        "input_digest": INPUT_DIGEST,
        "protocol": PROTOCOL,
        "quality": {
            "loss_concealed": False,
            "manifest_reject_count": 0,
            "payload_includes_reject_lines": False,
            "stored_omits_manifest_rejects": False,
            "stored_rejected_rows": 0,
            "stored_rows": [
                {
                    "accepted_rows": accepted_rows,
                    "attnum": None,
                    "batch_id": "t0.b0",
                    "key_digest": None,
                    "kind": "batch",
                    "physical_name": table_shadow,
                    "reason": None,
                    "rejected_rows": 0,
                    "row_index": None,
                    "value_len": None,
                }
            ],
        },
        "run_id": RUN_ID,
        "schema_digest": sha256_text(schema_preimage(columns)),
        "shadows_queried": False,
        "source_generation": 1,
        "source_id": SOURCE_ID,
        "source_introspection": "not_used",
        "state": "awaiting_publish",
        "tables": [
            {
                "accepted_rows": accepted_rows,
                "canonical_digest": sha256_text("oo-dl01w1-40:table"),
                "columns": columns,
                "logical_schema": "readings",
                "logical_table": "final",
                "rejects": [],
                "stored_namespace": shadow("pn_", "ns:readings"),
                "stored_physical": table_shadow,
            }
        ],
        "target_generation": 1,
        "tool_hook_digest": sha256_text("oo-dl01w1-40:hook"),
        "type_digest": TYPE_DIGEST,
    }


def faithful_columns() -> list[dict]:
    u64 = numeric_typmod(20, 0)
    decimal = numeric_typmod(76, 10)
    return [
        column("reading_id", 1700, u64, "uint64", True, "bigint_unsigned_numeric"),
        column("version", 1700, u64, "uint64", False, "bigint_unsigned_numeric"),
        column("amount", 1700, decimal, "decimal76_10", False, None),
        column("note", 17, -1, "nullable_string", False, None),
        column("code", 17, -1, "low_cardinality_string", False, None),
        column("code_or_null", 17, -1, "low_cardinality_nullable_string", False, None),
        column("qty", 20, -1, "int64", False, None),
        column("label", 17, -1, "string", False, None),
        column("wide_u64", 1700, u64, "uint64", False, "bigint_unsigned_numeric"),
    ]


def write_json(path: Path, body: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(body, indent=2) + "\n", encoding="utf-8")


def run(cmd: list[str], cwd: Path | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, cwd=cwd, text=True, capture_output=True)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--driver-digest", required=True)
    parser.add_argument("--gold", required=True, type=Path)
    parser.add_argument("--out-dir", required=True, type=Path)
    parser.add_argument("--preview-bin", required=True)
    parser.add_argument("--pg-builder", required=True)
    parser.add_argument("--bearer-file", required=True)
    parser.add_argument("--upstream-status", type=Path)
    parser.add_argument("--arrow", type=Path)
    parser.add_argument("--builder-pin", default="e4f9131c6c3634e30f5f73b6b3db19ab7aa06f7d")
    parser.add_argument("--admission-pin", default="5df40d24eefa4fbcfa00c4415cfcfde5d368db89")
    args = parser.parse_args()
    if "password" in args.driver_digest.lower() or "://" in args.driver_digest:
        print("clickhouse-preview: driver digest is empty or looks like a secret", file=sys.stderr)
        return 2
    gold = json.loads(args.gold.read_text(encoding="utf-8"))
    if not isinstance(gold, list) or not gold:
        print("clickhouse-preview: gold snapshot is empty", file=sys.stderr)
        return 2
    out = args.out_dir
    out.mkdir(parents=True, exist_ok=True)
    rows = len(gold)
    faithful = catalog(faithful_columns(), rows)
    datetime_columns = faithful_columns()[:1] + [
        column("ts", 1184, 9, "datetime64_9_utc", False, None)
    ]
    array_columns = faithful_columns()[:1] + [
        column("tags", 2277, -1, "array_int64", False, None),
        column("labels", 2277, -1, "array_nullable_string", False, None),
    ]
    write_json(out / "catalog-faithful.json", faithful)
    write_json(out / "catalog-datetime64.json", catalog(datetime_columns, rows))
    write_json(out / "catalog-array.json", catalog(array_columns, rows))
    decision = out / "admission-decision.json"
    admitted = run(
        [
            args.preview_bin,
            "--driver-digest",
            args.driver_digest,
            "--out",
            str(decision),
        ]
    )
    (out / "admission.exit").write_text(str(admitted.returncode) + "\n", encoding="utf-8")
    if admitted.stderr:
        (out / "admission.stderr").write_text(admitted.stderr, encoding="utf-8")
    seals = {}
    for name in ("faithful", "datetime64", "array"):
        proc = run(
            [
                args.pg_builder,
                "import-seal",
                "--catalog",
                str(out / f"catalog-{name}.json"),
                "--out",
                str(out / f"seal-{name}.json"),
            ]
        )
        seals[name] = proc.returncode
        (out / f"seal-{name}.exit").write_text(str(proc.returncode) + "\n", encoding="utf-8")
        if proc.stderr:
            (out / f"seal-{name}.stderr").write_text(proc.stderr, encoding="utf-8")
    settings = {
        "catalog_version": "pending",
        "knowledge": {"id": "know-clickhouse", "version": "1"},
        "ontology": {
            "classification": {"auto_accept": "rule_high_confidence"},
            "compile": {
                "budgets": {"readme_tokens": 2000, "table_file_tokens": 4000},
                "per_role": True,
                "sample_values": {"default": "masked", "max_per_column": 0},
            },
            "default_tier_preference": ["raw"],
            "include_schemas": ["readings"],
            "lifecycle": {"deprecation_min_releases": 2},
            "locale": "en-US",
            "name": "clickhouse",
            "publish": {"include_unreviewed": []},
            "spec_version": "oos/v1",
        },
        "ontology_version": "ont-1",
        "policy": {"id": "pol-clickhouse", "version": "1"},
        "project": SOURCE_ID,
    }
    visibility = {
        "catalog_version": "pending",
        "datasets": [
            {
                "columns": [item["logical_name"] for item in faithful_columns()],
                "dataset": "readings.final",
                "global_stats": "denied",
                "row_security": "none",
            }
        ],
        "issued_for_compiler": "oo-builder/role-pack/v1",
        "knowledge": {"id": "know-clickhouse", "version": "1"},
        "limits": {"max_entries": 128, "max_file_bytes": 262144, "max_total_bytes": 1048576},
        "ontology_version": "ont-1",
        "policy": {"id": "pol-clickhouse", "version": "1"},
        "principal": "operator",
        "project": SOURCE_ID,
        "protocol": "oo.builder.role-visibility/v1",
        "role": "oo_r_reader",
    }
    write_json(out / "settings.json", settings)
    write_json(out / "visibility.json", visibility)
    config = {
        "protocol": "oo.pg-builder.config/v1",
        "adapter": "loopback_http_local_dev",
        "namespace": "pg-builder",
        "purpose": "generate",
        "sample_policy": "none",
        "temperature": 0,
        "endpoint": {
            "origin": "http://127.0.0.1:8317",
            "path": "/v1/chat/completions",
            "model_id": "grok-4.7",
        },
        "secret": {"bearer_file": args.bearer_file},
        # Same object as evidence/.../mysql/native-loop/raw/config-live.json at e4f.
        # status declared_non_billing is an estimate. It is not a billing certificate.
        "declared_price": {
            "currency": "USD",
            "per_total_token": "0.000001",
            "status": "declared_non_billing",
        },
        "budget": {
            "deadline_ms": 120000,
            "max_request_bytes": 262144,
            "max_response_bytes": 262144,
            "max_prompt_tokens": 100000,
            "max_completion_tokens": 16000,
            "max_total_tokens": 200000,
            "max_tables": 16,
            "max_in_flight": 1,
        },
    }
    write_json(out / "builder-config.json", config)
    builder_out = out / "builder"
    cache = out / "builder-cache"
    builder_out.mkdir(parents=True, exist_ok=True)
    cache.mkdir(parents=True, exist_ok=True)
    result_body: dict = {}
    if seals.get("faithful") != 0:
        (out / "builder.exit").write_text("not_called\n", encoding="utf-8")
        (out / "builder.stderr").write_text(
            "sealed withheld: faithful import-seal did not return draft_compiled\n",
            encoding="utf-8",
        )
        built_code: int | None = None
    else:
        built = run(
            [
                args.pg_builder,
                "sealed",
                "--config",
                str(out / "builder-config.json"),
                "--catalog",
                str(out / "catalog-faithful.json"),
                "--settings",
                str(out / "settings.json"),
                "--visibility",
                str(out / "visibility.json"),
                "--cache",
                str(cache),
                "--out",
                str(builder_out),
                "--reviewer",
                "oo-dl01w1-40",
                "--reviewed-at",
                "2026-10-10",
                "--dataset",
                "readings.final",
            ]
        )
        built_code = built.returncode
        (out / "builder.exit").write_text(str(built.returncode) + "\n", encoding="utf-8")
        if built.stdout:
            (out / "builder.stdout").write_text(built.stdout, encoding="utf-8")
        if built.stderr:
            (out / "builder.stderr").write_text(built.stderr, encoding="utf-8")
        result_path = builder_out / "result.json"
        if result_path.is_file():
            try:
                loaded = json.loads(result_path.read_text(encoding="utf-8"))
            except json.JSONDecodeError:
                loaded = {}
            if isinstance(loaded, dict):
                result_body = loaded
    import_code, import_message = run_import_probe(args, out)
    write_loader_input(args, out, faithful)
    http_status, http_source = read_http_status(args.upstream_status, result_body)
    model = result_body.get("model") if isinstance(result_body.get("model"), str) else "not_called"
    pack = result_body.get("pack") if isinstance(result_body.get("pack"), str) else "not_produced"
    outcome = result_body.get("outcome") if isinstance(result_body.get("outcome"), str) else None
    socket = result_body.get("socket") if isinstance(result_body.get("socket"), str) else None
    http_attempted = http_status is not None or model in ("called", "cache_hit") or socket == "opened"
    if model == "not_called" and http_status is None and socket != "opened":
        http_attempted = False
    summary = {
        "admission_exit": admitted.returncode,
        "admission_pin": args.admission_pin,
        "builder_pin": args.builder_pin,
        "gold_rows": rows,
        "seal_exit": seals,
        "builder_exit": built_code,
        "builder_outcome": outcome,
        "builder_model": model,
        "builder_pack": pack,
        "builder_socket": socket,
        "http_attempted": http_attempted,
        "http_status": http_status,
        "http_status_source": http_source,
        "import_exit": import_code,
        "import_message": import_message,
        "generate": "not_called",
        "import_cards": "not_called",
        "role_pack": pack,
        "operator_query": "not_run",
        "physical_import": "withheld_mysql_manifest",
        "billing_certificate": False,
    }
    write_json(out / "orchestrator-summary.json", summary)
    write_json(
        out / "productmodel-provider-gate.json",
        {
            "protocol": "oo.productmodel-provider-gate/v1",
            "task_id": "DL01-W1-40",
            "source_id": SOURCE_ID,
            "builder": "pg-builder",
            "builder_pin": args.builder_pin,
            "admission_pin": args.admission_pin,
            "command": "sealed",
            "endpoint": {
                "origin": "http://127.0.0.1:8317",
                "path": "/v1/chat/completions",
                "model_id": "grok-4.7",
            },
            "transport": "container_loopback_forwarded_to_host.docker.internal:8317",
            "token_file_present": Path(args.bearer_file).is_file(),
            "token_in_argv": False,
            "http_attempted": http_attempted,
            "http_status": http_status,
            "http_status_source": http_source,
            "result_http_status": result_body.get("http_status"),
            "model": model,
            "pack": pack,
            "outcome": outcome,
            "socket": socket,
            "reason": result_body.get("reason"),
            "sealed_subcommand_exit": built_code,
            "import_seal_exit": seals,
            "import_exit": import_code,
            "generate": "not_called",
            "import_cards": "not_called",
        },
    )
    print(json.dumps(summary))
    typed_refused = seals.get("datetime64") == 1 and seals.get("array") == 1
    packed = pack == "produced" and built_code == 0
    blocked = (
        admitted.returncode != 0
        or seals.get("faithful") != 0
        or not typed_refused
        or not packed
        or import_code != 0
    )
    return 1 if blocked else 0


def read_http_status(path: Path | None, result_body: dict) -> tuple[int | None, str]:
    if path is not None and path.is_file():
        text = path.read_text(encoding="utf-8").strip()
        if text.isdigit():
            return int(text), "forwarder"
        if text in ("unavailable", "not_attempted"):
            return None, text
    raw = result_body.get("http_status")
    if isinstance(raw, int):
        return raw, "result.json"
    return None, "absent"


def write_loader_input(args: argparse.Namespace, out: Path, catalog_body: dict) -> None:
    """Record the real Arrow payload beside the sealed catalog. Do not load it."""
    body: dict = {
        "source_id": SOURCE_ID,
        "engine": ENGINE,
        "catalog_protocol": PROTOCOL,
        "catalog_file": "catalog-faithful.json",
        "schema_digest": catalog_body["schema_digest"],
        "run_id": catalog_body["run_id"],
        "physical_import": "not_called",
        "blocker": (
            "tools/pg-builder import requires oo.mysql-preview.manifest/v1 before spawn. "
            "crates/oo-pipeline catalog-import requires engine mysql. "
            "No generic Arrow physical loader is in the committed tree. "
            "This file does not seed rows and does not mint a seal."
        ),
    }
    if args.arrow is not None and args.arrow.is_file():
        payload = args.arrow.read_bytes()
        target = out / "arrow-final.arrow"
        target.write_bytes(payload)
        body["arrow_file"] = "arrow-final.arrow"
        body["arrow_sha256"] = hashlib.sha256(payload).hexdigest()
        body["arrow_bytes"] = len(payload)
        body["arrow_source"] = (
            "evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/"
            "clickhouse-driver-preflight/raw/attempt-06/ipc/oo-dl01w1-40-arrow-final.arrow"
        )
    write_json(out / "loader-inputs.json", body)


def run_import_probe(args: argparse.Namespace, out: Path) -> tuple[int, str]:
    """Ask run_import to load the ClickHouse sealed catalog. Do not retag it as MySQL."""
    manifest = out / "import-manifest.json"
    manifest.write_text((out / "catalog-faithful.json").read_text(encoding="utf-8"), encoding="utf-8")
    request = {
        "protocol": "oo.pg-builder.import-request/v1",
        "source_id": SOURCE_ID,
        "config_digest": sha256_text("oo-dl01w1-40:import-config"),
        "source_generation": 1,
        "manifest_file": str(manifest),
        "catalog_bin": "/work/no-catalog-import-bin",
        "target": {
            "host": "127.0.0.1",
            "port": 5432,
            "database": "unused",
            "runtime_principal": "unused",
            "password_file": "/work/absent-target-password",
            "ca_file": "/work/absent-target-ca",
        },
    }
    request_path = out / "import-request.json"
    write_json(request_path, request)
    proc = run(
        [
            args.pg_builder,
            "import",
            "--request",
            str(request_path),
            "--out",
            str(out / "import-out.json"),
            "--stderr",
            str(out / "import.stderr"),
        ]
    )
    (out / "import.exit").write_text(str(proc.returncode) + "\n", encoding="utf-8")
    message = proc.stderr.strip()
    stderr_path = out / "import.stderr"
    if stderr_path.is_file() and stderr_path.stat().st_size:
        extra = stderr_path.read_text(encoding="utf-8", errors="replace").strip()
        if extra:
            message = extra
    if proc.stdout:
        (out / "import.stdout").write_text(proc.stdout, encoding="utf-8")
    if message and not stderr_path.is_file():
        stderr_path.write_text(message + "\n", encoding="utf-8")
    return proc.returncode, message.splitlines()[-1] if message else ""


if __name__ == "__main__":
    sys.exit(main())
