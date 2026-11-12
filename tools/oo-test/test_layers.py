import datetime
import unittest
from unittest.mock import patch

import layers


class LayerPlanTests(unittest.TestCase):
    def test_gates_come_from_dispatch_code_not_tests(self):
        gates = layers.gates()
        pg = {g["gate"]: tuple(g["threshold"]) for g in gates if g["engine"] == "postgres"}
        self.assertEqual(pg["tid_range_scan"], (14, 0, 0))
        self.assertEqual(pg["virtual_generated_columns"], (18, 0, 0))
        oracle = {(g["gate"], tuple(g["threshold"])) for g in gates if g["engine"] == "oracle"}
        self.assertIn(("fetch_first", (12, 1, 0)), oracle)
        self.assertIn(("multitenant", (21, 0, 0)), oracle)
        # Asserted values inside #[cfg(test)] (e.g. 8.4.7) are not dispatch gates.
        mysql = {tuple(g["threshold"]) for g in gates if g["engine"] == "mysql"}
        self.assertNotIn((8, 4, 7), mysql)

    def test_classes_split_only_on_relevant_gates(self):
        gates = [{"engine": "postgres", "threshold": [14, 0, 0], "caps": ["snapshot"]},
                 {"engine": "postgres", "threshold": [16, 0, 0], "caps": ["cdc"]}]
        with patch.object(layers, "actual", lambda e, v: (int(v), 0, 0)):
            self.assertEqual(layers.classes("postgres", "T1", gates),
                             [["13"], ["14", "15", "16", "17", "18"]])
            self.assertEqual(layers.classes("postgres", "T2", gates),
                             [["13", "14", "15"], ["16", "17", "18"]])
            # Security assertions are never sampled.
            self.assertEqual(len(layers.classes("postgres", "T7", gates)), 6)

    def test_rotation_visits_every_member(self):
        members = ["14", "15", "16", "17"]
        start = datetime.date(2026, 11, 12)
        seen = {layers.representative(members, start + datetime.timedelta(days=i))
                for i in range(len(members))}
        self.assertEqual(seen, set(members))

    def test_every_version_runs_every_suite_fully_or_as_probe(self):
        cells = layers.l1_cells(["postgres", "mysql", "oracle"],
                                ["T0", "T1", "T2", "T4", "T7", "T8"], datetime.date(2026, 11, 12))
        self.assertEqual(len(cells), 14)
        for cell in cells:
            self.assertEqual(sorted(cell["full"] + cell["probe"]),
                             ["T0", "T1", "T2", "T4", "T7", "T8"])
            self.assertIn("T7", cell["full"])

    def test_impact_rules_coverage_and_unmapped_fallback(self):
        selected, reasons = layers.impacted(["crates/oo-connectors/src/oracle/engine.rs"])
        self.assertEqual(selected, {"oracle"})
        selected, _ = layers.impacted(["docs/guide/x.md", "execution/v0.2/ledger.md"])
        self.assertEqual(selected, set())
        selected, reasons = layers.impacted(
            ["crates/oo-core/src/lib.rs"], {"postgres": {"crates/oo-core/src/lib.rs"}})
        self.assertEqual((selected, reasons["crates/oo-core/src/lib.rs"]), ({"postgres"}, "coverage"))
        selected, reasons = layers.impacted(["crates/oo-gateway/src/main.rs"])
        self.assertEqual(selected, {"postgres", "mysql", "oracle"})
        self.assertEqual(reasons["crates/oo-gateway/src/main.rs"], "unmapped_selects_all")

    def test_scheduler_respects_memory_and_container_caps(self):
        cells = [{"engine": "oracle"}, {"engine": "oracle"}, {"engine": "postgres"},
                 {"engine": "mysql"}]
        waves = layers.schedule(cells, budget_gb=40)
        for wave in waves:
            engines = [c["engine"] for c in wave["cells"]]
            self.assertLessEqual(engines.count("oracle"), 1)
            other = sum(1 if e == "oracle" else 2 for e in engines)
            self.assertLessEqual(other, 2)
            self.assertLessEqual(wave["memory_gb"], 40)
        self.assertEqual(sum(len(w["cells"]) for w in waves), 4)
        roomy = layers.schedule(cells, budget_gb=40, max_other_containers=6)
        self.assertLess(len(roomy), len(waves))
        with self.assertRaises(ValueError):
            layers.schedule([{"engine": "oracle"}], budget_gb=4)


if __name__ == "__main__":
    unittest.main()
