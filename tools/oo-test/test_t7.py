import argparse
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import t7


class FakeBase:
    def __init__(self, path, engine, version, args):
        self.path, self.engine, self.version, self.args = path, engine, version, args
        self.id = "00000000-0000-0000-0000-000000000000"
        self.name = "oo-h-" + self.id
        self.seq = 0
        self.calls = []

    def command(self, command, **kwargs):
        self.seq += 1
        self.calls.append(command)
        if command[0] == "/bin/oo-load" and "--fail" in command:
            raise RuntimeError("product failed")
        return 0, ""

    def sql(self, text, admin=False, required=True):
        return self.command(["sql", text])

    def setup(self):
        self.calls.append(["setup"])

    def seed(self):
        self.calls.append(["seed"])


class T7Tests(unittest.TestCase):
    def make(self, engine="mysql", kind="incremental"):
        cls = t7.audited(FakeBase)
        run = cls(Path("/nonexistent"), engine, "8.4", argparse.Namespace(), "/bin/oo-load", kind)
        stamps = iter(f"2026-11-05 00:00:{i:02d}" for i in range(60))
        run.audit.clock = lambda: next(stamps)
        run.audit.enable = lambda: run.calls.append(["enable"])
        self.prints = iter([])
        run.audit.fingerprint = lambda: next(self.prints, {"catalog_sha256": "c", "tables": {"t": "1"}})
        return run

    def test_variant_attribution_from_product_flags(self):
        base = ["/bin/oo-load", "--stack", "s", "--source"]
        self.assertEqual(t7.variant_of(base + ["fixture", "--c5", "c"], "incremental"), "c5")
        self.assertEqual(t7.variant_of(base + ["fixture", "--sync", "c"], "incremental"), "c1")
        self.assertEqual(t7.variant_of(base + ["fixture", "--handoff", "h"], "incremental"), "c1")
        self.assertEqual(t7.variant_of(base + ["fixture_c5"], "incremental"), "c5")
        self.assertEqual(t7.variant_of(base + ["fixture_logminer"], "incremental"), "c1")
        self.assertEqual(t7.variant_of(base + ["fixture"], "snapshot"), "snapshot")

    def test_windows_wrap_only_product_and_preserve_sequence(self):
        run = self.make()
        run.command(["docker", "exec", "x"])
        before = run.seq
        run.command(["/bin/oo-load", "--source", "fixture_c5", "--c5", "c.json"])
        # The runner reads "<seq>.stderr" of the product command it just ran.
        self.assertEqual(run.seq, before + 1)
        with self.assertRaises(RuntimeError):
            run.command(["/bin/oo-load", "--source", "fixture_c1", "--sync", "x", "--fail"])
        self.assertEqual([(w["variant"], w["exit"]) for w in run.windows], [("c5", 0), ("c1", "raised")])
        self.assertTrue(all(w["start"] < w["end"] for w in run.windows))

    def test_quiesced_windows_are_fingerprinted_and_changes_reported(self):
        run = self.make()
        self.prints = iter([{"catalog_sha256": "c", "tables": {"a": "1", "b": "1"}},
                            {"catalog_sha256": "c", "tables": {"a": "1", "b": "2"}}])
        run.command(["/bin/oo-load", "--source", "fixture_c5", "--c5", "c.json"])
        self.assertEqual(run.windows[0]["fingerprint"]["changed_tables"], ["b"])
        self.assertFalse(run.windows[0]["fingerprint"]["equal"])
        # A live fixture writer (non-daemon thread) makes the window non-quiesced.
        import threading
        stop = threading.Event()
        writer = threading.Thread(target=stop.wait)
        writer.start()
        try:
            run.command(["/bin/oo-load", "--source", "fixture_c5", "--c5", "c.json"])
        finally:
            stop.set()
            writer.join()
        self.assertFalse(run.windows[1]["quiesced"])
        self.assertNotIn("fingerprint", run.windows[1])

    def test_mysql_logging_is_a_server_option(self):
        run = self.make()
        run.command(["docker", "create", "--name", run.name, "image"])
        self.assertEqual(run.calls[-1][-2:], ["--general-log=1", "--log-output=TABLE"])
        self.assertTrue(run.name.startswith("oo-t-"))

    def test_audit_enabled_before_reader_exists(self):
        run = self.make("postgres")
        run.setup()
        self.assertEqual(run.calls[-1], ["enable"])
        oracle = self.make("oracle")
        oracle.setup()
        self.assertNotIn(["enable"], oracle.calls)
        oracle.seed()
        self.assertEqual(oracle.calls[-1], ["enable"])

    def test_coverage_requires_workload_and_never_infers(self):
        loads = [{"variant": "snapshot", "exit": 0}] * 2 + [{"variant": "c5", "exit": 0}] * 3
        ok = t7.coverage("snapshot", "mysql", "8.4", {"T1": "passed"}, loads)
        self.assertEqual({k: v[0] for k, v in ok.items()}, {"snapshot": True, "c5": True})
        # T1 must pass: a comparison failure is no longer accepted as coverage.
        self.assertFalse(any(v[0] for v in t7.coverage(
            "snapshot", "oracle", "23", {"T1": "failed", "failure": "source target difference"},
            loads).values()))
        # Old three-load T1 without correction/idle C5 cycles is incomplete.
        self.assertFalse(t7.coverage("snapshot", "mysql", "8.4", {"T1": "passed"},
                                     [{"variant": "snapshot", "exit": 0}] * 3)["snapshot"][0])
        self.assertFalse(t7.coverage("snapshot", "mysql", "8.4", {"T1": "passed"},
                                     loads[:4] + [{"variant": "c5", "exit": 1}])["c5"][0])
        incremental = t7.coverage("incremental", "oracle", "23", {"C1": "passed", "C5": "failed"}, [])
        self.assertEqual({k: v[0] for k, v in incremental.items()}, {"c1": True, "c5": False})
        self.assertEqual(set(t7.coverage("incremental", "oracle", "21", {"C5": "passed"}, [])), {"c5"})

    def test_rejections_fail_every_variant(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            run = self.make()
            run.path = root / "work"
            run.path.mkdir()
            run.created_container, run.audit_enabled = True, True
            run.secret = run.reader_secret = "s"
            run.receipt = {"version_actual": "8.4.7"}
            run.record_version = lambda: None
            run.cleanup = lambda: []
            equal = {"equal": True}
            run.windows = [{"seq": 1, "variant": "c1", "fingerprint": equal},
                           {"seq": 2, "variant": "c5", "fingerprint": equal}]
            records = [{"category": "rejected", "rule": "write_keyword:INSERT", "variant": "harness",
                        "sql": "INSERT INTO t VALUES(1)"}]
            run.audit._admin = lambda statement: ""
            run.audit.collect = lambda windows: {
                "mode": "mysql", "users": ["fixture_reader"], "privileges": {"read_only": True},
                "statements": 3, "records": records, "slots_before_reader": {},
                "summary": {"c1": {"statements": 1, "rejected": 0}, "c5": {"statements": 1, "rejected": 0}}}
            args = argparse.Namespace(binary="/bin/oo-load", worker_image=None, scale=1, tables=0,
                                      disk_wait_minutes=0)
            with patch("t7.audited", return_value=lambda *a: run), \
                    patch("t7.runner.disk_gate"), \
                    patch("t7.t2_p0.run_cycles", return_value={"C1": "passed", "C5": "passed"}):
                report = t7.run_fixture("mysql", "8.4", "incremental", root, args)
            self.assertEqual({v: d["result"] for v, d in report["verdicts"].items()}, {"c1": "fail", "c5": "fail"})
            rows = json.loads((root / "t7/mysql-8.4/index-rows.json").read_text())
            self.assertEqual(sorted((r["suite"], r["variant"], r["result"]) for r in rows),
                             [("T7", "c1", "fail"), ("T7", "c5", "fail")])
            self.assertEqual(report["rejected"][0]["rule"], "write_keyword:INSERT")

    def test_fingerprint_change_or_missing_quiesced_window_fails(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            run = self.make()
            run.path = root / "work"
            run.path.mkdir()
            run.created_container, run.audit_enabled = True, True
            run.secret = run.reader_secret = "s"
            run.receipt = {"version_actual": "8.4.7"}
            run.record_version = lambda: None
            run.cleanup = lambda: []
            run.windows = [{"seq": 1, "variant": "c1", "fingerprint": {"equal": False}},
                           {"seq": 2, "variant": "c5", "quiesced": False}]
            run.audit._admin = lambda statement: ""
            run.audit.collect = lambda windows: {
                "mode": "mysql", "users": ["fixture_reader"], "privileges": {"read_only": True},
                "statements": 3, "records": [], "slots_before_reader": {},
                "summary": {"c1": {"statements": 1, "rejected": 0}, "c5": {"statements": 1, "rejected": 0}}}
            args = argparse.Namespace(binary="/bin/oo-load", worker_image=None, scale=1, tables=0,
                                      disk_wait_minutes=0)
            with patch("t7.audited", return_value=lambda *a: run), patch("t7.runner.disk_gate"), \
                    patch("t7.t2_p0.run_cycles", return_value={"C1": "passed", "C5": "passed"}):
                report = t7.run_fixture("mysql", "8.4", "incremental", root, args)
            self.assertIn("source fingerprint changed in quiesced window(s) 1", report["verdicts"]["c1"]["reasons"])
            self.assertIn("no quiesced fingerprinted c5 window", report["verdicts"]["c5"]["reasons"])
            self.assertIn("fingerprint positive control did not detect the admin write",
                          report["verdicts"]["c1"]["reasons"])


if __name__ == "__main__":
    unittest.main()
