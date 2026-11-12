from pathlib import Path
import tempfile
import unittest

import runner
import shared


class FakeRun:
    """Records SQL and models slots, publications and one mutable row."""
    def __init__(self, engine, path):
        self.engine, self.path, self.name = engine, path, "oo-ta-x"
        self.slots, self.data, self.pristine = [], "gold", None
        self.calls = []

    def gold(self):
        return {"Q1": self.data}

    def command(self, command, data=None, required=True):
        self.calls.append(data)
        if "pg_replication_slots ORDER" in data:
            return 0, "\n".join(self.slots)
        if ("pg_publication" in data or "pg_current_wal_lsn" in data
                or "pg_file_settings" in data or "pending_restart" in data):
            return 0, ""
        if "pg_drop_replication_slot" in data:
            self.slots.remove(data.split("'")[1])
        if f"TEMPLATE fixture;" in data:
            self.pristine = self.data
        if f"TEMPLATE {shared.PRISTINE}" in data:
            self.data = self.pristine
        return 0, ""

    def sql(self, text, admin=False, required=True):
        self.calls.append(text)
        if "information_schema.views" in text:
            return 0, ""
        if "information_schema.tables" in text:
            return 0, "customers\norders"
        if "SHOW MASTER STATUS" in text:
            return 0, "binlog.000002\t157"
        if f"CREATE DATABASE IF NOT EXISTS {shared.PRISTINE}" in text:
            self.pristine = self.data
        if "CREATE DATABASE IF NOT EXISTS fixture;" in text:
            self.data = self.pristine
        return 0, ""


class SharedSourceTests(unittest.TestCase):
    def setUp(self):
        root = runner.ROOT / "evidence"
        root.mkdir(exist_ok=True)
        self.directory = tempfile.TemporaryDirectory(dir=root)
        self.path = Path(self.directory.name)

    def tearDown(self):
        self.directory.cleanup()

    def test_destructive_suites_never_lease_the_shared_source(self):
        source = shared.SharedSource(FakeRun("postgres", self.path))
        source.prepare()
        with self.assertRaisesRegex(ValueError, "own_clone"):
            with source.lease("T3"):
                pass
        self.assertTrue(shared.needs_own_clone("T3"))
        self.assertFalse(any(shared.needs_own_clone(s) for s in shared.SHARED_SUITES))
        with self.assertRaises(ValueError):
            shared.SharedSource(FakeRun("oracle", self.path))

    def test_pg_lease_drops_new_slots_and_restores_pristine_data(self):
        run = FakeRun("postgres", self.path)
        source = shared.SharedSource(run)
        with self.assertRaisesRegex(RuntimeError, "not_prepared"):
            with source.lease("T1"):
                pass
        source.prepare()
        with source.lease("T1") as lease:
            self.assertEqual(lease["slot_prefix"], "oo_t1_")
            run.slots.append("oo_t1_capture")
            run.data = "mutated"
        self.assertEqual(run.slots, [])
        self.assertEqual(run.data, "gold")
        self.assertEqual(source.leases[0]["removed"]["slots"], ["oo_t1_capture"])
        self.assertTrue(any("GRANT CONNECT ON DATABASE fixture" in (c or "") for c in run.calls))
        self.assertTrue((self.path / "shared-leases.json").exists())

    def test_reset_fails_closed_when_gold_drifts(self):
        run = FakeRun("mysql", self.path)
        source = shared.SharedSource(run)
        source.prepare()
        with source.lease("T4"):
            run.data = "mutated"
        self.assertEqual(run.data, "gold")
        run.pristine = "corrupted"
        with self.assertRaisesRegex(RuntimeError, "changed_gold"):
            with source.lease("T2"):
                pass

    def test_mysql_copy_rejects_unexpected_table_names(self):
        run = FakeRun("mysql", self.path)
        run.sql = lambda text, admin=False, required=True: (0, "ok\nbad`name")
        with self.assertRaisesRegex(ValueError, "table_name"):
            shared.SharedSource(run).mysql_tables("fixture")


if __name__ == "__main__":
    unittest.main()
