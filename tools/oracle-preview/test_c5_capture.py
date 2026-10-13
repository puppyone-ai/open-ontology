"""Unit checks for the Oracle C5 shadow. Live waits are in c5_live."""

import json
import tempfile
import time
import unittest
from pathlib import Path

import c5_capture as c5
import c5_live


class C5CaptureTests(unittest.TestCase):
    def test_docs_default_is_daily_and_sla_holds(self):
        periods = c5.resolve_periods(None)
        self.assertEqual(periods["origin"], "docs_default")
        self.assertEqual(periods["reconcile_period_seconds"], 86400)
        self.assertEqual(periods["detect_period_seconds"], 86400)
        self.assertEqual(periods["pull_period_seconds"], 86400)
        with self.assertRaises(c5.CaptureError):
            c5.resolve_periods({
                "reconcile_period_seconds": 20,
                "delete_visibility_sla_seconds": 10,
            })

    def test_configured_wait_is_real(self):
        periods = c5.resolve_periods({
            "detect_period_seconds": 1,
            "pull_period_seconds": 1,
            "reconcile_period_seconds": 1,
            "delete_visibility_sla_seconds": 86400,
        })
        self.assertEqual(periods["origin"], "configured")
        started = time.monotonic()
        waited = c5.wait_realtime(periods)
        self.assertGreaterEqual(time.monotonic() - started, 3)
        self.assertGreaterEqual(waited["elapsed_seconds"], 3)
        self.assertEqual([step["name"] for step in waited["steps"]], ["detect", "pull", "reconcile"])

    def test_fake_clock_records_each_period_without_skipping(self):
        periods = c5.resolve_periods({
            "detect_period_seconds": 2,
            "pull_period_seconds": 3,
            "reconcile_period_seconds": 4,
            "delete_visibility_sla_seconds": 86400,
        })
        clock = {"now": 0.0}

        def sleeper(seconds):
            clock["now"] += seconds

        waited = c5.wait_periods(periods, sleeper, lambda: clock["now"])
        self.assertEqual([step["configured_seconds"] for step in waited["steps"]], [2, 3, 4])
        self.assertEqual(clock["now"], 9)

        def skip(_seconds):
            return None

        with self.assertRaises(c5.CaptureError):
            c5.wait_periods(periods, skip, lambda: 0.0)

    def test_diff_replay_interrupt_schema_expiry_and_stale_upsert(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            columns = ["ID", "V2"]
            rows = {"41": {"ID": 41, "V2": "a b"}, "42": {"ID": 42, "V2": None}}
            interrupted = c5.apply_snapshot(
                root, None, 1, "scn-1", columns, columns, rows, 8, interrupt=True,
            )
            self.assertFalse(interrupted["committed"])
            self.assertEqual(interrupted["state"], "interrupted")
            self.assertFalse(c5.generation_path(root, 1).exists())
            self.assertFalse(c5.staging_path(root, 1).exists())
            first = c5.apply_snapshot(root, None, 1, "scn-1", columns, columns, rows, 8)
            self.assertTrue(first["committed"])
            self.assertFalse(first["resumed"])
            self.assertIsNone(first["schema_digest"])
            self.assertFalse(first["admitted"])
            loaded = c5.load_generation(root, 1)
            replay = c5.apply_snapshot(root, loaded, 2, "scn-1", columns, columns, rows, 8)
            self.assertEqual(replay["events"], [])
            changed = {
                "41": {"ID": 41, "V2": "c"},
                "91": {"ID": 91, "V2": "new"},
            }
            second = c5.apply_snapshot(root, loaded, 2, "scn-2", columns, columns, changed, 8)
            self.assertEqual([event["op"] for event in second["events"]], ["update", "delete", "insert"])
            deleted = {"41": {"ID": 41, "V2": "c"}}
            third = c5.apply_snapshot(
                root, c5.load_generation(root, 2), 3, "scn-3", columns, columns, deleted, 8,
            )
            self.assertEqual(third["events"], [{"op": "delete", "key": "91"}])
            with self.assertRaises(c5.CaptureError):
                c5.apply_snapshot(
                    root,
                    c5.load_generation(root, 3),
                    4,
                    "scn-2",
                    columns,
                    columns,
                    changed,
                    8,
                    stale_upsert_generation=2,
                )
            paused = c5.apply_snapshot(
                root, c5.load_generation(root, 3), 4, "scn-4",
                ["ID", "V2", "C5_NOTE"], columns, {}, 8,
            )
            self.assertEqual(paused["state"], "paused_schema_change")
            self.assertFalse(c5.generation_path(root, 4).exists())
        self.assertEqual(c5.position_state(2, 1), "POSITION_EXPIRED")
        with self.assertRaises(c5.CaptureError):
            c5.resume_expired()

    def test_types_grade_audit_and_canonical_text(self):
        self.assertEqual(c5.column_flags("VARCHAR2", None, None), ["empty_string_is_null"])
        self.assertEqual(c5.column_flags("CHAR", None, None), ["empty_string_is_null"])
        self.assertEqual(c5.column_flags("NUMBER", None, None), ["unconstrained_number"])
        self.assertEqual(c5.column_flags("NUMBER", 16, 6), [])
        self.assertEqual(c5.column_flags("TIMESTAMP", None, 9), ["precision_loss"])
        self.assertEqual(c5.column_flags("DATE", None, None), [])
        self.assertIsNone(c5.normalize_text_cell("VARCHAR2", ""))
        self.assertEqual(c5.normalize_text_cell("VARCHAR2", "ZQ"), "ZQ")
        self.assertEqual(c5.consistency_grade(True, False), "S0")
        self.assertEqual(c5.consistency_grade(False, False), "B")
        self.assertNotEqual(c5.METHOD, "C1")
        self.assertNotEqual(c5.INITIAL_LOAD, "log_cdc")
        text = c5.canonical_json({"ID": 41, "V2": "a b", "N16": "1.250000", "C6": None})
        self.assertEqual(text, '{"C6":null,"ID":41,"N16":"1.250000","V2":"a b"}')
        audit = c5.classify_audit([
            {"action": "SELECT", "object_schema": "OO04_FIX", "object_name": "T_BATCH"},
            {"action": "INSERT", "object_schema": "AUDSYS", "object_name": "AUD$UNIFIED"},
            {"action": "LOGOFF", "object_schema": "-", "object_name": "-"},
        ])
        self.assertEqual(audit["platform_dml"], 0)
        self.assertEqual(audit["kernel_audit"], 1)
        with self.assertRaises(c5.CaptureError):
            c5.refuse_forbidden_sql("GRANT SELECT ON t TO u")
        self.assertEqual(c5.parse_dump("Typ=1 Len=2: 5a,51"), "ZQ")
        self.assertIsNone(c5.parse_dump("<null>"))
        self.assertIsNone(c5.parse_dump("NULL"))
        self.assertEqual(c5.parse_dump("Typ=96 Len=0: "), "")

    def test_observed_null_dump_is_a_sql_null(self):
        text = "\n".join([
            "B|41|3141592653.589793|0|2026-10-10T19:44:15|Typ=96 Len=6: 5a,51,20,20,20,20|Typ=1 Len=5: 61,6c,70,68,61|0001FF00|END",
            "B|42|-0.000001|1|2026-10-10T00:00:00|Typ=96 Len=6: 20,20,20,20,20,20|NULL|<null>|END",
            "B|43|<null>|<null>|2026-10-10T23:59:59|Typ=96 Len=6: 5a,20,20,20,20,20|NULL|00|END",
            "B|44|9999999999.999999|0|2026-10-10T12:00:01|Typ=96 Len=6: 41,42,20,43,20,20|Typ=1 Len=1: 20|DEAD00BE|END",
            "B|45|0.000000|1|2028-02-29T18:45:07|Typ=96 Len=6: 6e,20,20,20,20,20|Typ=1 Len=1: 78|00000000|END",
            "W|71|88888888888888888888888888888888888888|END",
            "W|72|1E-130|END",
            "W|73|<null>|END",
            "W|74|0|END",
            "T|81|2026-10-10T07:11:42.123456789|2026-10-10T07:11:42.123456789 +08:00|END",
            "T|82|2026-10-10T07:11:42.000000001|2026-10-10T07:11:42.000000001 -05:30|END",
        ])
        pulled = c5_live.parse_pull(text, False)
        row42 = pulled["T_BATCH"]["42"]
        self.assertEqual(row42["C6"], "      ")
        self.assertIsNone(row42["V2"])
        self.assertIsNone(row42["R"])
        row43 = pulled["T_BATCH"]["43"]
        self.assertIsNone(row43["N16"])
        self.assertIsNone(row43["V2"])
        self.assertEqual(row43["R"], "00")
        self.assertEqual(pulled["T_BATCH"]["41"]["V2"], "alpha")
        self.assertEqual(pulled["T_BATCH"]["41"]["C6"], "ZQ    ")
        self.assertEqual(pulled["T_BATCH"]["44"]["V2"], " ")
        self.assertEqual(pulled["T_WIDE"]["74"]["N_FREE"], "0")
        self.assertEqual(pulled["T_WIDE"]["72"]["N_FREE"], "1E-130")
        self.assertEqual(pulled["T_TS"]["81"]["TSTZ"], "2026-10-10T07:11:42.123456789 +08:00")
        self.assertTrue(pulled["T_TS"]["82"]["TSTZ"].endswith("-05:30"))

    def test_arrow_request_creates_its_directory(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(c5_live.write_arrow_request(
                Path(tmp) / "arrow-json", "T_BATCH", 1, ["ID", "V2"],
                {"41": {"ID": 41, "V2": None}},
            ))
            self.assertTrue(path.is_file())
            body = json.loads(path.read_text())
            self.assertNotIn("schema_digest", body)
            self.assertIsNone(body["rows"][0]["V2"])

    def test_volume_limit_does_not_commit(self):
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaises(c5.CaptureError):
                c5.apply_snapshot(
                    tmp, None, 1, "scn", ["ID"], ["ID"],
                    {"1": {"ID": 1}, "2": {"ID": 2}}, 1,
                )
            self.assertFalse(c5.generation_path(tmp, 1).exists())


if __name__ == "__main__":
    unittest.main()
