#!/usr/bin/env python3
"""The schema-message hash matches the fixed loader's gen1 Arrow file."""

import unittest
from pathlib import Path

import capture_receipt as receipt

ROOT = Path("/Users/redux/puppy-one/open-ontology")
GEN1 = (
    ROOT
    / "evidence/b5ac4bc8-1872-4fe4-a56a-04a70f7103a2/DL01/native-physical-import"
    / "source-bound-loader/raw/arrow/gen1.arrow"
)
STREAM = (
    ROOT
    / "evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product"
    / "raw/run/product-run-14-c4/arrows/oo-dl01w1-40-c4b-a18-all.arrow"
)


class ReceiptHashTests(unittest.TestCase):
    def test_gen1_file_schema_message_matches_the_fixed_receipt(self) -> None:
        data = GEN1.read_bytes()
        self.assertEqual(
            receipt.sha256_bytes(data),
            "ea5c4ded359e549417da7af4e6ec651fb58109c3bfb857b5367c3b10bf5218a7",
        )
        self.assertEqual(
            receipt.arrow_schema_sha256(data),
            "f4ebfa2fc6765828fee6e8db1d021002c39d0e1896678c12248b04bb838d0439",
        )

    def test_clickhouse_stream_schema_message_is_hex(self) -> None:
        data = STREAM.read_bytes()
        digest = receipt.arrow_schema_sha256(data)
        self.assertEqual(len(digest), 64)
        self.assertNotEqual(digest, receipt.sha256_bytes(data))

    def test_secret_key_is_refused(self) -> None:
        with self.assertRaises(receipt.ReceiptError):
            receipt.refuse_secret({"secret_ref": "secret://example"})


if __name__ == "__main__":
    unittest.main()
