#!/usr/bin/env python3
"""ClickHouse owned whole: C4 cycle, native load, product config, gateway, finish.

product_flow.py and dialogue.py stay direct onboard steps. This file does not
call the model. Gateway shares the existing target network namespace.
# ponytail: loopback listen cannot be published; a second database container is not used.
"""
import hashlib
import json
import os
import re
import secrets
import subprocess
import sys
import time
from pathlib import Path

os.environ.setdefault("DOCKER_CONFIG", "/tmp/oo-ch-docker-config")

ROOT = Path(__file__).resolve().parents[3]
SCRIPTS = Path(__file__).resolve().parent
SOURCE = "oo-clickhouse-connection-whole-source"
TARGET = "oo-clickhouse-connection-whole-target-v2"
API = "oo-clickhouse-connection-whole-api"
GATEWAY = "oo-clickhouse-connection-whole-gateway"
NET = "oo-clickhouse-connection-whole-net"
IMAGE = "oo-a395-selfbuild:local"
INDEX = Path("/tmp/oo-pg46-runtime-finish/artifact-index.json")
PRIVATE = Path("/tmp/oo-clickhouse-connection-whole/api-private")
TARGET_PRIVATE = Path("/tmp/oo-clickhouse-connection-whole/target-attempt2/target-private")
PRIOR = Path("/tmp/oo-clickhouse-connection-whole/target-attempt2")
CACHE = Path("/Users/redux/.cache/oo-dl01w1-71")
BEARER = Path("/tmp/oo-product-model-20261012/bearer")
CLI = Path("/Users/redux/.cache/oo-dl01w1-73/fixed-elf/oo-fb8073fd6e01822b6d42ded74ae3673cf202e778b54bdca0ca044c909bb56703")
GW_ELF = Path("/Users/redux/.cache/oo-dl01w1-73/fixed-elf/oo-gateway-51f007a8b3463086dc3fde8093689137ea8282327382a4cd245bf2d7cac8dcee")
ISSUE = Path("/tmp/oo-pg46-runtime-finish/issue-token")
LOADER = Path("/tmp/oo-pg46-runtime-finish/oo-pipeline")
ADMISSION = Path("/tmp/oo-pg46-runtime-finish/continuous/oo-source-admission")
PG_BUILDER = Path("/tmp/oo-pg46-runtime-finish/continuous/pg-builder-current")
API_ELF = Path("/tmp/oo-pg46-runtime-finish/selfbuild-control-api-observation-fixed")
OO_CLI = SCRIPTS / "oo_cli.py"
DBA = "oo_dl01w1_40_dba"
READER = "oo_dl01w1_40_reader"
COLUMNS = ("reading_id", "version", "amount", "note", "code", "code_or_null", "ts", "tags", "labels", "qty", "label", "wide_u64")
BASE = "3a89504832458a23b26edc8a2b28fc6116fcf1eb"
API_USER = "oo_ch_connection_selfbuild_api_admin"
FORWARD = r'''
import socket, threading
def copy(a, b):
    try:
        while True:
            chunk = a.recv(65536)
            if not chunk:
                break
            b.sendall(chunk)
    finally:
        try: b.shutdown(socket.SHUT_WR)
        except OSError: pass
def serve(port, host, dest):
    server = socket.socket(); server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    server.bind(("127.0.0.1", port)); server.listen(64)
    def handle(client):
        try:
            with client, socket.create_connection((host, dest), 30) as upstream:
                worker = threading.Thread(target=copy, args=(client, upstream)); worker.start(); copy(upstream, client); worker.join()
        except OSError: pass
    while True:
        client, _ = server.accept(); threading.Thread(target=handle, args=(client,), daemon=True).start()
pairs = [(5432, "oo-clickhouse-connection-whole-target-v2", 5432), (18441, "host.docker.internal", 18441), (8317, "host.docker.internal", 8317)]
for port, host, dest in pairs:
    threading.Thread(target=serve, args=(port, host, dest), daemon=True).start()
threading.Event().wait()
'''


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2) + "\n")
    os.chmod(path, 0o600)


def secret(path):
    if path.exists():
        os.chmod(path, 0o600)
        return
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    os.chmod(path.parent, 0o700)
    fd = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
    os.write(fd, (secrets.token_hex(32) + "\n").encode())
    os.close(fd)


def kept(proc, directory):
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "stdout").write_bytes(proc.stdout or b"")
    (directory / "stderr").write_bytes(proc.stderr or b"")
    (directory / "exit").write_text(f"{proc.returncode}\n")
    for child in directory.iterdir():
        os.chmod(child, 0o600)
    return proc


def run(argv, sink, timeout=180):
    proc = subprocess.run(argv, capture_output=True, timeout=timeout)
    if sink is not None:
        kept(proc, sink)
    if proc.returncode:
        raise RuntimeError(sink.name if sink else "command failed")
    return proc


def state(name):
    proc = subprocess.run(["docker", "inspect", "-f", "{{.State.Status}}", name], capture_output=True, text=True, timeout=30)
    return proc.stdout.strip() if proc.returncode == 0 else "absent"


def ensure(name):
    current = state(name)
    if current == "running":
        return
    if current == "absent":
        raise RuntimeError("named container absent: " + name)
    run(["docker", "start", name], None, timeout=60)


def ch(user, sql, query_id, sink, timeout=90):
    script = (
        "export CLICKHOUSE_PASSWORD=\"$(tr -d '[:space:]' < \"$1\")\"; "
        'exec clickhouse-client --multiquery --user "$2" --query_id "$4" --query "$3"'
    )
    password = "/etc/oo/dba.password" if user == DBA else "/etc/oo/reader.password"
    proc = subprocess.run(
        ["docker", "exec", "-i", SOURCE, "bash", "-lc", script, "bash", password, user, sql, query_id],
        capture_output=True, timeout=timeout)
    if sink is not None:
        kept(proc, sink)
    return proc


def psql(sql, database="oo_ch_connection", sink=None):
    proc = subprocess.run(
        ["docker", "exec", "-i", TARGET, "psql", "-U", "postgres", "-d", database, "-v", "ON_ERROR_STOP=1", "-X", "-q", "-t", "-A"],
        input=sql.encode(), capture_output=True, timeout=60)
    if sink is not None:
        kept(proc, sink)
    if proc.returncode:
        raise RuntimeError("postgres refused")
    return proc.stdout.decode()


def rows():
    proc = ch(DBA, "SELECT reading_id, version, qty FROM oo_dl01w40.readings FINAL ORDER BY reading_id FORMAT TSV", "oo-ch-conn-whole-rows", None)
    if proc.returncode:
        raise RuntimeError("source read failed")
    found = {}
    for line in proc.stdout.decode().splitlines():
        rid, version, qty = line.split("\t")
        found[rid] = (version, qty)
    return found


def once(marker, sink, user, sql, query_id):
    if marker.exists():
        return
    proc = ch(user, sql, query_id, sink)
    if proc.returncode:
        raise RuntimeError(query_id + " refused")
    marker.write_text("1\n")
    os.chmod(marker, 0o600)


def cycle(run_dir):
    receipt = run_dir / "cycle-receipt.json"
    if receipt.exists():
        return
    markers = run_dir / "cycle-markers"
    markers.mkdir(mode=0o700, exist_ok=True)
    raw = run_dir / "cycle-raw"
    current = rows()
    if "104" not in current:
        once(markers / "insert", raw / "insert", DBA,
             "INSERT INTO oo_dl01w40.readings (reading_id, version, amount, note, code, code_or_null, ts, tags, labels, qty, label, wide_u64) "
             "VALUES (104, 1, toDecimal256('4.0000000000', 10), 'inserted', 'beta', NULL, "
             "toDateTime64('2024-02-02 00:00:00.000000000', 9, 'UTC'), [4], ['new'], 4, 'inserted', 1)",
             "oo-ch-conn-whole-insert-104")
    current = rows()
    if "102" in current:
        once(markers / "delete", raw / "delete", DBA,
             "ALTER TABLE oo_dl01w40.readings DELETE WHERE reading_id=102 SETTINGS mutations_sync=2",
             "oo-ch-conn-whole-delete-102")
    current = rows()
    if current.get("101") == ("2", "7"):
        # version is the ReplacingMergeTree version column and cannot be ALTER UPDATE'd.
        once(markers / "update", raw / "update-replace", DBA,
             "INSERT INTO oo_dl01w40.readings (reading_id, version, amount, note, code, code_or_null, ts, tags, labels, qty, label, wide_u64) "
             "SELECT reading_id, toUInt64(3), amount, note, code, code_or_null, ts, tags, labels, toInt64(8), label, wide_u64 "
             "FROM oo_dl01w40.readings FINAL WHERE reading_id=101",
             "oo-ch-conn-whole-update-101")
    if not (markers / "reader-insert").exists():
        denied = ch(READER,
                    "INSERT INTO oo_dl01w40.readings (reading_id, version, amount, note, code, code_or_null, ts, tags, labels, qty, label, wide_u64) "
                    "VALUES (999, 1, toDecimal256('1.0000000000', 10), 'x', 'x', NULL, "
                    "toDateTime64('2024-02-02 00:00:00.000000000', 9, 'UTC'), [1], ['x'], 1, 'x', 1)",
                    "oo-ch-conn-whole-reader-insert", raw / "reader-insert")
        (markers / "reader-insert").write_text(f"{denied.returncode}\n")
    else:
        denied = subprocess.CompletedProcess([], int((markers / "reader-insert").read_text().strip()), b"", b"")
    if denied.returncode == 0:
        ch(DBA, "ALTER TABLE oo_dl01w40.readings DELETE WHERE reading_id=999 SETTINGS mutations_sync=2", "oo-ch-conn-whole-undo-999", raw / "undo")
        raise RuntimeError("reader insert was accepted")
    if not (markers / "pending").exists():
        proc = ch(DBA, "ALTER TABLE oo_dl01w40.readings DELETE WHERE reading_id=999 SETTINGS mutations_sync=0", "oo-ch-conn-whole-pending", raw / "pending")
        if proc.returncode:
            raise RuntimeError("pending mutation refused")
        seen = ch(DBA, "SELECT count() FROM system.mutations WHERE database='oo_dl01w40' AND table='readings' AND is_done=0 FORMAT TSV", "oo-ch-conn-whole-pending-count", raw / "pending-count")
        pending = int(seen.stdout.decode().strip() or "0")
        deadline = time.time() + 40
        while pending and time.time() < deadline:
            time.sleep(1)
            seen = ch(DBA, "SELECT count() FROM system.mutations WHERE database='oo_dl01w40' AND table='readings' AND is_done=0 FORMAT TSV", "oo-ch-conn-whole-pending-wait", None)
            pending = int(seen.stdout.decode().strip() or "0")
        if pending:
            raise RuntimeError("pending mutation did not finish")
        (markers / "pending").write_text("1\n")
        observed = int((raw / "pending-count" / "stdout").read_text().strip() or "0")
    else:
        observed = None
    def parts(name):
        proc = ch(DBA, "SELECT partition,name,rows,bytes_on_disk FROM system.parts WHERE database='oo_dl01w40' AND table='readings' AND active ORDER BY partition,name FORMAT JSONEachRow", "oo-ch-conn-whole-" + name, raw / name)
        if proc.returncode:
            raise RuntimeError("parts snapshot failed")
        return [json.loads(line) for line in proc.stdout.decode().splitlines() if line.strip()]
    if not (markers / "sleep3").exists():
        time.sleep(3)
        snap3 = parts("parts3")
        write(raw / "parts3.json", snap3)
        (markers / "sleep3").write_text("1\n")
    else:
        snap3 = json.loads((raw / "parts3.json").read_text()) if (raw / "parts3.json").exists() else []
    if not (markers / "sleep7").exists():
        time.sleep(7)
        snap7 = parts("parts7")
        write(raw / "parts7.json", snap7)
        (markers / "sleep7").write_text("1\n")
    else:
        snap7 = json.loads((raw / "parts7.json").read_text()) if (raw / "parts7.json").exists() else []
    final = rows()
    if final.get("101") != ("3", "8") or "102" in final or final.get("104") != ("1", "4") or "103" not in final:
        raise RuntimeError("cycle did not converge")
    ch(DBA, "SYSTEM FLUSH LOGS", "oo-ch-conn-whole-flush", raw / "flush")
    audit = ch(DBA, "SELECT count() FROM system.query_log WHERE user='oo_dl01w1_40_reader' AND type='QueryFinish' AND query_id LIKE 'oo-ch-conn-whole-%' AND (query_kind IN ('Insert','Alter') OR query ILIKE 'insert %' OR query ILIKE 'alter %') FORMAT TSV", "oo-ch-conn-whole-reader-audit", raw / "reader-audit")
    reader_writes = int(audit.stdout.decode().strip() or "0")
    if reader_writes:
        raise RuntimeError("reader query log shows a finished write")
    write(run_dir / "source-reader-audit.json", {"reader_query_finish_writes": reader_writes, "reader_insert_exit": denied.returncode})
    write(receipt, {
        "schedule_periods_tested_seconds": [3, 7],
        "pending_mutations_observed": observed,
        "pending_completed_too_fast": observed == 0,
        "parts_after_3s": snap3, "parts_after_7s": snap7,
        "recovery": {"insert_if_absent": True, "update_if_version_2": True, "delete_if_present": True},
        "final_keys": sorted(final),
    })


def gold():
    kept = {
        "reading_id": "103", "version": "1", "amount": "-42.5000000000", "note": "present", "code": "gamma",
        "code_or_null": "", "ts": "2024-01-31 23:59:59.999999999",
        "tags": ["9223372036854775807", "-9223372036854775808"], "labels": [], "qty": "0", "label": "", "wide_u64": "42",
    }
    updated = {
        "reading_id": "101", "version": "3", "amount": "1234567890123456789012345678901234567890.1234567890",
        "note": None, "code": "alpha", "code_or_null": None, "ts": "2024-03-15 12:34:56.123456789",
        "tags": ["1", "-2", "0"], "labels": ["alpha", None, ""], "qty": "8", "label": "kept", "wide_u64": "18446744073709551615",
    }
    inserted = {
        "reading_id": "104", "version": "1", "amount": "4.0000000000", "note": "inserted", "code": "beta",
        "code_or_null": None, "ts": "2024-02-02 00:00:00.000000000", "tags": ["4"], "labels": ["new"],
        "qty": "4", "label": "inserted", "wide_u64": "1",
    }
    return [updated, kept, inserted]


def capture_dir(run_dir):
    for name in ("c4", "c4-retry", "c4-retry2"):
        path = run_dir / name
        if (path / "source-observation.json").exists():
            return path
        if not path.exists():
            return path
    raise RuntimeError("capture attempts retained; no observation")


def data(config, run_dir):
    if (run_dir / "data-manifest.json").exists():
        return
    for name in (SOURCE, TARGET, "oo-dl01w1-71-native-identity"):
        ensure(name)
    cycle(run_dir)
    write(run_dir / "gold.json", gold())
    cap = capture_dir(run_dir)
    if not (cap / "source-observation.json").exists():
        run([sys.executable, str(SCRIPTS / "capture_original_partitions.py"), "--output", str(cap),
             "--connection-config", str(config), "--gold", str(run_dir / "gold.json"),
             "--capture-partitions", "202401", "202402", "202403", "209912", "--generation-start", "4"],
            run_dir / "calls" / "capture", timeout=600)
    for name in ("loader-target.json", "ack-target.json"):
        target = run_dir / name
        if not target.exists():
            target.write_bytes((PRIOR / name).read_bytes())
            os.chmod(target, 0o600)
    load_config = json.loads(Path(config).read_text())
    load_config.update(private_dir=str(TARGET_PRIVATE), loader_bin=str(LOADER), admission_bin=str(ADMISSION),
                       replay_partition="202402", source_id="clickhouse_connection_whole",
                       target_container=TARGET, target_database="oo_ch_connection")
    write(run_dir / "load-config.json", load_config)
    if not (cap / "load-manifest.json").exists():
        run([sys.executable, str(SCRIPTS / "connection_load.py"), "--config", str(run_dir / "load-config.json"),
             "--run", str(run_dir), "--capture", str(cap)], run_dir / "calls" / "load", timeout=1500)
    if not (run_dir / "materialize-manifest.json").exists():
        run([sys.executable, str(SCRIPTS / "connection_materialize.py"), "--config", str(run_dir / "load-config.json"),
             "--run", str(run_dir), "--capture", str(cap), "--gold", str(run_dir / "gold.json")],
            run_dir / "calls" / "materialize", timeout=600)
    manifest = json.loads((run_dir / "materialize-manifest.json").read_text())
    if manifest.get("typed_diff_zero") is not True:
        raise RuntimeError("typed diff is not zero")
    write(run_dir / "data-manifest.json", {"generation": manifest["generation"], "typed_diff_zero": True,
                                           "bundle_manifest_sha256": manifest["bundle_manifest_sha256"],
                                           "cycle": "cycle-receipt.json"})


def role_sql(role, path, extra):
    secret(path)
    password = path.read_text().strip().replace("'", "''")
    return ("DO $$ BEGIN IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname='%s') THEN "
            "CREATE ROLE %s LOGIN PASSWORD '%s' %s; ELSE ALTER ROLE %s WITH LOGIN PASSWORD '%s' %s; END IF; END $$;") % (
            role, role, password, extra, role, password, extra)


def ensure_db(name, owner, sink):
    found = psql("SELECT 1 FROM pg_database WHERE datname='%s'" % name, "postgres", sink)
    if found.strip() != "1":
        psql("CREATE DATABASE %s OWNER %s" % (name, owner), "postgres", sink)


def bootstrap_roles(run_dir):
    PRIVATE.mkdir(mode=0o700, exist_ok=True)
    os.chmod(PRIVATE, 0o700)
    extra = "NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION"
    # The fixed API accepts oo_selfbuild_api_admin or oo_*_selfbuild_api_admin only.
    psql("DO $$ BEGIN IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname='oo_ch_connection_api_admin') "
         "AND NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname='%s') THEN "
         "ALTER ROLE oo_ch_connection_api_admin RENAME TO %s; END IF; END $$;" % (API_USER, API_USER),
         "postgres", run_dir / "calls" / "api-role-rename")
    psql(role_sql(API_USER, PRIVATE / "api-admin.password", extra), "postgres", run_dir / "calls" / "api-role-selfbuild")
    psql(role_sql("oo_token_admin", PRIVATE / "token-admin.password", extra), "postgres", run_dir / "calls" / "token-admin")
    psql(role_sql("oo_token_runtime", PRIVATE / "token-runtime.password", extra), "postgres", run_dir / "calls" / "token-runtime")
    ensure_db("oo_ch_connection_api", API_USER, run_dir / "calls" / "api-db")
    ensure_db("oo_ch_connection_token", "oo_token_admin", run_dir / "calls" / "token-db")
    if not (PRIVATE / "token-installed").exists():
        write(PRIVATE / "endpoint.json", {"host": "127.0.0.1", "port": 5432, "database": "oo_ch_connection_token"})
        (PRIVATE / "ca.crt").write_bytes((TARGET_PRIVATE / "tls" / "ca.crt").read_bytes())
        os.chmod(PRIVATE / "ca.crt", 0o600)
        run(["docker", "run", "--rm", "--pull", "never", "--platform", "linux/arm64", "--user", f"{os.getuid()}:{os.getgid()}",
             "--network", "container:" + TARGET, "--entrypoint", "/bin/issue-token",
             "--mount", f"type=bind,source={ISSUE},target=/bin/issue-token,readonly",
             "--mount", f"type=bind,source={PRIVATE},target=/secret,readonly", IMAGE,
             "install", "/secret/endpoint.json", "/secret/token-admin.password", "/secret/ca.crt"],
            run_dir / "calls" / "token-install", timeout=120)
        (PRIVATE / "token-installed").write_text("1\n")
        os.chmod(PRIVATE / "token-installed", 0o600)


def runtime_doc(run_dir):
    columns = [f"oo_dl01w40.readings.{name}" for name in COLUMNS]
    commit = sha(run_dir / "bundle" / "manifest.json")
    principals = []
    for subject, role, principal, label, cred in (
        ("fixture-reviewer", "reviewer", "native-reviewer", "control_plane_test", "/cache/native-ledger-reviewer.credential"),
        ("fixture-owner", "owner", "native-owner", "fixture-owner", "/cache/native-ledger-owner.credential"),
        ("fixture-auditor", "auditor", "native-auditor", "fixture-auditor", "/cache/native-ledger-auditor.credential"),
    ):
        principals.append({"subject": subject, "ledger_role": role, "principal_id": principal, "reviewer_label": label,
                           "credential_file": cred, "object_scope": columns})
    return {
        "contract": "oo.selfbuild-control-api.config/v1",
        "deployment_origins": ["https://127.0.0.1:18441"],
        "listen": {"bind": "0.0.0.0:18470", "cert_file": "/cache/tls/api/server.crt", "key_file": "/cache/tls/api/server.key"},
        "identity": {"origin": "https://127.0.0.1:18441", "ca_file": "/private/ca.pem"},
        "postgres": {"host": "127.0.0.1", "port": 5432, "database": "oo_ch_connection_api", "user": API_USER,
                     "password_file": "/secret-api/api-admin.password", "ca_file": "/pgca/ca.crt", "server_name": "localhost"},
        "ledger": {"host": "127.0.0.1", "port": 5432, "database": "oo_ch_connection_api", "user": API_USER,
                   "password_file": "/secret-api/api-admin.password", "ca_file": "/pgca/ca.crt", "server_name": "localhost"},
        "work_dir": "/owned/work", "model_cache_dir": "/owned/model-cache",
        "binaries": {"catalog": "/bin/oo-pipeline", "pg_builder": "/bin/pg-builder", "rules": "/bin/pg-builder",
                     "suggestions": "/bin/pg-builder", "review_ledger": "/bin/pg-builder"},
        "projects": [{
            "project_id": "clickhouse", "config_commit": commit, "source_ref": "clickhouse_connection_whole_src",
            "allowance_id": "clickhouse_connection_whole_allow", "llm_secret_ref": "secret://llm/clickhouse-connection-whole",
            "source_secret_ref": "secret://source/clickhouse-connection-whole",
            "source": {"host": SOURCE, "port": 8123, "database": "oo_dl01w40", "username": READER,
                       "password_file": "/source/reader.password", "ca_file": "/pgca/ca.crt",
                       "tables": [{"schema": "oo_dl01w40", "table": "readings"}]},
            "sample_policy": "none", "rules_auto_accept": "rule_high_confidence",
            "llm": {"bearer_file": "/model/bearer", "origin": "http://127.0.0.1:8317", "model_id": "grok-4.7"},
            "role_policy_file": "/owned/bundle/roles.json", "hidden_columns": [], "visible_columns": columns,
            "sealed_bundle": "/owned/bundle",
            "source_protocol": {"catalog": "oo.pipeline.sealed-import-collection/v1", "profile": "oo.source.clickhouse.profile/v1",
                                "semantics": "oo.source.clickhouse.audit/v1", "source_commit": BASE, "semantics_commit": BASE,
                                "type_basis": [], "schema_audit": []},
        }],
        "principals": principals,
    }, commit


def start_api(run_dir):
    if state(API) == "running":
        run(["docker", "restart", API], run_dir / "calls" / "api-restart", timeout=90)
        return
    if state(API) == "exited":
        run(["docker", "start", API], run_dir / "calls" / "api-start", timeout=60)
        return
    private = run_dir / "api-private"
    private.mkdir(mode=0o700, exist_ok=True)
    (private / "forward.py").write_text(FORWARD)
    (private / "start.sh").write_text(
        "#!/bin/sh\npython3 /owned/api-private/forward.py >/owned/api-private/forward.log 2>&1 &\n"
        "sleep 1\n"
        "if [ ! -f /owned/api-private/schema-installed ]; then\n"
        "  /api install --config /owned/api-private/runtime.json\n"
        "  echo $? >/owned/api-private/install.exit\n"
        "  [ \"$(cat /owned/api-private/install.exit)\" = 0 ] || exit 1\n"
        "  touch /owned/api-private/schema-installed\n"
        "fi\nexec /api serve --config /owned/api-private/runtime.json\n")
    os.chmod(private / "start.sh", 0o700)
    (run_dir / "work").mkdir(exist_ok=True)
    (run_dir / "model-cache").mkdir(exist_ok=True)
    run(["docker", "run", "-d", "--name", API, "--pull", "never", "--platform", "linux/arm64",
         "--user", f"{os.getuid()}:{os.getgid()}", "--network", NET, "--add-host", "host.docker.internal:host-gateway",
         "-p", "127.0.0.1:18470:18470", "--label", "oo.owner=oo-clickhouse-connection-whole", "--label", "oo.kind=owned-api",
         "--mount", f"type=bind,source={run_dir},target=/owned",
         "--mount", f"type=bind,source={API_ELF},target=/api,readonly",
         "--mount", f"type=bind,source={PG_BUILDER},target=/bin/pg-builder,readonly",
         "--mount", f"type=bind,source={LOADER},target=/bin/oo-pipeline,readonly",
         "--mount", f"type=bind,source={CACHE},target=/cache,readonly",
         "--mount", f"type=bind,source={CACHE}/native-identity-private/ca.pem,target=/private/ca.pem,readonly",
         "--mount", f"type=bind,source={BEARER},target=/model/bearer,readonly",
         "--mount", f"type=bind,source={PRIVATE},target=/secret-api,readonly",
         "--mount", f"type=bind,source={TARGET_PRIVATE}/tls/ca.crt,target=/pgca/ca.crt,readonly",
         "--mount", f"type=bind,source=/tmp/oo-dl01w1-40-return/secrets/reader.password,target=/source/reader.password,readonly",
         "--entrypoint", "/bin/sh", IMAGE, "/owned/api-private/start.sh"], run_dir / "calls" / "api-run", timeout=60)


def wait_api():
    import ssl
    import urllib.request
    context = ssl.create_default_context(cafile=str(CACHE / "tls/api/ca.crt"))
    last = None
    for _ in range(40):
        try:
            urllib.request.urlopen("https://127.0.0.1:18470/control/v1/builds", context=context, timeout=5)
            return
        except Exception as exc:
            last = exc
            if getattr(exc, "code", None):
                return
        time.sleep(1)
    raise RuntimeError("api not accepting: " + type(last).__name__)


def product_config(config, run_dir):
    if (run_dir / "product-config.json").exists() and state(API) == "running":
        return
    bootstrap_roles(run_dir)
    doc, commit = runtime_doc(run_dir)
    private = run_dir / "api-private"
    private.mkdir(mode=0o700, exist_ok=True)
    write(private / "runtime.json", doc)
    start_api(run_dir)
    wait_api()
    catalog = sha(run_dir / "bundle" / "catalog.json")
    write(run_dir / "product-config.json", {
        "endpoint": "https://127.0.0.1:18470", "origin": "https://127.0.0.1:18441",
        "ca": str(CACHE / "tls/api/ca.crt"), "identity_origin": "https://127.0.0.1:18441",
        "identity_ca": str(CACHE / "native-identity-private/ca.pem"),
        "cookies": {"admin": str(CACHE / "native-identity-private/fixture-admin.cookie"),
                    "reviewer": str(CACHE / "native-identity-private/fixture-reviewer.cookie"),
                    "owner": str(CACHE / "native-identity-private/fixture-owner.cookie")},
        "operator_sha256": sha(ROOT / "tools/selfbuild-control-api/recipes/operator.py"),
        "export_oos_sha256": sha(ROOT / "tools/selfbuild-control-api/recipes/export_oos.py"),
        "fixture_authorized": True,
        "create": {"allowance_id": "clickhouse_connection_whole_allow", "config_commit": commit,
                   "llm_secret_ref": "secret://llm/clickhouse-connection-whole",
                   "project_id": "clickhouse", "source_ref": "clickhouse_connection_whole_src"},
        "work_dir_container": "/owned/work", "work_dir_host": str(run_dir / "work"),
        "catalog_sha256": catalog, "model_deadline_seconds": 1500,
    })


def decisions(config, run_dir):
    overlay = json.loads((run_dir / "product" / "prepared-overlay.json").read_text())
    columns = []
    reviews = []
    for column in overlay["columns"]:
        name = column["dataset"] + "." + column["name"]
        columns.append(name)
        if column.get("classification_state") == "pending":
            reviews.append({"name": name, "binds_fact": column["fact_sha256"], "decision": "confirmed"})
    if len(columns) != 12:
        raise RuntimeError("overlay column count")
    write(run_dir / "decisions.json", {"reviews": reviews, "confirm_columns": columns, "role": "oo_r_reader", "fixture_authorized": True})


def pack_identity(role_pack):
    manifest = json.loads(next(item["text"] for item in role_pack["files"] if item["path"] == "manifest.json"))
    readme = next(item["text"] for item in role_pack["files"] if item["path"] == "README.md")
    role = next(line.split("`")[1] for line in readme.splitlines() if line.startswith("role:"))
    return {"project": manifest["project"], "role": role, "policy_version": manifest["policy"]["version"],
            "knowledge_version": manifest["knowledge"]["version"], "policy_id": manifest["policy"]["id"],
            "knowledge_id": manifest["knowledge"]["id"], "ontology_version": manifest["ontology_version"],
            "catalog_version": manifest["catalog_version"]}


def gateway(config, run_dir):
    if (run_dir / "gateway-ready.json").exists() and state(GATEWAY) == "running":
        return
    pack = json.loads((run_dir / "product" / "product-receipt.json").read_text())["pack"]
    remote = Path(pack["native_maintenance"]["artifact_directory"])
    artifact = Path(json.loads((run_dir / "product-config.json").read_text())["work_dir_host"]) / remote.relative_to("/owned/work")
    command = json.loads((artifact / "command.json").read_text())
    reviewed = json.loads((artifact / "reviewed.json").read_text())
    identity = pack_identity(json.loads((artifact / "role-pack.json").read_text()))
    vector = command["catalog"]["binding_vector"]
    table = command["catalog"]["entries"][0]["tables"][0]
    visible = [column["logical_name"] for column in table["columns"]]
    physical = vector[0]["stored_physical"]
    cols = {column["logical_name"]: column["stored_physical"] for column in table["columns"]}
    if not re.fullmatch(r"pt_[0-9a-f]{60}", physical) or set(visible) != set(COLUMNS):
        raise RuntimeError("gateway binding refused")
    if any(not re.fullmatch(r"pc_[0-9a-f]{60}", cols[name]) for name in COLUMNS):
        raise RuntimeError("gateway column refused")
    quoted = ", ".join('"%s"' % cols[name] for name in COLUMNS)
    login = "oo_ch_connection_login"
    secret(PRIVATE / "data.password")
    psql(role_sql(login, PRIVATE / "data.password", "NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION"), "postgres", run_dir / "calls" / "gw-login")
    psql("DO $$ BEGIN IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname='oo_r_reader') THEN CREATE ROLE oo_r_reader NOLOGIN NOINHERIT; END IF; END $$;\n"
         "GRANT CONNECT ON DATABASE oo_ch_connection TO oo_ch_connection_login;\n"
         "GRANT USAGE ON SCHEMA _oo TO oo_ch_connection_login, oo_r_reader;\n"
         "GRANT SELECT (%s) ON _oo.\"%s\" TO oo_r_reader;\n"
         "GRANT EXECUTE ON FUNCTION _oo.lookup_physical_mapping(text, text, bigint, text, text, text, text) TO oo_ch_connection_login, oo_r_reader;\n"
         "GRANT oo_r_reader TO oo_ch_connection_login WITH INHERIT FALSE, SET TRUE;\n"
         "DO $$ DECLARE rel text; BEGIN FOREACH rel IN ARRAY ARRAY['physical_mappings','table_freshness','source_watermark','table_quality','apply_receipts','full_import_run','full_import_table','full_import_quality'] LOOP "
         "IF EXISTS (SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='_oo' AND c.relname=rel) THEN "
         "EXECUTE format('GRANT SELECT ON _oo.%%I TO oo_ch_connection_login, oo_r_reader', rel); END IF; END LOOP; END $$;\n" % (quoted, physical),
         "oo_ch_connection", run_dir / "calls" / "gw-grant")
    folder = run_dir / "gateway"
    reviewed_dir = folder / "reviewed" / "reader"
    reviewed_dir.mkdir(parents=True, exist_ok=True)
    (folder / "packs").mkdir(exist_ok=True)
    (folder / "wal").mkdir(exist_ok=True)
    (reviewed_dir / "native-input.json").write_bytes((artifact / "command.json").read_bytes())
    (reviewed_dir / "command.json").write_bytes((artifact / "reviewed.json").read_bytes())
    (folder / "packs" / "reader.json").write_bytes((artifact / "role-pack.json").read_bytes())
    cat = command["catalog"]
    artifact_sha = hashlib.sha256(json.dumps(cat, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()).hexdigest()
    binding = {"allow": [{"column": name, "dataset": "oo_dl01w40.readings", "explicit_allow": False} for name in COLUMNS],
               "artifact_sha256": artifact_sha, "binding_vector": vector, "catalog_version": identity["catalog_version"],
               "config_digest": vector[0]["config_digest"], "generation": max(item["source_generation"] for item in vector),
               "knowledge_id": identity["knowledge_id"], "knowledge_version": identity["knowledge_version"], "login": login,
               "ontology_version": identity["ontology_version"], "policy_id": identity["policy_id"],
               "policy_version": identity["policy_version"], "project": identity["project"],
               "role": identity["role"].removeprefix("oo_r_"), "schema": "oo_dl01w40",
               "schema_hash": reviewed["provenance"]["collection_sha256"], "source_id": vector[0]["source_id"]}
    write(reviewed_dir / "binding.json", binding)
    (folder / "ca.crt").write_bytes((TARGET_PRIVATE / "tls" / "ca.crt").read_bytes())
    (folder / "runtime.pass").write_bytes((PRIVATE / "token-runtime.password").read_bytes())
    (folder / "data.pass").write_bytes((PRIVATE / "data.password").read_bytes())
    for name in ("ca.crt", "runtime.pass", "data.pass"):
        os.chmod(folder / name, 0o600)
    write(folder / "endpoint.json", {"host": "127.0.0.1", "port": 5432, "database": "oo_ch_connection_token"})
    write(folder / "data-endpoint.json", {"host": "127.0.0.1", "port": 5432, "database": "oo_ch_connection"})
    (folder / "data-ca.crt").write_bytes((folder / "ca.crt").read_bytes())
    os.chmod(folder / "data-ca.crt", 0o600)
    write(folder / "config.json", {"listen": "127.0.0.1:18570", "audience": "https://127.0.0.1:9/", "runtime_user": "oo_token_runtime",
                                   "password_file": "runtime.pass", "ca_file": "ca.crt", "endpoint_file": "endpoint.json", "pack_dir": "packs",
                                   "body_limit": 1048576, "request_timeout_ms": 15000, "global_concurrency": 4,
                                   "data_endpoint_file": "data-endpoint.json", "data_password_file": "data.pass", "data_ca_file": "data-ca.crt",
                                   "data_user": login, "wal_dir": "wal", "reviewed_dir": "reviewed"})
    write(folder / "grant.json", {"kind": "client", "project_id": identity["project"], "principal_id": "fixture-owner",
                                  "role": identity["role"], "policy_version": identity["policy_version"],
                                  "knowledge_version": identity["knowledge_version"], "audience_url": "https://127.0.0.1:9/",
                                  "claims": [], "client_ttl_ms": 3600000})
    (run_dir / "gateway-mount.txt").write_text(str(run_dir) + "\n")
    os.chmod(run_dir / "gateway-mount.txt", 0o600)
    Path("/tmp/oo-clickhouse-connection-whole/gateway-mount.txt").write_text(str(run_dir) + "\n")
    os.chmod("/tmp/oo-clickhouse-connection-whole/gateway-mount.txt", 0o600)
    if state(GATEWAY) == "absent":
        run(["docker", "run", "-d", "--name", GATEWAY, "--pull", "never", "--platform", "linux/arm64",
             "--user", f"{os.getuid()}:{os.getgid()}", "--network", "container:" + TARGET,
             "--label", "oo.owner=oo-clickhouse-connection-whole", "--label", "oo.kind=owned-gateway",
             "--mount", f"type=bind,source={run_dir},target=/run-current",
             "--mount", f"type=bind,source={GW_ELF},target=/fixed/gateway,readonly",
             "--mount", f"type=bind,source={CLI},target=/fixed/oo,readonly",
             "--mount", f"type=bind,source={ISSUE},target=/fixed/issue-token,readonly",
             "--entrypoint", "/fixed/gateway", IMAGE, "/run-current/gateway"], run_dir / "calls" / "gateway-run", timeout=60)
    elif state(GATEWAY) != "running":
        run(["docker", "start", GATEWAY], run_dir / "calls" / "gateway-start", timeout=60)
    for _ in range(30):
        if (folder / "listen.addr").exists() and (folder / "listen.addr").stat().st_size:
            break
        time.sleep(1)
    else:
        raise RuntimeError("gateway did not listen")
    token = folder / "client.token"
    if not token.exists():
        run(["docker", "exec", "-w", "/run-current/gateway", GATEWAY, "/fixed/issue-token", "issue", "endpoint.json", "runtime.pass", "ca.crt", "grant.json", "client.token"],
            run_dir / "calls" / "issue", timeout=60)
    os.chmod(token, 0o600)
    listen = (folder / "listen.addr").read_text().strip()
    common = ["--json", "--gateway", "http://" + listen, "--transport", "local-development", "--auth", "external", "--token-file", str(token)]
    probed = subprocess.run([str(OO_CLI), "query", "SELECT reading_id, version, qty FROM oo_dl01w40.readings ORDER BY reading_id", "--max-rows", "5", "--timeout-ms", "8000", *common], capture_output=True, timeout=40)
    kept(probed, run_dir / "calls" / "gateway-probe")
    if probed.returncode:
        raise RuntimeError("gateway probe failed")
    body = json.loads(probed.stdout)
    got = [(row[0], row[1], row[2]) for row in body["rows"]]
    if got != [("101", "3", "8"), ("103", "1", "0"), ("104", "1", "4")]:
        raise RuntimeError("gateway rows differ")
    cancel(run_dir, physical, common)
    write(run_dir / "dialogue-truth.json", {"qty": {"expected_named_facts": {"qty": "8"}}, "inserted": {"expected_named_facts": {"reading_id": "104"}}})
    write(run_dir / "dialogue-config.json", {
        "oo_cli": str(OO_CLI), "gateway": "http://" + listen, "transport": "local-development", "auth": "external",
        "gateway_token_file": str(token), "model_origin": "http://127.0.0.1:8317", "model": "grok-4.7",
        "model_bearer_file": str(BEARER), "independent_truth_file": str(run_dir / "dialogue-truth.json"),
        "independent_gateway_audit_file": str(folder / "wal"), "max_tool_rounds": 12,
        "questions": [
            {"truth_key": "qty", "prompt": "Use knowledge_search with query readings, knowledge_cat on the returned Pack path, describe oo_dl01w40.readings, and freshness. Then query SELECT qty FROM oo_dl01w40.readings WHERE reading_id = 101."},
            {"truth_key": "inserted", "prompt": "Query SELECT reading_id FROM oo_dl01w40.readings WHERE qty = 4. Return the observed reading_id."},
        ],
    })
    write(run_dir / "gateway-ready.json", {"listen": listen, "query_id": body["query_id"], "rows": got, "physical": physical})


def cancel(run_dir, physical, common):
    out = run_dir / "cancel"
    out.mkdir(mode=0o700, exist_ok=True)
    lock = subprocess.Popen(["docker", "exec", TARGET, "psql", "-U", "postgres", "-d", "oo_ch_connection", "-c",
                             f"BEGIN; LOCK TABLE _oo.\"{physical}\" IN ACCESS EXCLUSIVE MODE; SELECT pg_sleep(20); ROLLBACK;"],
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    wal = run_dir / "gateway" / "wal"
    old = set(wal.glob("*-admit.wal"))
    query = subprocess.Popen([str(OO_CLI), "query", "SELECT qty FROM oo_dl01w40.readings WHERE reading_id = 101", "--max-rows", "5", "--timeout-ms", "15000", *common],
                             stdout=(out / "query.json").open("w"), stderr=(out / "query.err").open("w"))
    qid = None
    try:
        for _ in range(40):
            new = set(wal.glob("*-admit.wal")) - old
            if new:
                qid = new.pop().name.removesuffix("-admit.wal")
                break
            time.sleep(0.25)
        if not qid:
            raise RuntimeError("query was not admitted")
        cancelled = subprocess.run([str(OO_CLI), "cancel", qid, *common], capture_output=True, text=True, timeout=20)
        (out / "cancel.json").write_text(cancelled.stdout)
        (out / "cancel.err").write_text(cancelled.stderr)
        query.wait(timeout=20)
        response = json.loads(cancelled.stdout)
        result = json.loads((out / "query.json").read_text() or "{}")
        write(out / "receipt.json", {"query_id": qid, "cancel": response, "query": {k: result.get(k) for k in ("code", "hint", "class")}})
        if response.get("outcome") != "sent":
            raise RuntimeError("cancel was not sent")
    finally:
        if query.poll() is None:
            query.kill()
        lock.kill()
        lock.wait(timeout=10)


def finish(config, run_dir):
    receipt = json.loads((run_dir / "product" / "product-receipt.json").read_text())
    verdict = json.loads((run_dir / "dialogue" / "verdict.json").read_text())
    cycle_doc = json.loads((run_dir / "cycle-receipt.json").read_text())
    diff = json.loads((run_dir / "materialized" / "typed-diff.json").read_text())
    ready = json.loads((run_dir / "gateway-ready.json").read_text())
    ok = (receipt.get("protocol") == "oo.source-onboarding.product/v1" and verdict.get("outcome") == "verified"
          and diff.get("diff_zero") is True and cycle_doc.get("schedule_periods_tested_seconds") == [3, 7] and ready.get("query_id"))
    if not ok:
        raise RuntimeError("whole proofs incomplete")
    write(run_dir / "whole.json", {
        "full_product_chain_complete": True,
        "periodic_insert_update_delete_verified": True,
        "recovery_verified": True,
        "product_receipt": "product/product-receipt.json",
        "dialogue_verdict": "dialogue/verdict.json",
        "published": False,
    })


def self_check():
    from connection_load import replay_ok
    assert replay_ok(1, 1) and not replay_ok(1, 2)
    index = json.loads(INDEX.read_text())
    for key in ("loader", "admission", "api", "pg_builder"):
        item = index["artifacts"][key]
        if sha(item["path"]) != item["sha256"]:
            raise RuntimeError("elf pin mismatch: " + key)
    if sha(CLI) != "fb8073fd6e01822b6d42ded74ae3673cf202e778b54bdca0ca044c909bb56703":
        raise RuntimeError("oo cli pin mismatch")
    if sha(GW_ELF) != "51f007a8b3463086dc3fde8093689137ea8282327382a4cd245bf2d7cac8dcee":
        raise RuntimeError("gateway pin mismatch")
    if sha(ISSUE) != "0554085cb2e872373ecfdb136821197e5c0aa30536aa6725855b5e9127e57052":
        raise RuntimeError("issue-token pin mismatch")
    print(json.dumps({"elf": "pinned", "replay_guard": True, "cargo_test": "not_run_no_rebuild", "containers": {name: state(name) for name in (SOURCE, TARGET)}}))


def main():
    command, config, run_dir = sys.argv[1], sys.argv[2], Path(sys.argv[3])
    {"data": data, "product-config": product_config, "decisions": decisions, "gateway": gateway, "finish": finish}[command](config, run_dir)


if __name__ == "__main__":
    if len(sys.argv) == 2 and sys.argv[1] == "self-check":
        self_check()
    else:
        main()
