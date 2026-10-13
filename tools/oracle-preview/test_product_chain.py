import json
import tempfile
import unittest
from pathlib import Path

import full_source as fs
import product_chain as pc


BATCH_NOTE = "9c9619fb66cfa88de78615de7d7ceddcbb754d3587e329f67b20a6e06dc7eeca"
WIDE_NOTE = "81b05f6c621ddce8477155e954de506fc76c227c88e36b167a3d4b1829eae497"
TS_NOTE = "ae8f8e4ed4aee2aed8f659f5d13e93f34e367093fac2b707437cda90dc612463"
SCN = "6395124"
LIVE = (
    fs.SRC
    / "evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/live-chain"
)


def sealer(digest):
    return {
        "protocol": "oo.pipeline.sealed-import-catalog/v1",
        "engine": "oracle",
        "schema_digest": digest,
    }


class ProductChainAdmissionTests(unittest.TestCase):
    def test_closure2_note_is_the_measured_manifest_preimage(self):
        batch, _types = pc.closure2_note_digests("oo04_t_batch", SCN, pc.BATCH_FIELDS)
        wide_fields, _table, _fields, _columns = None, None, None, None
        wide_id, _table, wide_fields, _columns = pc.text_columns("wide")
        ts_id, _table, ts_fields, _columns = pc.text_columns("ts")
        wide, _types = pc.closure2_note_digests(wide_id, SCN, wide_fields)
        ts, _types = pc.closure2_note_digests(ts_id, SCN, ts_fields)
        self.assertEqual(batch, BATCH_NOTE)
        self.assertEqual(wide, WIDE_NOTE)
        self.assertEqual(ts, TS_NOTE)

    def test_preimage_is_refused_as_schema_digest(self):
        with self.assertRaises(pc.AdmissionRefused):
            pc.refuse_preimage_as_schema_digest(BATCH_NOTE, BATCH_NOTE)
        with self.assertRaises(pc.AdmissionRefused):
            pc.schema_digest_from_sealer(sealer(BATCH_NOTE), BATCH_NOTE)
        with self.assertRaises(pc.AdmissionRefused):
            pc.binding_catalog("oo04_t_batch", SCN, BATCH_NOTE, "ab" * 32, "cd" * 32, "ef" * 32, BATCH_NOTE)
        with self.assertRaises(pc.AdmissionRefused):
            pc.manifest_for(
                "oo04_t_batch", "t_batch", 5, "ef" * 32, pc.BATCH_MANIFEST,
                BATCH_NOTE, "ab" * 32, "cd" * 32, SCN, BATCH_NOTE,
            )

    def test_schema_digest_comes_only_from_a_sealer_catalog_object(self):
        digest = "12" * 32
        self.assertEqual(pc.schema_digest_from_sealer(sealer(digest), BATCH_NOTE), digest)
        catalog = sealer(digest)
        manifest = {
            "catalog_identity": {"schema_digest": digest},
            "snapshot": {"kind": "token", "value": f"oo-dl01w1-04-scn-{SCN}"},
        }
        self.assertTrue(pc.manifest_binds_arrow_schema(manifest, catalog, BATCH_NOTE))
        manifest["catalog_identity"]["schema_digest"] = BATCH_NOTE
        self.assertFalse(pc.manifest_binds_arrow_schema(manifest, catalog, BATCH_NOTE))

    def test_foreign_receipt_does_not_start_import(self):
        calls = []
        original = fs.run
        fs.run = lambda *args, **kwargs: calls.append(args)
        try:
            with tempfile.TemporaryDirectory() as tmp:
                directory = Path(tmp) / "import-batch"
                directory.mkdir()
                ok, body = pc.import_prepared(
                    directory, {"schema_digest": BATCH_NOTE}, {}, directory / "missing.arrow",
                    {"schema": pc.FOREIGN_ADMISSION_SCHEMA, "content_commit": pc.FOREIGN_ADMISSION_COMMIT},
                    {},
                )
                self.assertFalse(ok)
                self.assertEqual(body["reason"], "foreign_admission_required")
                self.assertFalse(body["docker_started"])
                self.assertFalse((directory / "native-readonly-receipt.json").exists())
                self.assertFalse((directory / "manifest.json").exists())
                self.assertEqual(calls, [])
                self.assertIsNone(pc.FIXED_IMPORTER_SHA)
        finally:
            fs.run = original

    def test_historical_envelope_is_not_rewritten(self):
        original_run = fs.run
        fs.run = lambda *args, **kwargs: (_ for _ in ()).throw(AssertionError("docker"))
        try:
            with tempfile.TemporaryDirectory() as tmp:
                directory = Path(tmp)
                (directory / "envelope.json").write_text("{}\n")
                (directory / "native-readonly-receipt.json").write_text('{"schema":"keep"}\n')
                before = (directory / "native-readonly-receipt.json").read_bytes()
                ok, body = pc.import_prepared(directory, {}, {}, directory / "a", {}, {})
                self.assertFalse(ok)
                self.assertEqual(body["reason"], "historical_import_preserved")
                self.assertEqual((directory / "native-readonly-receipt.json").read_bytes(), before)
        finally:
            fs.run = original_run

    def test_prepare_object_does_not_write_a_preimage_manifest(self):
        original = fs.EVID
        original_run = fs.run
        fs.run = lambda *args, **kwargs: (_ for _ in ()).throw(AssertionError("docker"))
        try:
            with tempfile.TemporaryDirectory() as tmp:
                fs.EVID = Path(tmp)
                arrow = Path(tmp) / "batch.arrow"
                arrow.write_bytes(b"not-an-arrow")
                item = pc.prepare_object(
                    "batch", "oo04_t_batch", "t_batch", 5, arrow,
                    pc.BATCH_FIELDS, pc.BATCH_MANIFEST, SCN, False, 8,
                )
                self.assertFalse(item["imported"])
                self.assertIsNone(item["schema_digest"])
                self.assertEqual(item["closure2_schema_preimage_sha256"], BATCH_NOTE)
                self.assertEqual(item["reason"], "foreign_admission_required")
                self.assertEqual(item["snapshot"], f"oo-dl01w1-04-scn-{SCN}")
                self.assertFalse((Path(tmp) / "import-batch").exists())
        finally:
            fs.EVID = original
            fs.run = original_run

    def test_committed_manifests_do_not_bind_the_sealed_schema(self):
        rows = []
        for name, source, fields, folder, catalog in (
            ("batch", "oo04_t_batch", pc.BATCH_FIELDS, "import-batch", "catalog-batch"),
            ("wide", "oo04_t_wide_text", pc.text_columns("wide")[2], "import-wide", "catalog-wide"),
            ("ts", "oo04_t_ts_text", pc.text_columns("ts")[2], "import-ts", "catalog-ts"),
        ):
            note, _type_note = pc.closure2_note_digests(source, SCN, fields)
            manifest_path = LIVE / folder / "manifest.json"
            catalog_path = LIVE / catalog / "catalog.json"
            receipt_path = LIVE / folder / "native-readonly-receipt.json"
            before = manifest_path.read_bytes()
            manifest = json.loads(before)
            sealed = json.loads(catalog_path.read_text())
            self.assertEqual(manifest["catalog_identity"]["schema_digest"], note)
            self.assertNotEqual(sealed["schema_digest"], note)
            self.assertFalse(pc.manifest_binds_arrow_schema(manifest, sealed, note))
            for table in sealed["tables"]:
                for column in table["columns"]:
                    self.assertIsNone(column.get("caveat"))
            self.assertEqual(fs.sha256_file(receipt_path), fs.RECEIPT_SHA)
            self.assertEqual(manifest["snapshot"]["value"], f"oo-dl01w1-04-scn-{SCN}")
            self.assertEqual(manifest_path.read_bytes(), before)
            rows.append(name)
        self.assertEqual(rows, ["batch", "wide", "ts"])

    def test_oracle_loss_flags_are_outside_the_sealer_caveat_set(self):
        blocked = pc.oracle_loss_blocked()
        self.assertTrue(blocked["blocked"])
        self.assertEqual(
            blocked["storage_caveat_can_store"],
            {
                "empty_string_is_null": False,
                "unconstrained_number": False,
                "precision_loss": False,
            },
        )
        self.assertFalse(pc.admission_is_foreign({
            "schema": "oo.oracle.capture-admission/v1",
            "content_commit": "not-the-w38-commit",
            "source_engine": "oracle",
        }))
        self.assertTrue(pc.admission_is_foreign({
            "schema": pc.FOREIGN_ADMISSION_SCHEMA,
            "content_commit": pc.FOREIGN_ADMISSION_COMMIT,
        }))

    def test_committed_query_packs_omit_the_loss_flags(self):
        packs = (
            LIVE / "builder/out-fresh/role-pack/pack.json",
            LIVE / "builder-wide/out-fresh/role-pack/pack.json",
            LIVE / "builder-ts/out-fresh/role-pack/pack.json",
        )
        for path in packs:
            before = path.read_bytes()
            text = before.decode()
            for token in ("empty_string_is_null", "unconstrained_number", "precision_loss", "6395124"):
                self.assertNotIn(token, text, path.name)
            self.assertEqual(path.read_bytes(), before)


if __name__ == "__main__":
    unittest.main()
