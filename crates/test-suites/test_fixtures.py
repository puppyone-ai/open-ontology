import unittest
from unittest.mock import patch
import fixtures


class InventoryTests(unittest.TestCase):
    def test_live_and_evidence_are_classified_but_product_tests_are_not(self):
        rows = fixtures.inventory()
        self.assertEqual(fixtures.select(rows, 'oo-connectors', 'live_source_catalog')['suite'], 'live')
        self.assertEqual(fixtures.select(rows, 'oo-pipeline', 'starrocks_standard_types_decode_without_rewriting_tokens')['suite'], 'evidence')
        self.assertEqual(fixtures.select(rows, 'oo-gateway', 'two_legal_calls_are_two_children')['suite'], 'platform')
        with self.assertRaises(ValueError):
            fixtures.select(rows, 'oo-pipeline', 'synthetic_standard_types_preserve_precision_nulls_and_timestamp_units')
        with self.assertRaises(ValueError):
            fixtures.select(rows, 'oo-connectors', 'typo')

    def test_ambiguous_selection_is_not_an_all_fixtures_run(self):
        row = dict(package='p', name='test')
        with self.assertRaises(ValueError):
            fixtures.select([row, row], 'p', 'test')

    def test_list_does_not_build_or_connect(self):
        with patch('fixtures.subprocess.run', side_effect=AssertionError('must not execute')):
            self.assertGreater(len(fixtures.inventory()), 0)


if __name__ == '__main__':
    unittest.main()
