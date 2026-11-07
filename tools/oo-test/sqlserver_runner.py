"""Lane E's SQL Server fixtures, using the shared matrix evidence contract."""
import json
import re
import shutil
import time
from decimal import Decimal

import runner
import sqlserver_seed


class SqlServerRun(runner.Run):
    def __init__(self, *args):
        super().__init__(*args)
        self.name = "oo-e-sqlserver-" + self.id
        self.secret = "A9!" + self.secret
        self.reader_secret = "B8!" + self.reader_secret
        self.tables = []

    def command(self, command, **kwargs):
        required = kwargs.pop("required", True)
        for attempt in range(3):
            code, out = super().command(command, required=False, **kwargs)
            error = (self.path / f"{self.seq:04d}.stderr").read_text()
            # Only a local socket refusal proves that Docker never dispatched
            # the operation. Never replay an SQL error or an ambiguous timeout.
            refused = (command[0] == "docker" and not out and
                       (error.startswith("Cannot connect to the Docker daemon at ")
                        and "Is the docker daemon running?" in error
                        or error.startswith("cannot connect to the Docker daemon. ")
                        and error.rstrip().endswith("connect: connection refused")))
            if code and refused and attempt < 2:
                time.sleep(.2)
                continue
            if code and required:
                raise RuntimeError(f"command {self.seq:04d} exited {code}")
            return code, out

    def private_file(self, name, value):
        path = self.private / name
        path.write_text(value)
        path.chmod(0o600)
        return path

    def sql(self, text=None, file=None, admin=False, required=True):
        user = "sa" if admin else "fixture_reader"
        password = "$MSSQL_SA_PASSWORD" if admin else "$(cat /run/oo/reader-password)"
        script = (f'export SQLCMDPASSWORD="{password}"; '
                  'if [ -x /opt/mssql-tools18/bin/sqlcmd ]; then c=/opt/mssql-tools18/bin/sqlcmd; '
                  'else c=/opt/mssql-tools/bin/sqlcmd; fi; '
                  f'exec "$c" -S localhost -U {user} -C -b -r1 -h-1 -y0 -w65535 -s "|"')
        # -y0 preserves large JSON/LOB cells; sqlcmd forbids combining it with -h.
        script = script.replace("-h-1 -y0", "-y0")
        if file:
            text = file.read_text()
        return self.command(["docker", "exec", "-i", self.name, "sh", "-c", script],
                            data="SET NOCOUNT ON;\n" + (text or "") + "\nGO\n",
                            timeout=1800, required=required)

    def query(self, query, admin=True):
        return self.sql("USE fixture;\n" + query, admin=admin)[1]

    def json_query(self, query, admin=True):
        raw = self.query(query + " FOR JSON PATH, INCLUDE_NULL_VALUES;", admin=admin)
        # sqlcmd wraps JSON strings; remove only transport line breaks.
        start, end = raw.find("[{"), raw.rfind("}]")
        if start == -1:
            if any(line.strip() == "[]" for line in raw.splitlines()):
                return []
            raise RuntimeError("SQL Server JSON transport missing")
        return json.loads("".join(raw[start:end + 2].splitlines()), parse_float=Decimal)

    def setup(self):
        item = runner.resolve("sqlserver", self.version)
        if self.args.platform not in (None, "linux/amd64"):
            raise ValueError("SQL Server requires linux/amd64")
        image = item["ref"] + "@" + item["platforms"]["linux/amd64"]["digest"]
        _, arch = self.command(["docker", "info", "--format", "{{.Architecture}}"])
        self.receipt = {
            "run_id": self.id, "image": image, "platform": "linux/amd64",
            "execution": "emulated" if arch.strip() in ("arm64", "aarch64") else "native",
            "scale": self.args.scale, "tables": self.args.tables, "cleanup_failed": [],
        }
        files = [runner.ROOT / "tools/oo-test" / name for name in (
            "sqlserver_runner.py", "sqlserver_seed.py", "sqlserver_acceptance.py",
            "sqlserver_cycles.py", "sqlserver_audit.py")]
        files += [runner.ENGINES / "sqlserver/seed/preamble.sql",
                  runner.ENGINES / "sqlserver/images.lock.json",
                  runner.ENGINES / "seed-common/generate.py",
                  runner.ROOT / "tools/oo-load/src/sqlserver_load.rs",
                  runner.ROOT / "crates/oo-connectors/src/sqlserver/mod.rs"]
        self.receipt["source_sha256"] = {
            str(f.relative_to(runner.ROOT)): runner.hashlib.sha256(f.read_bytes()).hexdigest()
            for f in files}
        runner.write_json(self.path / "resource-receipt.json", self.receipt)
        self.command(["docker", "pull", "--platform", "linux/amd64", image], timeout=1200)
        import t1
        t1.configure_tls(self, self.private)
        self.private_file("reader-password", self.reader_secret)
        env = self.private_file("container.env",
            f"ACCEPT_EULA=Y\nMSSQL_PID=Developer\nMSSQL_SA_PASSWORD={self.secret}\nMSSQL_AGENT_ENABLED=true\n")
        conf = self.private_file("mssql.conf",
            "[network]\ntlscert = /run/oo/server-cert\ntlskey = /run/oo/server-key\n"
            "tlsprotocols = 1.2\nforceencryption = 1\n")
        self.command(["docker", "create", "--name", self.name, "--platform", "linux/amd64",
                      "--label", "oo.lane=e", "--label", "oo.run_id=" + self.id,
                      "--user", "0", "--memory", "3g", "--env-file", str(env),
                      "-p", "127.0.0.1::1433", "-v", f"{self.private}:/run/oo:ro",
                      "-v", f"{conf}:/var/opt/mssql/mssql.conf:ro", image])
        self.created_container = True
        self.command(["docker", "start", self.name])
        deadline = time.monotonic() + (self.args.health_timeout or 600)
        while self.sql("SELECT 1;", admin=True, required=False)[0]:
            if time.monotonic() > deadline:
                raise RuntimeError("SQL Server startup timeout")
            time.sleep(2)
        _, port = self.command(["docker", "port", self.name, "1433/tcp"])
        self.port = int(port.strip().rsplit(":", 1)[1])

    def seed(self):
        self.sql("CREATE DATABASE fixture;\nGO\n"
                 "ALTER DATABASE fixture SET ALLOW_SNAPSHOT_ISOLATION ON;\n"
                 "ALTER DATABASE fixture SET CHANGE_TRACKING=ON (CHANGE_RETENTION=7 DAYS);\n"
                 f"CREATE LOGIN fixture_reader WITH PASSWORD='{self.reader_secret}';\n"
                 "GRANT VIEW SERVER STATE TO fixture_reader;\nGO\n"
                 "USE fixture; CREATE USER fixture_reader FOR LOGIN fixture_reader;\n"
                 "GRANT SELECT,VIEW DEFINITION TO fixture_reader;\n", admin=True)
        seed = self.private / "seed.sql"
        seed.write_text("USE fixture;\nGO\n" + "".join(
            sqlserver_seed.generate(runner.generator, self.args.scale, self.args.tables)))
        seed.chmod(0o600)
        self.sql(file=seed, admin=True)
        runner.write_json(self.path / "seed.json", {
            "sha256": runner.hashlib.sha256(seed.read_bytes()).hexdigest(),
            "scale": self.args.scale, "tables": self.args.tables})
        self.tables = self.json_query(
            "SELECT t.name FROM sys.tables t JOIN sys.schemas s ON s.schema_id=t.schema_id "
            "WHERE s.name='dbo' ORDER BY t.name")
        self.tables = [t["name"] for t in self.tables]

    def verify_shapes(self):
        counts = self.json_query("\nUNION ALL\n".join(
            f"SELECT '{t}' AS name,COUNT_BIG(*) AS n FROM [{t}]" for t in self.tables))
        observed = {r["name"]: r["n"] for r in counts}
        expected = {"regions": 2, "areas": 4, "sites": 8, "codes": 3, "customers": 24,
                    "orders": 120, "shipments": 90, "calendar": 28, "order_lines": 240,
                    "unique_only": 1, "no_key": 2, "events": self.args.scale,
                    "growth_events": min(self.args.scale, 100), "wide_rows": 1,
                    "string_keys": 7, "type_edges": 2, "partition_events": 2,
                    "orders_audit": 1, "orders_backup": 1}
        expected.update({f"log_2026{i:02d}": 1 for i in range(1, 13)})
        expected.update({f"small_{i:04d}": 1 for i in range(self.args.tables)})
        if observed != expected:
            raise RuntimeError("fixture row count mismatch")
        width = self.json_query("SELECT COUNT(*) AS n FROM sys.columns WHERE object_id=OBJECT_ID('wide_rows')")[0]["n"]
        if width != 300:
            raise RuntimeError("wide table shape mismatch")
        runner.write_json(self.path / "fixture-shapes.json", {"row_counts": observed, "wide_columns": width})

    def gold(self):
        version = self.json_query("SELECT CONVERT(nvarchar(128),SERVERPROPERTY('ProductVersion')) AS version")[0]["version"]
        if version != runner.resolve("sqlserver", self.version)["product_version"]:
            raise RuntimeError("SQL Server locked patch mismatch")
        self.receipt["version_actual"] = version
        answers = {}
        text = (runner.ENGINES / "gold/mysql.sql").read_text().replace("DATE '", "'")
        for question, statement in re.findall(r"-- (Q\d+):[^\n]*\n(.*?);", text, re.S):
            # Use sqlcmd's unheaded scalar mode for the common gold query output.
            command = ["docker", "exec", "-i", self.name, "sh", "-c",
                'export SQLCMDPASSWORD="$(cat /run/oo/reader-password)"; '
                'c=/opt/mssql-tools18/bin/sqlcmd; [ -x "$c" ] || c=/opt/mssql-tools/bin/sqlcmd; '
                'exec "$c" -S localhost -U fixture_reader -d fixture -C -b -h-1 -W -s "|"']
            _, raw = self.command(command, data="SET NOCOUNT ON;\n" + statement + ";\nGO\n")
            rows = []
            for line in raw.splitlines():
                if line.strip():
                    row = []
                    for v in line.split("|"):
                        v = v.strip()
                        try:
                            v = format(Decimal(v), "f")
                            if "." in v:
                                v = v.rstrip("0").rstrip(".")
                        except runner.InvalidOperation:
                            pass
                        row.append(v)
                    rows.append(row)
            answers[question] = rows
        if len(answers) != 20 or answers["Q17"] != [[str(self.args.scale)]]:
            raise RuntimeError("gold mismatch")
        runner.write_json(self.path / "gold-answers.json", answers)
        return answers

    def execute(self, requested):
        from sqlserver_acceptance import execute
        return execute(self, requested)

    def cleanup(self):
        failures = list(getattr(self, "receipt", {}).get("cleanup_failed", []))
        if self.created_container:
            _, raw = self.command(["docker", "inspect", "--format", "{{json .Config.Labels}}", self.name])
            labels = json.loads(raw)
            if labels.get("oo.lane") != "e" or labels.get("oo.run_id") != self.id:
                failures.append(self.name)
            else:
                self.command(["docker", "logs", self.name], required=False)
                if self.command(["docker", "rm", "-f", "-v", self.name], required=False)[0]:
                    failures.append(self.name)
        shutil.rmtree(self.private)
        receipt = getattr(self, "receipt", {})
        receipt["cleanup_failed"] = failures
        runner.write_json(self.path / "resource-receipt.json", receipt)
        return failures
