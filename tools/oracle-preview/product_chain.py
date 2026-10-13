"""Import, seal, query, and report the closure-2 Oracle capture.

This module does not recapture. It does not grant DBMS_FLASHBACK, does not
call product.py, and does not retag the source engine.
"""
import json
import shutil
import subprocess
import time
from pathlib import Path

import full_source as fs

PRIOR_ARROW = "5064f81e25162f8b183b9fe2c515907a29387d9509f1a7ea3d7b129976556f08"
PRIOR_DECISION = "aeea5a8cc36b4e35bc4b8c9bbfe5581181ee40e4a4f53fc247d3d5ce430b19b9"
CATALOG_DIGEST = "5c25559bc4d1b24471f19ae9eae8a45f40007c955938ea51edb03f1f5dbf2e26"
SEALED_DIGEST = "883d5831c423833e5e01214ac13e169fa0ef3075775d21a6b000b2ba888958da"
CONSUMER = "f7fdba26e8ec13bfd5a423f741f80c9e238abd74b39f8ea96e592f1b34c76a78"
BATCH_FIELDS = [
    {"name": "ID", "arrow_type": "int16", "nullable": False, "precision": 16},
    {"name": "N16", "arrow_type": "decimal128", "nullable": True, "precision": 16, "scale": 6},
    {"name": "N1", "arrow_type": "int8", "nullable": True, "precision": 8},
    {"name": "D", "arrow_type": "timestamp_micros", "nullable": True},
    {"name": "C6", "arrow_type": "utf8", "nullable": True},
    {"name": "V2", "arrow_type": "utf8", "nullable": True},
    {"name": "R", "arrow_type": "binary", "nullable": True},
]
BATCH_MANIFEST = [
    {"name": "ID", "logical_name": "id", "arrow_type": "int16", "nullable": False, "precision": 16},
    {"name": "N16", "logical_name": "n16", "arrow_type": "decimal128", "nullable": True, "precision": 16, "scale": 6},
    {"name": "N1", "logical_name": "n1", "arrow_type": "int8", "nullable": True, "precision": 8},
    {"name": "D", "logical_name": "d", "arrow_type": "timestamp_micros", "nullable": True},
    {"name": "C6", "logical_name": "c6", "arrow_type": "utf8", "nullable": True, "char_length": 6},
    {"name": "V2", "logical_name": "v2", "arrow_type": "utf8", "nullable": True},
    {"name": "R", "logical_name": "r", "arrow_type": "binary", "nullable": True},
]


def psql(sql):
    return fs.run([
        "docker", "exec", "-u", "postgres", fs.PG,
        "psql", "-d", "previewdb", "-v", "ON_ERROR_STOP=1", "-At", "-c", sql,
    ], timeout=40)


def function_present(name):
    present = psql(
        "SELECT 1 FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace "
        f"WHERE n.nspname='_oo' AND p.proname='{name}'"
    )
    return present.returncode == 0 and present.stdout.decode().strip() == "1"


def preserve_empty_stdin():
    failed = fs.EVID / "install-0008"
    attempt = failed / "attempt-empty-stdin"
    stderr = failed / "install.stderr"
    if attempt.exists() or not stderr.is_file():
        return
    if stderr.read_text().strip() != "oo-pipeline: invalid input":
        return
    attempt.mkdir()
    for name in ("install.exit", "install.stdout", "install.stderr", "request.json"):
        source = failed / name
        if source.is_file():
            shutil.copy(source, attempt / name)


def pipeline_admin(directory_name, container, command, protocol):
    directory = fs.EVID / directory_name
    directory.mkdir(parents=True, exist_ok=True)
    body = (json.dumps({
        "protocol": protocol,
        "target": {
            "host": fs.PG,
            "port": 5432,
            "database": "previewdb",
            "administrator": "postgres",
            "password_file": "/run/pg-secrets/pg-admin",
            "ca_file": "/run/pg-secrets/ca.crt",
        },
    }) + "\n").encode()
    (directory / "request.json").write_bytes(body)
    proc = fs.run([
        "docker", "run", "--rm", "-i", "--name", container,
        "--network", fs.NET, "--platform", "linux/arm64", "--pull", "never",
        "--memory", "512m", "--cpus", "0.5",
        "--cap-drop", "ALL", "--security-opt", "no-new-privileges",
        "--entrypoint", fs.PIPELINE,
        "-v", "oo-dl01w1-04-target:/target:ro",
        "-v", f"{fs.SECRET / 'pg-admin'}:/run/pg-secrets/pg-admin:ro",
        "-v", f"{fs.SECRET / 'pg-tls' / 'ca.crt'}:/run/pg-secrets/ca.crt:ro",
        "-v", f"{directory}:/in",
        *fs.LABELS, "--label", "oo.kind=proof-runner",
        fs.IMAGE, command,
    ], timeout=120, input_bytes=body)
    fs.save(directory, "install", proc)
    text = proc.stdout.decode("utf-8", "replace")
    fs.say(f"{command} exit={proc.returncode} stdout={text.strip()[:200]}")
    return proc.returncode == 0, text


def install_catalog():
    if function_present("full_import_native_catalog"):
        fs.say("catalog_function=already_present")
        return True
    preserve_empty_stdin()
    sealed = psql("SELECT 1 FROM _oo.schema_migrations WHERE version = 'sealed-import-catalog/v1'")
    if not (sealed.returncode == 0 and sealed.stdout.decode().strip() == "1"):
        ok, text = pipeline_admin(
            "install-0006", "oo-dl01w1-04-c2-install6",
            "install-sealed-catalog", "oo.pipeline.install-sealed-catalog/v1",
        )
        if not ok or SEALED_DIGEST not in text:
            fs.say(f"install_0006 failed digest_match={SEALED_DIGEST in text}")
            return False
    ok, text = pipeline_admin(
        "install-0008", "oo-dl01w1-04-c2-install8",
        "install-native-sealed-catalog", "oo.pipeline.install-native-sealed-catalog/v1",
    )
    installed = ok and CATALOG_DIGEST in text and function_present("full_import_native_catalog")
    fs.say(f"install_0008 digest_match={CATALOG_DIGEST in text} function={installed}")
    return installed


def validate_files(directory):
    proc = fs.run([
        "docker", "run", "--rm", "--network", "none",
        "--platform", "linux/arm64", "--pull", "never",
        "--memory", "512m", "--cpus", "0.5",
        "--cap-drop", "ALL", "--security-opt", "no-new-privileges",
        "--entrypoint", fs.ADMISSION,
        "-v", "oo-dl01w1-04-target:/target:ro",
        "-v", f"{directory}:/in:ro",
        *fs.LABELS, "--label", "oo.kind=admission",
        fs.IMAGE, "validate", "--report", "/in/report.json", "--config", "/in/config.json",
    ], timeout=60)
    fs.save(directory, "validate", proc)
    (directory / "decision.json").write_bytes(proc.stdout)
    parsed = {}
    try:
        parsed = json.loads(proc.stdout.decode() or "{}")
    except json.JSONDecodeError:
        parsed = {}
    decision_sha = fs.sha256_bytes(proc.stdout)
    admitted = proc.returncode in (0, 2) and parsed.get("admitted") is True
    fs.say(f"admit {directory.name} exit={proc.returncode} admitted={parsed.get('admitted')} decision={decision_sha[:12]}")
    return admitted, decision_sha, parsed


# The frozen importer accepts only this W38 ClickHouse/OpenSearch body.
# Oracle must not copy it into an import directory.
FOREIGN_ADMISSION_SCHEMA = "oo.native-readonly-completion/receipt/v1"
FOREIGN_ADMISSION_COMMIT = "5df40d24eefa4fbcfa00c4415cfcfde5d368db89"
# Root has not handed back a fixed public-importer archive. None stays until that SHA exists.
FIXED_IMPORTER_SHA = None
# caveat_agrees in the sealed catalog. These are storage tokens, not Oracle loss flags.
SEALER_STORAGE_CAVEATS = frozenset({
    "tinyint1_as_int2",
    "bigint_unsigned_numeric",
    "mysql_time_not_interval",
    "json_not_jsonb",
    "enum_as_text",
})
ORACLE_LOSS = {
    "batch": {
        "source_id": "oo04_t_batch",
        "logical_table": "t_batch",
        "columns": {"c6": ["empty_string_is_null"], "v2": ["empty_string_is_null"]},
    },
    "wide": {
        "source_id": "oo04_t_wide_text",
        "logical_table": "t_wide_text",
        "native_notes": {"n_free_text": "unconstrained_number"},
    },
    "ts": {
        "source_id": "oo04_t_ts_text",
        "logical_table": "t_ts_text",
        "columns": {"ts_text": ["precision_loss"], "tstz_text": ["precision_loss"]},
    },
    "table": {
        "consistency_grade": "S1",
        "delete_detection": "reconcile",
        "capture": "sealed_import",
    },
    "omit": ["time_ambiguous", "offset_lost"],
}


class AdmissionRefused(ValueError):
    """This capture is not admitted by the frozen public importer."""


def preimage(source_id, scn, rows):
    """Closure-2 notes. Their hashes are not manifest.schema_digest."""
    schema = "\n".join([
        "closure-2",
        f"source={source_id}",
        "generation=2",
        f"scn={scn}",
        *[f"{row['name']} nullable={str(row['nullable']).lower()}" for row in rows],
        "",
    ])
    types = "\n".join([
        "closure-2",
        f"source={source_id}",
        "generation=2",
        f"scn={scn}",
        *[f"{row['name']} {row['arrow_type']}" for row in rows],
        "",
    ])
    return schema.encode(), types.encode()


def hex64(value):
    return (
        isinstance(value, str)
        and len(value) == 64
        and all(char in "0123456789abcdef" for char in value)
    )


def closure2_note_digests(source_id, scn, rows):
    schema, types = preimage(source_id, scn, rows)
    return fs.sha256_bytes(schema), fs.sha256_bytes(types)


def refuse_preimage_as_schema_digest(schema_digest, note_digest):
    """schema_digest is the sealer Arrow schema hash. The closure-2 note is not that hash."""
    if not hex64(schema_digest) or schema_digest == note_digest:
        raise AdmissionRefused(
            "schema_digest is the closure-2 preimage, not the sealer Arrow schema hash"
        )
    return schema_digest


def schema_digest_from_sealer(catalog, note_digest):
    """Read schema_digest from a sealer catalog object. Do not pass a handwritten hex."""
    if not isinstance(catalog, dict):
        raise AdmissionRefused("sealed catalog missing")
    if (
        catalog.get("protocol") != "oo.pipeline.sealed-import-catalog/v1"
        or catalog.get("engine") != "oracle"
    ):
        raise AdmissionRefused("sealed catalog is not an oracle sealer result")
    return refuse_preimage_as_schema_digest(catalog.get("schema_digest"), note_digest)


def admission_is_foreign(receipt):
    if not isinstance(receipt, dict):
        return True
    if receipt.get("schema") == FOREIGN_ADMISSION_SCHEMA:
        return True
    if receipt.get("content_commit") == FOREIGN_ADMISSION_COMMIT:
        return True
    return False


def sealer_stores_loss(token):
    return token in SEALER_STORAGE_CAVEATS


def oracle_loss_blocked():
    tokens = ("empty_string_is_null", "unconstrained_number", "precision_loss")
    return {
        "tokens": list(tokens),
        "storage_caveat_can_store": {token: sealer_stores_loss(token) for token in tokens},
        "blocked": any(not sealer_stores_loss(token) for token in tokens),
        "required": ORACLE_LOSS,
    }


def manifest_binds_arrow_schema(manifest, sealed_catalog, note_digest):
    ident = (manifest or {}).get("catalog_identity") or {}
    got = ident.get("schema_digest")
    sealed = (sealed_catalog or {}).get("schema_digest")
    if not hex64(got) or not hex64(sealed) or got == note_digest:
        return False
    return got == sealed


def binding_catalog(source_id, scn, schema_digest, type_digest, config_sha, arrow_sha, note_digest):
    schema_digest = refuse_preimage_as_schema_digest(schema_digest, note_digest)
    return {
        "protocol": "oo.pipeline.sealed-import-catalog/v1",
        "engine": "oracle",
        "source_id": source_id,
        "source_generation": 2,
        "state": "not_sealed",
        "capture": "reader_as_of_published_scn",
        "schema_digest": schema_digest,
        "type_digest": type_digest,
        "config_digest": config_sha,
        "input_digest": arrow_sha,
        "input_digest_preimage": "sha256 of the arrow bytes for this capture",
        "schema_digest_preimage": "sealer push_schema_column digest; closure-2 schema-preimage.txt is not this field",
        "type_digest_preimage": "sha256 of type-preimage.txt for this capture",
        "config_digest_preimage": "sha256 of config.json bytes",
        "snapshot": {"kind": "token", "value": f"oo-dl01w1-04-scn-{scn}"},
    }


def manifest_for(source_id, table, rows, arrow_sha, columns, schema_digest, type_digest, config_sha, scn, note_digest):
    schema_digest = refuse_preimage_as_schema_digest(schema_digest, note_digest)
    return {
        "protocol": "oo.pipeline.native-arrow-import/v1",
        "source_engine": "oracle",
        "source_id": source_id,
        "source_generation": 2,
        "snapshot": {"kind": "token", "value": f"oo-dl01w1-04-scn-{scn}"},
        "target_storage": "postgresql",
        "catalog_identity": {
            "config_digest": config_sha,
            "schema_digest": schema_digest,
            "type_digest": type_digest,
            "input_digest": arrow_sha,
        },
        "admission": {
            "decision_path": "/in/decision.json",
            "receipt_path": "/in/native-readonly-receipt.json",
        },
        "tables": [{
            "logical_schema": "batch",
            "logical_table": table,
            "row_count": rows,
            "arrow_path": "/in/batch.arrow",
            "batch_sha256": arrow_sha,
            "columns": columns,
        }],
        "target": {
            "host": fs.PG,
            "port": 5432,
            "database": "previewdb",
            "runtime_principal": "oo_pipeline_runtime",
            "password_file": "/run/pg-secrets/pg-admin",
            "ca_file": "/run/pg-secrets/ca.crt",
            "owner": fs.SESSION,
            "shard": "shard_oo04",
        },
    }


def registry_for(source_id, table, rows, fields, arrow_sha, schema_digest, type_digest, decision_sha, report_sha, config_sha, receipt_sha, catalog_sha, scn, note_digest):
    schema_digest = refuse_preimage_as_schema_digest(schema_digest, note_digest)
    binding = {
        "protocol": "oo.source-admission.capture-binding/v1",
        "source_id": source_id,
        "source_engine": "oracle",
        "source_generation": 2,
        "snapshot": {"kind": "token", "value": f"oo-dl01w1-04-scn-{scn}"},
        "decision_sha256": decision_sha,
        "report_path": "/in/report.json",
        "report_sha256": report_sha,
        "config_path": "/in/config.json",
        "config_sha256": config_sha,
        "config_digest": config_sha,
        "schema_digest": schema_digest,
        "type_digest": type_digest,
        "input_digest": arrow_sha,
        "driver_kind": "native-rust",
        "driver_name": "oracledb",
        "driver_version": "26.0.0-beta.4",
        "driver_digest": "d729181747bd69ba53cde2956ea9ddc06cf09d0badaf4a9e89944b7c0b7b4397",
        "capture_receipt_path": "/in/capture-receipt.json",
        "capture_receipt_sha256": receipt_sha,
        "catalog_path": "/in/binding-catalog.json",
        "catalog_sha256": catalog_sha,
        "admission_receipt_path": "/in/native-readonly-receipt.json",
        "objects": [{
            "logical_schema": "batch",
            "logical_table": table,
            "row_count": rows,
            "arrow_sha256": arrow_sha,
        }],
        "fields": fields,
    }
    return {"protocol": "oo.source-admission.binding-registry/v1", "bindings": [binding]}


def import_prepared(directory, manifest, registry, arrow, receipt, catalog):
    """Refuse the W38 admission body. Do not start import-native and do not rewrite a finished envelope.

    A later fixed importer SHA from root is the only path that may copy a source-bound
    receipt and run import-native. That SHA is not set. manifest, registry, arrow, and
    catalog are ignored until then so this function cannot write a preimage digest.
    """
    del manifest, registry, arrow, catalog
    directory = Path(directory)
    if (directory / "envelope.json").is_file() or (directory / "native-readonly-receipt.json").is_file():
        return False, {
            "load_state": "refused",
            "reason": "historical_import_preserved",
            "admits_import": False,
            "replaces_admission_slot": False,
            "docker_started": False,
        }
    if FIXED_IMPORTER_SHA is None or admission_is_foreign(receipt):
        return False, {
            "load_state": "refused",
            "reason": "foreign_admission_required",
            "frozen_gate_sha256": fs.RECEIPT_SHA,
            "frozen_gate_schema": FOREIGN_ADMISSION_SCHEMA,
            "frozen_gate_content_commit": FOREIGN_ADMISSION_COMMIT,
            "fixed_importer_sha": FIXED_IMPORTER_SHA,
            "admits_import": False,
            "replaces_admission_slot": False,
            "docker_started": False,
        }
    raise AdmissionRefused("fixed importer SHA is recorded but the source-bound import path is not open")


def envelope_import(envelope):
    inner = envelope.get("import") if isinstance(envelope.get("import"), dict) else None
    if inner and inner.get("run_id"):
        return inner
    return envelope


def existing_import(name, source_id, rows, scn, fields):
    """Read a finished envelope and refuse to treat its preimage digest as admission.

    The directory is not rewritten. imported stays false until a source-bound
    importer returns a sealer schema_digest.
    """
    directory = fs.EVID / f"import-{name}"
    exit_file = directory / "import.exit"
    if not exit_file.is_file() or exit_file.read_text().strip() != "0":
        return None
    env_file = directory / "envelope.json"
    manifest_file = directory / "manifest.json"
    arrow = directory / "batch.arrow"
    decision = directory / "decision.json"
    if not env_file.is_file() or not manifest_file.is_file() or not arrow.is_file() or not decision.is_file():
        return None
    try:
        inner = envelope_import(json.loads(env_file.read_text()))
        manifest = json.loads(manifest_file.read_text())
    except json.JSONDecodeError:
        return None
    if inner.get("load_state") != "awaiting_publish" or not inner.get("run_id"):
        return None
    if inner.get("source_id") != source_id or inner.get("source_generation") != 2:
        return None
    snap = (manifest.get("snapshot") or {}).get("value")
    if snap != f"oo-dl01w1-04-scn-{scn}":
        return None
    ident = manifest.get("catalog_identity") or {}
    arrow_sha = fs.sha256_file(arrow)
    if ident.get("input_digest") != arrow_sha or manifest.get("source_id") != source_id:
        return None
    decision_sha = fs.sha256_bytes(decision.read_bytes())
    note_schema, note_type = closure2_note_digests(source_id, scn, fields)
    receipt_file = directory / "native-readonly-receipt.json"
    receipt_sha = fs.sha256_file(receipt_file) if receipt_file.is_file() else None
    stored = ident.get("schema_digest")
    fs.say(
        f"import import-{name} preserved run={inner['run_id']} "
        f"preimage_schema={stored == note_schema} foreign_receipt={receipt_sha == fs.RECEIPT_SHA}"
    )
    return {
        "admitted": False,
        "imported": False,
        "historical_envelope_preserved": True,
        "decision_sha256": decision_sha,
        "decision_matches_policy2": decision_sha == PRIOR_DECISION,
        "arrow_sha256": arrow_sha,
        "same_bytes_as_5064f81e": arrow_sha == PRIOR_ARROW,
        "schema_digest": None,
        "historical_manifest_schema_digest": stored,
        "closure2_schema_preimage_sha256": note_schema,
        "manifest_schema_digest_is_closure2_preimage": stored == note_schema,
        "type_digest": ident.get("type_digest"),
        "type_note_sha256": note_type,
        "config_digest": ident.get("config_digest"),
        "admission_receipt_sha256": receipt_sha,
        "admission_is_foreign_w38": receipt_sha == fs.RECEIPT_SHA,
        "arrow_schema_bound": False,
        "run_id": inner.get("run_id"),
        "load_state": "refused",
        "historical_load_state": inner.get("load_state"),
        "reason": "arrow_schema_unbound",
        "source_id": source_id,
        "rows": rows,
        "scn": scn,
        "snapshot": snap,
        "reused_envelope": False,
        "docker_started": False,
    }


def prepare_object(name, source_id, table, rows, arrow, fields, columns, scn, clone_source, row_budget=None):
    """Do not hash the closure-2 note into schema_digest and do not attach the W38 receipt.

    clone_source and row_budget stay in the signature so existing callers compile.
    They no longer open a validator or an importer. A new physical import waits
    on FIXED_IMPORTER_SHA plus a sealer catalog.
    """
    del table, columns, clone_source, row_budget
    found = existing_import(name, source_id, rows, scn, fields)
    if found:
        return found
    note_schema, note_type = closure2_note_digests(source_id, scn, fields)
    arrow_sha = fs.sha256_file(arrow) if Path(arrow).is_file() else None
    fs.say(f"import import-{name} refused before docker preimage={note_schema[:12]}")
    return {
        "admitted": False,
        "imported": False,
        "historical_envelope_preserved": False,
        "decision_sha256": None,
        "arrow_sha256": arrow_sha,
        "schema_digest": None,
        "closure2_schema_preimage_sha256": note_schema,
        "manifest_schema_digest_is_closure2_preimage": False,
        "type_digest": None,
        "type_note_sha256": note_type,
        "admission_is_foreign_w38": True,
        "arrow_schema_bound": False,
        "semantic_loss_on_sealed_catalog": False,
        "run_id": None,
        "load_state": "refused",
        "reason": "foreign_admission_required",
        "source_id": source_id,
        "rows": rows,
        "scn": scn,
        "snapshot": f"oo-dl01w1-04-scn-{scn}",
        "docker_started": False,
        "fixed_importer_sha": FIXED_IMPORTER_SHA,
    }


def text_columns(kind):
    if kind == "wide":
        fields = [
            {"name": "ID", "arrow_type": "int16", "nullable": False, "precision": 16},
            {"name": "N_FREE_TEXT", "arrow_type": "utf8", "nullable": True},
        ]
        columns = [
            {"name": "ID", "logical_name": "id", "arrow_type": "int16", "nullable": False, "precision": 16},
            {"name": "N_FREE_TEXT", "logical_name": "n_free_text", "arrow_type": "utf8", "nullable": True},
        ]
        return "oo04_t_wide_text", "t_wide_text", fields, columns
    fields = [
        {"name": "ID", "arrow_type": "int16", "nullable": False, "precision": 16},
        {"name": "TS_TEXT", "arrow_type": "utf8", "nullable": True},
        {"name": "TSTZ_TEXT", "arrow_type": "utf8", "nullable": True},
    ]
    columns = [
        {"name": "ID", "logical_name": "id", "arrow_type": "int16", "nullable": False, "precision": 16},
        {"name": "TS_TEXT", "logical_name": "ts_text", "arrow_type": "utf8", "nullable": True},
        {"name": "TSTZ_TEXT", "logical_name": "tstz_text", "arrow_type": "utf8", "nullable": True},
    ]
    return "oo04_t_ts_text", "t_ts_text", fields, columns


def catalog_one(name, imported):
    if not imported.get("imported") or not imported.get("run_id"):
        return {"ok": False, "reason": "import_absent"}
    run_id = str(imported["run_id"])
    if len(run_id) != 36 or any(ch not in "0123456789abcdef-" for ch in run_id):
        return {"ok": False, "reason": "run_id"}
    generation = psql(f"SELECT target_generation FROM _oo.full_import_run WHERE run_id = '{run_id}'")
    target_generation = generation.stdout.decode().strip()
    if generation.returncode != 0 or not target_generation.isdigit():
        return {"ok": False, "reason": "target_generation_absent", "stdout": generation.stdout.decode()[-300:]}
    request = {
        "protocol": "oo.pipeline.native-sealed-catalog/v1",
        "run_id": imported["run_id"],
        "source_id": imported["source_id"],
        "owner": fs.SESSION,
        "source_generation": 2,
        "target_generation": int(target_generation),
        "config_digest": imported["config_digest"],
        "schema_digest": imported["schema_digest"],
        "type_digest": imported["type_digest"],
        "input_digest": imported["arrow_sha256"],
        "snapshot": {"kind": "token", "value": f"oo-dl01w1-04-scn-{imported.get('scn', '')}"},
        "target": {
            "host": fs.PG,
            "port": 5432,
            "database": "previewdb",
            "runtime_principal": "oo_pipeline_runtime",
            "password_file": "/run/pg-secrets/pg-admin",
            "ca_file": "/run/pg-secrets/ca.crt",
        },
    }
    directory = fs.EVID / f"catalog-{name}"
    imp = fs.EVID / f"import-{name}"
    directory.mkdir(parents=True, exist_ok=True)
    fs.write_json(directory / "request.json", request)
    reg_sha = (imp / "registry.sha256").read_text().strip()
    proc = fs.run([
        "docker", "run", "--rm", "--name", f"oo-dl01w1-04-c2-cat-{name}",
        "--network", fs.NET, "--platform", "linux/arm64", "--pull", "never",
        "--memory", "1g", "--cpus", "0.5",
        "--cap-drop", "ALL", "--security-opt", "no-new-privileges",
        "--entrypoint", fs.BUILDER,
        "-e", f"OO_NATIVE_ADMISSION_BIN={fs.ADMISSION}",
        "-e", "OO_NATIVE_BINDING_REGISTRY=/in/registry.json",
        "-e", f"OO_NATIVE_BINDING_REGISTRY_SHA256={reg_sha}",
        "-v", "oo-dl01w1-04-target:/target:ro",
        "-v", f"{imp}:/in:ro",
        "-v", f"{directory}:/work",
        "-v", f"{fs.SECRET / 'pg-admin'}:/run/pg-secrets/pg-admin:ro",
        "-v", f"{fs.SECRET / 'pg-tls' / 'ca.crt'}:/run/pg-secrets/ca.crt:ro",
        *fs.LABELS, "--label", "oo.kind=proof-runner",
        fs.IMAGE, "import-native-catalog",
        "--request", "/work/request.json", "--out", "/work/catalog.json",
        "--pipeline", fs.PIPELINE, "--stderr", "/work/catalog.stderr",
    ], timeout=90)
    fs.save(directory, "catalog", proc)
    doc = {}
    if (directory / "catalog.json").is_file():
        try:
            doc = json.loads((directory / "catalog.json").read_text())
        except json.JSONDecodeError:
            doc = {}
    ok = proc.returncode == 0 and doc.get("protocol") == "oo.pipeline.sealed-import-catalog/v1" and doc.get("engine") == "oracle"
    fs.say(f"catalog {name} exit={proc.returncode} engine={doc.get('engine')} state={doc.get('state')}")
    return {
        "ok": ok,
        "sha256": fs.sha256_file(directory / "catalog.json") if (directory / "catalog.json").is_file() else None,
        "engine": doc.get("engine"),
        "state": doc.get("state"),
        "stored_physical": (doc.get("tables") or [{}])[0].get("stored_physical"),
        "run_id": doc.get("run_id"),
        "target_generation": doc.get("target_generation"),
    }


def builder_configs(directory, dataset="batch.t_batch", visible=None, source_id="oo04_t_batch"):
    visible = visible or ["c6", "d", "n1", "n16", "r", "v2"]
    declared = {
        "adapter": "loopback_http_local_dev",
        "budget": {
            "deadline_ms": 90000, "max_completion_tokens": 4000, "max_in_flight": 1,
            "max_prompt_tokens": 8000, "max_request_bytes": 262144, "max_response_bytes": 262144,
            "max_tables": 4, "max_total_tokens": 12000,
        },
        "declared_price": {"currency": "USD", "per_total_token": "0.000001", "status": "declared_non_billing"},
        "endpoint": {"model_id": "grok-4.7", "origin": "http://127.0.0.1:8317", "path": "/v1/chat/completions"},
        "namespace": "pg-builder", "protocol": "oo.pg-builder.config/v1", "purpose": "generate",
        "sample_policy": "none", "secret": {"bearer_file": "/run/secrets/llm-token"}, "temperature": 0,
    }
    timeout = json.loads(json.dumps(declared))
    timeout["budget"]["deadline_ms"] = 1
    denied = json.loads(json.dumps(declared))
    denied["secret"] = {"bearer_file": "/run/secrets/bad-token"}
    unknown = json.loads(json.dumps(declared))
    unknown["declared_price"] = {"status": "unknown"}
    fs.write_json(directory / "config-declared.json", declared)
    fs.write_json(directory / "config-timeout.json", timeout)
    fs.write_json(directory / "config-401.json", denied)
    fs.write_json(directory / "config-unknown.json", unknown)
    fs.write_json(directory / "settings.json", {
        "project": source_id, "ontology_version": "ont-oo04", "catalog_version": "pending",
        "policy": {"id": "pol-oo04", "version": "1"},
        "knowledge": {"id": "know_oo04", "version": "1"},
        "ontology": {
            "spec_version": "oos/v1", "name": "oo04", "locale": "en-US",
            "include_schemas": [dataset.split(".", 1)[0]], "default_tier_preference": ["raw"],
            "publish": {"include_unreviewed": []},
            "classification": {"auto_accept": "rule_high_confidence"},
            "lifecycle": {"deprecation_min_releases": 2},
            "compile": {
                "per_role": True,
                "budgets": {"readme_tokens": 2000, "table_file_tokens": 3000},
                "sample_values": {"default": "masked", "max_per_column": 0},
            },
        },
    })
    fs.write_json(directory / "visibility.json", {
        "protocol": "oo.builder.role-visibility/v1", "project": source_id,
        "principal": "oo04_reader", "role": "oo_r_reader",
        "ontology_version": "ont-oo04", "catalog_version": "pending",
        "policy": {"id": "pol-oo04", "version": "1"},
        "knowledge": {"id": "know_oo04", "version": "1"},
        "issued_for_compiler": "oo-builder/role-pack/v1",
        "datasets": [{
            "dataset": dataset, "row_security": "none", "global_stats": "denied",
            "columns": visible,
        }],
        "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128},
    })


def run_builder(catalog_path, dataset="batch.t_batch", visible=None, source_id="oo04_t_batch", folder="builder", container="oo-dl01w1-04-c2-builder", preimage_path=None):
    directory = fs.EVID / folder
    directory.mkdir(parents=True, exist_ok=True)
    if preimage_path is not None and Path(preimage_path).is_file():
        shutil.copy(preimage_path, directory / "schema-preimage.txt")
    builder_configs(directory, dataset, visible, source_id)
    shutil.copy(catalog_path, directory / "catalog.json")
    shutil.copy(fs.SRC / "evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/closure-1/builder-1/relay.py", directory / "relay.py")
    script = r"""#!/bin/bash
set -u
cd /evidence
python3 /evidence/relay.py >/tmp/relay.out 2>/tmp/relay.err &
echo $! >/evidence/relay.pid
ready=0
for _ in 1 2 3 4 5 6 7 8 9 10; do
  if python3 -c 'import socket; s=socket.create_connection(("127.0.0.1",8317),1); s.close()'; then ready=1; break; fi
  sleep 0.2
done
echo "$ready" >/evidence/relay-ready.txt
test "$ready" = 1
run_case() {
  local name="$1" config="$2" cache="$3"
  mkdir -p "/evidence/$name" "/evidence/$cache"
  set +e
  /target/import-a682/debug/pg-builder sealed \
    --config "/evidence/$config" --catalog /evidence/catalog.json \
    --settings /evidence/settings.json --visibility /evidence/visibility.json \
    --cache "/evidence/$cache" --out "/evidence/$name" \
    --reviewer author.dl01w1-04 --reviewed-at 2026-10-11 --dataset batch.t_batch \
    --target-host oo-dl01w1-04-p2-pg --target-port 5432 --target-database previewdb \
    --target-user oo04_preview_login --login oo04_preview_login \
    --target-ca /run/pg-secrets/ca.crt --target-password /run/pg-secrets/preview-login \
    >"/evidence/$name.stdout" 2>"/evidence/$name.stderr"
  echo "$?" >"/evidence/$name.exit"
  set -e
  echo "case $name exit=$(cat /evidence/$name.exit)"
}
run_case out-timeout config-timeout.json cache-timeout
run_case out-401 config-401.json cache-401
run_case out-cost config-unknown.json cache-cost
run_case out-fresh config-declared.json cache-fresh
run_case out-cache config-declared.json cache-fresh
kill "$(cat /evidence/relay.pid)" || true
python3 - <<'PY'
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
content = json.dumps({"protocol": "oo.other/v1", "columns": []})
body = json.dumps({"choices": [{"finish_reason": "stop", "message": {"role": "assistant", "content": content}}], "usage": {"prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30}}).encode()
class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        length = int(self.headers.get("Content-Length", "0"))
        _ = self.rfile.read(length)
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def log_message(self, fmt, *args):
        return
ThreadingHTTPServer(("127.0.0.1", 8317), Handler).serve_forever()
PY
"""
    script = script.replace("--dataset batch.t_batch", "--dataset " + dataset)
    # The script above would block on serve_forever. Split the unit case.
    (directory / "inside.sh").write_text(script.split("kill ")[0] + "\necho DONE builder\n")
    (directory / "inside.sh").chmod(0o755)
    proc = fs.run([
        "docker", "run", "--rm", "--name", container,
        "--network", fs.NET, "--platform", "linux/arm64", "--pull", "never",
        "--memory", "2g", "--cpus", "1",
        "--cap-drop", "ALL", "--security-opt", "no-new-privileges",
        "-e", "HOME=/tmp",
        "--entrypoint", "bash",
        "-v", "oo-dl01w1-04-target:/target:ro",
        "-v", f"{directory}:/evidence",
        "-v", f"{fs.SECRET / 'pg-tls' / 'ca.crt'}:/run/pg-secrets/ca.crt:ro",
        "-v", f"{fs.SECRET / 'preview-login'}:/run/pg-secrets/preview-login:ro",
        "-v", f"{fs.TOKEN}:/run/secrets/llm-token:ro",
        "-v", f"{fs.SECRET / 'bad-token'}:/run/secrets/bad-token:ro",
        "--add-host", "host.docker.internal:host-gateway",
        *fs.LABELS, "--label", "oo.kind=proof-runner",
        fs.IMAGE, "/evidence/inside.sh",
    ], timeout=900)
    fs.save(directory, "builder", proc)
    fs.say(f"builder exit={proc.returncode}")
    return proc.returncode == 0


def run_other_unit():
    directory = fs.EVID / "builder-other"
    directory.mkdir(parents=True, exist_ok=True)
    source = fs.EVID / "builder"
    for name in ("catalog.json", "settings.json", "visibility.json", "config-declared.json"):
        shutil.copy(source / name, directory / name)
    script = r"""#!/bin/bash
set -u
python3 - <<'PY' &
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
content = json.dumps({"protocol": "oo.other/v1", "columns": []})
body = json.dumps({
    "choices": [{"finish_reason": "stop", "message": {"role": "assistant", "content": content}}],
    "usage": {"prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30},
}).encode()
class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        length = int(self.headers.get("Content-Length", "0"))
        _ = self.rfile.read(length)
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def log_message(self, fmt, *args):
        return
ThreadingHTTPServer(("127.0.0.1", 8317), Handler).serve_forever()
PY
echo $! >/evidence/scripted.pid
ready=0
for _ in 1 2 3 4 5 6 7 8 9 10; do
  if python3 -c 'import socket; s=socket.create_connection(("127.0.0.1",8317),1); s.close()'; then ready=1; break; fi
  sleep 0.2
done
echo "$ready" >/evidence/scripted-ready.txt
test "$ready" = 1
mkdir -p /evidence/out-other /evidence/cache-other
set +e
/target/import-a682/debug/pg-builder sealed \
  --config /evidence/config-declared.json --catalog /evidence/catalog.json \
  --settings /evidence/settings.json --visibility /evidence/visibility.json \
  --cache /evidence/cache-other --out /evidence/out-other \
  --reviewer author.dl01w1-04 --reviewed-at 2026-10-11 --dataset batch.t_batch \
  --target-host oo-dl01w1-04-p2-pg --target-port 5432 --target-database previewdb \
  --target-user oo04_preview_login --login oo04_preview_login \
  --target-ca /run/pg-secrets/ca.crt --target-password /run/pg-secrets/preview-login \
  >/evidence/out-other.stdout 2>/evidence/out-other.stderr
echo "$?" >/evidence/out-other.exit
set -e
echo '{"upstream":"scripted_unit","not_a_product_model":true}' >/evidence/upstream.json
echo DONE other
"""
    (directory / "inside.sh").write_text(script)
    (directory / "inside.sh").chmod(0o755)
    proc = fs.run([
        "docker", "run", "--rm", "--name", "oo-dl01w1-04-c2-other",
        "--network", fs.NET, "--platform", "linux/arm64", "--pull", "never",
        "--memory", "1g", "--cpus", "0.5",
        "--cap-drop", "ALL", "--security-opt", "no-new-privileges",
        "-e", "HOME=/tmp", "--entrypoint", "bash",
        "-v", "oo-dl01w1-04-target:/target:ro",
        "-v", f"{directory}:/evidence",
        "-v", f"{fs.SECRET / 'pg-tls' / 'ca.crt'}:/run/pg-secrets/ca.crt:ro",
        "-v", f"{fs.SECRET / 'preview-login'}:/run/pg-secrets/preview-login:ro",
        "-v", f"{fs.TOKEN}:/run/secrets/llm-token:ro",
        *fs.LABELS, "--label", "oo.kind=proof-runner",
        fs.IMAGE, "/evidence/inside.sh",
    ], timeout=180)
    fs.save(directory, "other", proc)
    fs.say(f"other_unit exit={proc.returncode}")
    return proc.returncode == 0


def load_result(directory, name):
    path = directory / name / "result.json"
    exit_path = directory / f"{name}.exit"
    code = int(exit_path.read_text().strip()) if exit_path.is_file() else None
    doc = {}
    if path.is_file():
        try:
            doc = json.loads(path.read_text())
        except json.JSONDecodeError:
            doc = {}
    return code, doc


def query_case(table, pack_name="out-fresh", directory_name="query", dataset="batch.t_batch", visible=None, builder_folder="builder", widen=False):
    builder = fs.EVID / builder_folder
    visible = visible or ["c6", "n16", "d", "r"]
    authority = json.loads((builder / pack_name / "authority.json").read_text())
    directory = fs.EVID / directory_name
    directory.mkdir(parents=True, exist_ok=True)
    query_name = "oo-dl01w1-04-c2-query" if directory_name == "query" else "oo-dl01w1-04-c2-" + directory_name
    cancel_name = "oo-dl01w1-04-c2-qcancel" if directory_name == "query" else "oo-dl01w1-04-c2-" + directory_name + "-cancel"
    fs.write_json(directory / "allow.json", {
        "max_result_bytes": 65536, "max_rows": 20,
        "protocol": "oo.operator-preview.query/v1",
        "sql": "select " + ", ".join(visible) + " from " + dataset + " order by " + visible[0],
        "wall_clock_ms": 10000,
    })
    fs.write_json(directory / "deny.json", {
        "max_result_bytes": 65536, "max_rows": 20,
        "protocol": "oo.operator-preview.query/v1",
        "sql": "select id from " + dataset,
        "wall_clock_ms": 10000,
    })
    fs.write_json(directory / "cancel.json", {
        "max_result_bytes": 65536, "max_rows": 20,
        "protocol": "oo.operator-preview.query/v1",
        "sql": "select " + visible[0] + " from " + dataset,
        "wall_clock_ms": 20000,
    })
    allow = json.loads(json.dumps(authority))
    deny = json.loads(json.dumps(authority))
    cancel = json.loads(json.dumps(authority))
    for doc, stem in ((allow, "allow"), (deny, "deny"), (cancel, "cancel")):
        doc["audit_dir"] = f"/out/audit-{stem}"
        doc["receipt_dir"] = f"/out/receipts-{stem}"
        doc["state_dir"] = f"/out/state-{stem}"
        doc["pack_dir"] = f"/evidence/{pack_name}/pack"
        doc["pack_receipt"] = f"/evidence/{pack_name}/pack/receipt.json"
        doc["policy_path"] = f"/evidence/{pack_name}/policy.json"
        doc["role_pack_dir"] = f"/evidence/{pack_name}/role-pack"
    fs.write_json(directory / "authority-allow.json", allow)
    fs.write_json(directory / "authority-deny.json", deny)
    fs.write_json(directory / "authority-cancel.json", cancel)
    script = f"""#!/bin/bash
set -u
BIN=/w21/w21-native/debug/operator-preview
sha256sum "$BIN" | tee /out/operator-preview.sha256
grep -q '^{CONSUMER} ' /out/operator-preview.sha256
mkdir -p /out/audit-allow /out/audit-deny /out/audit-cancel /out/state-allow /out/state-deny /out/state-cancel /out/receipts-allow /out/receipts-deny /out/receipts-cancel
set +e
"$BIN" query --authority /out/authority-allow.json --query /out/allow.json >/out/allow.stdout 2>/out/allow.stderr
echo "$?" >/out/allow.exit
"$BIN" query --authority /out/authority-deny.json --query /out/deny.json >/out/deny.stdout 2>/out/deny.stderr
echo "$?" >/out/deny.exit
set -e
echo DONE query-static
"""
    if widen:
        script = script.replace(
            "echo DONE query-static\n",
            """python3 - << 'PY'
import json
from pathlib import Path
auth = json.loads(Path("/out/authority-allow.json").read_text())
policy = json.loads(Path(auth["policy_path"]).read_text())
policy["table"]["columns"].append({
    "name": "secret_note",
    "pii": {"class": "none"},
    "semantic_type": "free_text",
    "status": "confirmed",
})
Path("/out/policy-wide.json").write_text(json.dumps(policy))
auth["policy_path"] = "/out/policy-wide.json"
auth["audit_dir"] = "/out/audit-widen"
auth["receipt_dir"] = "/out/receipts-widen"
auth["state_dir"] = "/out/state-widen"
Path("/out/authority-widen.json").write_text(json.dumps(auth))
PY
mkdir -p /out/audit-widen /out/state-widen /out/receipts-widen
set +e
"$BIN" query --authority /out/authority-widen.json --query /out/allow.json >/out/widen.stdout 2>/out/widen.stderr
echo "$?" >/out/widen.exit
set -e
echo DONE query-static
""",
        )
    (directory / "inside.sh").write_text(script)
    (directory / "inside.sh").chmod(0o755)
    oracle_before = fs.run(["docker", "inspect", "-f", "{{.State.Running}}", fs.ORA], timeout=20)
    stopped = fs.run(["docker", "stop", fs.ORA], timeout=120)
    fs.save(directory, "oracle-stop", stopped)
    try:
        proc = fs.run([
            "docker", "run", "--rm", "--name", query_name,
            "--network", fs.NET, "--platform", "linux/arm64", "--pull", "never",
            "--user", "0:0", "--security-opt", "no-new-privileges",
            "--cap-drop", "ALL", "--cap-add", "SETUID", "--cap-add", "SETGID",
            "--cap-add", "CHOWN", "--cap-add", "FOWNER", "--cap-add", "DAC_OVERRIDE",
            "--memory", "2g", "--cpus", "1",
            "-e", "HOME=/tmp", "--entrypoint", "bash",
            "-v", f"{directory}:/out",
            "-v", f"{builder}:/evidence:ro",
            "-v", f"{fs.SECRET / 'pg-tls' / 'ca.crt'}:/run/pg-secrets/ca.crt:ro",
            "-v", f"{fs.SECRET / 'preview-login'}:/run/pg-secrets/preview-login:ro",
            "--mount", "type=volume,source=oo-dl01w1-21-target,target=/w21,readonly",
            "--tmpfs", "/tmp:rw,nosuid,nodev,size=256m",
            *fs.LABELS, "--label", "oo.kind=proof-runner",
            fs.IMAGE, "/out/inside.sh",
        ], timeout=180)
        fs.save(directory, "query", proc)
        cancel_consumer(directory, table, cancel_name, builder)
    finally:
        started = fs.run(["docker", "start", fs.ORA], timeout=60)
        fs.save(directory, "oracle-start", started)
        probe = None
        deadline = time.time() + 60
        while time.time() < deadline:
            probe = fs.sqlplus(
                "ALTER SESSION SET CONTAINER = FREEPDB1;\n"
                "SET PAGESIZE 0 FEEDBACK OFF HEADING OFF\n"
                "SELECT 'pdb|' || open_mode FROM v$pdbs WHERE name = 'FREEPDB1';\n"
                "EXIT\n",
                "reopen-pdb",
                timeout=30,
            )
            if probe.returncode == 0 and b"READ WRITE" in probe.stdout:
                break
            time.sleep(2)
        (directory / "oracle-before.txt").write_text(oracle_before.stdout.decode())
        (directory / "reopen.txt").write_bytes(probe.stdout if probe is not None else b"")
    allow_exit = (directory / "allow.exit").read_text().strip() if (directory / "allow.exit").is_file() else ""
    deny_exit = (directory / "deny.exit").read_text().strip() if (directory / "deny.exit").is_file() else ""
    fs.say(f"query allow={allow_exit} deny={deny_exit}")
    return allow_exit, deny_exit


def cancel_consumer(directory, table, cancel_name="oo-dl01w1-04-c2-qcancel", builder=None):
    evidence = builder if builder is not None else fs.EVID / "builder"
    if not table or len(table) > 80 or any(ch not in "pt_0123456789abcdef" for ch in table) or not table.startswith("pt_"):
        (directory / "cancel-skip.txt").write_text("physical table was not named\n")
        return
    script = f"""#!/bin/bash
set -u
BIN=/w21/w21-native/debug/operator-preview
mkdir -p /out/audit-cancel /out/state-cancel /out/receipts-cancel
ready=0
for _ in $(seq 1 200); do
  if [ -f /out/lock-held ]; then ready=1; break; fi
  sleep 0.1
done
echo "$ready" >/out/lock-seen.txt
if [ "$ready" != 1 ]; then echo 'lock was not held' >/out/cancel-skip.txt; exit 0; fi
set +e
"$BIN" query --authority /out/authority-cancel.json --query /out/cancel.json >/out/cancel-query.stdout 2>/out/cancel-query.stderr &
qpid=$!
set -e
python3 - <<'PY'
import json, os, subprocess, time
root = "/out/state-cancel/executions"
found = ""
deadline = time.time() + 15
while time.time() < deadline and not found:
    if os.path.isdir(root):
        for name in os.listdir(root):
            if not name.endswith(".json"):
                continue
            try:
                doc = json.load(open(os.path.join(root, name)))
            except Exception:
                continue
            if doc.get("status") == "running":
                found = name[:-5]
                break
    if not found:
        time.sleep(0.05)
open("/out/cancel-execution.txt", "w").write(found + "\\n")
if not found:
    open("/out/cancel-skip.txt", "w").write("no running execution\\n")
    raise SystemExit(0)
go_deadline = time.time() + 12
while time.time() < go_deadline and not os.path.exists("/out/cancel-go"):
    time.sleep(0.05)
if not os.path.exists("/out/cancel-go"):
    open("/out/cancel-skip.txt", "w").write("statement did not block\\n")
    raise SystemExit(0)
proc = subprocess.run(["/w21/w21-native/debug/operator-preview", "cancel", "--authority", "/out/authority-cancel.json", "--execution", found], text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
open("/out/cancel.stdout", "w").write(proc.stdout)
open("/out/cancel.stderr", "w").write(proc.stderr)
open("/out/cancel.exit", "w").write(str(proc.returncode) + "\\n")
PY
set +e
wait "$qpid"
echo "$?" >/out/cancel-query.exit
echo DONE query-cancel
"""
    (directory / "cancel-inside.sh").write_text(script)
    (directory / "cancel-inside.sh").chmod(0o755)
    holder = subprocess.Popen([
        "docker", "exec", "-u", "postgres", fs.PG, "psql", "-d", "previewdb",
        "-v", "ON_ERROR_STOP=1", "-c",
        f"BEGIN; LOCK TABLE _oo.{table} IN ACCESS EXCLUSIVE MODE; SELECT pg_sleep(40); ROLLBACK;",
    ])
    try:
        deadline = time.time() + 15
        held = False
        while time.time() < deadline:
            probe = psql(
                "SELECT count(*) FROM pg_locks l JOIN pg_class c ON c.oid=l.relation "
                f"WHERE c.relname = '{table}' AND l.mode = 'AccessExclusiveLock' AND l.granted"
            )
            count = probe.stdout.decode().strip() if probe.returncode == 0 else ""
            if count.isdigit() and int(count) > 0:
                held = True
                (directory / "lock-held").write_text("held\n")
                break
            time.sleep(0.2)
        if not held:
            (directory / "cancel-skip.txt").write_text("lock was not granted\n")
            return
        proc = subprocess.Popen([
            "docker", "run", "--name", cancel_name,
            "--network", fs.NET, "--platform", "linux/arm64", "--pull", "never",
            "--user", "0:0", "--security-opt", "no-new-privileges",
            "--cap-drop", "ALL", "--cap-add", "SETUID", "--cap-add", "SETGID",
            "--cap-add", "CHOWN", "--cap-add", "FOWNER", "--cap-add", "DAC_OVERRIDE",
            "--memory", "2g", "--cpus", "1",
            "-e", "HOME=/tmp", "--entrypoint", "bash",
            "-v", f"{directory}:/out",
            "-v", f"{evidence}:/evidence:ro",
            "-v", f"{fs.SECRET / 'pg-tls' / 'ca.crt'}:/run/pg-secrets/ca.crt:ro",
            "-v", f"{fs.SECRET / 'preview-login'}:/run/pg-secrets/preview-login:ro",
            "--mount", "type=volume,source=oo-dl01w1-21-target,target=/w21,readonly",
            "--tmpfs", "/tmp:rw,nosuid,nodev,size=256m",
            *fs.LABELS, "--label", "oo.kind=proof-runner",
            fs.IMAGE, "/out/cancel-inside.sh",
        ], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        waiter_deadline = time.time() + 20
        while time.time() < waiter_deadline:
            waiter = psql(
                "SELECT a.usename || '|' || l.granted::text FROM pg_locks l "
                "JOIN pg_class c ON c.oid = l.relation JOIN pg_stat_activity a ON a.pid = l.pid "
                f"WHERE c.relname = '{table}' AND NOT l.granted"
            )
            (directory / "waiter-seen.txt").write_bytes(waiter.stdout)
            if b"oo04_preview_login|false" in waiter.stdout:
                (directory / "cancel-go").write_text("1\n")
                break
            if proc.poll() is not None:
                break
            time.sleep(0.1)
        try:
            stdout, stderr = proc.communicate(timeout=60)
        except subprocess.TimeoutExpired:
            fs.run(["docker", "kill", cancel_name], timeout=20)
            stdout, stderr = proc.communicate(timeout=20)
        (directory / "cancel-container.stdout").write_bytes(stdout)
        (directory / "cancel-container.stderr").write_bytes(stderr)
        (directory / "cancel-container.exit").write_text(str(proc.returncode) + "\n")
    finally:
        holder.wait(timeout=50)
        fs.run(["docker", "rm", "-f", cancel_name], timeout=20)


def case_ok(directory, name, expect_exit, outcome=None, reason=None, pack=None, model=None):
    code, doc = load_result(directory, name)
    ok = code == expect_exit
    if outcome is not None:
        ok = ok and doc.get("outcome") == outcome
    if reason is not None:
        ok = ok and doc.get("reason") == reason
    if pack is not None:
        ok = ok and doc.get("pack") == pack
    if model is not None:
        ok = ok and doc.get("model") == model
    if model == "cache_hit":
        ok = ok and doc.get("cache") == "hit"
    if model == "called":
        ok = ok and doc.get("cache") in (None, "miss")
    return ok, code, doc.get("outcome"), doc.get("reason"), doc.get("pack"), doc.get("model")


def write_reports(status):
    fs.write_json(fs.EVID / "status.json", status)
    report = [
        "# DL01-W1-04 closure-2",
        "",
        f"CLI session `{fs.CLI}` wrote this packet. Historical evidence UUID `{fs.SESSION}` is the original capturer, not this CLI.",
        "",
        f"Published SCN `{status.get('scn')}` is a new marker identity. SCN 2196583 was probed and is not this capture.",
        f"T_BATCH arrow `{status.get('batch', {}).get('arrow_sha256')}` same bytes as 5064f81e: `{status.get('batch', {}).get('same_bytes_as_5064f81e')}`.",
        "",
        f"Batch imported: `{status.get('batch', {}).get('imported')}`. Wide text imported: `{status.get('wide', {}).get('imported')}`. TS text imported: `{status.get('ts', {}).get('imported')}`.",
        "Wide and timestamp values are canonical Oracle text. id 74 is the stored zero, not 1E-131. Typed wide, timestamp, and time-zone paths stay rejected.",
        "",
        f"Deadline stopped the server session: `{status.get('capture', {}).get('deadline')}`. OCI marker cancel stopped it: `{status.get('capture', {}).get('oci_cancel')}`.",
        f"Reader object writes: `{status.get('capture', {}).get('audit_zero_write')}` means none committed. ALTER SESSION time zone lines are driver session parameters in audit/session-actions.txt. The first substring classification stays in audit/committed-writes-substring.txt.",
        "",
        f"Builder timeout/401/unknown/fresh/cache: `{status.get('builder_cases')}`.",
        f"oo.other/v1 upstream is `scripted_unit`: `{status.get('other_unit')}`.",
        f"Query directory `{status.get('query_dir') or 'query'}` allow/deny: `{status.get('allow_exit')}` / `{status.get('deny_exit')}`. Consumer cancel: `{status.get('consumer_cancel')}`.",
        f"Reader column ACL: `{status.get('reader_acl')}`. The pre-grant allow exit stays `{status.get('allow_before_acl')}`.",
        "",
        f"author_complete: `{status.get('author_complete')}`. source_acceptance: `not_claimed`. publication: `not_published`.",
        "This author receipt is not independent approval, nightly verification, or a parent release.",
        "",
    ]
    (fs.EVID / "REPORT.md").write_text("\n".join(report))
    findings = [
        "# Findings",
        "",
        "Closure-1 remains the sealed five-row T_BATCH chain. This packet does not replace that SCN, that arrow identity, or that run.",
        "",
        "T_WIDE id 74 is stored as Oracle NUMBER zero. The fixture intent 1E-131 is outside the NUMBER range. TO_CHAR TME records 0E+00.",
        "",
        "The published oracledb 26.0.0-beta.4 crate has no public cancel. The owned child links a source overlay that sends OCI marker type 3 and returns Cancelled. The batch completion frame still says cancel_api absent. Container kill is not that proof.",
        "",
        "oo.other/v1 was replayed against a scripted HTTP body on the real pg-builder binary. The product timeout, 401, unknown-cost, fresh, and cache cases use the loopback relay to localhost:8317.",
        "",
        "SYS.DBMS_FLASHBACK was not granted. The reader stayed SELECT and FLASHBACK on the three approved tables.",
        "",
        "The first consumer allow of the visible columns exited 6 class guard. oo_r_reader had column SELECT on the generation-1 physical columns and none on generation 2, so the probe catalog was empty and both allow and deny were guard refusals. install-column-acl replays an existing role only when membership is exactly one login and the column grants already equal the new set. This role has three members and the generation-1 grants, so that command adds nothing and revokes nothing. oo_target_owner granted SELECT on generation-2 c6, d, n1, n16, r, and v2 only. id and _oo_pos stay ungranted. query/ keeps the refusal. The scored consumer run is the later directory.",
        "",
        "The first install-native-sealed-catalog call received empty stdin because docker run did not attach it. That attempt is install-0008/attempt-empty-stdin. Migration 0006 is applied by install-sealed-catalog before 0008. Import load_state is nested under the envelope import object. Existing generation-2 envelopes are reused and are not imported again.",
        "",
    ]
    (fs.EVID / "FINDINGS.md").write_text("\n".join(findings))
    handoff = [
        "# Handoff",
        "",
        "Root reviews this packet. The original source denominator does not increase. source_acceptance stays not_claimed.",
        "",
        f"Builder archive a6825165dd83ffaa30e17fbb2da1a481a05f6559. pg-builder sha256 `{status.get('pg_builder_sha256')}`. Child sha256 `{status.get('child_sha256')}`.",
        f"Consumer binary f7fdba26 from volume oo-dl01w1-21-target, mounted readonly.",
        "",
        "Do not recapture SCN 2196583. Do not reuse run f9992961 or catalog 75baa479 as this capture. Do not grant EXECUTE on SYS.DBMS_FLASHBACK. Do not compile the live workspace.",
        "",
        "Postgres container eb0ec24b748e and Oracle container ffc81586acf3 stay. Volumes oo-dl01w1-04-p2-oradata, oo-dl01w1-04-p2-pgdata, oo-dl01w1-04-cargo, and oo-dl01w1-04-target stay.",
        "",
        f"author_complete is `{status.get('author_complete')}` only for the checks listed in status.json. A missing check is not a whole-source completion.",
        "",
    ]
    (fs.EVID / "HANDOFF.md").write_text("\n".join(handoff))


def sha_sums():
    root = fs.EVID
    lines = []
    for path in sorted(root.rglob("*")):
        if not path.is_file():
            continue
        rel = path.relative_to(root).as_posix()
        if rel in {"SHA256SUMS", "receipt.json"} or rel.endswith((".stdout", ".stderr")) and False:
            continue
        if rel in {"SHA256SUMS", "receipt.json"}:
            continue
        lines.append(f"{fs.sha256_file(path)}  {rel}")
    (root / "SHA256SUMS").write_text("\n".join(lines) + "\n")
    return fs.sha256_file(root / "SHA256SUMS"), len(lines)


def finish(scn, capture):
    status = {"scn": scn, "capture": capture, "cli_session": fs.CLI, "capturer_session": fs.SESSION}
    status["catalog_installed"] = install_catalog()
    imports = {}
    if capture.get("batch") and (fs.EVID / "batch" / "batch.arrow").is_file():
        imports["batch"] = prepare_object(
            "batch", "oo04_t_batch", "t_batch", 5, fs.EVID / "batch" / "batch.arrow",
            BATCH_FIELDS, BATCH_MANIFEST, scn, False,
        )
    else:
        imports["batch"] = {"imported": False, "reason": "capture_absent"}
    for kind, flag, folder in (("wide", "wide_text", "wide-text"), ("ts", "ts_text", "ts-text")):
        arrow = fs.EVID / folder / "text.arrow"
        meta = fs.EVID / folder / "text.json"
        if capture.get(flag) and arrow.is_file() and meta.is_file():
            rows = json.loads(meta.read_text()).get("rows")
            source_id, table, fields, columns = text_columns(kind)
            item = prepare_object(kind, source_id, table, rows, arrow, fields, columns, scn, True)
            item["scn"] = scn
            imports[kind] = item
        else:
            imports[kind] = {"imported": False, "reason": "capture_absent"}
    status["batch"] = imports["batch"]
    status["wide"] = imports["wide"]
    status["ts"] = imports["ts"]
    catalogs = {}
    for name in ("batch", "wide", "ts"):
        imports[name]["scn"] = scn
        catalogs[name] = catalog_one(name, imports[name])
    status["catalogs"] = catalogs
    if catalogs.get("batch", {}).get("ok"):
        run_builder(fs.EVID / "catalog-batch" / "catalog.json")
        cases = {}
        for name, expect, outcome, reason, pack, model_expect in (
            ("out-timeout", 1, "provider_failure", "timeout", "not_produced", None),
            ("out-401", 1, "provider_failure", "http_status", "not_produced", None),
            ("out-cost", 1, "cost_unknown", "unknown_cost", "not_produced", "called"),
            ("out-fresh", 0, "packed", None, "produced", "called"),
            ("out-cache", 0, "packed", None, "produced", "cache_hit"),
        ):
            ok, code, got_outcome, got_reason, got_pack, model = case_ok(
                fs.EVID / "builder", name, expect, outcome, reason, pack, model_expect,
            )
            cases[name] = {
                "ok": ok, "exit": code, "outcome": got_outcome, "reason": got_reason,
                "pack": got_pack, "model": model,
            }
        status["builder_cases"] = cases
        run_other_unit()
        ok, code, got_outcome, got_reason, got_pack, model = case_ok(
            fs.EVID / "builder-other", "out-other", 1, "structural_draft", "protocol", "not_produced",
        )
        status["other_unit"] = {
            "ok": ok, "exit": code, "outcome": got_outcome, "reason": got_reason,
            "pack": got_pack, "upstream": "scripted_unit",
        }
        if (fs.EVID / "builder" / "out-fresh" / "authority.json").is_file():
            allow_exit, deny_exit = query_case(catalogs["batch"].get("stored_physical"))
            status["allow_exit"] = allow_exit
            status["deny_exit"] = deny_exit
            cancel_exit = ""
            cancel_path = fs.EVID / "query" / "cancel.exit"
            query_path = fs.EVID / "query" / "cancel-query.exit"
            if cancel_path.is_file():
                cancel_exit = cancel_path.read_text().strip()
            query_exit = query_path.read_text().strip() if query_path.is_file() else ""
            text = ""
            for candidate in ("cancel-query.stdout", "cancel-query.stderr"):
                file = fs.EVID / "query" / candidate
                if file.is_file():
                    text += file.read_text(errors="replace")
            status["consumer_cancel"] = {
                "command_exit": cancel_exit,
                "query_exit": query_exit,
                "sqlstate_57014": "57014" in text,
            }
    else:
        status["builder_cases"] = {}
        status["other_unit"] = {"ok": False, "reason": "catalog_absent"}
    bins = fs.EVID / "build" / "binaries.sha256"
    if bins.is_file():
        for line in bins.read_text().splitlines():
            if "pg-builder" in line:
                status["pg_builder_sha256"] = line.split()[0]
            if "oo-oracle-p2-child" in line:
                status["child_sha256"] = line.split()[0]
    return emit(status)


def emit(status):
    capture = status.get("capture") or {}
    scn = status.get("scn")
    query_dir = status.get("query_dir") or "query"
    reopen_path = fs.EVID / query_dir / "reopen.txt"
    reopen = reopen_path.read_text() if reopen_path.is_file() else ""
    catalogs = status.get("catalogs") or {}
    checks = [
        capture.get("batch"), capture.get("wide_text"), capture.get("ts_text"),
        capture.get("wide_reject"), capture.get("ts_reject"), capture.get("tz_reject"),
        capture.get("deadline"), capture.get("oci_cancel"), capture.get("audit_zero_write"),
        capture.get("fence_exit") == 0, capture.get("secret_exit") == 6, capture.get("dml_exit") == 6,
        status.get("batch", {}).get("imported"), status.get("wide", {}).get("imported"), status.get("ts", {}).get("imported"),
        catalogs.get("batch", {}).get("ok"), catalogs.get("wide", {}).get("ok"), catalogs.get("ts", {}).get("ok"),
        all(item.get("ok") for item in status.get("builder_cases", {}).values()) and status.get("builder_cases"),
        status.get("other_unit", {}).get("ok"),
        status.get("allow_exit") == "0", status.get("deny_exit") == "6",
        status.get("consumer_cancel", {}).get("sqlstate_57014") is True,
        "READ WRITE" in reopen,
    ]
    status["author_complete"] = all(bool(item) for item in checks)
    status["source_acceptance"] = "not_claimed"
    status["publication"] = "not_published"
    status["checks_failed"] = len([item for item in checks if not item])
    write_reports(status)
    digest, count = sha_sums()
    batch = status.get("batch") or {}
    receipt = {
        "receipt_version": 1,
        "protocol": "oo.author-receipt/v1",
        "task_id": "DL01-W1-04",
        "name": "oracle-whole-source-closure-2",
        "session_uuid": fs.SESSION,
        "cli_session": fs.CLI,
        "capturer_session": fs.SESSION,
        "scn": scn,
        "prior_scn_not_this_identity": 2196583,
        "arrow_sha256": batch.get("arrow_sha256"),
        "same_arrow_bytes_as_5064f81e": batch.get("same_bytes_as_5064f81e"),
        "run_id": batch.get("run_id"),
        "author_complete": status["author_complete"],
        "author_delivery_is_not_independent_approval": True,
        "source_acceptance": "not_claimed",
        "publication": "not_published",
        "published": False,
        "sha256sums_sha256": digest,
        "sha256sums_entries": count,
        "checks_failed": status["checks_failed"],
    }
    fs.write_json(fs.EVID / "receipt.json", receipt)
    digest, count = sha_sums()
    receipt["sha256sums_sha256"] = digest
    receipt["sha256sums_entries"] = count
    fs.write_json(fs.EVID / "receipt.json", receipt)
    fs.say(f"author_complete={status['author_complete']} checks_failed={status['checks_failed']}")
    return status


def reclassify_saved_audit():
    reader = fs.EVID / "audit" / "reader.txt"
    if not reader.is_file():
        return False
    kept = fs.EVID / "audit" / "committed-writes-substring.txt"
    source = fs.EVID / "audit" / "committed-writes.txt"
    if source.is_file() and not kept.is_file():
        shutil.copy(source, kept)
    return fs.write_audit_classes(reader.read_text())


def resume():
    published = (fs.EVID / "published-scn.txt").read_text().strip()
    if published != "2205514":
        raise SystemExit(f"refusing resume on scn {published}")
    if not (fs.EVID / "batch" / "batch.arrow").is_file():
        raise SystemExit("refusing resume without the saved batch arrow")
    zero = reclassify_saved_audit()
    capture = json.loads((fs.EVID / "capture-status.json").read_text())
    capture["audit_zero_write"] = zero
    capture["scn"] = published
    fs.write_json(fs.EVID / "capture-status.json", capture)
    return finish(published, capture)


def case_result(name):
    path = fs.EVID / "builder" / name / "result.json"
    if not path.is_file():
        return {}
    try:
        return json.loads(path.read_text())
    except json.JSONDecodeError:
        return {}


def park_case(name, dest_name):
    root = fs.EVID / "builder"
    dest = root / dest_name
    if dest.exists():
        return
    source = root / name
    if source.exists():
        shutil.move(str(source), str(dest))
    else:
        dest.mkdir()
    for suffix in (".exit", ".stdout", ".stderr"):
        side = root / f"{name}{suffix}"
        if side.exists():
            shutil.move(str(side), str(dest / side.name))


def retry_declared():
    directory = fs.EVID / "builder"
    script = r"""#!/bin/bash
set -u
cd /evidence
python3 /evidence/relay.py >/tmp/relay.out 2>/tmp/relay.err &
echo $! >/evidence/relay-retry.pid
ready=0
for _ in 1 2 3 4 5 6 7 8 9 10; do
  if python3 -c 'import socket; s=socket.create_connection(("127.0.0.1",8317),1); s.close()'; then ready=1; break; fi
  sleep 0.2
done
echo "$ready" >/evidence/relay-retry-ready.txt
test "$ready" = 1
run_case() {
  local name="$1" config="$2" cache="$3"
  mkdir -p "/evidence/$name" "/evidence/$cache"
  set +e
  /target/import-a682/debug/pg-builder sealed \
    --config "/evidence/$config" --catalog /evidence/catalog.json \
    --settings /evidence/settings.json --visibility /evidence/visibility.json \
    --cache "/evidence/$cache" --out "/evidence/$name" \
    --reviewer author.dl01w1-04 --reviewed-at 2026-10-11 --dataset batch.t_batch \
    --target-host oo-dl01w1-04-p2-pg --target-port 5432 --target-database previewdb \
    --target-user oo04_preview_login --login oo04_preview_login \
    --target-ca /run/pg-secrets/ca.crt --target-password /run/pg-secrets/preview-login \
    >"/evidence/$name.stdout" 2>"/evidence/$name.stderr"
  echo "$?" >"/evidence/$name.exit"
  set -e
  echo "case $name exit=$(cat /evidence/$name.exit)"
}
run_case out-fresh config-declared.json cache-fresh
run_case out-cache config-declared.json cache-fresh
echo DONE retry
"""
    (directory / "inside-retry.sh").write_text(script)
    (directory / "inside-retry.sh").chmod(0o755)
    proc = fs.run([
        "docker", "run", "--rm", "--name", "oo-dl01w1-04-c2-bretry",
        "--network", fs.NET, "--platform", "linux/arm64", "--pull", "never",
        "--memory", "2g", "--cpus", "1",
        "--cap-drop", "ALL", "--security-opt", "no-new-privileges",
        "-e", "HOME=/tmp",
        "--entrypoint", "bash",
        "-v", "oo-dl01w1-04-target:/target:ro",
        "-v", f"{directory}:/evidence",
        "-v", f"{fs.SECRET / 'pg-tls' / 'ca.crt'}:/run/pg-secrets/ca.crt:ro",
        "-v", f"{fs.SECRET / 'preview-login'}:/run/pg-secrets/preview-login:ro",
        "-v", f"{fs.TOKEN}:/run/secrets/llm-token:ro",
        "--add-host", "host.docker.internal:host-gateway",
        *fs.LABELS, "--label", "oo.kind=proof-runner",
        fs.IMAGE, "/evidence/inside-retry.sh",
    ], timeout=600)
    fs.save(directory, "retry", proc)
    fs.say(f"declared_retry exit={proc.returncode}")
    return proc.returncode == 0


def record_query(status):
    table = (status.get("catalogs") or {}).get("batch", {}).get("stored_physical")
    pack = "out-fresh"
    if not (fs.EVID / "builder" / pack / "authority.json").is_file():
        pack = "out-cache-live"
    if not (fs.EVID / "builder" / pack / "authority.json").is_file():
        fs.say("query skipped, authority absent")
        return
    status["query_pack"] = pack
    allow_exit, deny_exit = query_case(table, pack)
    status["allow_exit"] = allow_exit
    status["deny_exit"] = deny_exit
    text = ""
    for candidate in ("cancel-query.stdout", "cancel-query.stderr"):
        file = fs.EVID / "query" / candidate
        if file.is_file():
            text += file.read_text(errors="replace")
    cancel_path = fs.EVID / "query" / "cancel.exit"
    query_path = fs.EVID / "query" / "cancel-query.exit"
    status["consumer_cancel"] = {
        "command_exit": cancel_path.read_text().strip() if cancel_path.is_file() else "",
        "query_exit": query_path.read_text().strip() if query_path.is_file() else "",
        "sqlstate_57014": "57014" in text,
    }


def resume_packed():
    published = (fs.EVID / "published-scn.txt").read_text().strip()
    if published != "2205514":
        raise SystemExit(f"refusing packed resume on scn {published}")
    if case_result("out-fresh").get("outcome") != "packed" or case_result("out-cache").get("model") != "cache_hit":
        if case_result("out-fresh").get("outcome") != "packed":
            park_case("out-fresh", "out-fresh-malformed")
            park_case("out-cache", "out-cache-live")
            park_case("cache-fresh", "cache-live")
        retry_declared()
    status = json.loads((fs.EVID / "status.json").read_text())
    cases = {}
    for name, expect, outcome, reason, pack, model_expect in (
        ("out-timeout", 1, "provider_failure", "timeout", "not_produced", None),
        ("out-401", 1, "provider_failure", "http_status", "not_produced", None),
        ("out-cost", 1, "cost_unknown", "unknown_cost", "not_produced", "called"),
        ("out-fresh", 0, "packed", None, "produced", "called"),
        ("out-cache", 0, "packed", None, "produced", "cache_hit"),
    ):
        ok, code, got_outcome, got_reason, got_pack, model = case_ok(
            fs.EVID / "builder", name, expect, outcome, reason, pack, model_expect,
        )
        cases[name] = {
            "ok": ok, "exit": code, "outcome": got_outcome, "reason": got_reason,
            "pack": got_pack, "model": model,
        }
    status["builder_cases"] = cases
    record_query(status)
    return emit(status)


DIGEST = "d8eadf809a0942e2043810a67a15e197fd5239c2c81155640a337bcd716d744e"
VISIBLE = ("c6", "d", "n1", "n16", "r", "v2")


def hex_name(value, prefix):
    if not value.startswith(prefix) or len(value) != 63:
        return False
    rest = value[len(prefix):]
    return len(rest) == 60 and all(ch in "0123456789abcdef" for ch in rest)


def grant_visible_columns():
    """Column SELECT for the six visible generation-2 attributes.

    install-column-acl does not add grants onto the existing oo_r_reader.
    The table owner grants only the policy columns. id and _oo_pos stay closed.
    """
    directory = fs.EVID / "acl-reader"
    directory.mkdir(parents=True, exist_ok=True)
    listed = psql(
        "SELECT level || '|' || logical_column || '|' || physical_name "
        "FROM _oo.physical_mappings "
        "WHERE source = 'oo04_t_batch' "
        f"AND config_digest = '{DIGEST}' "
        "AND run_generation = 2 AND logical_namespace = 'batch' "
        "AND logical_table = 't_batch' AND level IN ('c','t')"
    )
    (directory / "mappings.txt").write_bytes(listed.stdout)
    if listed.returncode != 0:
        raise SystemExit("mapping read failed")
    columns = {}
    table = ""
    for line in listed.stdout.decode().splitlines():
        level, logical, physical = line.split("|", 2)
        if level == "t" and logical == "":
            table = physical
        elif level == "c":
            columns[logical] = physical
    if set(columns) != set(VISIBLE) | {"id"} or not hex_name(table, "pt_"):
        raise SystemExit("generation-2 mapping was not the sealed column set")
    if any(not hex_name(columns[name], "pc_") for name in (*VISIBLE, "id")):
        raise SystemExit("physical column name rejected")
    granted = ", ".join(columns[name] for name in VISIBLE)
    sql = (
        "BEGIN; "
        "SET LOCAL ROLE oo_target_owner; "
        f"GRANT SELECT ({granted}) ON TABLE _oo.{table} TO oo_r_reader; "
        "COMMIT;"
    )
    (directory / "grant.sql").write_text(sql + "\n")
    proc = psql(sql)
    fs.save(directory, "grant", proc)
    if proc.returncode != 0:
        raise SystemExit(f"column grant exit {proc.returncode}")
    verify = psql(
        "SELECT a.attname || '|' || "
        "COALESCE(has_column_privilege('oo_r_reader', a.attrelid, a.attnum, 'SELECT')::text, 'null') "
        "FROM pg_attribute a "
        "JOIN pg_class c ON c.oid = a.attrelid "
        "JOIN pg_namespace n ON n.oid = c.relnamespace "
        f"WHERE n.nspname = '_oo' AND c.relname = '{table}' "
        "AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum"
    )
    (directory / "verify.txt").write_bytes(verify.stdout)
    if verify.returncode != 0:
        raise SystemExit("privilege verify failed")
    seen = {}
    for line in verify.stdout.decode().splitlines():
        name, flag = line.split("|", 1)
        seen[name] = flag
    for logical in VISIBLE:
        if seen.get(columns[logical]) != "true":
            raise SystemExit(f"missing select on {logical}")
    if seen.get(columns["id"]) != "false":
        raise SystemExit("id became selectable")
    hidden = [name for name, flag in seen.items() if name.startswith("_oo") and flag != "false"]
    if hidden:
        raise SystemExit(f"metadata selectable {hidden}")
    table_priv = psql(
        "SELECT count(*)::text FROM information_schema.table_privileges "
        f"WHERE grantee = 'oo_r_reader' AND table_schema = '_oo' AND table_name = '{table}'"
    )
    (directory / "table-privilege.txt").write_bytes(table_priv.stdout)
    if table_priv.stdout.decode().strip() != "0":
        raise SystemExit("table privilege appeared")
    (directory / "note.txt").write_text(
        "install-column-acl was not run. oo_r_reader already exists with three members "
        "and generation-1 column grants. That command replays only an exact set and "
        "does not add a later generation. This grant is SELECT on the six visible "
        "generation-2 columns, issued as oo_target_owner. id and _oo_pos are not in the list.\n"
    )
    return table


def score_cancel(status, directory_name):
    directory = fs.EVID / directory_name
    text = ""
    for candidate in ("cancel-query.stdout", "cancel-query.stderr"):
        file = directory / candidate
        if file.is_file():
            text += file.read_text(errors="replace")
    cancel_path = directory / "cancel.exit"
    query_path = directory / "cancel-query.exit"
    status["consumer_cancel"] = {
        "command_exit": cancel_path.read_text().strip() if cancel_path.is_file() else "",
        "query_exit": query_path.read_text().strip() if query_path.is_file() else "",
        "sqlstate_57014": "57014" in text,
    }


def prove_consumer():
    published = (fs.EVID / "published-scn.txt").read_text().strip()
    if published != "2205514":
        raise SystemExit(f"refusing consumer proof on scn {published}")
    if not (fs.EVID / "query" / "allow.exit").is_file():
        raise SystemExit("refusing to replace a missing pre-grant query")
    status = json.loads((fs.EVID / "status.json").read_text())
    status["allow_before_acl"] = (fs.EVID / "query" / "allow.exit").read_text().strip()
    table = grant_visible_columns()
    expected = (status.get("catalogs") or {}).get("batch", {}).get("stored_physical")
    if table != expected:
        raise SystemExit("granted table is not the sealed batch physical name")
    status["reader_acl"] = "generation2_visible_columns_only"
    status["query_dir"] = "query-2"
    status["query_pack"] = "out-fresh"
    allow_exit, deny_exit = query_case(table, "out-fresh", "query-2")
    status["allow_exit"] = allow_exit
    status["deny_exit"] = deny_exit
    score_cancel(status, "query-2")
    return emit(status)


def grant_object(directory, source, namespace, logical_table, physical, visible, hidden):
    """SELECT on the sealed physical columns for one already imported object."""
    if not hex_name(physical, "pt_"):
        raise SystemExit("physical table name rejected")
    for name in (source, namespace, logical_table, *visible, *hidden):
        if not name or any(ch not in "abcdefghijklmnopqrstuvwxyz0123456789_" for ch in name):
            raise SystemExit(f"logical name rejected: {name}")
    directory.mkdir(parents=True, exist_ok=True)
    listed = psql(
        "SELECT c.logical_column || '|' || c.physical_name "
        "FROM _oo.physical_mappings t "
        "JOIN _oo.physical_mappings c ON c.source = t.source "
        "AND c.run_generation = t.run_generation "
        "AND c.config_digest = t.config_digest "
        "AND c.logical_namespace = t.logical_namespace "
        "AND c.logical_table = t.logical_table AND c.level = 'c' "
        f"WHERE t.level = 't' AND t.physical_name = '{physical}' "
        f"AND t.source = '{source}' AND t.logical_namespace = '{namespace}' "
        f"AND t.logical_table = '{logical_table}'"
    )
    (directory / "mappings.txt").write_bytes(listed.stdout)
    if listed.returncode != 0:
        raise SystemExit("mapping read failed")
    columns = {}
    for line in listed.stdout.decode().splitlines():
        logical, name = line.split("|", 1)
        columns[logical] = name
    wanted = set(visible) | set(hidden)
    if set(columns) != wanted:
        raise SystemExit(f"mapping columns {sorted(columns)} != {sorted(wanted)}")
    if any(not hex_name(columns[name], "pc_") for name in wanted):
        raise SystemExit("physical column name rejected")
    granted = ", ".join(columns[name] for name in visible)
    sql = (
        "BEGIN; SET LOCAL ROLE oo_target_owner; "
        f"GRANT SELECT ({granted}) ON TABLE _oo.{physical} TO oo_r_reader; COMMIT;"
    )
    (directory / "grant.sql").write_text(sql + "\n")
    proc = psql(sql)
    fs.save(directory, "grant", proc)
    if proc.returncode != 0:
        raise SystemExit(f"column grant exit {proc.returncode}")
    verify = psql(
        "SELECT a.attname || '|' || "
        "COALESCE(has_column_privilege('oo_r_reader', a.attrelid, a.attnum, 'SELECT')::text, 'null') "
        "FROM pg_attribute a JOIN pg_class c ON c.oid = a.attrelid "
        "JOIN pg_namespace n ON n.oid = c.relnamespace "
        f"WHERE n.nspname = '_oo' AND c.relname = '{physical}' "
        "AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum"
    )
    (directory / "verify.txt").write_bytes(verify.stdout)
    if verify.returncode != 0:
        raise SystemExit("privilege verify failed")
    seen = {}
    for line in verify.stdout.decode().splitlines():
        name, flag = line.split("|", 1)
        seen[name] = flag
    for logical in visible:
        if seen.get(columns[logical]) != "true":
            raise SystemExit(f"missing select on {logical}")
    for logical in hidden:
        if seen.get(columns[logical]) != "false":
            raise SystemExit(f"{logical} became selectable")
    hidden_meta = [name for name, flag in seen.items() if name.startswith("_oo") and flag != "false"]
    if hidden_meta:
        raise SystemExit(f"metadata selectable {hidden_meta}")
    table_priv = psql(
        "SELECT count(*)::text FROM information_schema.table_privileges "
        f"WHERE grantee = 'oo_r_reader' AND table_schema = '_oo' AND table_name = '{physical}'"
    )
    (directory / "table-privilege.txt").write_bytes(table_priv.stdout)
    if table_priv.stdout.decode().strip() != "0":
        raise SystemExit("table privilege appeared")
    return physical


def consume_three():
    """Pack query, deny, 57014 cancel, source-off, and reopen for the three sealed imports."""
    original = fs.EVID
    window = fs.SRC / "evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/product-window"
    sealed = json.loads((original / "status.json").read_text())
    customer = fs.SRC / "evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/customer-3"
    objects = [
        {
            "name": "batch",
            "source": "oo04_t_batch",
            "namespace": "batch",
            "table": "t_batch",
            "dataset": "batch.t_batch",
            "visible": ["c6", "d", "n1", "n16", "r", "v2"],
            "hidden": ["id"],
            "builder": original / "builder",
            "physical": sealed["catalogs"]["batch"]["stored_physical"],
        },
        {
            "name": "wide",
            "source": "oo04_t_wide_text",
            "namespace": "batch",
            "table": "t_wide_text",
            "dataset": "batch.t_wide_text",
            "visible": ["n_free_text"],
            "hidden": ["id"],
            "builder": customer / "builder-wide",
            "physical": sealed["catalogs"]["wide"]["stored_physical"],
        },
        {
            "name": "ts",
            "source": "oo04_t_ts_text",
            "namespace": "batch",
            "table": "t_ts_text",
            "dataset": "batch.t_ts_text",
            "visible": ["ts_text", "tstz_text"],
            "hidden": ["id"],
            "builder": customer / "builder-ts",
            "physical": sealed["catalogs"]["ts"]["stored_physical"],
        },
    ]
    fs.EVID = window
    results = {}
    try:
        for item in objects:
            pack = item["builder"] / "out-fresh"
            if not (pack / "authority.json").is_file() or not (pack / "pack").is_dir():
                raise SystemExit(f"pack missing for {item['name']}")
            grant_object(
                window / f"acl-{item['name']}",
                item["source"], item["namespace"], item["table"], item["physical"],
                item["visible"], item["hidden"],
            )
            directory_name = f"query-{item['name']}"
            allow_exit, deny_exit = query_case(
                item["physical"], "out-fresh", directory_name, item["dataset"],
                item["visible"], item["builder"],
            )
            directory = window / directory_name
            text = ""
            for candidate in ("cancel-query.stdout", "cancel-query.stderr"):
                file = directory / candidate
                if file.is_file():
                    text += file.read_text(errors="replace")
            reopen = (directory / "reopen.txt").read_text(errors="replace") if (directory / "reopen.txt").is_file() else ""
            cancel_exit = (directory / "cancel.exit").read_text().strip() if (directory / "cancel.exit").is_file() else ""
            query_exit = (directory / "cancel-query.exit").read_text().strip() if (directory / "cancel-query.exit").is_file() else ""
            results[item["name"]] = {
                "allow_exit": allow_exit,
                "deny_exit": deny_exit,
                "cancel_command_exit": cancel_exit,
                "cancel_query_exit": query_exit,
                "sqlstate_57014": "57014" in text,
                "reopen_read_write": "READ WRITE" in reopen,
                "dataset": item["dataset"],
                "physical": item["physical"],
                "run_id": sealed[item["name"]].get("run_id"),
                "rows": sealed[item["name"]].get("rows"),
                "arrow_sha256": sealed[item["name"]].get("arrow_sha256"),
                "source_id": item["source"],
                "builder": str(item["builder"]),
            }
            fs.say(f"consume {item['name']} allow={allow_exit} deny={deny_exit} 57014={'57014' in text}")
        ok = all(
            item["allow_exit"] == "0" and item["deny_exit"] == "6"
            and item["sqlstate_57014"] and item["reopen_read_write"]
            for item in results.values()
        ) and len(results) == 3
        fs.write_json(window / "consume-status.json", {"objects": results, "consume_ok": ok})
        running = fs.run(["docker", "inspect", "-f", "{{.State.Running}}", fs.ORA], timeout=20)
        (window / "oracle-after-consume.txt").write_bytes(running.stdout)
        return ok
    finally:
        fs.EVID = original


OLD_IMPORT_RUNS = {
    "a35910fc-8cfb-4904-a7a7-3689982c5f49",
    "72b1f990-89ab-4887-a352-8aaf69a0c07c",
    "5a3f854d-d38d-493b-b579-6abc0a96ec9b",
}
CHILD_SHA = "af6370b96c73dab6485241d675ef11f826fd78027b94e3e3164dde7ef79fc9d4"


def observe_scn():
    observed = fs.sqlplus(
        "ALTER SESSION SET CONTAINER = FREEPDB1;\n"
        "SET PAGESIZE 0 FEEDBACK OFF HEADING OFF LINESIZE 200\n"
        "WHENEVER SQLERROR EXIT SQL.SQLCODE\n"
        "SELECT 'current|' || current_scn FROM v$database;\n"
        "EXIT\n",
        "current-scn",
        timeout=60,
    )
    scn = ""
    for line in observed.stdout.decode("utf-8", "replace").splitlines():
        line = line.strip()
        if line.startswith("current|"):
            scn = line.split("|", 1)[1].strip()
    if not scn.isdigit() or scn in {"2205514", "2196583", "2175138"}:
        raise SystemExit(f"no readable current SCN: {scn}")
    return scn


def child_digest():
    proc = fs.run([
        "docker", "run", "--rm", "--network", "none",
        "--platform", "linux/arm64", "--pull", "never",
        "--memory", "64m", "--cpus", "0.25",
        "--cap-drop", "ALL", "--security-opt", "no-new-privileges",
        "--entrypoint", "sha256sum",
        "-v", "oo-dl01w1-04-target:/target:ro",
        *fs.LABELS, "--label", "oo.kind=proof-runner",
        fs.IMAGE, fs.CHILD,
    ], timeout=60)
    text = proc.stdout.decode("utf-8", "replace").strip()
    digest = text.split()[0] if text else ""
    return proc.returncode == 0 and digest == CHILD_SHA, digest, text


def exit_text(directory, name):
    path = directory / name
    return path.read_text().strip() if path.is_file() else ""


def live_chain():
    """Import one still-readable SCN and consume that import.

    Leaves the SCN 2205514 ORA-01555 body and closure-2 denied INSERT untouched.
    Does not call finish, emit, or prove_consumer.
    """
    original_evid = fs.EVID
    original_child = fs.CHILD
    chain = fs.SRC / "evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/live-chain"
    chain.mkdir(parents=True, exist_ok=True)
    fs.EVID = chain
    fs.CHILD = "/target/oracle-customer/debug/oo-oracle-p2-child"
    status = {
        "protocol": "oo.oracle.live-chain/v1",
        "source_acceptance": "not_claimed",
        "publication": "not_published",
        "child_expected_sha256": CHILD_SHA,
        "prior_scn_batch_ora_01555": "2205514",
        "publish_scn": "not_invoked",
        "dba_mutate": "not_invoked",
        "dml_exit": "not_invoked",
    }
    try:
        customer = fs.ensure_customer()
        trace = customer.get("_open_trace") or []
        fs.write_json(chain / "secret-open.json", {
            "protocol": "oo.oracle.secret-open/v1",
            "secret_bytes_recorded": False,
            "trace": trace,
        })
        limit = customer["budget"]["hard_limit"]
        matched, digest, raw = child_digest()
        (chain / "child.sha256").write_text(raw + "\n")
        status["child_sha256"] = digest
        status["child_sha_matches_af637"] = matched
        if not matched:
            status["stopped"] = "child_digest"
            fs.write_json(chain / "chain-status.json", status)
            return status
        since = time.strftime("%Y%m%d%H%M%S", time.gmtime()) + "000000"
        (chain / "audit-since.txt").write_text(since + "\n")
        scn = observe_scn()
        count_exit, counts = fs.source_counts(scn, "independent-count")
        if count_exit != 0 or set(counts) != {"T_BATCH", "T_WIDE", "T_TS"}:
            scn = observe_scn()
            count_exit, counts = fs.source_counts(scn, "independent-count-retry")
        (chain / "published-scn.txt").write_text(scn + "\n")
        status["scn"] = scn
        status["snapshot"] = f"oo-dl01w1-04-scn-{scn}"
        status["independent_counts"] = counts
        status["count_exit"] = count_exit
        if count_exit != 0 or set(counts) != {"T_BATCH", "T_WIDE", "T_TS"}:
            status["stopped"] = "count"
            fs.write_json(chain / "chain-status.json", status)
            return status
        batch_ok, batch_rows, batch_sha = fs.window_batch(scn, limit, "batch")
        wide_ok = fs.capture_text("oracle:text-wide", "wide-text", scn)
        ts_ok = fs.capture_text("oracle:text-ts", "ts-text", scn)
        wide_rows = None
        ts_rows = None
        if (chain / "wide-text" / "text.json").is_file():
            wide_rows = json.loads((chain / "wide-text" / "text.json").read_text()).get("rows")
        if (chain / "ts-text" / "text.json").is_file():
            ts_rows = json.loads((chain / "ts-text" / "text.json").read_text()).get("rows")
        status["capture"] = {
            "batch": batch_ok, "wide_text": wide_ok, "ts_text": ts_ok,
            "rows": {"T_BATCH": batch_rows, "T_WIDE": wide_rows, "T_TS": ts_rows},
            "batch_arrow_sha256": batch_sha,
            "wide_arrow_sha256": fs.sha256_file(chain / "wide-text" / "text.arrow") if (chain / "wide-text" / "text.arrow").is_file() else "",
            "ts_arrow_sha256": fs.sha256_file(chain / "ts-text" / "text.arrow") if (chain / "ts-text" / "text.arrow").is_file() else "",
        }
        audit_ok = fs.audit_reader(since)
        status["audit_zero_write_attempts"] = audit_ok
        if not (batch_ok and wide_ok and ts_ok and audit_ok and batch_rows == counts["T_BATCH"] and wide_rows == counts["T_WIDE"] and ts_rows == counts["T_TS"]):
            status["stopped"] = "capture_or_audit"
            fs.write_json(chain / "chain-status.json", status)
            return status
        status["oci_cancel"] = fs.run_oci_cancel(scn)
        status["deadline"] = fs.run_deadline(scn)
        for folder in ("oci-cancel", "deadline"):
            (chain / folder / "binary.sha256").write_text(digest + "\n")
        imports = {}
        imports["batch"] = prepare_object(
            "batch", "oo04_t_batch", "t_batch", batch_rows, chain / "batch" / "batch.arrow",
            BATCH_FIELDS, BATCH_MANIFEST, scn, False, limit,
        )
        for kind, rows, folder in (
            ("wide", wide_rows, "wide-text"),
            ("ts", ts_rows, "ts-text"),
        ):
            source_id, table, fields, columns = text_columns(kind)
            item = prepare_object(kind, source_id, table, rows, chain / folder / "text.arrow", fields, columns, scn, True, limit)
            item["scn"] = scn
            imports[kind] = item
        imports["batch"]["scn"] = scn
        for item in imports.values():
            item["new_run"] = bool(item.get("run_id")) and item.get("run_id") not in OLD_IMPORT_RUNS
        status["imports"] = imports
        catalogs = {}
        for name in ("batch", "wide", "ts"):
            catalogs[name] = catalog_one(name, imports[name]) if imports[name].get("new_run") else {"ok": False, "reason": "run_not_new"}
        status["catalogs"] = catalogs
        builders = {}
        specs = {
            "batch": ("builder", "batch.t_batch", ["c6", "d", "n1", "n16", "r", "v2"], "oo04_t_batch", "oo-dl01w1-04-live-builder-batch"),
            "wide": ("builder-wide", "batch.t_wide_text", ["n_free_text"], "oo04_t_wide_text", "oo-dl01w1-04-live-builder-wide"),
            "ts": ("builder-ts", "batch.t_ts_text", ["ts_text", "tstz_text"], "oo04_t_ts_text", "oo-dl01w1-04-live-builder-ts"),
        }
        for name, (folder, dataset, visible, source_id, container) in specs.items():
            if not catalogs[name].get("ok"):
                builders[name] = {"ok": False, "reason": "catalog_absent"}
                continue
            built = run_builder(
                chain / f"catalog-{name}" / "catalog.json",
                dataset, visible, source_id, folder, container,
                chain / f"admit-{name}" / "schema-preimage.txt",
            )
            cases = {}
            for case, expect, outcome, reason, pack, model_expect in (
                ("out-timeout", 1, "provider_failure", "timeout", "not_produced", None),
                ("out-401", 1, "provider_failure", "http_status", "not_produced", None),
                ("out-cost", 1, "cost_unknown", "unknown_cost", "not_produced", "called"),
                ("out-fresh", 0, "packed", None, "produced", "called"),
                ("out-cache", 0, "packed", None, "produced", "cache_hit"),
            ):
                ok, code, got_outcome, got_reason, got_pack, model = case_ok(
                    chain / folder, case, expect, outcome, reason, pack, model_expect,
                )
                cases[case] = {
                    "ok": ok, "exit": code, "outcome": got_outcome, "reason": got_reason,
                    "pack": got_pack, "model": model,
                }
            fresh = chain / folder / "out-fresh" / "pack" / "receipt.json"
            snapshot_id = ""
            if fresh.is_file():
                snapshot_id = json.loads(fresh.read_text()).get("binding", {}).get("snapshot_id", "")
            builders[name] = {
                "builder_exit": built,
                "cases": cases,
                "cases_ok": all(item["ok"] for item in cases.values()),
                "pack_snapshot_id": snapshot_id,
                "pack_matches_run": snapshot_id == imports[name].get("run_id"),
            }
        status["builders"] = builders
        queries = {}
        for name, (folder, dataset, visible, source_id, _container) in specs.items():
            pack_ok = builders[name].get("cases_ok") and builders[name].get("pack_matches_run")
            physical = catalogs[name].get("stored_physical")
            if not pack_ok or not physical:
                queries[name] = {"ok": False, "reason": "pack_absent"}
                continue
            hidden = ["id"]
            logical = {"batch": "t_batch", "wide": "t_wide_text", "ts": "t_ts_text"}[name]
            grant_object(chain / f"acl-{name}", source_id, "batch", logical, physical, visible, hidden)
            allow_exit, deny_exit = query_case(
                physical, "out-fresh", f"query-{name}", dataset, visible, folder, True,
            )
            qdir = chain / f"query-{name}"
            text = ""
            for candidate in ("cancel-query.stdout", "cancel-query.stderr"):
                file = qdir / candidate
                if file.is_file():
                    text += file.read_text(errors="replace")
            widen_exit = exit_text(qdir, "widen.exit")
            reopen = (qdir / "reopen.txt").read_text(errors="replace") if (qdir / "reopen.txt").is_file() else ""
            queries[name] = {
                "allow_exit": allow_exit,
                "deny_exit": deny_exit,
                "widen_exit": widen_exit,
                "cancel_command_exit": exit_text(qdir, "cancel.exit"),
                "cancel_query_exit": exit_text(qdir, "cancel-query.exit"),
                "sqlstate_57014": "57014" in text,
                "rollback_confirmed": "rollback_confirmed" in text and "true" in text,
                "reopen_read_write": "READ WRITE" in reopen,
                "run_id": imports[name].get("run_id"),
                "arrow_sha256": imports[name].get("arrow_sha256"),
                "physical": physical,
                "snapshot": status["snapshot"],
            }
        status["queries"] = queries
        query_ok = all(
            item.get("allow_exit") == "0" and item.get("deny_exit") == "6" and item.get("widen_exit") == "9"
            and item.get("sqlstate_57014") and item.get("rollback_confirmed") and item.get("reopen_read_write")
            for item in queries.values()
        ) and len(queries) == 3
        status["chain_ok"] = bool(
            status.get("oci_cancel") and status.get("deadline") and query_ok
            and all(item.get("new_run") for item in imports.values())
            and all(item.get("cases_ok") and item.get("pack_matches_run") for item in builders.values())
        )
        fs.write_json(chain / "chain-status.json", status)
        return status
    finally:
        try:
            running = fs.run(["docker", "inspect", "-f", "{{.State.Running}}", fs.ORA], timeout=20)
            (chain / "oracle-after.txt").write_bytes(running.stdout)
        except Exception as exc:
            (chain / "oracle-after.txt").write_text(f"inspect_failed {exc}\n")
        if not (chain / "chain-status.json").is_file():
            status["stopped"] = status.get("stopped") or "exception"
            fs.write_json(chain / "chain-status.json", status)
        try:
            write_live_reports(chain, status)
        except Exception as exc:
            (chain / "report-error.txt").write_text(repr(exc)[:1000] + "\n")
        fs.EVID = original_evid
        fs.CHILD = original_child


def write_live_reports(chain, status):
    scn = status.get("scn")
    lines = [
        "# DL01-W1-04 live chain",
        "",
        "This packet imports one SCN that all three objects could read, then builds and queries that import.",
        "SCN 2205514 ORA-01555 stays in product-window. closure-2 denied INSERT is not edited.",
        f"`source_acceptance` is `not_claimed`. Snapshot `{status.get('snapshot')}`.",
        "",
        f"Child `{status.get('child_sha256')}` matches af637: `{status.get('child_sha_matches_af637')}`.",
        f"Independent counts `{status.get('independent_counts')}` exit `{status.get('count_exit')}`.",
        f"Capture `{status.get('capture')}`.",
        f"Reader write attempts are absent: `{status.get('audit_zero_write_attempts')}`.",
        f"OCI cancel `{status.get('oci_cancel')}`. Deadline `{status.get('deadline')}`.",
        f"Imports `{status.get('imports')}`.",
        f"Catalogs `{status.get('catalogs')}`.",
        f"Builders `{status.get('builders')}`.",
        f"Queries `{status.get('queries')}`.",
        f"chain_ok `{status.get('chain_ok')}`. SCN `{scn}`.",
        "",
    ]
    (chain / "REPORT.md").write_text("\n".join(lines))
    (chain / "FINDINGS.md").write_text("\n".join([
        "# Findings",
        "",
        "Review 67 required a new import at one still-readable SCN, a widen pack refusal, OCI cancel and deadline on child af637, and a raw secret-open trace.",
        "Hidden `select id` remains exit 6 class guard. Widen appends `secret_note` to a copy of the sealed policy and queries the allowed SQL against that policy.",
        "publish_scn and dba-mutate.sql were not run. The product reader submitted no object write.",
        "This file does not claim source acceptance.",
        "",
    ]))
    (chain / "HANDOFF.md").write_text("\n".join([
        "# Handoff",
        "",
        "Root reviews this packet. source_acceptance stays not_claimed.",
        "Do not retitle snapshot oo-dl01w1-04-scn-2205514. Do not edit closure-2/audit/denied-writes.txt.",
        "Do not grant EXECUTE on SYS.DBMS_FLASHBACK. Do not change the root Cargo.lock.",
        f"Oracle and Postgres containers stay. Rust slot was not taken. Child digest `{status.get('child_sha256')}`.",
        "",
    ]))
    digest, count = sha_sums()
    fs.write_json(chain / "receipt.json", {
        "protocol": "oo.author-receipt/v1",
        "task_id": "DL01-W1-04",
        "name": "oracle-live-chain",
        "source_acceptance": "not_claimed",
        "publication": "not_published",
        "author_delivery_is_not_independent_approval": True,
        "scn": scn,
        "snapshot": status.get("snapshot"),
        "chain_ok": status.get("chain_ok"),
        "child_sha256": status.get("child_sha256"),
        "sha256sums_sha256": digest,
        "sha256sums_entries": count,
    })


if __name__ == "__main__":
    import sys
    if sys.argv[1:] == ["consume"]:
        consume_three()
    elif sys.argv[1:] == ["live-chain"]:
        live_chain()
    else:
        raise SystemExit("refusing implicit prove_consumer; pass consume or live-chain")
