#!/usr/bin/env python3
import json
import tempfile
import unittest
from pathlib import Path
import official_entry as entry

ROOT = Path(__file__).resolve()


class OfficialEntryTest(unittest.TestCase):
    def test_normal_input_is_six_fields(self):
        body = entry.six_fields()
        self.assertEqual(set(body), {"protocol", "connection"})
        self.assertEqual(set(body["connection"]), {"type", "database", "host", "port", "user", "secret_ref"})
        self.assertNotIn("recipe", body)
        self.assertNotIn("out", body)

    def test_identity_names_generation_seven_and_refuses_whole(self):
        identity = entry.current_identity()
        self.assertEqual(identity["source_generation"], 7)
        self.assertEqual(identity["sealed_catalog_sha256"], entry.SEALED_CATALOG)
        self.assertEqual(identity["role_pack_sha256"], entry.PACK_FILE)
        self.assertEqual(identity["build_id"], entry.BUILD)
        self.assertEqual(identity["run_id"], entry.RUN_ID)
        self.assertIs(identity["whole"], False)

    def test_materials_pin_the_fixed_acceptor_and_current_product_flow(self):
        source = Path(entry.__file__).read_text()
        self.assertNotIn("fixture_dml", source)
        self.assertNotIn("cycles(", source)
        self.assertNotIn("native-fire", source)
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "recipe.json"
            recipe = entry.materials(path)
        self.assertIs(recipe["owned_fixture"], True)
        self.assertNotIn("whole_run.py", json.dumps(recipe["steps"]))
        self.assertEqual(recipe["steps"][1]["argv"][1], str(entry.FLOW))
        pinned = {Path(item["path"]).name: item["sha256"] for item in recipe["pins"]}
        self.assertEqual(pinned["product_flow.py"], entry.sha(entry.FLOW))
        self.assertEqual(pinned["dialogue.py"], entry.sha(entry.DIALOGUE))
        self.assertEqual(pinned["dialogue-9485942.py"], entry.sha(entry.ACCEPTOR))
        self.assertNotIn("whole", recipe)
        self.assertEqual(recipe["acceptor_commit"], entry.ACCEPTOR_COMMIT)
        self.assertEqual(pinned["official_entry.py"], entry.sha(Path(entry.__file__).resolve()))


if __name__ == "__main__":
    unittest.main()
