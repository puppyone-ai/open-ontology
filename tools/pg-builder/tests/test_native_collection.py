import runpy
from pathlib import Path
import unittest

collection = runpy.run_path(str(Path(__file__).parents[1] / "scripts/export_native_collection.py"))["collection"]


class CollectionTest(unittest.TestCase):
    def test_preserves_distinct_run_bindings_and_refuses_duplicate_dataset(self):
        def entry(source, table):
            return {"protocol": "oo.pipeline.sealed-import-catalog/v1", "state": "awaiting_publish",
                    "shadows_queried": False, "source_id": source, "run_id": source + "-run",
                    "source_generation": 2, "target_generation": 3,
                    **{key: "a" * 64 for key in ("config_digest", "schema_digest", "type_digest", "input_digest")},
                    "tables": [{"logical_schema": "logistics", "logical_table": table,
                                "stored_namespace": "pn_a", "stored_physical": "pt_" + source}]}
        first, second = entry("source_a", "hubs"), entry("source_b", "shipments")
        output = collection([first, second])
        self.assertEqual(output["entries"], [first, second])
        self.assertEqual([row["source_id"] for row in output["binding_vector"]], ["source_a", "source_b"])
        self.assertNotIn("source_id", output)
        with self.assertRaises(AssertionError):
            collection([first, first])


if __name__ == "__main__":
    unittest.main()
