import json
from pathlib import Path
import sys
import tempfile
import unittest

import bounded_source as b
sys.path.insert(0, str(Path(__file__).resolve().parents[2]/"testing/dl01-targets/oracle/scripts"))
import execute_frozen_iud as execution

class FrozenExecutionTests(unittest.TestCase):
    def test_no_gate_or_wrong_source_never_reaches_source(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp); ready = root/"ready.json"; receipt = root/"root.json"
            b.atomic(ready, b.canonical({"source_id": execution.SOURCE}))
            b.atomic(receipt, b.canonical({"baseline_ready_sha256": "0"*64}))
            with self.assertRaisesRegex(ValueError, "root_readiness"):
                execution.verify_gate(ready, receipt)
            b.atomic(ready, b.canonical({"source_id": "old_C1_EVENTS"}))
            b.atomic(receipt, b.canonical({"baseline_ready_sha256": b.sha(ready), "source_id": execution.SOURCE}))
            with self.assertRaisesRegex(ValueError, "same_source_gate"):
                execution.verify_gate(ready, receipt)

    def test_only_one_ready_lane_is_not_enough(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp); ready = root/"ready.json"; receipt = root/"root.json"
            b.atomic(ready, b.canonical({"source_id": execution.SOURCE,
                "mutation_owner": execution.MUTATION_OWNER.removeprefix("oracle-owner-"),
                "target_baselines_confirmed": {"C1": True, "C5": False}}))
            b.atomic(receipt, b.canonical({"baseline_ready_sha256": b.sha(ready), "source_id": execution.SOURCE}))
            with self.assertRaisesRegex(ValueError, "same_source_gate"):
                execution.verify_gate(ready, receipt)

if __name__ == "__main__":
    unittest.main()
