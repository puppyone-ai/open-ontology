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
        receipt = entry.PRODUCT_DIR / "product-receipt.json"
        before = entry.sha(receipt)
        identity = entry.current_identity()
        self.assertEqual(identity["source_generation"], 7)
        self.assertEqual(identity["sealed_catalog_sha256"], entry.SEALED_CATALOG)
        self.assertEqual(identity["identity_hash"], entry.IDENTITY_HASH)
        self.assertEqual(identity["schema_digest"], "f002b65dac929bfd96866a1936003e92ed366a186e4dc624fb978ef4af2cc425")
        packs = identity["packs"]
        self.assertEqual(packs["role_pack_file_sha256"], entry.PACK_FILE)
        self.assertEqual(packs["semantic_pack_digest"], entry.PACK_DIGEST)
        self.assertEqual(packs["api_pack_sha256"], entry.API_PACK)
        self.assertEqual(packs["s5_receipt_pack_sha256"], entry.RECEIPT_PACK)
        self.assertEqual(len(set(packs.values())), 4)
        self.assertEqual(identity["build_id"], entry.BUILD)
        self.assertEqual(identity["run_id"], entry.RUN_ID)
        self.assertIs(identity["whole"], False)
        self.assertEqual(entry.sha(receipt), before)
        with tempfile.TemporaryDirectory() as tmp:
            cfg = entry.write_dialogue_config(tmp)
        self.assertEqual(cfg["identity"]["pack_sha256"], entry.PACK_FILE)
        self.assertNotIn("seed", cfg)
        self.assertNotIn("frozen_manifest", cfg)
        self.assertNotIn("prior_manifest", cfg)
        self.assertEqual(entry.sha(receipt), before)

    def test_materials_pin_the_repo_verifier(self):
        source = Path(entry.__file__).read_text()
        self.assertNotIn("fixture_dml", source)
        self.assertNotIn("cycles(", source)
        self.assertNotIn("native-fire", source)
        self.assertNotIn("dialogue-9485942", source)
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "recipe.json"
            recipe = entry.materials(path)
        self.assertIs(recipe["owned_fixture"], True)
        self.assertNotIn("whole_run.py", json.dumps(recipe["steps"]))
        self.assertNotIn("dialogue-9485942.py", json.dumps(recipe))
        self.assertEqual(recipe["steps"][1]["argv"][1], str(entry.FLOW))
        dialogue = next(step for step in recipe["steps"] if step["name"] == "dialogue")
        self.assertEqual(dialogue["argv"][1], str(entry.DIALOGUE))
        self.assertEqual(dialogue["argv"][2:6], ["--config", "{run}/dialogue-config.json", "--out", "{run}/dialogue"])
        pinned = {Path(item["path"]).name: item["sha256"] for item in recipe["pins"]}
        self.assertEqual(pinned["product_flow.py"], entry.sha(entry.FLOW))
        self.assertEqual(pinned["dialogue.py"], entry.sha(entry.DIALOGUE))
        self.assertEqual(pinned["dialogue.py"], "0aefc0b5b283abbf4c4845269cd99fe444de76ae1c273f456bebc49bfbb9096a")
        self.assertNotIn("whole", recipe)
        self.assertEqual(recipe["verifier_commit"], entry.VERIFIER_COMMIT)
        self.assertEqual(pinned["official_entry.py"], entry.sha(Path(entry.__file__).resolve()))


if __name__ == "__main__":
    unittest.main()
