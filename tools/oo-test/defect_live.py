"""Owned, bounded SQL detectors; no customer connections or external services."""
import json
import os
from pathlib import Path
import subprocess
import sys
from types import SimpleNamespace
import unittest

import audit
import runner
import snapshots
import t1
import t3t4


class LiveChecks(unittest.TestCase):
    def setUp(self):
        self.mode = os.environ["OO_TA_LIVE_ENGINE"]
        self.engine = "postgres" if self.mode == "publication" else self.mode
        self.version = "17" if self.engine == "postgres" else "5.7"
        path = Path(os.environ["OO_TA_LIVE_EVIDENCE"])
        path.mkdir(mode=0o700)
        t1.LANE = "ta"
        snapshots.CACHE = Path(os.environ["OO_TA_SNAPSHOT_CACHE"])
        arch = subprocess.check_output(["docker", "info", "--format", "{{.Architecture}}"],
                                       text=True).strip()
        native = "linux/arm64" if arch in ("arm64", "aarch64") else "linux/amd64"
        item = runner.resolve(self.engine, self.version)
        platform = native if native in item["platforms"] else "linux/amd64"
        args = SimpleNamespace(scale=2000, tables=3, platform=platform, health_timeout=None)
        key, manifest = snapshots.identity(self.engine, self.version, 2000, 3, platform)
        self.lock = snapshots.locked_cache(key)
        cache = self.lock.__enter__()
        self.addCleanup(self.lock.__exit__, None, None, None)
        self.run = snapshots.SnapshotRun.__new__(snapshots.SnapshotRun)
        self.addCleanup(self.cleanup)
        self.run.__init__(path, self.engine, self.version, args, cache, key, manifest)
        runner.write_json(path / "gate-owner.json", {"name": self.run.name, "run_id": self.run.id})
        self.run.setup()
        self.run.seed()

    def cleanup(self):
        if hasattr(self.run, "private"):
            self.assertEqual(self.run.cleanup(), [])

    def test_live_boundary(self):
        run = self.run
        if self.mode == "publication":
            _, observed = run.sql("SHOW server_version;", admin=True)
            self.publication_identity()
        elif self.engine == "postgres":
            code, observed = run.sql("SHOW server_version;", admin=True)
            self.assertEqual(code, 0)
            sql = "SELECT set_config('max_parallel_workers_per_gather','0',false)"
            code, result = run.sql(sql + ";")
            self.assertEqual(code, 0)
            self.assertEqual(result.strip(), "0")
            self.assertEqual(audit.classify_sql(sql, "postgres", "c5")[0], "d12_session_state")
        else:
            _, observed = run.sql("SELECT VERSION();", admin=True)
            h = SimpleNamespace(run=SimpleNamespace(engine="mysql", version=observed.strip()))
            run.sql("CREATE TABLE t4_rename_column "
                    "(id bigint PRIMARY KEY, payload varchar(80) NOT NULL, n int NOT NULL);"
                    "INSERT INTO t4_rename_column VALUES (1,'boundary',1);", admin=True)
            statements, _ = t3t4.ddl(h, "rename_column")
            code, _ = run.sql("\n".join(statements), admin=True, required=False)
            if code and "ERROR 1064" not in (run.path / f"{run.seq:04d}.stderr").read_text():
                raise RuntimeError("rename_infrastructure_failure")
            self.assertEqual(code, 0, "MySQL 5.7 patch-version RENAME must use CHANGE")
            _, result = run.sql("SELECT label FROM t4_rename_column;", admin=True)
            self.assertEqual(result.strip(), "boundary")
            before = audit.Audit(run).privileges()
            self.assertTrue(before["read_only"])
            run.sql("GRANT INSERT ON fixture.* TO 'fixture_reader'@'127.0.0.1';", admin=True)
            after = audit.Audit(run).privileges()
            runner.write_json(run.path / "privilege-check.json", {"before": before, "after": after})
            self.assertFalse(after["read_only"],
                             "INSERT must not pass the source audit")
        runner.write_json(run.path / "live-check.json",
                          {"engine": self.engine, "observed_version": observed.strip(),
                           "status": "passed"})

    def publication_identity(self):
        sql = lambda text: self.run.sql(text, admin=True)[1].strip()
        sql("CREATE ROLE oo_target_admin NOLOGIN; CREATE SCHEMA _oo;"
            "CREATE SCHEMA oo_publish_target;"
            "CREATE TABLE oo_publish_target.fence(role_name text, schema_name text,"
            "relation_name text,state text,opened_version text,opened_pack_digest text);"
            "CREATE FUNCTION _oo.ta_value(bigint) RETURNS bigint LANGUAGE sql IMMUTABLE "
            "AS 'SELECT $1 + 1';"
            "CREATE VIEW _oo.ta_view AS SELECT _oo.ta_value(1) AS value;")
        migrations = runner.ROOT / "crates/oo-pipeline/migrations"
        sql((migrations / "0135_logical_publication_identity.sql").read_text())
        sql("INSERT INTO oo_publish_target.fence VALUES"
            "('reader','_oo','ta_view','open','generation','publication');")
        self.assertEqual(sql("SELECT valid FROM _oo.publication_identity"), "t")
        sql((migrations / "0136_publication_routine_identity.sql").read_text())
        self.assertEqual(sql("SELECT valid FROM _oo.publication_identity"), "t",
                         "0136 must preserve an identity current under 0135")
        before = sql("SELECT _oo.publication_relation_digest('_oo','ta_view')")
        sql("CREATE OR REPLACE FUNCTION _oo.ta_value(bigint) RETURNS bigint "
            "LANGUAGE sql IMMUTABLE AS 'SELECT $1 + 2';")
        self.assertNotEqual(sql("SELECT _oo.publication_relation_digest('_oo','ta_view')"),
                            before, "0136 must bind routine definitions")
        self.assertEqual(sql("SELECT valid FROM _oo.publication_identity"), "f",
                         "0135 must tombstone changed identities")
        sql("CREATE OR REPLACE FUNCTION _oo.ta_value(bigint) RETURNS bigint "
            "LANGUAGE sql IMMUTABLE AS 'SELECT $1 + 1';")
        self.assertEqual(sql("SELECT valid FROM _oo.publication_identity"), "f",
                         "repair must not silently re-approve a publication")


if __name__ == "__main__":
    os.environ["OO_TA_LIVE_ENGINE"] = sys.argv.pop(1)
    unittest.main(verbosity=2)
