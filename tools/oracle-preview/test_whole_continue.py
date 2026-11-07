from unittest import mock
import tempfile
import os
import unittest
import whole_continue as wc

STEP = "complete=succeeded binding=e7325034-f8f4-4b72-91bd-4f3f9a48128a pin=2 task=t generation=1 load_state=awaiting_publish applied_pos=0\n"
WATERMARK = {"source_id": "oracle_oneclick_orders", "source_generation": 2, "load_state": "awaiting_publish"}
OWNED = {"host": "oo-dl01w1-04-p2-oracle", "port": 1521, "database": "FREEPDB1", "user": "OO04_READER", "secret_ref": "secret://oo04p2/driver/source/reader"}

class WholeContinueTest(unittest.TestCase):
    def test_customer_endpoint_refused_before_sql(self):
        with self.assertRaisesRegex(ValueError, "metadata-only"):
            with tempfile.NamedTemporaryFile("w", suffix=".txt") as terms:
                terms.write("192.0.2.10\n")
                terms.flush()
                with mock.patch.dict(os.environ, {"OO_PRIVATE_TERMS_FILE": terms.name}):
                    wc.owned({"host": "192.0.2.10", "port": 1521, "database": "ORACLE_SRC", "user": "source_reader", "secret_ref": "secret://customer"})

    def test_recorded_load_is_not_fired_again(self):
        wc.owned(OWNED)
        wc.recorded(STEP, WATERMARK)
        self.assertEqual(wc.next_action(["1", "2", "3"], False), "delete_key_2_then_capture_generation_3")
        self.assertEqual(wc.next_action(["1", "3"], False), "capture_after_committed_delete")
        self.assertEqual(wc.next_action(["1", "3"], True), "already_captured")

    def test_other_watermark_is_refused(self):
        with self.assertRaisesRegex(ValueError, "watermark"):
            wc.recorded(STEP, {**WATERMARK, "source_generation": 1})

if __name__ == "__main__":
    unittest.main()
