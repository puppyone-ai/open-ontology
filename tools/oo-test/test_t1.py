import unittest
import datetime as dt
from decimal import Decimal
import struct
import io
import json
import tempfile
from pathlib import Path
from unittest.mock import MagicMock, patch

import t1
import recompare_t1


class T1Tests(unittest.TestCase):
    def test_concurrent_snapshot_verdict_uses_one_c5_cycle_without_reload(self):
        root = t1.ROOT / "evidence"
        root.mkdir(exist_ok=True)
        for correction_fails in (False, True):
            with self.subTest(correction_fails=correction_fails), tempfile.TemporaryDirectory(dir=root) as tmp:
                run = MagicMock(engine="postgres", reader_secret="fixture-only")
                run.path = Path(tmp)
                binary = run.path / "oo-load"
                binary.write_bytes(b"test binary")
                run.sql.return_value = (0, "events\n")
                first = {"table": {"name": "events"}, "target": "oo_load_data.loaded",
                         "read_budget": {"duty_cycle_percent": 25},
                         "target_write_bytes": 100, "replay": False}
                corrected = dict(first, protocol="oo.sync/v1", mode="c5", target_write_bytes=10)
                calls = []
                def command(argv, **kwargs):
                    if argv[:2] == ["docker", "port"]:
                        return 0, "127.0.0.1:5432"
                    calls.append(argv)
                    config = json.loads((run.path / ".load-private/target.json").read_text())
                    if len(calls) == 1:
                        self.assertEqual(config["run_id"], "concurrent")
                        return 0, json.dumps(first)
                    if len(calls) == 2:
                        self.assertEqual(config["run_id"], "concurrent")
                        return 0, json.dumps(dict(first, replay=True))
                    self.assertEqual(argv[1], "sync")
                    self.assertEqual(argv[argv.index("--run-id")+1], "corrected")
                    self.assertEqual(json.loads(Path(argv[argv.index("--c5")+1]).read_text()),
                                     {"cycle": 1, "full_read_every": 2})
                    return (1, "") if correction_fails else (0, json.dumps(corrected))
                run.command.side_effect = command
                target = MagicMock(created=False, port=5433, password="fixture-only")
                meter = MagicMock(duty=25)
                meter.__enter__.return_value = meter
                meter.result.return_value = {"status": "passed"}
                def writer(source, stop, result):
                    result["cycles"] = 1
                with patch.object(t1, "AgentDB", return_value=target), \
                     patch.object(t1, "configure_pg"), patch.object(t1, "writer", writer), \
                     patch.object(t1, "SourceMeter", return_value=meter), \
                     patch.object(t1, "write_rows"), \
                     patch.object(t1, "compare_pg", return_value={"equal": True}) as comparator:
                    report = t1.run_t1(run, binary)
                self.assertEqual(len(calls), 3)
                self.assertNotIn("sync", calls[0])
                self.assertNotIn("sync", calls[1])
                self.assertEqual(report["T1"], "failed" if correction_fails else "passed")
                if correction_fails:
                    comparator.assert_not_called()
                else:
                    comparator.assert_called_once_with(run, target, corrected, "events")

    def test_loaded_chunk_drop_and_duplicate_fail_comparison(self):
        class Source:
            def sql(self, query):
                if "information_schema" in query:
                    return 0, "id|bigint\namount|numeric\n"
                return 0, "\n".join(json.dumps(row) for row in original)
        class LoadedChunk:
            def __init__(self, rows):
                self.rows = rows
            def sql(self, query):
                return "\n".join(json.dumps(row) for row in self.rows)
        original = [{"id": n, "amount": n} for n in range(1, 5)]
        receipt = {"target": "oo_load_data.loaded_chunk"}
        self.assertTrue(t1.compare_pg(Source(), LoadedChunk(original), receipt, "events")["equal"])
        # Include a count-preserving corruption: row counts alone cannot catch it.
        for rows in (original[:-1], original + [original[-1]],
                     original[:2] + [original[1]] + original[3:]):
            with self.subTest(rows=rows):
                result = t1.compare_pg(Source(), LoadedChunk(rows), receipt, "events")
                self.assertFalse(result["equal"])
                self.assertIsNotNone(result["first_difference"])
                self.assertNotEqual(*result["chunks"][0]["sha256"])

    def test_readonly_exec_retries_only_predispatch_transport_refusal(self):
        with tempfile.TemporaryDirectory(dir=t1.ROOT / "evidence") as temporary:
            run = t1.LaneRun.__new__(t1.LaneRun)
            run.path, run.seq = Path(temporary), 1
            refusal = "Cannot connect to the Docker daemon at fixture. Is the docker daemon running?"
            unix_refusal = ("cannot connect to the Docker daemon. Is 'docker daemon' running on this host?: "
                            "dial unix /fixture/docker.sock: connect: connection refused\n")
            for query, error, output, expected in [
                ("SELECT 1;", refusal, "", 2),
                ("SELECT 1;", unix_refusal, "", 2),
                ("SELECT 1;", unix_refusal, "partial result", 1),
                ("DELETE FROM growth_events;", unix_refusal, "", 1),
                ("SELECT 1;", unix_refusal.replace("connection refused", "permission denied"), "", 1),
                ("SELECT 1;", "ERROR: permission denied", "", 1),
                ("SELECT 1;", refusal, "partial result", 1),
                ("DELETE FROM growth_events;", refusal, "", 1),
            ]:
                (run.path / "0001.stderr").write_text(error)
                (run.path / "0001.stdout").write_text(output)
                with patch.object(t1.runner.Run, "command", side_effect=[RuntimeError("failed"), (0, "1")]) as call:
                    with patch.object(t1.time, "sleep"):
                        if expected == 2:
                            self.assertEqual(run.command(["docker", "exec", "owned"], data=query), (0, "1"))
                        else:
                            with self.assertRaises(RuntimeError):
                                run.command(["docker", "exec", "owned"], data=query)
                self.assertEqual(call.call_count, expected)
            (run.path / "0001.stdout").write_text("")
            with patch.object(t1.runner.Run, "command", side_effect=RuntimeError("failed")) as call:
                with patch.object(t1.time, "sleep"), self.assertRaises(RuntimeError):
                    run.command(["docker", "exec", "owned"], data="SELECT 1;")
            self.assertEqual(call.call_count, 3)

    def test_source_meter_fails_closed_and_enforces_aggregate_duty(self):
        class Run:
            engine = "mysql"
        meter = t1.SourceMeter(Run(), 0, 1)
        meter.started, meter.ended = 0, 10
        meter.samples = [(0, 1, 1), (2, 1, 0), (9, 1, 0)]
        self.assertEqual(meter.result()["status"], "passed")
        self.assertEqual(meter.result()["aggregate_duty_percent"], 20)
        meter.samples[1] = (3, 1, 0)
        self.assertEqual(meter.result()["status"], "failed")
        meter.samples = [(0, 2, 0), (9, 1, 0)]
        self.assertEqual(meter.result()["status"], "failed")
        meter.samples = [(0, 1, 0), (9, 1, 0)]
        meter.error = "query_failed"
        self.assertEqual(meter.result()["status"], "failed")
        meter.error, meter.samples = None, []
        self.assertEqual(meter.result()["status"], "failed")

    def test_source_meter_excludes_query_finishing_after_load(self):
        class Run:
            engine = "postgres"
        meter = t1.SourceMeter(Run(), 0, 2)
        meter.started, meter.ended = 0, 10
        meter.samples = [(1, 1, 0), (9, 1, 0), (11, 9, 9)]
        result = meter.result()
        self.assertEqual(result["status"], "passed")
        self.assertEqual(result["sample_count"], 2)
        self.assertEqual(result["connections_peak"], 1)
        self.assertEqual(result["aggregate_duty_percent"], 0)

    def test_postgres_meter_uses_one_session_and_fails_on_disconnect(self):
        class Child:
            def __init__(self, text):
                self.stdin = io.StringIO()
                self.stdout = io.StringIO(text)
            def wait(self, timeout=None):
                return 0
        class Stop:
            stopped = False
            def is_set(self):
                return self.stopped
            def wait(self, timeout):
                self.stopped = True
        class Run:
            engine, name = "postgres", "owned-fixture"
        with tempfile.TemporaryDirectory(dir=t1.ROOT / "evidence") as temporary:
            run = Run()
            run.path = Path(temporary)
            for text in ("2|1\n", ""):
                meter = t1.SourceMeter(run, 0, 2)
                meter.stop = Stop()
                with patch.object(t1.subprocess, "Popen", return_value=Child(text)) as launch:
                    meter.sample()
                self.assertEqual(launch.call_count, 1)
                self.assertEqual(len(meter.samples), int(bool(text)))
                self.assertEqual(meter.error is not None, not bool(text))

    def test_mysql_cells_preserve_unsigned_decimal_binary_and_utc(self):
        self.assertEqual(t1.mysql_cell("18446744073709551615", "bigint"), Decimal(2**64-1))
        self.assertEqual(t1.mysql_cell("12345678901234567890.1234567890", "decimal"),
                         Decimal("12345678901234567890.1234567890"))
        self.assertEqual(t1.mysql_cell("\\x0001ff", "longblob"), b"\x00\x01\xff")
        self.assertEqual(t1.mysql_cell("2026-01-01 00:00:00.123456", "timestamp").tzinfo,
                         dt.timezone.utc)

    def test_mysql_comparison_keeps_null_and_empty_rows_in_source_order(self):
        class Source:
            def sql(self, sql):
                if "information_schema.columns" in sql:
                    return 0, "value_no\tbigint\nnote\tvarchar\n"
                return 0, "31|-\n31|\n"
        class Target:
            def sql(self, sql):
                self.query = sql
                return '{"value_no":1,"note":null}\n{"value_no":1,"note":""}\n'
        target = Target()
        result = t1.compare_mysql(Source(), target, {"target": "synthetic"}, "no_key")
        self.assertTrue(result["equal"])
        self.assertIn('"note" NULLS FIRST', target.query)
        self.assertEqual(result["chunks"][0]["rows"], [2, 2])

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
            name, engine = "owned-fixture", "postgres"
        root = t1.ROOT / "evidence"
        root.mkdir(exist_ok=True)
        for engine, acknowledged in (("postgres", True), ("postgres", False),
                                     ("mysql", True), ("mysql", False)):
            with tempfile.TemporaryDirectory(dir=root) as temporary:
                source = Source()
                source.engine = engine
                source.path = Path(temporary)
                child = Child("INSERT 0 1\nOO_CYCLE_0\n" if acknowledged else "")
                result = {}
                with patch.object(t1.subprocess, "Popen", return_value=child) as launch:
                    t1.writer(source, Stop(), result)
                self.assertEqual(launch.call_count, 1)
                if engine == "mysql":
                    self.assertIn("--skip-reconnect", launch.call_args.args[0])
                    self.assertIn("--unbuffered", launch.call_args.args[0])
                self.assertIn("UPDATE growth_events SET id=", child.stdin.getvalue())
                self.assertIn("DELETE FROM growth_events", child.stdin.getvalue())
                self.assertIn("COMMIT;\nSELECT 'OO_CYCLE_0'", child.stdin.getvalue())
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
