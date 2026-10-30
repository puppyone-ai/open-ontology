import copy
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[2]/"testing/dl01-targets/oracle/scripts"))
import frozen_capture as capture

class FrozenCaptureTests(unittest.TestCase):
    def test_baseline_cannot_claim_cdc_or_include_unplanned_writes(self):
        receipt = {"snapshot_rows_not_c1": 10000, "captured_log_rows": 0,
                   "committed_counts": {"c": 0, "u": 0, "d": 0}}
        capture.check_receipt(receipt)
        for key, value in (("snapshot_rows_not_c1", 9999), ("captured_log_rows", 1),
                           ("committed_counts", {"c": 1, "u": 0, "d": 0})):
            bad = copy.deepcopy(receipt)
            bad[key] = value
            with self.assertRaisesRegex(ValueError, "baseline_only"):
                capture.check_receipt(bad)

    def test_new_scope_is_not_the_retained_extended_source(self):
        self.assertEqual(capture.SOURCE, "oracle_frozen_iud_20261027")
        self.assertEqual(capture.OWNER+"."+capture.TABLE, "OO_CONV_FROZEN27.IUD_4000_4000_2000")
        self.assertNotEqual(capture.RUN, "c1")

if __name__ == "__main__":
    unittest.main()
