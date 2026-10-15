import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


sys.path.insert(0, str(Path(__file__).parent))
import connection_cycle


class ConnectionCycleTest(unittest.TestCase):
    def test_new_cycle_skips_failed_historical_pins(self):
        def query(database, sql):
            if "jobs.tasks" in sql:
                return "0"
            if "row_to_json(s)" in sql:
                return json.dumps({"source_generation": 5, "pin": 5})
            if "source_progress" in sql:
                return ""
            if "pending_acks" in sql:
                return "0"
            if "max(pin)" in sql:
                return "7"
            raise AssertionError(sql)

        with tempfile.TemporaryDirectory() as temporary, patch.object(connection_cycle, "query", side_effect=query):
            state = connection_cycle.begin(Path(temporary), "insert-update-delete")

        self.assertEqual(state["generation"], 6)
        self.assertEqual(state["pin"], 8)

    def test_new_cycle_refuses_until_official_recovery_releases_fence(self):
        def query(database, sql):
            if "jobs.tasks" in sql:
                return "0"
            if "row_to_json(s)" in sql:
                return json.dumps({"source_generation": 5, "pin": 5})
            if "pending_acks" in sql:
                return "0"
            if "source_progress" in sql:
                return "c304902a-4de5-4b7b-8eef-e329906f3aec"
            raise AssertionError(sql)

        with tempfile.TemporaryDirectory() as temporary, patch.object(connection_cycle, "query", side_effect=query):
            with self.assertRaisesRegex(RuntimeError, "official C1 recovery or ACK is pending"):
                connection_cycle.begin(Path(temporary), "insert-update-delete")


if __name__ == "__main__":
    unittest.main()
