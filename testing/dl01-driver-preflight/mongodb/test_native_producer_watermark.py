"""Source generation follows an acknowledged worker watermark only."""
import importlib.util
import pathlib
import unittest


MODULE = pathlib.Path(__file__).resolve().parents[3] / "tools/mongodb-preview/scripts/native-producer.py"
spec = importlib.util.spec_from_file_location("native_producer", MODULE)
producer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(producer)


class WatermarkTests(unittest.TestCase):
    def test_generation_batch_identity_is_catalog_closed_and_distinct(self):
        first = producer.batch_id('oo_w141_native_c5', 1)
        second = producer.batch_id('oo_w141_native_c5', 2)
        self.assertRegex(first, r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}_1$')
        self.assertNotEqual(first, second)
        self.assertEqual(first, producer.batch_id('oo_w141_native_c5', 1))
        with self.assertRaises(RuntimeError):
            producer.batch_id('oo_w141_native_c5', 0)

    def test_initial_and_acknowledged_generation(self):
        self.assertEqual(producer.acknowledged_generation({"pin": 1, "acknowledged_source_generation": None, "acknowledged_pin": None}), 0)
        self.assertEqual(producer.acknowledged_generation({"pin": 4, "acknowledged_source_generation": 2, "acknowledged_pin": 3}), 2)

    def test_unacknowledged_or_malformed_state_refuses(self):
        for job in (
            {"pin": 2},
            {"pin": 2, "acknowledged_source_generation": 1, "acknowledged_pin": None},
            {"pin": 2, "acknowledged_source_generation": True, "acknowledged_pin": 1},
            {"pin": 2, "acknowledged_source_generation": 2, "acknowledged_pin": 2},
        ):
            with self.subTest(job=job), self.assertRaises(RuntimeError):
                producer.acknowledged_generation(job)

    def test_frozen_real_bson_tags_match_decoder_without_value_coercion(self):
        base = MODULE.parents[3] / "evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/native-c5-20261012/producer-cycles/41a14141-1414-4414-8414-202610120041/1"
        rows = __import__("json").loads((base / "decode.json").read_text())["rows"]
        raw = (base / "full-approved.bson.hex").read_text().splitlines()
        self.assertEqual(len(raw), 14)
        for row, document in zip(rows, raw, strict=True):
            tags = producer.bson_field_types(document)
            self.assertEqual(set(tags), set(row["keys"]))
            self.assertEqual(tags["_id"], {"variant": "07"})
            self.assertEqual(tags["uid"], {"variant": "05", "subtype": "04"})
            self.assertEqual(tags["blob"], {"variant": "05", "subtype": "00"})
        with self.assertRaisesRegex(RuntimeError, "framing"):
            producer.bson_field_types(raw[0][:-2])


if __name__ == "__main__":
    unittest.main()
