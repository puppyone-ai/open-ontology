#!/usr/bin/env python3
"""T3 fault injection and T4 schema change on owned MySQL/PostgreSQL/Oracle fixtures.

Reuses lane M's LaneRun/AgentDB conventions (t1.py) and the real oo-load
binary. One source container and one Agent DB container per version. A case
passes only if every fault resumes, or refuses with a named error and an
unchanged published target, and the Agent DB has zero divergence afterwards.
"""
import argparse
import os
import hashlib
import json
from pathlib import Path
import re
import shutil
import signal
import socket
import ssl
import subprocess
import time
import uuid

import runner
from verdicts import write_rows
import t1
from t1 import AgentDB, LaneRun, configure_mysql, configure_pg, private

ROOT = runner.ROOT
MYSQL = ("5.7", "8.0", "8.4", "9")
ORACLE = ("19", "23", "21", "11")
POSTGRES = ("13", "14", "15", "16", "17", "18")
T3 = ("source_restart_mid_chunk", "network_drop_mid_c1", "network_drop_mid_c5",
      "target_restart_mid_publish", "sigkill_mid_cycle", "retention_expiry",
      "slot_invalidation", "gtid_purge", "resetlogs_incarnation")
T4 = ("add_column", "drop_column", "rename_column", "widen_type", "widen_string", "add_table",
      "drop_table", "pk_change")
# Destructive log cases invalidate every earlier C1 binding; they run last.
ORDER = T4 + T3
BULK_ROWS = 20000
CHUNK_ROWS = 500
# Closed oo-load codes, or the connector's closed SourceError rendering.
ERROR = re.compile(r"[a-z0-9_]+|source error: [a-z_]+ fact=[a-z0-9_]+( sqlstate=[0-9A-Z]{5})?")


def rows(text):
    return [json.loads(line) for line in text.splitlines() if line.startswith("{")]


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def type_class(kind):
    """Coarse cross-engine column class; widening int4->int8 must be visible."""
    kind = kind.lower().split("(")[0].strip()
    if kind in ("int", "integer", "int4", "mediumint"):
        return "int4"
    if kind in ("bigint", "int8"):
        return "int8"
    if kind in ("varchar", "character varying", "text", "char", "character", "longtext"):
        return "text"
    return kind


def named(error):
    return isinstance(error, str) and bool(ERROR.fullmatch(error))


def classify(code, error, equal, state_unchanged):
    """One incremental/snapshot attempt after a fault or DDL."""
    if code == 0:
        return "online_apply" if equal else "silent_divergence"
    if not named(error):
        return "unnamed_failure"
    return "named_refusal" if state_unchanged else "refusal_changed_target"


class Source(LaneRun):
    """Owned source with a fixed loopback port: the binding identity contains it."""
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.host_port = free_port()

    def command(self, command, **kwargs):
        if command[:2] == ["docker", "create"] and self.name in command:
            command = ["oo.lane=" + t1.LANE if x == "oo.lane=h" else x for x in command]
            inner = {"postgres": 5432, "mysql": 3306, "oracle": 1521}[self.engine]
            command[2:2] = ["-p", f"127.0.0.1:{self.host_port}:{inner}"]
            if self.engine == "mysql":
                command += ["--server-id=41", "--log-bin=oo-bin", "--binlog-format=ROW",
                            "--binlog-row-image=FULL", "--gtid-mode=ON",
                            "--enforce-gtid-consistency=ON"]
            elif self.engine == "postgres":
                command += ["-c", "wal_level=logical", "-c", "max_replication_slots=64",
                            "-c", "max_wal_senders=64", "-c", "max_slot_wal_keep_size=256MB"]
            return runner.Run.command(self, command, **kwargs)
        return super().command(command, **kwargs)


class DurableAgentDB(AgentDB):
    """Agent DB on a labelled volume and fixed port so a restart keeps its data."""
    def start(self):
        item = runner.resolve("postgres", "17")
        _, arch = self.run.command(["docker", "info", "--format", "{{.Architecture}}"])
        platform = "linux/arm64" if arch.strip() in ("arm64", "aarch64") else "linux/amd64"
        image = item["ref"] + "@" + item["platforms"][platform]["digest"]
        self.run.command(["docker", "pull", "--platform", platform, image], timeout=1200)
        self.port = free_port()
        self.volume = self.name + "-data"
        labels = ["--label", "oo.lane=" + t1.LANE, "--label", "oo.run_id=" + self.run.id]
        self.run.command(["docker", "volume", "create", *labels, self.volume])
        self.created_volume = True
        self.run.command(["docker", "create", "--name", self.name, *labels, "--platform", platform,
                          "-p", f"127.0.0.1:{self.port}:5432",
                          "--mount", f"type=volume,src={self.volume},dst=/var/lib/postgresql/data",
                          "-v", f"{self.directory}:/run/private:ro",
                          "-e", "POSTGRES_PASSWORD_FILE=/tmp/pw", "--entrypoint", "bash", image,
                          "-c", "cp /run/private/password /tmp/pw; chown postgres:postgres /tmp/pw; "
                          "chmod 600 /tmp/pw; exec docker-entrypoint.sh postgres"])
        self.created = True
        self.run.command(["docker", "start", self.name])
        self.ready()
        self.sql("CREATE COLLATION fixture_ci (provider=icu,locale='und-u-ks-level1',deterministic=false);")

    def query(self, sql, required=True):
        return self.run.command(["docker", "exec", "-i", self.name, "psql", "-X", "-q", "-U", "postgres",
                                 "-v", "ON_ERROR_STOP=1", "-A", "-t", "-F", "\t", "-P", "null=NULL"],
                                data=sql, required=required)

    def close(self):
        try:
            super().close()
        finally:
            if getattr(self, "created_volume", False):
                _, raw = self.run.command(["docker", "volume", "inspect", "--format", "{{json .Labels}}",
                                           self.volume])
                labels = json.loads(raw)
                if labels.get("oo.lane") != t1.LANE or labels.get("oo.run_id") != self.run.id:
                    raise RuntimeError("target volume ownership")
                self.run.command(["docker", "volume", "rm", self.volume])


class Harness:
    def __init__(self, run, binary):
        self.run, self.binary = run, binary
        self.store = run.path / ".sync-private"
        self.store.mkdir(mode=0o700)
        self.target = DurableAgentDB(run)
        self.schema = "fixture" if run.engine == "mysql" else "public"
        self.stack = {"version": "oo.stack/v1", "sources": [], "model": {
            "provider": "openai-compatible", "base_url": "https://example.invalid/v1", "model": "unused",
            "key": {"secret_ref": "unused"}, "context_window": "auto"}}
        self.relations = {}
        self.sync = {}
        self.server_ids = iter(range(100, 10000))
        self.calls = 0
        self.next_key = 100000
        self.disconnected = False
        self.recoveries = []
        self.paths = ("c5", "c1")

    # Source/target access -------------------------------------------------
    def src(self, sql, required=True):
        if self.run.engine == "mysql":
            return self.run.sql(sql, admin=True, required=required)
        return self.run.command(["docker", "exec", "-i", "-u", "postgres", self.run.name, "psql", "-X", "-q",
                                 "-v", "ON_ERROR_STOP=1", "-A", "-t", "-F", "\t", "-P", "null=NULL",
                                 "-U", "postgres", "-d", "fixture"], data=sql, required=required)

    def tgt(self, sql, required=True):
        return self.target.query(sql, required=required)

    def wait_source(self):
        for _ in range(180):
            if self.src("SELECT 1;", required=False)[0] == 0:
                return
            time.sleep(1)
        raise RuntimeError("source readiness")

    def heal(self):
        reconnected = self.disconnected or self.__dict__.pop("pending_flush", False)
        if self.disconnected:
            self.run.command(["docker", "network", "connect", "bridge", self.run.name])
            self.disconnected = False
        for name in (self.run.name, self.target.name):
            _, state = self.run.command(["docker", "inspect", "--format", "{{.State.Status}}", name])
            if state.strip() != "running":
                self.run.command(["docker", "start", name])
        self.wait_source()
        self.target.ready()
        tls = self.run.engine == "postgres"
        for name, port, kind, handshake in ((self.run.name, self.run.host_port, self.run.engine, tls),
                                         (self.target.name, self.target.port, "postgres", False)):
            if wait_until(lambda: probe(port, kind, handshake), 30, 0.5):
                continue
            # Docker Desktop can lose a published port after network reconnect;
            # a container restart restores it. Harness recovery, recorded.
            self.run.command(["docker", "restart", name], timeout=600)
            self.recoveries.append({"after": "port_unreachable", "action": "docker restart", "container": name})
            self.wait_source() if name == self.run.name else self.target.ready()
            if not wait_until(lambda: probe(port, kind, handshake), 60, 0.5):
                raise RuntimeError("published port unreachable")
        if reconnected and self.run.engine == "mysql":
            # Handshakes cut by the drop count as host connection errors
            # (max_connect_errors); record the cache, then let the operator clear it.
            _, cache = self.src("SELECT HOST,SUM_CONNECT_ERRORS,COUNT_HOST_BLOCKED_ERRORS,"
                                "COUNT_HANDSHAKE_ERRORS FROM performance_schema.host_cache;")
            # LOCAL: a logged FLUSH is a binlog statement that C1 must refuse.
            flush = "FLUSH LOCAL HOSTS;" if self.run.version == "5.7" else "TRUNCATE performance_schema.host_cache;"
            self.src(flush)
            self.recoveries.append({"after": "network_reconnect", "host_cache": cache.strip(),
                                    "operator_action": flush})

    def prepare(self):
        run = self.run
        run.setup()
        private(self.store / "source-password", run.reader_secret)
        if run.engine == "mysql":
            configure_mysql(run, self.store)
            run.sql("GRANT REPLICATION SLAVE,REPLICATION CLIENT ON *.* TO 'fixture_reader'@'%';", admin=True)
        else:
            self.src("CREATE ROLE fixture_reader LOGIN REPLICATION;"
                     "REVOKE CREATE ON SCHEMA public FROM PUBLIC;"
                     "REVOKE TEMPORARY ON DATABASE fixture FROM PUBLIC;"
                     "GRANT USAGE ON SCHEMA public TO fixture_reader;"
                     "ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT SELECT ON TABLES TO fixture_reader;"
                     "GRANT EXECUTE ON FUNCTION pg_control_system() TO fixture_reader;"
                     "GRANT EXECUTE ON FUNCTION pg_control_checkpoint() TO fixture_reader;")
            configure_pg(run, self.store)
        tables = ["bulk", "items", "other"] + [f"t4_{c}" for c in T4] + ["t4_drop_table_b"]
        for table in tables:
            self.src(f"CREATE TABLE {table}(id bigint NOT NULL PRIMARY KEY,payload varchar(80) NOT NULL,"
                     f"n int NOT NULL){' DEFAULT CHARSET=utf8mb4' if run.engine == 'mysql' else ''};")
            if table != "bulk":
                self.src(f"INSERT INTO {table} VALUES" +
                         ",".join(f"({i},'base-{i}',{i})" for i in range(1, 21)) + ";")
        for start in range(1, BULK_ROWS + 1, 1000):
            self.src("INSERT INTO bulk VALUES" + ",".join(
                f"({i},'bulk-{i}',{i})" for i in range(start, start + 1000)) + ";")
        self.src("CREATE TABLE scratch(id bigint PRIMARY KEY,pad text);")
        self.target.start()
        private(self.store / "target-password", self.target.password)

    # oo-load invocation ---------------------------------------------------
    def add_source(self, sid, tables, chunk_rows=None):
        self.stack["sources"] = [s for s in self.stack["sources"] if s["id"] != sid]
        source = {"id": sid, "engine": self.run.engine, "host": "127.0.0.1", "port": self.run.host_port,
                  "database": "fixture", "user": "fixture_reader",
                  "password": {"secret_ref": "source-password"}, "tls_ca": {"secret_ref": "source-ca"},
                  "scope": {"include": [f"{self.schema}.{t}" for t in tables], "exclude": []},
                  "_chunk_rows": chunk_rows}
        self.stack["sources"].append(source)

    def tables(self, sid):
        source = next(s for s in self.stack["sources"] if s["id"] == sid)
        return [t.split(".", 1)[1] for t in source["scope"]["include"]]

    def argv(self, sid, run_id, mode=None, policy=None):
        self.calls += 1
        stack = dict(self.stack, sources=[{k: v for k, v in s.items() if not k.startswith("_")}
                                          for s in self.stack["sources"]])
        runner.write_json(self.store / "stack.json", stack)
        runner.write_json(self.store / "target.json", {
            "host": "127.0.0.1", "port": self.target.port, "database": "postgres", "user": "postgres",
            "password": {"secret_ref": "target-password"}, "local_fixture": True, "run_id": run_id})
        command = [str(self.binary), "--stack", str(self.store / "stack.json"), "--source", sid,
                   "--target", str(self.store / "target.json"), "--secrets-dir", str(self.store)]
        chunk = next(s for s in self.stack["sources"] if s["id"] == sid)["_chunk_rows"]
        if chunk and mode != "--handoff":
            command += ["--chunk-rows", str(chunk)]
        if mode:
            policy_file = self.store / f"policy-{self.calls}.json"
            runner.write_json(policy_file, policy)
            runner.write_json(self.run.path / f"policy-{self.calls}.json", policy)
            command += [mode, str(policy_file)]
        return command

    def error(self, seq):
        raw = (self.run.path / f"{seq:04d}.stderr").read_text().strip().splitlines()
        for line in reversed(raw):
            try:
                return json.loads(line)["error"]
            except (ValueError, KeyError, TypeError):
                continue
        return raw[-1][:200] if raw else None

    def call(self, sid, run_id, mode=None, policy=None, required=True, timeout=900):
        code, out = self.run.command(self.argv(sid, run_id, mode, policy), timeout=timeout, required=False)
        error = self.error(self.run.seq) if code else None
        if error and "connect" in error:
            self.connect_diagnostics(self.run.seq)
        if required and code:
            raise RuntimeError(f"oo-load {mode or 'snapshot'} {sid} failed: {error}")
        return code, rows(out), error

    def connect_diagnostics(self, seq):
        """Server-side view when the loader cannot connect: sessions and FATALs."""
        if self.run.engine == "postgres":
            _, sessions = self.src("SELECT usename,state,count(*) FROM pg_stat_activity "
                                   "WHERE usename IS NOT NULL GROUP BY 1,2 ORDER BY 1,2;", required=False)
            logs = ("/var/lib/postgresql/18/docker" if int(self.run.version) >= 18
                    else "/var/lib/postgresql/data") + "/log/oo-audit.csv"
            _, fatal = self.run.command(["docker", "exec", self.run.name, "bash", "-c",
                                         f"grep -h 'FATAL' {logs} | tail -5 | cut -c1-300"], required=False)
        else:
            _, sessions = self.src("SELECT USER,COMMAND,COUNT(*) FROM information_schema.PROCESSLIST "
                                   "GROUP BY 1,2;", required=False)
            fatal = None
        runner.write_json(self.run.path / f"{seq:04d}.connect-diagnostics.json",
                          {"sessions": sessions, "fatal": fatal})

    def spawn(self, sid, run_id, mode=None, policy=None):
        command = self.argv(sid, run_id, mode, policy)
        self.run.seq += 1
        prefix = self.run.path / f"{self.run.seq:04d}"
        runner.write_json(prefix.with_suffix(".json"), {
            "argv": [x.replace(str(ROOT), "<worktree>") for x in command], "background": True})
        out, err = prefix.with_suffix(".stdout").open("w"), prefix.with_suffix(".stderr").open("w")
        proc = subprocess.Popen(command, stdout=out, stderr=err, text=True, cwd=ROOT)
        proc.files, proc.seq, proc.started = (out, err), self.run.seq, time.monotonic()
        return proc

    def finish(self, proc, timeout=600):
        try:
            code = proc.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            # Where did it hang? Keep a stack sample before killing (macOS `sample`).
            sample = self.run.path / f"{proc.seq:04d}.hang-sample.txt"
            subprocess.run(["sample", str(proc.pid), "3", "-file", str(sample)],
                           capture_output=True, timeout=60)
            sockets = subprocess.run(["lsof", "-a", "-p", str(proc.pid), "-i", "-n", "-P"],
                                     capture_output=True, text=True, timeout=60).stdout
            activity = {}
            for side, query in (("source", self.src), ("target", self.tgt)):
                code, text = query("SELECT usename,application_name,state,wait_event_type,wait_event,"
                                   "left(query,80) FROM pg_stat_activity WHERE usename IS NOT NULL;"
                                   if self.run.engine == "postgres" or side == "target" else
                                   "SELECT USER,COMMAND,STATE,LEFT(INFO,80) FROM information_schema.PROCESSLIST;",
                                   required=False)
                activity[side] = text if code == 0 else None
            runner.write_json(self.run.path / f"{proc.seq:04d}.hang.json",
                              {"sockets": sockets, "activity": activity})
            proc.kill()
            proc.wait()
            code = 124
        for file in proc.files:
            file.close()
        prefix = self.run.path / f"{proc.seq:04d}"
        record = json.loads(prefix.with_suffix(".json").read_text())
        record.update(exit=code, elapsed_seconds=round(time.monotonic() - proc.started, 3))
        runner.write_json(prefix.with_suffix(".json"), record)
        text = prefix.with_suffix(".stdout").read_text()
        return code, rows(text), (self.error(proc.seq) if code else None)

    # Agent DB checks ------------------------------------------------------
    def state(self):
        parts = []
        for table in ("current", "sync_state", "c5_state"):
            _, exists = self.tgt(f"SELECT to_regclass('_oo_load.{table}') IS NOT NULL;")
            if exists.strip() == "t":
                parts.append(self.tgt(f"SELECT '{table}',row_to_json(x)::text FROM _oo_load.{table} x ORDER BY 2;")[1])
        return hashlib.sha256("\n".join(parts).encode()).hexdigest()

    def chunks(self):
        code, text = self.tgt("SELECT count(*) FROM _oo_load.chunks;", required=False)
        return int(text.strip()) if code == 0 and text.strip() else 0

    def binding_hash(self, sid):
        s = next(s for s in self.stack["sources"] if s["id"] == sid)
        text = json.dumps([s["id"], s["engine"], s["host"], s["port"], s["database"], s["user"]],
                          separators=(",", ":"))
        return hashlib.sha256(text.encode()).hexdigest()

    def position(self, sid):
        """Durable C1 position of one binding, read from the Agent DB."""
        code, text = self.tgt(f"SELECT position::text FROM _oo_load.sync_state WHERE source="
                              f"'{self.binding_hash(sid)}';", required=False)
        return json.loads(text) if code == 0 and text.strip() else None

    def key(self):
        self.next_key += 10000
        return self.next_key

    def source_columns(self, table):
        if self.run.engine == "mysql":
            sql = (f"SELECT COLUMN_NAME,DATA_TYPE FROM information_schema.COLUMNS WHERE TABLE_SCHEMA='fixture' "
                   f"AND TABLE_NAME='{table}' ORDER BY ORDINAL_POSITION;")
        else:
            sql = (f"SELECT column_name,data_type FROM information_schema.columns WHERE table_schema='public' "
                   f"AND table_name='{table}' ORDER BY ordinal_position;")
        _, text = self.src(sql)
        return [(n, type_class(k)) for n, k in (line.split("\t") for line in text.splitlines() if line)]

    def target_columns(self, relation):
        _, text = self.tgt(f"SELECT attname,format_type(atttypid,atttypmod) FROM pg_attribute WHERE "
                           f"attrelid='{relation}'::regclass AND attnum>0 AND NOT attisdropped "
                           f"AND attname NOT LIKE '\\_\\_oo%' ORDER BY attnum;")
        return [(n, type_class(k)) for n, k in (line.split("\t") for line in text.splitlines() if line)]

    def compare(self, sid):
        """Columns (name, class) and every row, ordered by the integer id."""
        result = {}
        for table in self.tables(sid):
            relation = self.relations.get((sid, table))
            columns = self.source_columns(table)
            if not columns:
                result[table] = {"equal": False, "reason": "source_table_missing"}
                continue
            if relation is None:
                result[table] = {"equal": False, "reason": "no_target_relation"}
                continue
            target_columns = self.target_columns(relation)
            if target_columns != columns:
                result[table] = {"equal": False, "reason": "columns", "source": columns, "target": target_columns}
                continue
            names = [n for n, _ in columns]
            mysql = ",".join(f"`{n}`" for n in names) if self.run.engine == "mysql" else ",".join(f'"{n}"' for n in names)
            _, expected = self.src(f"SELECT {mysql} FROM {table} ORDER BY id;")
            _, actual = self.tgt(f"SELECT {','.join(chr(34)+n+chr(34) for n in names)} FROM {relation} ORDER BY id;")
            result[table] = {"equal": expected == actual, "rows": len(expected.splitlines()),
                             "source_sha256": hashlib.sha256(expected.encode()).hexdigest(),
                             "target_sha256": hashlib.sha256(actual.encode()).hexdigest()}
        return result

    @staticmethod
    def equal(comparison):
        return bool(comparison) and all(v["equal"] for v in comparison.values())

    def remember(self, sid, receipts):
        for receipt in receipts:
            for item in receipt.get("bindings", [receipt]):
                if "table" in item and "target" in item:
                    table = item["table"]["name"] if isinstance(item["table"], dict) else item["table"].split(".")[-1]
                    self.relations[(sid, table)] = item["target"]

    # Bindings ---------------------------------------------------------------
    def snapshot(self, sid, tables, run_id=None, chunk_rows=None):
        self.add_source(sid, tables, chunk_rows)
        _, receipts, _ = self.call(sid, run_id or sid + "-base")
        self.remember(sid, receipts)
        return receipts

    def c5_binding(self, sid, tables, chunk_rows=None):
        self.snapshot(sid, tables, chunk_rows=chunk_rows)
        self.sync[sid] = {"cycle": 1}
        return self.c5(sid)

    def c5(self, sid, required=True, run_id=None):
        cycle = self.sync[sid]["cycle"]
        run_id = run_id or f"{sid}-c5-{cycle}"
        for _ in range(3):
            code, receipts, error = self.call(sid, run_id, "--c5", {"cycle": cycle, "full_read_every": 1000},
                                              required=False)
            if error != "c5_range_changed_retry":
                break
        if code == 0:
            self.sync[sid]["cycle"] += 1
        if required and code:
            raise RuntimeError(f"c5 {sid} failed: {error}")
        return code, receipts, error

    def c1_binding(self, sid, tables):
        if self.run.engine == "mysql":
            self.snapshot(sid, tables)
            server_id = next(self.server_ids)
            _, receipts, _ = self.call(sid, sid + "-handoff", "--handoff", {
                "sources": [{"source": sid, "server_id": server_id}], "max_keys": 100000,
                "max_bytes": 33554432, "required_history_seconds": 2})
            self.remember(sid, receipts)
            handoff = receipts[0]
            self.sync[sid] = {"initial_position": handoff["positions"][sid],
                              "source_epoch": handoff["source_epochs"][sid], "baseline_aligned": True,
                              "max_batches": 100, "server_id": next(self.server_ids),
                              "longest_cycle_seconds": 1, "recovery_seconds": 1}
        else:
            slot = "s_" + sid
            self.src(f"CREATE PUBLICATION p_{sid} FOR TABLE {','.join(tables)} "
                     "WITH(publish_via_partition_root=true);")
            _, lsn = self.src(f"SELECT lsn FROM pg_create_logical_replication_slot('{slot}','pgoutput');")
            hi, lo = lsn.strip().split("/")
            # Quiescent fixture between slot creation and snapshot: caller-asserted alignment.
            self.snapshot(sid, tables)
            self.sync[sid] = {"initial_position": {"kind": "pg_lsn", "lsn": (int(hi, 16) << 32) | int(lo, 16)},
                              "baseline_aligned": True, "slot": slot, "publication": "p_" + sid,
                              "required_wal_headroom": 1, "max_batches": 100}

    def c1(self, sid, required=True, drain=True):
        results = []
        for _ in range(8 if drain else 1):
            code, receipts, error = self.call(sid, f"{sid}-sync-{self.calls}", "--sync", self.sync[sid], required=False)
            results.append({"exit": code, "error": error, "receipts": receipts})
            if code or not drain or all(r.get("batches", 0) == 0 for r in receipts):
                break
        if required and code:
            raise RuntimeError(f"c1 {sid} failed: {error}")
        return code, results, error

    def retire(self, sid):
        if self.run.engine == "postgres" and "slot" in self.sync.get(sid, {}):
            self.src(f"SELECT pg_drop_replication_slot(slot_name) FROM pg_replication_slots "
                     f"WHERE slot_name='{self.sync[sid]['slot']}';", required=False)

    def dml(self, tables, start, count=4, columns=("id", "payload", "n")):
        statements = []
        for table in tables:
            for i in range(count):
                key = start + i
                values = {"id": str(key), "payload": f"'v{key}'", "label": f"'v{key}'", "n": str(key),
                          "extra": f"'x{key}'"}
                statements.append(f"INSERT INTO {table}({','.join(columns)}) VALUES("
                                  f"{','.join(values[c] for c in columns)});")
            text = "label" if "label" in columns else "payload"
            statements.append(f"UPDATE {table} SET {text}='upd{start}' WHERE id={start};")
            statements.append(f"DELETE FROM {table} WHERE id={start + count - 1};")
        if self.run.engine == "mysql":
            self.src("".join(statements))
        else:
            self.src("BEGIN;" + "".join(statements) + "COMMIT;")


def probe(port, kind, tls=False):
    """Protocol-level reachability: Docker Desktop accepts TCP even with no backend.

    With `tls`, a PostgreSQL probe also completes the TLS handshake (multi-KB
    server flight), not only the one-byte SSLRequest answer.
    """
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=5) as sock:
            sock.settimeout(5)
            if kind == "postgres":
                sock.sendall(b"\x00\x00\x00\x08\x04\xd2\x16\x2f")  # SSLRequest
                answer = sock.recv(1)
                if not tls or answer != b"S":
                    return answer in (b"S", b"N") and not tls
                # Reachability only: the handshake must complete. OpenSSL cannot
                # verify the fixture chain (CA and leaf share CN=localhost; the
                # loader's rustls verifier does); verification is oo-load's job.
                context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
                context.check_hostname = False
                context.verify_mode = ssl.CERT_NONE
                with context.wrap_socket(sock, server_hostname="localhost"):
                    return True
            return len(sock.recv(4)) == 4  # MySQL server greeting header
    except (OSError, ssl.SSLError):
        return False


def wait_until(predicate, timeout, interval=0.2):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return True
        time.sleep(interval)
    return False


# Faults ---------------------------------------------------------------------
def inject(h, fault, proc):
    run = h.run
    if fault == "source_restart":
        run.command(["docker", "restart", run.name], timeout=600)
    elif fault == "target_restart":
        run.command(["docker", "restart", h.target.name], timeout=600)
    elif fault == "network_drop":
        run.command(["docker", "network", "disconnect", "bridge", run.name])
        h.disconnected = True
    elif fault == "sigkill":
        proc.killed = True
        proc.send_signal(signal.SIGKILL)
    else:
        raise ValueError(fault)


def interrupted(h, fault, proc, progressed, outage=20):
    """Inject `fault` once the background process has visibly progressed."""
    injected = wait_until(lambda: progressed() or proc.poll() is not None, 900) and proc.poll() is None
    step = {"fault": fault, "injected_mid_run": injected}
    if injected:
        inject(h, fault, proc)
        step["injected_after_seconds"] = round(time.monotonic() - proc.started, 3)
        if fault == "network_drop":
            # A bounded outage; reconnect while the loader may still be running.
            time.sleep(outage)
            h.run.command(["docker", "network", "connect", "bridge", h.run.name])
            h.disconnected, h.pending_flush = False, True
            step["outage_seconds"] = outage
    # The loader paces itself at >=3x each stalled cycle (25% duty), so a
    # stall during the outage legitimately extends the run well past it.
    code, receipts, error = h.finish(proc, timeout=900)
    if fault == "network_drop":
        time.sleep(1)
    step.update(first_exit=code, first_error=error, hung_past_timeout=code == 124)
    h.heal()
    return step


def first_ok(step, fault):
    """The faulted attempt itself must end by name (or by our SIGKILL), never hang."""
    return step["first_exit"] != 124 and (step["first_exit"] == 0 or fault == "sigkill"
                                          or named(step["first_error"]))


def monotonic(positions):
    def order(p):
        return (p["file"], p["position"]) if "file" in p else (p.get("lsn", p.get("scn")),)
    return all(order(a) <= order(b) for a, b in zip(positions, positions[1:]))


def snapshot_fault(h, sid, fault):
    h.add_source(sid, ["bulk"], CHUNK_ROWS)
    before = h.chunks()
    proc = h.spawn(sid, sid + "-base")
    step = interrupted(h, fault, proc, lambda: h.chunks() >= before + 3)
    code, receipts, error = h.call(sid, sid + "-base", required=False)
    h.remember(sid, receipts)
    chunk_ids = [r["chunk_id"] for receipt in receipts for r in receipt.get("receipts", [])]
    step.update(resume_exit=code, resume_error=error, receipts=len(chunk_ids),
                duplicate_receipts=len(chunk_ids) != len(set(chunk_ids)),
                comparison=h.compare(sid) if code == 0 else None)
    step["passed"] = bool(step["injected_mid_run"] and first_ok(step, fault) and code == 0
                          and chunk_ids and not step["duplicate_receipts"] and h.equal(step["comparison"]))
    return step


def grow(h):
    """Ordinary growth past MAX(id) that keeps every range inside its row budget."""
    if h.run.engine == "oracle":
        return h.grow()
    _, top = h.src("SELECT MAX(id) FROM bulk;")
    top = int(top.strip())
    h.src(f"DELETE FROM bulk WHERE id > {top - 11};" + "INSERT INTO bulk VALUES" + ",".join(
        f"({top + i},'grown-{top + i}',{i})" for i in range(1, 6)) + ";")


def c5_fault(h, sid, fault):
    h.c5_binding(sid, ["bulk"], CHUNK_ROWS)
    h.src("UPDATE bulk SET n=n+1 WHERE MOD(id,37) = 0;")
    grow(h)
    before = h.chunks()
    cycle = h.sync[sid]["cycle"]
    proc = h.spawn(sid, f"{sid}-c5-{cycle}", "--c5", {"cycle": cycle, "full_read_every": 1000})
    step = interrupted(h, fault, proc, lambda: h.chunks() >= before + 2)
    code, receipts, error = h.c5(sid, required=False)
    step.update(resume_exit=code, resume_error=error, resume=receipts,
                comparison=h.compare(sid) if code == 0 else None)
    step["passed"] = bool(step["injected_mid_run"] and first_ok(step, fault) and code == 0
                          and h.equal(step["comparison"]))
    return step


def c1_fault(h, sid, fault):
    h.c1_binding(sid, ["items", "other"])
    h.c1(sid)
    base = h.key()
    for i in range(60):
        h.dml(["items", "other"], base + i * 10, count=3)
    before = h.position(sid)
    proc = h.spawn(sid, sid + "-sync", "--sync", h.sync[sid])
    step = interrupted(h, fault, proc, lambda: h.position(sid) not in (before, None))
    step["position_before"], step["position_at_fault"] = before, h.position(sid)
    code, results, error = h.c1(sid, required=False)
    positions = [before, step["position_at_fault"]] + [
        x["durable_position"] for r in results for x in r["receipts"] if x.get("durable_position")]
    step.update(resume_exit=code, resume_error=error, resume=results, position_after=h.position(sid),
                positions_monotonic=monotonic([p for p in positions if p]),
                comparison=h.compare(sid) if code == 0 else None)
    step["passed"] = bool(step["injected_mid_run"] and first_ok(step, fault) and code == 0
                          and step["positions_monotonic"] and h.equal(step["comparison"]))
    h.retire(sid)
    return step


def case_result(steps):
    return {"status": "passed" if steps and all(s.get("passed") for s in steps) else "failed", "steps": steps}


def t3_source_restart_mid_chunk(h):
    return case_result([dict(snapshot_fault(h, "t3sr_snap", "source_restart"), path="snapshot")])


def t3_network_drop_mid_c1(h):
    if "c1" not in h.paths:
        return {"status": "not_applicable", "reason": "C5-only version; C1 not requested"}
    return case_result([dict(c1_fault(h, "t3nd_c1", "network_drop"), path="c1")])


def t3_network_drop_mid_c5(h):
    return case_result([dict(c5_fault(h, "t3nd_c5", "network_drop"), path="c5")])


def t3_target_restart_mid_publish(h):
    return case_result([dict(snapshot_fault(h, "t3tr_snap", "target_restart"), path="snapshot"),
                        dict(c5_fault(h, "t3tr_c5", "target_restart"), path="c5"),
                        *([dict(c1_fault(h, "t3tr_c1", "target_restart"), path="c1")] if "c1" in h.paths else [])])


def t3_sigkill_mid_cycle(h):
    return case_result([dict(snapshot_fault(h, "t3sk_snap", "sigkill"), path="snapshot"),
                        dict(c5_fault(h, "t3sk_c5", "sigkill"), path="c5"),
                        *([dict(c1_fault(h, "t3sk_c1", "sigkill"), path="c1")] if "c1" in h.paths else [])])


def refused_then_rebaseline(h, sid, label, mutate, tables=("items", "other")):
    """Fault destroys history: require a named refusal, unchanged target, then a new generation."""
    h.c1_binding(sid, list(tables))
    h.dml(list(tables), h.key())
    h.c1(sid)
    h.dml(list(tables), h.key())
    evidence = mutate() or {}
    if h.run.engine == "mysql":
        # Let surviving history age past longest cycle + recovery (2 s), so a
        # refusal is caused by the lost/replaced history, not by its age.
        time.sleep(4)
    before = h.state()
    code, results, error = h.c1(sid, required=False, drain=False)
    step = {"path": "c1", "fault": label, "exit": code, "error": error, "detail": evidence,
            "target_state_unchanged": before == h.state()}
    step["outcome"] = classify(code, error, code == 0 and h.equal(h.compare(sid)), step["target_state_unchanged"])
    h.retire(sid)
    if step["outcome"] == "named_refusal":
        g2 = sid + "_g2"
        if h.run.engine == "mysql":
            time.sleep(3)  # actual binlog history must cover longest cycle + recovery
        h.c1_binding(g2, list(tables))
        h.dml(list(tables), h.key())
        code2, _, error2 = h.c1(g2, required=False)
        step.update(rebaseline_exit=code2, rebaseline_error=error2,
                    rebaseline_comparison=h.compare(g2) if code2 == 0 else None)
        h.retire(g2)
    step["passed"] = (step["outcome"] == "named_refusal" and step.get("rebaseline_exit") == 0
                      and h.equal(step["rebaseline_comparison"])) or (
        step["outcome"] == "online_apply")
    return step


def mysql_binlogs(h):
    _, text = h.src("SHOW BINARY LOGS;")
    return [line.split("\t")[0] for line in text.splitlines() if line]


def t3_retention_expiry(h):
    if h.run.engine == "oracle":
        return h.retention_expiry()
    if h.run.engine == "mysql":
        def expire():
            durable = h.sync["t3re_c1"]
            last = h.position("t3re_c1")
            before = mysql_binlogs(h)
            if h.run.version == "5.7":
                h.src("FLUSH LOCAL BINARY LOGS;")
                time.sleep(2)
                h.src("PURGE BINARY LOGS BEFORE NOW();")
                method = "PURGE BINARY LOGS BEFORE NOW() (5.7 expire_logs_days has 1-day granularity)"
            else:
                h.src("SET GLOBAL binlog_expire_logs_seconds=1;")
                for _ in range(3):
                    time.sleep(2)
                    h.src("FLUSH LOCAL BINARY LOGS;")
                h.src("SET GLOBAL binlog_expire_logs_seconds=2592000;")
                method = "binlog_expire_logs_seconds=1 + rotation"
            return {"method": method, "binlogs_before": before, "binlogs_after": mysql_binlogs(h),
                    "durable_position": last, "configured_initial": durable["initial_position"]}
        step = refused_then_rebaseline(h, "t3re_c1", "binlog_retention_expiry", expire)
    else:
        def drop():
            h.src("SELECT pg_drop_replication_slot('s_t3re_c1');")
            return {"method": "DBA dropped the replication slot; retained WAL released"}
        step = refused_then_rebaseline(h, "t3re_c1", "slot_dropped_wal_released", drop)
    return case_result([step])


def t3_slot_invalidation(h):
    if h.run.engine != "postgres":
        return {"status": "not_applicable", "reason": "PostgreSQL replication slots only"}
    steps = []

    def lose():
        h.src("ALTER SYSTEM SET max_slot_wal_keep_size='1MB';SELECT pg_reload_conf();")
        status = None
        for i in range(12):
            # 1.5 KB rows stay below the TOAST threshold, so WAL is not compressed away.
            h.src(f"INSERT INTO scratch SELECT g+{i}*100000,repeat(md5(g::text),46) FROM generate_series(1,4000) g;"
                  "SELECT pg_switch_wal();CHECKPOINT;SELECT pg_switch_wal();CHECKPOINT;")
            _, status = h.src("SELECT wal_status FROM pg_replication_slots WHERE slot_name='s_t3si_c1';")
            if status.strip() == "lost":
                break
        h.src("ALTER SYSTEM RESET max_slot_wal_keep_size;SELECT pg_reload_conf();")
        detail = {"method": "max_slot_wal_keep_size=1MB + WAL switch + CHECKPOINT", "wal_status": status.strip()}
        if int(h.run.version) >= 17:
            detail["invalidation_reason"] = h.src("SELECT invalidation_reason FROM pg_replication_slots "
                                                  "WHERE slot_name='s_t3si_c1';")[1].strip()
        if status.strip() != "lost":
            raise RuntimeError("slot was not invalidated")
        return detail
    steps.append(refused_then_rebaseline(h, "t3si_c1", "slot_wal_removed", lose))
    if int(h.run.version) >= 18:
        def idle():
            _, unit = h.src("SELECT unit FROM pg_settings WHERE name='idle_replication_slot_timeout';")
            value = "1min"
            h.src(f"ALTER SYSTEM SET idle_replication_slot_timeout='{value}';SELECT pg_reload_conf();")
            reason = None
            for _ in range(40):
                time.sleep(5)
                h.src("CHECKPOINT;")
                _, reason = h.src("SELECT invalidation_reason FROM pg_replication_slots WHERE slot_name='s_t3it_c1';")
                if reason.strip() == "idle_timeout":
                    break
            h.src("ALTER SYSTEM RESET idle_replication_slot_timeout;SELECT pg_reload_conf();")
            if reason.strip() != "idle_timeout":
                raise RuntimeError("idle slot was not invalidated")
            return {"method": "idle_replication_slot_timeout=1min + CHECKPOINT", "unit": unit.strip(),
                    "invalidation_reason": reason.strip()}
        steps.append(refused_then_rebaseline(h, "t3it_c1", "slot_idle_timeout", idle))
    else:
        steps.append({"path": "c1", "fault": "slot_idle_timeout", "passed": True, "outcome": "not_applicable",
                      "reason": "idle_replication_slot_timeout exists from PostgreSQL 18"})
    return case_result(steps)


def t3_gtid_purge(h):
    if h.run.engine != "mysql":
        return {"status": "not_applicable", "reason": "MySQL GTID only"}
    steps = []

    def purge():
        h.src("FLUSH LOCAL BINARY LOGS;")
        newest = mysql_binlogs(h)[-1]
        h.src(f"PURGE BINARY LOGS TO '{newest}';")
        _, purged = h.src("SELECT @@GLOBAL.gtid_purged;")
        return {"method": "FLUSH + PURGE BINARY LOGS TO newest", "gtid_purged": purged.strip().replace("\n", ""),
                "binlogs_after": mysql_binlogs(h)}
    steps.append(refused_then_rebaseline(h, "t3gp_c1", "gtid_purged", purge))

    def reset():
        # History replacement: the same file name later reappears with different content.
        durable = h.position("t3rs_c1") or {}
        reset_sql = "RESET MASTER;" if h.run.version in ("5.7", "8.0") else "RESET BINARY LOGS AND GTIDS;"
        h.src(reset_sql)
        files = mysql_binlogs(h)
        wanted = durable.get("file")
        guard = 0
        while wanted and wanted not in files and guard < 50:
            h.src("FLUSH LOCAL BINARY LOGS;")
            files = mysql_binlogs(h)
            guard += 1
        for i in range(40):
            _, text = h.src("SHOW BINARY LOGS;")
            sizes = {l.split("\t")[0]: int(l.split("\t")[1]) for l in text.splitlines() if l}
            if sizes.get(wanted, 0) > durable.get("position", 0) + 200:
                break
            h.src("INSERT INTO scratch(id,pad) VALUES" + ",".join(
                f"({900000 + i * 100 + j},REPEAT('r',200))" for j in range(50)) + ";")
        return {"method": reset_sql + " then rotate/write until the durable file name and offset exist again",
                "durable": durable, "binlog_sizes": sizes}
    steps.append(refused_then_rebaseline(h, "t3rs_c1", "gtid_history_reset", reset))
    return case_result(steps)


# T4 -------------------------------------------------------------------------
def ddl(h, case):
    if case == "widen_string":
        table = "t4_widen_string"
        statement = (f"ALTER TABLE {table} MODIFY (payload VARCHAR2(160));" if h.run.engine == "oracle" else
                     f"ALTER TABLE {table} MODIFY payload varchar(160) NOT NULL;" if h.run.engine == "mysql" else
                     f"ALTER TABLE {table} ALTER COLUMN payload TYPE varchar(160);")
        return [statement], ("id", "payload", "n")
    if h.run.engine == "oracle":
        from oracle_t3t4 import ddl as oracle_ddl
        return oracle_ddl(case)
    my = h.run.engine == "mysql"
    t = "t4_" + case
    if case == "add_column":
        return [f"ALTER TABLE {t} ADD COLUMN extra varchar(40) NULL;"], ("id", "payload", "n", "extra")
    if case == "drop_column":
        return [f"ALTER TABLE {t} DROP COLUMN n;"], ("id", "payload")
    if case == "rename_column":
        if my and h.run.version.startswith("5."):
            return [f"ALTER TABLE {t} CHANGE COLUMN payload label varchar(80) NOT NULL;"], ("id", "label", "n")
        return [f"ALTER TABLE {t} RENAME COLUMN payload TO label;"], ("id", "label", "n")
    if case == "widen_type":
        return [f"ALTER TABLE {t} MODIFY COLUMN n bigint NOT NULL;" if my else
                f"ALTER TABLE {t} ALTER COLUMN n TYPE bigint;"], ("id", "payload", "n")
    if case == "add_table":
        return [f"CREATE TABLE {t}_new(id bigint NOT NULL PRIMARY KEY,payload varchar(80) NOT NULL,n int NOT NULL)"
                f"{' DEFAULT CHARSET=utf8mb4' if my else ''};",
                f"INSERT INTO {t}_new VALUES(1,'new-1',1),(2,'new-2',2);"], ("id", "payload", "n")
    if case == "drop_table":
        return [f"DROP TABLE {t};"], ("id", "payload", "n")
    if case == "pk_drop":
        return [f"ALTER TABLE t4_pk_change DROP PRIMARY KEY;" if my else
                "ALTER TABLE t4_pk_change DROP CONSTRAINT t4_pk_change_pkey;"], ("id", "payload", "n")
    if case == "pk_add":
        return ["ALTER TABLE t4_pk_change ADD PRIMARY KEY(id,payload);"], ("id", "payload", "n")
    raise ValueError(case)


def scope(case, generation):
    base = "t4_" + case
    if case == "add_table":
        return [base] if generation == 1 else [base, base + "_new"]
    if case == "drop_table":
        return [base, base + "_b"] if generation == 1 else [base + "_b"]
    return [base]


def attempt(h, sid, path):
    before = h.state()
    if path == "c5":
        code, receipts, error = h.c5(sid, required=False)
        results = receipts
    else:
        code, results, error = h.c1(sid, required=False)
    comparison = h.compare(sid) if code == 0 else None
    unchanged = before == h.state()
    return {"exit": code, "error": error, "comparison": comparison, "target_state_unchanged": unchanged,
            "table_results": results if path == "c5" else [],
            "outcome": classify(code, error, code == 0 and h.equal(comparison), unchanged)}


def t4_case(h, case):
    strict = case in ("add_column", "drop_column", "rename_column", "widen_type", "widen_string")
    if strict:
        from t9_isolation import schema_case
        steps = []
        for path in h.paths:
            # Oracle LogMiner reports identifiers over 30 bytes as UNSUPPORTED;
            # keep every engine's fixture names within that limit.
            sid = "t4_" + case + "_" + path
            try:
                result = schema_case(h, case, tag=sid, mode=path)
                steps.append(dict(result, stage="published_projection", passed=result["status"] == "passed"))
            finally:
                h.retire(sid)
        return case_result(steps)
    paths = {}
    for path in h.paths:
        sid = f"t4{T4.index(case)}{path}"
        tables = scope(case, 1)
        if path == "c5":
            h.c5_binding(sid, tables)
        else:
            h.c1_binding(sid, tables)
            h.c1(sid)
        paths[path] = {"sid": sid, "steps": [], "refused": False}
    phases = ["pk_drop", "pk_add"] if case == "pk_change" else [case]
    key = h.key() * 10
    for number, phase in enumerate(phases):
        statements, columns = ddl(h, phase)
        for statement in statements:
            h.src(statement)
        if case == "add_table" and h.run.engine == "oracle":
            # An unrequested table is invisible to capture; exercise scope growth,
            # not just a DDL on a table the comparison never looks at.
            for item in paths.values():
                h.add_source(item["sid"], scope(case, 2))
        live = [t for t in scope(case, 2 if case == "drop_table" else 1)]
        for stage in ("after_ddl", "after_dml"):
            if stage == "after_dml":
                if phase == "pk_drop" and h.run.engine == "postgres":
                    # Published table without replica identity: source rejects UPDATE/DELETE.
                    h.src("".join(f"INSERT INTO {t}(id,payload,n) VALUES({key + i},'nopk',{i});"
                                  for t in live for i in range(3)))
                else:
                    h.dml(live, key + number * 1000, columns=columns)
                    if phase == "widen_type":
                        h.src(f"UPDATE t4_widen_type SET n=5000000000 WHERE id=2;")
            for path, item in paths.items():
                if item["refused"]:
                    continue
                result = attempt(h, item["sid"], path)
                result.update(phase=phase, stage=stage)
                item["steps"].append(result)
                item["refused"] = result["outcome"] != "online_apply"
        # Rebaseline each refused path as a new generation; unsupported shapes must refuse by name.
        for path, item in paths.items():
            if not item["refused"]:
                continue
            h.retire(item["sid"])
            sid = f"{item['sid']}g{number + 2}"
            tables = scope(case, 2)
            try:
                if path == "c5":
                    h.c5_binding(sid, tables)
                else:
                    h.c1_binding(sid, tables)
                if not (phase == "pk_drop" and h.run.engine == "postgres"):
                    h.dml(tables, h.key() * 10, columns=columns)
                result = attempt(h, sid, path)
                result.update(phase=phase, stage="rebaseline", sid=sid, unsupported_shape=phase == "pk_drop")
            except RuntimeError as error:
                text = str(error).split(" failed: ", 1)[-1]
                result = {"phase": phase, "stage": "rebaseline", "sid": sid, "exit": 1, "error": text,
                          "outcome": "named_refusal" if named(text) else "unnamed_failure",
                          "unsupported_shape": phase == "pk_drop"}
            item["steps"].append(result)
            item["sid"], item["refused"] = sid, result["outcome"] != "online_apply"
    steps = []
    for path, item in paths.items():
        for step in item["steps"]:
            ok = step["outcome"] in ("online_apply", "named_refusal")
            if step["stage"] == "rebaseline" and step["outcome"] != "online_apply":
                ok = step.get("unsupported_shape", False) and step["outcome"] == "named_refusal"
            steps.append(dict(step, path=path, passed=ok))
        last = item["steps"][-1] if item["steps"] else None
        steps.append({"path": path, "stage": "final_zero_divergence", "sid": item["sid"],
                      "passed": bool(last and last["outcome"] == "online_apply")})
        h.retire(item["sid"])
    return case_result(steps)


def t3_resetlogs_incarnation(h):
    if h.run.engine != "oracle":
        return {"status": "not_applicable", "reason": "Oracle incarnation only"}
    result = h.resetlogs_incarnation()
    for step in result["steps"]:
        if step.get("path") == "c5" and step.get("error") == "sync_all_stale":
            tables = step.get("table_results", [])
            step["passed"] = bool(
                step.get("exit") == 4 and step.get("target_state_unchanged")
                and step.get("epoch_before") != step.get("epoch_after") and tables
                and all(t.get("status") == "stale"
                        and t.get("stale_reason") == "rebaseline_required" for t in tables)
                and step.get("rebaseline", {}).get("outcome") == "online_apply")
    return case_result(result["steps"])

CASES = {name: globals()["t3_" + name] for name in T3}
CASES.update({name: (lambda case: lambda h: t4_case(h, case))(name) for name in T4})


def run_version(run, binary, cases):
    if run.engine == "oracle":
        from oracle_t3t4 import OracleHarness
        h = OracleHarness(run, binary)
    else:
        h = Harness(run, binary)
    report = {"engine": run.engine, "version": run.version, "run_id": run.id, "cases": {},
              "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "scope": "compact keyed bigint/int/varchar fixture; T3 faults and T4 DDL; not full type/scale",
              "consistency": "B"}
    try:
        h.prepare()
        version_sql = "SELECT VERSION();"
        if run.engine == "oracle":
            column = "version" if run.version == "11" else "version_full"
            version_sql = f"SELECT {column} FROM product_component_version WHERE product LIKE 'Oracle Database%';"
        _, version = h.src(version_sql)
        report["version_measured"] = version.strip().splitlines()[0][:120]
        report["operator_recoveries"] = h.recoveries
        for case in [c for c in ORDER if c in cases]:
            started = time.monotonic()
            try:
                h.heal()
                result = CASES[case](h)
            except Exception as error:  # driver text never reaches evidence; RuntimeErrors are ours
                result = {"status": "failed", "error": str(error) if isinstance(error, (RuntimeError, ValueError))
                          else type(error).__name__}
            result["elapsed_seconds"] = round(time.monotonic() - started, 1)
            report["cases"][case] = result
            runner.write_json(run.path / "t3t4.json", report)
            print(json.dumps({"version": run.version, "case": case, "status": result["status"]}), flush=True)
    except Exception as error:
        report["error"] = str(error) if isinstance(error, RuntimeError) else type(error).__name__
    finally:
        try:
            h.heal() if run.created_container and run.engine != "oracle" else None
        except Exception:
            pass
        if run.engine == "oracle":
            try:
                h.close_worker()
            except Exception as error:
                report["worker_cleanup_error"] = type(error).__name__
        try:
            h.target.close()
        except Exception as error:
            report["target_cleanup_error"] = type(error).__name__
        shutil.rmtree(h.store, ignore_errors=True)
        runner.write_json(run.path / "t3t4.json", report)
    return report


def disk_ready(min_free_gb, wait_minutes):
    deadline = time.monotonic() + wait_minutes * 60
    while shutil.disk_usage("/").free < min_free_gb * 1024 ** 3:
        if time.monotonic() >= deadline:
            return False
        time.sleep(60)
    return True


def accepted(result, cases):
    return (not result.get("error") and not result.get("cleanup_failures")
            and not result.get("version_error")
            and not result.get("target_cleanup_error")
            and not result.get("worker_cleanup_error")
            and set(result.get("cases", {})) == set(cases)
            and all(r["status"] in ("passed", "not_applicable") for r in result["cases"].values()))

def suite_verdicts(result, engine, version):
    verdicts = []
    for suite, required in (("T3", T3), ("T4", T4)):
        cases = {c: result.get("cases", {}).get(c, {}) for c in required}
        passed = (not any(result.get(k) for k in
                          ("error", "cleanup_failures", "target_cleanup_error", "worker_cleanup_error"))
                  and all(r.get("status") in ("passed", "not_applicable") for r in cases.values())
                  and any(r.get("status") == "passed" for r in cases.values()))
        for variant in ("c1", "c5"):
            outcome = "pass" if passed else "fail"
            if engine == "oracle" and version in ("11", "21") and variant == "c1":
                outcome = "skip"
            verdicts.append((suite, variant, outcome))
    return verdicts

def main(argv=None):
    def stop(signum, frame):
        raise KeyboardInterrupt(f"signal {signum}")
    signal.signal(signal.SIGTERM, stop)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--engine", choices=["mysql", "postgres", "oracle"], required=True)
    parser.add_argument("--versions", required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--cases", default=",".join(ORDER))
    parser.add_argument("--binary", type=Path, default=ROOT / "target/debug/oo-load")
    parser.add_argument("--min-free-gb", type=float, default=20.0)
    parser.add_argument("--disk-wait-minutes", type=float, default=90.0)
    parser.add_argument("--worker-image", help="digest-pinned 11g thick client image")
    args = parser.parse_args(argv)
    t1.LANE = os.environ.get("OO_TEST_LANE", "e" if args.engine == "oracle" else "n")
    versions = args.versions.split(",")
    allowed = {"mysql": MYSQL, "postgres": POSTGRES, "oracle": ORACLE}[args.engine]
    cases = args.cases.split(",")
    if not re.fullmatch(r"[A-Za-z0-9_-]+", args.run_id):
        parser.error("invalid run ID")
    if len(set(versions)) != len(versions) or any(v not in allowed for v in versions):
        parser.error("unsupported or duplicate versions")
    if len(set(cases)) != len(cases) or any(c not in ORDER for c in cases):
        parser.error("unknown or duplicate cases")
    if not args.binary.is_file():
        parser.error("build oo-load before starting database containers")
    if args.engine == "oracle" and "11" in versions and not args.worker_image:
        parser.error("Oracle 11 requires --worker-image (build the thick worker first)")
    root = ROOT / "evidence" / args.run_id
    root.mkdir(mode=0o700, exist_ok=False)
    results = {}
    lane = t1.LANE
    for version in versions:
        t1.LANE = "E19" if args.engine == "oracle" and version == "19" else lane
        if t1.LANE == "fhp":
            existing = subprocess.run(
                ["docker", "ps", "--filter", "label=oo.lane=fhp", "-q"],
                capture_output=True, text=True, check=True)
            if existing.stdout.strip():
                raise RuntimeError("fhp_fixture_resources_still_running")
        path = root / f"{args.engine}-{version}"
        path.mkdir()
        if not disk_ready(args.min_free_gb, args.disk_wait_minutes):
            # Shared host: never start containers below the free-space floor.
            results[version] = {"error": "host_disk_below_floor", "cases": {}, "cleanup_failures": [],
                                "min_free_gb": args.min_free_gb,
                                "observed_free_bytes": shutil.disk_usage("/").free}
            runner.write_json(path / "t3t4.json", results[version])
            from types import SimpleNamespace
            write_rows(SimpleNamespace(engine=args.engine, version=version, id=args.run_id),
                       path / "t3t4.json", suite_verdicts(results[version], args.engine, version))
            runner.write_json(root / "matrix.json", results)
            continue
        run = Source(path, args.engine, version,
                     argparse.Namespace(platform=None, health_timeout=1200, scale=1, tables=0,
                                        worker_image=args.worker_image))
        try:
            results[version] = run_version(run, args.binary.resolve(), cases)
        finally:
            try:
                run.record_version()
            except Exception:
                results.setdefault(version, {})["version_error"] = "version_unmeasured"
            results.setdefault(version, {})["cleanup_failures"] = run.cleanup()
            runner.write_json(path / "t3t4.json", results[version])
            write_rows(run, path / "t3t4.json", suite_verdicts(results[version], args.engine, version),
                       cleanup_failed=bool(results[version]["cleanup_failures"]))
            runner.checksums(path)
            runner.write_json(root / "matrix.json", results)
    runner.checksums(root)
    summary = {v: {c: r["status"] for c, r in res.get("cases", {}).items()} for v, res in results.items()}
    print(json.dumps({"run_id": args.run_id, "results": summary}))
    failed = any(not accepted(res, cases) for res in results.values())
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
