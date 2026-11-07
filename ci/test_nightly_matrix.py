import unittest

from nightly_matrix import summarize


class StabilityTests(unittest.TestCase):
    def test_three_identical_failures_are_not_stable(self):
        row = {"target": "mysql8-selfmanaged", "version": "8.4", "suite": "T7",
               "variant": None, "result": "fail"}
        summary = summarize([{"exit": 1, "rows": [row]}] * 3)
        self.assertTrue(summary["repeatable_outcomes"])
        self.assertFalse(summary["stable"])
        self.assertFalse(summary["consecutive_nights"])

    def test_empty_or_skipped_run_cannot_pass(self):
        self.assertFalse(summarize([])["stable"])
        self.assertFalse(summarize([{"exit": 0, "rows": []}])["stable"])
        row = {"target": "pg", "version": "17", "suite": "T0", "result": "skip"}
        self.assertFalse(summarize([{"exit": 0, "rows": [row]}])["stable"])


if __name__ == "__main__":
    unittest.main()
