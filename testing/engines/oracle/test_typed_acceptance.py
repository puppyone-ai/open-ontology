import json
import unittest
from unittest.mock import patch

import typed_acceptance as acceptance


class TypedComparisonTests(unittest.TestCase):
    def test_exact_json_binary_and_utc_instant(self):
        class Source:
            def sql(self, query):
                if "all_tab_columns" in query:
                    return 0, "N|NUMBER|38|10\nB|BLOB|-|-\nT|TIMESTAMP(6) WITH TIME ZONE|-|-"
                self.query = query
                return 0, ""

        class Target:
            def sql(self, query):
                if "pg_attribute" in query:
                    return "N|numeric(38,10)\nB|bytea\nT|timestamp with time zone"
                return '{"N":12345678901234567890.1234567890,"B":"\\\\x00ff","T":null}'

        source = Source()

        def compare(adapted_source, adapted_target, receipt, table):
            row = json.loads(adapted_target.sql("SELECT"))
            self.assertEqual(row["N"], "12345678901234567890.1234567890")
            self.assertEqual(row["B"], "00ff")
            self.assertIsNone(row["T"])
            adapted_source.sql('SELECT TO_CHAR("T",\'fmt\')')
            self.assertIn('"T" AT TIME ZONE \'UTC\'', source.query)
            return {"equal": True}

        with patch.object(acceptance, "original_compare", compare):
            result = acceptance.compare_typed(source, Target(), {"target": "fixture"}, "TYPE_EDGES")
        self.assertTrue(result["typed_schema_equal"])

    def test_text_instead_of_numeric_is_rejected_before_value_comparison(self):
        class Source:
            def sql(self, query):
                return 0, "N|NUMBER|18|3"

        class Target:
            def sql(self, query):
                return "N|text"

        with self.assertRaisesRegex(RuntimeError, "typed target schema mismatch"):
            acceptance.compare_typed(Source(), Target(), {"target": "fixture"}, "T")


if __name__ == "__main__":
    unittest.main()
