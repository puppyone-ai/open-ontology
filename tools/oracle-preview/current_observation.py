#!/usr/bin/env python3
"""Seal a current Oracle C5 observation beside its native maintenance build."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess

import c5_live
import configured_worker
import native_producer
import whole_run as w


SOURCE = "oracle_oneclick_orders"
TABLE = "OO04_FIX.ONBOARD_ORDERS"
SOURCE_BOUND = "oo.source-semantics.source-bound/v1"
TYPE_DICTIONARY = "oo.source-semantics.type-dictionary/v1"
HEAD = (
    "WHENEVER SQLERROR EXIT SQL.SQLCODE\n"
    "SET HEADING OFF FEEDBACK OFF PAGESIZE 0 LINESIZE 4000 TRIMSPOOL ON "
    "TAB OFF VERIFY OFF ECHO OFF WRAP OFF SERVEROUTPUT OFF\n"
)


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()


def put(path, body):
    path = Path(path)
    raw = json.dumps(body, sort_keys=True, indent=2, ensure_ascii=False).encode() + b"\n"
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "wb") as stream:
        stream.write(raw)
        stream.flush()
        os.fsync(stream.fileno())
    return hashlib.sha256(raw).hexdigest()


def save_existing(path, body):
    raw = json.dumps(body, sort_keys=True, indent=2, ensure_ascii=False).encode() + b"\n"
    Path(path).write_bytes(raw)
    Path(path).chmod(0o600)
    return hashlib.sha256(raw).hexdigest()


def one(text, prefix):
    found = c5_live.lines(text, prefix)
    if len(found) != 1:
        raise ValueError("oracle observation cardinality")
    return found[0]


def evidence_bytes(value):
    return json.dumps(value, sort_keys=True, indent=2, ensure_ascii=False).encode() + b"\n"


def private_dir(path):
    path = Path(path)
    if path.exists():
        metadata = path.lstat()
        if not stat.S_ISDIR(metadata.st_mode) or stat.S_ISLNK(metadata.st_mode) or metadata.st_uid != os.getuid():
            raise ValueError("private evidence directory")
    else:
        path.mkdir(mode=0o700)
    path.chmod(0o700)


def api_container_root(container, runtime_config, api_build_root, build_id):
    runtime_path = Path(runtime_config).resolve(strict=True)
    runtime = json.loads(runtime_path.read_text())
    work_dir = runtime.get("work_dir")
    if not isinstance(work_dir, str) or not Path(work_dir).is_absolute():
        raise ValueError("API work directory")
    expected_root = Path(work_dir) / "builds" / build_id
    inspected = subprocess.run(["docker", "inspect", container], capture_output=True, text=True, timeout=10)
    if inspected.returncode:
        raise ValueError("API container unavailable")
    details = json.loads(inspected.stdout)[0]
    if details.get("State", {}).get("Running") is not True:
        raise ValueError("API container not running")
    command = " ".join(details.get("Config", {}).get("Cmd") or [])
    args = " ".join(details.get("Args") or [])
    mounts = details.get("Mounts") or []
    mapped_roots = []
    mapped_configs = []
    for mount in mounts:
        source = Path(mount.get("Source", "")).resolve()
        destination = Path(mount.get("Destination", ""))
        try:
            relative_root = Path(api_build_root).resolve(strict=True).relative_to(source)
            relative_config = runtime_path.relative_to(source)
        except ValueError:
            continue
        mapped_roots.append(destination / relative_root)
        mapped_configs.append(destination / relative_config)
    if mapped_roots != [expected_root] or len(mapped_configs) != 1:
        raise ValueError("API build mount does not match runtime")
    config_argument = mapped_configs[0].as_posix()
    if config_argument not in command and config_argument not in args:
        raise ValueError("API is serving a different runtime")
    return expected_root


def api_stat(container, path):
    result = subprocess.run(["docker", "exec", container, "stat", "-c", "%u %a %s", str(path)], capture_output=True, text=True, timeout=10)
    if result.returncode:
        raise ValueError("API evidence metadata unavailable")
    fields = result.stdout.strip().split()
    if len(fields) != 3:
        raise ValueError("API evidence metadata malformed")
    return {"uid": int(fields[0]), "mode": int(fields[1], 8), "size": int(fields[2])}


def query_source(out):
    reader_password_path = Path("/Users/redux/.oo-dl01w1-04-p2/reader-window")
    reader_password = reader_password_path.read_text().strip()
    sql_dir = Path(out) / "source-observation-sql"
    sql_dir.mkdir(mode=0o700)
    query = HEAD + (
        "SELECT 'USER|' || SYS_CONTEXT('USERENV','SESSION_USER') FROM dual;\n"
        "SELECT 'CON|' || SYS_CONTEXT('USERENV','CON_NAME') FROM dual;\n"
        "SELECT 'BANNER|' || banner FROM v$version WHERE banner LIKE 'Oracle%';\n"
        "SELECT 'PRIV|' || privilege FROM session_privs ORDER BY privilege;\n"
        "SELECT 'GRANT|' || table_schema || '.' || table_name || '|' || privilege FROM all_tab_privs "
        "WHERE grantee=USER ORDER BY table_schema, table_name, privilege;\n"
        "SELECT 'COL|' || column_name || '|' || data_type || '|' || "
        "NVL(TO_CHAR(data_precision),'-') || '|' || NVL(TO_CHAR(data_scale),'-') || '|' || "
        "NVL(TO_CHAR(char_length),'-') || '|' || nullable FROM all_tab_columns "
        "WHERE owner='OO04_FIX' AND table_name='ONBOARD_ORDERS' ORDER BY column_id;\nEXIT\n"
    )
    code, text = c5_live.sqlplus("OO04_READER", reader_password, query, "current-bound-dictionary", sql_dir, [reader_password.encode()])
    if code or one(text, "USER|") != "OO04_READER" or one(text, "CON|") != "FREEPDB1":
        raise ValueError("source observation identity")
    banner = one(text, "BANNER|")
    if len(re.findall(r"\d+(?:\.\d+){3,}", banner)) != 1:
        raise ValueError("source release token")
    source_columns = {}
    oracle_columns = []
    for row in c5_live.lines(text, "COL|"):
        name, native, precision, scale, char_length, nullable = row.split("|")
        if nullable not in ("Y", "N"):
            raise ValueError("source dictionary nullability")
        if native in ("CHAR", "VARCHAR2"):
            length = int(char_length)
            p = s = "-"
        elif native == "NUMBER":
            p = precision
            s = scale
            length = 0
        elif native in ("DATE", "RAW"):
            p = s = "-"
            length = 0
        else:
            raise ValueError("unapproved Oracle type")
        source_columns[name] = f"{name}|{native}|{p}|{s}|{length}"
        oracle_columns.append({
            "name": name,
            "data_type": native,
            "precision": None if precision == "-" else int(precision),
            "scale": None if scale == "-" else int(scale),
            "char_length": None if char_length == "-" else int(char_length),
            "nullable": nullable == "Y",
        })
    session_privileges = sorted(c5_live.lines(text, "PRIV|"))
    object_privileges = []
    for row in c5_live.lines(text, "GRANT|"):
        parts = row.split("|", 1)
        if len(parts) != 2:
            raise ValueError("source reader grants")
        object_privileges.append({"object": parts[0], "privilege": parts[1]})
    scoped = [item["privilege"] for item in object_privileges if item["object"] == TABLE]
    if "CREATE SESSION" not in session_privileges or not {"SELECT", "FLASHBACK"}.issubset(scoped) or any(item in scoped for item in ("INSERT", "UPDATE", "DELETE")):
        raise ValueError("Oracle reader write scope")
    reader = {"protocol": "oo.oracle.reader-observation/v1", "user": "OO04_READER", "container": "FREEPDB1", "banner": banner,
              "dictionary_columns": list(source_columns.values()), "oracle_columns": oracle_columns,
              "session_privileges": session_privileges, "object_privileges": object_privileges}
    return banner, source_columns, reader


def query_db_contract(out):
    dba_path = c5_live.DBA_PASSWORD
    password = dba_path.read_text().strip()
    sql = HEAD + (
        "SELECT 'USER|' || SYS_CONTEXT('USERENV','SESSION_USER') FROM dual;\n"
        "SELECT 'LOG|' || log_mode FROM v$database;\n"
        "SELECT 'SUP|' || supplemental_log_data_min FROM v$database;\nEXIT\n"
    )
    directory = Path(out) / "source-observation-sql"
    code, text = c5_live.sqlplus("system", password, sql, "current-db-contract", directory, [password.encode()])
    if code or one(text, "USER|") != "SYSTEM":
        raise ValueError("source dba observation")
    log_mode = one(text, "LOG|")
    supplemental = one(text, "SUP|")
    if log_mode not in ("ARCHIVELOG", "NOARCHIVELOG") or supplemental not in ("YES", "NO"):
        raise ValueError("source logging observation")
    return {"protocol": "oo.oracle.database-contract-observation/v1", "user": "SYSTEM", "log_mode": log_mode, "supplemental_log": supplemental}


def query_scheduler_cut(cut):
    binding = cut.get("binding")
    if not isinstance(binding, str) or not re.fullmatch(r"[0-9a-fA-F-]{36}", binding):
        raise ValueError("Oracle scheduler binding identity")
    query = """
SELECT json_build_object('binding',b.id,'pin',m.pin,'task',t.id,'task_state',t.state,
 'lease_generation',t.generation,'source_generation',m.source_generation,
 'request',o.request,'manifest',b.manifest,'load',m.result)
FROM jw_native.bindings b JOIN jw_native.watermarks m ON m.binding_id=b.id
JOIN jw_native.occurrences o ON o.task_id=m.task_id JOIN jobs.tasks t ON t.id=m.task_id
WHERE b.id='""" + binding + "'::uuid;"
    actual = json.loads(w.psql("previewdb", query, tuples=True))
    for key in ("binding", "pin", "task", "task_state", "lease_generation", "source_generation", "request", "load"):
        if actual.get(key) != cut.get(key):
            raise ValueError("Oracle acknowledged scheduler cut changed")
    if actual.get("task_state") != "succeeded" or actual.get("source_generation") != cut.get("load", {}).get("source_generation"):
        raise ValueError("Oracle scheduler did not acknowledge this source generation")
    return actual


def source_provenance(cut, catalog):
    request = cut["request"]
    registry_path = Path(request["registry_path"]).resolve(strict=True)
    admission_path = Path(request["admission_path"]).resolve(strict=True)
    source_root = registry_path.parent.parent
    source_request_path = source_root / "request.json"
    source_catalog_path = registry_path.parent / "catalog.json"
    registry_bytes = registry_path.read_bytes()
    registry = json.loads(registry_bytes)
    if registry.get("protocol") != "oo.source-admission.binding-registry/v1" or len(registry.get("bindings", [])) != 1:
        raise ValueError("Oracle source registry protocol")
    entry = registry["bindings"][0]
    capture_path = Path(entry["capture_receipt_path"]).resolve(strict=True)
    paths = [registry_path, admission_path, source_request_path, source_catalog_path, capture_path]
    if registry_path.name != "registry.json" or admission_path != registry_path.parent / "admission-receipt.json" or source_catalog_path != registry_path.parent / "catalog.json":
        raise ValueError("Oracle sealed source paths")
    if any(path.is_symlink() or not path.is_file() or path.stat().st_size > 16 * 1024 * 1024 for path in paths):
        raise ValueError("Oracle source provenance file")
    registry_raw = registry_bytes
    admission_raw = admission_path.read_bytes()
    source_request_raw = source_request_path.read_bytes()
    source_catalog_raw = source_catalog_path.read_bytes()
    capture_raw = capture_path.read_bytes()
    admission = json.loads(admission_raw)
    source_request = json.loads(source_request_raw)
    source_catalog = json.loads(source_catalog_raw)
    capture = json.loads(capture_raw)
    raw_sha = lambda raw: hashlib.sha256(raw).hexdigest()
    actual = query_scheduler_cut(cut)
    manifest = actual.get("manifest")
    if not isinstance(manifest, dict):
        raise ValueError("Oracle native scheduler manifest")
    manifest_bytes = canonical(manifest)
    if manifest.get("protocol") != "oo.jobs.native-schedule/v1" or manifest.get("binding") != actual["binding"] or manifest.get("source_id") != SOURCE or manifest.get("method") != "C5" or manifest.get("cycle") != "pull":
        raise ValueError("Oracle native scheduler manifest identity")
    if manifest.get("registration", {}).get("registry_path", "").format(binding=actual["binding"], pin=actual["pin"]) != str(registry_path):
        raise ValueError("Oracle manifest registry binding")
    producer = manifest.get("producer", {})
    loader = manifest.get("loader", {})
    if producer.get("path") != "/opt/oracle-driver-probe" or producer.get("sha256") != entry.get("driver_digest") or loader.get("path") != "/opt/loader" or loader.get("sha256") != configured_worker.LOADER_SHA:
        raise ValueError("Oracle manifest executable pins")
    if manifest.get("registration", {}).get("admission_binary") != "/opt/admission" or manifest.get("registration", {}).get("admission_sha256") != native_producer.ADMISSION_SHA:
        raise ValueError("Oracle admission executable pin")
    if sha(configured_worker.LOADER) != configured_worker.LOADER_SHA or sha(native_producer.ADMISSION) != native_producer.ADMISSION_SHA:
        raise ValueError("Oracle fixed native binaries drifted")
    stamp = entry.get("admission_receipt_sha256", "")
    if not re.fullmatch(r"[0-9a-f]{64}", stamp) or registry_raw.count(stamp.encode()) != 1:
        raise ValueError("Oracle registry receipt stamp")
    registry_preimage = registry_raw.replace(stamp.encode(), b"0" * 64, 1)
    source_registry_preimage_sha256 = raw_sha(registry_preimage)
    source_admission_sha256 = raw_sha(admission_raw)
    source_catalog_sha256 = raw_sha(source_catalog_raw)
    capture_receipt_sha256 = raw_sha(capture_raw)
    source_request_sha256 = raw_sha(source_request_raw)
    if stamp != source_admission_sha256 or admission.get("registry_sha256") != source_registry_preimage_sha256:
        raise ValueError("Oracle registry/admission digest chain")
    if source_catalog_sha256 != entry.get("catalog_sha256") or entry.get("catalog_path") != str(source_catalog_path):
        raise ValueError("Oracle source catalog seal")
    if entry.get("admission_receipt_path") != str(admission_path) or entry.get("capture_receipt_sha256") != capture_receipt_sha256 or entry.get("capture_receipt_path") != str(capture_path):
        raise ValueError("Oracle source capture seal")
    if source_request != actual.get("request") or actual.get("request") != cut.get("request"):
        raise ValueError("Oracle source request differs from immutable scheduler occurrence")
    for key in ("source_id", "source_generation", "snapshot", "config_digest", "schema_digest", "type_digest", "input_digest"):
        if source_request.get(key) != entry.get(key):
            raise ValueError("Oracle request/registry source identity")
    if source_request.get("source_engine") != "oracle" or source_request.get("admission_expect", {}).get("declared_receipt_sha256") != stamp:
        raise ValueError("Oracle request admission binding")
    if source_catalog.get("protocol") != "oo.pipeline.sealed-import-catalog/v1" or source_catalog.get("engine") != "oracle":
        raise ValueError("Oracle source catalog protocol")
    for key in ("source_id", "source_generation", "schema_digest", "type_digest", "config_digest", "input_digest"):
        if source_catalog.get(key) != entry.get(key):
            raise ValueError("Oracle registry/source catalog identity")
    if actual.get("task_state") != "succeeded" or not isinstance(actual.get("pin"), int) or actual["pin"] < 1:
        raise ValueError("Oracle task/occurrence state")
    target_table = catalog.get("tables", [{}])[0]
    physical = target_table.get("stored_physical", "")
    if not re.fullmatch(r"[a-z0-9_]+", physical):
        raise ValueError("Oracle target physical name")
    columns_sql = (
        "SELECT json_agg(json_build_object('name',a.attname,'oid',a.atttypid,'typmod',a.atttypmod,"
        "'nullable',NOT a.attnotnull,'type',format_type(a.atttypid,a.atttypmod),'ordinal',a.attnum) ORDER BY a.attnum) "
        "FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace "
        "WHERE n.nspname='_oo' AND c.relname='" + physical + "' AND a.attnum>0 AND NOT a.attisdropped;"
    )
    target_columns = json.loads(w.psql("previewdb", columns_sql, tuples=True))
    if not isinstance(target_columns, list):
        raise ValueError("Oracle target column observation")
    for item in target_columns:
        item["oid"] = int(item["oid"])
    entry_objects = entry.get("objects")
    if not isinstance(entry_objects, list) or entry_objects != [{"arrow_sha256": request.get("input_digest"), "logical_schema": request.get("logical_schema"), "logical_table": request.get("logical_table"), "row_count": cut.get("load", {}).get("row_count")}]:
        raise ValueError("Oracle registry object/run mapping")
    return {
        "protocol": "oo.oracle.native-c5-provenance/v1",
        "source_registry_raw_json": registry_raw.decode("utf-8"),
        "source_registry_sha256": raw_sha(registry_raw),
        "source_registry_preimage_sha256": source_registry_preimage_sha256,
        "source_registry_path": str(registry_path),
        "source_request_path": str(source_request_path),
        "source_admission_raw_json": admission_raw.decode("utf-8"),
        "source_admission_sha256": source_admission_sha256,
        "source_admission_path": str(admission_path),
        "source_catalog_raw_json": source_catalog_raw.decode("utf-8"),
        "source_catalog_sha256": source_catalog_sha256,
        "source_catalog_path": str(source_catalog_path),
        "source_request_raw_json": source_request_raw.decode("utf-8"),
        "source_request_sha256": source_request_sha256,
        "capture_receipt_raw_json": capture_raw.decode("utf-8"),
        "capture_receipt_sha256": capture_receipt_sha256,
        "capture_receipt_path": str(capture_path),
        "source_schema_digest": entry["schema_digest"],
        "target_schema_digest": catalog["schema_digest"],
        "acknowledged_request": actual["request"],
        "acknowledged_request_sha256": hashlib.sha256(canonical(actual["request"])).hexdigest(),
        "scheduler_manifest": manifest,
        "scheduler_manifest_sha256": hashlib.sha256(manifest_bytes).hexdigest(),
        "binding": actual["binding"], "pin": actual["pin"], "task": actual["task"],
        "task_state": actual["task_state"], "lease_generation": actual["lease_generation"],
        "source_generation": actual["source_generation"],
        "target_columns": target_columns,
    }


def run(gold_dir, api_build_root, out, api_container, api_runtime_config):
    gold_dir = Path(gold_dir).resolve(strict=True)
    api_build_root = Path(api_build_root).resolve(strict=True)
    out = Path(out).resolve()
    for directory in (gold_dir, api_build_root):
        info = directory.lstat()
        if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid() or stat.S_ISLNK(info.st_mode):
            raise ValueError("private evidence root")
    if gold_dir.stat().st_mode & 0o077:
        raise ValueError("gold evidence root is not private")
    out.mkdir(mode=0o700, parents=True, exist_ok=False)
    c5_live.require_running(c5_live.ORACLE, c5_live.ORACLE_ID)
    cut = json.loads((gold_dir / "acknowledged-cut.json").read_text())
    catalog = json.loads((gold_dir / "catalog.json").read_text())
    request = cut["request"]
    loaded = cut["load"]
    if loaded.get("source_id") != SOURCE or request.get("source_id") != SOURCE:
        raise ValueError("source identity")
    if loaded.get("protocol") != "oo.pipeline.native-update-result/v1" or loaded.get("load_state") != "awaiting_publish":
        raise ValueError("source load state")
    if catalog.get("source_generation") != loaded.get("source_generation") or catalog.get("run_id") != loaded.get("run_id"):
        raise ValueError("catalog/load identity")
    provenance = source_provenance(cut, catalog)
    api_root = api_container_root(api_container, api_runtime_config, api_build_root, Path(api_build_root).name)
    table = catalog["tables"][0]
    position = request["snapshot"]["value"]
    match = re.fullmatch(r"configured-oracle-scn-(\d+)", position)
    if request.get("capture_method") != "c5_diff" or not match:
        raise ValueError("Oracle C5 position")
    shadow_path = Path(request["arrow_path"]).parents[3] / "capture/run/shadow/ONBOARD_ORDERS" / f"gen-{loaded['source_generation']}.json"
    shadow = json.loads(shadow_path.read_text())
    source_gold = json.loads((gold_dir / "source-gold.json").read_text())
    target_gold = json.loads((gold_dir / "target-gold.json").read_text())
    typed = json.loads((gold_dir / "typed-gold.json").read_text())
    if source_gold != target_gold or typed.get("differences") != 0 or typed.get("reader") != "OO04_READER":
        raise ValueError("typed source/target gold")
    if sha(request["arrow_path"]) != request.get("input_digest"):
        raise ValueError("source Arrow input digest")

    banner, source_columns, reader_observation = query_source(out)
    db_contract = query_db_contract(out)
    log_mode = db_contract["log_mode"]
    supplemental = db_contract["supplemental_log"]
    bindings = []
    for column in table["columns"]:
        logical = column["logical_name"]
        name = bytes.fromhex(logical.removeprefix("q_")).decode("ascii")
        if name not in source_columns:
            raise ValueError("source dictionary does not cover sealed column")
        bindings.append({"sealed_column": logical, "dictionary_column": name})
    if set(source_columns) != {row["dictionary_column"] for row in bindings}:
        raise ValueError("source dictionary has unselected columns")
    bound = {
        "protocol": SOURCE_BOUND,
        "engine": catalog["engine"],
        "source_id": catalog["source_id"],
        "source_generation": catalog["source_generation"],
        "target_generation": catalog["target_generation"],
        "run_id": catalog["run_id"],
        "schema_digest": catalog["schema_digest"],
        "type_digest": catalog["type_digest"],
        "config_digest": catalog["config_digest"],
        "input_digest": catalog["input_digest"],
        "banner": banner,
        "scn": match.group(1),
        "registry_snapshot": position,
        "published_scn": match.group(1),
        "consistency_grade": request["consistency_grade"],
        "delete_detection": request["delete_detection"],
        "dictionary": {"protocol": TYPE_DICTIONARY, "columns": [source_columns[row["dictionary_column"]] for row in bindings]},
        "bindings": bindings,
    }

    request_record = {key: request[key] for key in (
        "protocol", "source_engine", "source_id", "source_generation", "parent_source_generation", "full_snapshot", "snapshot",
        "capture_method", "delete_detection", "consistency_grade", "cross_table_consistent", "position_codec", "position_lineage",
        "applied_pos", "logical_schema", "logical_table", "columns", "tombstones", "config_digest", "schema_digest", "type_digest", "input_digest",
    )}
    request_record.update(run_id=loaded["run_id"], target_generation=loaded["target_generation"], arrow_sha256=request["input_digest"])
    load_record = {key: loaded[key] for key in (
        "protocol", "source_engine", "source_id", "source_generation", "target_generation", "run_id", "logical_schema", "logical_table",
        "physical_name", "row_count", "tombstone_count", "loader_invoked", "source_bound", "mirror_committed", "load_state", "watermark_state",
    )}
    typed["target_gold_sha256"] = sha(gold_dir / "target-gold.json")
    typed["source_gold_sha256"] = sha(gold_dir / "source-gold.json")
    typed["all_approved_columns"] = sorted(row["dictionary_column"] for row in bindings)
    typed["source_generation"] = catalog["source_generation"]
    typed["source_id"] = catalog["source_id"]
    save_existing(gold_dir / "typed-gold.json", typed)
    quality = {
        "tables": 1,
        "rows": len(source_gold),
        "columns": len(bindings),
        "typed_cells": len(source_gold) * len(bindings),
        "typed_cell_differences": 0,
        "delete_quality": (shadow.get("quality") or ["UNKNOWN"])[0],
        "key_2_absent": "2" not in shadow.get("cells", {}),
    }
    records = {
        "capture-request": request_record,
        "db-contract": db_contract,
        "load-result": load_record,
        "reader-observation": reader_observation,
        "source-provenance": provenance,
        "shadow": shadow,
        "source-bound": bound,
        "source-gold": source_gold,
        "target-gold": target_gold,
        "typed-gold": typed,
    }
    # The evidence-set directory is content-addressed over its exact member hashes.
    hashes = {name: hashlib.sha256(evidence_bytes(value)).hexdigest() for name, value in sorted(records.items())}
    evidence_set_id = hashlib.sha256(canonical(hashes)).hexdigest()
    local_dir = out / "source-evidence" / evidence_set_id
    api_dir = api_build_root / "source-evidence" / evidence_set_id
    for parent in (out, api_build_root):
        parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    for base in (local_dir.parent, api_dir.parent):
        private_dir(base)
    for base in (local_dir, api_dir):
        private_dir(base)
        for name, value in records.items():
            target = base / f"{name}.json"
            expected = evidence_bytes(value)
            if target.exists():
                metadata = target.lstat()
                if not stat.S_ISREG(metadata.st_mode) or stat.S_ISLNK(metadata.st_mode) or metadata.st_uid != os.getuid() or target.read_bytes() != expected:
                    raise ValueError("existing C5 evidence differs")
            else:
                put(target, value)
            target.chmod(0o600)
    api_root_stat = api_stat(api_container, api_root)
    api_evidence_root_stat = api_stat(api_container, api_root / "source-evidence")
    api_evidence_dir = api_root / "source-evidence" / evidence_set_id
    api_evidence_dir_stat = api_stat(api_container, api_evidence_dir)
    if api_evidence_root_stat["uid"] != api_root_stat["uid"] or api_evidence_root_stat["mode"] != 0o700 or api_evidence_dir_stat["uid"] != api_root_stat["uid"] or api_evidence_dir_stat["mode"] != 0o700:
        raise ValueError("API evidence directory ownership")
    evidence = {}
    for name, value in sorted(records.items()):
        raw = evidence_bytes(value)
        metadata = api_stat(api_container, api_evidence_dir / f"{name}.json")
        if metadata["uid"] != api_root_stat["uid"] or metadata["mode"] != 0o600 or metadata["size"] != len(raw):
            raise ValueError("API evidence file ownership")
        evidence[name] = {
            "path": f"source-evidence/{evidence_set_id}/{name}.json",
            "sha256": hashlib.sha256(raw).hexdigest(),
            "size": metadata["size"],
            "uid": metadata["uid"],
            "mode": metadata["mode"],
        }
    semantics = {
        "protocol": "oo.oracle.native-c5-source-semantics/v1",
        "source_id": catalog["source_id"],
        "executed": True,
        "source_generation": catalog["source_generation"],
        "target_generation": catalog["target_generation"],
        "product": "Oracle",
        "capture_method": "C5",
        "c1": "off",
        "consistency_grade": request["consistency_grade"],
        "delete_detection": request["delete_detection"],
        "source_contract": {
            "reader": "OO04_READER",
            "write_attempts": 0,
            "log_mode": log_mode,
            "supplemental_log": supplemental,
            "empty_string": "null",
            "position": position,
        },
        "quality": quality,
        "evidence_set_id": evidence_set_id,
        "evidence": evidence,
    }
    bundle = gold_dir / "bundle"
    source_semantics_path = bundle / "source_semantics.json"
    source_semantics_path.write_bytes(json.dumps(semantics, sort_keys=True, indent=2).encode() + b"\n")
    source_semantics_path.chmod(0o600)
    manifest_path = bundle / "manifest.json"
    manifest = json.loads(manifest_path.read_text())
    manifest["files"]["source_semantics.json"] = sha(source_semantics_path)
    manifest_path.write_text(json.dumps(manifest, sort_keys=True, separators=(",", ":")) + "\n")
    manifest_path.chmod(0o600)
    out_file = out / "current-source-semantics.json"
    out_file.write_bytes(source_semantics_path.read_bytes())
    out_file.chmod(0o600)
    return {"evidence_set_id": evidence_set_id, "source_semantics_sha256": sha(source_semantics_path), "evidence_files": len(records), "source_generation": catalog["source_generation"], "target_generation": catalog["target_generation"], "rows": quality["rows"], "cells": quality["typed_cells"], "differences": 0, "source_semantics": str(out_file), "sealed_bundle": str(bundle), "api_evidence_dir": str(api_dir), "api_container": api_container, "api_build_root_uid": api_root_stat["uid"], "source_banner": banner, "log_mode": log_mode, "supplemental_log": supplemental}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--gold-dir", type=Path, required=True)
    parser.add_argument("--api-build-root", type=Path, required=True)
    parser.add_argument("--api-container", required=True)
    parser.add_argument("--api-runtime-config", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(run(args.gold_dir, args.api_build_root, args.out, args.api_container, args.api_runtime_config), sort_keys=True))


if __name__ == "__main__":
    main()
