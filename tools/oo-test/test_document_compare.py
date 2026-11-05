import unittest

from document_compare import compare_documents, document


class DocumentComparisonTests(unittest.TestCase):
    def compare(self, a, b):
        return compare_documents(a, b, document_column=2)["equal"]

    def test_jsonb_key_order_and_exact_numbers(self):
        self.assertTrue(self.compare(
            [["1", 3, '{"n":12345678901234567890.123456789,"x":null}']],
            [["1", 3, '{"x":null,"n":12345678901234567890.1234567890}']]))
        self.assertFalse(self.compare(
            [["1", 3, '{"n":12345678901234567890.123456789}']],
            [["1", 3, '{"n":12345678901234567890.123456788}']]))

    def test_missing_null_and_scalar_types_are_not_conflated(self):
        self.assertFalse(self.compare([["1", None, "{}"]], [["1", None, '{"x":null}']]))
        self.assertFalse(self.compare([["1", 1, "{}"]], [["1", "1", "{}"]]))
        self.assertFalse(self.compare(
            [["1", 1, '{"n":{"$numberLong":"1"}}']],
            [["1", 1, '{"n":{"$numberInt":"1"}}']]))

    def test_invalid_or_ambiguous_documents_and_ids_fail_closed(self):
        for text in ('{"x":1,"x":2}', '{"n":NaN}', '[]', 'null'):
            with self.assertRaises(ValueError):
                document(text)
        for rows in ([["1", 1, "{}"], ["1", 2, "{}"]],
                     [["2", 1, "{}"], ["1", 1, "{}"]]):
            with self.assertRaises(ValueError):
                self.compare(rows, rows)


if __name__ == "__main__":
    unittest.main()
