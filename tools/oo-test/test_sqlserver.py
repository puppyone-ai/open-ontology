import unittest
import tempfile
import base64
import copy
from pathlib import Path
from unittest.mock import patch

import matrix
import runner
import sqlserver_seed
from sqlserver_acceptance import fingerprint
from sqlserver_audit import attribute_cdc, cdc_state, classify
from sqlserver_cycles import drain_cdc
from sqlserver_runner import SqlServerRun


class SqlServerTests(unittest.TestCase):
    def test_pins_are_one_immutable_amd64_patch_per_major(self):
        for version in ("2019", "2022", "2025"):
            pin = runner.resolve("sqlserver", version)
            self.assertEqual(set(pin["platforms"]), {"linux/amd64"})
            self.assertRegex(pin["platforms"]["linux/amd64"]["digest"], r"^sha256:[a-f0-9]{64}$")
            self.assertRegex(pin["product_version"], r"^(15|16|17)\.\d+\.\d+\.\d+$")
            self.assertEqual(matrix.target_id("sqlserver", version), "sqlserver-selfmanaged")
        with self.assertRaises(ValueError):
            runner.resolve("sqlserver", "202")

    def test_neutral_generator_shapes_and_sqlserver_dialect(self):
        sql = "".join(sqlserver_seed.generate(runner.generator, 2_000_000, 200))
        self.assertIn("WHERE n<=2000000", sql)
        self.assertEqual(sql.count("CREATE TABLE small_"), 200)
        self.assertIn("c299 BIGINT", sql)
        self.assertIn("NVARCHAR(100)", sql)
        self.assertIn("IDENTITY(1000,1)", sql)
        self.assertIn("SET IDENTITY_INSERT regions ON", sql)
        self.assertIn("GO\nCREATE VIEW", sql)
        for unsupported in ("AUTO_INCREMENT", "MOD(", "DATE '", "COMMIT;", "TIMESTAMP"):
            self.assertNotIn(unsupported, sql)

    def test_multiset_comparison_preserves_duplicates_nulls_and_empty(self):
        self.assertEqual(fingerprint([{"x": "a"}, {"x": "b"}]),
                         fingerprint([{"x": "b"}, {"x": "a"}]))
        self.assertNotEqual(fingerprint([{"x": "a"}]), fingerprint([{"x": "a"}, {"x": "a"}]))
        self.assertNotEqual(fingerprint([{"x": None}]), fingerprint([{"x": ""}]))

    def test_readonly_audit_refuses_writes_and_unknown_statements(self):
        for sql in ("DELETE FROM dbo.x", "SELECT * INTO x FROM y",
                    "SELECT 1; EXEC evil", "SELECT /* hidden */ 1", "GRANT SELECT TO x",
                    "SELECT dbo.evil()", "SELECT NEXT VALUE FOR dbo.counter",
                    "SELECT * FROM OPENQUERY(remote,'EXEC evil')"):
            with self.assertRaises(ValueError):
                classify(sql)
        self.assertEqual(classify("SELECT [UPDATE] FROM dbo.x WHERE value=N'DELETE'"), "select")
        self.assertEqual(classify("ROLLBACK TRANSACTION"), "snapshot_session")

    def test_empty_cdc_metadata_is_an_explicit_json_array(self):
        class Run:
            def json_query(self, query):
                if "sys.tables" in query:
                    return [{"schema_name": "cdc", "name": "ddl_history", "is_ms_shipped": True}]
                if "capture_instance" in query:
                    return [{"capture_instance": "dbo_growth_events"}]
                self.assertion = "COALESCE((SELECT *" in query
                return [{"payload": "[]"}]
        run = Run()
        self.assertEqual(cdc_state(run)["ddl_history"]["fingerprint"]["rows"], 0)
        self.assertTrue(run.assertion)

    def test_cdc_alignment_uses_a_captured_marker_not_a_timer(self):
        class Run:
            count = 0
            def json_query(self, query):
                return [{"n": self.count}]
            def query(self, query):
                self.count += 1
                self.statement = query
        run = Run()
        drain_cdc(run)
        self.assertEqual(run.count, 1)
        self.assertIn("DELETE growth_events WHERE id=9000001", run.statement)

    def test_docker_retry_is_only_for_proven_pre_dispatch_refusal(self):
        with tempfile.TemporaryDirectory(dir=runner.ROOT / "evidence") as directory:
            run = SqlServerRun(Path(directory), "sqlserver", "2022", None)
            messages = ["Cannot connect to the Docker daemon at unix:///fixture.sock. "
                        "Is the docker daemon running?", ""]
            def command(instance, argv, **kwargs):
                instance.seq += 1
                error = messages.pop(0)
                (instance.path / f"{instance.seq:04d}.stderr").write_text(error)
                return (1, "") if error else (0, "ready")
            with patch.object(runner.Run, "command", command), patch("sqlserver_runner.time.sleep"):
                self.assertEqual(run.command(["docker", "exec"]), (0, "ready"))
                self.assertEqual(run.seq, 2)
                messages[:] = ["SQL execution failed"]
                with self.assertRaises(RuntimeError):
                    run.command(["docker", "exec"])
                self.assertEqual(run.seq, 3)
                messages[:] = ["Cannot connect to the Docker daemon at unix:///fixture.sock. "
                               "Is the docker daemon running?"] * 3
                with self.assertRaises(RuntimeError):
                    run.command(["docker", "exec"])
                self.assertEqual(run.seq, 6)

    def test_matrix_distinguishes_c5_and_cdc_audit_coverage(self):
        with tempfile.TemporaryDirectory(dir=runner.ROOT / "evidence") as directory:
            root = Path(directory)
            path = root / "sqlserver-2022"
            path.mkdir()
            (path / "SHA256SUMS").write_text("fixture")
            report = {"run_id": "test", "results": [{
                "engine": "sqlserver", "version": "2022",
                "suites": {"T2": "passed", "T7": "passed"}}]}
            rows = matrix.index_rows(root, report, ["T2", "T7"])
            self.assertEqual([(r["suite"], r["variant"]) for r in rows],
                             [("T2", "c1"), ("T2", "c5"), ("T7", "c1")])

    def test_only_native_idle_mapping_appends_are_attributed(self):
        binary = lambda n, size: base64.b64encode(n.to_bytes(size, "big")).decode()
        first = {"start_lsn": binary(10, 10), "tran_id": binary(1, 6),
                 "tran_begin_lsn": binary(9, 10),
                 "tran_begin_time": "2026-11-04T00:00:00",
                 "tran_end_time": "2026-11-04T00:00:00"}
        idle = {"start_lsn": binary(11, 10), "tran_id": binary(0, 1),
                "tran_begin_lsn": binary(0, 10),
                "tran_begin_time": "2026-11-04T00:05:00",
                "tran_end_time": "2026-11-04T00:05:00"}
        def state(rows):
            return {"lsn_time_mapping": {
                "catalog": {"schema_name": "cdc", "name": "lsn_time_mapping",
                            "object_id": 42, "is_ms_shipped": True},
                "rows": rows, "fingerprint": fingerprint(rows)}}
        before, after = state([first]), state([first, idle])
        start, end = "2026-11-04T00:00:00", "2026-11-04T00:10:00"
        scan = {"empty_scans": 5, "errors": 0}
        later = {"empty_scans": 6, "errors": 0}
        self.assertEqual(attribute_cdc(before, after, start, end, scan, later),
                         {"cdc_idle_scan_mapping_append": 1})
        for changes in ({"tran_id": binary(1, 1)}, {"tran_begin_lsn": binary(1, 10)},
                        {"start_lsn": binary(8, 10)}, {"unexpected": 1},
                        {"tran_begin_time": "2026-11-04T00:04:00"},
                        {"tran_begin_time": "2026-11-05T00:05:00",
                         "tran_end_time": "2026-11-05T00:05:00"}):
            with self.assertRaises(ValueError):
                attribute_cdc(before, state([first, dict(idle, **changes)]), start, end, scan, later)
        for bad in (state([idle]), state([first, idle, idle]),
                    state([dict(first, tran_id=binary(2, 6)), idle])):
            with self.assertRaises(ValueError):
                attribute_cdc(before, bad, start, end, scan, later)
        bad = copy.deepcopy(after)
        bad["lsn_time_mapping"]["catalog"]["is_ms_shipped"] = False
        with self.assertRaises(ValueError):
            attribute_cdc(before, bad, start, end, scan, later)
        for bad_scan in (scan, dict(later, errors=1)):
            with self.assertRaises(ValueError):
                attribute_cdc(before, after, start, end, scan, bad_scan)


if __name__ == "__main__":
    unittest.main()
