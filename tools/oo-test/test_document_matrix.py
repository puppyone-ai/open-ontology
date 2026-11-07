import unittest
import json
from pathlib import Path
from tempfile import TemporaryDirectory
from types import SimpleNamespace

from document_matrix import result
from verdicts import write_rows


class DocumentMatrixTests(unittest.TestCase):
    def test_source_success_cannot_promote_product_acceptance(self):
        for engine in ("mongodb", "opensearch", "elasticsearch"):
            report = result(engine, 0)
            self.assertEqual(report["source_checks"], "passed")
            self.assertFalse(report["product_t0"])
            self.assertEqual(set(report["suites"].values()), {"not_accepted"})

    def test_source_failure_is_not_hidden(self):
        self.assertEqual(result("mongodb", 1)["source_checks"], "failed")

    def test_product_results_are_not_inferred_from_exit(self):
        report = result("opensearch", 0, {"T1": "passed", "T2": "failed"})
        self.assertEqual(report["suites"]["T1"], "passed")
        self.assertEqual(report["suites"]["T2"], "failed")
        self.assertFalse(report["product_t0"])

    def test_mongodb_c5_does_not_accept_c1(self):
        report = result("mongodb", 0, {"T2": "passed", "C5": "passed"})
        self.assertEqual(report["suites"]["T2"], "not_accepted")

    def test_startup_failure_still_writes_unmeasured_verdicts(self):
        with TemporaryDirectory() as directory:
            raw = Path(directory) / "summary.json"
            raw.write_text(json.dumps({"failure": "startup"}))
            for engine in ("mongodb", "opensearch", "elasticsearch"):
                run = SimpleNamespace(engine=engine, version="8.0.34",
                                      id="fixture", receipt={})
                rows = write_rows(run, raw, [("T1", "snapshot", "pass")])
                self.assertEqual(rows[0]["result"], "fail")
                self.assertFalse(rows[0]["version_measured"])


if __name__ == "__main__":
    unittest.main()
