#!/usr/bin/env python3
"""T1 hook and lane-M matrix driver. Does not change lane H's runner API."""
import argparse
import copy
import datetime as dt
import hashlib
from decimal import Decimal
import json
import os
import re
from pathlib import Path
import secrets
import signal
import shutil
import socketserver
import subprocess
import threading
import time
import uuid

from comparison import compare
import runner

ROOT = runner.ROOT


class Relay:
    """Loopback transport to an owned fixture with no published port."""
    def __init__(self, container, port):
        self.children = []
        owner = self
        class Handler(socketserver.BaseRequestHandler):
            def handle(self):
                child = subprocess.Popen(
                    ["docker", "exec", "-i", container, "bash", "-c",
                     f"exec 3<>/dev/tcp/127.0.0.1/{port}; cat <&3 & cat >&3; kill $! 2>/dev/null"],
                    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
                owner.children.append(child)
                def send():
                    try:
                        while data := self.request.recv(65536):
                            child.stdin.write(data)
                            child.stdin.flush()
                    except (OSError, ValueError):
                        pass
                    finally:
                        child.stdin.close()
                thread = threading.Thread(target=send, daemon=True)
                thread.start()
                try:
                    while data := child.stdout.read1(65536):
                        self.request.sendall(data)
                except OSError:
                    pass
                finally:
                    child.terminate()
                    child.wait()
        self.server = socketserver.ThreadingTCPServer(("127.0.0.1", 0), Handler)
        self.server.daemon_threads = True
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.port = self.server.server_address[1]

    def close(self):
        self.server.shutdown()
        self.server.server_close()
        for child in self.children:
            if child.poll() is None:
                child.terminate()
                child.wait()


def private(path, text):
    path.write_text(text)
    path.chmod(0o600)


class LaneRun(runner.Run):
    """Reuse H's generator/seed SQL, but own only explicitly labelled M resources."""
    def command(self, command, **kwargs):
        command = ["oo.lane=m" if x == "oo.lane=h" else x for x in command]
        if command[:2] == ["docker", "create"] and self.name in command:
            port = 5432 if self.engine == "postgres" else 1521
            command[2:2] = ["-p", f"127.0.0.1::{port}"]
        return super().command(command, **kwargs)

    def cleanup(self):
        failures = []
        for kind, name, created in [("container", self.name, self.created_container),
                                    ("volume", self.volume, self.created_volume)]:
            if not created:
                continue
            try:
                fmt = "{{json .Config.Labels}}" if kind == "container" else "{{json .Labels}}"
                _, raw = self.command(["docker", kind, "inspect", "--format", fmt, name])
                labels = json.loads(raw)
                if labels.get("oo.lane") != "m" or labels.get("oo.run_id") != self.id:
                    raise RuntimeError("cleanup ownership")
                if kind == "container":
                    self.command(["docker", "logs", name], required=False)
                code, _ = self.command(["docker", kind, "rm"] + (["-f", "-v"] if kind == "container" else []) + [name],
                                       required=False)
                if code:
                    failures.append(name)
            except (RuntimeError, ValueError):
                failures.append(name)
        # Only this runner's generated private files.
        shutil.rmtree(self.private)
        runner.write_json(self.path / "cleanup.json", {"failures": failures})
        return failures


class AgentDB:
    def __init__(self, run):
        self.run = run
        self.name = "oo-m-target-" + uuid.uuid4().hex[:12]
        self.created = False
        self.password = secrets.token_hex(24)
        self.directory = run.path / ".target-private"
        self.directory.mkdir(mode=0o700)
        private(self.directory / "password", self.password)

    def start(self):
        item = runner.resolve("postgres", "17")
        _, arch = self.run.command(["docker", "info", "--format", "{{.Architecture}}"])
        platform = "linux/arm64" if arch.strip() in ("arm64", "aarch64") else "linux/amd64"
        image = item["ref"] + "@" + item["platforms"][platform]["digest"]
        self.run.command(["docker", "pull", "--platform", platform, image], timeout=1200)
        self.run.command(["docker", "create", "--name", self.name, "--label", "oo.lane=m",
                          "--label", "oo.run_id=" + self.run.id, "--platform", platform,
                          "-p", "127.0.0.1::5432", "--tmpfs", "/var/lib/postgresql/data",
                          "-v", f"{self.directory}:/run/private:ro",
                          "-e", "POSTGRES_PASSWORD_FILE=/tmp/pw", "--entrypoint", "bash", image,
                          "-c", "cp /run/private/password /tmp/pw; chown postgres:postgres /tmp/pw; "
                          "chmod 600 /tmp/pw; exec docker-entrypoint.sh postgres"])
        self.created = True
        self.run.command(["docker", "start", self.name])
        for _ in range(90):
            if self.run.command(["docker", "exec", self.name, "pg_isready", "-U", "postgres"],
                                required=False)[0] == 0:
                break
            time.sleep(1)
        else:
            raise RuntimeError("target health timeout")
        _, out = self.run.command(["docker", "port", self.name, "5432/tcp"])
        self.port = int(out.strip().rsplit(":", 1)[1])
        self.sql("CREATE COLLATION fixture_ci (provider=icu,locale='und-u-ks-level1',deterministic=false);")

    def sql(self, sql):
        return self.run.command(["docker", "exec", "-i", self.name, "psql", "-X", "-U", "postgres",
                                 "-v", "ON_ERROR_STOP=1", "-A", "-t"], data=sql)[1]

    def close(self):
        if self.created:
            _, raw = self.run.command(["docker", "inspect", "--format", "{{json .Config.Labels}}", self.name])
            labels = json.loads(raw)
            if labels.get("oo.lane") != "m" or labels.get("oo.run_id") != self.run.id:
                raise RuntimeError("target cleanup ownership")
            self.run.command(["docker", "rm", "-f", "-v", self.name])
        shutil.rmtree(self.directory)


def configure_pg(run, store):
    # All of these are fixture-admin actions, never product source writes.
    for cmd in [
        ["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", str(store / "ca-key"),
         "-out", str(store / "source-ca"), "-days", "2", "-subj", "/CN=localhost",
         "-addext", "basicConstraints=critical,CA:TRUE"],
        ["openssl", "req", "-newkey", "rsa:2048", "-nodes", "-keyout", str(store / "server-key"),
         "-out", str(store / "server-csr"), "-subj", "/CN=localhost"],
    ]:
        run.command(cmd)
    private(store / "extensions", "basicConstraints=critical,CA:FALSE\nsubjectAltName=DNS:localhost,IP:127.0.0.1\n"
            "keyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n")
    run.command(["openssl", "x509", "-req", "-in", str(store / "server-csr"),
                 "-CA", str(store / "source-ca"), "-CAkey", str(store / "ca-key"), "-CAcreateserial",
                 "-out", str(store / "server-cert"), "-days", "2", "-extfile", str(store / "extensions")])
    for file in ("server-key", "source-ca", "server-cert"):
        (store / file).chmod(0o600)
        run.command(["docker", "cp", str(store / file), run.name + ":/tmp/" + file])
    run.command(["docker", "exec", "-u", "root", run.name, "bash", "-c",
                 "chown postgres:postgres /tmp/server-key /tmp/server-cert; chmod 600 /tmp/server-key"])
    run.sql("ALTER SYSTEM SET ssl_cert_file='/tmp/server-cert';\n"
            "ALTER SYSTEM SET ssl_key_file='/tmp/server-key';\n"
            "ALTER SYSTEM SET ssl='on';\nSELECT pg_reload_conf();\n"
            f"ALTER ROLE fixture_reader PASSWORD '{run.reader_secret}';\n"
            # The connector sets read-only itself and refuses role startup GUCs.
            "ALTER ROLE fixture_reader RESET default_transaction_read_only;\n"
            "GRANT pg_read_all_stats TO fixture_reader;\n", admin=True)
    time.sleep(1)


def writer(run, stop, result):
    """Every committed cycle includes insert/update/delete/primary-key update."""
    run = copy.copy(run)
    run.seq = 1_000_000
    cycles = 0
    try:
        while not stop.is_set():
            key = 10_000_000 + cycles * 2
            sql = (f"INSERT INTO growth_events VALUES({key},1);\n"
                   f"UPDATE growth_events SET order_id=2 WHERE id={key};\n"
                   f"UPDATE growth_events SET id={key+1} WHERE id={key};\n"
                   f"DELETE FROM growth_events WHERE id={key+1};\n"
                   f"INSERT INTO growth_events VALUES({20_000_000+cycles},3);\nCOMMIT;\n")
            if run.engine == "oracle":
                sql = sql.replace(";\n", ";\nCOMMIT;\n")
            run.sql(sql, admin=True)
            cycles += 1
            stop.wait(0.15)
        result["cycles"] = cycles
    except Exception as error:
        result.update(cycles=cycles, error=type(error).__name__)


def compare_pg(run, target, receipt, table):
    _, columns_text = run.sql(
        "SELECT column_name||'|'||data_type FROM information_schema.columns "
        f"WHERE table_schema='public' AND table_name='{table}' ORDER BY ordinal_position;\n")
    columns = [line.split("|") for line in columns_text.splitlines() if line]
    if not columns:
        raise RuntimeError("comparison metadata missing")
    names = [c[0] for c in columns]
    # Fixture keys are known; no-key rows are ordered by all source values.
    keys = ["order_id", "line_no"] if table == "order_lines" else (
        ["id", "month_no"] if table.startswith("partition_events") else
        ["id"] if "id" in names else ["code"] if "code" in names else
        ["c000"] if "c000" in names else names)
    order = ",".join('"' + n + '"' + (' COLLATE fixture_ci' if table == "string_keys" and n == "id" else "") for n in keys)
    # Arrays and JSON retain their semantic type across canonical Arrow text.
    projection = ",".join(
        f'"{name}"::json AS "{name}"' if kind in ("ARRAY", "json", "jsonb") else f'"{name}"'
        for name, kind in columns)
    _, left = run.sql(f'SET TIME ZONE \'UTC\'; SELECT row_to_json(t) FROM (SELECT * FROM public."{table}" ORDER BY {order}) t;\n')
    right = target.sql(f'SET TIME ZONE \'UTC\'; SELECT row_to_json(t) FROM (SELECT {projection} FROM {receipt["target"]} ORDER BY {order}) t;\n')
    def rows(text):
        for line in text.splitlines():
            if line.startswith("{"):
                row = json.loads(line, parse_float=Decimal, parse_int=Decimal)
                yield [pg_cell(row[name], kind) for name, kind in columns]
    return compare(rows(left), rows(right), key_columns=[names.index(k) for k in keys],
                   chunk_size=10_000, consistency="B")


def pg_cell(value, kind):
    if value is None:
        return None
    if kind == "date":
        return dt.date.fromisoformat(value)
    if kind.startswith("timestamp"):
        return dt.datetime.fromisoformat(value)
    if kind == "bytea":
        if not value.startswith("\\x"):
            raise RuntimeError("bytea hex representation required")
        return bytes.fromhex(value[2:])
    if kind in ("real", "double precision"):
        if isinstance(value, str):
            raise RuntimeError("nonfinite float requires a wire-bit comparison")
        return float(value)
    return value


def compare_oracle(run, target, receipt, table):
    _, raw = run.sql("SELECT column_name||'|'||data_type FROM all_tab_columns "
                     f"WHERE owner='FIXTURE' AND table_name='{table}' ORDER BY column_id;")
    columns = [line.strip().split("|") for line in raw.splitlines() if "|" in line]
    if not columns:
        raise RuntimeError("Oracle comparison metadata missing")
    names = [c[0] for c in columns]
    keys = ["ORDER_ID", "LINE_NO"] if table == "ORDER_LINES" else (
        ["ID", "MONTH_NO"] if table == "PARTITION_EVENTS" else
        ["ID"] if "ID" in names else ["CODE"] if "CODE" in names else
        ["C000"] if "C000" in names else names)
    expressions, target_order = [], []
    for name, kind in columns:
        col = '"' + name + '"'
        if kind in ("NUMBER", "FLOAT", "INTEGER"):
            expr = f"TO_CHAR({col},'TM9','NLS_NUMERIC_CHARACTERS=''.,''')"
            order = col + "::numeric"
        elif kind in ("VARCHAR2", "CHAR", "NVARCHAR2", "NCHAR"):
            expr, order = col, col + ' COLLATE "C"'
        elif kind == "RAW":
            expr, order = f"LOWER(RAWTOHEX({col}))", col
        elif kind == "DATE":
            expr, order = f"""TO_CHAR({col},'SYYYY-MM-DD"T"HH24:MI:SS')""", col
        elif kind.startswith("TIMESTAMP"):
            fmt = 'SYYYY-MM-DD"T"HH24:MI:SS.FF9' + (" TZH:TZM" if "TIME ZONE" in kind else "")
            expr, order = f"TO_CHAR({col},'{fmt}')", col
        else:
            raise RuntimeError("Oracle comparison unsupported source type: " + kind)
        expressions.append(f"NVL(RAWTOHEX(UTL_I18N.STRING_TO_RAW({expr},'AL32UTF8')),'-')")
        if name in keys:
            target_order.append(order)
    order = ",".join('"' + n + '"' for n in keys)
    _, left = run.sql(f'SELECT ' + "||'|'||".join(expressions) + f' FROM FIXTURE."{table}" ORDER BY {order};')
    right = target.sql(f'SELECT row_to_json(t) FROM (SELECT * FROM {receipt["target"]} ORDER BY {",".join(target_order)}) t;')
    def normalize(values):
        out = []
        for value, (_, kind) in zip(values, columns):
            if value is None:
                out.append(None)
            elif kind in ("NUMBER", "FLOAT", "INTEGER"):
                out.append(Decimal(value))
            elif kind == "RAW":
                out.append(bytes.fromhex(value))
            elif kind == "DATE" or kind.startswith("TIMESTAMP"):
                fraction = re.search(r"\.(\d+)", value)
                if fraction and any(c != "0" for c in fraction[1][6:]):
                    raise RuntimeError("submicrosecond Oracle comparison unavailable")
                out.append(dt.datetime.fromisoformat(value.lstrip(" +").replace(" ", "")))
            else:
                out.append(value)
        return out
    def source_rows():
        for line in left.splitlines():
            if not line.strip():
                continue
            cells = line.strip().split("|")
            if len(cells) != len(columns):
                raise RuntimeError("Oracle comparison row framing")
            yield normalize([None if c == "-" else bytes.fromhex(c).decode("utf-8") for c in cells])
    def target_rows():
        for line in right.splitlines():
            if line.startswith("{"):
                row = json.loads(line)
                yield normalize([row[n] for n in names])
    return compare(source_rows(), target_rows(), key_columns=[names.index(k) for k in keys],
                   consistency="B", oracle_empty=True)


def run_t1(run, binary, worker_image=None):
    target = AgentDB(run)
    store = run.path / ".load-private"
    store.mkdir(mode=0o700)
    stop = threading.Event()
    thread = None
    worker = None
    relay = None
    report = {"T1": "failed", "T8": "not_measured", "tables": [], "writer": {},
              "correction": "quiesce_writer_then_reread_all_tables", "consistency": "B"}
    try:
        report["binary_sha256"] = hashlib.sha256(binary.read_bytes()).hexdigest()
        private(store / "source-password", run.reader_secret)
        private(store / "target-password", target.password)
        if run.engine == "postgres":
            configure_pg(run, store)
            schema, service, source_port = "public", "fixture", 5432
            _, table_text = run.sql("SELECT tablename FROM pg_tables WHERE schemaname='public' ORDER BY tablename;")
            tables = table_text.split()
        else:
            schema = "FIXTURE"
            service = "XE" if run.version.startswith("11") else ("XEPDB1" if run.version.startswith("21") else "FREEPDB1")
            source_port = 1521
            run.sql("GRANT SELECT ON SYS.V_$DATABASE TO fixture_reader;\n"
                    "GRANT SELECT ON SYS.V_$SESSION TO fixture_reader;\n"
                    "GRANT SELECT ON SYS.V_$SQL_PLAN TO fixture_reader;\n"
                    "GRANT SELECT ON SYS.DBA_EXTENTS TO fixture_reader;\n"
                    "GRANT EXECUTE ON SYS.DBMS_ROWID TO fixture_reader;\n"
                    "BEGIN FOR t IN (SELECT table_name FROM all_tables WHERE owner='FIXTURE') LOOP "
                    "EXECUTE IMMEDIATE 'GRANT FLASHBACK ON fixture.'||t.table_name||' TO fixture_reader'; END LOOP; END;\n/\n", admin=True)
            _, table_text = run.sql("SELECT table_name FROM all_tables WHERE owner='FIXTURE' ORDER BY table_name;")
            tables = [s for s in table_text.split() if s.isidentifier()]
            # Fail early on the required type-edge fixture rather than copying
            # millions of rows before discovering an unsupported source type.
            tables.sort(key=lambda t: (t not in ("AREAS", "TYPE_EDGES"), t))
        if not tables:
            raise RuntimeError("fixture tables missing")
        code, port_text = run.command(["docker", "port", run.name, f"{source_port}/tcp"], required=False)
        if not code and port_text.strip():
            port = int(port_text.strip().rsplit(":", 1)[1])
        else:
            relay = Relay(run.name, source_port)
            port = relay.port
        worker_args = []
        if run.engine == "oracle" and run.version.startswith("11"):
            if not worker_image:
                raise RuntimeError("11g thick worker image required")
            worker = "oo-m-worker-" + uuid.uuid4().hex[:12]
            run.command(["docker", "create", "--name", worker, "--platform", "linux/amd64",
                         "--label", "oo.lane=m", "--label", "oo.run_id=" + run.id,
                         "--network", "container:" + run.name, "--entrypoint", "sleep", worker_image, "infinity"])
            run.command(["docker", "start", worker])
            executable = store / "worker"
            private(executable, f'#!/bin/sh\nexec docker exec -i {worker} python /worker.py\n')
            executable.chmod(0o700)
            worker_args = ["--oracle-worker", str(executable)]
            port = 1521
        source = {"id": "fixture", "engine": run.engine, "host": "127.0.0.1", "port": port,
                  "database": service, "user": "fixture_reader", "password": {"secret_ref": "source-password"},
                  "scope": {"include": [f"{schema}.{t}" for t in tables], "exclude": []}}
        if run.engine == "postgres":
            source["tls_ca"] = {"secret_ref": "source-ca"}
        stack = {"version": "oo.stack/v1", "sources": [source],
                 "model": {"provider": "openai-compatible", "base_url": "https://example.invalid/v1",
                           "model": "unused", "key": {"secret_ref": "unused"}, "context_window": "auto"}}
        runner.write_json(store / "stack.json", stack)
        target.start()
        target_config = {"host": "127.0.0.1", "port": target.port, "database": "postgres",
                         "user": "postgres", "password": {"secret_ref": "target-password"}, "local_fixture": True}
        thread = threading.Thread(target=writer, args=(run, stop, report["writer"]))
        thread.start()
        def load(run_id):
            runner.write_json(store / "target.json", dict(target_config, run_id=run_id))
            code, out = run.command([str(binary), "--stack", str(store / "stack.json"),
                                    "--source", "fixture", "--target", str(store / "target.json"),
                                    "--secrets-dir", str(store), "--chunk-rows", "10000", *worker_args],
                                   timeout=7200, required=False)
            receipts = [json.loads(line) for line in out.splitlines() if line.startswith("{")]
            runner.write_json(run.path / f"{run_id}-receipts.json", receipts)
            if code:
                raise RuntimeError(f"{run_id} product load failed")
            if len(receipts) != len(tables):
                raise RuntimeError("incomplete table receipts")
            return receipts
        first = load("concurrent")
        stop.set()
        thread.join(timeout=1800)
        if thread.is_alive() or report["writer"].get("error") or report["writer"].get("cycles", 0) < 1:
            raise RuntimeError("concurrent writer failed")
        corrected = load("corrected")
        replay = load("corrected")
        if [r["target_write_bytes"] for r in replay] != [r["target_write_bytes"] for r in corrected] or not all(r["replay"] for r in replay):
            raise RuntimeError("resume rewrote target")
        report["target_write_bytes"] = sum(r["target_write_bytes"] for r in first + corrected)
        for receipt in corrected:
            table = receipt["table"]["name"]
            comparison = (compare_pg if run.engine == "postgres" else compare_oracle)(run, target, receipt, table)
            runner.write_json(run.path / f"compare-{table}.json", comparison)
            report["tables"].append({"table": table, "equal": comparison["equal"]})
            if not comparison["equal"]:
                raise RuntimeError("source target difference")
        report["T1"] = "passed"
        # SourceEngine does not expose a neutral resource meter. Do not turn
        # an observed target byte count into full resource-budget acceptance.
        report["T8"] = "partial"
        report["connections_peak"] = None
        report["source_duty_cycle"] = None
        report["model_tokens"] = 0
    except Exception as error:
        report["failure"] = str(error) if isinstance(error, RuntimeError) else type(error).__name__
    finally:
        stop.set()
        if thread:
            thread.join(timeout=1800)
        if target.created and hasattr(target, "port"):
            try:
                total = target.sql("SELECT COALESCE(sum(write_bytes),0)::bigint FROM _oo_load.chunks;")
                report["target_write_bytes"] = int(total.strip())
                report["T8"] = "partial"
            except (RuntimeError, ValueError):
                report["target_write_bytes"] = None
        if worker:
            _, raw = run.command(["docker", "inspect", "--format", "{{json .Config.Labels}}", worker])
            labels = json.loads(raw)
            if labels.get("oo.lane") != "m" or labels.get("oo.run_id") != run.id:
                raise RuntimeError("worker cleanup ownership")
            run.command(["docker", "rm", "-f", worker])
        target.close()
        if relay:
            relay.close()
        shutil.rmtree(store)
        runner.write_json(run.path / "t1-t8.json", report)
    return report


def main():
    def interrupted(signum, frame):
        raise KeyboardInterrupt(f"signal {signum}")
    signal.signal(signal.SIGINT, interrupted)
    signal.signal(signal.SIGTERM, interrupted)
    if os.environ.get("OO_TEST_CONTEXT"):
        context = json.loads(Path(os.environ["OO_TEST_CONTEXT"]).read_text())
        labels = json.loads(subprocess.check_output(
            ["docker", "inspect", "--format", "{{json .Config.Labels}}", context["container"]], text=True))
        if labels.get("oo.lane") not in ("h", "m") or labels.get("oo.run_id") != context["run_id"]:
            raise RuntimeError("hook fixture ownership")
        if context["engine"] not in ("postgres", "oracle"):
            raise RuntimeError("T1 hook engine unavailable")
        run = LaneRun.__new__(LaneRun)
        run.name, run.id = context["container"], context["run_id"]
        run.engine, run.version = context["engine"], context["version"]
        run.path = Path(context["evidence"]) / "m-t1"
        run.path.mkdir(mode=0o700)
        run.seq, run.secret = 0, secrets.token_hex(24)
        run.reader_secret = Path(context["reader_password_file"]).read_text().strip()
        result = run_t1(run, ROOT / "target/debug/oo-load", os.environ.get("OO_TEST_ORACLE_WORKER_IMAGE"))
        runner.checksums(run.path)
        raise SystemExit(0 if result["T1"] == "passed" else 1)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--engine", choices=["postgres", "oracle"], required=True)
    parser.add_argument("--versions", required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/debug/oo-load")
    parser.add_argument("--worker-image")
    parser.add_argument("--scale", type=int, default=2_000_000)
    parser.add_argument("--tables", type=int, default=200)
    args = parser.parse_args()
    if not 1 <= args.scale <= 10_000_000 or not 0 <= args.tables <= 5000:
        parser.error("scale must be 1..10000000; tables must be 0..5000")
    if not args.run_id or any(c not in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_" for c in args.run_id):
        parser.error("invalid run ID")
    base = ROOT / "evidence" / args.run_id
    base.mkdir(mode=0o700, exist_ok=False)
    results = []
    for version in args.versions.split(","):
        path = base / (args.engine + "-" + version)
        path.mkdir(mode=0o700)
        options = argparse.Namespace(scale=args.scale, tables=args.tables, platform=None, health_timeout=None)
        run = LaneRun(path, args.engine, version, options)
        try:
            run.setup()
            run.seed()
            result = run_t1(run, args.binary.resolve(), args.worker_image)
        except Exception as error:
            result = {"T1": "failed", "T8": "not_run", "failure": type(error).__name__}
            runner.write_json(path / "t1-t8.json", result)
        finally:
            failures = run.cleanup()
            runner.checksums(path)
        results.append(dict(result, version=version, cleanup_failed=failures))
        runner.write_json(base / "results.json", results)
    print(base)
    raise SystemExit(0 if all(r["T1"] == "passed" and not r["cleanup_failed"] for r in results) else 1)


if __name__ == "__main__":
    main()
