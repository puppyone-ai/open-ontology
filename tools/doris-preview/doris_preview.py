#!/usr/bin/env python3
"""Customer CLI for one Apache Doris source preview.

Secrets stay in files named by secret_ref. This process does not print them,
does not retag Doris as MySQL or PostgreSQL, and does not invent a sealed pack.
"""

import hashlib
import json
import os
import subprocess
import sys
import urllib.error
import urllib.request

PROTOCOL = "oo.doris.preview-request/v0"
SOURCE_ID = "dorispreview"


def main():
    if len(sys.argv) < 2:
        print(
            "usage: doris_preview.py run|admit|catalog-gap|refuse-object|import-local|detach|reopen|ask|close --config <file>",
            file=sys.stderr,
        )
        return 2
    command = sys.argv[1]
    config_path = flag("--config")
    if not config_path:
        print("config path required", file=sys.stderr)
        return 2
    config = json.loads(open(config_path, encoding="utf-8").read())
    if config.get("protocol") != PROTOCOL:
        print("config protocol", file=sys.stderr)
        return 2
    evidence = config["evidence"]
    os.makedirs(evidence, exist_ok=True)
    customer = customer_config(config)
    if customer is None:
        print("secret_ref and approved_objects are required", file=sys.stderr)
        return 2
    config["secret_ref"], config["approved_objects"] = customer
    commands = {
        "admit": admit,
        "catalog-gap": catalog_gap,
        "refuse-object": refuse_object,
        "import-local": import_local,
        "detach": detach,
        "reopen": reopen,
        "ask": ask,
        "close": close,
        "run": run_all,
    }
    if command not in commands:
        print("unknown command", file=sys.stderr)
        return 2
    return commands[command](config, evidence)


def flag(name):
    if name in sys.argv:
        index = sys.argv.index(name)
        if index + 1 < len(sys.argv):
            return sys.argv[index + 1]
    return None


def write(evidence, name, body):
    path = os.path.join(evidence, name)
    if isinstance(body, (dict, list)):
        text = json.dumps(body, indent=2) + "\n"
    else:
        text = body
    with open(path, "w", encoding="utf-8") as handle:
        handle.write(text)
    return path


def customer_config(config):
    secret = config.get("secret_ref")
    approved = config.get("approved_objects")
    if not isinstance(secret, str) or not secret.startswith("secret://") or any(ch.isspace() for ch in secret):
        return None
    if isinstance(approved, str):
        approved = [part.strip() for part in approved.split(",") if part.strip()]
    if not isinstance(approved, list) or not approved:
        return None
    if not all(isinstance(item, str) and item and " " not in item for item in approved):
        return None
    if len(approved) != len(set(approved)):
        return None
    return secret, approved


def refuse_object(config, evidence):
    name = config.get("object")
    approved = set(config["approved_objects"])
    if not isinstance(name, str) or not name or name in approved:
        print("object must be outside the approved set", file=sys.stderr)
        return 2
    write(evidence, "object-refusal.json", {
        "exit": 1,
        "object": name,
        "reason": "not_approved",
        "source_read": False,
    })
    return 1


def admit(config, evidence):
    digest = "unbuilt"
    binary = config.get("driver_bin")
    if binary and os.path.isfile(binary):
        digest = hashlib.sha256(open(binary, "rb").read()).hexdigest()
    report = {
        "protocol": "oo.source-admission.report/v1",
        "target_id": SOURCE_ID,
        "source_id": SOURCE_ID,
        "secret_ref": config["secret_ref"],
        "driver": {"kind": "native", "name": "mysql_async", "version": "0.36.1", "digest": digest},
        "compat_mode": "doris",
        "license_flags": ["Apache-2.0"],
        "permissions": [{"name": "SELECT", "status": "known", "granted": True}],
        "capabilities": [{
            "name": "BatchFull",
            "cursor": None,
            "feed": None,
            "cdc": None,
            "token_order": None,
            "budget_unit": None,
            "level": "experimental",
            "write_class": "R0",
            "consistency": "S0",
        }],
        "r1w": None,
        "capture_methods": [],
        "cost_model": {
            "unit": "Rows",
            "confidence": "unknown",
            "estimate": config.get(
                "row_cap_estimate",
                "observed reader row cap; Doris did not publish a billing unit",
            ),
        },
    }
    observed_rows = config.get("budgets", {}).get("max_rows")
    if not isinstance(observed_rows, int) or observed_rows <= 0:
        write(evidence, "admission-invocation.json", {
            "invoked": False,
            "reason": "observed row cap was not supplied",
            "source_acceptance": False,
        })
        return 1
    if digest == "unbuilt":
        write(evidence, "admission-invocation.json", {
            "invoked": False,
            "reason": "driver digest is not the built reader",
            "source_acceptance": False,
        })
        return 1
    source_config = {
        "protocol": "oo.source-admission.config/v1",
        "target_id": SOURCE_ID,
        "source_id": SOURCE_ID,
        "secret_ref": config["secret_ref"],
        "compat_mode": "doris",
        "permissions": [{"name": "SELECT", "status": "known", "granted": True}],
        "budget": {"unit": "Rows", "hard_limit": observed_rows, "confidence": "unknown"},
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
    unknown = json.loads(json.dumps(source_config))
    unknown_report = json.loads(json.dumps(report))
    unknown["budget"]["unit"] = "DorisScan"
    unknown_report["cost_model"]["unit"] = "DorisScan"
    write(evidence, "admission-report.json", report)
    write(evidence, "admission-config.json", source_config)
    write(evidence, "admission-unknown-unit-report.json", unknown_report)
    write(evidence, "admission-unknown-unit-config.json", unknown)
    binary = config.get("admission_bin")
    if not binary or not os.path.isfile(binary):
        write(evidence, "admission-invocation.json", {
            "invoked": False,
            "reason": "oo-source-admission binary was not built in this pass",
            "expected_blocks": ["COMPAT_MODE_UNKNOWN", "BUDGET_UNIT_UNKNOWN"],
            "source_acceptance": False,
        })
        return 0
    decisions = []
    for label, report_name, config_name in (
        ("doris-compat", "admission-report.json", "admission-config.json"),
        ("unknown-unit", "admission-unknown-unit-report.json", "admission-unknown-unit-config.json"),
    ):
        result = subprocess.run(
            [binary, "validate", "--report", os.path.join(evidence, report_name), "--config", os.path.join(evidence, config_name)],
            text=True, capture_output=True,
        )
        decision_path = os.path.join(evidence, f"admission-decision-{label}.json")
        with open(decision_path, "w", encoding="utf-8") as handle:
            handle.write(result.stdout)
        write(evidence, f"admission-{label}.exit", str(result.returncode) + "\n")
        write(evidence, f"admission-{label}.stderr", result.stderr)
        if label == "doris-compat":
            saw = result.returncode == 0 and '"admitted": true' in result.stdout and "COMPAT_MODE_UNKNOWN" not in result.stdout
        else:
            saw = result.returncode == 2 and "BUDGET_UNIT_UNKNOWN" in result.stdout
        decisions.append({"label": label, "exit": result.returncode, "saw": saw})
    write(evidence, "admission-invocation.json", {"invoked": True, "decisions": decisions, "source_acceptance": False})
    if not all(item["saw"] for item in decisions):
        return 1
    return 0


def catalog_gap(config, evidence):
    gap = {
        "invoked": False,
        "command": "pg-builder import-native-catalog",
        "request_protocol": "oo.pipeline.native-sealed-catalog/v1",
        "install_protocol": "oo.pipeline.install-native-sealed-catalog/v1",
        "engine_argument": False,
        "catalog_import_invoked": False,
        "reason": "Doris uses the native sealed catalog read. catalog-import is not called.",
        "source_acceptance": False,
        "approved_objects": config["approved_objects"],
    }
    write(evidence, "catalog-gap.json", gap)
    return 0


def import_local(config, evidence):
    oracle_path = config.get("oracle")
    decode_report = config.get("decode_report")
    if not oracle_path or not os.path.isfile(oracle_path):
        write(evidence, "import-ready.json", {"ready": False, "reason": "oracle missing"})
        return 1
    if not decode_report or not os.path.isfile(decode_report):
        write(evidence, "import-ready.json", {"ready": False, "reason": "decode report missing"})
        return 1
    report = json.loads(open(decode_report, encoding="utf-8").read())
    if not report.get("ok"):
        write(evidence, "import-ready.json", {"ready": False, "reason": "arrow did not match the oracle"})
        return 1
    image = config.get("postgres_image")
    if not image:
        write(evidence, "import-ready.json", {"ready": False, "reason": "postgres image not configured"})
        return 1
    # The load uses oracle strings only after Arrow equality. It is target storage, not a sealed import.
    oracle = json.loads(open(oracle_path, encoding="utf-8").read())
    sql = typed_sql(oracle)
    write(evidence, "import.sql", sql)
    started = start_pg(config, image, evidence)
    if started != 0:
        return started
    partial = partial_cleanup(config, evidence)
    loaded = psql(config, sql, evidence, "import")
    if loaded != 0:
        psql(config, "DROP SCHEMA IF EXISTS oo_doris CASCADE;", evidence, "import-cleanup")
        write(evidence, "import-ready.json", {"ready": False, "sealed_import": False, "partial": partial})
        return loaded
    profile = profile_sql()
    profiled = psql(config, profile, evidence, "profile")
    write(evidence, "import-ready.json", {
        "ready": profiled == 0,
        "sealed_import": False,
        "storage_label": "postgresql_target_storage",
        "engine_preserved": "apache-doris",
        "half_ready": False,
        "partial_cleanup": partial,
        "source_acceptance": False,
    })
    return profiled


def typed_sql(oracle):
    lines = ["CREATE SCHEMA IF NOT EXISTS oo_doris;"]
    for name, table in oracle["tables"].items():
        columns = []
        for column in table["columns"]:
            kind = column["kind"]
            if kind == "int64":
                pg = "bigint"
            elif kind == "utf8":
                pg = "text"
            elif kind == "date":
                pg = "date"
            elif kind == "decimal":
                pg = f"numeric({column['precision']},{column['scale']})"
            else:
                raise SystemExit(f"unsupported kind {kind}")
            null = "" if kind == "int64" else ""
            columns.append(f"{column['name']} {pg}{null}")
        lines.append(f"DROP TABLE IF EXISTS oo_doris.{name};")
        lines.append(f"CREATE TABLE oo_doris.{name} ({', '.join(columns)});")
        for row in table["rows"]:
            values = []
            for cell in row:
                if cell is None:
                    values.append("NULL")
                elif isinstance(cell, int):
                    values.append(str(cell))
                else:
                    values.append("'" + str(cell).replace("'", "''") + "'")
            lines.append(f"INSERT INTO oo_doris.{name} VALUES ({', '.join(values)});")
    return "\n".join(lines) + "\n"


def profile_sql():
    return """
SELECT 'dup_events' AS table_name, COUNT(*)::text AS n, COALESCE(SUM(qty), 0)::text AS qty FROM oo_doris.dup_events
UNION ALL
SELECT 'agg_qty', COUNT(*)::text, COALESCE(SUM(qty), 0)::text FROM oo_doris.agg_qty
UNION ALL
SELECT 'uniq_acct', COUNT(*)::text, COALESCE(SUM(balance), 0)::text FROM oo_doris.uniq_acct
UNION ALL
SELECT 'nz_obs', COUNT(*) FILTER (WHERE qty IS NULL)::text, COUNT(*) FILTER (WHERE qty = 0)::text FROM oo_doris.nz_obs;
"""


def partial_cleanup(config, evidence):
    sql = """
CREATE SCHEMA IF NOT EXISTS oo_scratch;
CREATE TABLE oo_scratch.narrow (n numeric(10,2));
INSERT INTO oo_scratch.narrow VALUES ('12345678901234567890.123456789012345678');
"""
    failed = psql(config, sql, evidence, "partial-attempt")
    psql(config, "DROP SCHEMA IF EXISTS oo_scratch CASCADE;", evidence, "partial-cleanup")
    remains = psql(config, "SELECT COUNT(*) FROM information_schema.schemata WHERE schema_name = 'oo_scratch';", evidence, "partial-remain")
    count = None
    remain_path = os.path.join(evidence, "partial-remain.stdout")
    if os.path.isfile(remain_path):
        for line in open(remain_path, encoding="utf-8"):
            text = line.strip()
            if text.isdigit():
                count = int(text)
                break
    return {
        "insert_exit": failed,
        "overflow_rejected": failed != 0,
        "schema_count": count,
        "schema_dropped": remains == 0 and count == 0,
        "ready": False,
    }


def start_pg(config, image, evidence):
    import fcntl
    name = config.get("pg_container", "oo-dl01w1-43-pg")
    lock = open("/tmp/oo-dl01-wave1-resources.lock", "a")
    fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
    launch = "absent"
    try:
        if subprocess.run(["docker", "container", "inspect", name], capture_output=True).returncode == 0:
            write(evidence, "pg-container.txt", "already present\n")
            launch = "present"
        else:
            launch = start_pg_locked(config, image, evidence, name)
    finally:
        fcntl.flock(lock.fileno(), fcntl.LOCK_UN)
    if launch in ("present", "started"):
        return wait_pg(name, evidence)
    return launch if isinstance(launch, int) else 1


def start_pg_locked(config, image, evidence, name):
    ids = [line for line in subprocess.run(["docker", "ps", "-q"], text=True, capture_output=True).stdout.split() if line]
    cpu = 0.0
    mem = 0
    for container in ids:
        nano, memory = subprocess.run(
            ["docker", "inspect", "-f", "{{.HostConfig.NanoCpus}} {{.HostConfig.Memory}}", container],
            text=True, capture_output=True,
        ).stdout.split()
        nano = int(nano)
        memory = int(memory)
        cpu += 1.0 if nano == 0 else nano / 1_000_000_000
        mem += memory
    total = int(subprocess.run(["docker", "info", "-f", "{{.MemTotal}}"], text=True, capture_output=True).stdout.strip() or "0")
    new_mem = 1024 ** 3
    builders = subprocess.run(
        ["docker", "ps", "-q", "--filter", "label=oo.kind=rust-builder"],
        text=True, capture_output=True,
    ).stdout.split()
    own = subprocess.run(
        ["docker", "ps", "-q", "--filter", "label=oo.owner=DL01-W1-43", "--filter", "label=oo.kind=rust-builder"],
        text=True, capture_output=True,
    ).stdout.split()
    host_rust = 0
    for line in subprocess.run(["ps", "-ax", "-o", "command="], text=True, capture_output=True).stdout.splitlines():
        base = os.path.basename(line.strip().split()[0]) if line.strip() else ""
        if base in {"rustc", "cargo", "rustdoc"}:
            host_rust += 1
    snap = {
        "cpu_after": cpu + 1.0,
        "headroom_bytes": total - mem - new_mem,
        "global_rust": len(builders) + host_rust,
        "own_rust_builders": len(own),
    }
    write(evidence, "pg-capacity.json", snap)
    if snap["own_rust_builders"] or snap["global_rust"] >= 10 or snap["cpu_after"] > 16 or snap["headroom_bytes"] <= 10 * 1024 ** 3:
        write(evidence, "pg-pending.json", {"status": "pending-capacity", **snap})
        return 3
    result = subprocess.run([
        "docker", "run", "-d", "--name", name,
        "--platform", "linux/arm64", "--pull", "never",
        "--memory", "1g", "--memory-swap", "1g", "--cpus", "1",
        "--label", "oo.project=open-ontology",
        "--label", "oo.owner=DL01-W1-43",
        "--label", "oo.session=5106fed0-5bcd-4aaf-8df7-838c95a7d7e2",
        "--label", "oo.prefix=oo-dl01w1-43",
        "--label", "oo.kind=postgres-target",
        "-e", "POSTGRES_HOST_AUTH_METHOD=trust",
        image,
    ], text=True, capture_output=True)
    write(evidence, "pg-run.stdout", result.stdout)
    write(evidence, "pg-run.stderr", result.stderr)
    write(evidence, "pg-run.exit", str(result.returncode) + "\n")
    if result.returncode != 0:
        return result.returncode
    return "started"


def wait_pg(name, evidence):
    import time
    deadline = time.time() + 60
    last = ""
    while time.time() < deadline:
        result = subprocess.run(
            ["docker", "exec", name, "pg_isready", "-U", "postgres"],
            text=True, capture_output=True,
        )
        last = (result.stdout + result.stderr).strip()
        if result.returncode == 0:
            write(evidence, "pg-ready.txt", last + "\n")
            return 0
        state = subprocess.run(
            ["docker", "inspect", "-f", "{{.State.Status}}", name],
            text=True, capture_output=True,
        ).stdout.strip()
        if state not in ("running", "created", ""):
            write(evidence, "pg-ready.txt", state + "\n" + last + "\n")
            return 1
        time.sleep(2)
    write(evidence, "pg-ready.txt", "timeout\n" + last + "\n")
    return 1


def psql(config, sql, evidence, name):
    container = config.get("pg_container", "oo-dl01w1-43-pg")
    result = subprocess.run(
        ["docker", "exec", "-i", container, "psql", "-U", "postgres", "-v", "ON_ERROR_STOP=1", "-f", "-"],
        input=sql, text=True, capture_output=True,
    )
    write(evidence, f"{name}.stdout", result.stdout)
    write(evidence, f"{name}.stderr", result.stderr)
    write(evidence, f"{name}.exit", str(result.returncode) + "\n")
    return result.returncode


def detach(config, evidence):
    profiled = psql(config, profile_sql(), evidence, "detach-profile")
    write(evidence, "detach.json", {
        "source_attached": False,
        "storage_label": "postgresql_target_storage",
        "profile_exit": profiled,
        "source_acceptance": False,
    })
    return profiled


def reopen(config, evidence):
    profiled = psql(config, profile_sql(), evidence, "reopen-profile")
    write(evidence, "reopen.json", {
        "source_attached": False,
        "repeat": True,
        "profile_exit": profiled,
        "source_acceptance": False,
    })
    return profiled


def ask(config, evidence):
    endpoint = config.get("model_endpoint", "http://127.0.0.1:8317/v1/chat/completions")
    token_path = config.get("model_token_file", "/tmp/oo-customer-demo-20261007/llm-token")
    token_present = os.path.isfile(token_path)
    provider = provider_once(endpoint, token_path if token_present else None)
    write(evidence, "model-provider-gate.json", {
        "protocol": "oo.doris.model-provider-gate/v1",
        "endpoint": endpoint,
        "model": "grok-4.7",
        "token_file_present": token_present,
        "token_printed": False,
        "requests": 1 if token_present else 0,
        "provider": provider,
        "http_200_is_builder_proof": False,
        "builder_invoked": False,
        "role_pack": False,
        "reason": "one approved-token request; no sealed role pack and no retry",
        "unknown_cost": True,
        "source_acceptance": False,
    })
    return 0


def provider_once(endpoint, token_path):
    if not token_path:
        return {"error": "token_file_missing", "builder_proof": False}
    token = open(token_path, encoding="utf-8").read().strip()
    body = json.dumps({
        "model": "grok-4.7",
        "messages": [{"role": "user", "content": "ping"}],
        "max_tokens": 1,
    }).encode("utf-8")
    request = urllib.request.Request(
        endpoint,
        data=body,
        headers={"Authorization": "Bearer " + token, "Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=20) as response:
            raw = response.read(400)
            return {"status": response.status, "body": redact(raw.decode("utf-8", "replace"), token), "builder_proof": False}
    except urllib.error.HTTPError as err:
        raw = err.read(400).decode("utf-8", "replace")
        return {"status": err.code, "body": redact(raw, token), "builder_proof": False}
    except Exception as err:
        return {"error": type(err).__name__, "builder_proof": False}


def redact(text, token):
    if token:
        text = text.replace(token, "[redacted]")
    return text[:240]


def http_status(endpoint, headers):
    request = urllib.request.Request(endpoint, data=b"{}", headers={**headers, "Content-Type": "application/json"}, method="POST")
    try:
        with urllib.request.urlopen(request, timeout=5) as response:
            return {"status": response.status, "builder_proof": False}
    except urllib.error.HTTPError as err:
        return {"status": err.code, "builder_proof": False}
    except Exception as err:
        return {"error": type(err).__name__, "builder_proof": False}


def timeout_probe():
    request = urllib.request.Request("http://192.0.2.1:8317/", method="GET")
    try:
        with urllib.request.urlopen(request, timeout=2) as response:
            return {"status": response.status, "timed_out": False}
    except Exception as err:
        return {"error": type(err).__name__, "timed_out": True}


def close(config, evidence):
    container = config.get("pg_container", "oo-dl01w1-43-pg")
    psql(config, "DROP SCHEMA IF EXISTS oo_doris CASCADE; DROP SCHEMA IF EXISTS oo_scratch CASCADE;", evidence, "close-sql")
    stopped = subprocess.run(["docker", "stop", container], text=True, capture_output=True)
    removed = subprocess.run(["docker", "rm", container], text=True, capture_output=True)
    write(evidence, "close.json", {
        "stopped": stopped.returncode,
        "removed": removed.returncode,
        "source_acceptance": False,
    })
    return 0


def run_all(config, evidence):
    steps = {
        "admit": admit(config, evidence),
        "catalog_gap": catalog_gap(config, evidence),
        "ask": ask(config, evidence),
    }
    if config.get("import_local"):
        steps["import_local"] = import_local(config, evidence)
        if steps["import_local"] == 0:
            steps["detach"] = detach(config, evidence)
            steps["reopen"] = reopen(config, evidence)
    write(evidence, "HANDOFF.json", {
        "task": "DL01-W1-43",
        "status": "author_complete_review_deferred",
        "source_acceptance": False,
        "grade": None,
        "parent_signed": False,
        "steps": steps,
        "consolidated_review": "required after the fixed author chain",
    })
    return 0 if all(code == 0 for code in steps.values()) else 1


if __name__ == "__main__":
    sys.exit(main())
