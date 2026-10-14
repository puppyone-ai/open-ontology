import json
import tempfile
import unittest
from pathlib import Path

import sealed_bundle as sb

CATALOG = {
    "protocol": "oo.pipeline.sealed-import-catalog/v1", "engine": "oracle", "source_id": sb.SOURCE,
    "source_generation": 3, "target_generation": 3,
    "tables": [{"accepted_rows": 2, "logical_schema": "q_4f4f30345f464958", "logical_table": "q_4f4e424f4152445f4f5244455253",
                "stored_namespace": "pn_b3e8410a8553cdb7b182ff6b1923a3729a304087865b4dc5f359ff816e34",
                "stored_physical": "pt_c3325c512dc36084cae19f708143453edc4d8bdbbe1e4cd81a141d14ecef",
                "columns": [
                    {"logical_name": "q_4b45595f4944", "source_type": "decimal128_4_0"},
                    {"logical_name": "q_414d4f554e54", "source_type": "decimal128_12_2"},
                    {"logical_name": "q_4f43435552524544", "source_type": "timestamp_micros"},
                    {"logical_name": "q_4c4142454c", "source_type": "utf8"},
                    {"logical_name": "q_5041594c4f4144", "source_type": "binary"},
                ]}],
}


class SealedBundleTest(unittest.TestCase):
    def test_catalog_bytes_and_role_are_bound(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            catalog = root / "catalog.json"
            raw = json.dumps(CATALOG).encode()
            catalog.write_bytes(raw)
            shadow = root / "shadow.json"
            shadow.write_text(json.dumps({"generation": 3, "position": "configured-oracle-scn-7746927", "tombstones": ["2:3"],
                                          "cells": {"1": {"KEY_ID": "1", "AMOUNT": "22.5", "OCCURRED": "a", "LABEL": "b", "PAYLOAD": "c"},
                                                    "3": {"KEY_ID": "3", "AMOUNT": "7.75", "OCCURRED": "d", "LABEL": "e", "PAYLOAD": "f"}}}))
            out = root / "bundle"
            built = sb.build(catalog, out, shadow)
            self.assertEqual((out / "catalog.json").read_bytes(), raw)
            manifest = json.loads((out / "manifest.json").read_text())
            self.assertEqual(set(manifest["files"]), {path.name for path in out.iterdir() if path.name != "manifest.json"} | set())
            for name, digest in manifest["files"].items():
                if name != "manifest.json":
                    self.assertEqual(sb.sha(out / name), digest)
            self.assertEqual(built["columns"], 5)
            self.assertEqual(json.loads((out / "generation.json").read_text())["outcome"], "structural_draft")
            model = json.loads((out / "native-model.json").read_text())
            self.assertEqual(model["settings"]["project"], "oracle")
            self.assertEqual(model["visibility"]["project"], "oracle")
            self.assertFalse(json.loads((out / "read_logic.json").read_text())["executed"])

    def test_other_source_is_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            catalog = Path(tmp) / "catalog.json"
            catalog.write_text(json.dumps({**CATALOG, "source_id": "scm5"}))
            with self.assertRaisesRegex(ValueError, "owned oracle"):
                sb.build(catalog, Path(tmp) / "out")


if __name__ == "__main__":
    unittest.main()
