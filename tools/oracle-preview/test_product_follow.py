import hashlib
import json
import tempfile
import unittest
import uuid
from pathlib import Path

import product_follow as follow_module


class PublicationContextTests(unittest.TestCase):
    def test_oracle_c5_single_table_uses_official_per_table_collection_shape(self):
        follow = follow_module.Follow.__new__(follow_module.Follow)
        follow.c = {"source_id": follow_module.w.SOURCE}
        run_id = str(uuid.uuid4())
        current = {
            "generation": 11,
            "root": None,
            "request": {
                "position_codec": "snapshot-zero/v1",
                "position_lineage": "configured.oracle",
                "applied_pos": "0",
                "consistency_grade": "S0",
                "cross_table_consistent": False,
                "capture_method": "c5_diff",
                "delete_detection": "reconcile",
                "config_digest": "b" * 64,
            },
            "result": {"run_id": run_id},
            "table": {
                "logical_schema": "q_4f4f30345f464958",
                "logical_table": "q_4f4e424f4152445f4f5244455253",
                "stored_physical": "pt_" + "a" * 60,
                "accepted_rows": 4,
                "canonical_digest": "a" * 64,
            },
        }
        with tempfile.TemporaryDirectory() as tmp:
            current["root"] = Path(tmp)
            mapping = follow.publication_context(current)

            receipt = mapping["publication"]["receipt"]
            self.assertEqual(receipt["collection_mode"], "per_table")
            self.assertEqual(receipt["run_ids"], [run_id])
            self.assertEqual(receipt["tables"][0]["run_id"], run_id)
            self.assertEqual(receipt["consistency"], "per_table")
            self.assertEqual(receipt["consistency_grade"], "S0")
            self.assertIs(receipt["cross_table_consistent"], False)
            self.assertEqual(receipt["position_codec"], "snapshot-zero/v1")
            self.assertEqual(receipt["applied_pos"], "0")
            self.assertEqual(receipt["tables"][0]["capture_method"], "c5_diff")
            self.assertEqual(receipt["tables"][0]["delete_detection"], "reconcile")
            self.assertEqual(
                mapping["publication"]["receipt_sha256"],
                hashlib.sha256(follow_module.canonical(receipt)).hexdigest(),
            )
            self.assertEqual(json.loads((Path(tmp) / "publication.json").read_text()), receipt)


if __name__ == "__main__":
    unittest.main()
