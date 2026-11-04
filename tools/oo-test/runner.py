"""Single-container, digest-only synthetic fixture runner."""
import argparse
from decimal import Decimal, InvalidOperation
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import secrets
import signal
import subprocess
import sys
import time
import uuid

# Sibling modules resolve even when this file is loaded by path from another tool.
sys.path.insert(0, str(Path(__file__).resolve().parent))
from audit import Audit
from datetime import datetime, timezone
from matrix import P0, downgrade_report, index_rows

ROOT = Path(__file__).resolve().parents[2]
ENGINES = ROOT / "testing/engines"
spec = importlib.util.spec_from_file_location("fixture_generate", ENGINES / "seed-common/generate.py")
generator = importlib.util.module_from_spec(spec)
spec.loader.exec_module(generator)


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, ensure_ascii=True) + "\n")


def redact(text, *passwords):
    for password in passwords:
        text = text.replace(password, "[REDACTED]")
    # Registry errors sometimes contain short-lived signed blob download URLs.
    return re.sub(r'(https?://[^\s?"<>]+)\?[^\s"<>]+', r'\1?[REDACTED_QUERY]', text)


def checksums(path):
    rows = []
    for file in sorted(path.rglob("*")):
        if file.is_file() and file != path / "SHA256SUMS" and ".private" not in file.parts:
            digest = hashlib.sha256()
            with file.open("rb") as stream:
                for block in iter(lambda: stream.read(1024 * 1024), b""):
                    digest.update(block)
            rows.append(f"{digest.hexdigest()}  {file.relative_to(path)}")
    (path / "SHA256SUMS").write_text("\n".join(rows) + "\n")


def resolve(engine, version):
    aliases = {"9.x": "9", "23ai": "23",
               "21c": "21", "11g": "11"}
    version = aliases.get(version, version)
    candidates = []
    for item in json.loads((ROOT / "testing/infra/images.lock.json").read_text())["images"]:
        if item.get("engine_matrix") != "v0.2":
            continue
        name = item["id"]
        prefix = "oracle-" if engine == "oracle" else engine + "-"
        if not name.startswith(prefix):
            continue
        number = re.search(r"\d+(?:\.\d+)*", name).group()
        if number == version or number.startswith(version + "."):
            candidates.append((tuple(map(int, number.split("."))), item))
    if not candidates:
        raise ValueError(f"no locked image for {engine} {version}")
    return max(candidates, key=lambda pair: pair[0])[1]


def suites(value):
    result = []
    for part in value.split(","):
        if re.fullmatch(r"T[0-8]-T[0-8]", part):
            lo, hi = int(part[1]), int(part[-1])
            if lo > hi:
                raise argparse.ArgumentTypeError("descending suite range")
            result.extend(f"T{i}" for i in range(lo, hi + 1))
        elif re.fullmatch(r"T[0-8]", part):
            result.append(part)
        else:
            raise argparse.ArgumentTypeError("expected T0,T1 or T0-T8")
    return list(dict.fromkeys(result))


class Run:
    def __init__(self, path, engine, version, args):
        self.path, self.engine, self.version, self.args = path, engine, version, args
        path.chmod(0o700)
        self.id = str(uuid.uuid4())
        self.name = "oo-h-" + self.id
        self.volume = self.name + "-data"
        self.seq = 0
        # Oracle XE 11g/21c passwords are limited to 30 bytes.
        self.secret = secrets.token_hex(12)
        self.reader_secret = secrets.token_hex(12)
        self.private = path / ".private"
        self.private.mkdir(mode=0o700)
        self.created_container = False
        self.created_volume = False
        self.started = time.monotonic()

    def command(self, command, *, data=None, stdin=None, timeout=300, required=True):
        self.seq += 1
        prefix = self.path / f"{self.seq:04d}"
        try:
            result = subprocess.run(command, input=data, stdin=stdin, capture_output=True,
                                    text=True, timeout=timeout, cwd=ROOT)
            code, out, err = result.returncode, result.stdout, result.stderr
        except subprocess.TimeoutExpired as error:
            code, out, err = 124, error.stdout or "", error.stderr or ""
            if isinstance(out, bytes):
                out = out.decode(errors="replace")
            if isinstance(err, bytes):
                err = err.decode(errors="replace")
            err += "\ncommand timeout"
        except OSError as error:
            code, out, err = 127, "", str(error)
        for suffix, text in [("stdout", out), ("stderr", err)]:
            prefix.with_suffix("." + suffix).write_text(redact(text, self.secret, self.reader_secret))
        sqlplus_error = "sqlplus" in command and bool(
            re.search(r"^(?:SP2-|ORA-|TNS-|ERROR at line)", out + "\n" + err, re.M))
        write_json(prefix.with_suffix(".json"), {
            "argv": [str(x).replace(str(ROOT), "<worktree>").replace(self.secret, "[REDACTED]") for x in command],
            "exit": code, "sqlplus_error": sqlplus_error})
        if required and (code or sqlplus_error):
            raise RuntimeError(f"command {self.seq:04d} exited {code}, sqlplus_error={sqlplus_error}")
        return code or int(sqlplus_error), out

    def sql(self, text=None, file=None, admin=False, required=True):
        if self.engine == "mysql":
            command = ["docker", "exec", "-i", self.name, "mysql",
                       "--defaults-extra-file=/run/oo/" + ("admin.cnf" if admin else "reader.cnf"),
                       "--protocol=tcp", "--host=127.0.0.1", "--connect-timeout=5", "--batch", "--raw",
                       "--skip-column-names", "--default-character-set=utf8mb4", "fixture"]
        elif self.engine == "postgres":
            command = ["docker", "exec", "-i", "-u", "postgres", self.name, "psql", "-X",
                       "-v", "ON_ERROR_STOP=1", "-A", "-t", "-F", "|", "-U", "postgres" if admin else "fixture_reader", "-d", "fixture"]
        else:
            command = ["docker", "exec", "-i", "-e", "NLS_LANG=.AL32UTF8", self.name,
                       "sqlplus", "-s", "/nolog"]
            header = ("whenever sqlerror exit failure rollback\nwhenever oserror exit failure rollback\n"
                      "set echo off verify off feedback off heading off pagesize 0 linesize 32767 trimout on tab off\n"
                      "set colsep '|'\n")
            if admin:
                header += "connect / as sysdba\n"
            else:
                header += f"connect fixture_reader/{self.reader_secret}\n"
            if not self.version.startswith("11"):
                if admin:
                    header += f"ALTER SESSION SET CONTAINER={'FREEPDB1' if self.version.startswith('23') else 'XEPDB1'};\n"
                else:
                    header = header.replace(f"connect fixture_reader/{self.reader_secret}",
                                            f"connect fixture_reader/{self.reader_secret}@localhost/{'FREEPDB1' if self.version.startswith('23') else 'XEPDB1'}")
            if admin:
                header += "ALTER SESSION SET CURRENT_SCHEMA=fixture;\n"
            if file:
                # SQL*Plus streams the generated file inside the owned container.
                text = header + "@/run/oo/seed.sql\nexit\n"
                file = None
            else:
                text = header + (text or "") + "\nexit\n"
        if file:
            with file.open() as stream:
                return self.command(command, stdin=stream, timeout=1800, required=required)
        return self.command(command, data=text, timeout=1800, required=required)

    def setup(self):
        item = resolve(self.engine, self.version)
        _, arch = self.command(["docker", "info", "--format", "{{.Architecture}}"])
        native = "linux/arm64" if arch.strip() in ("aarch64", "arm64") else "linux/amd64"
        platform = self.args.platform or (native if native in item["platforms"] else "linux/amd64")
        if platform not in item["platforms"]:
            raise ValueError("platform absent from image lock")
        digest = item["platforms"][platform]["digest"]
        image = item["ref"] + "@" + digest
        self.receipt = {"run_id": self.id, "image": image, "platform": platform,
                        "execution": "emulated" if platform != native else "native",
                        "engine": self.engine, "version_requested": self.version,
                        "scale": self.args.scale, "tables": self.args.tables,
                        "consistency": "B", "scope": "fixture T0, not product doctor or T1-T8"}
        self.receipt["source_sha256"] = {
            str(file.relative_to(ROOT)): hashlib.sha256(file.read_bytes()).hexdigest()
            for file in [Path(__file__), Path(__file__).with_name("audit.py"),
                         ENGINES / "seed-common/generate.py",
                         ENGINES / self.engine / "seed/preamble.sql",
                         ENGINES / "gold" / f"{self.engine}.sql"]}
        write_json(self.path / "resource-receipt.json", self.receipt)
        for attempt in range(3):
            code, _ = self.command(["docker", "pull", "--platform", platform, image],
                                   timeout=1200, required=False)
            if code == 0:
                break
            if attempt == 2:
                raise RuntimeError("pinned image pull failed after three attempts")
            time.sleep(2)
        labels = ["--label", "oo.lane=h", "--label", "oo.run_id=" + self.id]
        self.created_volume = True
        self.command(["docker", "volume", "create", *labels, self.volume])
        env = []
        if self.engine == "mysql":
            destination = "/var/lib/mysql"
            env = ["-e", "MYSQL_ROOT_PASSWORD_FILE=/run/oo/password", "-e", "MYSQL_DATABASE=fixture"]
            for name, user in [("admin.cnf", "root"), ("reader.cnf", "fixture_reader")]:
                password = self.secret if user == "root" else self.reader_secret
                (self.private / name).write_text(f"[client]\nuser={user}\npassword={password}\n")
        elif self.engine == "postgres":
            major = int(re.search(r"\d+", item["id"]).group())
            destination = "/var/lib/postgresql" if major >= 18 else "/var/lib/postgresql/data"
            env = ["-e", "POSTGRES_PASSWORD_FILE=/run/oo/password", "-e", "POSTGRES_DB=fixture"]
        else:
            destination = "/u01/app/oracle/oradata" if self.version.startswith("11") else "/opt/oracle/oradata"
            env = ["-e", "ORACLE_PASSWORD_FILE=/run/oo/password"]
        (self.private / "password").write_text(self.secret)
        (self.private / "reader-password").write_text(self.reader_secret)
        # Bind directory is private on the host; clients run as image-specific users.
        for file in self.private.iterdir():
            file.chmod(0o644)
        self.private.chmod(0o755)
        self.created_container = True
        server_args = (["-c", "logging_collector=on", "-c", "log_destination=csvlog",
                        "-c", "log_filename=oo-audit.csv", "-c", "log_rotation_age=0",
                        "-c", "log_rotation_size=0"] if self.engine == "postgres" else [])
        self.command(["docker", "create", "--name", self.name, *labels, "--platform", platform,
                      "--mount", f"type=volume,src={self.volume},dst={destination}",
                      "--mount", f"type=bind,src={self.private},dst=/run/oo,readonly",
                      "--shm-size", "1g", *env, image, *server_args])
        self.command(["docker", "start", self.name])
        timeout = self.args.health_timeout or (1200 if platform != native or self.engine == "oracle" else 300)
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if self.engine == "oracle":
                _, startup = self.command(["docker", "logs", "--tail", "80", self.name],
                                          required=False, timeout=30)
                if "DATABASE IS READY TO USE" not in startup:
                    code = 1
                else:
                    code, _ = self.command(["docker", "exec", self.name, "healthcheck.sh"],
                                           required=False, timeout=30)
            elif self.engine == "postgres":
                code, _ = self.command(["docker", "exec", self.name, "pg_isready",
                                        "-h", "127.0.0.1", "-U", "postgres", "-d", "fixture"],
                                       required=False, timeout=30)
            else:
                # Query, not only TCP/pg_isready. This uses the separate loader account.
                code, _ = self.sql("SELECT 1;\n", admin=True, required=False)
            if code == 0:
                break
            _, state = self.command(["docker", "inspect", "--format", "{{.State.Status}}", self.name])
            if state.strip() in ("exited", "dead"):
                raise RuntimeError("container stopped before readiness")
            time.sleep(3)
        else:
            raise RuntimeError("database health timeout")

    def seed(self):
        if self.engine == "oracle":
            if "T7" in getattr(self.args, "suites", []) and self.version.startswith(("11", "21")):
                # Standard audit configuration is static, before any fixture activity.
                _, unified = self.command(
                    ["docker", "exec", "-i", self.name, "sqlplus", "-s", "/nolog"],
                    data="whenever sqlerror exit failure\nconnect / as sysdba\n"
                         "set heading off feedback off pagesize 0\n"
                         "SELECT value FROM v$option WHERE parameter='Unified Auditing';\nexit\n")
                if unified.strip() != "TRUE":
                    self.command(["docker", "exec", "-i", self.name, "sqlplus", "-s", "/nolog"],
                                 data="whenever sqlerror exit failure\nconnect / as sysdba\n"
                                      "ALTER SYSTEM SET audit_trail='DB','EXTENDED' SCOPE=SPFILE;\n"
                                      "SHUTDOWN IMMEDIATE;\nSTARTUP;\nexit\n")
            # Bootstrap schema before sql() starts using CURRENT_SCHEMA.
            pdb = "" if self.version.startswith("11") else (
                "ALTER SESSION SET CONTAINER=" + ("FREEPDB1" if self.version.startswith("23") else "XEPDB1") + ";\n")
            bootstrap = ("whenever sqlerror exit failure\nconnect / as sysdba\n" + pdb +
                         f"CREATE USER fixture IDENTIFIED BY \"{self.secret}\";\n"
                         "GRANT CREATE SESSION,RESOURCE,UNLIMITED TABLESPACE,CREATE VIEW TO fixture;\nexit\n")
            self.command(["docker", "exec", "-i", self.name, "sqlplus", "-s", "/nolog"], data=bootstrap)
        seed = self.private / "seed.sql"
        with seed.open("w") as stream:
            for part in generator.generate(self.engine, self.version, self.args.scale, self.args.tables):
                stream.write(part)
        seed.chmod(0o644)
        self.sql(file=seed, admin=True)
        write_json(self.path / "seed.json", {"sha256": hashlib.sha256(seed.read_bytes()).hexdigest(),
                                            "scale": self.args.scale, "tables": self.args.tables})
        if self.engine == "mysql":
            grants = (f"CREATE USER 'fixture_reader'@'127.0.0.1' IDENTIFIED BY '{self.reader_secret}';\n"
                      "GRANT SELECT,SHOW VIEW ON fixture.* TO 'fixture_reader'@'127.0.0.1';\n")
        elif self.engine == "postgres":
            grants = ("REVOKE CREATE ON SCHEMA public FROM PUBLIC;\n"
                      "REVOKE TEMPORARY ON DATABASE fixture FROM PUBLIC;\n"
                      "CREATE ROLE fixture_reader LOGIN;\n"
                      "ALTER ROLE fixture_reader SET default_transaction_read_only=on;\n"
                      "GRANT CONNECT ON DATABASE fixture TO fixture_reader;\n"
                      "GRANT USAGE ON SCHEMA public TO fixture_reader;\nGRANT SELECT ON ALL TABLES IN SCHEMA public TO fixture_reader;\n")
        else:
            grants = (f"CREATE USER fixture_reader IDENTIFIED BY \"{self.reader_secret}\";\nGRANT CREATE SESSION TO fixture_reader;\n"
                      "BEGIN FOR t IN (SELECT table_name AS object_name FROM all_tables WHERE owner='FIXTURE'\n"
                      "UNION ALL SELECT view_name FROM all_views WHERE owner='FIXTURE') LOOP\n"
                      "EXECUTE IMMEDIATE 'GRANT SELECT ON fixture.'||t.object_name||' TO fixture_reader';\n"
                      "EXECUTE IMMEDIATE 'CREATE SYNONYM fixture_reader.'||t.object_name||' FOR fixture.'||t.object_name;\n"
                      "END LOOP; END;\n/\n")
        self.sql(grants, admin=True)

    def gold(self):
        version_sql = {"mysql": "SELECT VERSION();", "postgres": "SHOW server_version;",
                       "oracle": ("SELECT " + ("version" if self.version.startswith("11") else "version_full") +
                                  " FROM product_component_version WHERE product LIKE 'Oracle Database%';")}[self.engine]
        _, raw_version = self.sql(version_sql)
        actual = re.search(r"\d+\.\d+(?:\.\d+)*", raw_version)
        locked = re.search(r"\d+(?:\.\d+)*", resolve(self.engine, self.version)["id"]).group()
        if not actual or actual.group() != locked:
            # Oracle's image tag omits trailing database zero components.
            if not (self.engine == "oracle" and actual and actual.group().startswith(locked + ".")):
                raise RuntimeError(f"version mismatch: expected {locked}, found {raw_version.strip()}")
        self.receipt["version_actual"] = actual.group()
        answers = {}
        text = (ENGINES / "gold" / f"{self.engine}.sql").read_text()
        for question, statement in re.findall(r"-- (Q\d+):[^\n]*\n(.*?);", text, re.S):
            _, output = self.sql(statement + ";\n")
            rows = []
            for line in output.splitlines():
                if not line.strip():
                    continue
                fields = line.split("\t" if self.engine == "mysql" else "|")
                row = []
                for value in fields:
                    value = value.strip()
                    try:
                        number = Decimal(value)
                        if not number.is_finite():
                            raise InvalidOperation
                        value = format(number, "f")
                        if "." in value:
                            value = value.rstrip("0").rstrip(".")
                    except InvalidOperation:
                        pass
                    row.append(value)
                rows.append(row)
            if not rows:
                raise RuntimeError(f"empty gold answer {question}")
            answers[question] = rows
        if len(answers) != 20 or answers["Q17"] != [[str(self.args.scale)]]:
            raise RuntimeError("gold question count or scale verification failed")
        write_json(self.path / "gold-answers.json", answers)
        return answers

    def verify_shapes(self):
        counts = {"regions": 2, "areas": 4, "sites": 8, "codes": 3, "customers": 24,
                  "orders": 120, "shipments": 90, "calendar": 28, "order_lines": 240,
                  "unique_only": 1, "no_key": 2, "events": self.args.scale,
                  "growth_events": min(self.args.scale, 100), "wide_rows": 1,
                  "string_keys": 7, "type_edges": 2, "partition_events": 2,
                  "orders_audit": 1, "orders_backup": 1}
        counts.update({f"log_2026{i:02d}": 1 for i in range(1, 13)})
        counts.update({f"small_{i:04d}": 1 for i in range(self.args.tables)})
        # A single read-only UNION query avoids hundreds of client launches.
        statement = "\nUNION ALL\n".join(
            f"SELECT '{table}' AS table_name,COUNT(*) AS row_count FROM {table}" for table in counts)
        _, output = self.sql(statement + ";\n")
        observed = {}
        for line in output.splitlines():
            if line.strip():
                name, count = line.split("\t" if self.engine == "mysql" else "|")
                observed[name.strip()] = int(count.strip())
        if observed != counts:
            raise RuntimeError("fixture table counts differ")
        columns = {
            "mysql": "SELECT COUNT(*) FROM information_schema.columns WHERE table_schema='fixture' AND table_name='wide_rows';",
            "postgres": "SELECT COUNT(*) FROM information_schema.columns WHERE table_schema='public' AND table_name='wide_rows';",
            "oracle": "SELECT COUNT(*) FROM all_tab_columns WHERE owner='FIXTURE' AND table_name='WIDE_ROWS';"}
        _, output = self.sql(columns[self.engine])
        if output.strip() != "300":
            raise RuntimeError("wide fixture must have exactly 300 columns")
        _, view_total = self.sql("SELECT SUM(total) FROM order_totals;\n")
        if Decimal(view_total.strip()) != Decimal("7275"):
            raise RuntimeError("reader cannot reproduce the exact view total")
        if self.engine == "postgres":
            _, rights = self.sql(
                "SELECT has_schema_privilege(current_user,'public','CREATE'),"
                "has_database_privilege(current_user,'fixture','TEMP'),"
                "has_table_privilege(current_user,'orders','INSERT,UPDATE,DELETE,TRUNCATE');\n")
            if rights.strip() != "f|f|f":
                raise RuntimeError("PostgreSQL reader has write privileges")
        elif self.engine == "mysql":
            _, rights = self.sql("SHOW GRANTS FOR CURRENT_USER;\n")
            if not re.search(r"GRANT SELECT, SHOW VIEW ON", rights) or any(
                    word in rights for word in ("ALL PRIVILEGES", " INSERT", " UPDATE", " DELETE", " CREATE")):
                raise RuntimeError("MySQL reader grants are not read-only")
        else:
            _, rights = self.sql("SELECT COUNT(*) FROM user_sys_privs WHERE privilege<>'CREATE SESSION';\n")
            if rights.strip() != "0":
                raise RuntimeError("Oracle reader has extra system privileges")
            _, table_rights = self.sql("SELECT COUNT(*) FROM user_tab_privs_recd WHERE privilege<>'SELECT';\n")
            if table_rights.strip() != "0":
                raise RuntimeError("Oracle reader has write grants")
        write_json(self.path / "fixture-shapes.json", {
            "row_counts": observed, "wide_columns": 300,
            "limitations": (["VARCHAR PK is binary; separate NLSSORT unique index enforces case/accent equivalence"] +
                            (["11g XE partition fixture is unpartitioned", "11g JSON is CLOB, not native JSON"]
                             if self.version.startswith("11") else ["JSON stored as validated CLOB"]))
            if self.engine == "oracle" else []})

    def cleanup(self):
        failures = []
        for kind, name, created in [("container", self.name, self.created_container),
                                    ("volume", self.volume, self.created_volume)]:
            if not created:
                continue
            code, labels = self.command(["docker", kind, "inspect", "--format", "{{json .Config.Labels}}" if kind == "container" else "{{json .Labels}}", name],
                                        required=False)
            try:
                labels = json.loads(labels)
            except (ValueError, TypeError):
                labels = None
            if code or not isinstance(labels, dict) or labels.get("oo.run_id") != self.id or labels.get("oo.lane") != "h":
                failures.append(name)
                continue
            if kind == "container":
                self.command(["docker", "logs", name], required=False)
                self.command(["docker", "stats", "--no-stream", "--format", "{{json .}}", name], required=False)
            command = ["docker", kind, "rm"] + (["-f"] if kind == "container" else []) + [name]
            if self.command(command, required=False)[0]:
                failures.append(name)
        for file in self.private.iterdir():
            file.unlink()
        self.private.rmdir()
        receipt = getattr(self, "receipt", {"run_id": self.id})
        receipt.update(elapsed_seconds=round(time.monotonic() - self.started, 3),
                       cleanup_failed=failures, connections_peak=None, source_duty_cycle=None,
                       model_tokens=0, measurement_note="docker stats is a final sample, not peak proof")
        write_json(self.path / "resource-receipt.json", receipt)
        return failures


def execute_suites(run, requested, hooks):
    results = {}
    audit = Audit(run) if "T7" in requested else None
    if audit:
        audit.enable()
        audit.marker("start")
    context = run.path / "hook-context.json"
    write_json(context, {"container": run.name, "engine": run.engine, "version": run.version,
                         "run_id": run.id, "evidence": str(run.path), "reader": "fixture_reader",
                         "variant": getattr(run.args, "variant", None),
                         "reader_password_file": str(run.private / "reader-password")})
    old_context = os.environ.get("OO_TEST_CONTEXT")
    os.environ["OO_TEST_CONTEXT"] = str(context)
    try:
        for suite in requested:
            if suite == "T7" or suite == "T0" and suite not in hooks:
                continue
            if suite not in hooks:
                results[suite] = "not_implemented"
                continue
            if audit:
                audit.marker(suite + "_start")
            code, _ = run.command([hooks[suite]], timeout=3600, required=False)
            results[suite] = "passed" if code == 0 else "failed"
            if audit:
                audit.marker(suite + "_end")
    finally:
        if old_context is None:
            os.environ.pop("OO_TEST_CONTEXT", None)
        else:
            os.environ["OO_TEST_CONTEXT"] = old_context
        if audit:
            audit.marker("end")
            report = audit.collect()
            report["dependencies"] = {s: results.get(s, "not_run") for s in ("T1", "T2")}
            report["passed"] &= all(v == "passed" for v in report["dependencies"].values())
            write_json(run.path / "audit-report.json", report)
            results["T7"] = "passed" if report["passed"] else "failed"
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    matrix = sub.add_parser("matrix")
    selection = matrix.add_mutually_exclusive_group(required=True)
    selection.add_argument("--engine", choices=["mysql", "postgres", "oracle"])
    selection.add_argument("--all", action="store_true", help="all P0 targets, including blocked Oracle 19c")
    matrix.add_argument("--versions")
    matrix.add_argument("--suites", type=suites, default=["T0"])
    matrix.add_argument("--scale", type=int, default=2_000_000)
    matrix.add_argument("--tables", type=int, default=200)
    matrix.add_argument("--platform", choices=["linux/amd64", "linux/arm64"])
    matrix.add_argument("--runner", choices=["local", "remote"], default="local")
    matrix.add_argument("--health-timeout", type=int)
    matrix.add_argument("--baseline", type=Path, help="another run's gold-answers.json")
    matrix.add_argument("--variant", choices=["c1", "c5"], help="capability path exercised by hooks")
    matrix.add_argument("--hook", action="append", default=[], metavar="T1=PATH",
                        help="executable hook; receives owned-container context via OO_TEST_CONTEXT")
    args = parser.parse_args()
    if "T7" in args.suites:
        args.suites = sorted(set(args.suites) | {"T1", "T2"})
    if args.runner == "remote":
        parser.error("remote runner not implemented; no SSH connection attempted")
    if not 1 <= args.scale <= 10_000_000 or not 0 <= args.tables <= 5000:
        parser.error("scale must be 1..10000000; tables must be 0..5000")
    if args.all and args.versions or not args.all and not args.versions:
        parser.error("--versions is required with --engine and forbidden with --all")
    versions = args.versions.split(",") if args.versions else []
    if len(versions) != len(set(versions)):
        parser.error("duplicate versions")
    if args.health_timeout is not None and args.health_timeout <= 0:
        parser.error("health timeout must be positive")
    for version in versions:
        if not re.fullmatch(r"[0-9][0-9.a-z]*", version):
            parser.error("invalid version")
        resolve(args.engine, version)
    hooks = {}
    for hook in args.hook:
        suite, sep, path = hook.partition("=")
        if not sep or not re.fullmatch(r"T[0-68]", suite) or not Path(path).is_file() or not os.access(path, os.X_OK):
            parser.error("hook must be T0..T6 or T8=existing-executable; T7 is built in")
        hooks[suite] = str(Path(path).resolve())
    targets = P0 if args.all else [(args.engine, version) for version in versions]
    run_id = "h-s5-" + datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S%fZ-") + str(uuid.uuid4())
    root = ROOT / "evidence" / run_id
    root.mkdir(parents=True)
    print(f"evidence/{run_id}", flush=True)
    baseline = json.loads(args.baseline.read_text()) if args.baseline else None
    report = {"run_id": run_id, "consistency": "B", "cross_engine_baseline": str(args.baseline) if args.baseline else None,
              "hook_sha256": {suite: hashlib.sha256(Path(path).read_bytes()).hexdigest()
                              for suite, path in hooks.items()},
              "source_sha256": {name: hashlib.sha256(Path(__file__).with_name(name).read_bytes()).hexdigest()
                                for name in ("runner.py", "audit.py", "matrix.py")},
              "results": []}
    def interrupted(signum, frame):
        raise KeyboardInterrupt(f"signal {signum}")
    signal.signal(signal.SIGTERM, interrupted)
    interrupted_run = False
    for engine, version in targets:
        path = root / f"{engine}-{version}"
        path.mkdir()
        run = Run(path, engine, version, args)
        result = {"engine": engine, "version": version, "status": "failed", "suites": {},
                  "product_t0": "T0" in hooks, "variant": args.variant}
        try:
            item = resolve(engine, version)
            result["version_locked"] = re.search(r"\d+(?:\.\d+)*", item["id"]).group()
            if args.all:
                missing = [s for s in args.suites if s not in ("T0", "T7") and s not in hooks]
                if missing:
                    result["suites"].update({s: "not_implemented" for s in missing})
                    raise RuntimeError("missing product hooks: " + ",".join(missing))
            run.setup()
            run.seed()
            run.verify_shapes()
            answers = run.gold()
            if baseline is None:
                baseline = answers
                result["comparison"] = "reference-only"
            else:
                differences = [q for q in answers if answers[q] != baseline.get(q)]
                result["comparison"] = {"equal": not differences, "differing_questions": differences}
                if differences or set(answers) != set(baseline):
                    raise RuntimeError("gold mismatch")
            if "T0" in args.suites:
                result["suites"]["T0"] = "passed"
            result["suites"].update(execute_suites(run, args.suites, hooks))
            if any(value != "passed" for value in result["suites"].values()):
                raise RuntimeError("requested suite failed or is not implemented")
            result["status"] = "passed"
        except (Exception, KeyboardInterrupt) as error:
            result["error"] = redact(str(error), run.secret, run.reader_secret)
            interrupted_run = isinstance(error, KeyboardInterrupt)
        finally:
            # A second interrupt must not strand resources during normal cleanup.
            previous = {sig: signal.signal(sig, signal.SIG_IGN) for sig in (signal.SIGINT, signal.SIGTERM)}
            try:
                if run.cleanup():
                    result["status"] = "failed"
            finally:
                for sig, handler in previous.items():
                    signal.signal(sig, handler)
            write_json(path / "comparison-report.json", result)
            checksums(path)
        report["results"].append(result)
        write_json(root / "comparison-report.json", report)
        print(json.dumps(result), flush=True)
        if interrupted_run:
            break
    # Unstarted targets must not disappear on interruption.
    for engine, version in targets[len(report["results"]):]:
        path = root / f"{engine}-{version}"
        path.mkdir()
        result = {"engine": engine, "version": version, "status": "failed",
                  "suites": {}, "error": "interrupted before target"}
        write_json(path / "comparison-report.json", result)
        checksums(path)
        report["results"].append(result)
    write_json(root / "comparison-report.json", report)
    rows = index_rows(root, report, args.suites)
    write_json(root / "evidence-index.json", rows)
    write_json(root / "downgrade-report.json", downgrade_report(rows))
    checksums(root)
    return 0 if all(r["status"] == "passed" for r in report["results"]) else 1
