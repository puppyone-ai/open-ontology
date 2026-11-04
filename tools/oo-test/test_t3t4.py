import subprocess
import sys
import types
import unittest
from unittest import mock
from pathlib import Path

import t3t4


class T3T4(unittest.TestCase):
    def test_type_classes_keep_widening_visible(self):
        self.assertEqual(t3t4.type_class("int"), "int4")
        self.assertEqual(t3t4.type_class("integer"), "int4")
        self.assertEqual(t3t4.type_class("bigint"), "int8")
        self.assertEqual(t3t4.type_class("character varying(80)"), "text")
        self.assertNotEqual(t3t4.type_class("int"), t3t4.type_class("bigint"))

    def test_only_closed_codes_count_as_named_refusals(self):
        for error in ("sync_rebaseline_required_retention", "c5_rebaseline_required_lineage",
                      "source error: budget fact=client_statement_timeout",
                      "source error: auth fact=authentication_failed sqlstate=28P01"):
            self.assertTrue(t3t4.named(error), error)
        for error in (None, "command timeout", "source error: protocol",
                      "thread 'main' panicked at src/main.rs", "command 0215 exited 1, sqlplus_error=False"):
            self.assertFalse(t3t4.named(error), error)

    def test_outcomes_never_accept_silent_divergence(self):
        self.assertEqual(t3t4.classify(0, None, True, False), "online_apply")
        self.assertEqual(t3t4.classify(0, None, False, False), "silent_divergence")
        self.assertEqual(t3t4.classify(1, "c5_rebaseline_required_plan", False, True), "named_refusal")
        self.assertEqual(t3t4.classify(1, "c5_rebaseline_required_plan", False, False), "refusal_changed_target")
        self.assertEqual(t3t4.classify(124, "command timeout", False, True), "unnamed_failure")

    def test_positions_must_not_regress(self):
        mysql = [{"file": "oo-bin.000002", "position": 9}, {"file": "oo-bin.000003", "position": 4}]
        self.assertTrue(t3t4.monotonic(mysql))
        self.assertFalse(t3t4.monotonic(mysql[::-1]))
        self.assertTrue(t3t4.monotonic([{"lsn": 1}, {"lsn": 1}, {"lsn": 7}]))
        self.assertFalse(t3t4.monotonic([{"lsn": 7}, {"lsn": 1}]))

    def test_schema_change_statements_follow_engine_version(self):
        def harness(engine, version):
            return types.SimpleNamespace(run=types.SimpleNamespace(engine=engine, version=version))
        self.assertIn("CHANGE COLUMN payload label", t3t4.ddl(harness("mysql", "5.7"), "rename_column")[0][0])
        self.assertIn("RENAME COLUMN payload TO label", t3t4.ddl(harness("mysql", "8.4"), "rename_column")[0][0])
        self.assertIn("ALTER COLUMN n TYPE bigint", t3t4.ddl(harness("postgres", "17"), "widen_type")[0][0])
        self.assertIn("DROP CONSTRAINT t4_pk_change_pkey", t3t4.ddl(harness("postgres", "13"), "pk_drop")[0][0])
        self.assertEqual(t3t4.scope("drop_table", 2), ["t4_drop_table_b"])
        self.assertEqual(t3t4.scope("add_table", 2), ["t4_add_table", "t4_add_table_new"])

    def test_every_required_case_is_registered_and_destructive_cases_run_last(self):
        self.assertEqual(set(t3t4.CASES), set(t3t4.T3) | set(t3t4.T4))
        order = list(t3t4.ORDER)
        for case in ("retention_expiry", "slot_invalidation", "gtid_purge"):
            self.assertGreater(order.index(case), max(order.index(c) for c in t3t4.T4))

    def test_containers_never_start_below_the_disk_floor(self):
        usage = types.SimpleNamespace(free=19 * 1024 ** 3)
        with mock.patch("t3t4.shutil.disk_usage", return_value=usage):
            self.assertFalse(t3t4.disk_ready(20, 0))
            self.assertTrue(t3t4.disk_ready(18, 0))

    def test_invalid_scope_exits_before_creating_resources(self):
        entry = str(Path(__file__).with_name("t3t4.py"))
        for args in (["--engine", "mysql", "--versions", "8.4,8.4"],
                     ["--engine", "postgres", "--versions", "12"],
                     ["--engine", "mysql", "--versions", "8.4", "--cases", "add_column,nope"]):
            result = subprocess.run([sys.executable, entry, *args, "--run-id", "invalid-unit"],
                                    capture_output=True, text=True)
            self.assertEqual(result.returncode, 2, args)


if __name__ == "__main__":
    unittest.main()
