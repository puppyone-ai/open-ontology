#!/usr/bin/env python3
"""Write the source-product delivery from files the loop actually produced."""

import csv
import hashlib
import json
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[3]
TOOL = ROOT / "tools" / "postgres-preview"
EVIDENCE = ROOT / "evidence" / "ad13411e-d04e-4c91-8a20-49caf867aa23" / "DL01" / "postgres-product"
RUN = EVIDENCE / "linux" / "runner" / "out" / "run"

CONTRACT = {
    "preview.contract",
    "preview.query",
    "preview.cancel",
    "builder.catalog",
    "builder.profile",
    "builder.comments",
    "builder.seal",
    "builder.cards",
    "builder.curated",
    "builder.approve",
    "builder.generate",
    "builder.review",
    "builder.pack",
    "builder.review_input",
    "builder.classification",
    "builder.exit",
    "import.rejected",
    "import.state",
    "import.shape",
    "catalog.engine",
    "catalog.identity",
    "builder.sealed",
    "consumer.uuid",
}


ALLOWED = {
    "SELECT", "SHOW", "SET", "BEGIN", "COMMIT", "ROLLBACK", "START", "EXPLAIN", "FETCH", "CLOSE",
    "DECLARE", "WITH",
}


def statement_verb(message: str):
    text = message.strip()
    sql = None
    if text.startswith("statement: "):
        sql = text[len("statement: "):]
    elif text.startswith("execute ") and ": " in text:
        sql = text.split(": ", 1)[1]
    if sql is None:
        return None
    word = []
    for char in sql.lstrip(" \n\t("):
        if char.isalpha():
            word.append(char.upper())
        else:
            break
    return "".join(word) or "NONE"


def statement_verb_counts(path: pathlib.Path, reader: str):
    reader_bad = {}
    other_bad = {}
    if not path.is_file():
        return reader_bad, other_bad
    with path.open(newline="", encoding="utf-8", errors="replace") as handle:
        for row in csv.reader(handle):
            if len(row) < 14:
                continue
            verb = statement_verb(row[13])
            if verb is None or verb in ALLOWED:
                continue
            if row[1] == reader:
                reader_bad[verb] = reader_bad.get(verb, 0) + 1
            else:
                key = f"{row[1]} {verb}"
                other_bad[key] = other_bad.get(key, 0) + 1
    return reader_bad, other_bad


def split_statement(message: str):
    text = message.strip()
    sql = None
    prepared = None
    if text.startswith("statement: "):
        sql = text[len("statement: "):]
    elif text.startswith("execute ") and ": " in text:
        head, sql = text.split(": ", 1)
        prepared = head[len("execute "):]
    if sql is None:
        return None, None, None
    word = []
    for char in sql.lstrip(" \n\t("):
        if char.isalpha():
            word.append(char.upper())
        else:
            break
    return "".join(word) or "NONE", sql, prepared


def stream_lock_boundary(path: pathlib.Path):
    """Map reader SAVEPOINT/RELEASE rows to the next business SELECT."""
    preview = {}
    producer = {}
    pairs = []
    open_pair = None
    if not path.is_file():
        return None
    with path.open(newline="", encoding="utf-8", errors="replace") as handle:
        for idx, row in enumerate(csv.reader(handle), start=1):
            if len(row) < 23:
                continue
            verb, sql, prepared = split_statement(row[13])
            if verb is None:
                continue
            user = row[1]
            app = row[22]
            if user != "oo_dl01w1_46_reader":
                continue
            if app == "oo-dl01w1-46":
                preview[verb] = preview.get(verb, 0) + 1
            elif app == "oo-pg-source":
                producer[verb] = producer.get(verb, 0) + 1
            if app != "oo-pg-source":
                continue
            entry = {
                "csv_record": idx,
                "time": row[0],
                "user": user,
                "database": row[2],
                "backend_pid": row[3],
                "session_id": row[5],
                "session_line": row[6],
                "command_tag": row[7],
                "prepared": prepared,
                "sql": sql,
                "application_name": app,
            }
            if verb == "SAVEPOINT":
                open_pair = {"savepoint": entry, "business": None, "rollback_to": None, "release": None}
                pairs.append(open_pair)
            elif open_pair and open_pair["business"] is None and verb == "SELECT":
                open_pair["business"] = entry
            elif (
                open_pair
                and open_pair["rollback_to"] is None
                and verb == "ROLLBACK"
                and sql.startswith("ROLLBACK TO SAVEPOINT")
            ):
                open_pair["rollback_to"] = entry
            elif open_pair and open_pair["release"] is None and verb == "RELEASE":
                open_pair["release"] = entry
                open_pair = None
    return {"preview_verbs": preview, "producer_verbs": producer, "pairs": pairs}


def outside_allowlist(verbs: dict) -> dict:
    return {name: verbs[name] for name in sorted(verbs) if name not in ALLOWED}


def verb_text(verbs: dict) -> str:
    return ", ".join(f"{name} {verbs[name]}" for name in sorted(verbs)) or "none"


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def tree_sha(root: pathlib.Path, skip: set[pathlib.Path]) -> str:
    rows = []
    if root.exists():
        for path in sorted(root.rglob("*")):
            if not path.is_file() or path in skip:
                continue
            rows.append(f"{sha256_file(path)}  {path.relative_to(root).as_posix()}")
    return hashlib.sha256("\n".join(rows).encode()).hexdigest()


def read_json(path: pathlib.Path):
    if not path.is_file():
        return None
    text = path.read_text()
    if not text.strip():
        return None
    return json.loads(text)


def capacity_fact(label: str, doc) -> str:
    if not doc:
        return f"{label} absent"
    return (
        f"{label} ok {doc.get('ok')} need_cpus {doc.get('need_cpus')} "
        f"reserved_cpus {doc.get('reserved_cpus')} rust_builders {doc.get('rust_builders')} "
        f"own_rust_builders {doc.get('own_rust_builders')} host_ncpu {doc.get('host_ncpu')} "
        f"docker_started {doc.get('docker_started')} "
        f"memory_free_percentage {doc.get('memory_free_percentage')} "
        f"headroom_bytes {doc.get('headroom_bytes')}"
    )


def container_state(name: str) -> str:
    try:
        return subprocess.check_output(
            ["docker", "inspect", "-f", "{{.State.Status}}", name],
            text=True,
            stderr=subprocess.DEVNULL,
        ).strip()
    except (subprocess.CalledProcessError, FileNotFoundError):
        return "absent"


def stage_text(name: str) -> str:
    path = RUN / "stages" / name
    if not path.is_file():
        return ""
    return path.read_text(errors="replace").strip()


def attach_gap(attach: dict) -> str:
    aligned = attach.get("bind_split") is False and attach.get("project") == attach.get("source_id")
    if aligned:
        relation = "Authority project matches import source_id. The consumer project check is unchanged."
    else:
        relation = "Authority project differs from import source_id. The consumer returns request before a session."
    return (
        "postgres-preview consume executed operator-preview "
        f"{attach.get('consumer_commit') or 'absent'} "
        f"binary {attach.get('binary_sha256') or 'absent'}. "
        f"Sealed authority project is {attach.get('project') or 'absent'} and import source_id is {attach.get('source_id') or 'absent'}. "
        f"bind_split is {attach.get('bind_split')}. "
        f"Raw exits: describe {attach.get('describe_exit')}, query {attach.get('query_exit')}, "
        f"hidden {attach.get('hidden_exit')}, widen {attach.get('widen_exit')}, "
        f"cancel {attach.get('cancel_exit')}, reopen {attach.get('reopen_exit')}. "
        f"source_open is {attach.get('source_open')}. oracle_literals is {attach.get('oracle_literals')}. "
        f"inflight_57014 is {attach.get('inflight_57014')}. rollback_confirmed is {attach.get('rollback_confirmed')}. "
        f"install_exit is {attach.get('install_exit')}. install_effect is {attach.get('install_effect') or 'absent'}. "
        f"provision_exit is {attach.get('provision_exit')}. provision_effect is {attach.get('provision_effect') or 'absent'}. "
        f"model_called is {attach.get('model_called')}. source_captured is {attach.get('source_captured')}. "
        f"Reason: {attach.get('reason') or 'absent'}. "
        f"{relation} "
        "This record is the author attach result. It is not independent acceptance."
    )


def load_table_consume() -> dict:
    """Same-capture three-table consume. The hubs attach record is not this check."""
    result = read_json(RUN / "consumer-all" / "result.json") or {}
    oracle = read_json(RUN / "typed-table-oracle-result.json") or {}
    authorities = read_json(RUN / "dataset-authorities.json") or {}
    queries = result.get("queries") if isinstance(result.get("queries"), list) else []
    expected = [
        "public.hubs",
        "public.hubs",
        "public.shipments",
        "public.shipments",
        "public.tracking_events",
        "public.tracking_events",
    ]
    datasets = [item.get("dataset") for item in queries if isinstance(item, dict)]
    exits = [item.get("exit") for item in queries if isinstance(item, dict)]
    retry = read_json(RUN / "consumer-all" / "acl-set-retry" / "install-column-acl.stdout") or {}
    stderr_path = RUN / "consumer-all" / "acl-set" / "install-column-acl.stderr"
    first_err = stderr_path.read_text(errors="replace").strip() if stderr_path.is_file() else ""
    role_path = RUN / "consumer-all" / "acl-set" / "partial-role.txt"
    dropped = role_path.read_text(errors="replace").strip() if role_path.is_file() else ""
    provision = read_json(RUN / "consumer-all" / "acl-set" / "provision-mapping-reader.stdout") or {}
    known = {
        "public.hubs": ("14f642744cf1871fe5edae3eb5f72507b40429b112553b4c62bd94dbd631576f", "cache_hit", "hit"),
        "public.shipments": ("f06919765b51a975270f91d969cb56728910723fef440159b911f703a0e16511", "called", "miss"),
        "public.tracking_events": ("cd1a0b0f32fc205abc9af8b0276c12156276cc25031a7a8131198f8a3b936404", "called", "miss"),
    }
    seen = []
    identity = None
    config = None
    same_identity = True
    same_config = True
    packs_ok = True
    pack_rows = []
    for item in authorities.get("datasets") or []:
        if not isinstance(item, dict):
            packs_ok = False
            continue
        doc = item.get("result") or {}
        name = item.get("dataset")
        seen.append(name)
        if identity is None:
            identity = doc.get("identity_hash")
        elif doc.get("identity_hash") != identity:
            same_identity = False
        if config is None:
            config = doc.get("config_digest")
        elif doc.get("config_digest") != config:
            same_config = False
        expect = known.get(name)
        if expect != (doc.get("pack_digest"), doc.get("model"), doc.get("cache")):
            packs_ok = False
        usage = doc.get("usage") or {}
        pack_rows.append(
            f"{name} pack {doc.get('pack_digest') or 'absent'} model {doc.get('model') or 'absent'} "
            f"cache {doc.get('cache') or 'absent'} tokens {usage.get('total_tokens')}"
        )
    mismatches = oracle.get("mismatches")
    tables = retry.get("tables") if isinstance(retry.get("tables"), list) else []
    ok = (
        result.get("complete") is True
        and result.get("source_open") is False
        and datasets == expected
        and exits == [0, 0, 0, 0, 0, 0]
        and oracle.get("queries") == 6
        and oracle.get("cells") == 1600
        and mismatches == []
        and authorities.get("complete") is True
        and seen == ["public.hubs", "public.shipments", "public.tracking_events"]
        and same_identity
        and same_config
        and identity == "4f82a0c529b7596da0e86ed98c72978f4ff700fb3c7897f1f98add2440969f2e"
        and config == "df690a6f697d3f78d95d3bf878df7254e16665a15e0e8407bb3dc64f64368306"
        and packs_ok
        and retry.get("effect") == "installed"
        and retry.get("role") == "oo_r_reader"
        and len(tables) == 3
        and first_err == "install-column-acl: policy mismatch"
        and dropped == "dropped"
        and provision.get("effect") == "replay"
        and provision.get("login") == "oo_dl01w1_46_target"
    )
    return {
        "ok": ok,
        "cells": oracle.get("cells"),
        "mismatch_count": 0 if mismatches == [] else None if mismatches is None else len(mismatches),
        "identity_hash": identity,
        "config_digest": config,
        "install_effect": retry.get("effect"),
        "partial_role": dropped or "absent",
        "first_stderr": first_err or "absent",
        "provision_effect": provision.get("effect"),
        "source_open": result.get("source_open"),
        "complete": result.get("complete"),
        "oracle_verified_flag": result.get("oracle_verified"),
        "pack_rows": pack_rows,
        "query_exits": exits,
    }


CUSTOMER = EVIDENCE / "linux" / "runner" / "out" / "customer-east"
LOGISTICS_IDENTITY = "4f82a0c529b7596da0e86ed98c72978f4ff700fb3c7897f1f98add2440969f2e"
LOGISTICS_CONFIG = "df690a6f697d3f78d95d3bf878df7254e16665a15e0e8407bb3dc64f64368306"
LOGISTICS_RUN = "94dd1704-f5e5-46c0-b7a8-aef390aa964c"
REJECTED_41_RUN = "55b58814-64e5-4b3c-8006-c17a5b26d8b5"
REJECTED_41_PHYSICAL = "pt_b3e5b7311822da5eaad315f84a2615671a2c898a18a361f3bca299cffe27"
WRITE_VERBS = {"INSERT", "UPDATE", "DELETE", "COPY", "CREATE", "DROP", "GRANT", "TRUNCATE", "ALTER", "REVOKE", "COMMENT"}
CONSUMER_SHA = "f7fdba26e8ec13bfd5a423f741f80c9e238abd74b39f8ea96e592f1b34c76a78"
PREVIEW_SHA = "68f53da81c22bcd8f5b729b67e9c3ffa9e68187669a201982dd57b13d8246a28"


def exit_text(path: pathlib.Path) -> str:
    return path.read_text(errors="replace").strip() if path.is_file() else "absent"


def hex64(value) -> bool:
    return isinstance(value, str) and len(value) == 64 and all(char in "0123456789abcdef" for char in value)


def import_bound(doc: dict, run_id: str, identity: str, generation, config_digest: str, role: str, policy_id: str, knowledge_id: str) -> bool:
    imp = doc.get("import") if isinstance(doc.get("import"), dict) else {}
    know = doc.get("knowledge") if isinstance(doc.get("knowledge"), dict) else {}
    pol = doc.get("policy") if isinstance(doc.get("policy"), dict) else {}
    columns = pol.get("columns") if isinstance(pol.get("columns"), list) else []
    return (
        imp.get("snapshot_id") == run_id
        and imp.get("import_hash") == identity
        and imp.get("import_generation") == generation
        and imp.get("target_config_sha256") == config_digest
        and pol.get("role") == role
        and pol.get("policy_pack_id") == policy_id
        and know.get("id") == knowledge_id
        and "secret_note" not in columns
    )


def replay_ok(doc: dict, role: str, grants: int, dataset: str) -> bool:
    return (
        doc.get("protocol") == "oo.postgres-preview.role-replay/v1"
        and doc.get("role") == role
        and doc.get("dataset") == dataset
        and doc.get("hidden_exit") == 6
        and doc.get("object_exit") == 6
        and doc.get("widen_exit") == 9
        and doc.get("cancel_exit") == 0
        and doc.get("inflight_57014") == "user_cancel"
        and doc.get("rollback_confirmed") is True
        and doc.get("reopen_exit") == 0
        and doc.get("install_called") is False
        and doc.get("source_open") is False
        and doc.get("role_exists_before") is True
        and doc.get("role_exists_after") is True
        and doc.get("role_grants_before") == grants
        and doc.get("role_grants_after") == grants
        and doc.get("binary_sha256") == CONSUMER_SHA
    )


def customer_replay_raws(replay: dict, run_id, identity, generation, config_digest, pack, physical) -> bool:
    """Each probe raw must carry the current seal. Exit codes alone do not."""
    role = "oo_r_east"
    policy_id = "pol-customer_east"
    knowledge_id = "know-customer_east"
    base = CUSTOMER / "consumer-role-replay"
    hidden = read_json(base / "hidden.stdout") or {}
    object_doc = read_json(base / "object.stdout") or {}
    widen = read_json(base / "widen.stdout") or {}
    reopen = read_json(base / "reopen.stdout") or {}
    cancel = read_json(base / "cancel-query.stdout") or {}
    authority = read_json(base / "authority-hidden.json") or {}
    failed = EVIDENCE / "linux" / "failed-attempt-customer-50-import-rejected" / "consumer-role-replay" / "hidden.stdout"
    current = base / "hidden.stdout"
    same_bytes = failed.is_file() and current.is_file() and sha256_file(failed) == sha256_file(current)
    retained = replay.get("retained_physical_tables")
    auth_imp = authority.get("import") if isinstance(authority.get("import"), dict) else {}
    auth_know = authority.get("knowledge") if isinstance(authority.get("knowledge"), dict) else {}

    def one(doc: dict, kind: str) -> bool:
        ok = import_bound(doc, run_id, identity, generation, config_digest, role, policy_id, knowledge_id)
        know = doc.get("knowledge") if isinstance(doc.get("knowledge"), dict) else {}
        if kind == "guard":
            return ok and doc.get("class") == "guard" and doc.get("outcome") == "refused" and know.get("pack_digest") == pack
        if kind == "widen":
            return ok and doc.get("class") == "pack_mismatch" and doc.get("outcome") == "refused" and "pack_digest" not in know
        if kind == "reopen":
            return ok and doc.get("outcome") == "succeeded" and know.get("pack_digest") == pack
        if kind == "cancel":
            return (
                ok
                and doc.get("outcome") == "canceled"
                and doc.get("sqlstate") == "57014"
                and doc.get("rollback_confirmed") is True
                and know.get("pack_digest") == pack
            )
        return False

    return (
        replay.get("snapshot_id") == run_id
        and replay.get("import_hash") == identity
        and replay.get("import_generation") == generation
        and replay.get("pack_digest") == pack
        and replay.get("knowledge_id") == knowledge_id
        and replay.get("policy_pack_id") == policy_id
        and replay.get("config_digest") == config_digest
        and replay.get("physical_table") == physical
        and replay.get("max_generation") == generation
        and replay.get("retained_role_grants") == 0
        and replay.get("hidden_class") == "guard"
        and replay.get("hidden_column") == "secret_note"
        and replay.get("hidden_column_in_scope") is False
        and replay.get("object_class") == "guard"
        and replay.get("widen_class") == "pack_mismatch"
        and isinstance(retained, list)
        and REJECTED_41_PHYSICAL in retained
        and physical not in retained
        and physical != REJECTED_41_PHYSICAL
        and run_id != REJECTED_41_RUN
        and not same_bytes
        and one(hidden, "guard")
        and one(object_doc, "guard")
        and one(widen, "widen")
        and one(reopen, "reopen")
        and one(cancel, "cancel")
        and auth_imp.get("snapshot_id") == run_id
        and auth_imp.get("import_hash") == identity
        and auth_imp.get("import_generation") == generation
        and auth_know.get("id") == knowledge_id
        and authority.get("policy_pack_id") == policy_id
        and authority.get("role") == role
        and exit_text(base / "hidden.stderr") == "operator-preview: guard"
        and exit_text(base / "object.stderr") == "operator-preview: guard"
        and exit_text(base / "widen.stderr") == "operator-preview: pack"
    )


def uuid_text(value) -> bool:
    if not isinstance(value, str) or len(value) != 36:
        return False
    parts = value.split("-")
    if [len(part) for part in parts] != [8, 4, 4, 4, 12]:
        return False
    return all(char in "0123456789abcdef-" for char in value)


def customer_window(path: pathlib.Path) -> dict:
    """Recount the customer csvlog window. The logistics source.csv is not an input."""
    records = 0
    dba_create = 0
    dba_copy = 0
    reader_statements = 0
    reader_selects = 0
    reader_writes = 0
    marker = False
    names_customer = False
    if path.is_file():
        with path.open(newline="", encoding="utf-8", errors="replace") as handle:
            for row in csv.reader(handle):
                if len(row) < 14:
                    continue
                records += 1
                message = row[13]
                if "oo-dl01w1-46-customer-50-window" in message:
                    marker = True
                if "customer_east" in message.lower():
                    names_customer = True
                verb, sql, _prepared = split_statement(message)
                if verb is None or sql is None:
                    continue
                user = row[1]
                named = "customer_east" in sql.lower()
                if user == "oo_dl01w1_46_dba":
                    if verb == "CREATE" and named:
                        dba_create += 1
                    if verb == "COPY" and "customer_east.delivery_notes" in sql.lower():
                        dba_copy += 1
                elif user == "oo_dl01w1_46_reader":
                    reader_statements += 1
                    if verb in WRITE_VERBS:
                        reader_writes += 1
                    if verb == "SELECT" and "customer_east.delivery_notes" in sql.lower():
                        reader_selects += 1
    ok = (
        records > 2
        and marker
        and names_customer
        and dba_create > 0
        and dba_copy > 0
        and reader_statements > 0
        and reader_selects > 0
        and reader_writes == 0
    )
    return {
        "ok": ok,
        "records": records,
        "dba_create": dba_create,
        "dba_copy": dba_copy,
        "reader_statements": reader_statements,
        "query_count": reader_selects,
        "write_attempts": reader_writes,
    }


def load_customer() -> dict:
    """Caller-named customer_east.delivery_notes chain. It does not replace the 100k capture."""
    load = read_json(CUSTOMER / "customer-load.json") or {}
    refusal = read_json(CUSTOMER / "unapproved-refusal.json") or {}
    state = read_json(CUSTOMER / "state.json") or {}
    stops = read_json(CUSTOMER / "chain-stops.json") or {}
    imported = read_json(CUSTOMER / "stages" / "import-pg.stdout") or {}
    sealed = read_json(CUSTOMER / "stages" / "sealed" / "result.json") or {}
    hit = read_json(CUSTOMER / "stages" / "sealed-table-0" / "result.json") or {}
    denied = read_json(CUSTOMER / "stages" / "sealed-401" / "result.json") or {}
    timeout = read_json(CUSTOMER / "stages" / "sealed-timeout" / "result.json") or {}
    unknown = read_json(CUSTOMER / "stages" / "sealed-unknown" / "result.json") or {}
    policy = read_json(CUSTOMER / "stages" / "sealed" / "policy.json") or {}
    visibility = read_json(CUSTOMER / "stages" / "visibility.json") or {}
    consume = read_json(CUSTOMER / "consumer-all" / "result.json") or {}
    install = read_json(CUSTOMER / "consumer-all" / "acl-set" / "install-column-acl.stdout") or {}
    retry_install = read_json(CUSTOMER / "consumer-all" / "acl-set-retry" / "install-column-acl.stdout") or {}
    chosen = retry_install if retry_install else install
    provision = read_json(CUSTOMER / "consumer-all" / "acl-set" / "provision-mapping-reader.stdout") or {}
    acl_dir = CUSTOMER / "consumer-all" / "acl-set"
    first_err = (acl_dir / "install-column-acl.stderr").read_text(errors="replace") if (acl_dir / "install-column-acl.stderr").is_file() else ""
    partial = (acl_dir / "partial-role.txt").read_text(errors="replace").strip() if (acl_dir / "partial-role.txt").is_file() else ""
    settings = read_json(CUSTOMER / "stages" / "settings.json") or {}
    catalog_doc = read_json(CUSTOMER / "stages" / "catalog-import.stdout") or {}
    source_oracle = read_json(CUSTOMER / "source-table-oracle.json") or {}
    source_count = read_json(CUSTOMER / "source-count.json") or {}
    pack_json = read_json(CUSTOMER / "stages" / "sealed" / "role-pack" / "pack.json") or {}
    window = customer_window(CUSTOMER / "source-window.csv")
    failed_load = read_json(EVIDENCE / "linux" / "failed-attempt-customer-41" / "customer-load.json") or {}
    replay = read_json(CUSTOMER / "consumer-role-replay" / "result.json") or {}
    oracle = read_json(CUSTOMER / "typed-table-oracle-result.json") or {}
    logistics_replay = read_json(RUN / "consumer-role-replay" / "result.json") or {}
    logistics_import = read_json(RUN / "stages" / "import-pg.stdout") or {}
    config = read_json(EVIDENCE / "linux" / "runner" / "work" / "config-customer-east.json") or {}
    catalog_path = CUSTOMER / "catalog-after-consume.txt"
    catalog = catalog_path.read_text(errors="replace") if catalog_path.is_file() else ""
    refused_status = exit_text(CUSTOMER / "second-database-refused" / "init-pg.status")
    refused_err = exit_text(CUSTOMER / "second-database-refused" / "init-pg.stderr")
    producer_path = CUSTOMER / "stages" / "producer-customer_east--delivery_notes.stdout"
    producer = producer_path.read_text(errors="replace") if producer_path.is_file() else ""
    arrow = CUSTOMER / "producer" / "customer_east--delivery_notes" / "delivery_notes.arrow"
    arrow_sha = sha256_file(arrow) if arrow.is_file() else ""
    tables = imported.get("tables") if isinstance(imported.get("tables"), list) else []
    table = tables[0] if len(tables) == 1 and isinstance(tables[0], dict) else {}
    queries = consume.get("queries") if isinstance(consume.get("queries"), list) else []
    budget = config.get("budget") if isinstance(config.get("budget"), dict) else {}
    install_tables = chosen.get("tables") if isinstance(chosen.get("tables"), list) else []
    physical = install_tables[0].get("physical_table") if len(install_tables) == 1 and isinstance(install_tables[0], dict) else None
    policy_table = policy.get("table") if isinstance(policy.get("table"), dict) else {}
    policy_columns = [item.get("name") for item in policy_table.get("columns") or [] if isinstance(item, dict)]
    config_columns = [item.get("column") for item in config.get("approved_fields") or [] if isinstance(item, dict)]
    expected_columns = ["note_id", "amount", "receipt", "note_token", "signed_at", "memo"]
    pack = sealed.get("pack_digest")
    identity = sealed.get("identity_hash")
    config_digest = sealed.get("config_digest")
    measured = load.get("rows")
    chain = (sealed, hit, denied, timeout, unknown)
    run_id = imported.get("run_id")
    generation = catalog_doc.get("source_generation")
    generation_text = str(generation) if isinstance(generation, int) else generation if isinstance(generation, str) else ""
    catalog_digest = catalog_doc.get("config_digest")
    ontology = settings.get("ontology") if isinstance(settings.get("ontology"), dict) else {}
    knowledge = settings.get("knowledge") if isinstance(settings.get("knowledge"), dict) else {}
    policy_identity = settings.get("policy") if isinstance(settings.get("policy"), dict) else {}
    registry = config.get("registry") if isinstance(config.get("registry"), dict) else {}
    settings_text = json.dumps(settings, sort_keys=True)
    vis_knowledge = visibility.get("knowledge") if isinstance(visibility.get("knowledge"), dict) else {}
    vis_policy = visibility.get("policy") if isinstance(visibility.get("policy"), dict) else {}
    readme = ""
    for item in pack_json.get("files") or []:
        if isinstance(item, dict) and item.get("path") == "README.md":
            readme = item.get("text") or ""
    window_ids = []
    for query in source_oracle.get("queries") or []:
        if not isinstance(query, dict):
            continue
        for row in query.get("rows") or []:
            if isinstance(row, list) and row:
                window_ids.append(row[0])
    count_tables = source_count.get("tables") if isinstance(source_count.get("tables"), list) else []
    count_row = count_tables[0] if len(count_tables) == 1 and isinstance(count_tables[0], dict) else {}
    gaps = []

    def need(flag: bool, label: str) -> None:
        if not flag:
            gaps.append(label)

    need(load.get("protocol") == "oo.postgres-preview.customer-load/v1", "load.protocol")
    need(measured == 50 and table.get("rows") == 50 and load.get("distinct_note_ids") == 50, "measured_50")
    need(count_row.get("rows") == "50" and count_row.get("distinct_ids") == "50" and count_row.get("id_column") == "note_id", "source_count")
    need(len(window_ids) == 50 and len(set(window_ids)) == 50, "window_distinct")
    need(load.get("hubs") == 2000 and load.get("shipments") == 8000 and load.get("tracking_events") == 90000, "logistics_counts")
    need(load.get("payroll_select") is False and load.get("note_insert") is False and "product_write_attempts" not in load, "dba_window")
    need(failed_load.get("rows") == 41, "preserved_41")
    need(window["ok"], "source_window")
    need(budget.get("max_rows") != measured, "budget_not_rowcount")
    need(config.get("source_id") == "oo_dl01w1_46" and config.get("secret_ref") == "secret://oo-dl01w1-46/reader", "customer_input")
    need(config.get("pack_role") == "oo_r_east" and config.get("out_dir") == "/work-out/customer-east", "pack_role")
    need((config.get("target") or {}).get("database") == "dst", "target_database")
    need(config.get("approved_objects") == [{"schema": "customer_east", "table": "delivery_notes"}], "approved_objects")
    need(policy_columns == config_columns == expected_columns, "approved_fields")
    need(exit_text(CUSTOMER / "customer-load.exit") == "0", "load.exit")
    need(exit_text(CUSTOMER / "unapproved-refusal.exit") == "6", "refusal.exit")
    need(refusal.get("connected") is False and refusal.get("read_attempted") is False, "refusal.before_read")
    need(refusal.get("objects") == [{"schema": "customer_east", "table": "payroll"}], "refusal.object")
    need(refusal.get("fields") == [{"column": "internal_note", "schema": "customer_east", "table": "delivery_notes"}], "refusal.field")
    need(exit_text(CUSTOMER / "customer-run.exit") == "0", "customer-run.exit")
    need(exit_text(CUSTOMER / "consume-all.exit") == "0", "consume-all.exit")
    need(exit_text(CUSTOMER / "consumer-role-replay.exit") == "0", "customer-replay.exit")
    need(state.get("phase") == "closed" and state.get("partial") is False, "state.closed")
    need(stops.get("catalog-import") == "ok" and stops.get("grant") == "ok" and stops.get("import-pg") == "ok", "chain.import")
    need(stops.get("init-pg") == "ok" and stops.get("install-sealed") == "ok" and stops.get("owner-migration") == "ok", "chain.seal")
    need(stops.get("pack_role") == "oo_r_east" and stops.get("configured_role") == "oo_dl01w1_46_reader", "chain.role")
    need(imported.get("state") == "awaiting_publish" and uuid_text(run_id) and run_id not in {LOGISTICS_RUN, REJECTED_41_RUN} and imported.get("table_count") == 1, "import.run")
    need(table.get("schema") == "customer_east" and table.get("name") == "delivery_notes", "import.table")
    need(table.get("physical_name") == physical, "physical_table")
    need(sealed.get("cache") == "miss" and sealed.get("model") == "called" and sealed.get("pack") == "produced" and sealed.get("outcome") == "packed", "builder.fresh")
    need(hex64(pack) and hit.get("pack_digest") == pack, "pack_digest")
    need(hit.get("cache") == "hit" and hit.get("model") == "cache_hit" and hit.get("pack") == "produced" and hit.get("outcome") == "packed", "builder.cache")
    need(all(hex64(doc.get("identity_hash")) and doc.get("identity_hash") == identity and doc.get("config_digest") == config_digest for doc in chain), "same_chain")
    need(identity != LOGISTICS_IDENTITY and config_digest != LOGISTICS_CONFIG, "distinct_from_logistics")
    need(denied.get("outcome") == "provider_failure" and denied.get("http_status") == 401 and denied.get("reason") == "http_status" and denied.get("pack") == "not_produced" and denied.get("model") == "called", "builder.401")
    need(timeout.get("outcome") == "provider_failure" and timeout.get("reason") == "timeout" and timeout.get("http_status") == 0 and timeout.get("pack") == "not_produced" and timeout.get("model") == "called", "builder.timeout")
    need(unknown.get("outcome") == "cost_unknown" and unknown.get("reason") == "unknown_cost" and unknown.get("pack") == "not_produced", "builder.cost_unknown")
    need(exit_text(CUSTOMER / "stages" / "sealed.status") == "0" and exit_text(CUSTOMER / "stages" / "sealed-table-0.status") == "0", "seal.status")
    need(exit_text(CUSTOMER / "stages" / "sealed-401.status") == "1" and exit_text(CUSTOMER / "stages" / "sealed-timeout.status") == "1" and exit_text(CUSTOMER / "stages" / "sealed-unknown.status") == "1", "negative.status")
    need(policy.get("role") == "east" and policy.get("login") == "oo_dl01w1_46_target", "policy.role")
    need(policy_table.get("namespace") == "customer_east" and policy_table.get("name") == "delivery_notes", "policy.table")
    need(visibility.get("role") == "oo_r_east", "visibility.role")
    need(hex64(catalog_digest) and settings.get("catalog_version") == catalog_digest, "settings.catalog_version")
    need(knowledge.get("id") == "know-customer_east" and knowledge.get("version") == generation_text, "settings.knowledge")
    need(policy_identity.get("id") == "pol-customer_east" and policy_identity.get("version") == generation_text, "settings.policy")
    need(ontology.get("name") == "customer_east" and settings.get("ontology_version") == "ont-customer_east", "settings.ontology")
    need(ontology.get("include_schemas") == ["customer_east"], "settings.schemas")
    need("logistics" not in settings_text and "pending" not in settings_text, "settings.generic")
    need(visibility.get("catalog_version") == catalog_digest and vis_knowledge.get("id") == "know-customer_east" and vis_policy.get("id") == "pol-customer_east" and visibility.get("ontology_version") == "ont-customer_east", "visibility.identity")
    need(registry.get("ontology_name") == "customer_east" and registry.get("knowledge_id") == "know-customer_east" and registry.get("policy_id") == "pol-customer_east" and registry.get("ontology_version") == "ont-customer_east" and registry.get("knowledge_version") == generation_text and registry.get("policy_version") == generation_text, "caller.registry")
    need(config.get("source_generation") == generation and imported.get("source_generation") == generation, "caller.generation")
    need(isinstance(readme, str) and readme.startswith("# customer_east\n") and "ont-customer_east" in readme and "# logistics" not in readme, "readme")
    need(consume.get("complete") is True and consume.get("source_open") is False and consume.get("oracle_verified") is False, "consume.result")
    need([item.get("dataset") for item in queries if isinstance(item, dict)] == ["customer_east.delivery_notes", "customer_east.delivery_notes"], "consume.datasets")
    need([item.get("exit") for item in queries if isinstance(item, dict)] == [0, 0], "consume.exits")
    need(chosen.get("effect") == "installed" and chosen.get("role") == "oo_r_east", "install.role")
    need((not (CUSTOMER / "consumer-all" / "acl-set-retry").exists()) or ("mismatch" in first_err and partial == "dropped"), "east_retry")
    need(provision.get("effect") in {"replay", "installed"} and provision.get("login") == "oo_dl01w1_46_target" and provision.get("role") == "oo_r_east", "provision")
    need(replay_ok(replay, "oo_r_east", 6, "customer_east.delivery_notes"), "customer.replay")
    need(customer_replay_raws(replay, run_id, identity, generation, config_digest, pack, physical), "customer.replay.raw")
    need(f"{REJECTED_41_RUN}|awaiting_publish" in catalog, "retained_41_run")
    need(oracle.get("queries") == 2 and oracle.get("cells") == 300 and oracle.get("mismatches") == [], "customer.oracle")
    need(replay_ok(logistics_replay, "oo_r_reader", 32, "public.hubs"), "logistics.replay")
    need(logistics_import.get("run_id") == LOGISTICS_RUN and logistics_import.get("state") == "awaiting_publish" and logistics_import.get("table_count") == 3, "logistics.import")
    need(refused_status == "6" and refused_err == "init-pg: target catalog refused", "second_database_refused")
    need(exit_text(CUSTOMER / "stages" / "init-pg.status") == "0" and exit_text(CUSTOMER / "stages" / "producer-customer_east--delivery_notes.status") == "0", "dst_init_and_producer")
    need(bool(measured) and f"rows={measured}" in producer and hex64(arrow_sha) and f"arrow_sha256={arrow_sha}" in producer, "arrow")
    need("oo_dl01w1_46_target|oo_pipeline_runtime|f|f" in catalog, "inherit_false")
    need("oo_r_east|6" in catalog and "oo_r_reader|32" in catalog, "live_grants")
    need(isinstance(run_id, str) and f"{run_id}|awaiting_publish" in catalog and f"{LOGISTICS_RUN}|awaiting_publish" in catalog, "live_runs")
    summary = (
        "customer_east.delivery_notes is the caller-named object on source oo_dl01w1_46. "
        f"DBA load rows {load.get('rows')} distinct_note_ids {load.get('distinct_note_ids')} "
        f"hubs {load.get('hubs')} shipments {load.get('shipments')} "
        f"tracking_events {load.get('tracking_events')} payroll_select {load.get('payroll_select')} "
        f"note_insert {load.get('note_insert')}. "
        f"Source count rows {count_row.get('rows')} distinct_ids {count_row.get('distinct_ids')}. "
        f"LIMIT windows cover {len(set(window_ids))} distinct note_id values from {len(window_ids)} cells in the id column. "
        f"Customer csv window records {window['records']} dba_create {window['dba_create']} dba_copy {window['dba_copy']} "
        f"reader_statements {window['reader_statements']} query_count {window['query_count']} "
        f"write_attempts {window['write_attempts']}. "
        f"The rejected 41-row load remains at linux/failed-attempt-customer-41 rows {failed_load.get('rows')}. "
        f"Settings ontology {ontology.get('name')} knowledge {knowledge.get('id')} policy {policy_identity.get('id')} "
        f"ontology_version {settings.get('ontology_version')} catalog_version {settings.get('catalog_version')}. "
        f"README starts {readme.splitlines()[0] if readme else 'absent'}. "
        f"Budget max_rows is {budget.get('max_rows')}. "
        "refuse-unapproved exited 6 with connected false and read_attempted false for customer_east.payroll and delivery_notes.internal_note. "
        f"customer-run exit {exit_text(CUSTOMER / 'customer-run.exit')}. "
        f"import-pg run_id {imported.get('run_id') or 'absent'} state {imported.get('state') or 'absent'} "
        f"schema {table.get('schema') or 'absent'} table {table.get('name') or 'absent'} rows {table.get('rows')}. "
        f"Logistics run {logistics_import.get('run_id') or 'absent'} stays {logistics_import.get('state') or 'absent'}. "
        f"init-pg on dst status {exit_text(CUSTOMER / 'stages' / 'init-pg.status')}. "
        f"The preserved second-database refusal status is {refused_status} stderr {refused_err}. That refusal is a failed attempt, and the dst seal is the customer chain. "
        f"Builder sealed cache {sealed.get('cache') or 'absent'} model {sealed.get('model') or 'absent'} "
        f"pack {sealed.get('pack') or 'absent'} outcome {sealed.get('outcome') or 'absent'} "
        f"pack_digest {pack or 'absent'}. "
        f"sealed-table-0 cache {hit.get('cache') or 'absent'} model {hit.get('model') or 'absent'} "
        f"pack_digest {hit.get('pack_digest') or 'absent'}. "
        f"identity_hash {identity or 'absent'} config_digest {config_digest or 'absent'}. "
        f"401 status {exit_text(CUSTOMER / 'stages' / 'sealed-401.status')} http_status {denied.get('http_status')} "
        f"reason {denied.get('reason') or 'absent'} pack {denied.get('pack') or 'absent'}. "
        f"timeout status {exit_text(CUSTOMER / 'stages' / 'sealed-timeout.status')} reason {timeout.get('reason') or 'absent'} "
        f"http_status {timeout.get('http_status')} pack {timeout.get('pack') or 'absent'}. "
        f"cost_unknown status {exit_text(CUSTOMER / 'stages' / 'sealed-unknown.status')} outcome {unknown.get('outcome') or 'absent'} "
        f"reason {unknown.get('reason') or 'absent'} pack {unknown.get('pack') or 'absent'}. "
        f"Policy role {policy.get('role') or 'absent'} login {policy.get('login') or 'absent'} visibility {visibility.get('role') or 'absent'}. "
        f"install effect {chosen.get('effect') or 'absent'} role {chosen.get('role') or 'absent'} "
        f"physical_table {physical or 'absent'}. acl-set-retry exists {(CUSTOMER / 'consumer-all' / 'acl-set-retry').exists()}. "
        f"provision effect {provision.get('effect') or 'absent'} role {provision.get('role') or 'absent'}. "
        f"Customer typed queries {oracle.get('queries')} cells {oracle.get('cells')} mismatches {oracle.get('mismatches')}. "
        f"consume-all oracle_verified is {consume.get('oracle_verified')}. "
        f"Customer replay role {replay.get('role') or 'absent'} snapshot {replay.get('snapshot_id') or 'absent'} "
        f"pack {replay.get('pack_digest') or 'absent'} knowledge {replay.get('knowledge_id') or 'absent'} "
        f"physical {replay.get('physical_table') or 'absent'} max_generation {replay.get('max_generation')} "
        f"retained {replay.get('retained_physical_tables')} retained_role_grants {replay.get('retained_role_grants')} "
        f"hidden {replay.get('hidden_exit')} class {replay.get('hidden_class') or 'absent'} "
        f"hidden_column {replay.get('hidden_column') or 'absent'} in_scope {replay.get('hidden_column_in_scope')} "
        f"object {replay.get('object_exit')} widen {replay.get('widen_exit')} class {replay.get('widen_class') or 'absent'} "
        f"cancel {replay.get('cancel_exit')} inflight {replay.get('inflight_57014') or 'absent'} "
        f"rollback {replay.get('rollback_confirmed')} reopen {replay.get('reopen_exit')} install_called {replay.get('install_called')} "
        f"grants {replay.get('role_grants_before')}/{replay.get('role_grants_after')}. "
        f"Logistics replay role {logistics_replay.get('role') or 'absent'} dataset {logistics_replay.get('dataset') or 'absent'} "
        f"grants {logistics_replay.get('role_grants_before')}/{logistics_replay.get('role_grants_after')} "
        f"hidden {logistics_replay.get('hidden_exit')} object {logistics_replay.get('object_exit')} "
        f"widen {logistics_replay.get('widen_exit')} cancel {logistics_replay.get('cancel_exit')} "
        f"inflight {logistics_replay.get('inflight_57014') or 'absent'} rollback {logistics_replay.get('rollback_confirmed')} "
        f"reopen {logistics_replay.get('reopen_exit')} install_called {logistics_replay.get('install_called')}. "
        "catalog-after-consume.txt records inherit false, oo_r_east 6, oo_r_reader 32, and both awaiting_publish runs. "
        f"Producer arrow sha256 {arrow_sha or 'absent'}."
    )
    if gaps:
        summary += " Gaps: " + ", ".join(gaps) + "."
    return {
        "ok": not gaps,
        "gaps": gaps,
        "rows": measured,
        "run_id": imported.get("run_id"),
        "pack_digest": pack,
        "identity_hash": identity,
        "config_digest": config_digest,
        "cache": sealed.get("cache"),
        "model": sealed.get("model"),
        "hit_cache": hit.get("cache"),
        "hit_model": hit.get("model"),
        "role": policy.get("role"),
        "visibility_role": visibility.get("role"),
        "install_effect": chosen.get("effect"),
        "install_role": chosen.get("role"),
        "distinct_note_ids": load.get("distinct_note_ids"),
        "window_queries": window["query_count"],
        "window_writes": window["write_attempts"],
        "physical_table": physical,
        "replay_role": replay.get("role"),
        "hidden_exit": replay.get("hidden_exit"),
        "object_exit": replay.get("object_exit"),
        "widen_exit": replay.get("widen_exit"),
        "cancel_exit": replay.get("cancel_exit"),
        "inflight_57014": replay.get("inflight_57014"),
        "rollback_confirmed": replay.get("rollback_confirmed"),
        "reopen_exit": replay.get("reopen_exit"),
        "install_called": replay.get("install_called"),
        "grants": replay.get("role_grants_before"),
        "oracle_cells": oracle.get("cells"),
        "oracle_mismatches": oracle.get("mismatches"),
        "logistics_replay_role": logistics_replay.get("role"),
        "logistics_replay_grants": logistics_replay.get("role_grants_before"),
        "second_database_status": refused_status,
        "arrow_sha256": arrow_sha,
        "summary": summary,
    }


def main() -> None:
    exits = []
    exit_path = EVIDENCE / "linux" / "loop-exits.txt"
    if exit_path.is_file():
        for line in exit_path.read_text().splitlines():
            parts = line.split()
            if len(parts) == 3:
                exits.append({"name": parts[0], "got": int(parts[1]), "want": int(parts[2])})
    build = EVIDENCE / "linux" / "compile" / "build.log"
    build_text = build.read_text(errors="replace") if build.is_file() else ""
    compiled = "compile-inner-ok" in build_text
    run = read_json(RUN / "run.json") or {}
    preview = read_json(RUN / "preview-attempt.json") or {}
    token = run.get("token", "")
    mismatches = [item for item in exits if item["got"] != item["want"]]
    hubs_out = stage_text("producer-hubs.stdout")
    ship_out = stage_text("producer-shipments.stdout")
    events_out = stage_text("producer-tracking_events.stdout")
    events_status = stage_text("producer-tracking_events.status")
    producer_row_block = events_out == "step=read fact=max_rows" and events_status == "1"
    events_evidence = read_json(RUN / "producer" / "tracking_events" / "evidence.json") or {}
    events_decision = read_json(RUN / "producer" / "tracking_events" / "decision.json") or {}
    row_rate = (events_evidence.get("settings") or {}).get("row_rate_per_sec")
    audit_doc = read_json(RUN / "audit.json") or {}
    audit_decision = audit_doc.get("decision") or {}
    audit_zero = audit_decision.get("zero_write") or {}
    audit_local = audit_doc.get("local_classifier_not_admission_binary") or {}
    audit_verbs = sorted(set(audit_decision.get("reader_verbs") or []))
    cancel_doc = read_json(RUN / "cancel.json") or {}
    source_csv = EVIDENCE / "linux" / "audit" / "source.csv"
    savepoint_count = source_csv.read_bytes().upper().count(b"SAVEPOINT") if source_csv.is_file() else None
    reader_bad, other_bad = statement_verb_counts(source_csv, "oo_dl01w1_46_reader")
    boundary = stream_lock_boundary(source_csv)
    reader_bad_text = ", ".join(f"{name} {reader_bad[name]}" for name in sorted(reader_bad)) or "none"
    other_bad_text = ", ".join(f"{name} {other_bad[name]}" for name in sorted(other_bad)) or "none"
    import_doc = read_json(RUN / "stages" / "import-pg.stdout") or {}
    import_rows = [
        f"{item.get('name')} {item.get('rows')}"
        for item in import_doc.get("tables") or []
        if isinstance(item, dict)
    ]
    scenario3_exit_path = EVIDENCE / "linux" / "scenario-3.exit"
    scenario3_exit = scenario3_exit_path.read_text().strip() if scenario3_exit_path.is_file() else "absent"
    preview_ok = preview.get("describe") == 0 and preview.get("query") == 0 and preview.get("cancel") == 0 and preview.get("detach_describe") == 0
    catalog_request = read_json(RUN / "stages" / "catalog-import.request.json") or {}
    catalog_stdout = read_json(RUN / "stages" / "catalog-import.stdout") or {}
    consumer_wait = read_json(RUN / "consumer-wait.json") or {}
    attach = read_json(RUN / "consumer-attach" / "result.json") or {}
    sealed_result = read_json(RUN / "stages" / "sealed" / "result.json") or {}
    unknown_result = read_json(RUN / "stages" / "sealed-unknown" / "result.json") or {}
    rebind_auth = read_json(RUN / "stages" / "sealed-rebind" / "authority.json") or {}
    rebind_result = read_json(RUN / "stages" / "sealed-rebind" / "result.json") or {}
    rebind_project = rebind_auth.get("project")
    rebind_source = (rebind_auth.get("import") or {}).get("source_id")
    sealed_auth = read_json(RUN / "stages" / "sealed" / "authority.json") or {}
    sealed_project = sealed_auth.get("project")
    sealed_source = (sealed_auth.get("import") or {}).get("source_id")
    project_aligned = sealed_project == sealed_source == "oo_dl01w1_46"
    auth_401 = read_json(RUN / "stages" / "sealed-401" / "result.json") or {}
    auth_timeout = read_json(RUN / "stages" / "sealed-timeout" / "result.json") or {}
    source_state = container_state("oo-dl01w1-46-src")
    target_state = container_state("oo-dl01w1-46-tgt")
    tools_state = container_state("oo-dl01w1-46-tools")
    table_consume = load_table_consume()
    customer = load_customer()
    preview_log_path = EVIDENCE / "linux" / "compile" / "build-preview-rebuild.log"
    preview_log = preview_log_path.read_text(errors="replace") if preview_log_path.is_file() else ""
    preview_lines = preview_log.splitlines()
    preview_compile_ok = (
        "test result: ok. 42 passed; 0 failed" in preview_log
        and "preview-clippy" in preview_lines
        and "preview-clippy-ok" in preview_lines
        and preview_lines[-1:] == ["compile-preview-ok"]
        and "manual_contains" not in preview_log
    )
    preview_binary = EVIDENCE / "linux" / "product" / "postgres-preview"
    preview_sha = sha256_file(preview_binary) if preview_binary.is_file() else "absent"
    preview_capacity = read_json(EVIDENCE / "linux" / "compile" / "capacity-preview.json")
    arrow_doc = read_json(RUN / "stages" / "arrow-reseal.stdout") or {}
    arrow_identity = read_json(RUN / "stages" / "arrow-reseal-identity.json") or {}
    arrow_reason = arrow_doc.get("reason")
    rebind_attempted = (RUN / "stages" / "sealed-rebind.stdout").is_file() or (RUN / "stages" / "rebind-input" / "settings.json").is_file()
    arrow_attempted = (RUN / "stages" / "arrow-reseal.stdout").is_file()
    provenance = (EVIDENCE / "linux" / "product" / "reuse-provenance.txt").read_text(errors="replace").strip() if (EVIDENCE / "linux" / "product" / "reuse-provenance.txt").is_file() else ""
    catalog_engine = (catalog_request.get("manifest") or {}).get("engine")
    catalog_stderr_path = RUN / "stages" / "catalog-import.stderr"
    catalog_stderr = catalog_stderr_path.read_text(errors="replace") if catalog_stderr_path.is_file() else ""
    engine_refused = "source engine" in catalog_stderr
    gates_ok = (
        compiled
        and exits
        and not mismatches
        and token == "closed"
        and preview_ok
        and catalog_engine == "postgresql"
        and not engine_refused
    )
    before_capacity = read_json(EVIDENCE / "linux" / "compile" / "capacity-before-900.json")
    after_capacity = read_json(EVIDENCE / "linux" / "compile" / "capacity.json")
    run_exit_path = EVIDENCE / "linux" / "run-all.exit"
    run_exit = run_exit_path.read_text().strip() if run_exit_path.is_file() else "absent"
    db_before = read_json(EVIDENCE / "linux" / "databases" / "capacity-before-900.json")
    db_after = read_json(EVIDENCE / "linux" / "databases" / "capacity.json")
    phase_exit_path = EVIDENCE / "linux" / "phase-after-compile.exit"
    phase_exit = phase_exit_path.read_text().strip() if phase_exit_path.is_file() else "absent"
    db_blocked = bool(db_after) and db_after.get("ok") is False and not exits
    product_dir = EVIDENCE / "linux" / "product"
    products = []
    if product_dir.is_dir():
        for path in sorted(product_dir.iterdir()):
            if path.is_file():
                products.append({"name": path.name, "bytes": path.stat().st_size, "sha256": sha256_file(path)})
    present = {row["name"] for row in products}
    closure_ok = (
        project_aligned
        and not mismatches
        and attach.get("bind_split") is False
        and attach.get("describe_exit") == 0
        and attach.get("query_exit") == 0
        and attach.get("oracle_literals") is True
        and attach.get("hidden_exit") == 6
        and attach.get("widen_exit") == 9
        and attach.get("reopen_exit") == 0
        and attach.get("cancel_exit") == 0
        and attach.get("inflight_57014") == "user_cancel"
        and attach.get("rollback_confirmed") is True
        and attach.get("source_open") is False
        and attach.get("source_captured") is False
        and attach.get("install_exit") == 0
        and attach.get("install_effect") in {"installed", "replay"}
        and attach.get("provision_exit") == 0
        and attach.get("provision_effect") in {"installed", "replay"}
        and sealed_result.get("cache") == "miss"
        and sealed_result.get("model") == "called"
        and sealed_result.get("pack") == "produced"
        and sealed_result.get("outcome") == "packed"
        and auth_401.get("http_status") == 401
        and auth_401.get("pack") == "not_produced"
        and auth_401.get("model") == "called"
        and auth_timeout.get("reason") == "timeout"
        and auth_timeout.get("http_status") == 0
        and auth_timeout.get("pack") == "not_produced"
        and auth_timeout.get("model") == "called"
        and unknown_result.get("outcome") == "cost_unknown"
        and unknown_result.get("pack") == "not_produced"
        and target_state == "running"
        and source_state == "exited"
        and table_consume["ok"]
        and preview_compile_ok
        and preview_sha == PREVIEW_SHA
        and customer["ok"]
    )
    if closure_ok:
        outcome = "author_complete"
        remaining = []
    elif exits or attach.get("called") is True or bool(sealed_result.get("outcome")):
        outcome = "incomplete"
        remaining = []
        if mismatches:
            remaining.append(
                "loop-exits: "
                + ", ".join(f"{item['name']} {item['got']}/{item['want']}" for item in mismatches)
            )
        remaining.append(
            f"sealed project {sealed_project or 'absent'} source_id {sealed_source or 'absent'}. "
            f"Builder outcome {sealed_result.get('outcome') or 'absent'}, pack {sealed_result.get('pack') or 'absent'}, "
            f"cache {sealed_result.get('cache') or 'absent'}, model {sealed_result.get('model') or 'absent'}, "
            f"pack_digest {sealed_result.get('pack_digest') or 'absent'}."
        )
        remaining.append(
            f"401 http_status {auth_401.get('http_status')}, pack {auth_401.get('pack') or 'absent'}, model {auth_401.get('model') or 'absent'}. "
            f"timeout reason {auth_timeout.get('reason') or 'absent'}, http_status {auth_timeout.get('http_status')}, "
            f"pack {auth_timeout.get('pack') or 'absent'}, model {auth_timeout.get('model') or 'absent'}. "
            f"unknown outcome {unknown_result.get('outcome') or 'absent'}, pack {unknown_result.get('pack') or 'absent'}."
        )
        if attach.get("called") is True:
            remaining.append(attach_gap(attach))
        else:
            remaining.append(
                f"consume result is absent. consumer-wait token {consumer_wait.get('token') or 'absent'}, called {consumer_wait.get('called')}."
            )
        remaining.append(
            "three-table consume complete "
            f"{table_consume['complete']} source_open {table_consume['source_open']} "
            f"exits {table_consume['query_exits']} oracle cells {table_consume['cells']} "
            f"mismatches {table_consume['mismatch_count']} install {table_consume['install_effect']} "
            f"partial_role {table_consume['partial_role']} first_stderr {table_consume['first_stderr']} "
            f"preview_compile_ok {preview_compile_ok} preview_sha {preview_sha}."
        )
        if not customer["ok"]:
            remaining.append("customer chain gaps: " + ", ".join(customer["gaps"]))
        remaining.append(f"containers source {source_state}, target {target_state}, tools {tools_state}.")
        remaining.append(
            "A missing owned database is an incomplete source run. import-native postgresql_retag is an intentional refusal, not an external block."
        )
    elif project_aligned or rebind_attempted or arrow_attempted:
        # The historical run.json token recorded the old project split. It is not this termination.
        outcome = "blocked_external"
        remaining = []
        if project_aligned:
            usage = rebind_result.get("usage") or {}
            remaining.append(
                f"rebind authority project {rebind_project} matches import source_id {rebind_source}. "
                f"Builder outcome {rebind_result.get('outcome') or 'absent'}, pack {rebind_result.get('pack') or 'absent'}, "
                f"pack_digest {rebind_result.get('pack_digest') or 'absent'}, cache {rebind_result.get('cache') or 'absent'}, "
                f"model {rebind_result.get('model') or 'absent'}, "
                f"prompt_tokens {usage.get('prompt_tokens')}, completion_tokens {usage.get('completion_tokens')}, "
                f"total_tokens {usage.get('total_tokens')}."
            )
        else:
            remaining.append(
                f"rebind authority project {rebind_project or 'absent'} import source_id {rebind_source or 'absent'} "
                f"builder outcome {rebind_result.get('outcome') or 'absent'}."
            )
        if arrow_attempted:
            remaining.append(
                f"import-native source_engine postgresql reason {arrow_reason or 'absent'} "
                f"load_state {arrow_doc.get('load_state') or 'absent'} "
                f"new_run_id {arrow_identity.get('new_run_id')}. "
                f"prior seal run {arrow_identity.get('prior_seal_run_id') or 'absent'} is not reused. "
                "Named volume oo-dl01w1-46 target data is absent. "
                "The public command did not receive a relabeled engine."
            )
        if attach.get("called") is True:
            remaining.append(attach_gap(attach))
            describe_doc = read_json(RUN / "consumer-attach" / "describe.stdout") or {}
            dialed = (describe_doc.get("import") or {}).get("source_dialed")
            if attach.get("query_exit") == 4 or attach.get("hidden_exit") != 6:
                remaining.append(
                    f"describe outcome {describe_doc.get('outcome') or 'absent'}, source_dialed {dialed}. "
                    "No oo-dl01w1-46 database container was running after the target volume was absent. "
                    "Hidden guard exit 6, trusted oracle rows, in-flight SQLSTATE 57014 rollback, and a source reopen session were not reached."
                )
        remaining.append(
            f"historical run token {token or 'absent'} stays on run.json. "
            "Source-owned project mismatch is not the shared-contract termination."
        )
        if mismatches:
            remaining.append(
                "historical loop-exits stay recorded: "
                + ", ".join(f"{item['name']} {item['got']}/{item['want']}" for item in mismatches)
            )
    elif engine_refused or catalog_engine == "mysql" or token in CONTRACT or any(str(item.get("name", "")).startswith("builder") for item in mismatches):
        outcome = "blocked_shared_contract"
        remaining = [f"{item['name']} got {item['got']} wanted {item['want']}" for item in mismatches]
        if token:
            remaining.append(f"run token {token}")
        if engine_refused:
            remaining.append("catalog-import stderr contains: source engine")
        if catalog_engine and catalog_engine != "postgresql":
            remaining.append(f"catalog request engine {catalog_engine}")
        if boundary and boundary["pairs"]:
            session = boundary["pairs"][0]["savepoint"]["session_id"]
            preview_n = sum(boundary["preview_verbs"].values())
            preview_outside = sum(outside_allowlist(boundary["preview_verbs"]).values())
            remaining.append(
                "reader user oo_dl01w1_46_reader application_name oo-pg-source "
                f"emitted SAVEPOINT {boundary['producer_verbs'].get('SAVEPOINT', 0)} "
                f"and RELEASE {boundary['producer_verbs'].get('RELEASE', 0)} "
                f"on session {session}; "
                f"owned application_name oo-dl01w1-46 has {preview_n} reader statements "
                f"({verb_text(boundary['preview_verbs'])}) "
                f"and {preview_outside} outside the original allow-list; "
                "source.csv is retained; the allow-list was not edited; "
                "index linux/audit/savepoint-boundary.json"
            )
        if attach.get("called") is True:
            remaining.append(attach_gap(attach))
            remaining.append(
                f"Sealed outcome is {sealed_result.get('outcome') or 'absent'}, pack is {sealed_result.get('pack') or 'absent'}, "
                f"pack_digest {sealed_result.get('pack_digest') or 'absent'}. "
                f"Unknown-price outcome is {unknown_result.get('outcome') or 'absent'}, pack is {unknown_result.get('pack') or 'absent'}."
            )
        elif consumer_wait.get("token") == "consumer.uuid":
            remaining.append(
                "operator-preview was not called. consumer-wait.json token is consumer.uuid. "
                "The typed consumer commit on hand is 8757999b2, which admits numeric precision 1..=76, varchar, float8, timestamptz, and json. "
                "A handoff that admits PostgreSQL uuid oid 2950 is absent. "
                f"Sealed outcome is {sealed_result.get('outcome') or 'absent'}, pack is {sealed_result.get('pack') or 'absent'}. "
                f"Unknown-price outcome is {unknown_result.get('outcome') or 'absent'}, pack is {unknown_result.get('pack') or 'absent'}. "
                "Trusted typed query, deny, and sealed consumer cancel stay unconnected."
            )
    else:
        outcome = "blocked_external"
        remaining = [f"{item['name']} got {item['got']} wanted {item['want']}" for item in mismatches]
        if db_blocked:
            remaining.append(capacity_fact("linux/databases/capacity-before-900.json", db_before))
            remaining.append(capacity_fact("linux/databases/capacity.json", db_after))
            remaining.append(
                f"live.sh exit {run_exit} after compile-inner-ok and builder-removed; "
                "one 900s wait; database capacity() recheck exited before docker run"
            )
            remaining.append(
                f"immediate recheck linux/phase-after-compile.exit {phase_exit}; "
                f"need_cpus {db_after.get('need_cpus')} reserved_cpus {db_after.get('reserved_cpus')} "
                f"rust_builders {db_after.get('rust_builders')} "
                f"own_rust_builders {db_after.get('own_rust_builders')} "
                "ok false docker_started false; reserved_cpus + need_cpus exceeds host_ncpu 16; "
                "no oo-dl01w1-46 database container started; source loop did not start"
            )
        elif after_capacity and after_capacity.get("ok") is False and not compiled:
            remaining.append(capacity_fact("capacity-before-900.json", before_capacity))
            remaining.append(capacity_fact("capacity.json", after_capacity))
            remaining.append(
                f"live.sh exit {run_exit} after one 900s wait; "
                "capacity() on the recheck exited before docker run; no attempt-13"
            )
        if not compiled and not db_blocked:
            remaining.append("compile did not reach compile-inner-ok")
        for name in ("oo-pipeline", "init-pg", "pg-builder"):
            if name not in present:
                remaining.append(f"linux/product/{name} absent")
        if token:
            remaining.append(f"run token {token}")
        if producer_row_block:
            remaining.append(
                "producer-tracking_events status 1 stdout step=read fact=max_rows; "
                f"producer-hubs stdout {hubs_out or 'absent'} status 0; "
                f"producer-shipments stdout {ship_out or 'absent'} status 0"
            )
            remaining.append(
                "tracking_events decision admitted "
                f"{events_decision.get('admitted')} blocks {events_decision.get('blocks')}; "
                f"evidence row_rate_per_sec {row_rate}; oracle event_count 90000; "
                f"run.json rows {run.get('rows')} bytes {run.get('bytes')}; "
                "pinned producer 6590cff4acc1cb265d0ed05d460b0582251cf360 gate-read uses "
                "limit max_rows+1 and refuses when that limit exceeds MAX_ROWS_PER_SEC 50000 "
                "and the table fills the window; the producer was not edited"
            )
            remaining.append(
                "import-pg, catalog-import, pg-builder, and operator-preview did not run; "
                f"model_called {run.get('model_called')}"
            )
            remaining.append(
                f"{capacity_fact('scenario-3 linux/databases/capacity.json', db_after)}; "
                "four database containers started and were removed; "
                f"scenario-3.exit {scenario3_exit} is the harness record_exit result, not product success"
            )
            remaining.append(
                "audit stage is not the sealed preview path: "
                f"admission_binary {audit_doc.get('admission_binary')} "
                f"zero_write.proven {audit_zero.get('proven')} basis {audit_zero.get('basis')} "
                f"local.proven {audit_local.get('proven')} local.basis {audit_local.get('basis')} "
                f"reader_statements {audit_decision.get('reader_statements')} "
                f"forbidden {audit_decision.get('forbidden')} "
                f"other_users_with_forbidden {audit_decision.get('other_users_with_forbidden')} "
                f"reader_verbs {' '.join(audit_verbs) or 'absent'} "
                f"source.csv SAVEPOINT count {savepoint_count}; "
                f"cancel sqlstate {cancel_doc.get('sqlstate')} "
                f"backend_active {cancel_doc.get('backend_active')} "
                f"rows {cancel_doc.get('rows')} via tokio-postgres CancelToken"
            )
        if not exits and not db_blocked:
            remaining.append("source loop did not record stage exits")
    boundary_doc = None
    if boundary is not None:
        tables = []
        for item in (catalog_request.get("manifest") or {}).get("tables") or []:
            if isinstance(item, dict):
                tables.append({
                    "accepted_rows": item.get("accepted_rows"),
                    "logical_schema": item.get("logical_schema"),
                    "logical_table": item.get("logical_table"),
                })
        boundary_doc = {
            "protocol": "oo.postgres-preview.savepoint-boundary/v1",
            "source_csv": "linux/audit/source.csv",
            "allowlist": sorted(ALLOWED),
            "owned_preview": {
                "application_name": "oo-dl01w1-46",
                "source": "tools/postgres-preview/src/pg.rs:113",
                "reader": "oo_dl01w1_46_reader",
                "statements": sum(boundary["preview_verbs"].values()),
                "verbs": boundary["preview_verbs"],
                "outside_allowlist": outside_allowlist(boundary["preview_verbs"]),
                "deadline_transaction": "tools/postgres-preview/src/pg.rs:291 BEGIN; SET LOCAL statement_timeout = '400ms'; SELECT pg_sleep(2); ROLLBACK",
                "cancel": "tools/postgres-preview/src/pg.rs:311 CancelToken.cancel_query on SELECT pg_sleep(30)",
            },
            "producer": {
                "owner": "DL01-W1-38",
                "commit": "4055237dfeb6df672388419dd3889a6fcaa4d356",
                "evidence_receipt": "a3cd94c2c4337528342c8ece8d3ec142e2d8b2fc",
                "application_name": "oo-pg-source",
                "application_name_source": "crates/oo-connectors/src/sql.rs:4 APPLICATION_NAME = oo-pg-source",
                "reader": "oo_dl01w1_46_reader",
                "verbs": {name: boundary["producer_verbs"][name] for name in sorted(boundary["producer_verbs"])},
                "outside_allowlist": outside_allowlist(boundary["producer_verbs"]),
                "w46_call": "tools/postgres-preview/src/chain.rs produce_one runs oo-source-admission-pg-budget --phase bounded-read and does not call read_probe",
                "pairs": boundary["pairs"],
                "prior_savepoint_log": "linux/failed-attempt-c2cbba992/source.csv is producer 8ecabc22cc2943932c2f51113600713571a37db0",
            },
            "catalog_import": {
                "owner": "DL01-W1-06",
                "pipeline_commit": "5d5547882bdb20df2ac96db6c5727c823667fa1d",
                "semantics_commit": "f395b0015711920ed0fe66a6c7ce1b928a48635f",
                "request": "linux/runner/out/run/stages/catalog-import.request.json",
                "request_protocol": catalog_request.get("protocol"),
                "manifest_protocol": (catalog_request.get("manifest") or {}).get("protocol"),
                "engine": catalog_engine,
                "source_id": catalog_request.get("source_id"),
                "seal_run_id": (catalog_request.get("manifest") or {}).get("run_id"),
                "tables": tables,
                "stderr": catalog_stderr.strip(),
                "status": stage_text("catalog-import.status") or None,
                "stdout_protocol": catalog_stdout.get("protocol"),
                "stop": "catalog-import-pg wrote oo.pipeline.sealed-import-catalog/v1 after the target applied migration 0007. The pinned installer applies through 0006.",
                "postgresql_retag": "not called",
            },
        }
        boundary_path = EVIDENCE / "linux" / "audit" / "savepoint-boundary.json"
        boundary_path.write_text(json.dumps(boundary_doc, indent=2, sort_keys=True) + "\n")
        print(
            "boundary"
            f" pairs {len(boundary['pairs'])}"
            f" preview_outside {sum(outside_allowlist(boundary['preview_verbs']).values())}"
            f" producer_savepoint {boundary['producer_verbs'].get('SAVEPOINT', 0)}"
            f" producer_release {boundary['producer_verbs'].get('RELEASE', 0)}"
        )
    decisive = []
    for relative in [
        "linux/compile/attempt-1.log",
        "linux/compile/attempt-2.log",
        "linux/compile/attempt-3.log",
        "linux/compile/attempt-4.log",
        "linux/compile/attempt-5.log",
        "linux/compile/attempt-6.log",
        "linux/compile/attempt-7.log",
        "linux/compile/attempt-8.log",
        "linux/compile/attempt-9.log",
        "linux/compile/attempt-10.log",
        "linux/compile/attempt-11.log",
        "linux/compile/attempt-12.log",
        "linux/compile/attempt-13.log",
        "linux/bounded-read-loop.log",
        "linux/compile/build.log",
        "linux/compile/capacity-before-900.json",
        "linux/compile/capacity.json",
        "linux/compile/containers.txt",
        "linux/compile/run-all-attempt-12.log",
        "linux/run-all.log",
        "linux/run-all.exit",
        "linux/loop-exits.txt",
        "linux/runner/out/run/run.json",
        "linux/runner/out/run/plan.json",
        "linux/runner/out/run/preview-attempt.json",
        "linux/runner/out/run/stages/import-pg.stderr",
        "linux/runner/out/run/stages/init-pg.stderr",
        "linux/runner/out/run/stages/catalog-import.stderr",
        "linux/runner/out/run/stages/catalog-import.request.json",
        "linux/runner/out/run/chain-stops.json",
        "linux/runner/out/run/stages/sealed.stderr",
        "linux/runner/out/run/stages/preview-describe.stderr",
        "linux/audit/source.csv",
        "linux/audit/savepoint-boundary.json",
        "linux/databases/capacity-before-900.json",
        "linux/databases/capacity.json",
        "linux/databases/containers-before-900.txt",
        "linux/databases/containers.txt",
        "linux/failed-attempt-d70bd5ff3/whole-source-delivery.json",
        "linux/failed-attempt-d70bd5ff3/run-all.exit",
        "linux/failed-attempt-db-capacity-exit3/capacity.json",
        "linux/failed-attempt-db-capacity-exit3/containers.txt",
        "linux/phase-after-compile.log",
        "linux/phase-after-compile.exit",
        "linux/product/binary-sha256.txt",
        "linux/product/reuse-provenance.txt",
        "linux/compile/build-e16f5a892.log",
        "linux/scenario-3.log",
        "linux/scenario-3.exit",
        "linux/scenario-2.log",
        "linux/scenario-2.exit",
        "linux/failed-attempt-admission-denied/run.json",
        "linux/failed-attempt-admission-denied/decision.json",
        "linux/failed-attempt-db-capacity-exit3/whole-source-delivery.json",
        "linux/failed-attempt-producer-rows/scenario-3.log",
        "linux/failed-attempt-producer-rows/run.json",
        "linux/runner/out/run/stages/producer-tracking_events.stdout",
        "linux/runner/out/run/stages/producer-tracking_events.status",
        "linux/runner/out/run/stages/producer-hubs.stdout",
        "linux/runner/out/run/stages/producer-shipments.stdout",
        "linux/runner/out/run/producer/tracking_events/decision.json",
        "linux/runner/out/run/audit.json",
        "linux/runner/out/run/cancel.json",
        "linux/audit/source.csv",
        "linux/compile/build-consume.log",
        "linux/compile/capacity-consume.json",
        "linux/compile/containers-consume.txt",
        "linux/consume.exit",
        "linux/consume.stdout",
        "linux/consume.stderr",
        "linux/runner/out/run/consumer-attach/result.json",
        "linux/runner/out/run/consumer-attach/describe.stdout",
        "linux/runner/out/run/consumer-attach/describe.stderr",
        "linux/runner/out/run/consumer-attach/describe.exit",
        "linux/runner/out/run/consumer-attach/query.stdout",
        "linux/runner/out/run/consumer-attach/query.stderr",
        "linux/runner/out/run/consumer-attach/query.exit",
        "linux/runner/out/run/consumer-attach/hidden.stdout",
        "linux/runner/out/run/consumer-attach/hidden.stderr",
        "linux/runner/out/run/consumer-attach/hidden.exit",
        "linux/runner/out/run/consumer-attach/widen.stdout",
        "linux/runner/out/run/consumer-attach/widen.stderr",
        "linux/runner/out/run/consumer-attach/widen.exit",
        "linux/runner/out/run/consumer-attach/cancel.stdout",
        "linux/runner/out/run/consumer-attach/cancel.stderr",
        "linux/runner/out/run/consumer-attach/cancel.exit",
        "linux/runner/out/run/consumer-attach/reopen.stdout",
        "linux/runner/out/run/consumer-attach/reopen.stderr",
        "linux/runner/out/run/consumer-attach/reopen.exit",
        "linux/consumer-attach/describe.stderr",
        "linux/consumer-attach/query.stderr",
        "linux/consumer-attach/cancel.stderr",
        "linux/consumer-attach/hash.txt",
        "linux/runner/out/run/consumer-all/result.json",
        "linux/runner/out/run/typed-table-oracle-result.json",
        "linux/runner/out/run/dataset-authorities.json",
        "linux/compile/build-preview-rebuild.log",
        "linux/runner/out/run/consumer-all/acl-set/install-column-acl.stderr",
        "linux/runner/out/run/consumer-all/acl-set/partial-role.txt",
        "linux/runner/out/run/consumer-all/acl-set-retry/install-column-acl.stdout",
        "linux/runner/out/run/consumer-role-replay/result.json",
        "linux/runner/work/config-customer-east.json",
        "linux/runner/work/customer-east-unapproved.json",
        "linux/runner/out/customer-east/customer-load.json",
        "linux/runner/out/customer-east/customer-load.exit",
        "linux/runner/out/customer-east/unapproved-refusal.json",
        "linux/runner/out/customer-east/unapproved-refusal.exit",
        "linux/runner/out/customer-east/customer-run.exit",
        "linux/runner/out/customer-east/consume-all.exit",
        "linux/runner/out/customer-east/consumer-role-replay.exit",
        "linux/runner/out/customer-east/consumer-role-replay/result.json",
        "linux/runner/out/customer-east/consumer-all/result.json",
        "linux/runner/out/customer-east/consumer-all/acl-set/install-column-acl.stdout",
        "linux/runner/out/customer-east/typed-table-oracle-result.json",
        "linux/runner/out/customer-east/chain-stops.json",
        "linux/runner/out/customer-east/state.json",
        "linux/runner/out/customer-east/stages/import-pg.stdout",
        "linux/runner/out/customer-east/stages/sealed/result.json",
        "linux/runner/out/customer-east/stages/sealed/policy.json",
        "linux/runner/out/customer-east/stages/sealed-table-0/result.json",
        "linux/runner/out/customer-east/stages/sealed-401/result.json",
        "linux/runner/out/customer-east/stages/sealed-timeout/result.json",
        "linux/runner/out/customer-east/stages/sealed-unknown/result.json",
        "linux/runner/out/customer-east/second-database-refused/init-pg.status",
        "linux/runner/out/customer-east/second-database-refused/init-pg.stderr",
        "linux/runner/out/customer-east/catalog-after-consume.txt",
        "linux/runner/out/customer-east/source-count.json",
        "linux/runner/out/customer-east/source-window.csv",
        "linux/failed-attempt-customer-41/customer-load.json",
        "linux/runner/out/customer-east/producer/customer_east--delivery_notes/delivery_notes.arrow",
        "linux/compile/build-preview-measure-ok.log",
    ]:
        if (EVIDENCE / relative).is_file():
            decisive.append(relative)
    skip = {
        EVIDENCE / "whole-source-delivery.json",
        EVIDENCE / "SHA256SUMS",
        EVIDENCE / "REPORT.md",
        EVIDENCE / "FINDINGS.md",
        EVIDENCE / "HANDOFF.md",
        EVIDENCE / "receipt.json",
    }
    source_sha = tree_sha(TOOL, set())
    evidence_sha = tree_sha(EVIDENCE, skip)
    delivery = {
        "protocol": "oo.source-product-delivery/v1",
        "task_id": "DL01-W1-46",
        "source_id": "oo_dl01w1_46",
        "outcome": outcome,
        "source_sha": source_sha,
        "evidence_sha": evidence_sha,
        "decisive_case_paths": decisive,
        "remaining_conditions": remaining,
        "not_independent_acceptance": True,
        "attempts": {
            "attempt_1": "linux/compile/attempt-1.log login shell PATH, cargo not found, exit 127",
            "attempt_2": "linux/compile/attempt-2.log preview rustc E0425 E0599 E0282 E0716, container exit 101",
            "attempt_3": "linux/compile/attempt-3.log three E0277 serde_json::Error to Fail in chain.rs, container exit 101",
            "attempt_4": "linux/compile/attempt-4.log three E0631 i64::to_string fn(&i64) does not match Option::map, container exit 101",
            "attempt_5": "linux/compile/attempt-5.log lib test E0277 Parsed and Spawned missing Debug, E0506 facts borrowed, container exit 101",
            "attempt_6": "linux/compile/attempt-6.log E0618 facts() shadowed by the local SessionFacts binding, container exit 101",
            "attempt_7": "linux/compile/attempt-7.log arrow round-trip test looked for raw IPC bytes shipment_id; 20 other tests passed; container exit 101",
            "attempt_8": "linux/compile/attempt-8.log 21 tests passed; cargo-clippy component missing so cargo clippy exited 1; container exit 1",
            "attempt_9": "linux/compile/attempt-9.log clippy -D warnings: while_let_on_iterator and let_underscore_future; container exit 101",
            "attempt_10": "linux/compile/attempt-10.log 21 tests passed and clippy finished; release binary was not linked, cp exited 1; container exit 1",
            "attempt_11": "linux/compile/attempt-11.log admission binary linked; pg-budget manifest could not inherit edition because the archive had no workspace root; container exit 101",
            "attempt_12": "linux/compile/attempt-12.log budget Finished release in 4m 48s; cargo --locked refused to update /frozen/pipeline/Cargo.lock after the archive rewrote workspace members; container exit 101",
            "capacity_900": "linux/failed-attempt-d70bd5ff3 archives the earlier live.sh exit 3: one 900s wait; recheck reserved_cpus 16.0 need_cpus 1 rust_builders 0; docker_started false; no attempt-13. That receipt is not this launch.",
            "compile_39b2": "The 39b2c1e17202a089897b3076ee0d71e194f767e7 pipeline compile is preserved in linux/compile/build-e16f5a892.log. The later 23-test preview rebuild used admission d740f7a1 and budget a7b06ae4 from 6590cff4acc1cb265d0ed05d460b0582251cf360. linux/compile/build.log is no longer that log.",
            "attempt_13": "linux/compile/attempt-13.log clippy items_after_test_module after 24 tests passed; container exit 101. The test module now follows write_phase.",
            "bounded_read_compile": "linux/compile/build.log is compile-inner-ok for producer 4055237dfeb6df672388419dd3889a6fcaa4d356, pipeline 5d5547882bdb20df2ac96db6c5727c823667fa1d, and semantics overlay f395b0015711920ed0fe66a6c7ce1b928a48635f. Hashes are linux/product/binary-sha256.txt. The 8ecabc22 and 39b2 hashes stay in linux/failed-attempt-c2cbba992/binary-sha256.txt.",
            "capacity_900_databases": "linux/run-all.exit 3 after compile-ok and builder-removed. capacity-before-900 need_cpus 3 reserved_cpus 14.0 ok false. After 900s, archived linux/failed-attempt-db-capacity-exit3/capacity.json need_cpus 3 reserved_cpus 15.0 rust_builders 1 ok false docker_started false.",
            "capacity_recheck": "linux/phase-after-compile.exit 3. Fresh capacity() need_cpus 3 reserved_cpus 15.0 rust_builders 1 own_rust_builders 0 ok false docker_started false. No oo-dl01w1-46 database container started.",
            "drafts_before_compile": "CLI sources were written during the 2026-10-10T04:29:58Z capacity wait, before either Linux compile",
            "blocked_receipt_d70bd5ff3": "linux/failed-attempt-d70bd5ff3 archives the blocked_external receipt from 606d3893d and d70bd5ff3",
            "blocked_receipt_e16f5a892": "linux/failed-attempt-db-capacity-exit3/whole-source-delivery.json is the capacity receipt from e16f5a892. It is not this run.",
            "admission_denied": "linux/failed-attempt-admission-denied archives main-run exit 2 token admission_denied. The sealed chain did not run.",
            "scenario_2_init_sql": "linux/scenario-2.exit 1. Preview rebuild was compile-inner-ok. Loop stopped at pg-not-ready because a hash comment was inside the init SQL. Containers were removed.",
            "scenario_3_producer_rows": "linux/scenario-3.log reused pinned binaries. capacity ok true need_cpus 3 reserved_cpus 6.0. tracking_events gate-read status 1 stdout step=read fact=max_rows. main-run 8 wanted 0 token producer.rows. Harness scenario-3.exit 0 is not product success.",
            "consumer_attach_compile": (
                "linux/compile/build-consume.log "
                + (
                    "contains compile-preview-ok. "
                    if "compile-preview-ok" in (EVIDENCE / "linux" / "compile" / "build-consume.log").read_text(errors="replace")
                    else "does not contain compile-preview-ok. "
                )
                + "That container compiled tools/postgres-preview only. Admission, pipeline, pg-builder, and operator-preview were not rebuilt."
            ) if (EVIDENCE / "linux" / "compile" / "build-consume.log").is_file() else "linux/compile/build-consume.log absent",
        },
        "partial_product_sha256": products,
        "stage_exits": exits,
        "run_token": token,
        "preview_attempt": preview,
    }
    (EVIDENCE / "whole-source-delivery.json").write_text(json.dumps(delivery, indent=2) + "\n")
    report = [
        "# DL01-W1-46 postgres product",
        "",
        f"Outcome recorded in whole-source-delivery.json: {outcome}.",
        "This file is the author's run record. It is not independent acceptance.",
        "",
        f"Source content hash: {source_sha}",
        f"Evidence content hash: {evidence_sha}",
        f"Run token: {token or 'absent'}",
        f"Compile reached compile-inner-ok: {compiled}",
        "",
        "Stage exits (name got wanted):",
    ]
    report.extend(f"- {item['name']} {item['got']} {item['want']}" for item in exits)
    if not exits:
        report.append("- none recorded")
    report.extend(["", "Remaining conditions:"])
    report.extend(f"- {item}" for item in remaining)
    if not remaining and outcome == "author_complete":
        report.append("- none")
    (EVIDENCE / "REPORT.md").write_text("\n".join(report) + "\n")
    findings = [
        "# Findings",
        "",
        "Attempt 1 used bash -lc. The login shell dropped the Rust image PATH. cargo was not found. The log is attempt-1.log.",
        "Attempt 2 kept that PATH fix and exited 101. Frozen admission tests passed. postgres-preview failed with E0425, E0599, E0282, and E0716. The log is attempt-2.log.",
        "Attempt 3 compiled that fixed tree and exited 101 before clippy. rustc reported three E0277 errors in chain.rs: serde_json::Error does not convert to Fail. The log is attempt-3.log. The login-shell PATH failure remains attempt-1.log.",
        "Attempt 4 replaced those conversions and then failed three E0631 errors in logistics.rs: i64::to_string is fn(&i64) and Option::map requires fn(i64). The log is attempt-4.log.",
        "Attempt 5 compiled the library and failed the test build. Parsed and Spawned lack Debug, so unwrap_err does not compile. The identity test assigns server_version_num while AdmissionInput still borrows facts. The log is attempt-5.log.",
        "Attempt 6 failed one E0618 in that test: the second facts() call saw the local SessionFacts binding. The log is attempt-6.log.",
        "Attempt 7 ran the unit tests. Twenty passed. The Arrow test required the raw IPC bytes to contain shipment_id, and the stream encoding does not. The log is attempt-7.log.",
        "Attempt 8 ran after one 900s capacity wait. The recheck admitted the builder: reserved_cpus 14, rust_builders 1. All 21 unit tests passed. cargo clippy then exited because the clippy component is not installed in the image. The log is attempt-8.log.",
        "Attempt 9 installed clippy and failed two clippy denies: while_let_on_iterator in arrow_out.rs and let_underscore_future on the model-forward spawn. The log is attempt-9.log.",
        "Attempt 10 passed all 21 tests and clippy -D warnings. cargo clippy did not link target/release/postgres-preview, so the copy exited 1. The log is attempt-10.log.",
        "Attempt 11 linked postgres-preview and oo-source-admission. The budget package failed before compile: oo-connectors inherits edition from the workspace root, and that archive had no root manifest. The log is attempt-11.log.",
        "Attempt 12 linked postgres-preview, oo-source-admission, and oo-source-admission-pg-budget. pipeline-build then exited 101: cargo --locked refused to update /frozen/pipeline/Cargo.lock because the archive had rewritten the workspace members. The log is attempt-12.log. That rewrite is no longer in live.sh.",
        "The launch archived at linux/failed-attempt-d70bd5ff3 pinned the full 7e4e42f member set and did not rewrite pipeline Cargo.toml. capacity() recorded reserved_cpus 16.0 need_cpus 1 rust_builders 0. The script waited 900s. The recheck stayed reserved_cpus 16.0, ok false, docker_started false. capacity() exited 3 before docker run. That blocked_external receipt is preserved. That launch wrote no attempt-13.log.",
        "linux/compile/build.log is this launch and ends at compile-inner-ok. Current binary hashes are linux/product/binary-sha256.txt. The previous admission ae645bda4a5b48a620e036975edccbd865f2c82558812bb8288302813cf7ed0d, budget 285a3bd0afb105e212d852356af759fb9ea77e2d129d0d2a28d76860c288d089, pipeline 2eef24d216240f265f682fa21159c000737ec8f25a29a190544a7a03d211ceba, init-pg 258503d70dd7076c5ffcd27e1d3be8453d74e61602ae1faceda515e6e12f8424, pg-builder db992bff6eae52e7956d29ffa856be3f6bae6701c3645317fddf9b601528fb49, and postgres-preview 2a5c2dcd098f8108a23bcfb1201af2f25e5a1f22b245fe873638d2caba97d5eb stay in linux/failed-attempt-c2cbba992/binary-sha256.txt. operator-preview remains 6608eb5885eacf131121b06ab2337f747210434949f4e3bf34118729a65f8938 and was not executed. linux/compile/attempt-13.log is the earlier clippy items_after_test_module failure, container exit 101. linux/compile/attempt-14.log is the column_typmod import failure.",
        f"The compile capacity() is {capacity_fact('linux/compile/capacity.json', after_capacity)}. docker_started false is the pre-start field. The builder container was removed after compile-ok.",
        "The database-capacity block stays archived. linux/databases/capacity-before-900.json is need_cpus 3 reserved_cpus 14.0 ok false. linux/failed-attempt-db-capacity-exit3/capacity.json is need_cpus 3 reserved_cpus 15.0 rust_builders 1 ok false docker_started false. linux/run-all.exit is 3. linux/phase-after-compile.exit is 3. The receipt from e16f5a892 is copied in that archive. Those files are history.",
        "linux/compile/build-e16f5a892.log is the full 39b2 compile. The 23-test preview rebuild that reused admission d740f7a1 and budget a7b06ae4 from 6590cff4acc1cb265d0ed05d460b0582251cf360 is the committed build.log at 89b8d9f6ce934c49500c0d3768fa4489ef8139ea.",
        "linux/failed-attempt-admission-denied archives the first databases loop. main-run exited 2 with token admission_denied. That producer evidence recorded startup_option, permission status unknown, and temp true. Its reader audit agreed proven true on verbs SELECT, BEGIN, SET, and ROLLBACK, and its source.csv had no SAVEPOINT statement. CancelToken returned sqlstate 57014. The sealed chain did not run.",
        "linux/scenario-2.exit is 1. The preview rebuild succeeded and the loop stopped at pg-not-ready because a hash comment was inside the init SQL. Containers were removed. The comment was deleted. That shell change was not another compile.",
        "Scenario-3 is archived at linux/failed-attempt-producer-rows. Its capacity.json is need_cpus 3 reserved_cpus 6.0 ok true. tracking_events gate-read status 1 stdout was step=read fact=max_rows. main-run token was producer.rows. scenario-3.exit 0 is the harness record.",
        f"This database capacity() is {capacity_fact('linux/databases/capacity.json', db_after)}. docker_started false is the pre-start field. The loop then started oo-dl01w1-46-src, oo-dl01w1-46-tgt, and oo-dl01w1-46-tools. linux/bounded-read-loop.log records the stage lines.",
        f"Admission on this run is known and read_only. Hubs stdout is {hubs_out or 'absent'}. Shipments stdout is {ship_out or 'absent'}. tracking_events status is {events_status or 'absent'} and stdout is {events_out or 'absent'}. Decision admitted is {events_decision.get('admitted')} and blocks are {events_decision.get('blocks')}. row_rate_per_sec is {row_rate}. Oracle counts are hub 2000, shipment 8000, event 90000. run.json run_id is {run.get('run_id')}, rows are {run.get('rows')}, bytes are {run.get('bytes')}, token is {token or 'absent'}, model_called is {run.get('model_called')}. import-pg status is 0, state is {import_doc.get('state')}, seal run_id is {import_doc.get('run_id')}, table rows are {', '.join(import_rows) or 'absent'}. catalog-import request engine is {catalog_engine or 'absent'}.",
        f"The fresh source csvlog audit is part of this loop. admission_binary is {audit_doc.get('admission_binary')}. zero_write.proven is {audit_zero.get('proven')} basis {audit_zero.get('basis')}. The local classifier proven is {audit_local.get('proven')} basis {audit_local.get('basis')}. reader_statements are {audit_decision.get('reader_statements')}, forbidden is {audit_decision.get('forbidden')}, other_users_with_forbidden is {audit_decision.get('other_users_with_forbidden')}. Admission reader verb set is {' '.join(audit_verbs) or 'absent'}. Reader statements outside the allow-list are {reader_bad_text}. Other users' statements outside the allow-list are {other_bad_text}. The byte count of SAVEPOINT in source.csv is {savepoint_count}. The cancel stage sqlstate is {cancel_doc.get('sqlstate')}, backend_active is {cancel_doc.get('backend_active')}, rows are {cancel_doc.get('rows')}. That cancel stage uses tokio-postgres CancelToken.cancel_query. preview-cancel was not written. SAVEPOINT stays off the allow-list. The 091fa109 csvlog is archived at linux/failed-attempt-c2cbba992/source.csv and is not this run.",
        (
            "linux/audit/savepoint-boundary.json is the csv split of the csv copied for this loop. "
            f"Owned application_name oo-dl01w1-46, user oo_dl01w1_46_reader, has {sum(boundary['preview_verbs'].values()) if boundary else 0} statements: {verb_text(boundary['preview_verbs']) if boundary else 'absent'}. "
            f"Statements outside that application's allow-list: {verb_text(outside_allowlist(boundary['preview_verbs'])) if boundary else 'absent'}. "
            "tools/postgres-preview/src/pg.rs sets that application_name and emits the deadline BEGIN/ROLLBACK and the CancelToken cancel. "
            f"Application oo-pg-source verbs: {verb_text(boundary['producer_verbs']) if boundary else 'absent'}. "
            f"Those outside the allow-list: {verb_text(outside_allowlist(boundary['producer_verbs'])) if boundary else 'absent'}. "
            f"SAVEPOINT/RELEASE pairs on that application: {len(boundary['pairs']) if boundary else 0}. "
            "produce_one passes --phase bounded-read and does not pass read_probe. "
            "The allow-list in audit_local.rs is the original list. "
            f"Provenance file: {provenance or 'absent'}."
        ),
        f"scenario-3.exit is {scenario3_exit}. That file is the archived harness record.",
        "The producer pin for this launch is 4055237dfeb6df672388419dd3889a6fcaa4d356, evidence receipt a3cd94c2c4337528342c8ece8d3ec142e2d8b2fc. The phase is bounded-read. The prior gate-read refusal at 6590cff4acc1cb265d0ed05d460b0582251cf360 stays in linux/failed-attempt-producer-rows. The 8ecabc22 window and its csvlog stay in linux/failed-attempt-c2cbba992.",
        "This launch pins pipeline and pg-builder at 5d5547882bdb20df2ac96db6c5727c823667fa1d and overlays crates/oo-core, crates/oo-builder, and tools/source-semantics from f395b0015711920ed0fe66a6c7ce1b928a48635f. The catalog command is catalog-import-pg. The manifest protocol is oo.pipeline.pg-sealed-manifest/v1. pg-builder sealed reads the sealed-import catalog. A mysql manifest is not sent. The historical catalog-import did not call postgresql_retag. A later arrow-reseal, when present, is a separate import-native document whose source_engine stays postgresql.",
        (
            "operator-preview 6608eb5885eacf131121b06ab2337f747210434949f4e3bf34118729a65f8938 stays at linux/product/operator-preview. "
            "The executed consumer is linux/product/operator-preview-bytea. "
            + (attach_gap(attach) if attach.get("called") is True else "The consume result is absent, so that binary has no product-command record in this delivery.")
        ),
        (
            "consume-all used one install-column-acl document for public.hubs, public.shipments, and public.tracking_events. "
            f"The first install stderr is {table_consume['first_stderr']}. partial-role.txt is {table_consume['partial_role']}. "
            f"The retry effect is {table_consume['install_effect'] or 'absent'} and the role is oo_r_reader. "
            f"provision effect is {table_consume['provision_effect'] or 'absent'}. "
            f"Query exits are {table_consume['query_exits']}. complete is {table_consume['complete']}. source_open is {table_consume['source_open']}. "
            f"typed-table-oracle-result.json cells are {table_consume['cells']} and mismatches are {table_consume['mismatch_count']}. "
            f"result.json oracle_verified is {table_consume['oracle_verified_flag']} because consume-all does not run that checker. "
            f"Dataset identity_hash is {table_consume['identity_hash'] or 'absent'}. config_digest is {table_consume['config_digest'] or 'absent'}. "
            + " ".join(table_consume["pack_rows"])
            + f" postgres-preview sha256 is {preview_sha}. compile-preview log ok is {preview_compile_ok}. "
            f"That capacity snapshot is {capacity_fact('linux/compile/capacity-preview.json', preview_capacity)}. "
            "The hubs-only guard tree remains consumer-all-hubs-authority-guard. "
            "The per-table grant mismatch tree remains consumer-all-partial-grant-mismatch. "
            "postgres-preview consume was not run again after the three-table grant, because a one-table policy is a grant mismatch against that role. "
            "The earlier consumer-attach exits on this capture stay describe 0, hidden 6, widen 9, cancel 0, reopen 0, inflight user_cancel, rollback confirmed. "
            "Those commands were not repeated after oo_r_reader was replaced. "
            "The three-table role replay is linux/runner/out/run/consumer-role-replay/result.json."
        ),
        customer["summary"],
        f"catalog-import request engine: {catalog_engine or 'absent'}.",
        "catalog-import stderr: " + (catalog_stderr.strip().splitlines()[-1][:500] if catalog_stderr.strip() else "absent"),
        "",
        f"Delivery outcome: {outcome}.",
    ]
    (EVIDENCE / "FINDINGS.md").write_text("\n".join(findings) + "\n")
    pair_lines = []
    if boundary:
        for pair in boundary["pairs"]:
            business = pair.get("business") or {}
            release = pair.get("release") or {}
            rollback = pair.get("rollback_to") or {}
            savepoint = pair.get("savepoint") or {}
            pair_lines.append(
                "SAVEPOINT session_line {sp_line} csv_record {sp_rec} prepared {sp_prep} at {sp_time}; "
                "business session_line {biz_line} csv_record {biz_rec} prepared {biz_prep}: {biz_sql}; "
                "ROLLBACK TO session_line {rb_line} prepared {rb_prep}; "
                "RELEASE session_line {rel_line} csv_record {rel_rec} prepared {rel_prep} at {rel_time}.".format(
                    sp_line=savepoint.get("session_line"),
                    sp_rec=savepoint.get("csv_record"),
                    sp_prep=savepoint.get("prepared"),
                    sp_time=savepoint.get("time"),
                    biz_line=business.get("session_line"),
                    biz_rec=business.get("csv_record"),
                    biz_prep=business.get("prepared"),
                    biz_sql=business.get("sql"),
                    rb_line=rollback.get("session_line"),
                    rb_prep=rollback.get("prepared"),
                    rel_line=release.get("session_line"),
                    rel_rec=release.get("csv_record"),
                    rel_prep=release.get("prepared"),
                    rel_time=release.get("time"),
                )
            )
    handoff = [
        "# Handoff",
        "",
        "Consumers: DL01-W1-21 author-complete operator-preview is 6d9d6cbb98f249709a462915afc8f45e4db3c047, binary f7fdba26e8ec13bfd5a423f741f80c9e238abd74b39f8ea96e592f1b34c76a78. This product does not compile that operator. The DL01-W1-46 coordinator inspects the chain.",
        "Owned paths: tools/postgres-preview and evidence/ad13411e-d04e-4c91-8a20-49caf867aa23/DL01/postgres-product.",
        "Product command: postgres-preview run --config <operator file>. Attach command: postgres-preview consume --config <operator file>. Customer input is oo.postgres-preview.config/v1. Query input is oo.operator-preview.query/v1.",
        "Binaries, when the compile finished, are under linux/product.",
        "Loop outputs, when the databases ran, are under linux/runner/out.",
        "Index: linux/audit/savepoint-boundary.json. The prior 091fa109 csv is linux/failed-attempt-c2cbba992/source.csv.",
        "Source passwords stayed in /tmp/oo-dl01w1-46-secrets and are not in this packet.",
        f"No source, parent, or DL01 acceptance is claimed. author_complete is {str(outcome == 'author_complete').lower()}. Outcome is {outcome}.",
        "",
        f"This run: catalog request protocol {catalog_request.get('protocol') or 'absent'}, manifest protocol {(catalog_request.get('manifest') or {}).get('protocol') or 'absent'}, engine {catalog_engine or 'absent'}, owner {catalog_request.get('owner') or 'absent'}. Catalog stdout protocol {catalog_stdout.get('protocol') or 'absent'}, engine {catalog_stdout.get('engine') or 'absent'}. stderr {catalog_stderr.strip() or 'absent'}. Run token is {token or 'absent'}. model_called is {run.get('model_called')}. Sealed outcome {sealed_result.get('outcome') or 'absent'}, pack {sealed_result.get('pack') or 'absent'}, dependency_sha {sealed_result.get('dependency_sha') or 'absent'}. Unknown-price outcome {unknown_result.get('outcome') or 'absent'}, pack {unknown_result.get('pack') or 'absent'}. consumer-wait token {consumer_wait.get('token') or 'absent'}, called {consumer_wait.get('called')}. Bounded-read capture: hubs {hubs_out or 'absent'}; shipments {ship_out or 'absent'}; tracking_events {events_out or 'absent'}. import-pg state is {import_doc.get('state')} seal run_id {import_doc.get('run_id')}. Audit basis {audit_zero.get('basis')}, proven {audit_zero.get('proven')}, reader statements outside the allow-list {reader_bad_text}. Owned application_name oo-dl01w1-46 has {sum(boundary['preview_verbs'].values()) if boundary else 0} reader statements ({verb_text(boundary['preview_verbs']) if boundary else 'absent'}) and {sum(outside_allowlist(boundary['preview_verbs']).values()) if boundary else 'absent'} outside the original allow-list. Attempt logs and the failed-attempt archives stay.",
        "",
        (
            "Producer application oo-pg-source on this csv: "
            f"SAVEPOINT {boundary['producer_verbs'].get('SAVEPOINT', 0) if boundary else 0}, "
            f"RELEASE {boundary['producer_verbs'].get('RELEASE', 0) if boundary else 0}. "
            f"Pairs {len(boundary['pairs']) if boundary else 0}. "
            "The consumed producer is 4055237dfeb6df672388419dd3889a6fcaa4d356. "
            "read_probe is not on the bounded-read argv. "
            "The 8ecabc22 SAVEPOINT oo_stream_lock log stays in linux/failed-attempt-c2cbba992/source.csv."
        ),
    ]
    handoff.extend(pair_lines)
    handoff.extend([
        "W46 calls oo-source-admission-pg-budget --phase bounded-read from chain.rs produce_one. The allow-list in tools/postgres-preview/src/audit_local.rs stays SELECT SHOW SET BEGIN COMMIT ROLLBACK START EXPLAIN FETCH CLOSE DECLARE WITH. SAVEPOINT and RELEASE were not added. Rows were not deleted from the csv.",
        "",
        f"Catalog command is catalog-import-pg. Request protocol {catalog_request.get('protocol') or 'absent'}. Manifest protocol {(catalog_request.get('manifest') or {}).get('protocol') or 'absent'}. Engine {catalog_engine or 'absent'}. Owner {catalog_request.get('owner') or 'absent'}. Source id {catalog_request.get('source_id') or 'absent'}. Manifest run_id {(catalog_request.get('manifest') or {}).get('run_id') or 'absent'}. Stdout protocol {catalog_stdout.get('protocol') or 'absent'}. Status {stage_text('catalog-import.status') or 'absent'}. identity_hash {sealed_result.get('identity_hash') or 'absent'} is the catalog version on the effective document. Pipeline pin 5d5547882bdb20df2ac96db6c5727c823667fa1d. Semantics overlay f395b0015711920ed0fe66a6c7ce1b928a48635f. Core bpchar commit 29aae36b58d61c67f001893cd38aac28ec0f959f is a descendant of that overlay and was not required for this uuid catalog. The pinned installer applies sealed catalog through 0006. This product then applies that pin's 0007_sealed_import_owner.sql on the target superuser. pg-builder sealed calls compile_sealed_import_catalog, then the model, then compile_reviewed_sealed_import and compile_role_pack. The source string pg_builder_dependency_sha_source in reuse-provenance.txt is the stamp in pg-builder source, not a second compiler pin. capture_pg_catalog and oo.source.pgsql/v1 are not the catalog input. Historical catalog-import did not call postgresql_retag. import-native arrow-reseal reason is {arrow_reason or 'absent'}.",
        "",
        (
            f"consumer-wait protocol {consumer_wait.get('protocol') or 'absent'}, called {consumer_wait.get('called')}, token {consumer_wait.get('token') or 'absent'}, reason {consumer_wait.get('reason') or 'absent'}. "
            + (
                attach_gap(attach)
                if attach.get("called") is True
                else "The consume result is absent. The typed consumer runs only after the source is closed."
            )
        ),
        (
            "consume-all is the three-table typed consume on this capture. "
            f"complete {table_consume['complete']}, exits {table_consume['query_exits']}, "
            f"oracle cells {table_consume['cells']}, mismatches {table_consume['mismatch_count']}, "
            f"identity_hash {table_consume['identity_hash'] or 'absent'}. "
            "Pack rows: " + "; ".join(table_consume["pack_rows"]) + ". "
            f"preview sha256 {preview_sha}. "
            "The single-table consume command was not repeated after the role replacement. "
            "linux/runner/out/run/consumer-role-replay/result.json is the replay of oo_r_reader with the three-table grants left in place."
        ),
        customer["summary"],
        "The harness CancelToken cancel, deadline, oracle, and the same-run source audit are the owned reader path. They are not the operator consumer.",
        f"Containers at delivery: source {source_state}, target {target_state}, tools {tools_state}. The loop stops the source after the csv audit and leaves the target running.",
        "Evidence owner uuid is ad13411e-d04e-4c91-8a20-49caf867aa23. Current CLI uuid is c01c0d05-25f9-490d-8057-65ac2a26c7f8. The prior capture is linux/preserved-390b2fc4-audit.",
        "Import grants oo_pipeline_runtime to the target login WITH INHERIT TRUE, SET FALSE. That inheritance is schema USAGE plus EXECUTE on lookup_physical_mapping and register_physical_mapping, and not SELECT on _oo.physical_mappings. provision-mapping-reader refuses that partial set. After import, consume keeps the membership and, when inherit_option is true, runs GRANT oo_pipeline_runtime TO the login WITH INHERIT FALSE, SET FALSE. It does not grant the mapping privileges itself. The official provision command then sees an absent or exact set. The recorded word is linux/runner/out/run/consumer-attach/pipeline-inherit.txt. Column role oo_r_reader stays the install-column-acl role. Target stays running for review. Source stays exited.",
        "The pinned native probe catalog carries no comparison operators, so a predicate with = or IN is guard exit 6 before a session. The admitted typed statement is SELECT hub_id, hub_token, capacity_grams, note IS NULL FROM public.hubs ORDER BY hub_id LIMIT 7. Hubs 1 through 7 are the first seven hub_id values. The oracle requires both hub uuid literals and the int8 minimum -9223372036854775808. native_wal begin accepts only a header-only seg-00000001.wal. query, cancel, and reopen each receive a new audit directory under consumer-attach. The sealed audit directory is not rewritten.",
    ])
    (EVIDENCE / "HANDOFF.md").write_text("\n".join(handoff) + "\n")
    receipt = {
        "protocol": "oo.postgres-preview.receipt/v1",
        "task_id": "DL01-W1-46",
        "source_id": "oo_dl01w1_46",
        "outcome": outcome,
        "source_sha": source_sha,
        "evidence_sha": evidence_sha,
        "producer_commit": "4055237dfeb6df672388419dd3889a6fcaa4d356",
        "producer_evidence": "a3cd94c2c4337528342c8ece8d3ec142e2d8b2fc",
        "prior_gate_read_commit": "6590cff4acc1cb265d0ed05d460b0582251cf360",
        "pipeline_commit": "5d5547882bdb20df2ac96db6c5727c823667fa1d",
        "semantics_commit": "f395b0015711920ed0fe66a6c7ce1b928a48635f",
        "semantics_evidence": "6fd28c439820ad9262e49ef5f2a119fd97e28590",
        "catalog_protocol": catalog_request.get("protocol"),
        "catalog_manifest_protocol": (catalog_request.get("manifest") or {}).get("protocol"),
        "sealed_protocol": catalog_stdout.get("protocol"),
        "sealed_outcome": sealed_result.get("outcome"),
        "sealed_pack": sealed_result.get("pack"),
        "identity_hash": sealed_result.get("identity_hash"),
        "unknown_outcome": unknown_result.get("outcome"),
        "consumer_wait": consumer_wait.get("token"),
        "consumer_commit": attach.get("consumer_commit"),
        "consumer_binary_sha256": attach.get("binary_sha256"),
        "consumer_called": attach.get("called"),
        "consumer_bind_split": attach.get("bind_split"),
        "consumer_project": attach.get("project"),
        "consumer_source_id": attach.get("source_id"),
        "consumer_describe_exit": attach.get("describe_exit"),
        "consumer_query_exit": attach.get("query_exit"),
        "consumer_hidden_exit": attach.get("hidden_exit"),
        "consumer_widen_exit": attach.get("widen_exit"),
        "consumer_cancel_exit": attach.get("cancel_exit"),
        "consumer_reopen_exit": attach.get("reopen_exit"),
        "consumer_source_open": attach.get("source_open"),
        "consumer_inflight_57014": attach.get("inflight_57014"),
        "consumer_model_called": attach.get("model_called"),
        "consumer_source_captured": attach.get("source_captured"),
        "pack_digest": sealed_result.get("pack_digest"),
        "rebind_project": rebind_project,
        "rebind_source_id": rebind_source,
        "rebind_outcome": rebind_result.get("outcome"),
        "rebind_pack": rebind_result.get("pack"),
        "rebind_pack_digest": rebind_result.get("pack_digest"),
        "rebind_identity_hash": rebind_result.get("identity_hash"),
        "rebind_dependency_sha": rebind_result.get("dependency_sha"),
        "rebind_model": rebind_result.get("model"),
        "arrow_reseal_reason": arrow_reason,
        "arrow_reseal_new_run_id": arrow_identity.get("new_run_id"),
        "arrow_reseal_prior_seal_run_id": arrow_identity.get("prior_seal_run_id"),
        "builder_compiler": sealed_result.get("compiler"),
        "dependency_sha": sealed_result.get("dependency_sha"),
        "model": sealed_result.get("model"),
        "config_digest": sealed_result.get("config_digest"),
        "reuse_provenance": provenance,
        "run_id": run.get("run_id"),
        "seal_run_id": import_doc.get("run_id"),
        "savepoint_boundary": "linux/audit/savepoint-boundary.json",
        "savepoint_owner": "DL01-W1-38" if boundary and boundary["pairs"] else None,
        "catalog_import_owner": "DL01-W1-06",
        "catalog_import_engine": catalog_engine,
        "catalog_import_stderr": catalog_stderr.strip(),
        "not_independent_acceptance": True,
        "evidence_owner_uuid": "ad13411e-d04e-4c91-8a20-49caf867aa23",
        "current_cli_uuid": "c01c0d05-25f9-490d-8057-65ac2a26c7f8",
        "preserved_prior_audit": "linux/preserved-390b2fc4-audit/source.csv",
        "sealed_project": sealed_project,
        "sealed_source_id": sealed_source,
        "builder_cache": sealed_result.get("cache"),
        "builder_401_http_status": auth_401.get("http_status"),
        "builder_401_pack": auth_401.get("pack"),
        "builder_401_model": auth_401.get("model"),
        "builder_timeout_reason": auth_timeout.get("reason"),
        "builder_timeout_http_status": auth_timeout.get("http_status"),
        "builder_timeout_pack": auth_timeout.get("pack"),
        "builder_timeout_model": auth_timeout.get("model"),
        "consumer_rollback_confirmed": attach.get("rollback_confirmed"),
        "consumer_install_exit": attach.get("install_exit"),
        "consumer_install_effect": attach.get("install_effect"),
        "consumer_install_commit": attach.get("install_commit"),
        "consumer_provision_exit": attach.get("provision_exit"),
        "consumer_provision_effect": attach.get("provision_effect"),
        "source_container": source_state,
        "target_container": target_state,
        "tools_container": tools_state,
        "table_consume_ok": table_consume["ok"],
        "table_oracle_cells": table_consume["cells"],
        "table_oracle_mismatches": table_consume["mismatch_count"],
        "table_query_exits": table_consume["query_exits"],
        "table_identity_hash": table_consume["identity_hash"],
        "table_config_digest": table_consume["config_digest"],
        "table_install_effect": table_consume["install_effect"],
        "table_partial_role": table_consume["partial_role"],
        "table_provision_effect": table_consume["provision_effect"],
        "preview_binary_sha256": preview_sha,
        "preview_compile_ok": preview_compile_ok,
        "customer_ok": customer["ok"],
        "customer_rows": customer["rows"],
        "customer_distinct_note_ids": customer.get("distinct_note_ids"),
        "customer_window_query_count": customer.get("window_queries"),
        "customer_window_write_attempts": customer.get("window_writes"),
        "customer_import_run_id": customer["run_id"],
        "customer_pack_digest": customer["pack_digest"],
        "customer_identity_hash": customer["identity_hash"],
        "customer_config_digest": customer["config_digest"],
        "customer_builder_cache": customer["cache"],
        "customer_builder_model": customer["model"],
        "customer_cache_hit": customer["hit_cache"],
        "customer_cache_model": customer["hit_model"],
        "customer_policy_role": customer["role"],
        "customer_visibility_role": customer["visibility_role"],
        "customer_install_effect": customer["install_effect"],
        "customer_install_role": customer["install_role"],
        "customer_physical_table": customer["physical_table"],
        "customer_replay_role": customer["replay_role"],
        "customer_hidden_exit": customer["hidden_exit"],
        "customer_object_exit": customer["object_exit"],
        "customer_widen_exit": customer["widen_exit"],
        "customer_cancel_exit": customer["cancel_exit"],
        "customer_inflight_57014": customer["inflight_57014"],
        "customer_rollback_confirmed": customer["rollback_confirmed"],
        "customer_reopen_exit": customer["reopen_exit"],
        "customer_install_called": customer["install_called"],
        "customer_grants": customer["grants"],
        "customer_oracle_cells": customer["oracle_cells"],
        "customer_oracle_mismatches": customer["oracle_mismatches"],
        "logistics_replay_role": customer["logistics_replay_role"],
        "logistics_replay_grants": customer["logistics_replay_grants"],
        "second_database_refused_status": customer["second_database_status"],
        "customer_arrow_sha256": customer["arrow_sha256"],
        "customer_gaps": customer["gaps"],
        "preview_capacity": preview_capacity,
        "rust_image": "docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5",
        "rustc_version": "1.94.0 (4a4ef493e 2026-03-02)",
    }
    (EVIDENCE / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    sums = []
    for path in sorted(EVIDENCE.rglob("*")):
        if path.is_file() and path.name != "SHA256SUMS":
            sums.append(f"{sha256_file(path)}  {path.relative_to(EVIDENCE).as_posix()}")
    (EVIDENCE / "SHA256SUMS").write_text("\n".join(sums) + "\n")
    print(outcome)


if __name__ == "__main__":
    sys.exit(0 if main() is None else 0)
