"""Host checks for the C5 witness. No database, no sealer, no loader."""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

import c5_witness as witness


class ArrowParse(unittest.TestCase):
    def test_committed_streams(self):
        for name, schema_sha in witness.SCHEMA_MESSAGE.items():
            blob = (witness.ARROW_DIR / name).read_bytes()
            self.assertEqual(witness.sha256_hex(blob), witness.STREAM_SHA[name])
            parsed = witness.parse_arrow(blob)
            self.assertEqual(parsed["schema_sha256"], schema_sha)
            self.assertEqual(witness.schema_message_sha256(blob), schema_sha)
            self.assertEqual(parsed["rows"], witness.ROW_COUNT[name])
            self.assertNotEqual(parsed["schema_sha256"], witness.sha256_hex(blob))

    def test_wide_and_ts_types_come_from_the_ipc(self):
        wide = witness.parse_arrow((witness.ARROW_DIR / "t_wide-gen-1.arrow").read_bytes())
        self.assertTrue(wide["supported"])
        self.assertEqual(
            [(field["name"], field["arrow_type"], field["nullable"], field["precision"]) for field in wide["fields"]],
            [("ID", "int16", False, 16), ("N_FREE", "utf8", True, None)],
        )
        ts = witness.parse_arrow((witness.ARROW_DIR / "t_ts-gen-1.arrow").read_bytes())
        self.assertTrue(ts["supported"])
        self.assertEqual([field["arrow_type"] for field in ts["fields"]], ["int16", "utf8", "utf8"])
        self.assertEqual([field["name"] for field in ts["fields"]], ["ID", "TS", "TSTZ"])
        self.assertFalse(ts["fields"][0]["nullable"])
        self.assertTrue(ts["fields"][1]["nullable"])

    def test_batch_date_keeps_timestamp_micros_and_c5_note(self):
        parsed = witness.parse_arrow((witness.ARROW_DIR / "t_batch-gen-8.arrow").read_bytes())
        self.assertTrue(parsed["supported"])
        names = [field["name"] for field in parsed["fields"]]
        self.assertEqual(names, ["ID", "N16", "N1", "D", "C6", "V2", "R", "C5_NOTE"])
        date = next(field for field in parsed["fields"] if field["name"] == "D")
        self.assertTrue(date["supported"])
        self.assertEqual(date["arrow_type"], "timestamp_micros")
        note = next(field for field in parsed["fields"] if field["name"] == "C5_NOTE")
        self.assertEqual(note["arrow_type"], "utf8")
        number = next(field for field in parsed["fields"] if field["name"] == "N16")
        self.assertEqual((number["arrow_type"], number["precision"], number["scale"]), ("decimal128", 16, 6))


class WitnessPackage(unittest.TestCase):
    def test_tombstone_stays_out_of_generation_8_cells(self):
        shadow = witness.load_json(witness.shadow_path("T_BATCH", 8))
        self.assertEqual(shadow["tombstones"], ["91:5"])
        self.assertNotIn("91", shadow["cells"])

    def test_grants_omit_inherit_privileges(self):
        sent, skipped = witness.grants_from_site(witness.load_json(witness.SITE_PATH))
        names = {item["name"] for item in sent}
        self.assertEqual(names, {"CREATE SESSION", "SELECT", "FLASHBACK"})
        session = next(item for item in sent if item["name"] == "CREATE SESSION")
        self.assertNotIn("scope", session)
        self.assertTrue(any(item["name"] == "INHERIT PRIVILEGES" for item in skipped))
        self.assertFalse(any("secret://" in json.dumps(item) for item in sent))

    def test_package_has_no_trusted_bit_and_yaml_shows_semantics(self):
        driver = {
            "kind": "native-rust",
            "name": "oracledb",
            "version": "26.0.0-beta.4",
            "digest": "ab" * 32,
        }
        with tempfile.TemporaryDirectory(prefix="oo04-witness-") as tmp:
            root = Path(tmp)
            driver_path = root / "driver"
            driver_path.write_bytes(b"oracle-child-fixture\n")
            out = root / "pkg"
            witness.write_tree(out, driver_path, driver)
            text = (out / "standard.yaml").read_text()
            fresh = (out / "freshness.yaml").read_text()
            for token in (
                "C5_NOTE",
                "empty_string_is_null",
                "unconstrained_number",
                "precision_loss",
                "1E-130",
                "+08:00",
                "-05:30",
                "123456789",
                "91:5",
                "DELETES_LAGGED",
            ):
                self.assertIn(token, text)
                self.assertIn(token, fresh)
            self.assertIn("t_batch_generation_8_has_91: false", text)
            self.assertIn("key_91_in_cells: false", fresh)
            self.assertIn("not a compiler role pack", text)
            for source_id in ("oo04_c5_wide", "oo04_c5_ts", "oo04_c5_batch"):
                body = json.loads((out / source_id / "witness.json").read_text())
                self.assertNotIn("admitted", body)
                self.assertNotIn("trusted", body)
                self.assertFalse(any("sha256" in key for key in body))
                capturer = json.loads((out / source_id / "tick.json").read_text())
                self.assertIs(capturer["snapshot_token"], False)
                self.assertEqual(capturer["product_reader_write_attempts"], 0)
                self.assertNotIn("admitted", capturer)
                self.assertNotIn("trusted", capturer)
                self.assertNotIn("secret_ref", capturer)
                self.assertEqual(capturer["snapshot"]["kind"], "scn")
            wide = json.loads((out / "oo04_c5_wide" / "schema.json").read_text())
            self.assertEqual(wide[0]["columns"][0]["precision"], 16)
            self.assertNotIn("precision", wide[0]["columns"][1])
            batch = json.loads((out / "oo04_c5_batch" / "arrow-parse.json").read_text())
            self.assertEqual(batch["unsupported_columns"], [])
            schema = json.loads((out / "oo04_c5_batch" / "schema.json").read_text())
            fixed_char = next(c for c in schema[0]["columns"] if c["name"] == "C6")
            self.assertEqual(fixed_char["char_length"], 6)
            self.assertNotIn("91", batch["cell_keys"])


if __name__ == "__main__":
    unittest.main()
