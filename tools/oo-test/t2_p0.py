#!/usr/bin/env python3
"""Owned MySQL/Oracle T2 cycles against a real Agent DB; no acceptance by exit code alone."""
import argparse
import hashlib
import copy
import threading
import json
from pathlib import Path
import shutil
import signal
import uuid

import runner
import t1
from t2 import c1_summaries, seed_scale
from verdicts import incremental, write_rows
from t1 import AgentDB, LaneRun, SourceMeter, configure_mysql, private

ROOT = runner.ROOT
MYSQL = {"5.7": "5.7.44", "8.0": "8.0.44", "8.4": "8.4.7", "9": "9.5.0"}
TARGET_FORKS = ("SELECT c.relname,pg_relation_size(c.oid,'main'),"
                "pg_relation_size(c.oid,'fsm'),pg_relation_size(c.oid,'vm') "
                "FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace "
                "WHERE n.nspname='_oo_load' AND c.relkind='r' ORDER BY c.relname;")

class SyncRun(LaneRun):
    def command(self, command, **kwargs):
        if command[:2] == ["docker", "create"] and self.name in command and self.engine == "mysql":
            command = [*command, "--server-id=41", "--log-bin=oo-bin", "--binlog-format=ROW", "--binlog-row-image=FULL"]
        return super().command(command, **kwargs)

    def setup(self):
        # Resolve the same immutable lock as every other suite, never a local tag.
        super().setup()


def rows(text):
    return [json.loads(line) for line in text.splitlines() if line.startswith("{")]


def canonical(text):
    return [tuple(part.strip() for part in line.replace("\t", "|").split("|"))
            for line in text.splitlines() if line.strip()]


def run_cycles(run, binary, worker_image):
    report = {"T2": "not_passed", "C1": "not_run", "C5": "not_run", "handoff": "not_run",
              "baseline_rows_per_table": run.args.scale,
              "cycles": [], "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "scope": f"two keyed numeric/text tables; {run.args.scale} baseline rows each; not full type coverage",
              "consistency": "B"}
    store = run.path / ".sync-private"
    store.mkdir(mode=0o700)
    target = AgentDB(run)
    worker = None
    writer = None
    try:
        run.setup()
        worker_args = []
        if run.engine == "mysql":
            for table in ("items", "other"):
                run.sql(f"CREATE TABLE {table}(id bigint PRIMARY KEY,payload varchar(80) NOT NULL) DEFAULT CHARSET=utf8mb4;"
                        f"INSERT INTO {table} VALUES(1,'baseline');", admin=True)
            seed_scale(run, "fixture", ["items", "other"])
            configure_mysql(run, store)
            run.sql("GRANT REPLICATION SLAVE,REPLICATION CLIENT ON *.* TO 'fixture_reader'@'%';", admin=True)
            _, epoch = run.sql("SELECT @@GLOBAL.server_uuid;", admin=True)
            _, position = run.sql("SHOW MASTER STATUS;" if run.version in ("5.7", "8.0") else
                                  "SHOW BINARY LOG STATUS;", admin=True)
            fields = position.split()
            sync = {"initial_position": {"kind": "mysql_file_pos", "file": fields[0], "position": int(fields[1])},
                    "source_epoch": epoch.strip(), "baseline_aligned": True, "max_batches": 100,
                    "server_id": 42, "longest_cycle_seconds": 1, "recovery_seconds": 1}
            schema, service, port_number = "fixture", "fixture", 3306
        else:
            run.seed()
            schema = "FIXTURE"
            service = "XE" if run.version == "11" else ("XEPDB1" if run.version == "21" else "FREEPDB1")
            port_number = 1521
            for table in ("ITEMS", "OTHER"):
                run.sql(f"CREATE TABLE {table}(id NUMBER(18) PRIMARY KEY,payload VARCHAR2(80) NOT NULL);\n"
                        f"INSERT INTO {table} VALUES(1,'baseline');\nCOMMIT;\n"
                        f"GRANT SELECT,FLASHBACK ON {table} TO fixture_reader;", admin=True)
            run.sql("GRANT SELECT ON SYS.V_$DATABASE TO fixture_reader;\n"
                    "GRANT SELECT ON SYS.V_$SESSION TO fixture_reader;\n"
                    "GRANT SELECT ON SYS.V_$SQL_PLAN TO fixture_reader;\n"
                    "GRANT SELECT ON SYS.DBA_EXTENTS TO fixture_reader;\n"
                    "GRANT EXECUTE ON SYS.DBMS_ROWID TO fixture_reader;", admin=True)
            sync = None
        _, raw = run.command(["docker", "port", run.name, f"{port_number}/tcp"])
        port = int(raw.strip().rsplit(":", 1)[1])
        if run.engine == "oracle" and run.version == "11":
            if not worker_image:
                raise RuntimeError("11g thick worker image required")
            worker = "oo-m-worker-" + uuid.uuid4().hex[:12]
            run.command(["docker", "create", "--name", worker, "--platform", "linux/amd64",
                         "--label", "oo.lane=" + t1.LANE, "--label", "oo.run_id="+run.id,
                         "--network", "container:"+run.name, "--mount",
                         f"type=bind,src={ROOT / 'testing/engines/oracle/worker/worker.py'},dst=/worker.py,readonly",
                         "--entrypoint", "sleep", worker_image, "infinity"])
            run.command(["docker", "start", worker])
            executable = store / "worker"
            private(executable, f'#!/bin/sh\nexec docker exec -i {worker} python /worker.py\n')
            executable.chmod(0o700)
            worker_args = ["--oracle-worker", str(executable)]
            port = 1521
        target.start()
        # Also redact the separately generated Agent DB credential in raw command output.
        private(store / "target-password", target.password)
        private(store / "source-password", run.reader_secret)
        tables = ["items", "other"] if run.engine == "mysql" else ["ITEMS", "OTHER"]
        source = {"id": "fixture", "engine": run.engine, "host": "127.0.0.1", "port": port,
                  "database": service, "user": "fixture_reader", "password": {"secret_ref": "source-password"},
                  "scope": {"include": [schema+"."+t for t in tables], "exclude": []}}
        if run.engine == "mysql":
            source["tls_ca"] = {"secret_ref": "source-ca"}
        stack = {"version": "oo.stack/v1", "sources": [source], "model": {
            "provider": "openai-compatible", "base_url": "https://example.invalid/v1", "model": "unused",
            "key": {"secret_ref": "unused"}, "context_window": "auto"}}
        config = {"host": "127.0.0.1", "port": target.port, "database": "postgres", "user": "postgres",
                  "password": {"secret_ref": "target-password"}, "local_fixture": True, "run_id": "base"}
        command = [str(binary), "--stack", str(store / "stack.json"), "--source", "fixture", "--target",
                   str(store / "target.json"), "--secrets-dir", str(store), *worker_args]
        for mode in (["C1", "C5"] if sync else ["C5"]):
            source["id"] = "fixture_"+mode.lower()
            command[command.index("--source")+1] = source["id"]
            runner.write_json(store / "stack.json", stack)
            config["run_id"] = mode+"-baseline"
            runner.write_json(store / "target.json", config)
            _, text = run.command(command, timeout=300)
            relations = {r["table"]["name"]: r["target"] for r in rows(text)}
            previous = None
            for cycle in range(3):
                if cycle < 2:
                    for table in tables:
                        key = (100 if mode == "C1" else 1000)+cycle*10
                        run.sql(f"INSERT INTO {schema}.{table} VALUES({key},'new');\n"
                                f"UPDATE {schema}.{table} SET id={key+1},payload='updated' WHERE id={key};\n"
                                f"DELETE FROM {schema}.{table} WHERE id=1;\nCOMMIT;", admin=True)
                policy = sync if mode == "C1" else {"cycle": cycle+1, "full_read_every": 2}
                runner.write_json(store / "cycle.json", policy)
                config["run_id"] = f"{mode}-{cycle}"
                runner.write_json(store / "target.json", config)
                if cycle == 2 and run.args.scale > 1:
                    # Settle target visibility/free-space maps before the strict
                    # zero-growth assertion; do not disable source autovacuum.
                    target.sql("VACUUM;")
                    report.setdefault("noop_target_maintenance", []).append(mode)
                    target.sql(TARGET_FORKS)
                before = target.sql("SELECT COALESCE(sum(pg_total_relation_size(c.oid)),0) FROM pg_class c "
                                    "JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='_oo_load' AND c.relkind='r';")
                with SourceMeter(run, (0 if mode == "C1" else 10)+cycle, 3 if mode == "C1" else 1) as meter:
                    _, text = run.command(command + ["--sync" if mode == "C1" else "--c5", str(store / "cycle.json")], timeout=600)
                receipts = c1_summaries(text) if mode == "C1" else rows(text)
                comparisons = {}
                for table, relation in relations.items():
                    _, expected = run.sql(f"SELECT id,payload FROM {schema}.{table} ORDER BY id;")
                    actual = target.sql(f'SELECT "{ "id" if run.engine == "mysql" else "ID"}",'
                                        f'"{ "payload" if run.engine == "mysql" else "PAYLOAD"}" FROM {relation} ORDER BY 1;')
                    equal = canonical(expected) == canonical(actual)
                    comparisons[table] = {"equal": equal,
                        "source_sha256": hashlib.sha256(json.dumps(canonical(expected)).encode()).hexdigest(),
                        "target_sha256": hashlib.sha256(json.dumps(canonical(actual)).encode()).hexdigest()}
                    if not equal:
                        raise RuntimeError("source target difference")
                after = target.sql("SELECT COALESCE(sum(pg_total_relation_size(c.oid)),0) FROM pg_class c "
                                   "JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='_oo_load' AND c.relkind='r';")
                if cycle == 2 and run.args.scale > 1:
                    target.sql(TARGET_FORKS)
                for receipt in receipts:
                    if mode == "C1":
                        p = receipt["durable_position"]
                        current = (p["file"], p["position"])
                        if previous is not None and current < previous:
                            raise RuntimeError("position regressed")
                        previous = current
                    if cycle == 2 and (receipt["target_write_bytes"] or receipt["new_generations"] or receipt.get("payload_bytes", 0)):
                        raise RuntimeError("no-change rewrote target")
                if cycle == 2 and before != after:
                    raise RuntimeError("no-change physical target rewrite")
                result = {"mode": mode, "cycle": cycle+1, "receipts": receipts, "comparison": comparisons,
                          "T8": meter.result(), "target_bytes_before": before.strip(), "target_bytes_after": after.strip()}
                report["cycles"].append(result)
                runner.write_json(run.path / "t2.json", report)
                if result["T8"]["status"] != "passed":
                    raise RuntimeError("T8 budget")
            report[mode] = "passed"
            if mode == "C1":
                before = target.sql("SELECT source,lineage,position,generation FROM _oo_load.sync_state ORDER BY source;")
                bad = dict(sync, source_epoch="different-epoch")
                runner.write_json(store / "cycle.json", bad)
                code, _ = run.command(command + ["--sync", str(store / "cycle.json"),
                    "--run-id", "different-epoch"], required=False)
                error = json.loads((run.path / f"{run.seq:04d}.stderr").read_text())["error"]
                if not code or error != "sync_rebaseline_required_lineage":
                    raise RuntimeError("different epoch admitted")
                after = target.sql("SELECT source,lineage,position,generation FROM _oo_load.sync_state ORDER BY source;")
                if before != after:
                    raise RuntimeError("epoch rejection modified target")
                report["epoch_negative"] = {"error": error, "target_unchanged": True}
        if run.engine == "mysql":
            handoff_schema = "fixture"
            if run.args.scale > 100000:
                # Full-scale C1/C5 remains intact. The repair contract is bounded;
                # prove refusal before running its original small positive case.
                state_sql = ("SELECT 'current',row_to_json(c)::text FROM _oo_load.current c UNION ALL "
                             "SELECT 'publication',row_to_json(p)::text FROM _oo_load.publications p UNION ALL "
                             "SELECT 'sync',row_to_json(s)::text FROM _oo_load.sync_state s ORDER BY 1,2;")
                before = target.sql(state_sql)
                runner.write_json(store / "oversized-handoff.json", {
                    "sources": [{"source": source["id"], "server_id": 45}],
                    "max_keys": 100000, "max_bytes": 33554432, "required_history_seconds": 2})
                code, _ = run.command(command + ["--handoff", str(store / "oversized-handoff.json"),
                    "--run-id", "oversized-handoff"], required=False, timeout=600)
                error = json.loads((run.path / f"{run.seq:04d}.stderr").read_text())["error"] if code else None
                if (not code or error != "source error: budget fact=mysql_repair_staging_budget"
                        or target.sql(state_sql) != before):
                    raise RuntimeError("oversized handoff did not refuse atomically")
                report["oversized_handoff"] = {"error": error, "target_unchanged": True}
                handoff_schema = "fixture_handoff"
                run.sql("CREATE DATABASE fixture_handoff;"
                        "CREATE TABLE fixture_handoff.items LIKE fixture.items;"
                        "CREATE TABLE fixture_handoff.other LIKE fixture.other;"
                        "INSERT INTO fixture_handoff.items VALUES(1,'baseline');"
                        "INSERT INTO fixture_handoff.other VALUES(1,'baseline');"
                        "GRANT SELECT,SHOW VIEW ON fixture_handoff.* TO 'fixture_reader'@'%';", admin=True)
                source = dict(source, id="fixture_bounded_handoff", database=handoff_schema,
                              scope={"include": [handoff_schema+"."+t for t in tables], "exclude": []})
                stack["sources"].append(source)
                runner.write_json(store / "stack.json", stack)
                command[command.index("--source")+1] = source["id"]
                config["run_id"] = "bounded-handoff-base"
                runner.write_json(store / "target.json", config)
                run.command(command, timeout=300)
                runner.write_json(store / "bounded-c5.json", {"cycle": 1, "full_read_every": 2})
                run.command(command + ["--c5", str(store / "bounded-c5.json"),
                    "--run-id", "bounded-handoff-c5"], timeout=300)
                report["handoff_scale"] = "bounded separate fixture; full-scale source preserved"
            first = source["id"]
            run.sql("CREATE DATABASE fixture_peer;"
                    "CREATE TABLE fixture_peer.items(id bigint PRIMARY KEY,payload varchar(80) NOT NULL) DEFAULT CHARSET=utf8mb4;"
                    "CREATE TABLE fixture_peer.other LIKE fixture_peer.items;"
                    "INSERT INTO fixture_peer.items VALUES(1,'peer-baseline');"
                    "INSERT INTO fixture_peer.other VALUES(1,'peer-other');"
                    "GRANT SELECT,SHOW VIEW ON fixture_peer.* TO 'fixture_reader'@'%';", admin=True)
            second = dict(source, id="fixture_second", database="fixture_peer",
                          scope={"include": ["fixture_peer.items", "fixture_peer.other"], "exclude": []})
            stack["sources"].append(second)
            runner.write_json(store / "stack.json", stack)
            command[command.index("--source")+1] = second["id"]
            config["run_id"] = "second-base"
            runner.write_json(store / "target.json", config)
            run.command(command, timeout=300)
            config["run_id"] = "second-c5"
            runner.write_json(store / "target.json", config)
            runner.write_json(store / "second-c5.json", {"cycle": 1, "full_read_every": 2})
            _, second_c5 = run.command(command + ["--c5", str(store / "second-c5.json")], timeout=300)
            report["second_source_c5"] = rows(second_c5)
            command[command.index("--source")+1] = first
            config["run_id"] = "handoff"
            runner.write_json(store / "target.json", config)
            runner.write_json(store / "handoff.json", {
                "sources": [{"source": first, "server_id": 45}, {"source": second["id"], "server_id": 46}],
                "max_keys": 100000, "max_bytes": 33554432, "required_history_seconds": 2})
            # A failure on the second member must roll back the first publication and cursor.
            state_sql = ("SELECT 'current',row_to_json(c)::text FROM _oo_load.current c UNION ALL "
                         "SELECT 'publication',row_to_json(p)::text FROM _oo_load.publications p UNION ALL "
                         "SELECT 'sync',row_to_json(s)::text FROM _oo_load.sync_state s ORDER BY 1,2;")
            before = target.sql(state_sql)
            missing = dict(source, id="fixture_missing")
            stack["sources"].append(missing)
            runner.write_json(store / "stack.json", stack)
            runner.write_json(store / "failed-handoff.json", {
                "sources": [{"source": first, "server_id": 45}, {"source": missing["id"], "server_id": 46}],
                "max_keys": 100000, "max_bytes": 33554432, "required_history_seconds": 2})
            code, _ = run.command(command + ["--handoff", str(store / "failed-handoff.json")], required=False, timeout=600)
            error = json.loads((run.path / f"{run.seq:04d}.stderr").read_text())["error"] if code else None
            if not code or error != "load_target" or target.sql(state_sql) != before:
                raise RuntimeError("handoff did not roll back incomplete scope")
            report["handoff_rollback"] = {"second_source_missing_baseline": True, "target_state_unchanged": True}
            stack["sources"].pop()
            runner.write_json(store / "stack.json", stack)
            writer_run = copy.copy(run)
            writer_run.seq = 5000000
            writer_errors = []
            def write_handoff():
                try:
                    sql = "\n".join(f"START TRANSACTION; INSERT INTO items VALUES({20000+i},'during-handoff'); "
                                    f"INSERT INTO other VALUES({20000+i},'paired'); COMMIT; DO SLEEP(0.05);"
                                    for i in range(24))
                    writer_run.sql(f"USE {handoff_schema};\n" + sql, admin=True)
                except Exception as error:
                    writer_errors.append(type(error).__name__)
            writer = threading.Thread(target=write_handoff)
            writer.start()
            with SourceMeter(run, 20, 3) as meter:
                _, text = run.command(command + ["--handoff", str(store / "handoff.json")], timeout=600)
            writer.join(timeout=60)
            if writer.is_alive() or writer_errors:
                raise RuntimeError("handoff writer failed")
            handoff = rows(text)[0]
            if len(handoff["positions"]) != 2 or len(handoff["bindings"]) != 4 or not handoff["scope_committed_atomically"]:
                raise RuntimeError("incomplete handoff scope")
            for schema_name in (handoff_schema, "fixture_peer"):
                for table in tables:
                    run.sql(f"INSERT INTO {schema_name}.{table} VALUES(9999,'after-handoff');COMMIT;", admin=True)
            for identifier, position in handoff["positions"].items():
                command[command.index("--source")+1] = identifier
                runner.write_json(store / "cycle.json", dict(sync, initial_position=position,
                    source_epoch=handoff["source_epochs"][identifier]))
                run.command(command + ["--sync", str(store / "cycle.json")], timeout=300)
                for item in handoff["bindings"]:
                    if item["source"] == identifier:
                        _, expected = run.sql(f"SELECT id,payload FROM {item['table']['schema']}.{item['table']['name']} ORDER BY id;")
                        actual = target.sql(f"SELECT id,payload FROM {item['target']} ORDER BY id;")
                        if canonical(expected) != canonical(actual):
                            raise RuntimeError("handoff continuation difference")
            report["handoff"] = {"status": "passed", "receipt": handoff, "continuation_equal": True,
                                 "T8": meter.result(), "source_basis": "two distinct source databases on one physical fixture"}
            if report["handoff"]["T8"]["status"] != "passed":
                raise RuntimeError("handoff T8 budget")
        if run.engine == "oracle":
            _, epoch = run.sql("SELECT TO_CHAR(dbid)||':'||TO_CHAR(resetlogs_change#) FROM v$database;", admin=True)
            _, scn = run.sql("SELECT TO_CHAR(current_scn) FROM v$database;", admin=True)
            policy = {"initial_position": {"kind": "oracle_scn", "scn": int(scn.strip())},
                      "source_epoch": epoch.strip(), "baseline_aligned": True, "max_batches": 3}
            runner.write_json(store / "cycle.json", policy)
            code, _ = run.command(command + ["--sync", str(store / "cycle.json")], required=False)
            error = json.loads((run.path / f"{run.seq:04d}.stderr").read_text())["error"]
            if not code or error != "oracle_logminer_explicit_opt_in_required":
                raise RuntimeError("Oracle implicit LogMiner admitted")
            report["opt_in_negative"] = {"error": error}
            if run.version in ("19", "23"):
                # Separate DBA fixture connection prepares archived redo and minimal miner grants.
                sql = ("whenever sqlerror exit failure rollback\nconnect / as sysdba\n"
                       "SHUTDOWN IMMEDIATE;\nSTARTUP MOUNT;\nALTER DATABASE ARCHIVELOG;\n"
                       "ALTER DATABASE OPEN;\nALTER PLUGGABLE DATABASE ALL OPEN;\n"
                       "ALTER DATABASE ADD SUPPLEMENTAL LOG DATA;\n"
                       f'CREATE USER C##OO_MINER IDENTIFIED BY "{run.reader_secret}" CONTAINER=ALL;\n'
                       "GRANT CREATE SESSION,LOGMINING,SELECT ANY TRANSACTION TO C##OO_MINER CONTAINER=ALL;\n"
                       "GRANT EXECUTE ON SYS.DBMS_LOGMNR TO C##OO_MINER CONTAINER=ALL;\n")
                for view in ["V_$DATABASE", "V_$ARCHIVED_LOG", "V_$LOG", "V_$LOGFILE",
                             "V_$LOGMNR_CONTENTS", "V_$LOGMNR_LOGS", "V_$SESSION", "V_$TRANSACTION"]:
                    sql += f"GRANT SELECT ON SYS.{view} TO C##OO_MINER CONTAINER=ALL;\n"
                sql += "ALTER SESSION SET CONTAINER=FREEPDB1;\n"
                for table in tables:
                    sql += f"ALTER TABLE FIXTURE.{table} ADD SUPPLEMENTAL LOG DATA (ALL) COLUMNS;\n"
                sql += "exit\n"
                run.command(["docker", "exec", "-i", run.name, "sqlplus", "-s", "/nolog"], data=sql, timeout=300)
                # Fresh immutable baseline bound to its SCN, not the earlier C5 overlay.
                source["id"] = "fixture_logminer"
                command[command.index("--source")+1] = source["id"]
                runner.write_json(store / "stack.json", stack)
                config["run_id"] = "oracle-c1-base"
                runner.write_json(store / "target.json", config)
                writer_run = copy.copy(run)
                writer_run.seq = 6000000
                writer_errors = []
                # 19c's first post-restart catalog load exceeds eight seconds.
                # Keep the absence assertion below: a missed overlap still fails.
                hold_seconds = 60 if run.version == "19" else 8
                def long_transaction():
                    try:
                        writer_run.sql("INSERT INTO FIXTURE.ITEMS VALUES(7000,'long-tx');\n"
                                       "INSERT INTO FIXTURE.OTHER VALUES(7000,'long-tx');\n"
                                       f"BEGIN DBMS_SESSION.SLEEP({hold_seconds}); END;\n/\nCOMMIT;", admin=True)
                    except Exception as error:
                        writer_errors.append(type(error).__name__)
                writer = threading.Thread(target=long_transaction)
                writer.start()
                import time
                for _ in range(30):
                    _, floor_text = run.sql("SELECT TO_CHAR(MIN(start_scn)) FROM v$transaction;", admin=True)
                    if floor_text.strip():
                        break
                    time.sleep(0.1)
                else:
                    raise RuntimeError("long transaction not observed")
                floor = int(floor_text.strip())
                _, scn = run.sql("SELECT TO_CHAR(current_scn) FROM v$database;", admin=True)
                initial = int(scn.strip())
                if floor >= initial:
                    raise RuntimeError("transaction did not precede baseline")
                policy.update(initial_position={"kind": "oracle_scn", "scn": initial},
                              oracle_logminer_opt_in=True, oracle_retained_floor=floor,
                              mining={"service": "FREE", "user": "C##OO_MINER", "password": {"secret_ref": "source-password"}})
                runner.write_json(store / "cycle.json", policy)
                _, loaded = run.command(command, timeout=300)
                c1_relations = {r["table"]["name"]: r["target"] for r in rows(loaded)}
                # Verify the uncommitted transaction is absent from this baseline.
                for relation in c1_relations.values():
                    if target.sql(f'SELECT count(*) FROM {relation} WHERE "ID"::numeric=7000;').strip() != "0":
                        raise RuntimeError("long transaction committed before baseline")
                writer.join(timeout=60)
                if writer.is_alive() or writer_errors:
                    raise RuntimeError("long transaction writer failed")
                report["long_transaction"] = {"floor": floor, "initial_scn": initial, "absent_from_baseline": True}
                previous = initial
                for cycle in range(3):
                    if cycle < 2:
                        statements = []
                        for table in tables:
                            key = 30000+cycle*10
                            statements.extend([f"INSERT INTO FIXTURE.{table} VALUES({key},'oracle-c1');",
                                               f"UPDATE FIXTURE.{table} SET id={key+1},payload='updated' WHERE id={key};",
                                               f"DELETE FROM FIXTURE.{table} WHERE id=1001;"])
                        run.sql("\n".join(statements)+"\nCOMMIT;", admin=True)
                    before = target.sql("SELECT COALESCE(sum(pg_total_relation_size(c.oid)),0) FROM pg_class c "
                                        "JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='_oo_load' AND c.relkind='r';")
                    with SourceMeter(run, 30+cycle, 2, include_logminer=True) as meter:
                        _, text = run.command(command + ["--sync", str(store / "cycle.json"),
                                                         "--run-id", f"oracle-c1-{cycle}"], timeout=600)
                    receipts = c1_summaries(text)
                    comparisons = {}
                    for table, relation in c1_relations.items():
                        _, expected = run.sql(f"SELECT id,payload FROM FIXTURE.{table} ORDER BY id;")
                        actual = target.sql(f'SELECT "ID","PAYLOAD" FROM {relation} ORDER BY "ID"::numeric;')
                        equal = canonical(expected) == canonical(actual)
                        comparisons[table] = {"equal": equal,
                            "source_sha256": hashlib.sha256(json.dumps(canonical(expected)).encode()).hexdigest(),
                            "target_sha256": hashlib.sha256(json.dumps(canonical(actual)).encode()).hexdigest()}
                        if not equal:
                            raise RuntimeError("Oracle C1 source target difference")
                    after = target.sql("SELECT COALESCE(sum(pg_total_relation_size(c.oid)),0) FROM pg_class c "
                                       "JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='_oo_load' AND c.relkind='r';")
                    for receipt in receipts:
                        current = receipt["durable_position"]["scn"]
                        if current < previous:
                            raise RuntimeError("Oracle SCN regressed")
                        previous = current
                        if cycle == 2 and (receipt["new_generations"] or receipt["target_write_bytes"]):
                            raise RuntimeError("Oracle C1 no-change rewrite")
                    result = {"mode": "C1", "cycle": cycle+1, "receipts": receipts, "comparison": comparisons,
                              "T8": meter.result(), "target_bytes_before": before.strip(), "target_bytes_after": after.strip()}
                    report["cycles"].append(result)
                    runner.write_json(run.path / "t2.json", report)
                    if result["T8"]["status"] != "passed" or (cycle == 2 and before != after):
                        raise RuntimeError("Oracle C1 T8 or no-change rewrite")
                report["C1"] = "passed"
                before = target.sql("SELECT source,lineage,position,generation FROM _oo_load.sync_state ORDER BY source;")
                runner.write_json(store / "cycle.json", dict(policy, oracle_retained_floor=floor+1))
                code, _ = run.command(command + ["--sync", str(store / "cycle.json")], required=False, timeout=300)
                error = json.loads((run.path / f"{run.seq:04d}.stderr").read_text())["error"] if code else None
                if not code or error != "sync_rebaseline_required_lineage_or_scope" or target.sql(
                        "SELECT source,lineage,position,generation FROM _oo_load.sync_state ORDER BY source;") != before:
                    raise RuntimeError("Oracle transaction floor changed on resume")
                report["floor_negative"] = {"error": error, "target_unchanged": True}
        report["T2"] = "passed"
        return report
    finally:
        runner.write_json(run.path / "t2.json", report)
        if writer:
            writer.join(timeout=60)
        if worker:
            _, raw = run.command(["docker", "inspect", "--format", "{{json .Config.Labels}}", worker])
            labels = json.loads(raw)
            if labels.get("oo.lane") != t1.LANE or labels.get("oo.run_id") != run.id:
                raise RuntimeError("worker cleanup ownership")
            run.command(["docker", "rm", "-f", worker])
        target.close()
        shutil.rmtree(store)


def mariadb_source_epoch(server, domain, gtids):
    identities = {(int(domain), int(server))}
    identities.update(tuple(map(int, g.split('-')[:2])) for g in gtids.split(',') if g)
    return f'mariadb:{server}:{domain}:' + ','.join(f'{d}-{s}' for d, s in sorted(identities))

def duration_micros(text):
    from decimal import Decimal
    if text is None:
        return None
    hours, minutes, seconds = text.lstrip('-').split(':')
    value = (Decimal(hours)*3600 + Decimal(minutes)*60 + Decimal(seconds))*1000000
    return int(value) * (-1 if text.startswith('-') else 1)


def family_acceptance(config_path, binary):
    """Reuse the owned lane-C fixture and M's target/meter/receipt conventions."""
    import time
    import pymysql
    import t1
    config = json.loads(config_path.read_text())
    family = config['family']
    if family not in ('mariadb', 'tidb', 'oceanbase'):
        raise ValueError('unsupported family')
    t1.LANE = 'c'
    path = Path(config['out']) / 'acceptance'
    path.mkdir(mode=0o700)

    class FamilyRun(runner.Run):
        bootstrap = family == 'tidb'

        def connect(self, admin=False):
            user = 'root' if admin else 'reader'
            return pymysql.connect(host='127.0.0.1', port=config['port'],
                user=user + ('@test' if family == 'oceanbase' else ''),
                # The isolated unistore fixture bootstraps its root without a password.
                password='' if admin and self.bootstrap else Path(config[user + '_password']).read_text(), database='fixture',
                ssl_ca=config['ca'], ssl_verify_cert=True, ssl_verify_identity=True,
                charset='utf8mb4', autocommit=True, connect_timeout=10, read_timeout=120)

        def sql(self, text, admin=False, **kwargs):
            text = text.replace("USER='fixture_reader'", "USER='reader'")
            with self.connect(admin) as conn, conn.cursor() as cursor:
                cursor.execute(text)
                result = cursor.fetchall()
            return 0, '\n'.join('\t'.join('NULL' if v is None else str(v) for v in row) for row in result)

    class FamilyMeter(SourceMeter):
        def sample(self):
            # These snapshots can finish in <1s. Use a persistent admin sampler
            # instead of assigning a whole 200ms interval to a millisecond query.
            try:
                with self.run.connect(True) as conn, conn.cursor() as cursor:
                    while not self.stop.is_set():
                        cursor.execute("SELECT count(*),COALESCE(sum(COMMAND<>'Sleep'),0) "
                                       "FROM information_schema.PROCESSLIST WHERE USER='reader'")
                        connections, active = map(int, cursor.fetchone())
                        self.samples.append((time.monotonic(), connections, active))
                        self.stop.wait(0.01)
            except Exception as error:
                self.error = type(error).__name__

    run = FamilyRun(path, 'mysql', family, argparse.Namespace())
    run.secret = Path(config['root_password']).read_text()
    run.reader_secret = Path(config['reader_password']).read_text()
    # Refuse a stale or foreign fixture before any administrative SQL.
    _, raw = run.command(['docker', 'inspect', '--format', '{{json .Config.Labels}}', config['container']])
    labels = json.loads(raw)
    if labels.get('oo.lane') != 'c' or labels.get('oo.run_id') != Path(config['out']).parent.name:
        raise RuntimeError('family fixture ownership')
    target = AgentDB(run)
    store = run.private
    report = {'T0': 'not_run', 'T1': 'not_passed', 'T2': 'not_passed', 'T8': 'not_passed',
              'C1': 'not_run', 'C5': 'not_run', 'cycles': [], 'tables': {},
              'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
              'consistency': 'B', 'scope': 'six fixture shapes; writer correction; two keyed sync tables'}
    stop = threading.Event()
    thread = None
    def measured(meter):
        result = meter.result()
        result['session_scope'] = 'reader'
        result['sample_interval_seconds'] = 0.01
        return result

    try:
        if run.bootstrap:
            with run.connect(True) as conn, conn.cursor() as cursor:
                cursor.execute('SELECT CURRENT_USER()')
                user, host = cursor.fetchone()[0].rsplit('@', 1)
                cursor.execute('ALTER USER %s@%s IDENTIFIED BY %s', (user, host, run.secret))
            run.bootstrap = False
        if family == 'oceanbase':
            run.sql("GRANT SELECT ON oceanbase.DBA_OB_TENANTS TO 'reader'@'%'", admin=True)
        probe = Path(config['out']) / 'probe.json'
        if probe.exists():
            report['T0'] = 'passed'
            report['probe'] = json.loads(probe.read_text())
        target.start()
        private(store / 'target-password', target.password)
        private(store / 'source-password', run.reader_secret)
        private(store / 'source-ca', Path(config['ca']).read_text())
        source = {'id': 'family_t1', 'engine': family, 'host': '127.0.0.1', 'port': config['port'],
                  'database': 'fixture', 'user': 'reader@test' if family == 'oceanbase' else 'reader',
                  'password': {'secret_ref': 'source-password'}, 'tls_ca': {'secret_ref': 'source-ca'},
                  'scope': {'include': ['fixture.*'], 'exclude': []}}
        stack = {'version': 'oo.stack/v1', 'sources': [source], 'model': {
            'provider': 'openai-compatible', 'base_url': 'https://example.invalid/v1', 'model': 'unused',
            'key': {'secret_ref': 'unused'}, 'context_window': 'auto'}}
        target_config = {'host': '127.0.0.1', 'port': target.port, 'database': 'postgres', 'user': 'postgres',
                         'password': {'secret_ref': 'target-password'}, 'local_fixture': True, 'run_id': 'base'}
        command = [str(binary), '--stack', str(store / 'stack.json'), '--source', source['id'],
                   '--target', str(store / 'target.json'), '--secrets-dir', str(store), '--chunk-rows', '1000']
        def load(tables, generation, mode=None, policy=None, required=True):
            runner.write_json(store / 'stack.json', stack)
            command[command.index('--source')+1] = source['id']
            target_config['run_id'] = generation
            runner.write_json(store / 'target.json', target_config)
            args = command + ['--tables', ','.join('fixture.' + t for t in tables)]
            if mode:
                runner.write_json(store / 'cycle.json', policy)
                args += [mode, str(store / 'cycle.json')]
            code, text = run.command(args, timeout=600, required=required)
            return code, rows(text)

        def compare_table(table, relation):
            if not __import__('re').fullmatch(r'oo_load_data\.[a-z0-9_]+', relation):
                raise RuntimeError('invalid relation')
            if table == 'typed_rows':
                # TiDB TIME_TO_SEC truncates fractions; compare the actual TIME(6).
                left = "JSON_OBJECT('id',id,'amount',CAST(amount AS CHAR),'big',CAST(big AS CHAR),'doc',doc,'raw',LOWER(HEX(raw)),'elapsed',CAST(elapsed AS CHAR))"
                right = "SELECT id,amount::text,big::text,doc::text,encode(raw,'hex') AS raw,(extract(epoch FROM elapsed)*1000000)::bigint AS elapsed"
            else:
                columns = ['k', 'payload'] if table == 'text_rows' else ['tenant', 'seq', 'payload'] if table == 'composite_rows' else ['id', 'payload']
                left = 'JSON_OBJECT(' + ','.join("'"+c+"',`"+c+'`' for c in columns) + ')'
                right = 'SELECT ' + ','.join('"'+c+'"' for c in columns)
            # Keep verification sessions separate from the metered product user.
            _, text = run.sql(f'SELECT {left} FROM fixture.`{table}`', admin=True)
            expected = [json.loads(line) for line in text.splitlines()]
            actual = json.loads(target.sql(f"SELECT COALESCE(jsonb_agg(to_jsonb(t)), '[]'::jsonb) FROM ({right} FROM {relation}) t;"))
            if table == 'typed_rows':
                for row in expected:
                    row['elapsed'] = duration_micros(row['elapsed'])
                for row in expected + actual:
                    if isinstance(row['doc'], str):
                        row['doc'] = json.loads(row['doc'])
            norm = lambda data: sorted(json.dumps(r, sort_keys=True, ensure_ascii=True) for r in data)
            result = {'equal': norm(expected) == norm(actual), 'rows': len(expected),
                      'source_sha256': hashlib.sha256(json.dumps(norm(expected)).encode()).hexdigest(),
                      'target_sha256': hashlib.sha256(json.dumps(norm(actual)).encode()).hexdigest()}
            runner.write_json(path / ('compare-'+table+'.json'), result)
            if not result['equal']:
                runner.write_json(path / ('difference-'+table+'.json'), {'source': expected, 'target': actual})
                raise RuntimeError('source target difference: '+table)
            return result

        tables = ['rows', 'text_rows', 'composite_rows', 'no_key', 'empty_rows', 'typed_rows']
        writer_result = {'cycles': 0}
        ready = threading.Event()
        def writer():
            try:
                with run.connect(True) as conn, conn.cursor() as cursor:
                    while not stop.is_set():
                        cursor.execute("UPDATE fixture.`rows` SET payload=%s WHERE id=2", ('writer-'+str(writer_result['cycles']),))
                        writer_result['cycles'] += 1
                        ready.set()
                        stop.wait(0.1)
            except Exception as error:
                writer_result['error'] = type(error).__name__
                ready.set()
        thread = threading.Thread(target=writer)
        thread.start()
        if not ready.wait(30) or writer_result.get('error'):
            raise RuntimeError('writer startup')
        before = writer_result['cycles']
        measurements = []
        with FamilyMeter(run, 0, 1) as meter:
            _, receipts = load(tables, 'concurrent')
        measurements.append(measured(meter))
        stop.set()
        thread.join(timeout=30)
        if thread.is_alive() or writer_result.get('error') or writer_result['cycles'] <= before:
            raise RuntimeError('concurrent writer did not commit during load')
        report['writer'] = writer_result
        # B consistency: quiesce the writer and reread before exact comparison.
        with FamilyMeter(run, 1, 1) as meter:
            _, receipts = load(tables, 'corrected')
        measurements.append(measured(meter))
        for receipt in receipts:
            table = receipt['table']['name']
            report['tables'][table] = compare_table(table, receipt['target'])
        if set(report['tables']) != set(tables):
            raise RuntimeError('missing snapshot receipt')
        report['T1'] = 'passed'
        report['T1_T8'] = measurements
        if any(m['status'] != 'passed' for m in measurements):
            raise RuntimeError('snapshot T8 budget')
        runner.write_json(path / 'acceptance.json', report)
        tables = ['sync_items', 'sync_other']
        for table in tables:
            run.sql(f'CREATE TABLE {table}(id BIGINT PRIMARY KEY,payload VARCHAR(80) NOT NULL) DEFAULT CHARSET=utf8mb4', admin=True)
            run.sql(f"INSERT INTO {table} VALUES(1,'baseline')", admin=True)
        for mode in (['C1', 'C5'] if family == 'mariadb' else ['C5']):
            source['id'] = 'family_' + mode.lower()
            sync = None
            if mode == 'C1':
                run.sql("GRANT REPLICATION SLAVE, BINLOG MONITOR, REPLICA MONITOR ON *.* TO 'reader'@'%'", admin=True)
                _, raw_epoch = run.sql('SELECT @@GLOBAL.server_id,@@GLOBAL.gtid_domain_id,@@GLOBAL.gtid_binlog_pos', admin=True)
                server, domain, gtids = raw_epoch.split('\t')
                epoch = mariadb_source_epoch(server, domain, gtids)
                _, position = run.sql('SHOW MASTER STATUS', admin=True)
                fields = position.split()
                sync = {'initial_position': {'kind': 'mysql_file_pos', 'file': fields[0], 'position': int(fields[1])},
                        'source_epoch': epoch, 'baseline_aligned': True, 'max_batches': 100,
                        'server_id': 732, 'longest_cycle_seconds': 1, 'recovery_seconds': 1}
            _, baseline = load(tables, mode+'-baseline')
            relations = {r['table']['name']: r['target'] for r in baseline}
            if set(relations) != set(tables):
                raise RuntimeError('missing sync baseline')
            previous = None
            for cycle in range(3):
                if cycle < 2:
                    key = (100 if mode == 'C1' else 1000) + cycle*10
                    for table in tables:
                        for sql in (f"INSERT INTO {table} VALUES({key},'new')",
                                    f"UPDATE {table} SET id={key+1},payload='updated' WHERE id={key}",
                                    f'DELETE FROM {table} WHERE id=1'):
                            run.sql(sql, admin=True)
                policy = sync if mode == 'C1' else {'cycle': cycle+1, 'full_read_every': 2}
                before = target.sql("SELECT COALESCE(sum(pg_total_relation_size(c.oid)),0) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='_oo_load' AND c.relkind='r';")
                with FamilyMeter(run, 10+cycle+(0 if mode == 'C1' else 10), 3 if mode == 'C1' else 1) as meter:
                    _, receipts = load(tables, f'{mode}-{cycle}', '--sync' if mode == 'C1' else '--c5', policy)
                if len(receipts) != (1 if mode == 'C1' else len(tables)):
                    raise RuntimeError('missing cycle receipt')
                comparisons = {table: compare_table(table, relation) for table, relation in relations.items()}
                after = target.sql("SELECT COALESCE(sum(pg_total_relation_size(c.oid)),0) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='_oo_load' AND c.relkind='r';")
                for receipt in receipts:
                    if mode == 'C1':
                        p = receipt['durable_position']
                        current = (p['file'], p['position'])
                        if previous is not None and current < previous:
                            raise RuntimeError('cursor regressed')
                        previous = current
                    if cycle == 2 and (receipt['target_write_bytes'] or receipt['new_generations'] or receipt.get('payload_bytes', 0)):
                        raise RuntimeError('no-change rewrote target')
                if cycle == 2 and before != after:
                    raise RuntimeError('no-change physical rewrite')
                result = {'mode': mode, 'cycle': cycle+1, 'receipts': receipts, 'comparison': comparisons, 'T8': measured(meter)}
                report['cycles'].append(result)
                runner.write_json(path / 'acceptance.json', report)
                if result['T8']['status'] != 'passed':
                    raise RuntimeError('cycle T8 budget')
            report[mode] = 'passed'
            if mode == 'C1':
                state = 'SELECT source,lineage,position,generation FROM _oo_load.sync_state ORDER BY source;'
                before = target.sql(state)
                code, _ = load(tables, 'bad-epoch', '--sync', dict(sync, source_epoch='different-domain'), required=False)
                error = json.loads((path / f'{run.seq:04d}.stderr').read_text())['error']
                if not code or error != 'sync_rebaseline_required_lineage' or before != target.sql(state):
                    raise RuntimeError('epoch rejection failed')
                report['epoch_negative'] = {'error': error, 'target_unchanged': True}
                _, domain = run.sql('SELECT @@GLOBAL.gtid_domain_id', admin=True)
                try:
                    run.sql(f'SET GLOBAL gtid_domain_id={(int(domain)+1) % (2**32)}', admin=True)
                    code, _ = load(tables, 'changed-domain', '--sync', sync, required=False)
                    error = json.loads((path / f'{run.seq:04d}.stderr').read_text())['error']
                    if not code or error != 'sync_rebaseline_required_lineage' or before != target.sql(state):
                        raise RuntimeError('live GTID domain change admitted')
                    report['domain_negative'] = {'error': error, 'target_unchanged': True}
                finally:
                    run.sql(f'SET GLOBAL gtid_domain_id={int(domain)}', admin=True)
        if family != 'mariadb':
            code, _ = load(tables, 'unsupported-c1', '--sync', {}, required=False)
            error = json.loads((path / f'{run.seq:04d}.stderr').read_text())['error']
            if not code or error != family+'_c1_unsupported_use_c5':
                raise RuntimeError('C1 rejection failed')
            report['C1'] = 'unsupported_named_error'
        report['T2'] = report['T8'] = 'passed'
    except Exception as error:
        # Driver exceptions can include source SQL; never persist their message.
        report['error'] = str(error) if isinstance(error, RuntimeError) else type(error).__name__
    finally:
        stop.set()
        if thread:
            thread.join(timeout=130)
        try:
            target.close()
            report['target_cleanup'] = 'removed'
        finally:
            shutil.rmtree(store)
            runner.write_json(path / 'acceptance.json', report)
            runner.checksums(path)
    print(json.dumps({k: report.get(k) for k in ('T0', 'T1', 'T2', 'T8', 'C1', 'C5', 'error')}))
    return int(any(report[k] != 'passed' for k in ('T0', 'T1', 'T2', 'T8')))


def main():
    def interrupted(signum, frame):
        raise KeyboardInterrupt(f"signal {signum}")
    signal.signal(signal.SIGTERM, interrupted)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--family-config", type=Path, help="owned lane-C fixture; requires PyMySQL")
    parser.add_argument("--engine", choices=["mysql", "oracle"])
    parser.add_argument("--versions")
    parser.add_argument("--run-id")
    parser.add_argument("--binary", type=Path, default=ROOT / "target/debug/oo-load")
    parser.add_argument("--worker-image")
    parser.add_argument("--scale", type=int, default=1)
    args = parser.parse_args()
    if not 1 <= args.scale <= 10_000_000 or (args.engine == "oracle" and args.scale != 1):
        parser.error("scale must be 1..10000000; Oracle uses its existing fixture")
    if args.family_config:
        raise SystemExit(family_acceptance(args.family_config, args.binary.resolve()))
    if not args.engine or not args.versions or not args.run_id:
        parser.error("--engine, --versions and --run-id are required")
    versions = args.versions.split(",")
    if not args.run_id or not all(c.isalnum() or c in "-_" for c in args.run_id):
        parser.error("invalid run ID")
    if len(set(versions)) != len(versions) or any(v not in (MYSQL if args.engine == "mysql" else ["19", "23", "21", "11"]) for v in versions):
        parser.error("unsupported or duplicate versions")
    path = ROOT / "evidence" / args.run_id
    path.mkdir(mode=0o700, exist_ok=False)
    results = {}
    lane = t1.LANE
    for version in versions:
        t1.LANE = "E19" if args.engine == "oracle" and version == "19" else lane
        directory = path / (args.engine+"-"+version)
        directory.mkdir()
        run = SyncRun(directory, args.engine, version,
                      argparse.Namespace(platform=None, health_timeout=1200, scale=args.scale, tables=0))
        results[version] = {"T2": "not_passed", "C1": "not_run", "C5": "not_run"}
        try:
            results[version] = run_cycles(run, args.binary.resolve(), args.worker_image)
        except Exception as error:
            raw = directory / "t2.json"
            results[version] = json.loads(raw.read_text()) if raw.exists() else {}
            results[version].update(error=type(error).__name__, T2="not_passed")
        finally:
            try:
                run.record_version()
            except Exception:
                results[version]["version_error"] = "version_unmeasured"
            results[version]["cleanup_failures"] = run.cleanup()
            runner.write_json(directory / "t2.json", results[version])
            write_rows(run, directory / "t2.json", incremental(results[version]),
                       cleanup_failed=bool(results[version]["cleanup_failures"]))
            runner.checksums(directory)
            runner.write_json(path / "matrix.json", results)
    runner.checksums(path)
    print(json.dumps({v: {k: r.get(k) for k in ("C1", "C5", "T2", "error", "cleanup_failures")} for v, r in results.items()}))
    if any(r.get("T2") != "passed" or r.get("version_error") or r.get("C5") != "passed"
           or r["cleanup_failures"] or ((args.engine == "mysql" or v in ("19", "23")) and r.get("C1") != "passed")
           for v, r in results.items()):
        raise SystemExit(1)

if __name__ == "__main__":
    main()
