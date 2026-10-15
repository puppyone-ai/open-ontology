import unittest
from pathlib import Path
import uuid

import rust_c5_reader as reader


class RustC5ReaderTest(unittest.TestCase):
    def setUp(self):
        self.cfg = {
            "host": "oo-dl01w1-04-p2-oracle",
            "port": 1521,
            "database": "FREEPDB1",
            "user": "OO04_READER",
            "hard_limit": 8,
        }
        self.selection = {
            "object": "OO04_FIX.ONBOARD_ORDERS",
            "key": "KEY_ID",
            "source_id": "oracle_oneclick_orders",
        }
        self.ca = Path("/tmp/oo-oracle-reader-test/ca-ref")

    def test_customer_endpoint_is_rejected_before_secret_access(self):
        self.cfg["host"] = "47.104.150.9"
        with self.assertRaisesRegex(ValueError, "authorized local fixture"):
            reader._checked_argv(self.cfg, self.selection, "12345", 0, None, None, None,
                                 Path("/tmp/oo-oracle-reader-test/out"), self.ca)

    def test_parent_must_match_acknowledged_generation(self):
        with self.assertRaisesRegex(ValueError, "parent artifact"):
            reader._checked_argv(self.cfg, self.selection, "12345", 3, None, None, None,
                                 Path("/tmp/oo-oracle-reader-test/out"), self.ca)

    def test_arrow_roundtrip_compares_numeric_value_without_losing_scale(self):
        columns = [{"name": "AMOUNT", "type": "decimal128", "precision": 12, "scale": 2}]
        self.assertTrue(reader._same_rows(columns, [{"AMOUNT": "11.5"}], [{"AMOUNT": "11.50"}]))
        self.assertFalse(reader._same_rows(columns, [{"AMOUNT": "11.5"}], [{"AMOUNT": "11.51"}]))

    def test_nochange_envelope_uses_rust_snapshot_and_actual_binding(self):
        incoming = {
            "protocol": "oo.jobs.native-producer/v1",
            "binding": str(uuid.UUID(int=1)),
            "pin": 8,
            "source_id": "oracle_oneclick_orders",
            "method": "C5",
            "cycle": "reconcile",
        }
        digest = "a" * 64
        parent = "b" * 64
        result = {
            "protocol": "oo.oracle.rust-c5-snapshot/v1",
            "source_id": incoming["source_id"],
            "source_generation": 7,
            "acknowledged_source_generation": 7,
            "empty_diff": True,
            "snapshot": {"kind": "c5_cycle", "value": digest},
            "source_digest": digest,
            "parent_digest": parent,
            "parent_input_digest": parent,
            "data_gen": False,
            "apply": False,
            "ack": False,
            "reader_mutations": 0,
        }
        response = reader.nochange_response(result, incoming, 7)
        self.assertEqual(response["protocol"], "oo.jobs.native-c5-nochange/v1")
        self.assertEqual((response["binding"], response["pin"], response["source_generation"]),
                         (incoming["binding"], 8, 7))
        result["parent_input_digest"] = "c" * 64
        with self.assertRaisesRegex(ValueError, "snapshot binding"):
            reader.nochange_response(result, incoming, 7)


if __name__ == "__main__":
    unittest.main()
