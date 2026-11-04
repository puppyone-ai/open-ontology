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
from t1 import AgentDB, LaneRun, SourceMeter, configure_mysql, private

ROOT = runner.ROOT
MYSQL = {"5.7": "5.7.44", "8.0": "8.0.44", "8.4": "8.4.7", "9": "9.5.0"}

class SyncRun(LaneRun):
    def command(self, command, **kwargs):
        if command[:2] == ["docker", "create"] and self.name in command and self.engine == "mysql":
            command = [*command, "--server-id=41", "--log-bin=oo-bin", "--binlog-format=ROW", "--binlog-row-image=FULL"]
        return super().command(command, **kwargs)

    def setup(self):
        original = runner.resolve
        if self.engine == "mysql":
            version = MYSQL[self.version]
            ref = "mysql:" + version
            _, raw = self.command(["docker", "image", "inspect", ref])
            image = json.loads(raw)[0]
            digest = image["RepoDigests"][0].split("@")[1]
            item = {"id": "mysql-"+version, "ref": ref,
                    "platforms": {"linux/"+image["Architecture"]: {"digest": digest}}}
            runner.resolve = lambda engine, major: item if engine == "mysql" else original(engine, major)
        try:
            super().setup()
        finally:
            runner.resolve = original


def rows(text):
    return [json.loads(line) for line in text.splitlines() if line.startswith("{")]


def canonical(text):
    return [tuple(part.strip() for part in line.replace("\t", "|").split("|"))
            for line in text.splitlines() if line.strip()]


def run_cycles(run, binary, worker_image):
    report = {"T2": "not_passed", "C1": "not_run", "C5": "not_run", "handoff": "not_run",
              "cycles": [], "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "scope": "two keyed numeric/text tables; not full type/scale coverage", "consistency": "B"}
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
                         "--label", "oo.lane=m", "--label", "oo.run_id="+run.id,
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
                before = target.sql("SELECT COALESCE(sum(pg_total_relation_size(c.oid)),0) FROM pg_class c "
                                    "JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='_oo_load' AND c.relkind='r';")
                with SourceMeter(run, (0 if mode == "C1" else 10)+cycle, 3 if mode == "C1" else 1) as meter:
                    _, text = run.command(command + ["--sync" if mode == "C1" else "--c5", str(store / "cycle.json")], timeout=600)
                receipts = rows(text)
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
                code, _ = run.command(command + ["--sync", str(store / "cycle.json")], required=False)
                error = json.loads((run.path / f"{run.seq:04d}.stderr").read_text())["error"]
                if not code or error != "sync_rebaseline_required_lineage":
                    raise RuntimeError("different epoch admitted")
                after = target.sql("SELECT source,lineage,position,generation FROM _oo_load.sync_state ORDER BY source;")
                if before != after:
                    raise RuntimeError("epoch rejection modified target")
                report["epoch_negative"] = {"error": error, "target_unchanged": True}
        if run.engine == "mysql":
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
                    writer_run.sql(sql, admin=True)
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
            for schema_name in ("fixture", "fixture_peer"):
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
            if run.version == "23":
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
                def long_transaction():
                    try:
                        writer_run.sql("INSERT INTO FIXTURE.ITEMS VALUES(7000,'long-tx');\n"
                                       "INSERT INTO FIXTURE.OTHER VALUES(7000,'long-tx');\n"
                                       "BEGIN DBMS_SESSION.SLEEP(8); END;\n/\nCOMMIT;", admin=True)
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
                        _, text = run.command(command + ["--sync", str(store / "cycle.json")], timeout=600)
                    receipts = rows(text)
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
            if labels.get("oo.lane") != "m" or labels.get("oo.run_id") != run.id:
                raise RuntimeError("worker cleanup ownership")
            run.command(["docker", "rm", "-f", worker])
        target.close()
        shutil.rmtree(store)


def main():
    def interrupted(signum, frame):
        raise KeyboardInterrupt(f"signal {signum}")
    signal.signal(signal.SIGTERM, interrupted)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--engine", choices=["mysql", "oracle"], required=True)
    parser.add_argument("--versions", required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/debug/oo-load")
    parser.add_argument("--worker-image")
    args = parser.parse_args()
    versions = args.versions.split(",")
    if not args.run_id or not all(c.isalnum() or c in "-_" for c in args.run_id):
        parser.error("invalid run ID")
    if len(set(versions)) != len(versions) or any(v not in (MYSQL if args.engine == "mysql" else ["23", "21", "11"]) for v in versions):
        parser.error("unsupported or duplicate versions")
    path = ROOT / "evidence" / args.run_id
    path.mkdir(mode=0o700, exist_ok=False)
    results = {}
    for version in versions:
        directory = path / (args.engine+"-"+version)
        directory.mkdir()
        run = SyncRun(directory, args.engine, version,
                      argparse.Namespace(platform=None, health_timeout=1200, scale=1, tables=0))
        try:
            results[version] = run_cycles(run, args.binary.resolve(), args.worker_image)
        except Exception as error:
            results[version] = json.loads((directory / "t2.json").read_text())
            results[version]["error"] = str(error)
        finally:
            results[version]["cleanup_failures"] = run.cleanup()
            runner.checksums(directory)
            runner.write_json(path / "matrix.json", results)
    runner.checksums(path)
    print(json.dumps({v: {k: r.get(k) for k in ("C1", "C5", "T2", "error", "cleanup_failures")} for v, r in results.items()}))
    if any(r["C5"] != "passed" or r["cleanup_failures"] or ((args.engine == "mysql" or v == "23") and r["C1"] != "passed") for v, r in results.items()):
        raise SystemExit(1)

if __name__ == "__main__":
    main()
