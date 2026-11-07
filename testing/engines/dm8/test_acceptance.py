import importlib.util
from pathlib import Path
import sys
import unittest

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parents[2] / "tools/oo-test"))

import fixture  # noqa: E402
import matrix  # noqa: E402
from target_acceptance import (  # noqa: E402
    SELECT_LIMIT, TYPES, TYPES_TARGET, canonical_source, canonical_target, columns,
    merge_groups, resources_ok, source_expression, source_selects, validate_cycle,
)

spec = importlib.util.spec_from_file_location("dm8_worker", HERE / "worker/worker.py")
worker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(worker)


def receipt(table, **values):
    return dict({"protocol": "oo.sync/v1", "mode": "c5", "replay": False,
                 "table": {"name": table}, "target_write_bytes": 0, "new_generations": 0,
                 "payload_bytes": 0, "source_read_bytes": 0}, **values)


class SeedTests(unittest.TestCase):
    def test_full_fixture_is_the_matrix_scale(self):
        sql, tables, scale = fixture.statements(full=True)
        self.assertEqual((len(tables), len(set(tables)), scale), (231, 231, 2_000_000))
        self.assertEqual(sum(s.startswith("GRANT SELECT") for s in sql), 231)
        self.assertTrue(any("C299 VARCHAR(8)" in s for s in sql))

    def test_seed_never_carries_a_secret_and_closes_the_schema_block(self):
        sql, tables, _ = fixture.statements()
        self.assertEqual(tables, fixture.FOCUSED)
        self.assertIn('IDENTIFIED BY "{reader}"', sql[0])
        script = fixture.script(sql)
        self.assertIn("CREATE SCHEMA FIXTURE;\n/\n", script)
        self.assertEqual(len(fixture.TYPES_ROWS[2].split(",")), len(TYPES))

    def test_long_statements_break_only_outside_literals(self):
        from admin import wrap
        sql, _, _ = fixture.statements(full=True)
        wide = next(s for s in sql if s.startswith("INSERT INTO FIXTURE.WIDE_ROWS"))
        wrapped = wrap(wide)
        self.assertEqual(wrapped.replace("\n", ""), wide)
        self.assertTrue(all(len(line) < 2400 for line in wrapped.split("\n")))
        self.assertEqual(wrap("SELECT 'a,b'"), "SELECT 'a,b'")
        self.assertEqual(wrap("'" + "x," * 600 + "'", limit=10), "'" + "x," * 600 + "'")

    def test_matrix_target_is_dm8(self):
        self.assertEqual(matrix.target_id("dm8", "8"), "dm8")
        self.assertEqual(set(TYPES_TARGET), {name for name, _ in TYPES})


class OracleTests(unittest.TestCase):
    def test_zoned_values_compare_as_utc_instants(self):
        source = canonical_source("tsz", "2026-11-05 01:02:03.123456 +05:30")
        self.assertEqual(source, canonical_target("tsz", "2026-11-04T19:32:03.123456+00:00"))
        self.assertNotEqual(source, canonical_target("tsz", "2026-11-05T01:02:03.123456"))
        self.assertNotEqual(canonical_source("ts", "2026-11-05 01:02:03.000000"),
                            canonical_target("ts", "2026-11-05T01:02:03+00:00"))

    def test_exact_values_nulls_and_empty_strings_are_distinct(self):
        self.assertEqual(canonical_source("dec", "-0.00000000"), canonical_target("dec", "0"))
        self.assertNotEqual(canonical_source("dec", "0.14285714"), canonical_target("dec", "0.1428571"))
        self.assertIsNone(canonical_source("text", "~"))
        self.assertEqual(canonical_source("text", "x"), "")
        self.assertEqual(canonical_source("text", "x" + "中".encode().hex()), "中")
        self.assertEqual(canonical_source("bin", "x00FF"), canonical_target("bin", "\\x00ff"))
        self.assertEqual(canonical_source("bool", "1"), canonical_target("bool", True))
        self.assertNotEqual(canonical_source("bool", "0"), canonical_target("bool", 0))
        self.assertEqual(canonical_source("real", "3.25E0"), canonical_target("real", 3.25))

    def test_source_rendering_marks_nulls_and_hexes_text(self):
        expression = source_expression("V", "text")
        self.assertIn("IS NULL THEN '~'", expression)
        self.assertIn("RAWTOHEX(UTL_RAW.CAST_TO_RAW", expression)
        self.assertEqual(columns("TABLE_001"), [("ID", "int"), ("VALUE", "text")])
        self.assertEqual(len(columns("WIDE_ROWS")), 300)

    def test_wide_tables_are_read_in_keyed_column_groups(self):
        self.assertEqual(len(source_selects("TYPES", TYPES)), 1)
        selects = source_selects("WIDE_ROWS", columns("WIDE_ROWS"))
        self.assertGreater(len(selects), 1)
        self.assertTrue(all(len(s) <= SELECT_LIMIT for s in selects))
        key = source_expression("ID", "int")
        self.assertTrue(all(s.startswith("SELECT 'R|'||" + key + "||'|'||") for s in selects))
        self.assertEqual(sum(s.count("CASE WHEN") for s in selects) - len(selects), 299)
        merged = merge_groups([["1|a|b", "2|~|c"], ["2|d", "1|e"]])
        self.assertEqual(sorted(merged), ["1|a|b|e", "2|~|c|d"])
        with self.assertRaises(RuntimeError):
            merge_groups([["1|a"], ["2|b"]])
        with self.assertRaises(RuntimeError):
            merge_groups([["1|a", "1|b"], ["1|c"]])


class CycleTests(unittest.TestCase):
    tables = ["EVENTS", "NO_KEY"]

    def test_idle_cycles_need_zero_writes_and_reads(self):
        idle = [receipt(t) for t in self.tables]
        self.assertEqual(validate_cycle(idle, self.tables, 2), (0, 0))
        reread = [receipt("EVENTS", source_read_bytes=1), receipt("NO_KEY")]
        with self.assertRaises(ValueError):
            validate_cycle(reread, self.tables, 4)
        with self.assertRaises(ValueError):
            validate_cycle(idle[:1], self.tables, 2)

    def test_changed_cycle_needs_one_generation_and_correction_may_write(self):
        changed = [receipt("EVENTS", target_write_bytes=10, new_generations=1), receipt("NO_KEY")]
        self.assertEqual(validate_cycle(changed, self.tables, 3), (10, 1))
        self.assertEqual(validate_cycle(changed, self.tables, 1), (10, 1))
        with self.assertRaises(ValueError):
            validate_cycle([receipt(t) for t in self.tables], self.tables, 1)
        with self.assertRaises(ValueError):
            validate_cycle([receipt(t) for t in self.tables], self.tables, 3)
        with self.assertRaises(ValueError):
            validate_cycle([receipt(t, replay=True) for t in self.tables], self.tables, 1)

    def test_resources_need_one_session_and_quarter_duty(self):
        ok = {"sessions": 1, "read_micros": 10, "sleep_micros": 30}
        self.assertTrue(resources_ok([ok]))
        self.assertFalse(resources_ok([]))
        self.assertFalse(resources_ok([dict(ok, sessions=2)]))
        self.assertFalse(resources_ok([dict(ok, sleep_micros=29)]))


class WorkerTests(unittest.TestCase):
    def test_values_are_lossless_text_or_refused(self):
        import datetime
        import decimal
        self.assertEqual(worker.text(True), "1")
        self.assertEqual(worker.text(decimal.Decimal("1E+3")), "1000")
        self.assertEqual(worker.text(0.1), "0.1")
        self.assertEqual(worker.text(b"\x00\xff"), "00ff")
        self.assertIsNone(worker.text(None))
        with self.assertRaises(TypeError):
            worker.text(datetime.timedelta(days=1))

    def test_only_bounded_selects_reach_the_driver(self):
        good = {"operation": "query", "sql": "SELECT 1 FROM DUAL", "max_rows": 1,
                "max_bytes": 1, "timeout_ms": 1}
        worker.validate(good)
        for bad in ({"sql": "DELETE FROM T"}, {"sql": "SELECT 1; DROP TABLE T"},
                    {"max_rows": 0}, {"max_bytes": 1 << 30}, {"timeout_ms": 0}):
            with self.assertRaises(ValueError):
                worker.validate(dict(good, **bad))
        with self.assertRaises(ValueError):
            worker.endpoint({"host": "db.example", "port": 5236, "user": "U", "password": "p"})
        with self.assertRaises(ValueError):
            worker.endpoint({"host": "127.0.0.1", "port": 5236, "user": "U", "password": "a\nb"})


if __name__ == "__main__":
    unittest.main()
