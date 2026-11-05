#!/usr/bin/env python3
"""T1 hook and lane-M matrix driver. Does not change lane H's runner API."""
import argparse
import copy
import datetime as dt
import hashlib
from decimal import Decimal
from functools import partial
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
from oracle_t1 import compare_oracle as compare_oracle_lobs
import runner
from verdicts import status, write_rows

ROOT = runner.ROOT
LANE = os.environ.get("OO_TEST_LANE", "m")
if LANE not in ("h", "m", "e", "p", "g", "c"):
    raise ValueError("unsupported T1 lane")
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
    """Reuse H's generator/seed SQL, but own only explicitly labelled lane resources."""
    def command(self, command, **kwargs):
        command = ["oo.lane=" + LANE if x == "oo.lane=h" else x for x in command]
        if command[:2] == ["docker", "create"] and self.name in command:
            port = {"postgres": 5432, "mysql": 3306, "oracle": 1521}[self.engine]
            command[2:2] = ["-p", f"127.0.0.1::{port}"]
        # A local Docker connection refusal happens before exec is dispatched.
        # Retry read-only queries and cleanup ownership inspections, retaining every log.
        data = (kwargs.get("data") or "").lstrip()
        readonly = data.startswith(("SELECT ", "SET TIME ZONE 'UTC'; SELECT ",
                                    "SET time_zone='+00:00'; SELECT "))
        if "sqlplus" in command and data.startswith("whenever sqlerror exit failure rollback\n"):
            # Run.sql prepends login/session settings before Oracle SELECTs.
            body = data.partition("\nconnect ")[2].partition("\n")[2]
            body = re.sub(
                r"^(?:(?:ALTER SESSION SET (?:CONTAINER=(?:FREEPDB1|XEPDB1)|"
                r"CURRENT_SCHEMA=fixture);|SET ARRAYSIZE 1000|SET LINESIZE 1024)\n)*",
                "", body)
            readonly = body.startswith("SELECT ")
        inspection = (command[:2] == ["docker", "inspect"] or
                      command[:3] in (["docker", "container", "inspect"],
                                      ["docker", "volume", "inspect"]))
        if inspection or command[:2] == ["docker", "exec"] and readonly:
            for attempt in range(3):
                try:
                    return super().command(command, **kwargs)
                except RuntimeError:
                    prefix = self.path / f"{self.seq:04d}"
                    err = prefix.with_suffix(".stderr").read_text()
                    out = prefix.with_suffix(".stdout").read_text()
                    refused = (
                        err.startswith("Cannot connect to the Docker daemon at ")
                        and "Is the docker daemon running?" in err
                    ) or (
                        err.startswith("cannot connect to the Docker daemon. ")
                        and "dial unix " in err and err.rstrip().endswith("connect: connection refused")
                    )
                    if attempt == 2 or out or not refused:
                        raise
                    time.sleep(0.2)
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
                if labels.get("oo.lane") != LANE or labels.get("oo.run_id") != self.id:
                    raise RuntimeError("cleanup ownership")
                if kind == "container":
                    self.command(["docker", "logs", name], required=False)
                    if self.engine == "postgres":
                        self.command(["docker", "exec", name, "sh", "-c",
                                      'cat "$PGDATA"/log/oo-audit.csv*'], required=False)
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
        self.run.command(["docker", "create", "--name", self.name, "--label", "oo.lane=" + LANE,
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
            if labels.get("oo.lane") != LANE or labels.get("oo.run_id") != self.run.id:
                raise RuntimeError("target cleanup ownership")
            self.run.command(["docker", "rm", "-f", "-v", self.name])
        shutil.rmtree(self.directory)


def configure_tls(run, store):
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
    for file in ("source-ca", "server-cert", "server-key"):
        (store / file).chmod(0o600)

def configure_pg(run, store):
    configure_tls(run, store)
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


def configure_mysql(run, store):
    configure_tls(run, store)
    for source, dest in (("source-ca", "ca.pem"), ("server-cert", "server-cert.pem"),
                         ("server-key", "server-key.pem")):
        run.command(["docker", "cp", str(store / source), run.name + ":/var/lib/mysql/" + dest])
    run.command(["docker", "exec", "-u", "root", run.name, "bash", "-c",
                 "chown mysql:mysql /var/lib/mysql/*.pem; chmod 600 /var/lib/mysql/*key.pem"])
    # T0 and T1 may configure the same owned fixture in one matrix invocation.
    run.sql(f"CREATE USER IF NOT EXISTS 'fixture_reader'@'%' IDENTIFIED BY '{run.reader_secret}' REQUIRE SSL;\n"
            f"ALTER USER 'fixture_reader'@'%' IDENTIFIED BY '{run.reader_secret}' REQUIRE SSL;\n"
            "GRANT SELECT,SHOW VIEW ON fixture.* TO 'fixture_reader'@'%';\n", admin=True)
    run.command(["docker", "restart", run.name])
    for _ in range(90):
        if run.sql("SELECT 1;", admin=True, required=False)[0] == 0:
            return
        time.sleep(1)
    raise RuntimeError("mysql TLS restart timeout")


class SourceMeter:
    """Admin-only source samples; aggregate active session seconds / load seconds."""
    def __init__(self, run, label, connections, duty=25, include_logminer=False):
        self.run = copy.copy(run)
        self.run.seq = 2_000_000 + label * 100_000
        self.stop = threading.Event()
        self.samples = []
        self.error = None
        self.child = None
        self.label = label
        self.connections, self.duty = connections, duty
        self.include_logminer = include_logminer
        self.thread = threading.Thread(target=self.sample, daemon=True)

    def sample(self):
        queries = {
            "postgres": "SELECT count(*),count(*) FILTER (WHERE state='active') FROM pg_stat_activity WHERE usename='fixture_reader';",
            "mysql": "SELECT count(*),COALESCE(sum(COMMAND<>'Sleep'),0) FROM information_schema.PROCESSLIST WHERE USER='fixture_reader';",
            "oracle": "SELECT count(*),count(CASE WHEN status='ACTIVE' THEN 1 END) FROM v$session WHERE username='FIXTURE_READER';",
        }
        if self.include_logminer:
            # PDB-local V$SESSION hides the common user's root mining session.
            queries["oracle"] = "ALTER SESSION SET CONTAINER=CDB$ROOT;\n" + queries["oracle"].replace(
                "username='FIXTURE_READER'", "username IN ('FIXTURE_READER','C##OO_MINER')")
        stderr = None
        try:
            if self.run.engine in ("postgres", "mysql"):
                # Keep observation off the Docker exec hot path, like the writer:
                # one exec per sample (5/s) exhausts the Docker socket on long loads.
                if self.run.engine == "postgres":
                    command = ["docker", "exec", "-i", "-u", "postgres", self.run.name,
                               "psql", "-X", "-q", "-v", "ON_ERROR_STOP=1", "-A", "-t", "-F", "|",
                               "-U", "postgres", "-d", "fixture"]
                else:
                    command = ["docker", "exec", "-i", self.run.name, "mysql",
                               "--defaults-extra-file=/run/oo/admin.cnf", "--protocol=tcp",
                               "--host=127.0.0.1", "--connect-timeout=5", "--batch", "--raw",
                               "--skip-column-names", "--unbuffered", "fixture"]
                runner.write_json(self.run.path / f"source-meter-{self.label}-command.json",
                                  {"argv": command, "query": queries[self.run.engine]})
                stderr = (self.run.path / f"source-meter-{self.label}.stderr").open("w")
                self.child = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                              stderr=stderr, text=True, bufsize=1)
            while not self.stop.is_set():
                if self.child:
                    self.child.stdin.write(queries[self.run.engine] + "\n")
                    self.child.stdin.flush()
                    text = self.child.stdout.readline()
                    if not text:
                        raise RuntimeError("sampler connection closed")
                else:
                    _, text = self.run.sql(queries[self.run.engine], admin=True)
                values = re.split(r"[|\s]+", text.strip())
                connections, active = map(int, values)
                self.samples.append((time.monotonic(), connections, active))
                self.stop.wait(0.2)
        except Exception as error:
            self.error = type(error).__name__
        finally:
            if self.child:
                self.child.stdin.close()
                try:
                    self.child.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    self.child.kill()
                    self.child.wait()
                self.child.stdout.close()
            if stderr:
                stderr.close()

    def __enter__(self):
        self.started = time.monotonic()
        self.thread.start()
        return self

    def __exit__(self, *args):
        self.ended = time.monotonic()
        self.stop.set()
        self.thread.join(timeout=60)
        if self.thread.is_alive():
            self.error = "sampler_timeout"
            if self.child:
                self.child.kill()
                self.thread.join(timeout=5)

    def result(self):
        elapsed = self.ended - self.started
        # A query already in flight can finish after the loader exits.
        samples = [(t, c, a) for t, c, a in self.samples if self.started <= t <= self.ended]
        busy = sum(active * ((samples[i+1][0] if i+1 < len(samples) else self.ended) - stamp)
                   for i, (stamp, _, active) in enumerate(samples))
        peak = max((c for _, c, _ in samples), default=0)
        duty = 100 * busy / elapsed if elapsed else None
        passed = (not self.error and len(samples) >= 2 and peak > 0 and
                  peak <= self.connections and duty is not None and duty <= self.duty)
        return {"status": "passed" if passed else "failed", "error": self.error,
                "session_scope": "root_reader_and_miner" if self.include_logminer else "fixture_reader",
                "connections_peak": peak, "aggregate_duty_percent": duty,
                "elapsed_seconds": elapsed, "sample_count": len(samples),
                "budget": {"reader_connections": self.connections, "duty_cycle_percent": self.duty},
                "basis": "sampled_active_reader_sessions_time_weighted_over_load_wall_time",
                "samples": [[t-self.started, c, a] for t,c,a in self.samples]}


def keyless_writes(cycle):
    key = 30_000_000 + cycle
    return (f"INSERT INTO no_key VALUES ({key},'before');\n"
            f"UPDATE no_key SET note='after' WHERE value_no={key};\n"
            f"DELETE FROM no_key WHERE value_no={key};\n"
            f"INSERT INTO no_key VALUES ({key},'duplicate');\n"
            f"INSERT INTO no_key VALUES ({key},'duplicate');\n")


def writer(run, stop, result):
    """Every committed cycle includes insert/update/delete/primary-key update."""
    if run.engine in ("postgres", "mysql"):
        return persistent_writer(run, stop, result)
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
                   f"INSERT INTO growth_events VALUES({20_000_000+cycles},3);\n"
                   + keyless_writes(cycles) + "COMMIT;\n")
            if run.engine == "oracle":
                sql = sql.replace(";\n", ";\nCOMMIT;\n")
            run.sql(sql, admin=True)
            cycles += 1
            stop.wait(0.15)
        result["cycles"] = cycles
    except Exception as error:
        result.update(cycles=cycles, error=type(error).__name__)


def persistent_writer(run, stop, result):
    """One admin session, rather than thousands of Docker API exec requests."""
    if run.engine == "mysql":
        command = ["docker", "exec", "-i", run.name, "mysql",
                   "--defaults-extra-file=/run/oo/admin.cnf",
                   "--protocol=tcp", "--host=127.0.0.1", "--connect-timeout=5",
                   "--batch", "--raw", "--skip-column-names", "--unbuffered",
                   "--skip-reconnect", "--default-character-set=utf8mb4", "fixture"]
    else:
        command = ["docker", "exec", "-i", "-u", "postgres", "-e",
                   "PGOPTIONS=-c statement_timeout=30000", run.name,
                   "psql", "-X", "-v", "ON_ERROR_STOP=1", "-A", "-t",
                   "-U", "postgres", "-d", "fixture"]
    cycles = 0
    child = None
    try:
        with (run.path / "writer.stdout").open("w") as output, (run.path / "writer.stderr").open("w") as errors:
            child = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                     stderr=errors, text=True, bufsize=1)
            while not stop.is_set():
                key = 10_000_000 + cycles * 2
                marker = f"OO_CYCLE_{cycles}"
                child.stdin.write(
                    f"INSERT INTO growth_events VALUES({key},1);\n"
                    f"UPDATE growth_events SET order_id=2 WHERE id={key};\n"
                    f"UPDATE growth_events SET id={key+1} WHERE id={key};\n"
                    f"DELETE FROM growth_events WHERE id={key+1};\n"
                    f"INSERT INTO growth_events VALUES({20_000_000+cycles},3);\n"
                    + keyless_writes(cycles) + "COMMIT;\n"
                    + f"SELECT '{marker}';\n")
                child.stdin.flush()
                while True:
                    line = child.stdout.readline()
                    if not line:
                        raise RuntimeError("writer ended before commit acknowledgement")
                    output.write(line)
                    if line.strip() == marker:
                        break
                output.flush()
                cycles += 1
                stop.wait(0.15)
            child.stdin.close()
            if child.wait(timeout=60):
                raise RuntimeError("writer process failed")
            result["cycles"] = cycles
    except Exception as error:
        result.update(cycles=cycles, error=type(error).__name__)
    finally:
        if child and child.poll() is None:
            child.kill()
            child.wait()
        runner.write_json(run.path / "writer-process.json",
                          {"argv": command, "exit": child.returncode if child else None, **result})


def mysql_cell(value, kind):
    if value is None:
        return None
    if kind in ("tinyint", "smallint", "mediumint", "int", "bigint", "decimal"):
        return Decimal(value)
    if kind in ("float", "double"):
        return float(value)
    if kind in ("blob", "longblob", "mediumblob", "tinyblob", "binary", "varbinary"):
        return bytes.fromhex(value.removeprefix("\\x"))
    if kind == "json":
        return json.loads(value, parse_float=Decimal, parse_int=Decimal) if isinstance(value, str) else value
    if kind == "date":
        return dt.date.fromisoformat(value)
    if kind in ("timestamp", "datetime"):
        stamp = dt.datetime.fromisoformat(value)
        return stamp.replace(tzinfo=dt.timezone.utc) if kind == "timestamp" and stamp.tzinfo is None else stamp
    return value


def compare_mysql(run, target, receipt, table):
    _, metadata = run.sql("SELECT column_name,data_type FROM information_schema.columns "
                          f"WHERE table_schema='fixture' AND table_name='{table}' ORDER BY ordinal_position;")
    columns = [line.split("\t") for line in metadata.splitlines() if line]
    if not columns:
        raise RuntimeError("comparison metadata missing")
    names = [n for n, _ in columns]
    keys = ["order_id", "line_no"] if table == "order_lines" else (
        ["id", "month_no"] if table == "partition_events" else
        ["id"] if "id" in names else ["code"] if "code" in names else
        ["c000"] if "c000" in names else names)
    binary = ("blob", "longblob", "mediumblob", "tinyblob", "binary", "varbinary")
    projection = ",".join(f"IFNULL(HEX(`{n}`),'-')" if k in binary else
                          f"IFNULL(HEX(CAST(`{n}` AS CHAR CHARACTER SET utf8mb4)),'-')" for n,k in columns)
    source_order = ",".join(f"`{n}`" for n in keys)
    _, left = run.sql(f"SET time_zone='+00:00'; SELECT CONCAT_WS('|',{projection}) FROM `{table}` ORDER BY {source_order};")
    target_order = ",".join('"' + n + '"' + (' COLLATE fixture_ci' if table == "string_keys" and n == "id" else "") + " NULLS FIRST" for n in keys)
    right = target.sql(f"SET TIME ZONE 'UTC'; SELECT row_to_json(t) FROM (SELECT * FROM {receipt['target']} ORDER BY {target_order}) t;")
    def source_rows():
        for line in left.splitlines():
            cells = line.split("|")
            if len(cells) != len(columns):
                raise RuntimeError("mysql comparison framing")
            yield [None if cell == "-" else mysql_cell(cell if kind in binary else bytes.fromhex(cell).decode("utf-8"), kind)
                   for cell, (_, kind) in zip(cells, columns)]
    def target_rows():
        for line in right.splitlines():
            if line.startswith("{"):
                row = json.loads(line, parse_float=Decimal, parse_int=Decimal)
                yield [mysql_cell(row[n], kind) for n,kind in columns]
    return compare(source_rows(), target_rows(), key_columns=[names.index(k) for k in keys], consistency="B")


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
                out.append(bytes.fromhex(value.removeprefix("\\x")))
            elif kind == "DATE" or kind.startswith("TIMESTAMP"):
                fraction = re.search(r"\.(\d+)", value)
                if fraction and any(c != "0" for c in fraction[1][6:]):
                    raise RuntimeError("submicrosecond Oracle comparison unavailable")
                stamp = dt.datetime.fromisoformat(value.lstrip(" +").replace(" ", ""))
                # D17: the Agent DB keeps the instant of a zoned timestamp, not its offset.
                out.append(stamp.astimezone(dt.timezone.utc) if "TIME ZONE" in kind else stamp)
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
                row = json.loads(line, parse_float=Decimal)
                yield normalize([row[n] for n in names])
    return compare(source_rows(), target_rows(), key_columns=[names.index(k) for k in keys],
                   consistency="B", oracle_empty=True)


compare_oracle = partial(compare_oracle_lobs, scalar_compare=compare_oracle)

def run_t1(run, binary, worker_image=None):
    target = AgentDB(run)
    store = run.path / ".load-private"
    store.mkdir(mode=0o700)
    stop = threading.Event()
    thread = None
    worker = None
    relay = None
    measurements = []
    report = {"T1": "failed", "T8": "not_measured", "tables": [], "writer": {},
              "correction": "quiesce_writer_then_one_product_c5_cycle", "consistency": "B"}
    try:
        report["binary_sha256"] = hashlib.sha256(binary.read_bytes()).hexdigest()
        private(store / "source-password", run.reader_secret)
        private(store / "target-password", target.password)
        if run.engine == "postgres":
            configure_pg(run, store)
            schema, service, source_port = "public", "fixture", 5432
            _, table_text = run.sql("SELECT tablename FROM pg_tables WHERE schemaname='public' ORDER BY tablename;")
            tables = table_text.split()
        elif run.engine == "mysql":
            configure_mysql(run, store)
            schema, service, source_port = "fixture", "fixture", 3306
            _, table_text = run.sql("SELECT table_name FROM information_schema.tables WHERE table_schema='fixture' AND table_type='BASE TABLE' ORDER BY table_name;")
            tables = table_text.split()
            # Reject unsupported required types before copying the scale table.
            tables.sort(key=lambda t: (t != "type_edges", t))
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
                         "--label", "oo.lane=" + LANE, "--label", "oo.run_id=" + run.id,
                         "--network", "container:" + run.name,
                         "--mount", f"type=bind,src={ROOT / 'testing/engines/oracle/worker/worker.py'},dst=/worker.py,readonly",
                         "--entrypoint", "sleep", worker_image, "infinity"])
            run.command(["docker", "start", worker])
            executable = store / "worker"
            private(executable, f'#!/bin/sh\nexec docker exec -i {worker} python /worker.py\n')
            executable.chmod(0o700)
            worker_args = ["--oracle-worker", str(executable)]
            port = 1521
        source = {"id": "fixture", "engine": run.engine, "host": "127.0.0.1", "port": port,
                  "database": service, "user": "fixture_reader", "password": {"secret_ref": "source-password"},
                  "scope": {"include": [f"{schema}.{t}" for t in tables], "exclude": []}}
        if run.engine in ("postgres", "mysql"):
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
        def load(run_id, *, correction=False, only=None, cycle=1):
            runner.write_json(store / "target.json", dict(target_config, run_id=run_id))
            mode = []
            if correction:
                runner.write_json(store / "c5.json", {"cycle": cycle, "full_read_every": 2})
                mode = ["sync", "--run-id", run_id, "--c5", str(store / "c5.json")]
            if only:
                mode += ["--tables", f"{schema}.{only}"]
            with SourceMeter(run, len(measurements), 2 if run.engine == "postgres" else 1) as meter:
                code, out = run.command([str(binary), *mode, "--stack", str(store / "stack.json"),
                                        "--source", "fixture", "--target", str(store / "target.json"),
                                        "--secrets-dir", str(store), "--chunk-rows", "10000", *worker_args],
                                       timeout=7200, required=False)
            measurement = meter.result()
            measurements.append(measurement)
            runner.write_json(run.path / f"source-meter-{len(measurements)}.json", measurement)
            receipts = [json.loads(line) for line in out.splitlines() if line.startswith("{")]
            runner.write_json(run.path / f"{run_id}-receipts.json", receipts)
            if code:
                raise RuntimeError(f"{run_id} product load failed")
            if len(receipts) != (1 if only else len(tables)):
                raise RuntimeError("incomplete table receipts")
            if correction and any(r.get("protocol") != "oo.sync/v1" or r.get("mode") != "c5"
                                  for r in receipts):
                raise RuntimeError("correction did not use product C5")
            if not correction and any(r["read_budget"]["duty_cycle_percent"] != meter.duty for r in receipts):
                raise RuntimeError("sampled budget does not match product ReadBudget")
            if not correction and run.engine == "mysql" and any(
                    ".".join(str(r["version"][key]) for key in ("major", "minor", "patch")) !=
                    re.search(r"\d+(?:\.\d+)*", runner.resolve("mysql", run.version)["id"]).group()
                    for r in receipts):
                raise RuntimeError("MySQL version does not match exact card version")
            return receipts
        first = load("concurrent")
        stop.set()
        thread.join(timeout=1800)
        if thread.is_alive() or report["writer"].get("error") or report["writer"].get("cycles", 0) < 1:
            raise RuntimeError("concurrent writer failed")
        replay = load("concurrent")
        if [r["target_write_bytes"] for r in replay] != [r["target_write_bytes"] for r in first] or not all(r["replay"] for r in replay):
            raise RuntimeError("resume rewrote target")
        corrected = load("corrected", correction=True)
        if {(r["table"]["name"], r["target"]) for r in corrected} != {
                (r["table"]["name"], r["target"]) for r in first}:
            raise RuntimeError("correction changed loaded table scope or target")
        report["target_write_bytes"] = sum(r["target_write_bytes"] for r in first + corrected)
        for receipt in corrected:
            table = receipt["table"]["name"]
            comparison = ({"postgres": compare_pg, "mysql": compare_mysql, "oracle": compare_oracle}[run.engine])(run, target, receipt, table)
            runner.write_json(run.path / f"compare-{table}.json", comparison)
            report["tables"].append({"table": table, "equal": comparison["equal"]})
            if not comparison["equal"]:
                raise RuntimeError("source target difference")
        keyless = next((t for t in tables if t.lower() == "no_key"), None)
        if keyless:
            for cycle in (3, 4):
                idle = load(f"keyless-idle-{cycle}", correction=True, only=keyless, cycle=cycle)
                if any(r.get(field) != 0 for r in idle
                       for field in ("target_write_bytes", "payload_bytes", "new_generations",
                                     "ranges_replaced")):
                    raise RuntimeError("idle keyless C5 rewrote target")
                if cycle == 3 and idle[0].get("source_read_bytes") != 0:
                    raise RuntimeError("idle keyless C5 read payload without a full-read cycle")
            report["keyless_idle"] = "digest-only and periodic full-read: zero writes/generations"
        report["T1"] = "passed"
        # Enforce every invocation, including the no-rewrite resume.
        report["T8"] = "passed" if all(m["status"] == "passed" for m in measurements) else "failed"
        report["model_tokens"] = 0
    except Exception as error:
        report["failure"] = str(error) if isinstance(error, RuntimeError) else type(error).__name__
    finally:
        report["source_metrics"] = [{k:v for k,v in m.items() if k != "samples"} for m in measurements]
        stop.set()
        if thread:
            thread.join(timeout=1800)
        if target.created and hasattr(target, "port"):
            try:
                total = target.sql("SELECT COALESCE(sum(write_bytes),0)::bigint FROM _oo_load.chunks;")
                report["target_write_bytes"] = int(total.strip())
                if report["T8"] == "not_measured":
                    report["T8"] = "partial"
            except (RuntimeError, ValueError):
                report["target_write_bytes"] = None
        if worker:
            _, raw = run.command(["docker", "inspect", "--format", "{{json .Config.Labels}}", worker])
            labels = json.loads(raw)
            if labels.get("oo.lane") != LANE or labels.get("oo.run_id") != run.id:
                raise RuntimeError("worker cleanup ownership")
            run.command(["docker", "rm", "-f", worker])
        target.close()
        if relay:
            relay.close()
        shutil.rmtree(store)
        runner.write_json(run.path / "t1-t8.json", report)
        try:
            run.record_version()
        except Exception:
            report["version_error"] = "version_unmeasured"
            runner.write_json(run.path / "t1-t8.json", report)
        write_rows(run, run.path / "t1-t8.json",
                   [(suite, "snapshot", status(report[suite])) for suite in ("T1", "T8")])
    return report


def main():
    global LANE
    def interrupted(signum, frame):
        raise KeyboardInterrupt(f"signal {signum}")
    signal.signal(signal.SIGINT, interrupted)
    signal.signal(signal.SIGTERM, interrupted)
    if os.environ.get("OO_TEST_CONTEXT"):
        context = json.loads(Path(os.environ["OO_TEST_CONTEXT"]).read_text())
        labels = json.loads(subprocess.check_output(
            ["docker", "inspect", "--format", "{{json .Config.Labels}}", context["container"]], text=True))
        # runner.py labels Oracle 19 enterprise fixtures E19.
        if labels.get("oo.lane") not in ("h", "E19", LANE) or labels.get("oo.run_id") != context["run_id"]:
            raise RuntimeError("hook fixture ownership")
        LANE = labels["oo.lane"]
        if context["engine"] not in ("postgres", "mysql", "oracle"):
            raise RuntimeError("T1 hook engine unavailable")
        run = LaneRun.__new__(LaneRun)
        run.name, run.id = context["container"], context["run_id"]
        run.engine, run.version = context["engine"], context["version"]
        run.path = Path(context["evidence"]) / "m-t1"
        run.path.mkdir(mode=0o700)
        run.seq, run.secret = 0, secrets.token_hex(24)
        run.reader_secret = Path(context["reader_password_file"]).read_text().strip()
        receipt = Path(context["evidence"]) / "resource-receipt.json"
        run.receipt = json.loads(receipt.read_text())
        result = run_t1(run, ROOT / "target/debug/oo-load", os.environ.get("OO_TEST_ORACLE_WORKER_IMAGE"))
        runner.checksums(run.path)
        raise SystemExit(0 if result["T1"] == "passed" and result["T8"] == "passed" else 1)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--engine", choices=["postgres", "mysql", "oracle"], required=True)
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
        result = {"T1": "failed", "T8": "not_run"}
        try:
            run.setup()
            run.seed()
            result = run_t1(run, args.binary.resolve(), args.worker_image)
        except Exception as error:
            result = {"T1": "failed", "T8": "not_run", "failure": type(error).__name__}
            runner.write_json(path / "t1-t8.json", result)
        finally:
            failures = run.cleanup()
            result["cleanup_failed"] = failures
            runner.write_json(path / "t1-t8.json", result)
            write_rows(run, path / "t1-t8.json",
                       [(suite, "snapshot", status(result[suite])) for suite in ("T1", "T8")],
                       cleanup_failed=bool(failures))
            runner.checksums(path)
        results.append(dict(result, version=version, cleanup_failed=failures))
        runner.write_json(base / "results.json", results)
    print(base)
    raise SystemExit(0 if all(r["T1"] == "passed" and r["T8"] == "passed"
                             and not r.get("version_error") and not r["cleanup_failed"] for r in results) else 1)


if __name__ == "__main__":
    main()
