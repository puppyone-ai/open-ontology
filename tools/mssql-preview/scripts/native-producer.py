#!/usr/bin/env python3
"""Source-owned C2 producer. Runs real driver, reads target CAS state, seals actual inputs.
No source enablement, target import, lease creation, trust/self-admission, or local position ACK.
Context is operator-supplied private configuration, not a jobs payload.
"""
import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import uuid


def reject_inline_secrets(value):
    if isinstance(value, dict):
        for key, child in value.items():
            if key.lower() in ("password", "passwd", "pwd", "sa_password", "mssql_sa_password", "secret"):
                raise ValueError("inline_secret")
            reject_inline_secrets(child)
    elif isinstance(value, list):
        for child in value:
            reject_inline_secrets(child)


def read(path):
    return json.loads(Path(path).read_bytes())


def write(path, value):
    data = json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "wb") as f:
        f.write(data)
        f.flush()
        os.fsync(f.fileno())
    return str(Path(path).resolve())


def sha(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def executable(item):
    path = Path(item["path"])
    if not path.is_absolute() or sha(path) != item["sha256"]:
        raise ValueError("executable_pin")
    return str(path)


def call(binary, args, directory, name):
    # No secret values are passed or returned; diagnostics remain in private artifacts.
    result = subprocess.run([binary, *args], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    for suffix, value in (("stdout", result.stdout), ("stderr", result.stderr), ("exit", str(result.returncode).encode())):
        path = directory / (name + "." + suffix)
        fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(fd, "wb") as f:
            f.write(value)
    if result.returncode:
        raise ValueError(name + "_failed")
    return result.stdout


def classify_audit(raw, reader, application):
    if raw.get("protocol") != "oo.mssql-preview.raw-audit/v1" or raw.get("tds_query_executed") is not True:
        raise ValueError("audit_not_observed")
    if raw.get("reader") != reader or raw.get("application_name") != application:
        raise ValueError("audit_identity")
    events = raw.get("events")
    if not isinstance(events, list) or not events:
        raise ValueError("audit_empty")
    forbidden = 0
    reads = 0
    for event in events:
        if event.get("reader") != reader or event.get("application_name") != application:
            raise ValueError("audit_cross_reader")
        # Failed write attempts count too. Unknown audit actions fail closed.
        if event.get("action_id", "").strip() != "SL":
            forbidden += 1
        elif event.get("succeeded") is True:
            reads += 1
    if forbidden or not reads:
        raise ValueError("audit_write_or_unobserved")
    return {"reader_statements": len(events), "successful_selects": reads,
            "product_reader_write_attempts": forbidden, "basis": "sqlserver_audit_file"}


def selected(capture, schema, table):
    rows = [t for t in capture["tables"] if t["schema"] == schema and t["name"] == table]
    if len(rows) != 1 or not rows[0].get("native_apply_supported"):
        raise ValueError("native_key_or_scope_unsupported")
    return rows[0]


def assemble(context, source, capture, directory, raw_audit_path, generation, parent):
    if capture.get("protocol") != "oo.mssql-preview.capture/v1" or capture.get("tds_query_executed") is not True:
        raise ValueError("capture_not_observed")
    if capture["source_id"] != source["source_id"] or capture.get("position_advanced") is not False:
        raise ValueError("capture_identity")
    schema, table = context["object"]["schema"], context["object"]["name"]
    item = selected(capture, schema, table)
    position = next(p for p in capture["candidate_position"]["tables"] if p["schema"] == schema and p["name"] == table)
    arrow_path = directory / "capture" / item["upsert_arrow"]["file"]
    if sha(arrow_path) != item["upsert_arrow"]["ipc_sha256"]:
        raise ValueError("arrow_digest")
    raw = read(raw_audit_path)
    audit = classify_audit(raw, source["endpoint"]["username"], capture["audit_application_name"])
    audit["raw_sha256"] = sha(raw_audit_path)
    audit_path = write(directory / "audit-summary.json", audit)
    ct_facts = {"database": capture["candidate_position"]["database"], "database_guid": capture["candidate_position"]["database_guid"],
                "recovery_fork_guid": capture["candidate_position"]["recovery_fork_guid"], "table": table, "schema": schema,
                "object_id": item["object_id"], "current_version": item["current_version"], "min_valid_version": item["min_valid_version"],
                "snapshot_isolation": capture["snapshot_isolation"], "tds_query_executed": True}
    ct_path = write(directory / "ct-facts.json", ct_facts)
    provenance = read(context["dba_provenance_file"])
    if provenance.get("protocol") != "oo.mssql-preview.dba-provenance/v1" or provenance.get("database") != source["endpoint"]["database"] or provenance.get("object") != context["object"] or provenance.get("created_by") != "dba":
        raise ValueError("dba_provenance_required")
    # Explicit operator provenance is separate from observed CT state and cannot enable it.
    dba_objects = [{"kind": k, "name": n, "created_by": "dba", "observed": True,
                    "platform_created": False, "platform_write_attempts": 0,
                    "risk": provenance["risk"], "rollback": provenance["rollback"],
                    "observation_path": ct_path, "observation_sha256": sha(ct_path)}
                   for k, n in (("ct_database", source["endpoint"]["database"]), ("ct_table", schema + "." + table))]
    snapshot = {"kind": "mssql_ct_version", "value": str(position["version"])}
    capturer = copy.deepcopy(capture)
    capturer.update({"logical_schema": schema, "logical_table": table, "source_database": source["endpoint"]["database"],
                     "source_generation": generation, "snapshot": snapshot, "snapshot_token": False,
                     "product_reader_write_attempts": audit["product_reader_write_attempts"],
                     "audit_observation_path": str(raw_audit_path), "audit_observation_sha256": sha(raw_audit_path), "reader_audit": audit,
                     "dba_objects": dba_objects, "run_id": context["run_id"], "cross_table_consistency": "S1"})
    selected_table = copy.deepcopy(item)
    selected_table.update({"arrow": arrow_path.name, "row_count": item["upserts"], "route": "C2", "consistency": "S1", "delete_mode": "native"})
    # Single-table producer export keeps cursor evidence at its root, as required by
    # the narrow C2 admission binder. Original full manifest stays retained unmodified.
    capturer.pop("tables", None)
    capturer.update(selected_table)
    capturer_path = write(directory / "capturer.json", capturer)
    driver_path = write(directory / "driver.json", {"kind": "native-rust", "name": "tiberius", "version": "0.13.0",
                                                   "crate_sha256": "e07324791de2bdaed058af4aa433a0b5ec5c0beac407a9797419c19805df2415",
                                                   "binary_sha256": context["reader_binary"]["sha256"]})
    columns = item["columns"]
    schema_path = write(directory / "schema-preimage.json", [{"table": table, "columns": columns}])
    type_path = write(directory / "type-preimage.json", [dict(c, table=table, approval="loader") for c in columns])
    witness_path = write(directory / "witness.json", {"protocol": "oo.source-admission.capture-witness/v1",
        "source_engine": "sqlserver", "source_id": source["source_id"], "source_generation": generation,
        "snapshot": snapshot, "run_id": context["run_id"], "logical_schema": schema, "logical_table": table,
        "capturer_path": capturer_path, "arrow_path": str(arrow_path), "driver_path": driver_path,
        "schema_preimage_path": schema_path, "type_preimage_path": type_path})
    permissions = [{"name": "SELECT", "status": "known", "granted": True},
                   {"name": "VIEW CHANGE TRACKING", "status": "known", "granted": True}]
    report = {"protocol": "oo.source-admission.report/v1", "target_id": "sqlserver-selfmanaged",
        "source_id": source["source_id"], "secret_ref": source["secret_ref"], "compat_mode": "sqlserver",
        "driver": {"kind": "native-rust", "name": "tiberius", "version": "0.13.0", "digest": sha(driver_path)},
        "license_flags": [], "permissions": permissions, "capabilities": [], "capture_methods": [],
        "cost_model": {"unit": "Rows", "confidence": "known", "estimate": "snapshot_count_big"}, "r1w": None,
        "capture_witness": witness_path}
    for name in ("Catalog", "BatchFull", "ChangeFeed", "DeleteNative", "Budget"):
        report["capabilities"].append({"name": name, "cursor": None, "feed": "ChangeTracking" if name == "ChangeFeed" else None,
            "cdc": None, "token_order": "None", "budget_unit": "Rows" if name == "Budget" else None,
            "level": "experimental", "write_class": "R1" if name in ("ChangeFeed", "DeleteNative") else "R0", "consistency": "S1"})
    config = {"protocol": "oo.source-admission.config/v1", "target_id": "sqlserver-selfmanaged", "source_id": source["source_id"],
        "secret_ref": source["secret_ref"], "compat_mode": "sqlserver", "permissions": permissions,
        "budget": {"unit": "Rows", "hard_limit": source["budget"]["hard_limit"], "confidence": "known"},
        "write_options": {"signal_table": False, "heartbeat_write": False, "create_stream": False, "create_trigger": False},
        "debezium": {"signal_data_collection": None, "heartbeat_action_query": None, "snapshot_mode": "never", "create_stream": False, "create_trigger": False},
        "requested_write_class": "R1", "r1w": None}
    report_path = write(directory / "admission-report.json", report)
    config_path = write(directory / "admission-config.json", config)
    seal_dir = directory / "seal"
    call(executable(context["admission_binary"]), ["seal-native", "--report", report_path, "--config", config_path, "--out", str(seal_dir)], directory, "admission")
    catalog = read(seal_dir / "catalog.json")
    sealed = catalog
    request = {"protocol": "oo.pipeline.native-update/v1", "source_engine": "sqlserver", "source_id": source["source_id"],
        "source_generation": generation, "parent_source_generation": parent, "logical_schema": schema, "logical_table": table,
        "key_columns": item["primary_key"], "batch_id": context["run_id"], "capture_method": "c2_change_feed", "delete_detection": "native",
        "consistency_grade": "S1", "cross_table_consistent": False, "position_codec": "mssql-ct-version/v1",
        "position_lineage": "mssql." + capture["candidate_position"]["database_guid"] + "." + capture["candidate_position"]["recovery_fork_guid"],
        "applied_pos": str(position["version"]), "columns": columns, "tombstones": item["tombstones"],
        "full_snapshot": capture["mode"] == "snapshot", "arrow_path": str(arrow_path), "max_arrow_bytes": context.get("max_arrow_bytes", 268435456),
        "snapshot": snapshot, "target": context["target"], "audit_digest": sha(capturer_path),
        "run_id": context["run_id"], "admission_path": str(seal_dir / "admission-receipt.json"),
        "registry_path": str(seal_dir / "registry.json"),
        "admission_expect": {"source_engine": "sqlserver", "source_id": source["source_id"], "source_generation": generation,
                             "declared_receipt_sha256": sha(seal_dir / "admission-receipt.json")},
        "sqlserver_candidate_position": capture["candidate_position"], "sqlserver_prior_position": capture["prior_position"]}
    for key in ("config_digest", "schema_digest", "type_digest", "input_digest"):
        request[key] = sealed.get(key, catalog.get(key))
        if not isinstance(request[key], str) or len(request[key]) != 64:
            raise ValueError("sealed_digest_absent")
    write(directory / "request.json", request)
    return request


def produce(context, job):
    if context.get("protocol") != "oo.mssql-preview.native-producer-context/v1" or job.get("protocol") != "oo.jobs.native-producer/v1":
        raise ValueError("protocol")
    reject_inline_secrets(context)
    source = read(context["customer_config"])
    reject_inline_secrets(source)
    if job.get("method") != "C2" or job.get("source_id") != source["source_id"] or job.get("binding") != context["binding"]:
        raise ValueError("job_source_binding")
    root = Path(context["artifact_root"])
    if not root.is_absolute() or not root.is_dir():
        raise ValueError("artifact_root")
    binding = str(uuid.UUID(job["binding"]))
    if binding != job["binding"] or int(job["pin"]) < 1:
        raise ValueError("job_binding_or_pin")
    binding_root = root / binding
    binding_root.mkdir(mode=0o700, exist_ok=True)
    cache = binding_root / ("pin-" + str(int(job["pin"])))
    context_digest = hashlib.sha256(json.dumps(context, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
    if cache.exists():
        if (cache / "context.sha256").read_text() != context_digest:
            raise ValueError("inflight_context_changed")
        saved = read(cache / "request.json")
        if saved["source_id"] != source["source_id"]:
            raise ValueError("cached_source_changed")
        return saved
    directory = binding_root / ("attempt-pin-" + str(int(job["pin"])) + "-" + str(uuid.uuid4()))
    directory.mkdir(mode=0o700)
    worker = executable(context["worker_binary"])
    state = json.loads(call(worker, ["--config", context["worker_config"], "native-state", "--binding", job["binding"]], directory, "state"))
    if state.get("protocol") != "oo.jobs.native-state/v1" or state.get("source_id") != source["source_id"]:
        raise ValueError("target_state_identity")
    parent = int(state["source_generation"]) if state.get("applied") is True else None
    generation = parent + 1 if parent is not None else 1
    context = dict(context, run_id=str(uuid.uuid4()))
    reader = executable(context["reader_binary"])
    args = ["--config", context["customer_config"], "--object", context["object"]["schema"] + "." + context["object"]["name"],
            "--out", str(directory / "capture"), "--chunk-rows", str(context.get("chunk_rows", 1000))]
    # One source/table binding coordinates all cycles. Reconciliation replaces the whole
    # table every configured N successful captures; no separate racing cycle writers.
    interval = int(context.get("reconcile_every", 100))
    if interval < 1:
        raise ValueError("reconcile_every")
    if parent is not None and generation % interval:
        prior = state.get("sqlserver_candidate_position")
        if not isinstance(prior, dict):
            raise ValueError("applied_position_absent")
        prior_path = write(directory / "applied-position.json", prior)
        call(reader, ["ct-pull", *args, "--position", prior_path], directory, "capture-command")
    else:
        call(reader, ["snapshot-full", *args], directory, "capture-command")
    capture = read(directory / "capture" / "manifest.json")
    audit_source = read(context["audit_config"])
    if any(audit_source["endpoint"][k] != source["endpoint"][k] for k in ("host", "port", "database")):
        raise ValueError("audit_source_identity")
    audit_path = directory / "raw-audit.json"
    call(reader, ["audit-export", "--config", context["audit_config"], "--audit-file-glob", context["audit_file_glob"],
        "--reader", source["endpoint"]["username"], "--application", capture["audit_application_name"], "--out", str(audit_path)], directory, "audit-command")
    request = assemble(context, source, capture, directory, audit_path, generation, parent)
    # Publish the immutable request/registry cache atomically. Underlying raw capture and
    # seal paths stay in the retained attempt directory; no references are renamed.
    publish = binding_root / (".publish-" + str(uuid.uuid4()))
    publish.mkdir(mode=0o700)
    request["registry_path"] = str(cache / "registry.json")
    write(publish / "request.json", request)
    fd = os.open(publish / "registry.json", os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "wb") as f:
        f.write((directory / "seal" / "registry.json").read_bytes())
        f.flush()
        os.fsync(f.fileno())
    (publish / "context.sha256").write_text(context_digest)
    try:
        publish.rename(cache)
    except FileExistsError:
        # Another attempt won; its exact request is the immutable capture for this pin.
        if (cache / "context.sha256").read_text() != context_digest:
            raise ValueError("inflight_context_changed")
        return read(cache / "request.json")
    return request


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--context", required=True)
    args = parser.parse_args()
    try:
        context = read(args.context)
        request = produce(context, json.load(sys.stdin))
        print(json.dumps(request, ensure_ascii=False, separators=(",", ":")))
    except Exception:
        # Raw failure artifacts remain private. No raw SQL/customer data or credential leaks.
        print("mssql-native-producer: refused; inspect retained private phase artifacts", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
