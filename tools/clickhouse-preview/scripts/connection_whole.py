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
MAINT_API = Path("/Users/redux/.cache/oo-grok-connection-full-20261014/fixed-api-47c2c99ed/selfbuild-control-api-5bc832f4a1bb9ada23a6f2a2691bf7d784d09977aa770f7a944c3c9d123be88d")
MAINT_API_SHA = "5bc832f4a1bb9ada23a6f2a2691bf7d784d09977aa770f7a944c3c9d123be88d"
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


def configured_periods(capture):
    if capture.get("sync") != "partition_refresh":
        raise RuntimeError("capture is not C4 partition_refresh")
    if (capture.get("delete_detection") or {}).get("mode") != "partition_replace":
        raise RuntimeError("C4 delete detection is not partition_replace")
    periods = capture.get("periods")
    if not isinstance(periods, list) or len(periods) != 2 or periods[0] == periods[1]:
        raise RuntimeError("two distinct capture periods are required")
    if capture.get("every") != periods[0]:
        raise RuntimeError("capture.every is not the first period")
    return periods


def added_row(reading_id, qty, day):
    label = "p" + reading_id
    return {
        "reading_id": reading_id, "version": "1", "amount": f"{qty}.0000000000", "note": label,
        "code": "beta", "code_or_null": None, "ts": f"2024-03-{day} 00:00:00.000000000",
        "tags": [str(qty)], "labels": [label], "qty": str(qty), "label": label, "wide_u64": str(qty),
    }


def insert_sql(row):
    tags = ",".join(row["tags"])
    labels = ",".join("'" + item + "'" for item in row["labels"])
    return (
        "INSERT INTO oo_dl01w40.readings (reading_id, version, amount, note, code, code_or_null, ts, tags, labels, qty, label, wide_u64) "
        f"VALUES ({row['reading_id']}, 1, toDecimal256('{row['amount']}', 10), '{row['note']}', 'beta', NULL, "
        f"toDateTime64('{row['ts']}', 9, 'UTC'), [{tags}], [{labels}], {row['qty']}, '{row['label']}', {row['wide_u64']})"
    )


def partition_state():
    text = psql("SELECT source_generation::text||'|'||partition_id||'|'||acknowledged::text||'|'||coalesce(current_signature,'') "
                "FROM _oo.native_partition_state WHERE source='clickhouse_connection_whole' ORDER BY source_generation")
    found = []
    for line in text.splitlines():
        if not line.strip():
            continue
        generation, partition, acknowledged, signature = line.split("|", 3)
        found.append({"source_generation": int(generation), "partition_id": partition,
                      "acknowledged": acknowledged == "true", "signature": signature or None})
    return found


def lease_report(run_dir):
    leases = int(psql("SELECT count(*) FROM jobs.leases").strip() or "0")
    tasks = [line for line in psql("SELECT state||':'||count(*)::text FROM jobs.tasks GROUP BY state ORDER BY state").splitlines() if line.strip()]
    report = {"open_leases": leases, "tasks": tasks, "baseline_rows": rows(), "partitions": partition_state()}
    write(run_dir / "lease-report.json", report)
    if leases:
        raise RuntimeError("open job lease; no new enqueue")
    generations = {item["source_generation"]: item for item in report["partitions"]}
    if any(not generations.get(generation, {}).get("acknowledged") for generation in (4, 5)):
        raise RuntimeError("committed generations are not acknowledged")
    water = max((item["source_generation"] for item in report["partitions"] if item["partition_id"] == "202403" and item["acknowledged"]), default=0)
    base = report["baseline_rows"]
    if water >= 8:
        if base.get("101") != ("3", "8") or base.get("103") != ("1", "0") or base.get("104") != ("1", "4") or base.get("105") != ("1", "5") or "106" in base:
            raise RuntimeError("source baseline is not the committed fixture")
    elif water == 6 and not (run_dir / "cycle-markers" / "insert-105").exists():
        if base != {"101": ("3", "8"), "103": ("1", "0"), "104": ("1", "4")} or max(generations) != 6:
            raise RuntimeError("source baseline is not the committed fixture")
    elif not (run_dir / "cycle-markers" / "insert-105").exists():
        raise RuntimeError("target generation is not the committed high water")
    return report


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


def load_one(config, run_dir, capture, generation, replay):
    for name in ("loader-target.json", "ack-target.json"):
        target = run_dir / name
        if not target.exists():
            target.write_bytes((PRIOR / name).read_bytes())
            os.chmod(target, 0o600)
    load_config = json.loads(Path(config).read_text())
    load_config.update(private_dir=str(TARGET_PRIVATE), loader_bin=str(LOADER), admission_bin=str(ADMISSION),
                       replay_partition="202403" if replay else "", source_id="clickhouse_connection_whole",
                       target_container=TARGET, target_database="oo_ch_connection")
    path = run_dir / f"load-config-g{generation}.json"
    write(path, load_config)
    if not (capture / "load-manifest.json").exists():
        run([sys.executable, str(SCRIPTS / "connection_load.py"), "--config", str(path),
             "--run", str(run_dir), "--capture", str(capture), "--partitions", "202403"],
            run_dir / "calls" / f"load-g{generation}", timeout=1500)


def apply_period(item, markers, raw):
    change = item["change"]
    if change == "insert 107":
        once(markers / "insert-107", raw / "insert-107", DBA, insert_sql(added_row("107", 7, "19")), "oo-ch-default-insert-107")
    elif change == "physical delete 107":
        once(markers / "delete-107", raw / "delete-107", DBA,
             "ALTER TABLE oo_dl01w40.readings DELETE WHERE reading_id=107 SETTINGS mutations_sync=2",
             "oo-ch-default-delete-107")
    elif change == "insert 105 and 106":
        once(markers / "insert-105", raw / "insert-105", DBA, insert_sql(added_row("105", 5, "16")), "oo-ch-default-insert-105")
        once(markers / "insert-106", raw / "insert-106", DBA, insert_sql(added_row("106", 6, "17")), "oo-ch-default-insert-106")
    elif change == "physical delete 106":
        once(markers / "delete-106", raw / "delete-106", DBA,
             "ALTER TABLE oo_dl01w40.readings DELETE WHERE reading_id=106 SETTINGS mutations_sync=2",
             "oo-ch-default-delete-106")
    else:
        raise RuntimeError("period change is not an authorized fixture statement")


def period_plan(advanced, periods):
    kept = added_row("105", 5, "16")
    if advanced:
        added = added_row("107", 7, "19")
        return [gold() + [kept, added], gold() + [kept]], {
            "method": "C4", "delete_detection": "partition_replace", "partition_id": "202403",
            "committed_generations_not_replayed": [4, 5, 7, 8],
            "periods": [
                {"every": periods[0], "generation": 9, "change": "insert 107", "keys": ["101", "103", "104", "105", "107"], "expect": {"107": ["1", "7"]}},
                {"every": periods[1], "generation": 10, "change": "physical delete 107", "keys": ["101", "103", "104", "105"], "expect": {}},
            ],
        }
    added = added_row("106", 6, "17")
    return [gold() + [kept, added], gold() + [kept]], {
        "method": "C4", "delete_detection": "partition_replace", "partition_id": "202403",
        "committed_generations_not_replayed": [4, 5, 6],
        "periods": [
            {"every": periods[0], "generation": 7, "change": "insert 105 and 106", "keys": ["101", "103", "104", "105", "106"], "expect": {"105": ["1", "5"], "106": ["1", "6"]}},
            {"every": periods[1], "generation": 8, "change": "physical delete 106", "keys": ["101", "103", "104", "105"], "expect": {}},
        ],
    }


def data(config, run_dir):
    if (run_dir / "data-manifest.json").exists():
        return
    for name in (SOURCE, TARGET, "oo-dl01w1-71-native-identity"):
        ensure(name)
    document = json.loads(Path(config).read_text())
    before = lease_report(run_dir)
    from c4_capture import observe_period
    periods = configured_periods(document["capture"])
    advanced = before["baseline_rows"].get("105") == ("1", "5") and "106" not in before["baseline_rows"]
    declared, plan = period_plan(advanced, periods)
    plan_path = run_dir / "period-plan.json"
    if not plan_path.exists():
        write(plan_path, plan)
    elif json.loads(plan_path.read_text())["periods"] != plan["periods"]:
        raise RuntimeError("period plan drifted")
    markers = run_dir / "cycle-markers"
    markers.mkdir(mode=0o700, exist_ok=True)
    raw = run_dir / "cycle-raw"
    if not (markers / "reader-insert").exists():
        denied = ch(READER, insert_sql(added_row("998", 1, "18")), "oo-ch-default-reader-insert", raw / "reader-insert")
        (markers / "reader-insert").write_text(f"{denied.returncode}\n")
        os.chmod(markers / "reader-insert", 0o600)
    else:
        denied = subprocess.CompletedProcess([], int((markers / "reader-insert").read_text().strip()), b"", b"")
    if denied.returncode == 0:
        raise RuntimeError("reader insert was accepted")
    done = []
    for index, every in enumerate(periods):
        item = plan["periods"][index]
        gold_path = run_dir / f"gold-period-{index + 1}.json"
        if not gold_path.exists():
            write(gold_path, declared[index])
        capture = run_dir / f"c4-p{index + 1}"
        loaded = (capture / "load-manifest.json").exists()
        if not loaded:
            apply_period(item, markers, raw)
            observed = rows()
            expect = item.get("expect") or {}
            if sorted(observed) != item["keys"] or any(observed.get(key) != tuple(pair) for key, pair in expect.items()):
                raise RuntimeError("source does not match the declared period plan")
        sleep_marker = markers / f"slept-{index}"
        if sleep_marker.exists():
            elapsed = float(sleep_marker.read_text())
        else:
            elapsed = observe_period(every, time.sleep, time.monotonic)
            sleep_marker.write_text(str(elapsed) + "\n")
            os.chmod(sleep_marker, 0o600)
        capture = run_dir / f"c4-p{index + 1}"
        if not (capture / "source-observation.json").exists():
            run([sys.executable, str(SCRIPTS / "capture_original_partitions.py"), "--output", str(capture),
                 "--connection-config", str(config), "--gold", str(gold_path),
                 "--capture-partitions", "202403", "--generation-start", str(item["generation"])],
                run_dir / "calls" / f"capture-g{item['generation']}", timeout=600)
        load_one(config, run_dir, capture, item["generation"], replay=index == 1)
        done.append({"every": every, "slept_seconds": elapsed, "generation": item["generation"],
                     "change": item["change"], "declared_keys": item["keys"], "capture": capture.name})
    replay = json.loads((run_dir / "c4-p2" / "replay.json").read_text())
    if type(replay.get("before")) is not int or replay.get("before") != replay.get("after"):
        raise RuntimeError("this-run replay changed the physical count")
    generation = plan["periods"][1]["generation"]
    first_load = json.loads((run_dir / "target-raw" / f"load-g{generation}-202403.stdout").read_text())
    replay_load = json.loads((run_dir / "target-raw" / f"replay-g{generation}-202403.stdout").read_text())
    lease = replay_load["jobs_lease_receipt"]
    if (replay_load.get("row_count") != replay["before"] or replay_load.get("watermark_state") != first_load.get("watermark_state")
            or lease.get("id") != first_load["jobs_lease_receipt"].get("id") or lease.get("inserted") is not False
            or replay_load.get("source_generation") != first_load.get("source_generation")):
        raise RuntimeError("replay moved watermark or lease")
    after_periods = partition_state()
    if len([item for item in after_periods if item["source_generation"] in (4, 5) and item["acknowledged"]]) != 2:
        raise RuntimeError("older acknowledged partitions were not retained")
    ch(DBA, "SYSTEM FLUSH LOGS", "oo-ch-default-flush", raw / "flush")
    audit = ch(DBA, "SELECT count() FROM system.query_log WHERE user='oo_dl01w1_40_reader' AND type='QueryFinish' AND query_id LIKE 'oo-ch-default-%' AND (query_kind IN ('Insert','Alter') OR query ILIKE 'insert %' OR query ILIKE 'alter %') FORMAT TSV", "oo-ch-default-reader-audit", raw / "reader-audit")
    reader_writes = int(audit.stdout.decode().strip() or "0")
    if reader_writes:
        raise RuntimeError("reader query log shows a finished write")
    write(run_dir / "source-reader-audit.json", {"reader_query_finish_writes": reader_writes, "reader_insert_exit": denied.returncode})
    write(run_dir / "gold.json", declared[1])
    catalog_capture = run_dir / "c4-p2"
    scope_ids = json.loads((catalog_capture / "admission-202403" / "sealed" / "registry.json").read_text())["bindings"][0]["partition_scope"]["observed_partition_ids"]
    if "209912" not in scope_ids:
        # Gen 10 ACK omitted the retained empty partition, so the vector catalog refuses it.
        catalog_capture = run_dir / "c4-p3"
        generation = 11
        if not (catalog_capture / "source-observation.json").exists():
            run([sys.executable, str(SCRIPTS / "capture_original_partitions.py"), "--output", str(catalog_capture),
                 "--connection-config", str(config), "--gold", str(run_dir / "gold.json"),
                 "--capture-partitions", "202403", "--generation-start", "11"],
                run_dir / "calls" / "capture-g11", timeout=600)
        load_one(config, run_dir, catalog_capture, generation, replay=False)
    if not (run_dir / "materialize-manifest.json").exists():
        run([sys.executable, str(SCRIPTS / "connection_materialize.py"), "--config", str(run_dir / f"load-config-g{generation}.json"),
             "--run", str(run_dir), "--capture", str(catalog_capture), "--gold", str(run_dir / "gold.json")],
            run_dir / "calls" / "materialize", timeout=600)
    manifest = json.loads((run_dir / "materialize-manifest.json").read_text())
    if manifest.get("typed_diff_zero") is not True:
        raise RuntimeError("typed diff is not zero")
    after = partition_state()
    freshness = psql("SELECT source_generation::text||'|'||state||'|'||mode||'|'||(source_committed_at IS NULL)::text FROM _oo.table_freshness WHERE source='clickhouse_connection_whole'")
    watermark = psql("SELECT consistency_grade||'|'||state||'|'||position_codec||'|'||(source_committed_at IS NULL)::text||'|'||cross_table_consistent::text||'|'||source_generation::text FROM _oo.source_watermark WHERE source='clickhouse_connection_whole'")
    grade, _wm_state, _codec, clock_missing, cross_consistent, wm_gen = watermark.strip().split("|")
    if int(wm_gen) != int(manifest["generation"]):
        raise RuntimeError("period watermark does not cover the loaded generation")
    write(run_dir / "cycle-receipt.json", {
        "method": "C4", "delete_detection": "partition_replace",
        "source_commit_clock": None if clock_missing == "true" else watermark.strip(),
        "consistency_grade": grade, "cross_table": "B" if cross_consistent == "false" else cross_consistent,
        "periods": done, "final_keys": sorted(rows()),
        "partitions_before": before["partitions"], "partitions_after": after,
        "freshness": freshness.strip(), "watermark": watermark.strip(),
        "recovery": {"scope": "this-run c4-p2 replay", "before": replay["before"], "after": replay["after"],
                     "second_import": replay["second_import"], "replay_file": "c4-p2/replay.json",
                     "watermark_state": replay_load["watermark_state"],
                     "watermark_state_before_replay": first_load["watermark_state"],
                     "lease_id": lease["id"], "lease_inserted": lease["inserted"],
                     "source_generation": replay_load["source_generation"]},
    })
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


def mounted_here(name, run_dir):
    proc = subprocess.run(["docker", "inspect", "-f", "{{range .Mounts}}{{.Source}}\n{{end}}", name], capture_output=True, text=True, timeout=30)
    return proc.returncode == 0 and str(run_dir) in proc.stdout.splitlines()


def replace_if_other_mount(name, run_dir):
    if state(name) == "absent":
        return
    if mounted_here(name, run_dir) and state(name) == "running":
        return
    run(["docker", "rm", "-f", name], None, timeout=60)


def api_mount_source():
    proc = subprocess.run(["docker", "inspect", "-f", '{{range .Mounts}}{{if eq .Destination "/api"}}{{.Source}}{{end}}{{end}}', API], capture_output=True, text=True, timeout=30)
    return proc.stdout.strip() if proc.returncode == 0 else ""


def start_api(run_dir):
    if sha(MAINT_API) != MAINT_API_SHA:
        raise RuntimeError("maintenance api elf pin mismatch")
    replace_if_other_mount(API, run_dir)
    if state(API) == "running" and api_mount_source() == str(MAINT_API):
        return
    if state(API) != "absent":
        run(["docker", "rm", "-f", API], run_dir / "calls" / "api-replace-maint", timeout=60)
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
         "--mount", f"type=bind,source={MAINT_API},target=/api,readonly",
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


ACL_ELF = Path.home() / ".cache/oo-grok-connection-full-20261014/fixed-acl-50fd9d462/install-column-acl"
ACL_SHA = "aea3dad54671b9608a2ab1a739a2160681bc9cd8f4eacf08a49ce3c45a79cf05"
DIGEST = "12b7cd95e01c621fc79e00007b95f1ae0acb3cbffe8ae0fab06b2b5d55811e43"


def acl_document(generation):
    return {
        "protocol": "oo.gateway.install-native-vector-acl-cli/v1",
        "endpoint": {"host": "127.0.0.1", "port": 5432, "database": "oo_ch_connection", "user": "postgres"},
        "password_file": "/secret/admin.password", "ca_file": "/secret/ca.crt",
        "policies": [{
            "project": "clickhouse_connection_whole", "config_digest": DIGEST, "generation": generation,
            "role": "reader", "login": "oo_ch_connection_login", "row_filter": None,
            "table": {"namespace": "oo_dl01w40", "name": "readings",
                      "columns": [{"name": name, "status": "confirmed", "semantic_type": "source", "pii": {"class": "none"}} for name in COLUMNS]},
        }],
    }


def install_generation_acl(run_dir, generation):
    if sha(ACL_ELF) != ACL_SHA:
        raise RuntimeError("acl elf pin mismatch")
    granted = sorted({int(line) for line in psql(
        "SELECT DISTINCT m.run_generation::text FROM information_schema.column_privileges p "
        "JOIN _oo.physical_mappings m ON m.physical_name=p.table_name AND m.level='t' "
        "AND m.source='clickhouse_connection_whole' WHERE p.grantee='oo_r_reader'").splitlines() if line.strip()})
    if granted == [generation]:
        return
    if granted != [6] or generation <= 6:
        raise RuntimeError("acl grants are not the committed generation")
    folder = run_dir / "acl"
    folder.mkdir(mode=0o700, exist_ok=True)
    previous = folder / "previous.json"
    if not previous.exists():
        write(previous, acl_document(6))
    psql("REVOKE SELECT ON TABLE _oo.apply_receipts, _oo.full_import_quality, _oo.full_import_run, _oo.full_import_table, "
         "_oo.physical_mappings, _oo.source_watermark, _oo.table_freshness, _oo.table_quality FROM oo_r_reader",
         "oo_ch_connection", run_dir / "calls" / "acl-revoke-table")
    proc = subprocess.run(["docker", "run", "-i", "--rm", "--pull", "never", "--platform", "linux/arm64",
                           "--user", f"{os.getuid()}:{os.getgid()}", "--network", "container:" + TARGET,
                           "--mount", f"type=bind,source={ACL_ELF},target=/acl/install-column-acl,readonly",
                           "--mount", f"type=bind,source={TARGET_PRIVATE}/admin.password,target=/secret/admin.password,readonly",
                           "--mount", f"type=bind,source={TARGET_PRIVATE}/tls/ca.crt,target=/secret/ca.crt,readonly",
                           "--mount", f"type=bind,source={folder},target=/policies,readonly",
                           "--entrypoint", "/acl/install-column-acl", IMAGE,
                           "--previous-policy-file", "/policies/previous.json",
                           "--previous-policy-sha256", sha(previous)],
                          input=json.dumps(acl_document(generation)).encode(), capture_output=True, timeout=60)
    kept(proc, run_dir / "calls" / "acl-transition")
    if proc.returncode:
        raise RuntimeError("acl transition refused")
    write(run_dir / "acl-transition.json", json.loads(proc.stdout))


def gateway(config, run_dir):
    if (run_dir / "gateway-ready.json").exists() and state(GATEWAY) == "running" and mounted_here(GATEWAY, run_dir):
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
    login = "oo_ch_connection_login"
    secret(PRIVATE / "data.password")
    psql(role_sql(login, PRIVATE / "data.password", "NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION"), "postgres", run_dir / "calls" / "gw-login")
    psql("DO $$ BEGIN IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname='oo_r_reader') THEN CREATE ROLE oo_r_reader NOLOGIN NOINHERIT; END IF; END $$;\n"
         "GRANT CONNECT ON DATABASE oo_ch_connection TO oo_ch_connection_login;\n"
         "GRANT USAGE ON SCHEMA _oo TO oo_ch_connection_login, oo_r_reader;\n"
         "GRANT EXECUTE ON FUNCTION _oo.lookup_physical_mapping(text, text, bigint, text, text, text, text) TO oo_ch_connection_login, oo_r_reader;\n"
         "GRANT oo_r_reader TO oo_ch_connection_login WITH INHERIT FALSE, SET TRUE;\n"
         "DO $$ DECLARE rel text; BEGIN FOREACH rel IN ARRAY ARRAY['physical_mappings','table_freshness','source_watermark','table_quality','apply_receipts','full_import_run','full_import_table','full_import_quality'] LOOP "
         "IF EXISTS (SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='_oo' AND c.relname=rel) THEN "
         "EXECUTE format('GRANT SELECT ON _oo.%I TO oo_ch_connection_login', rel); END IF; END LOOP; END $$;\n",
         "oo_ch_connection", run_dir / "calls" / "gw-grant")
    install_generation_acl(run_dir, max(item["source_generation"] for item in vector))
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
    replace_if_other_mount(GATEWAY, run_dir)
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
    expected = [(row["reading_id"], row["version"], row["qty"]) for row in json.loads((run_dir / "gold.json").read_text())]
    expected.sort(key=lambda row: int(row[0]))
    if got != expected:
        raise RuntimeError("gateway rows differ")
    cancel(run_dir, physical, common)
    write(run_dir / "dialogue-truth.json", {"qty": {"expected_named_facts": {"qty": "5"}}, "inserted": {"expected_named_facts": {"reading_id": "105"}}})
    write(run_dir / "dialogue-config.json", {
        "oo_cli": str(OO_CLI), "gateway": "http://" + listen, "transport": "local-development", "auth": "external",
        "gateway_token_file": str(token), "model_origin": "http://127.0.0.1:8317", "model": "grok-4.7",
        "model_bearer_file": str(BEARER), "independent_truth_file": str(run_dir / "dialogue-truth.json"),
        "independent_gateway_audit_file": str(folder / "wal"), "max_tool_rounds": 12,
        "questions": [
            {"truth_key": "qty", "prompt": "In the first tool response, call all four together: knowledge_search query readings, knowledge_cat path INDEX.md, describe table oo_dl01w40.readings, and freshness. Then query SELECT qty FROM oo_dl01w40.readings WHERE reading_id = 105."},
            {"truth_key": "inserted", "prompt": "Query SELECT reading_id FROM oo_dl01w40.readings WHERE qty = 5. Return the observed reading_id."},
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
    recovery = cycle_doc.get("recovery") or {}
    periods = cycle_doc.get("periods") or []
    period_ok = len(periods) == 2 and periods[0]["every"] != periods[1]["every"] and periods[1]["change"].startswith("physical delete")
    wm = (cycle_doc.get("watermark") or "").split("|")
    disclosed = len(wm) == 6 and wm[0] == "S0" and wm[3] == "true" and wm[4] == "false" and cycle_doc.get("source_commit_clock") is None
    recovery_ok = (recovery.get("scope") == "this-run c4-p2 replay" and recovery.get("before") == recovery.get("after")
                   and recovery.get("second_import") in {"accepted", "refused_count_unchanged"}
                   and recovery.get("watermark_state") == recovery.get("watermark_state_before_replay")
                   and recovery.get("lease_inserted") is False and bool(recovery.get("lease_id")))
    ok = (receipt.get("protocol") == "oo.source-onboarding.product/v1" and verdict.get("outcome") == "verified"
          and diff.get("diff_zero") is True and period_ok and recovery_ok and ready.get("query_id") and disclosed)
    if not ok:
        raise RuntimeError("whole proofs incomplete")
    write(run_dir / "whole.json", {
        "full_product_chain_complete": True,
        "periodic_insert_update_delete_verified": period_ok,
        "recovery_verified": recovery_ok,
        "product_receipt": "product/product-receipt.json",
        "dialogue_verdict": "dialogue/verdict.json",
        "published": False,
    })


def progress_row(catalog):
    if catalog.get("protocol") != "oo.pipeline.sealed-import-collection/v1":
        raise RuntimeError("catalog is not the sealed collection")
    if "source_generation_high_watermark" in catalog or "target_generation_high_watermark" in catalog:
        raise RuntimeError("high watermark is not the progress record")
    entry, table = catalog["entries"][0], catalog["entries"][0]["tables"][0]
    row = {"dataset": table["logical_schema"] + "." + table["logical_table"], "source_id": entry["source_id"],
           "source_generation": entry["source_generation"], "target_generation": entry["target_generation"],
           "run_id": entry["run_id"], "input_digest": entry["input_digest"], "stored_physical": table["stored_physical"],
           "schema_digest": entry["schema_digest"], "type_digest": entry["type_digest"], "engine": entry["engine"]}
    vector = catalog["binding_vector"]
    if len(vector) != 1 or any(vector[0].get(key) != row[key] for key in ("dataset", "source_id", "source_generation", "target_generation", "run_id", "input_digest")):
        raise RuntimeError("binding vector does not match the table row")
    return row


def schema_drift(previous, current):
    drift = {}
    for key in ("schema_digest", "type_digest", "source_id", "engine"):
        if previous[key] != current[key]:
            drift[key] = [previous[key], current[key]]
    return drift


def cover_current(config, run_dir):
    """One configured period over the loaded generation, then equal-generation rebind. No new DML or model call."""
    import ssl
    import urllib.error
    import urllib.request
    if sha(MAINT_API) != MAINT_API_SHA:
        raise RuntimeError("maintenance api elf pin mismatch")
    smoke = subprocess.run(["docker", "run", "--rm", "--pull", "never", "--platform", "linux/arm64",
                            "--mount", f"type=bind,source={MAINT_API},target=/api,readonly",
                            "--entrypoint", "/api", IMAGE], capture_output=True, timeout=60)
    if smoke.returncode != 1:
        raise RuntimeError("linux maintenance api did not refuse empty argv")
    catalog = json.loads((run_dir / "bundle" / "catalog.json").read_text())
    command = json.loads((run_dir / "work" / "builds" / "b_e40e75018cac19da2b3f7b29fcc03cc7" / "native-model-output" / "command.json").read_text())
    current, original = progress_row(catalog), progress_row(command["catalog"])
    live = psql("SELECT run_id||'|'||source_generation::text||'|'||target_generation::text||'|'||input_digest||'|'||physical_name "
                "FROM _oo.full_import_run r JOIN _oo.physical_mappings m ON m.source=r.source_id AND m.level='t' AND m.run_generation=r.target_generation "
                "WHERE r.run_id='%s'" % current["run_id"]).strip()
    expect = "|".join(str(current[key]) for key in ("run_id", "source_generation", "target_generation", "input_digest", "stored_physical"))
    if live != expect:
        raise RuntimeError("live import row is not the sealed catalog")
    drift = schema_drift(original, current)
    cover = run_dir / "maintenance-cover"
    cover.mkdir(mode=0o700, exist_ok=True)
    if drift:
        write(cover / "schema-block.json", {"blocked": True, "drift": drift, "llm_calls": 0})
        raise RuntimeError("schema or semantic identity changed; maintenance refused")
    from c4_capture import observe_period
    every = json.loads((run_dir / "period-plan.json").read_text())["periods"][0]["every"]
    slept = observe_period(every, time.sleep, time.monotonic)
    watermark = psql("SELECT consistency_grade||'|'||state||'|'||position_codec||'|'||(source_committed_at IS NULL)::text||'|'||cross_table_consistent::text||'|'||source_generation::text FROM _oo.source_watermark WHERE source='clickhouse_connection_whole'").strip()
    pending = int(ch(DBA, "SELECT count() FROM system.mutations WHERE database='oo_dl01w40' AND is_done=0 FORMAT TSV", "oo-ch-cover-mutations", None).stdout.decode().strip() or "0")
    if pending:
        write(cover / "pending-resource.json", {"pending_mutations": pending, "marker": "retained", "completed": False})
        raise RuntimeError("source mutation still pending")
    marker = Path("/tmp/oo-clickhouse-connection-whole/whole-run/cycle-markers/pending")
    if not watermark.endswith("|" + str(current["source_generation"])):
        raise RuntimeError("live watermark does not cover the sealed generation")
    product_config_doc = json.loads((run_dir / "product-config.json").read_text())
    operator = ROOT / "tools/selfbuild-control-api/recipes/operator.py"
    if sha(operator) != product_config_doc["operator_sha256"]:
        raise RuntimeError("operator pin drifted")
    start_api(run_dir)
    wait_api()
    context = ssl.create_default_context(cafile=product_config_doc["identity_ca"])
    try:
        urllib.request.urlopen(urllib.request.Request(product_config_doc["identity_origin"].rstrip("/") + "/control/v1/identity/current"), context=context, timeout=30)
        anonymous = 200
    except urllib.error.HTTPError as error:
        anonymous = error.code
    if anonymous != 401:
        raise RuntimeError("anonymous identity was not refused")
    cookie = Path(product_config_doc["cookies"]["admin"]).read_text().strip()
    identity_req = urllib.request.Request(product_config_doc["identity_origin"].rstrip("/") + "/control/v1/identity/current", headers={"Cookie": "__Host-oo-session=" + cookie})
    with urllib.request.urlopen(identity_req, context=context, timeout=30) as response:
        identity = json.loads(response.read().decode())
    if identity.get("install_state") != "initialized" or not any(item.get("project_id") == "clickhouse" and item.get("role") == "admin" for item in identity.get("projects") or []):
        raise RuntimeError("admin session is not the clickhouse project")
    revision = int(identity["revision"])

    def control(name, path, body):
        target = cover / f"{name}.response.json"
        if target.exists():
            raise RuntimeError("refusing to overwrite " + name)
        payload = dict(body, identity_revision=revision, idempotency_key=secrets.token_hex(16))
        body_path = cover / f"{name}.body.json"
        write(body_path, payload)
        proc = subprocess.run([sys.executable, str(operator), "--endpoint", product_config_doc["endpoint"], "--origin", product_config_doc["origin"],
                               "--ca", product_config_doc["ca"], "--cookie-file", product_config_doc["cookies"]["admin"],
                               "--identity-origin", product_config_doc["identity_origin"], "--identity-ca", product_config_doc["identity_ca"],
                               "--path", path, "--body-file", str(body_path), "--out", str(target)], capture_output=True, timeout=70)
        kept(proc, cover / name)
        saved = json.loads(target.read_text())
        if saved.get("status", 500) >= 400:
            code = saved.get("response", {}).get("error") or saved.get("response", {}).get("detail_code") or saved["status"]
            raise RuntimeError("maintenance api refused " + str(code))
        return saved["response"]

    catalog_sha = sha(run_dir / "bundle" / "catalog.json")
    maintained = control("maintenance", "/control/v1/builds/b_e40e75018cac19da2b3f7b29fcc03cc7/maintenance",
                         {"catalog_sha256": catalog_sha, "trigger": "source_generation_advanced"})
    packed = control("role-pack", "/control/v1/builds/b_e40e75018cac19da2b3f7b29fcc03cc7/role-pack",
                     {"policy_version": "oos/v1", "role": "oo_r_reader"})
    native = maintained.get("native_maintenance") or {}
    if native.get("llm_calls") != 0 or native.get("publication") != "not_published":
        raise RuntimeError("maintenance was not a zero-model unpublished rebind")
    install_generation_acl(run_dir, int(current["source_generation"]))
    folder = run_dir / "gateway"
    listen = (folder / "listen.addr").read_text().strip()
    common = ["--json", "--gateway", "http://" + listen, "--transport", "local-development", "--auth", "external", "--token-file", str(folder / "client.token")]
    probed = subprocess.run([str(OO_CLI), "query", "SELECT reading_id, version, qty FROM oo_dl01w40.readings ORDER BY reading_id", "--max-rows", "5", "--timeout-ms", "8000", *common], capture_output=True, timeout=40)
    kept(probed, cover / "gateway-probe")
    if probed.returncode:
        raise RuntimeError("gateway probe failed")
    body = json.loads(probed.stdout)
    got = [(row[0], row[1], row[2]) for row in body["rows"]]
    expected = sorted(((row["reading_id"], row["version"], row["qty"]) for row in json.loads((run_dir / "gold.json").read_text())), key=lambda row: int(row[0]))
    if got != expected:
        raise RuntimeError("gateway rows differ")
    write(cover / "receipt.json", {
        "method": "C4", "every": every, "slept_seconds": slept, "watermark": watermark, "pending_mutations": pending,
        "historical_pending_marker": marker.read_text() if marker.is_file() else None, "historical_pending_marker_retained": marker.is_file(),
        "progress": {key: current[key] for key in ("dataset", "source_generation", "target_generation", "run_id", "input_digest", "stored_physical")},
        "catalog_sha256": catalog_sha, "maintenance_llm_calls": native["llm_calls"], "publication": native["publication"],
        "role_pack_status": packed.get("operation") or packed.get("protocol"), "query_id": body["query_id"], "rows": got,
        "api_elf_sha256": MAINT_API_SHA, "producer_sha256": sha(ROOT / "tools/source-onboarding/product_flow.py"),
        "producer_gate": "onboard.py prove_whole source_not_fresh compares product.producer_sha256 to product_flow.py",
    })


def self_check():
    from connection_load import replay_ok
    from c4_capture import observe_period
    assert replay_ok(1, 1) and not replay_ok(1, 2)
    sample = {"protocol": "oo.pipeline.sealed-import-collection/v1", "entries": [{"source_id": "s", "source_generation": 11, "target_generation": 11, "run_id": "r", "input_digest": "d", "schema_digest": "a", "type_digest": "t", "engine": "clickhouse", "tables": [{"logical_schema": "n", "logical_table": "readings", "stored_physical": "p"}]}], "binding_vector": [{"dataset": "n.readings", "source_id": "s", "source_generation": 11, "target_generation": 11, "run_id": "r", "input_digest": "d"}]}
    assert progress_row(sample)["stored_physical"] == "p" and schema_drift(progress_row(sample), progress_row(sample)) == {}
    changed = json.loads(json.dumps(sample))
    changed["entries"][0]["schema_digest"] = "b"
    assert schema_drift(progress_row(sample), progress_row(changed)) == {"schema_digest": ["a", "b"]}
    periods = configured_periods({"sync": "partition_refresh", "every": "3s", "periods": ["3s", "7s"], "delete_detection": {"mode": "partition_replace"}})
    fresh = period_plan(True, periods)[1]["periods"]
    assert [item["every"] for item in fresh] == ["3s", "7s"] and fresh[0]["generation"] == 9 and fresh[1]["change"] == "physical delete 107"
    assert "105" not in fresh[0]["change"] and "106" not in fresh[0]["change"]
    slept = []
    observe_period(periods[0], slept.append, lambda: 0)
    observe_period(periods[1], slept.append, lambda: 0)
    assert slept == [3.0, 7.0]
    if sha(ACL_ELF) != ACL_SHA:
        raise RuntimeError("acl elf pin mismatch")
    usage = subprocess.run(["docker", "run", "--rm", "--pull", "never", "--platform", "linux/arm64",
                            "--mount", f"type=bind,source={ACL_ELF},target=/acl/install-column-acl,readonly",
                            "--entrypoint", "/acl/install-column-acl", IMAGE, "--help"], capture_output=True, timeout=60)
    if usage.returncode != 2 or b"usage" not in usage.stderr:
        raise RuntimeError("linux acl elf did not refuse usage")
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
    {"data": data, "product-config": product_config, "decisions": decisions, "gateway": gateway, "finish": finish, "cover": cover_current}[command](config, run_dir)


if __name__ == "__main__":
    if len(sys.argv) == 2 and sys.argv[1] == "self-check":
        self_check()
    else:
        main()
