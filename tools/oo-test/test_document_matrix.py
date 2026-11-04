import unittest

from document_matrix import result


class DocumentMatrixTests(unittest.TestCase):
    def test_source_success_cannot_promote_product_acceptance(self):
        for engine in ("mongodb", "opensearch", "elasticsearch"):
            report = result(engine, 0)
            self.assertEqual(report["source_checks"], "passed")
            self.assertFalse(report["product_t0"])
            self.assertEqual(set(report["suites"].values()), {"not_accepted"})

    def test_source_failure_is_not_hidden(self):
        self.assertEqual(result("mongodb", 1)["source_checks"], "failed")


if __name__ == "__main__":
    unittest.main()
