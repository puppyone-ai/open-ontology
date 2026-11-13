"""Extend the owned live Gateway fixture with lane H's neutral gold data."""
import argparse
import json
import os
import re
import subprocess

from runner import ROOT, generator, write_json
from t1 import LaneRun, configure_mysql, configure_pg, private
from t6 import cell, load_module, redact_material

cycles = load_module("t6_gateway_cycles", ROOT / "tools/oo-load/test-gateway-cycles.py")
TABLES = ["regions", "areas", "sites", "codes", "customers", "orders", "shipments",
          "calendar", "order_lines", "events", "log_202601", "log_202602"]


def ident(value):
    if not re.fullmatch(r"[a-zA-Z_][a-zA-Z0-9_]*", value):
        raise ValueError("FIXTURE_IDENTIFIER")
    return '"' + value + '"'


def prepare_failure(result):
    """Name a publish run without a cut by oo-load's stable error code only."""
    try:
        code = json.loads(result.stderr.decode().strip().splitlines()[-1])["error"]
    except (ValueError, IndexError, KeyError, TypeError):
        code = None
    if isinstance(code, str) and re.fullmatch(r"[a-z][a-z0-9_]{0,40}", code):
        return "PREPARE_" + code.upper()
    return "PREPARE_NO_CUT"


class GoldFixture(cycles.Live):
    def __init__(self, out, engine, scale, version=None, worker_image=None):
        super().__init__(out, engine)
        self.version = version or {"postgres": "17", "mysql": "8.4", "oracle": "23"}[engine]
        self.worker_image = worker_image
        self.oracle_worker = None
        self.worker_args = []
        self.separate_postgres = engine == "postgres" and not self.version.startswith("17")
        self.scale = scale
        self.seeded = False
        self.schema = {"postgres": "app", "mysql": "fixture", "oracle": "FIXTURE"}[engine]
        if self.separate_postgres:
            self.schema = "public"
        self.names = [t.upper() if engine == "oracle" else t for t in TABLES]
        self.run.secret = self.db.password  # Shared logger redacts the fixture DB password.
        self.command_seq = 0
        self.cli_env = dict(os.environ, OO_CONFIG_DIR=str(self.root / "client"))
        self.identity = {"project_id": "f22", "principal_id": "alice", "role": "oo_r_f22",
                         "policy_version": "pv1", "knowledge_version": "kv1"}

    def source_sql(self, sql):
        if self.separate_postgres and "CREATE SCHEMA app" in sql:
            return ""  # The independently versioned source is seeded before fixture-pack.
        result = super().source_sql(sql)
        if self.engine == "postgres" and not self.seeded and "CREATE SCHEMA app" in sql:
            self.seeded = True
            seed = "SET search_path TO app;\n" + "".join(
                generator.generate("postgres", "17", self.scale, 200))
            super().source_sql(seed)
            super().source_sql("GRANT USAGE ON SCHEMA app TO reader; "
                              "GRANT SELECT ON ALL TABLES IN SCHEMA app TO reader;")
        return result

    def external_source(self):
        path = self.out / "source"
        path.mkdir()
        self.source = LaneRun(path, self.engine, self.version, argparse.Namespace(
            platform=None, health_timeout=None, scale=self.scale, tables=200))
        self.source.setup()
        self.source.seed()
        private(self.db.directory / "source-password", self.source.reader_secret)
        if self.engine in ("postgres", "mysql"):
            tls = self.root / "source-tls"
            tls.mkdir()
            (configure_pg if self.engine == "postgres" else configure_mysql)(self.source, tls)
            private(self.db.directory / "source-tls-ca", (tls / "source-ca").read_text())
        else:
            self.source.sql(
                "GRANT SELECT ON SYS.V_$DATABASE TO fixture_reader;\n"
                "GRANT SELECT ON SYS.V_$SESSION TO fixture_reader;\n"
                "GRANT SELECT ON SYS.V_$SQL_PLAN TO fixture_reader;\n"
                "GRANT SELECT ON SYS.DBA_EXTENTS TO fixture_reader;\n"
                "GRANT EXECUTE ON SYS.DBMS_ROWID TO fixture_reader;\n" +
                "\n".join(f'GRANT FLASHBACK ON fixture.{ident(t)} TO fixture_reader;'
                          for t in self.names), admin=True)
        self.gold = self.source.gold()
        write_json(self.out / "gold-answers.json", self.gold)
        port = {"postgres": 5432, "mysql": 3306, "oracle": 1521}[self.engine]
        _, endpoint = self.source.command(["docker", "port", self.source.name, f"{port}/tcp"])
        database = "fixture"
        if self.engine == "oracle":
            database = ("XE" if self.version.startswith("11") else
                        "XEPDB1" if self.version.startswith("21") else "FREEPDB1")
        config = {"id": "owned", "engine": self.engine, "host": "127.0.0.1",
                  "port": int(endpoint.strip().rsplit(":", 1)[1]), "database": database,
                  "user": "fixture_reader", "password": {"secret_ref": "source-password"},
                  "scope": {"include": [self.schema + "." + t for t in self.names]}}
        if self.engine in ("postgres", "mysql"):
            config["tls_ca"] = {"secret_ref": "source-tls-ca"}
        if self.engine == "oracle" and self.version.startswith("11"):
            if not self.worker_image:
                raise ValueError("ORACLE_WORKER_REQUIRED")
            name = "oo-g-worker-" + self.source.id
            self.source.command([
                "docker", "create", "--name", name, "--platform", "linux/amd64",
                "--label", "oo.lane=g", "--label", "oo.run_id=" + self.source.id,
                "--network", "container:" + self.source.name, "--mount",
                f"type=bind,src={ROOT / 'testing/engines/oracle/worker/worker.py'},dst=/worker.py,readonly",
                "--entrypoint", "sleep", self.worker_image, "infinity"])
            self.oracle_worker = name
            self.source.command(["docker", "start", name])
            worker = self.root / "oracle-worker"
            private(worker, f"#!/bin/sh\nexec docker exec -i {name} python /worker.py\n")
            worker.chmod(0o700)
            self.worker_args = ["--oracle-worker", str(worker)]
            config["port"] = 1521
        write_json(self.out / "source-version.json", {
            "engine": self.engine, "version": self.source.receipt["version_actual"]})
        return config

    def read_source(self, sql):
        if self.source:
            return self.source.sql(sql)[1]
        return self.run.command([
            "docker", "exec", "-i", self.db.name, "psql", "-X", "-U", "reader",
            "-d", "source", "-v", "ON_ERROR_STOP=1", "-A", "-t", "-F", "|",
            "-P", "null=__OO_NULL__"], data="SET search_path TO app;\n" + sql)[1]

    def inventory(self):
        names = ",".join("'" + t + "'" for t in self.names)
        if self.engine == "oracle":
            sql = ("SELECT table_name||'|'||column_name||'|'||data_type "
                   f"FROM all_tab_columns WHERE owner='{self.schema}' AND table_name IN ({names}) "
                   "ORDER BY table_name,column_id;")
        else:
            sep = "||'|'||" if self.engine == "postgres" else ", '|', "
            expression = sep.join(["table_name", "column_name", "data_type"])
            if self.engine == "mysql":
                expression = "CONCAT(" + expression + ")"
            sql = (f"SELECT {expression} FROM information_schema.columns "
                   f"WHERE table_schema='{self.schema}' AND table_name IN ({names}) "
                   "ORDER BY table_name,ordinal_position;")
        tables = {}
        for line in self.read_source(sql).splitlines():
            if "|" not in line:
                continue
            table, column, dtype = line.strip().split("|")
            name = column.lower()
            semantic = ("identifier" if name in ("id", "line_no") else
                        "foreign_key" if name.endswith("_id") else
                        "event_time" if name.endswith("_at") else
                        "date" if name == "day_date" else
                        "code" if name in ("code", "status_code", "month_no") else
                        "quantity" if name in ("amount", "qty", "event_no") else "free_text")
            tables.setdefault(table, []).append({
                "name": column, "data_type": dtype.lower(),
                "semantic_type": semantic, "pii": "none",
                "classification": {"state": "confirmed", "by": "fixture/accept-all"},
                "provenance": "declared"})
        if set(tables) != set(self.names):
            raise ValueError("FIXTURE_INVENTORY")
        self.columns = tables
        if self.engine == "postgres" and not self.source:
            self.gold = {}
            text = (ROOT / "testing/engines/gold/postgres.sql").read_text()
            for question, statement in re.findall(r"-- (Q\d+):[^\n]*\n(.*?);", text, re.S):
                raw = self.read_source(statement + ";")
                self.gold[question] = [
                    [None if v == "__OO_NULL__" else cell(v.strip()) for v in line.split("|")]
                    for line in raw.splitlines() if line.strip() and line.strip() != "SET"]
            version = self.db.sql("SHOW server_version").strip()
            if not version.startswith("17.11"):
                raise ValueError("SOURCE_VERSION")
            write_json(self.out / "source-version.json", {"engine": "postgres", "version": version})
            write_json(self.out / "gold-answers.json", self.gold)
        if len(self.gold) != 20 or self.gold["Q17"] != [[str(self.scale)]]:
            raise ValueError("GOLD_COUNT")

    def command(self, argv, label, data=None):
        if label == "fixture-pack":
            if self.separate_postgres:
                stack = json.loads(self.stack.read_text())
                stack["sources"] = [self.external_source()]
                self.stack.write_text(json.dumps(stack))
            self.inventory()
            request = json.loads(data)
            # Bootstrap the inherited fixture only. The published Pack is built
            # after oo-load supplies authoritative mapped names and target types.
            self.pack_request = request
            self.pack_effective = request["effective_oos"]
        # Do not use Live.command's Oracle single-column specialization.
        command = [str(a) for a in argv]
        try:
            result = subprocess.run(command, input=data, capture_output=True,
                                    timeout=1800, env=dict(os.environ, CARGO_TARGET_DIR=str(ROOT / "target")))
        except subprocess.TimeoutExpired as exc:
            result = subprocess.CompletedProcess(command, 124, exc.stdout or b"", exc.stderr or b"")
        write_json(self.out / (label + ".command.json"), {
            "argv": [a.replace(str(ROOT), "<worktree>") for a in command],
            "exit": result.returncode})
        (self.out / (label + ".stdout")).write_bytes(result.stdout)
        (self.out / (label + ".stderr")).write_bytes(result.stderr)
        if result.returncode:
            raise RuntimeError("COMMAND_" + label)
        return result

    def load(self, prefix, run):
        stack = json.loads(self.stack.read_text())
        stack["sources"][0]["scope"]["include"] = [self.schema + "." + t for t in self.names]
        self.stack.write_text(json.dumps(stack))
        result = super().load([*prefix, *self.worker_args], run)
        write_json(self.out / "source-column-types.json", self.columns)
        # Gateway binds exact Agent DB format_type values, not source type
        # spellings (Oracle NUMBER, for example, can be projected as text).
        receipts = [json.loads(line) for line in result.stdout.splitlines() if line.strip()]
        seen = set()
        for receipt in receipts:
            table, target = receipt["table"]["name"], receipt["target"]
            if table in seen or table not in self.columns:
                raise ValueError("LOAD_TABLE_IDENTITY")
            seen.add(table)
            if not re.fullmatch(r"oo_load_data\.[a-z0-9_]+", target):
                raise ValueError("LOAD_TARGET_IDENTITY")
            raw = self.db.sql(
                "SELECT attname||'|'||format_type(atttypid,atttypmod) FROM pg_attribute "
                f"WHERE attrelid='{target}'::regclass AND attnum>0 AND NOT attisdropped ORDER BY attnum;")
            actual = dict(line.split("|", 1) for line in raw.strip().splitlines())
            if list(actual) != [c["name"] for c in self.columns[table]]:
                raise ValueError("LOAD_COLUMN_IDENTITY")
            for column in self.columns[table]:
                column["data_type"] = actual[column["name"]]
        if seen != set(self.names):
            raise ValueError("LOAD_TABLE_COUNT")
        return result

    def compile_catalog(self, cut):
        datasets = []
        published = {t["physical_name"]: t for shard in cut["receipt"]["shards"]
                     for t in shard["receipt"]["tables"]}
        seen = set()
        for entry in cut["data_vector"]:
            source = published[entry["physical_name"]]
            table = source["table"]
            if source["schema"] != self.schema or table in seen or table not in self.columns:
                raise ValueError("PACK_SOURCE_IDENTITY")
            seen.add(table)
            original = self.columns[table]
            expected = entry["expected_columns"]
            if len(original) != len(expected) or any(
                    c["data_type"] != mapped["data_type"] for c, mapped in zip(original, expected)):
                raise ValueError("PACK_TARGET_TYPES")
            datasets.append({
                "schema_version": "oos/v1", "dataset": entry["dataset"], "label": table,
                "tier": "raw", "primary_key": [],
                "columns": [dict(c, name=mapped["name"], label=c["name"])
                            for c, mapped in zip(original, expected)]})
        if seen != set(self.names):
            raise ValueError("PACK_TABLE_COUNT")
        effective, visibility = self.pack_request["effective_oos"], self.pack_request["visibility"]
        effective["datasets"] = datasets
        effective["ontology"]["include_schemas"] = sorted(
            {t["dataset"].split(".")[0] for t in datasets})
        visibility["datasets"] = [
            {"dataset": t["dataset"], "columns": [c["name"] for c in t["columns"]],
             "row_security": "none", "global_stats": "denied"} for t in datasets]
        built = self.command([self.args.builder, "role-pack"], "target-pack",
                             self.fixture.encode(self.pack_request))
        self.artifact = json.loads(built.stdout)["artifact"]
        # Build/review is deterministic fixture metadata, not a T5 model-build claim.
        review = self.base_catalog["envelope"]["review"]
        review["columns"] = {
            f"{t['dataset']}.{c['name']}": {
                "pii": "none", "semantic_type": c["semantic_type"], "reviewer": "fixture/accept-all"}
            for t in datasets for c in t["columns"]}
        write_json(self.out / "accept-all-review.json", review)
        producer = self.base_catalog["envelope"]["producer"]
        producer.update(artifact=self.artifact, pack_digest=self.artifact["pack_digest"],
                        effective_sha256=self.fixture.sha(effective),
                        visibility_sha256=self.fixture.sha(visibility),
                        review_sha256=self.fixture.sha(review))
        self.base_catalog.update(effective=effective, visibility=visibility)
        self.base_catalog["envelope"]["output_sha256"] = self.fixture.sha(producer)
        write_json(self.out / "pack-input.json", self.pack_request)

    def prepare(self, run):
        argv = [ROOT / "target/debug/oo-load", "publish", "--stack", self.stack,
                "--source", "owned", "--target", self.target, "--secrets-dir", self.db.directory,
                "--generation", run, "--library", "owned"]
        pending = subprocess.run([str(a) for a in argv], capture_output=True, timeout=120)
        write_json(self.out / "prepare.command.json", {
            "argv": [str(a).replace(str(ROOT), "<worktree>") for a in argv],
            "exit": pending.returncode})
        (self.out / "prepare.stdout").write_bytes(pending.stdout)
        (self.out / "prepare.stderr").write_bytes(pending.stderr)
        try:
            cut = json.loads(pending.stdout)
        except ValueError:
            raise ValueError(prepare_failure(pending)) from None
        if pending.returncode == 0 or cut["state"] != "pending":
            raise ValueError("EXPECTED_PENDING_FENCE")
        self.cuts.append(cut)
        return argv, cut

    def publish(self, run):
        argv, cut = self.prepare(run)
        self.compile_catalog(cut)
        for entry in cut["data_vector"]:
            physical = entry["physical_name"]
            columns = self.db.sql(
                "SELECT attname FROM pg_attribute "
                f"WHERE attrelid='_oo.{physical}'::regclass AND attnum>0 AND NOT attisdropped "
                "ORDER BY attnum;").strip().splitlines()
            quoted = ",".join(ident(c) for c in columns)
            literals = ",".join("'" + c + "'" for c in columns)
            self.db.sql(f"""BEGIN;
INSERT INTO oo_publish_target.fence(singleton,state,role_name,schema_name,relation_name,opened_columns,streams_stopped)
VALUES(true,'closed','oo_r_f22','_oo','{physical}','{{}}',0);
GRANT USAGE ON SCHEMA _oo TO oo_r_f22,oo_f22_login;
GRANT SELECT ({quoted}) ON _oo.{ident(physical)} TO oo_r_f22;
UPDATE oo_publish_target.fence SET state='granted' WHERE relation_name='{physical}';
UPDATE oo_publish_target.fence SET state='open',opened_columns=ARRAY[{literals}],
opened_pack_digest='{self.artifact['pack_digest']}',opened_version='review-{run}',candidate_id='self-{run}'
WHERE relation_name='{physical}'; COMMIT;""")
        ready = json.loads(self.command(argv, "publish-" + run).stdout)
        if ready["state"] != "ready":
            raise ValueError("PUBLICATION_NOT_READY")
        publication = {"protocol": "oo.pipeline.native-library-publication/v1", "run_id": run,
                       "receipt_sha256": cut["receipt_sha256"], "receipt": cut["receipt"]}
        bindings = []
        for entry in cut["data_vector"]:
            # Resolve against the loader's authoritative table-level mappings.
            dataset = self.db.sql(
                "SELECT logical_namespace||'.'||logical_table FROM _oo.physical_mappings "
                f"WHERE source='owned' AND run_generation={cut['generation']} AND level='t' "
                f"AND physical_name='{entry['physical_name']}';").strip()
            if dataset != entry["dataset"]:
                raise ValueError("PUBLICATION_DATASET")
            bindings.append({"dataset": dataset, "source": "owned",
                             "config_digest": cut["config_digest"], "generation": cut["generation"],
                             "type_digest": entry["recipe_sha256"], "input_digest": entry["recipe_sha256"]})
        path = self.runtime / "reviewed/library/library.json"
        path.parent.mkdir(parents=True, exist_ok=True)
        self.fixture.replace(path, self.fixture.encode(
            self.base_catalog | {"publication": publication, "bindings": bindings}))
        write_json(self.out / "cut.json", cut)
        return ready

    def start_gateway(self):
        super().start_gateway()
        # Enable the existing lifecycle API on this owned Gateway only.
        self.gateway.terminate()
        self.gateway.wait(timeout=10)
        migration = ROOT / "tools/gateway-token-store/migrations/oo_token_control_v2.sql"
        self.db.sql(migration.read_text())
        self.db.sql("INSERT INTO oo_token_lifecycle.migration VALUES(2,'" +
                    self.fixture.sha(migration.read_bytes()) + "');")
        self.db.sql(r"""SELECT format('CREATE ROLE oo_token_admin LOGIN PASSWORD %L',pg_read_file('/tmp/pw')) \gexec
GRANT CONNECT ON DATABASE postgres TO oo_token_admin;
GRANT EXECUTE ON FUNCTION pg_catalog.pg_advisory_xact_lock(integer,integer),
pg_catalog.pg_advisory_xact_lock(bigint) TO oo_token_admin;
GRANT USAGE ON SCHEMA oo_token_control,oo_token_lifecycle TO oo_token_admin;
GRANT ALL ON ALL TABLES IN SCHEMA oo_token_control,oo_token_lifecycle TO oo_token_admin;
GRANT ALL ON ALL SEQUENCES IN SCHEMA oo_token_control,oo_token_lifecycle TO oo_token_admin;
""")
        config_path = self.runtime / "config.json"
        config = json.loads(config_path.read_text())
        # Lifecycle CLI uses the origin as audience. Both are valid canonical
        # audiences; keep the fixture's issued tuples and Gateway in agreement.
        config.update(audience=self.origin + "/", token_admin_password_file="password",
                      token_admin_workload="urn:oo:workload:lane-m")
        fields = {"project_id": "project_id", "principal_id": "principal_id", "role": "role_name",
                  "policy_version": "policy_version", "knowledge_version": "knowledge_version"}
        values = [(field, self.identity[key]) for key, field in fields.items()]
        values.append(("audience_url", self.origin + "/"))
        assignments = ",".join(field + "='" + value.replace("'", "''") + "'" for field, value in values)
        self.db.sql("UPDATE oo_token_control.token_record SET " + assignments + ";")
        self.fixture.write(config_path, config)
        (self.runtime / "listen.addr").unlink()
        self.gateway = subprocess.Popen([str(ROOT / "target/debug/oo-gateway"), str(self.runtime)],
                                        stdout=self.log, stderr=self.log)
        self.fixture.wait(lambda: (self.runtime / "listen.addr").exists()
                          or self.gateway.poll() is not None, "lifecycle startup")
        if self.gateway.poll() is not None:
            raise ValueError("LIFECYCLE_STARTUP")
        write_json(self.out / "startup-catalog.json", self.http_request("/v1/tables"))

    def share_prompt(self):
        client = self.root / "client"
        client.mkdir(mode=0o700)
        private(self.root / "client.pem", (self.root / "client.crt").read_text()
                + (self.root / "client.key").read_text())
        profile = {"schema_version": 1, "gateway": self.origin,
                   "token_file": str(self.root / "client-token"),
                   "ca_file": str(self.runtime / "ca.crt"), "client_pem": str(self.root / "client.pem")}
        private(client / "config.json", json.dumps(profile))
        grant = dict(self.identity, audience_url=self.origin + "/", ttl_ms=3600000,
                     purpose="gold-dialogue")
        private(self.root / "grant.json", json.dumps(grant))
        result = self.cli(["share-prompt", "--grant-file", str(self.root / "grant.json")], record=False)
        if result["exit"]:
            write_json(self.out / "bind-failure.json", result)
            raise ValueError("SHARE_PROMPT")
        prompt = result["body"]
        codes = re.findall(r"OO-[0-9A-Z]{4}-[0-9A-Z]{4}", prompt)
        if len(codes) != 1:
            raise ValueError("SHARE_CODE")
        (self.out / "share-prompt.txt").write_text(redact_material(prompt))
        # The Agent must bind from the actual prompt, without borrowing the
        # fixture token used only to request that prompt.
        (client / "config.json").unlink()
        return prompt

    def cli(self, args, record=True):
        command = [str(ROOT / "target/debug/oo"), *args]
        stdin = None
        if args[0] == "login":
            command = [str(ROOT / "target/debug/oo"), "login", "--gateway", self.origin,
                       "--code-stdin", "--ca-file", str(self.runtime / "ca.crt"),
                       "--client-pem", str(self.root / "client.pem")]
            stdin = args[1] + "\n"
        if args[0] != "share-prompt":
            command.append("--json")
        if args[0] not in ("login", "whoami", "share-prompt"):
            p = self.profile
            command += ["--gateway", self.origin, "--auth", "external", "--token-file", p["token_file"],
                        "--ca-file", p["ca_file"], "--client-pem", p["client_pem"]]
        result = subprocess.run(command, input=stdin, text=True, capture_output=True,
                                timeout=150, env=self.cli_env)
        raw = result.stdout if args[0] == "share-prompt" else redact_material(result.stdout)
        if args[0] == "login" and result.returncode == 0:
            self.profile = json.loads((self.root / "client/config.json").read_text())
        try:
            body = json.loads(raw)
        except ValueError:
            body = raw
        output = {"exit": result.returncode, "body": body, "stderr": redact_material(result.stderr)}
        if record:
            self.command_seq += 1
            write_json(self.out / f"cli-{self.command_seq:03d}.json",
                       {"args": [redact_material(a) for a in args], **output})
        return output

    def close(self):
        if self.db.created and hasattr(self, "stack"):
            try:
                state = self.db.sql(
                    "SELECT json_build_object('open_fences',"
                    "(SELECT count(*) FROM oo_publish_target.fence WHERE state='open'),"
                    "'heads',(SELECT json_agg(json_build_object('library',library_id,'state',state)) "
                    "FROM _oo.native_library_head));")
                write_json(self.out / "final-publication-state.json", json.loads(state))
            except Exception:
                write_json(self.out / "final-publication-state.json", {"status": "unmeasured"})
        worker_error = None
        if self.oracle_worker:
            try:
                _, labels = self.source.command([
                    "docker", "inspect", "--format", "{{json .Config.Labels}}", self.oracle_worker])
                labels = json.loads(labels)
                if labels.get("oo.lane") != "g" or labels.get("oo.run_id") != self.source.id:
                    raise RuntimeError("WORKER_CLEANUP_OWNERSHIP")
                self.source.command(["docker", "rm", "-f", "-v", self.oracle_worker])
            except Exception as exc:
                worker_error = exc
        super().close()
        if worker_error:
            raise RuntimeError("WORKER_CLEANUP_FAILED") from worker_error
        paths = [self.out / "cleanup.json"]
        if self.source:
            paths.append(self.source.path / "cleanup.json")
        if any(json.loads(p.read_text()).get("failures") for p in paths):
            raise RuntimeError("CLEANUP_FAILED")
