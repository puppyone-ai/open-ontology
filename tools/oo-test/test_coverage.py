import unittest

import coverage
import runner


def function(name, *regions):
    return {"filenames": [name], "regions": [list(r) for r in regions]}


class CoverageDiffTests(unittest.TestCase):
    def test_only_product_code_regions_count(self):
        root = str(runner.ROOT)
        self.assertEqual(coverage.product_file(root + "/tools/oo-load/src/c5.rs"),
                         "tools/oo-load/src/c5.rs")
        self.assertEqual(coverage.product_file(root + "/crates/oo-core/src/lib.rs"),
                         "crates/oo-core/src/lib.rs")
        for outside in (root + "/crates/oo-connectors/vendor/sqlx-core/src/a.rs",
                        "/Users/x/.cargo/registry/src/tokio/lib.rs", root + "/tools/oo-test/x.rs"):
            self.assertIsNone(coverage.product_file(outside))

    def test_generic_instances_union_and_only_code_regions(self):
        name = str(runner.ROOT / "crates/oo-core/src/lib.rs")
        export = {"data": [{"functions": [
            function(name, (1, 1, 2, 2, 0, 0, 0, 0), (3, 1, 4, 2, 5, 0, 0, 0),
                     (9, 1, 9, 5, 7, 0, 0, 2)),
            function(name, (1, 1, 2, 2, 4, 0, 0, 0)),  # another monomorphization
        ]}]}
        seen, covered = coverage.regions(export)
        self.assertEqual(len(seen), 2)  # the gap region (kind 2) is ignored
        self.assertEqual(covered, seen)

    def test_summary_counts_per_file(self):
        seen = {("a", 1, 1, 1, 2), ("a", 2, 1, 2, 2), ("b", 1, 1, 1, 2)}
        summary = coverage.summarize(seen, {("a", 1, 1, 1, 2)})
        self.assertEqual(summary["percent"], 33.33)
        self.assertEqual(summary["files"]["a"], {"regions": 2, "covered": 1})


if __name__ == "__main__":
    unittest.main()
