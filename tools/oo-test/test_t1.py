import unittest
import datetime as dt
from decimal import Decimal
import struct
import io
import tempfile
from pathlib import Path
from unittest.mock import patch

import t1
import recompare_t1


class T1Tests(unittest.TestCase):
    def test_persistent_writer_requires_a_commit_acknowledgement(self):
        class Input(io.StringIO):
            def close(self):
                pass
        class Child:
            def __init__(self, output):
                self.stdin, self.stdout = Input(), io.StringIO(output)
                self.returncode = None
            def wait(self, timeout=None):
                if self.returncode is None:
                    self.returncode = 0
                return self.returncode
            def poll(self):
                return self.returncode
            def kill(self):
                self.returncode = -9
        class Stop:
            stopped = False
            def is_set(self):
                return self.stopped
            def wait(self, timeout):
                self.stopped = True
        class Source:
            name = "owned-fixture"
        root = t1.ROOT / "evidence"
        root.mkdir(exist_ok=True)
        for acknowledged in (True, False):
            with tempfile.TemporaryDirectory(dir=root) as temporary:
                source = Source()
                source.path = Path(temporary)
                child = Child("INSERT 0 1\nOO_CYCLE_0\n" if acknowledged else "")
                result = {}
                with patch.object(t1.subprocess, "Popen", return_value=child) as launch:
                    t1.postgres_writer(source, Stop(), result)
                self.assertEqual(launch.call_count, 1)
                self.assertIn("UPDATE growth_events SET id=", child.stdin.getvalue())
                self.assertIn("DELETE FROM growth_events", child.stdin.getvalue())
                self.assertEqual(result["cycles"], int(acknowledged))
                self.assertEqual("error" in result, not acknowledged)

    def test_archive_reaudit_is_typed_and_append_only(self):
        root = t1.ROOT / "evidence"
        root.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=root) as temporary:
            path = Path(temporary)
            t1.runner.write_json(path / "t1-t8.json", {"T1": "passed", "tables": [{"table": "events"}]})
            t1.runner.write_json(path / "corrected-receipts.json", [
                {"table": {"name": "events"}, "target": "oo_load_data.synthetic"}])
            (path / "SHA256SUMS").write_text("original evidence manifest\n")
            for n in range(1, 7):
                t1.runner.write_json(path / f"{n:04d}.json",
                                     {"exit": 0, "argv": ["oo-load" if n <= 3 else "psql"]})
            (path / "0004.stdout").write_text("id|bigint\n")
            for n in (5, 6):
                (path / f"{n:04d}.stdout").write_text('{"id":1}\n')
            result = recompare_t1.recompare(path)
            self.assertEqual(result["tables"], [{"table": "events", "rows": 1, "equal": True}])
            self.assertEqual((path / "SHA256SUMS").read_text(), "original evidence manifest\n")
            with self.assertRaises(FileExistsError):
                recompare_t1.recompare(path)

    def test_database_json_is_restored_to_typed_oracle_inputs(self):
        self.assertEqual(t1.pg_cell("2026-01-02", "date"), dt.date(2026, 1, 2))
        value = t1.pg_cell("2026-01-02T03:04:05.123456+08:00", "timestamp with time zone")
        self.assertEqual(value.utcoffset(), dt.timedelta(hours=8))
        self.assertEqual(value.microsecond, 123456)
        self.assertEqual(t1.pg_cell("\\x00ff", "bytea"), b"\x00\xff")
        self.assertEqual(struct.pack(">d", t1.pg_cell(Decimal("-0"), "double precision")).hex(),
                         "8000000000000000")
        with self.assertRaises(RuntimeError):
            t1.pg_cell("NaN", "double precision")

    def test_only_source_create_gets_a_source_port_and_all_owned_labels_are_m(self):
        run = t1.LaneRun.__new__(t1.LaneRun)
        run.name, run.engine = "owned-source", "oracle"
        with patch.object(t1.runner.Run, "command", return_value=(0, "")) as command:
            run.command(["docker", "create", "--name", "owned-source", "--label", "oo.lane=h", "image"])
            self.assertEqual(command.call_args.args[0][2:4], ["-p", "127.0.0.1::1521"])
            self.assertIn("oo.lane=m", command.call_args.args[0])
            target = ["docker", "create", "--name", "owned-target", "-p", "127.0.0.1::5432", "image"]
            run.command(target)
            self.assertEqual(command.call_args.args[0], target)

    def test_pg_comparison_does_not_hide_changed_rows_or_keys(self):
        class Source:
            def sql(self, query):
                if "information_schema" in query:
                    return 0, "id|bigint\namount|numeric\n"
                return 0, '{"id":1,"amount":1.000}\n{"id":2,"amount":2.125}\n'
        class Target:
            def __init__(self, last):
                self.last = last
            def sql(self, query):
                assert "ORDER BY" in query
                return '{"id":1,"amount":1.0}\n' + self.last
        receipt = {"target": "oo_load_data.synthetic"}
        good = t1.compare_pg(Source(), Target('{"id":2,"amount":2.125}\n'), receipt, "orders")
        self.assertTrue(good["equal"])
        bad = t1.compare_pg(Source(), Target('{"id":3,"amount":2.125}\n'), receipt, "orders")
        self.assertFalse(bad["equal"])
        self.assertEqual(bad["first_difference"]["row_index"], 1)
        self.assertNotEqual(*bad["chunks"][0]["sha256"])

    def test_oracle_numeric_and_empty_semantics_remain_explicit(self):
        class Source:
            def sql(self, query):
                if "all_tab_columns" in query:
                    return 0, "ID|NUMBER\nNOTE|VARCHAR2\n"
                return 0, "31|-\n32|20\n"
        class Target:
            def sql(self, query):
                assert '"ID"::numeric' in query
                return '{"ID":"1","NOTE":null}\n{"ID":"2","NOTE":" "}\n'
        result = t1.compare_oracle(Source(), Target(), {"target": "oo_load_data.synthetic"}, "CUSTOMERS")
        self.assertTrue(result["equal"])
        self.assertTrue(result["oracle_empty_is_null"])

    def test_failed_writer_is_reported_not_passed(self):
        class Source:
            seq = 0
            engine = "oracle"
            def sql(self, *args, **kwargs):
                raise RuntimeError("synthetic")
        result = {}
        t1.writer(Source(), t1.threading.Event(), result)
        self.assertEqual(result, {"cycles": 0, "error": "RuntimeError"})


if __name__ == "__main__":
    unittest.main()
