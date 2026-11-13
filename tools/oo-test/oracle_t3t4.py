"""Oracle dialect/transport for the shared T3/T4 state machine; owned fixtures only."""
import hashlib
import json
import re
import time
import uuid

import runner
import t3t4
from t1 import private


def controlfile_script(trace):
    match = re.search(r"CREATE CONTROLFILE\b.*?;", trace, re.S)
    if not match or not re.search(r"\bRESETLOGS\b", match[0]):
        raise RuntimeError("resetlogs_controlfile_trace_missing")
    return match[0]


def ddl(case):
    table = "t4_" + case
    columns = ("id", "payload", "n")
    if case == "add_column":
        return [f"ALTER TABLE {table} ADD extra VARCHAR2(40) DEFAULT 'd' NOT NULL;"], (*columns, "extra")
    if case == "drop_column":
        return [f"ALTER TABLE {table} DROP COLUMN n;"], columns[:2]
    if case == "rename_column":
        return [f"ALTER TABLE {table} RENAME COLUMN payload TO label;"], ("id", "label", "n")
    if case == "widen_string":
        return [f"ALTER TABLE {table} MODIFY (payload VARCHAR2(160));"], columns
    if case == "widen_type":
        return [f"ALTER TABLE {table} MODIFY (n NUMBER(18),payload VARCHAR2(160));"], columns
    if case == "add_table":
        return [f"CREATE TABLE {table}_new(id NUMBER(18) PRIMARY KEY,payload VARCHAR2(80) NOT NULL,n NUMBER(10) NOT NULL);",
                f"GRANT SELECT,FLASHBACK ON {table}_new TO fixture_reader;",
                f"ALTER TABLE {table}_new ADD SUPPLEMENTAL LOG DATA (ALL) COLUMNS;",
                f"INSERT INTO {table}_new VALUES(1,'new-1',1);",
                f"INSERT INTO {table}_new VALUES(2,'new-2',2);"], columns
    if case == "drop_table":
        return [f"DROP TABLE {table};"], columns
    if case == "pk_drop":
        return ["ALTER TABLE t4_pk_change DROP PRIMARY KEY;"], columns
    if case == "pk_add":
        return ["ALTER TABLE t4_pk_change ADD PRIMARY KEY(id,payload);"], columns
    raise ValueError(case)


class OracleHarness(t3t4.Harness):
    def __init__(self, run, binary):
        super().__init__(run, binary)
        self.schema = "FIXTURE"
        self.service = {"19": "FREEPDB1", "23": "FREEPDB1", "21": "XEPDB1", "11": "XE"}[run.version]
        self.paths = ("c5", "c1") if run.version in ("19", "23") else ("c5",)
        self.worker = None
        self.worker_args = []

    def root_sql(self, sql):
        return self.run.command(["docker", "exec", "-i", self.run.name, "sqlplus", "-s", "/nolog"],
                                data="whenever sqlerror exit failure rollback\nconnect / as sysdba\n" + sql + "\nexit\n",
                                timeout=600)

    def src(self, sql, required=True):
        # Each admin DML statement commits; never sent through the product reader.
        return self.run.sql(sql.replace(";", ";\n") + "\nCOMMIT;", admin=True, required=required)

    def wait_source(self):
        for _ in range(180):
            # SYS can answer dual before the restarted PDB/reader is usable.
            if self.run.sql("SELECT id FROM FIXTURE.bulk WHERE id=1;", required=False)[0] == 0:
                return
            time.sleep(1)
        raise RuntimeError("source readiness")

    def heal(self):
        if self.disconnected:
            self.run.command(["docker", "network", "connect", "bridge", self.run.name])
            self.disconnected = False
        for name in (self.run.name, self.target.name):
            _, state = self.run.command(["docker", "inspect", "--format", "{{.State.Status}}", name])
            if state.strip() != "running":
                self.run.command(["docker", "start", name])
        self.wait_source()
        self.target.ready()

    def prepare(self):
        run = self.run
        run.setup()
        pdb = "" if run.version == "11" else f"ALTER SESSION SET CONTAINER={self.service};\n"
        self.root_sql(pdb + f'CREATE USER fixture IDENTIFIED BY "{run.secret}";\n'
                      "GRANT CREATE SESSION,RESOURCE,UNLIMITED TABLESPACE TO fixture;\n"
                      f'CREATE USER fixture_reader IDENTIFIED BY "{run.reader_secret}";\n'
                      "GRANT CREATE SESSION TO fixture_reader;\n")
        for view in ("V_$DATABASE", "V_$SESSION", "V_$SQL_PLAN", "DBA_EXTENTS"):
            self.src(f"GRANT SELECT ON SYS.{view} TO fixture_reader;")
        self.src("GRANT EXECUTE ON SYS.DBMS_ROWID TO fixture_reader;")
        tables = ["bulk", "items", "other"] + ["t4_" + c for c in t3t4.T4] + ["t4_drop_table_b"]
        for table in tables:
            self.src(f"CREATE TABLE {table}(id NUMBER(18) PRIMARY KEY,payload VARCHAR2(80) NOT NULL,n NUMBER(10) NOT NULL);"
                     f"INSERT INTO {table} SELECT level,'base-'||TO_CHAR(level),level FROM dual "
                     f"CONNECT BY level <= {t3t4.BULK_ROWS if table == 'bulk' else 20};"
                     f"GRANT SELECT,FLASHBACK ON {table} TO fixture_reader;")
        if run.version in ("19", "23"):
            self.root_sql("SHUTDOWN IMMEDIATE;\nSTARTUP MOUNT;\nALTER DATABASE ARCHIVELOG;\n"
                          "ALTER DATABASE OPEN;\nALTER PLUGGABLE DATABASE ALL OPEN;\n"
                          "ALTER DATABASE ADD SUPPLEMENTAL LOG DATA;\n"
                          f'CREATE USER C##OO_MINER IDENTIFIED BY "{run.reader_secret}" CONTAINER=ALL;\n'
                          "GRANT CREATE SESSION,LOGMINING,SELECT ANY TRANSACTION TO C##OO_MINER CONTAINER=ALL;\n"
                          "GRANT EXECUTE ON SYS.DBMS_LOGMNR TO C##OO_MINER CONTAINER=ALL;\n" +
                          "\n".join(f"GRANT SELECT ON SYS.{v} TO C##OO_MINER CONTAINER=ALL;" for v in
                                    ("V_$DATABASE", "V_$ARCHIVED_LOG", "V_$LOG", "V_$LOGFILE",
                                     "V_$LOGMNR_CONTENTS", "V_$LOGMNR_LOGS", "V_$SESSION", "V_$TRANSACTION")))
            for table in tables:
                self.src(f"ALTER TABLE {table} ADD SUPPLEMENTAL LOG DATA (ALL) COLUMNS;")
        private(self.store / "source-password", run.reader_secret)
        if run.version == "11":
            if not run.args.worker_image:
                raise RuntimeError("11g thick worker image required")
            _, image = run.command(["docker", "image", "inspect", run.args.worker_image, "--format", "{{.Id}}"])
            self.worker = "oo-e-t3-worker-" + uuid.uuid4().hex[:12]
            run.command(["docker", "create", "--name", self.worker, "--platform", "linux/amd64",
                         "--label", "oo.lane=" + t3t4.t1.LANE, "--label", "oo.run_id=" + run.id,
                         "--mount", f"type=bind,src={runner.ROOT / 'testing/engines/oracle/worker/worker.py'},dst=/worker.py,readonly",
                         "--entrypoint", "sleep", image.strip(), "infinity"])
            run.command(["docker", "start", self.worker])
            executable = self.store / "worker"
            private(executable, f"#!/bin/sh\nexec docker exec -i {self.worker} python /worker.py\n")
            executable.chmod(0o700)
            self.worker_args = ["--oracle-worker", str(executable)]
        if not t3t4.disk_ready(20, 0):
            raise RuntimeError("host_disk_below_floor_before_target")
        self.target.start()
        private(self.store / "target-password", self.target.password)

    def close_worker(self):
        if self.worker:
            _, raw = self.run.command(["docker", "inspect", "--format", "{{json .Config.Labels}}", self.worker])
            labels = json.loads(raw)
            if labels.get("oo.lane") != t3t4.t1.LANE or labels.get("oo.run_id") != self.run.id:
                raise RuntimeError("worker cleanup ownership")
            self.run.command(["docker", "rm", "-f", self.worker])
            self.worker = None

    def add_source(self, sid, tables, chunk_rows=None):
        super().add_source(sid, tables, chunk_rows)
        source = self.stack["sources"][-1]
        source.pop("tls_ca")
        source["database"] = self.service
        source["scope"]["include"] = [f"FIXTURE.{t.upper()}" for t in tables]
        if self.worker:
            source["host"] = "host.docker.internal"

    def tables(self, sid):
        return [t.lower() for t in super().tables(sid)]

    def argv(self, *args, **kwargs):
        return super().argv(*args, **kwargs) + self.worker_args

    def connect_diagnostics(self, seq):
        _, text = self.src("SELECT username||'|'||status||'|'||event FROM v$session WHERE username IS NOT NULL;",
                           required=False)
        runner.write_json(self.run.path / f"{seq:04d}.connect-diagnostics.json", {"sessions": text})

    def remember(self, sid, receipts):
        super().remember(sid, receipts)
        self.relations = {(s, t.lower()): r for (s, t), r in self.relations.items()}

    @runner.timed("compare")
    def compare(self, sid):
        result = {}
        for table in self.tables(sid):
            _, text = self.src("SELECT column_name||'|'||data_type||'|'||NVL(TO_CHAR(data_precision),'-') "
                               f"FROM all_tab_columns WHERE owner='FIXTURE' AND table_name='{table.upper()}' ORDER BY column_id;")
            cols = [line.strip().split("|") for line in text.splitlines() if "|" in line]
            relation = self.relations.get((sid, table))
            if not cols or not relation:
                result[table] = {"equal": False, "reason": "missing_table_or_target"}
                continue
            expected_types = [(n, "int8" if k == "NUMBER" else "text") for n, k, _ in cols]
            if self.target_columns(relation) != expected_types:
                result[table] = {"equal": False, "reason": "columns"}
                continue
            # Hex framing distinguishes NULL and empty strings, delimiters and whitespace.
            expressions = []
            for name, kind, _ in cols:
                cell = f"TO_CHAR({name},'TM9')" if kind == "NUMBER" else name
                expressions.append(f"NVL(RAWTOHEX(UTL_I18N.STRING_TO_RAW({cell},'AL32UTF8')),'-')")
            projection = "||'|'||".join(expressions)
            _, left = self.src(f"SELECT {projection} FROM {table} ORDER BY id;")
            names = ",".join('"' + n + '"' for n, _, _ in cols)
            _, right = self.tgt(f'SELECT row_to_json(t) FROM (SELECT {names} FROM {relation} ORDER BY "ID") t;')
            left = [line.strip() for line in left.splitlines() if line.strip()]
            right = ["|".join("-" if v is None else str(v).encode().hex().upper() for v in json.loads(line).values())
                     for line in right.splitlines() if line.startswith("{")]
            result[table] = {"equal": left == right, "rows": len(left),
                             "source_sha256": hashlib.sha256(json.dumps(left).encode()).hexdigest(),
                             "target_sha256": hashlib.sha256(json.dumps(right).encode()).hexdigest()}
        return result

    def dml(self, tables, start, count=4, columns=("id", "payload", "n")):
        statements = []
        for table in tables:
            for i in range(count):
                key = start + i
                values = {"id": str(key), "payload": f"'v{key}'", "label": f"'v{key}'", "n": str(key), "extra": f"'x{key}'"}
                statements.append(f"INSERT INTO {table}({','.join(columns)}) VALUES({','.join(values[c] for c in columns)});")
            text = "label" if "label" in columns else "payload"
            statements.append(f"UPDATE {table} SET {text}='upd{start}' WHERE id={start};"
                              f"DELETE FROM {table} WHERE id={start + count - 1};")
        self.src("".join(statements))

    def grow(self):
        _, top = self.src("SELECT TO_CHAR(MAX(id)) FROM bulk;")
        top = int(top.strip())
        self.src(f"DELETE FROM bulk WHERE id>{top-11};")
        for i in range(1, 6):
            self.src(f"INSERT INTO bulk VALUES({top+i},'grown-{top+i}',{i});")

    def c1_binding(self, sid, tables):
        _, epoch = self.src("SELECT TO_CHAR(dbid)||':'||TO_CHAR(resetlogs_change#) FROM v$database;")
        _, scn = self.src("SELECT TO_CHAR(current_scn) FROM v$database;")
        self.snapshot(sid, tables)
        self.sync[sid] = {"initial_position": {"kind": "oracle_scn", "scn": int(scn.strip())},
                          "source_epoch": epoch.strip(), "baseline_aligned": True, "max_batches": 100,
                          "oracle_logminer_opt_in": True, "oracle_retained_floor": int(scn.strip()),
                          "mining": {"service": "FREE", "user": "C##OO_MINER",
                                     "password": {"secret_ref": "source-password"}}}

    def retention_expiry(self):
        if "c1" not in self.paths:
            return {"status": "not_applicable", "reason": "C5 has no redo cursor; C1 outside requested 11g/21c scope"}
        code, _ = self.run.command(["docker", "exec", self.run.name, "sh", "-c",
                                   'test -x "$ORACLE_HOME/bin/rman"'], required=False)
        if code:
            return {"status": "not_run", "reason": "pinned slim image has no RMAN; physical archive expiry not injected"}
        def expire():
            self.root_sql("ALTER SYSTEM ARCHIVE LOG CURRENT;\n" * 4)
            self.run.command(["docker", "exec", "-i", self.run.name, "sh", "-c",
                              'exec "$ORACLE_HOME/bin/rman" target /'],
                             data="DELETE FORCE NOPROMPT ARCHIVELOG ALL;\nexit;\n", timeout=300)
            return {"method": "four log switches then RMAN DELETE FORCE ARCHIVELOG ALL"}
        step = t3t4.refused_then_rebaseline(self, "t3re_c1", "archivelog_expiry", expire)
        # History-loss cases may not pass by merely surviving the attempted fault.
        error = (step["error"] or "").removeprefix("source error: catalog fact=")
        step["passed"] = bool(step["passed"] and step["outcome"] == "named_refusal"
                              and error in ("sync_rebaseline_required_retention",
                                            "logminer_resume_floor_rebaseline",
                                            "logminer_position_expired_rebaseline",
                                            "logminer_log_gap_rebaseline"))
        return t3t4.case_result([step])

    def resetlogs_incarnation(self):
        bindings = {}
        for path in self.paths:
            sid = "t3epoch_" + path
            if path == "c5":
                self.c5_binding(sid, ["items"])
            else:
                self.c1_binding(sid, ["items"])
                self.c1(sid)
            bindings[path] = sid
        _, before = self.src("SELECT TO_CHAR(dbid)||':'||TO_CHAR(resetlogs_change#) FROM v$database;")
        trace = "/tmp/oo-t3-resetlogs.sql"
        self.root_sql(f"ALTER DATABASE BACKUP CONTROLFILE TO TRACE AS '{trace}' REUSE RESETLOGS;")
        _, script = self.run.command(["docker", "exec", self.run.name, "cat", trace])
        # Use Oracle's own file list, only within this run's disposable instance.
        self.root_sql("SHUTDOWN IMMEDIATE;\nSTARTUP NOMOUNT;\n" + controlfile_script(script))
        # Recovery may print ORA-00279/00280 prompts. OPEN RESETLOGS, measured
        # epoch change, and source/target comparison are the acceptance gates.
        self.run.command(["docker", "exec", "-i", self.run.name, "sqlplus", "-s", "/nolog"],
                         data="connect / as sysdba\nRECOVER DATABASE USING BACKUP CONTROLFILE UNTIL CANCEL;\nCANCEL\nexit\n",
                         required=False, timeout=120)
        _, logging = self.root_sql("SET HEADING OFF FEEDBACK OFF PAGESIZE 0\n"
                                  "SELECT log_mode||'|'||supplemental_log_data_min FROM v$database;")
        restore = "ALTER DATABASE ADD SUPPLEMENTAL LOG DATA;\n" if "c1" in self.paths else ""
        self.root_sql("ALTER DATABASE OPEN RESETLOGS;\n" +
                      ("ALTER PLUGGABLE DATABASE ALL OPEN;\n" if self.run.version != "11" else ""))
        steps = []
        if restore:
            # The controlfile recipe clears minimal logging. First prove that a
            # fresh C1 binding refuses this misconfiguration, then repair it as DBA.
            guard_sid = "t3epoch_logging_guard"
            self.c1_binding(guard_sid, ["items"])
            guard = t3t4.attempt(self, guard_sid, "c1")
            guard.update(path="c1", fault="controlfile_cleared_logging")
            guard["passed"] = (guard["outcome"] == "named_refusal"
                               and guard["error"] ==
                               "source error: catalog fact=logminer_logging_prerequisites_rebaseline")
            steps.append(guard)
            self.root_sql(restore)
        _, restored = self.src("SELECT log_mode||'|'||supplemental_log_data_min FROM v$database;")
        runner.write_json(self.run.path / "resetlogs-logging.json",
                          {"before_restore": logging.strip(), "after_restore": restored.strip(),
                           "admin_restore": restore.strip(),
                           "basis": "controlfile trace requires supplemental logging restoration; delayed to test C1 refusal"})
        _, after = self.src("SELECT TO_CHAR(dbid)||':'||TO_CHAR(resetlogs_change#) FROM v$database;")
        if before.strip() == after.strip():
            raise RuntimeError("resetlogs_epoch_did_not_change")
        for path, sid in bindings.items():
            step = t3t4.attempt(self, sid, path)
            step.update(path=path, epoch_before=before.strip(), epoch_after=after.strip())
            g2 = sid + "_g2"
            if path == "c5":
                self.c5_binding(g2, ["items"])
            else:
                self.c1_binding(g2, ["items"])
            self.dml(["items"], self.key())
            rebased = t3t4.attempt(self, g2, path)
            step["rebaseline"] = rebased
            step["passed"] = (step["outcome"] == "named_refusal"
                              and step["error"] in ("c5_rebaseline_required_lineage",
                                                    "sync_rebaseline_required_lineage")
                              and rebased["outcome"] == "online_apply")
            steps.append(step)
        return t3t4.case_result(steps)
