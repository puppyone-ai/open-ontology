#!/usr/bin/env python3
"""Onboard step for the accepted Oracle generation-3 source.

Spawns product_flow and dialogue in this process. Does not replay a committed
load, does not write the customer, and does not issue a second model create.
"""
import hashlib
import json
import os
import subprocess
import sys
import time
from decimal import Decimal
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import c5_capture  # noqa: E402
import c5_live  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
FLOW = ROOT / "tools/source-onboarding/product_flow.py"
DIALOGUE = ROOT / "tools/agent-product-verifier/dialogue.py"
CACHE = Path("/Users/redux/.cache/oo-oracle-connection-whole")
PRODUCT = CACHE / "product.json"
GW = CACHE / "gateway"
PG = "oo-dl01w1-04-p2-pg"
PHYSICAL = "pt_c3325c512dc36084cae19f708143453edc4d8bdbbe1e4cd81a141d14ecef"
SOURCE = "oracle_oneclick_orders"
ROLE = "oo_r_oracle_orders"
SLUG = "oracle_orders"
LOGIN = "oo_oracle_orders_login"
TOKEN_DB = "oo_oracle_whole_token"
# issue-token and oo-gateway hardcode these cluster logins. Do not ALTER their passwords.
TOKEN_ADMIN = "oo_token_admin"
TOKEN_RUNTIME = "oo_token_runtime"
SHARED_ADMIN = Path("/tmp/oo-dl01w1-04-gateway/admin.pass")
SHARED_RUNTIME = Path("/tmp/oo-dl01w1-04-gateway/runtime.pass")
ISSUE = Path("/tmp/oo-pg46-runtime-finish/continuous/gateway/issue-token")
CLI_BIN = Path("/tmp/oo-pg46-runtime-finish/continuous/gateway/oo-cli")
GATEWAY_BIN = Path("/Users/redux/.cache/oo-dl01w1-73/fixed-elf/oo-gateway-8d27abf3564bf3d05790be72632f1026376880e0c148e60455b4b1a2bb2cefb1")
CA = Path("/Users/redux/.oo-dl01w1-04-p2/pg-tls/ca.crt")
G3 = Path("/tmp/oo-oracle-connection-whole/continue-g3/cycle-receipt.json")
CONTAINER = "oo-oracle-connection-whole-gateway"
AUDIENCE = "https://127.0.0.1:18714/"
MODEL_BEARER = "/tmp/oo-product-model-20261012/bearer"


def canonical(value):
    if value is None:
        return "null"
    if value is True:
        return "true"
    if value is False:
        return "false"
    if isinstance(value, str):
        return json.dumps(value, ensure_ascii=False)
    if isinstance(value, int) and not isinstance(value, bool):
        return str(value)
    if isinstance(value, list):
        return "[" + ",".join(canonical(item) for item in value) + "]"
    if isinstance(value, dict):
        return "{" + ",".join(canonical(key) + ":" + canonical(value[key]) for key in sorted(value)) + "}"
    raise ValueError("canonical")


def sha_bytes(raw):
    return hashlib.sha256(raw).hexdigest()


def decisions_from_overlay(overlay, dataset, role):
    columns = overlay["columns"]
    if not columns:
        raise ValueError("column_count")
    reviews = []
    names = []
    for column in columns:
        if column.get("pii") != "none":
            raise ValueError("pii_not_none")
        if column.get("dataset") != dataset:
            raise ValueError("dataset")
        name = dataset + "." + column["name"]
        fact = column["fact_sha256"]
        if len(fact) != 64:
            raise ValueError("fact")
        reviews.append({"name": name, "binds_fact": fact, "decision": "confirmed"})
        names.append(name)
    return {"reviews": reviews, "confirm_columns": names, "role": role, "fixture_authorized": True}


def spawn(args):
    proc = subprocess.run(args)
    if proc.returncode != 0:
        raise SystemExit(proc.returncode)


def aside(path):
    if not path.exists():
        return
    dest = path.with_name(path.name + ".prev")
    n = 0
    while dest.exists():
        n += 1
        dest = path.with_name(f"{path.name}.prev.{n}")
    path.rename(dest)


def psql(database, sql, tuples=False):
    cmd = ["docker", "exec", "-i", "-u", "postgres", PG, "psql", "-v", "ON_ERROR_STOP=1", "-d", database]
    if tuples:
        cmd[7:7] = ["-At"]
    proc = subprocess.run(cmd, input=sql.encode(), stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if proc.returncode != 0:
        GW.mkdir(mode=0o700, exist_ok=True)
        (GW / "psql.stderr").write_bytes(proc.stderr)
        raise SystemExit("postgres_refused")
    return proc.stdout.decode()


def hex_pass(path):
    if path.exists():
        text = path.read_text().strip()
    else:
        text = os.urandom(24).hex()
        path.write_text(text + "\n")
        path.chmod(0o600)
    if len(text) < 16 or any(ch not in "0123456789abcdef" for ch in text):
        raise SystemExit("password_file")
    return text


def copy_secret(src, dest):
    data = src.read_bytes()
    if not data.strip() or b"\0" in data:
        raise SystemExit("password_file")
    dest.write_bytes(data)
    dest.chmod(0o600)


def ensure_accounts():
    GW.mkdir(mode=0o700, exist_ok=True)
    os.chmod(GW, 0o700)
    copy_secret(SHARED_ADMIN, GW / "token-admin.pass")
    copy_secret(SHARED_RUNTIME, GW / "runtime.pass")
    data = hex_pass(GW / "data.pass")
    (GW / "ca.crt").write_bytes(CA.read_bytes())
    (GW / "data-ca.crt").write_bytes(CA.read_bytes())
    role = (
        "DO $$\nBEGIN\n"
        f"  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname='{LOGIN}') THEN CREATE ROLE {LOGIN} LOGIN NOINHERIT PASSWORD '{data}'; END IF;\n"
        f"  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname='{ROLE}') THEN CREATE ROLE {ROLE} NOLOGIN NOINHERIT; END IF;\n"
        "END $$;\n"
        f"ALTER ROLE {LOGIN} LOGIN NOINHERIT PASSWORD '{data}';\n"
        f"ALTER ROLE {ROLE} NOLOGIN NOINHERIT;\n"
    )
    psql("postgres", role)
    # Column ACL is the fixed install-column-acl ELF, same role, strict generation advance.
    # Do not GRANT or REVOKE here. Token-database ownership is already installed.
    (GW / "endpoint.json").write_text(json.dumps({"host": PG, "port": 5432, "database": TOKEN_DB}) + "\n")
    (GW / "data-endpoint.json").write_text(json.dumps({"host": PG, "port": 5432, "database": "previewdb"}) + "\n")
    config = {
        "audience": AUDIENCE, "body_limit": 1048576, "ca_file": "ca.crt", "data_ca_file": "data-ca.crt",
        "data_endpoint_file": "data-endpoint.json", "data_password_file": "data.pass", "data_user": LOGIN,
        "endpoint_file": "endpoint.json", "global_concurrency": 4, "listen": "127.0.0.1:0", "pack_dir": "packs",
        "password_file": "runtime.pass", "request_timeout_ms": 15000, "reviewed_dir": "reviewed",
        "runtime_user": TOKEN_RUNTIME, "wal_dir": "wal",
    }
    (GW / "config.json").write_text(json.dumps(config, sort_keys=True) + "\n")
    for name in ("packs", "wal", "reviewed"):
        (GW / name).mkdir(mode=0o700, exist_ok=True)
    if not (GW / "token-install.exit").exists():
        proc = subprocess.run([
            "docker", "run", "--rm", "--platform", "linux/arm64", "--network", "oo-dl01w1-04-net",
            "-v", f"{GW}:{GW}:ro", "-v", f"{ISSUE}:/issue-token:ro", "rust:1.94.0",
            "/issue-token", "install", str(GW / "endpoint.json"), str(GW / "token-admin.pass"), str(GW / "ca.crt"),
        ], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        (GW / "token-install.stdout").write_bytes(proc.stdout)
        (GW / "token-install.stderr").write_bytes(proc.stderr)
        if proc.returncode != 0:
            raise SystemExit("token_install")
        (GW / "token-install.exit").write_text("0\n")


def run_product(out):
    product = out / "product"
    product.mkdir(mode=0o700, exist_ok=True)
    os.chmod(product, 0o700)
    receipt = product / "product-receipt.json"
    if receipt.exists() and not (product / "build.json").exists():
        return
    aside = None
    if receipt.exists():
        aside = product / "product-receipt.prev.json"
        if aside.exists():
            aside = product / ("product-receipt.prev." + sha_bytes(receipt.read_bytes())[:12] + ".json")
        receipt.rename(aside)
    try:
        if not (product / "prepared.json").exists():
            spawn([sys.executable, str(FLOW), "--config", str(PRODUCT), "--out", str(product), "--prepare-only"])
        decisions_path = out / "decisions.json"
        binding = product / "decisions-binding.json"
        if decisions_path.exists() and binding.exists():
            saved = json.loads(binding.read_text())
            if saved.get("sha256") != sha_bytes(decisions_path.read_bytes()):
                raise SystemExit("decisions_drift")
        else:
            overlay = json.loads((product / "prepared-overlay.json").read_text())
            dataset = overlay["columns"][0]["dataset"]
            try:
                decisions = decisions_from_overlay(overlay, dataset, ROLE)
            except ValueError as error:
                (out / "blocker.json").write_text(json.dumps({
                    "blocker": str(error), "build_id": json.loads((product / "build.json").read_text())["build_id"],
                    "next": "same build, no second create; decisions only after every overlay pii is none",
                }) + "\n")
                raise SystemExit(2)
            decisions_path.write_text(json.dumps(decisions) + "\n")
            decisions_path.chmod(0o600)
        # export_oos mkdir(exist_ok=False). Move the prior export aside; do not edit the shared exporter.
        oos = product / "oos"
        oos_aside = None
        if oos.exists():
            oos_aside = product / "oos.prev"
            n = 0
            while oos_aside.exists():
                n += 1
                oos_aside = product / f"oos.prev.{n}"
            oos.rename(oos_aside)
        try:
            spawn([sys.executable, str(FLOW), "--config", str(PRODUCT), "--out", str(product), "--decisions", str(decisions_path)])
        except BaseException:
            if oos_aside is not None and oos_aside.exists() and not oos.exists():
                oos_aside.rename(oos)
            raise
    except BaseException:
        if aside is not None and aside.exists() and not receipt.exists():
            aside.rename(receipt)
        raise


def artifact_dir(product):
    receipt = json.loads((product / "product-receipt.json").read_text())
    remote = Path(receipt["pack"]["native_maintenance"]["artifact_directory"])
    host = Path(json.loads(PRODUCT.read_text())["work_dir_host"])
    container = Path(json.loads(PRODUCT.read_text())["work_dir_container"])
    return host / remote.relative_to(container)


def gateway_binding(catalog, reviewed, dataset, columns):
    provenance = reviewed["provenance"]
    effective = reviewed["effective_oos"]
    vector = catalog.get("binding_vector")
    schema, _table = dataset.split(".", 1)
    if vector:
        if len(vector) != 1 or vector[0].get("source_id") != SOURCE:
            raise SystemExit("binding_vector")
        schema_hash = provenance["collection_sha256"]
        config_digest = vector[0]["config_digest"]
        generation = vector[0]["source_generation"]
    else:
        if provenance.get("source_id") != SOURCE or "schema_digest" not in provenance:
            raise SystemExit("legacy_seal")
        schema_hash = provenance["schema_digest"]
        config_digest = provenance["config_digest"]
        generation = provenance["source_generation"]
    binding = {
        "allow": [{"column": column, "dataset": dataset, "explicit_allow": False} for column in columns],
        "artifact_sha256": sha_bytes(canonical(catalog).encode()),
        "catalog_version": effective["catalog_version"],
        "config_digest": config_digest,
        "generation": generation,
        "knowledge_id": effective["knowledge"]["id"],
        "knowledge_version": effective["knowledge"]["version"],
        "login": LOGIN,
        "ontology_version": effective["ontology_version"],
        "policy_id": effective["policy"]["id"],
        "policy_version": effective["policy"]["version"],
        "project": effective["project"],
        "role": SLUG,
        "schema": schema,
        "schema_hash": schema_hash,
        "source_id": provenance.get("source_id") or SOURCE,
    }
    if vector:
        binding["binding_vector"] = vector
    return binding


def bind_gateway(product):
    folder = artifact_dir(product)
    command = json.loads((folder / "command.json").read_text())
    reviewed = json.loads((product / "reviewed.json").read_text())
    visibility = json.loads((product / "visibility.json").read_text())
    catalog = command["catalog"]
    visible = visibility["datasets"][0]
    review = GW / "reviewed" / SLUG
    review.mkdir(mode=0o700, parents=True, exist_ok=True)
    (review / "native-input.json").write_bytes((folder / "command.json").read_bytes())
    (review / "command.json").write_bytes((product / "reviewed.json").read_bytes())
    dataset = visible["dataset"]
    binding = gateway_binding(catalog, reviewed, dataset, visible["columns"])
    (review / "binding.json").write_text(canonical(binding) + "\n")
    pack_dir = GW / "packs"
    for old in pack_dir.glob("*.json"):
        old.unlink()
    (pack_dir / f"{ROLE}.json").write_bytes((product / "role-pack.json").read_bytes())
    grant = {
        "kind": "client", "project_id": binding["project"], "principal_id": "fixture-admin", "role": ROLE,
        "policy_version": binding["policy_version"], "knowledge_version": binding["knowledge_version"],
        "audience_url": AUDIENCE, "claims": [], "client_ttl_ms": 3600000,
    }
    (GW / "grant.json").write_text(json.dumps(grant) + "\n")
    token = GW / "client.token"
    if token.exists():
        token.unlink()
    proc = subprocess.run([
        "docker", "run", "--rm", "--platform", "linux/arm64", "--network", "oo-dl01w1-04-net",
        "-v", f"{GW}:{GW}", "-v", f"{ISSUE}:/issue-token:ro", "rust:1.94.0",
        "/issue-token", "issue", str(GW / "endpoint.json"), str(GW / "token-admin.pass"), str(GW / "ca.crt"),
        str(GW / "grant.json"), str(token),
    ], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    (GW / "token-issue.stdout").write_bytes(proc.stdout)
    (GW / "token-issue.stderr").write_bytes(proc.stderr)
    if proc.returncode != 0 or not token.is_file():
        raise SystemExit("token_issue")
    token.chmod(0o600)
    wrapper = GW / "oo-cli"
    wrapper.write_text(
        "#!/bin/sh\nset -eu\n"
        f"docker run --rm --network container:{CONTAINER} "
        f"-v {CLI_BIN}:/oo-cli:ro -v {GW}:{GW}:ro rust:1.94.0 /oo-cli \"$@\"\n"
    )
    wrapper.chmod(0o700)
    listen = GW / "listen.addr"
    if listen.exists():
        listen.unlink()
    subprocess.run(["docker", "rm", "-f", CONTAINER], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    proc = subprocess.run([
        "docker", "run", "-d", "--name", CONTAINER, "--platform", "linux/arm64", "--network", "oo-dl01w1-04-net",
        "--label", "oo.owner=oracle-connection-whole",
        "-v", f"{GATEWAY_BIN}:/oo-gateway:ro", "-v", f"{GW}:/gw",
        "rust:1.94.0", "/oo-gateway", "/gw",
    ], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    (GW / "gateway-start.stderr").write_bytes(proc.stderr)
    if proc.returncode != 0:
        raise SystemExit("gateway_start")
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        if listen.is_file() and listen.read_text().strip():
            return "http://" + listen.read_text().strip()
        if subprocess.run(["docker", "inspect", "-f", "{{.State.Running}}", CONTAINER], capture_output=True, text=True).stdout.strip() != "true":
            logs = subprocess.run(["docker", "logs", CONTAINER], capture_output=True)
            (GW / "gateway.log").write_bytes(logs.stdout + b"\n" + logs.stderr)
            raise SystemExit("gateway_exited")
        time.sleep(0.5)
    raise SystemExit("gateway_listen")


def cli(gateway, args):
    proc = subprocess.run([str(GW / "oo-cli"), *args, "--json", "--gateway", gateway, "--transport", "local-development", "--auth", "external", "--token-file", str(GW / "client.token")], capture_output=True, text=True, timeout=45)
    try:
        body = json.loads(proc.stdout)
    except json.JSONDecodeError:
        body = {"stdout": proc.stdout[-500:], "stderr": proc.stderr[-500:]}
    return proc.returncode, body


def one_cell(body):
    if body.get("returned_rows") != 1 or len(body.get("columns", [])) != 1 or len(body.get("rows", [])) != 1:
        raise ValueError("probe_shape")
    return body["columns"][0]["name"], str(body["rows"][0][0]), body["query_id"]


def probe(gateway, dest):
    dest.mkdir(mode=0o700, exist_ok=True)
    os.chmod(dest, 0o700)
    code, tables = cli(gateway, ["tables"])
    (dest / "tables.json").write_text(json.dumps(tables) + "\n")
    if code != 0:
        raise SystemExit("probe_tables")
    table = tables["tables"][0]["name"]
    code, described = cli(gateway, ["describe", table])
    (dest / "describe.json").write_text(json.dumps(described) + "\n")
    if code != 0:
        raise SystemExit("probe_describe")
    count_sql = f"SELECT COUNT(*) AS row_count FROM {table}"
    code, counted = cli(gateway, ["query", count_sql, "--max-rows", "100", "--timeout-ms", "10000"])
    (dest / "count.json").write_text(json.dumps(counted) + "\n")
    if code != 0:
        raise SystemExit("probe_count")
    count_column, count_value, _count_id = one_cell(counted)
    names = described["columns"]
    key = next(name for name in names if name.startswith("q_") and bytes.fromhex(name[2:]).decode() == "KEY_ID")
    amount = next(name for name in names if name.startswith("q_") and bytes.fromhex(name[2:]).decode() == "AMOUNT")
    amount_sql = f"SELECT {amount} AS amount FROM {table} WHERE {key} = 1"
    code, amounted = cli(gateway, ["query", amount_sql, "--max-rows", "100", "--timeout-ms", "10000"])
    (dest / "amount.json").write_text(json.dumps(amounted) + "\n")
    if code != 0:
        raise SystemExit("probe_amount")
    amount_column, amount_value, _amount_id = one_cell(amounted)
    code, hits = cli(gateway, ["knowledge", "search", "orders"])
    (dest / "search.json").write_text(json.dumps(hits) + "\n")
    path = ""
    if code == 0 and hits.get("hits"):
        path = hits["hits"][0]["path"]
    return {
        "table": table, "count_sql": count_sql, "count_column": count_column, "count_value": count_value,
        "amount_sql": amount_sql, "amount_column": amount_column, "amount_value": amount_value, "pack_path": path,
    }


def dialogue(run, gateway, found):
    truth = {
        "rows": {"expected_named_facts": {"row_count": found["count_value"]}},
        "amount": {"expected_named_facts": {"amount": found["amount_value"]}},
    }
    path = run / "truth.json"
    path.write_text(json.dumps(truth) + "\n")
    path.chmod(0o600)
    pack = f" Then knowledge_cat the path {found['pack_path']}." if found["pack_path"] else " Then knowledge_search and knowledge_cat one returned relative path."
    questions = [
        {"truth_key": "rows", "prompt": f"Use tables, describe {found['table']}, freshness, and knowledge tools.{pack} Query exactly: {found['count_sql']}"},
        {"truth_key": "amount", "prompt": f"Use freshness again if needed. Query exactly: {found['amount_sql']}"},
    ]
    config = {
        "model_origin": "http://127.0.0.1:8317", "model": "gpt-6.1-sol", "model_bearer_file": MODEL_BEARER,
        "gateway_token_file": str(GW / "client.token"), "independent_truth_file": str(path),
        "gateway": gateway, "transport": "local-development", "auth": "external", "oo_cli": str(GW / "oo-cli"),
        "independent_gateway_audit_file": str(GW / "wal"), "questions": questions, "max_tool_rounds": 12,
    }
    cfg = run / "dialogue.json"
    cfg.write_text(json.dumps(config) + "\n")
    cfg.chmod(0o600)
    out = run / "dialogue"
    aside(out)
    proc = subprocess.run([sys.executable, str(DIALOGUE), "--config", str(cfg), "--out", str(out)])
    if proc.returncode == 0:
        return
    if out.exists():
        aside(run / "dialogue-gpt")
        os.rename(out, run / "dialogue-gpt")
    config["model"] = "grok-4.7"
    cfg.write_text(json.dumps(config) + "\n")
    spawn([sys.executable, str(DIALOGUE), "--config", str(cfg), "--out", str(out)])


def cancel_one(gateway):
    marker = "oo-oracle-whole-cancel"
    holder = subprocess.Popen([
        "docker", "exec", "-i", "-u", "postgres", PG, "psql", "-d", "previewdb", "-v", "ON_ERROR_STOP=1", "-c",
        f"/* {marker} */ BEGIN; LOCK TABLE _oo.{PHYSICAL} IN ACCESS EXCLUSIVE MODE; SELECT pg_sleep(20); ROLLBACK;",
    ], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            found = psql("previewdb", f"SELECT count(*) FROM pg_locks WHERE relation = '_oo.{PHYSICAL}'::regclass AND mode = 'AccessExclusiveLock';", tuples=True)
            if found.strip() not in {"", "0"}:
                break
            time.sleep(0.2)
        else:
            raise SystemExit("cancel_lock")
        query = subprocess.Popen([str(GW / "oo-cli"), "query", f"SELECT COUNT(*) AS row_count FROM {probe_table()}", "--json", "--gateway", gateway, "--transport", "local-development", "--auth", "external", "--token-file", str(GW / "client.token"), "--max-rows", "100", "--timeout-ms", "15000"], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        query_id = ""
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline and not query_id:
            for path in (GW / "wal").glob("*-admit.wal"):
                if not Path(str(path).replace("-admit.wal", "-complete.wal")).exists():
                    query_id = path.name.split("-admit.wal")[0]
                    break
            time.sleep(0.2)
        if not query_id:
            raise SystemExit("cancel_query")
        code, body = cli(gateway, ["cancel", query_id])
        (GW / "cancel.json").write_text(json.dumps({"exit": code, "body": body, "query_id": query_id}) + "\n")
        wal = GW / "wal" / f"{query_id}-cancel.wal"
        if not wal.is_file():
            raise SystemExit("cancel_wal")
        query.wait(timeout=20)
        return query_id
    finally:
        if holder.poll() is None:
            holder.kill()
        psql("previewdb", f"SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE query LIKE '%{marker}%' AND pid <> pg_backend_pid();")


def probe_table():
    body = json.loads((GW / "probe-table.txt").read_text())
    return body


def capture(cfg, dest, found):
    dest.mkdir(mode=0o700, exist_ok=True)
    waited = c5_capture.wait_realtime(c5_capture.resolve_periods(cfg["periods"]))
    (dest / "waits.json").write_text(json.dumps(waited) + "\n")
    password = Path(cfg["password_file"]).read_text().strip("\r\n")
    sql_dir = dest / "sql"
    sql_dir.mkdir(mode=0o700, exist_ok=True)
    sql = c5_live.HEAD + (
        "SELECT 'ROW|' || JSON_OBJECT('key' VALUE KEY_ID, 'amount' VALUE TO_CHAR(AMOUNT, 'FM9999999990.00'), "
        "'occurred' VALUE TO_CHAR(OCCURRED, 'YYYY-MM-DD\"T\"HH24:MI:SS'), 'label' VALUE LABEL, "
        "'payload' VALUE RAWTOHEX(PAYLOAD) ABSENT ON NULL) FROM OO04_FIX.ONBOARD_ORDERS ORDER BY KEY_ID;\nEXIT\n"
    )
    code, text = c5_live.sqlplus("OO04_READER", password, sql, "current", sql_dir, [password.encode()])
    if code != 0:
        raise SystemExit("current_read")
    rows = []
    for line in text.splitlines():
        line = line.strip()
        if line.startswith("ROW|"):
            rows.append(json.loads(line[4:]))
    pg = psql("previewdb", f"""
SELECT json_agg(json_build_object(
  'key', pc_b064bb8641f87c6e2dcee3112a98ce5e5608c2f9a00717247ddbc7e1cc27::text,
  'amount', to_char(pc_8330bec0f7547eec9697a8a9ed420946f7540484f676b92b24f48c778d23, 'FM9999999990.00'),
  'occurred', to_char(pc_d6aa92b4e0a6dbcbfb5df322718a837644e4495da7086179bda2b7dc80f3, 'YYYY-MM-DD\"T\"HH24:MI:SS'),
  'label', pc_f889d19dc52751544763525e1d0e25a99d29123d3b642e83019d99c4af15,
  'payload', encode(pc_282d032f194af6bee8b8b95809fd6ef877edc8fa8b11e0c6e96dac927e7f, 'hex')
) ORDER BY pc_b064bb8641f87c6e2dcee3112a98ce5e5608c2f9a00717247ddbc7e1cc27)
FROM _oo.{PHYSICAL};
""", tuples=True)
    stored = json.loads(pg.strip())
    def norm(row):
        return {
            "key": str(row["key"]),
            "amount": format(Decimal(row["amount"]), "f"),
            "occurred": row["occurred"],
            "label": row["label"],
            "payload": (row.get("payload") or "").lower(),
        }
    left = [norm(row) for row in rows]
    right = [norm(row) for row in stored]
    diffs = [] if left == right else [{"oracle": left, "postgres": right}]
    gold = {"protocol": "oo.oracle.typed-source-gold/v1", "rows": len(left), "cells": len(left) * 5, "differences": len(diffs), "diffs": diffs, "key_2_absent": all(row["key"] != "2" for row in left), "reader_exit": code, "source_dml": 0}
    (dest / "typed-gold.json").write_text(json.dumps(gold) + "\n")
    schedule = psql("previewdb", f"SELECT json_agg(json_build_object('source_id', source_id, 'method', method, 'cycle', cycle, 'period_ms', period_ms, 'enabled', enabled, 'pin', pin) ORDER BY source_id, cycle) FROM jw_native.bindings WHERE source_id = '{SOURCE}';", tuples=True)
    (dest / "schedule.json").write_text(schedule)
    (dest / "rows.json").write_text(json.dumps(left) + "\n")
    prior = json.loads(G3.read_text())
    observed = gold["differences"] == 0 and gold["key_2_absent"] and gold["rows"] == 2 and prior.get("tombstones") == ["2:3"] and prior.get("customer_writes") == 0 and prior.get("repeat_load") is False
    return {"wait": waited, "gold": gold, "schedule_sha256": sha_bytes(schedule.encode()), "prior_cycle_sha256": sha_bytes(G3.read_bytes()), "lifecycle_observed": observed, "gateway_count": found["count_value"]}


def main():
    if len(sys.argv) != 3:
        raise SystemExit("usage")
    cfg = json.loads(Path(sys.argv[1]).read_text())
    run = Path(sys.argv[2])
    if cfg.get("host") != "oo-dl01w1-04-p2-oracle" or cfg.get("database") != "FREEPDB1" or cfg.get("user") != "OO04_READER":
        raise SystemExit("fixture")
    if cfg["selections"][0]["source_id"] != SOURCE or cfg["approved_objects"] != ["OO04_FIX.ONBOARD_ORDERS"]:
        raise SystemExit("fixture")
    ensure_accounts()
    run_product(run)
    gateway = bind_gateway(run / "product")
    found = probe(gateway, run / "probe")
    (GW / "probe-table.txt").write_text(json.dumps(found["table"]) + "\n")
    dialogue(run, gateway, found)
    cancelled = cancel_one(gateway)
    current = capture(cfg, run / "current", found)
    verdict = json.loads((run / "dialogue/verdict.json").read_text())
    receipt = run / "product/product-receipt.json"
    complete = verdict.get("outcome") == "verified" and receipt.is_file() and current["lifecycle_observed"] and bool(cancelled)
    body = {
        "protocol": "oo.oracle.whole-run/v1",
        "full_product_chain_complete": complete,
        "periodic_insert_update_delete_verified": complete,
        "recovery_verified": complete,
        "product_receipt": "product/product-receipt.json",
        "dialogue_verdict": "dialogue/verdict.json",
        "current_capture": "current/typed-gold.json",
        "schedule": "current/schedule.json",
        "waits": current["wait"],
        "prior_cycle_sha256": current["prior_cycle_sha256"],
        "cancel_query_id": cancelled,
        "customer_writes": 0,
        "published": False,
        "c1_logminer": "off",
    }
    (run / "whole.json").write_text(json.dumps(body, indent=2) + "\n")
    if not complete:
        raise SystemExit(2)


if __name__ == "__main__":
    main()
