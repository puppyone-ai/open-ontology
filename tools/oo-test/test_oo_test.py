import datetime as dt
from decimal import Decimal
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import comparison
import runner



def evidence_root():
    """The git-ignored evidence directory is absent in a fresh checkout."""
    path = runner.ROOT / "evidence"
    path.mkdir(exist_ok=True)
    return path

class ComparisonTests(unittest.TestCase):
    def test_decimal_lossless_and_typed(self):
        number = Decimal("12345678901234567890.1234567890")
        self.assertEqual(comparison.canonical(number), ["decimal", "12345678901234567890.123456789"])
        self.assertEqual(comparison.canonical(Decimal("1.00")), comparison.canonical(1))
        self.assertNotEqual(comparison.canonical("1"), comparison.canonical(1))
        self.assertNotEqual(comparison.canonical(None), comparison.canonical(""))
        self.assertEqual(comparison.canonical("", oracle_empty=True), ["null"])
        left = [(1, "", {"text": ""}, [""])]
        right = [(1, None, {"text": ""}, [""])]
        self.assertTrue(comparison.compare(left, right, key_columns=[0], oracle_empty=True)["equal"])
        right = [(1, None, {"text": None}, [""])]
        self.assertFalse(comparison.compare(left, right, key_columns=[0], oracle_empty=True)["equal"])

    def test_float_bits_and_timezone(self):
        self.assertNotEqual(comparison.canonical(0.0), comparison.canonical(-0.0))
        value = dt.datetime.fromisoformat("2026-01-01T08:00:00.123456+08:00")
        self.assertEqual(comparison.canonical(value),
                         ["timestamp", "2026-01-01T00:00:00.123456+00:00", {"offset": "+0800", "zone": None}])
        self.assertEqual(comparison.canonical(value.replace(tzinfo=None))[0], "local-timestamp")
        self.assertEqual(comparison.canonical(b"\x00\xff"), ["bytes", "00ff"])

    def test_chunks_and_first_differing_key(self):
        left = [(1, Decimal("2.0")), (2, "a"), (10, None)]
        right = [(1, Decimal("2.00")), (2, "b")]
        report = comparison.compare(iter(left), iter(right), key_columns=[0], chunk_size=2)
        self.assertFalse(report["equal"])
        self.assertEqual(report["first_difference"]["row_index"], 1)
        self.assertEqual(report["chunks"][1]["rows"], [1, 0])
        digest = hashlib.sha256(b"".join(comparison.encoded(row) for row in left[:2])).hexdigest()
        self.assertEqual(report["chunks"][0]["sha256"][0], digest)
        self.assertTrue(comparison.compare([], [], key_columns=[0])["equal"])

    def test_order_is_not_reinterpreted(self):
        a = [("z", 1), ("A", 2)]
        report = comparison.compare(a, list(reversed(a)), key_columns=[0], consistency="S1")
        self.assertEqual(report["first_difference"]["keys"][0], ["array", [["text", "z"]]])
        with self.assertRaises(ValueError):
            comparison.compare([], [], key_columns=[0], chunk_size=0)


class FixtureTests(unittest.TestCase):
    def test_deterministic_shapes(self):
        for engine in ("mysql", "postgres", "oracle"):
            a = "".join(runner.generator.generate(engine, scale=2_000_000, tables=200))
            self.assertEqual(a, "".join(runner.generator.generate(engine, scale=2_000_000, tables=200)))
            self.assertIn("n<=2000000", a)
            self.assertIn("c299", a)
            self.assertNotIn("c300", a)
            self.assertIn("small_0199", a)
            self.assertIn("log_202612", a)
            self.assertIn("FOREIGN KEY(customer_id)", a)
            self.assertLess(len(a.encode()), 1024 * 1024)
            self.assertLess(max(len(line) for line in a.splitlines()), 2499)

    def test_invalid_sizes(self):
        for scale, tables in [(0, 1), (10_000_001, 1), (1, -1), (1, 5001)]:
            with self.assertRaises(ValueError):
                list(runner.generator.generate("mysql", scale=scale, tables=tables))
        for engine in ("mysql", "postgres", "oracle"):
            self.assertIn("INSERT INTO growth_events", runner.generator.growth(engine, 100, 10000))
        with self.assertRaises(ValueError):
            runner.generator.growth("mysql", 0, 10)

    def test_questions_and_dialects(self):
        import re
        questions = json.loads((runner.ENGINES / "gold/questions.json").read_text())
        self.assertEqual(len(questions), 20)
        for engine in ("mysql", "postgres", "oracle"):
            sql = (runner.ENGINES / "gold" / (engine + ".sql")).read_text()
            self.assertEqual(re.findall(r"-- (Q\d+):", sql), [q["id"] for q in questions])

    def test_pin_resolution_and_duplicate_pins(self):
        pins = {}
        for line in (runner.ROOT / "testing/infra/images.pins").read_text().splitlines():
            if not line or line.startswith("#"):
                continue
            name, platform, digest = line.split()
            self.assertNotIn((name, platform), pins)
            pins[name, platform] = digest
        for engine, versions in [("mysql", ["5.7.44", "8.0", "8.4", "9.x"]),
                                 ("postgres", [str(n) for n in range(12, 19)]),
                                 ("oracle", ["23ai", "21c", "11g"])]:
            for version in versions:
                item = runner.resolve(engine, version)
                for platform, value in item["platforms"].items():
                    self.assertEqual(value["digest"], pins[item["id"], platform])
        self.assertEqual(runner.resolve("oracle", "23ai")["id"], "oracle-free-23.5-slim")
        with self.assertRaises(ValueError):
            runner.resolve("mysql", "10")

    def test_acceptance_patches_exclude_legacy_pins(self):
        expected = {
            "mysql": {"5.7": "5.7.44", "8.0": "8.0.44", "8.4": "8.4.7", "9.x": "9.5.0"},
            "postgres": {"12": "12.22", "13": "13.23", "14": "14.24",
                         "15": "15.19", "16": "16.15", "17": "17.11", "18": "18.6"},
        }
        import t1
        import t2
        self.assertIs(t1.runner.resolve, runner.resolve)
        self.assertIs(t2.runner.resolve, runner.resolve)
        self.assertFalse(Path(t1.__file__).with_name("t1-images.json").exists())
        for engine, versions in expected.items():
            locked = json.loads((runner.ROOT / "testing/infra/images.lock.json").read_text())
            suffix = "-bookworm" if engine == "postgres" else ""
            self.assertEqual(
                sorted(item["id"] for item in locked["images"]
                       if item.get("engine_matrix") == "v0.2" and item["id"].startswith(engine + "-")),
                sorted(engine + "-" + version + suffix for version in versions.values()))
            for major, patch_version in versions.items():
                item = runner.resolve(engine, major)
                self.assertEqual(item["id"], engine + "-" + patch_version + suffix)
                self.assertEqual(item["engine_matrix"], "v0.2")
                self.assertEqual(runner.resolve(engine, patch_version), item)
        for engine, version in [("mysql", "8.4.10"), ("postgres", "17.10")]:
            with self.assertRaises(ValueError):
                runner.resolve(engine, version)

    def test_suite_range(self):
        self.assertEqual(runner.suites("T0-T2,T1,T8"), ["T0", "T1", "T2", "T8"])
        with self.assertRaises(ValueError):
            comparison.canonical(Decimal("NaN"))
        with self.assertRaises(TypeError):
            comparison.canonical({1: "not a JSON key"})


class RunnerTests(unittest.TestCase):
    def test_reader_grants_include_views_without_public_create(self):
        from argparse import Namespace
        for engine, version in [("oracle", "11g"), ("postgres", "13")]:
            with tempfile.TemporaryDirectory(dir=evidence_root()) as directory:
                run = runner.Run(Path(directory), engine, version, Namespace(scale=1, tables=0))
                self.assertLessEqual(len(run.secret), 30)
                self.assertNotEqual(run.secret, run.reader_secret)
                with patch.object(run, "command"), patch.object(run, "sql") as sql:
                    run.seed()
                grants = sql.call_args.args[0]
                if engine == "oracle":
                    self.assertIn("all_views", grants)
                    self.assertIn("t.object_name", grants)
                else:
                    self.assertIn("REVOKE CREATE ON SCHEMA public FROM PUBLIC", grants)
                    self.assertIn("REVOKE TEMPORARY ON DATABASE fixture FROM PUBLIC", grants)
                run.cleanup()

    def test_cleanup_checks_both_labels(self):
        with tempfile.TemporaryDirectory(dir=evidence_root()) as directory:
            run = runner.Run(Path(directory), "mysql", "8.4", None)
            run.created_container = True
            with patch.object(run, "command", return_value=(0, '{"oo.lane":"other","oo.run_id":"other"}')) as cmd:
                self.assertEqual(run.cleanup(), [run.name])
                self.assertEqual(cmd.call_count, 1)

    def test_cleanup_rejects_unparseable_labels(self):
        with tempfile.TemporaryDirectory(dir=evidence_root()) as directory:
            run = runner.Run(Path(directory), "mysql", "8.4", None)
            run.created_container = True
            with patch.object(run, "command", return_value=(0, "null")) as cmd:
                self.assertEqual(run.cleanup(), [run.name])
                self.assertEqual(cmd.call_count, 1)

    def test_checksums_cover_nested_manifest_not_private_material(self):
        with tempfile.TemporaryDirectory(dir=evidence_root()) as directory:
            path = Path(directory)
            (path / "child").mkdir()
            (path / "child" / "receipt.json").write_text("{}")
            (path / ".private").mkdir()
            (path / ".private" / "password").write_text("never include")
            runner.checksums(path / "child")
            runner.checksums(path)
            manifest = (path / "SHA256SUMS").read_text()
            self.assertIn("child/SHA256SUMS", manifest)
            self.assertNotIn("password", manifest)

    def test_failure_raw_exit_and_redaction(self):
        with tempfile.TemporaryDirectory(dir=evidence_root()) as directory:
            run = runner.Run(Path(directory), "mysql", "8.4", None)
            import subprocess
            result = subprocess.CompletedProcess([], 9, run.secret, run.secret)
            with patch("runner.subprocess.run", return_value=result):
                with self.assertRaises(RuntimeError):
                    run.command(["false"])
            self.assertEqual(json.loads((Path(directory) / "0001.json").read_text())["exit"], 9)
            self.assertNotIn(run.secret, (Path(directory) / "0001.stderr").read_text())
            run.cleanup()

    def test_signed_url_redaction(self):
        self.assertEqual(runner.redact('failed "https://registry.invalid/blob?Signature=private&Expires=1"'),
                         'failed "https://registry.invalid/blob?[REDACTED_QUERY]"')

    def test_sqlplus_error_with_zero_exit_is_failure(self):
        with tempfile.TemporaryDirectory(dir=evidence_root()) as directory:
            run = runner.Run(Path(directory), "oracle", "11g", None)
            import subprocess
            result = subprocess.CompletedProcess([], 0, "SP2-0027: Input is too long", "")
            with patch("runner.subprocess.run", return_value=result):
                with self.assertRaises(RuntimeError):
                    run.command(["sqlplus", "-s", "/nolog"])
            raw = json.loads((Path(directory) / "0001.json").read_text())
            self.assertEqual(raw["exit"], 0)
            self.assertTrue(raw["sqlplus_error"])
            run.cleanup()


if __name__ == "__main__":
    unittest.main()
