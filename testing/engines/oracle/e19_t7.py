#!/usr/bin/env python3
"""Oracle 19c adapter for lane T's runner, with its audit rules unchanged."""
import hashlib
import json
from pathlib import Path
import sys

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "tools/oo-test"))
import t7
from audit import Audit, ORACLE_MINER


def unwrap_audit_rows(text):
    """Undo SQLPlus's 32767-column wrapping, rejecting any lost SQL text."""
    rows, pending = [], ""
    for line in text.split("\n"):
        if not line and not pending:
            continue
        pending += line
        try:
            row = json.loads(pending, strict=False)
        except json.JSONDecodeError:
            continue
        if row.get("sql_length") is not None:
            if int(row["sql_length"]) != len(row.get("sql") or ""):
                raise ValueError("Oracle audit SQL length mismatch")
        rows.append(pending)
        pending = ""
    if pending:
        raise ValueError("incomplete Oracle audit JSON")
    return "\n".join(rows)


class EnterpriseAudit(Audit):
    def raw(self):
        if self.run.version != "19" or self.mode != "oracle-standard":
            return super().raw()
        command = self.run.command

        def standard_query(argv, **kwargs):
            text = kwargs.get("data", "")
            standard_audit = "FROM sys.aud$ a LEFT JOIN audit_actions" in text
            if standard_audit:
                # The shared common-user query currently assumes unified audit.
                kwargs["data"] = text.replace(
                    f"WHERE dbusername='{ORACLE_MINER}'",
                    f"WHERE a.userid='{ORACLE_MINER}'")
            code, output = command(argv, **kwargs)
            return code, unwrap_audit_rows(output) if standard_audit and code == 0 else output

        self.run.command = standard_query
        try:
            return super().raw()
        finally:
            self.run.command = command

    def enable_miner(self):
        if self.run.version != "19" or self.mode != "oracle-standard":
            return super().enable_miner()
        self.run.command(
            ["docker", "exec", "-i", self.run.name, "sqlplus", "-s", "/nolog"],
            data="whenever sqlerror exit failure\nconnect / as sysdba\n"
                 f"AUDIT ALL STATEMENTS BY {ORACLE_MINER} BY ACCESS;\n"
                 f"AUDIT ALL BY {ORACLE_MINER} BY ACCESS;\n"
                 "AUDIT SELECT TABLE,INSERT TABLE,UPDATE TABLE,DELETE TABLE,"
                 f"EXECUTE PROCEDURE BY {ORACLE_MINER} BY ACCESS;\nexit\n")
        self.users.append(ORACLE_MINER)


shared_coverage = t7.coverage
shared_run_fixture = t7.run_fixture


def coverage(kind, engine, version, workload, windows):
    # Only the coverage dispatch differs; classifications remain lane T's.
    version = "23" if engine == "oracle" and version == "19" else version
    return shared_coverage(kind, engine, version, workload, windows)


def run_fixture(engine, version, kind, root, args):
    report = shared_run_fixture(engine, version, kind, root, args)
    if engine == "oracle" and version == "19" and kind == "incremental":
        # The shared runner's legacy XE annotation excludes all non-23 versions.
        # Correct metadata only; retain its actual C1 verdict and rejection reasons.
        report.pop("not_run", None)
        out = root / "t7" / "oracle-19"
        path = out / "t7-incremental.json"
        t7.runner.write_json(path, report)
        index = out / "index-rows.json"
        rows = json.loads(index.read_text())
        for row in rows:
            if row["evidence"] == path.name:
                row["sha256"] = hashlib.sha256(path.read_bytes()).hexdigest()
        t7.runner.write_json(index, rows)
        t7.runner.checksums(out)
    return report


def main():
    t7.LANE = "E19"
    t7.VERSIONS["oracle"] = ["19"]
    t7.Audit = EnterpriseAudit
    t7.coverage = coverage
    t7.run_fixture = run_fixture
    t7.SOURCES["e19_t7.py"] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    return t7.main()

if __name__ == "__main__":
    raise SystemExit(main())
