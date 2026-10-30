import copy
import unittest
from unittest.mock import Mock

import measure_reconciliation as measures
from test_library_metrics import fixture


class MeasureReconciliationTest(unittest.TestCase):
    def setUp(self):
        self.facts, _ = fixture()
        self.spec = {'metric': 'recorded_weight', 'dataset': 'app.shipments', 'key': 'id',
                     'scope': {'id': '12'}, 'kind': 'sum',
                     'components': [{'column': 'weight', 'sign': 1}],
                     'time_column': 'event_date', 'period': {'start': '2026-09-01', 'end': '2026-10-01'}}

    def test_two_different_calculations_share_one_snapshot_and_explicit_scope(self):
        logical, native = measures.statements(self.facts, self.spec)
        self.assertIn('SUM("weight")', logical)
        self.assertIn(">=TIMESTAMP '2026-09-01'", logical)
        self.assertIn("<TIMESTAMP '2026-10-01'", logical)
        self.assertIn('WITH raw AS MATERIALIZED', native)
        self.assertIn('SUM(c0 * n)', native)
        self.assertIn('GROUP BY c0', native)
        self.assertIn('pg_current_snapshot()', native)
        self.assertNotEqual(logical, native)

    def test_exact_comparison_does_not_round_large_amounts_or_empty_sums(self):
        result = {'records': 1, 'distinct_keys': 1, 'null_keys': 0, 'null_measure_rows': 0,
                  'snapshot': '1:2:', 'observed_at': '2026-10-23',
                  'value': '9007199254740993.01', 'independent_value': '9007199254740993.02'}
        db = Mock(query=Mock(return_value=result))
        report = measures.reconcile(db, self.facts, self.spec)
        self.assertFalse(report['arithmetic_matches'])
        self.assertTrue(report['key_unique_in_scope'])
        self.assertFalse(report['gateway_snapshot_comparable'])
        result.update(value=None, independent_value=None, records=0, distinct_keys=0)
        report = measures.reconcile(db, self.facts, self.spec)
        self.assertTrue(report['arithmetic_matches'])
        self.assertIsNone(report['result']['value'])

    def test_bad_scope_types_keys_signs_and_periods_are_not_guessed(self):
        for change in [
            {'scope': {}}, {'key': 'weight'}, {'period': {'start': '2026-10-01', 'end': '2026-09-01'}},
            {'time_column': 'weight'}, {'components': [{'column': 'weight', 'sign': True}]},
            {'components': [{'column': 'weight', 'sign': 2}]},
            {'components': [{'column': 'hidden', 'sign': 1}]}, {'kind': 'raw_rows'},
        ]:
            with self.subTest(change=change), self.assertRaises(ValueError):
                measures.statements(self.facts, dict(self.spec, **change))
        bad = copy.deepcopy(self.facts)
        next(c for c in bad['tables'][0]['columns'] if c['name'] == 'weight')['data_type'] = 'double precision'
        with self.assertRaisesRegex(ValueError, 'exact'):
            measures.statements(bad, self.spec)

    def test_distinct_oracle_groups_keys_instead_of_repeating_same_count(self):
        spec = dict(self.spec, metric='record_count', kind='count_distinct', components=[])
        logical, native = measures.statements(self.facts, spec)
        self.assertIn('COUNT(DISTINCT "id")', logical)
        self.assertIn('event_key IS NOT NULL GROUP BY event_key', native)


if __name__ == '__main__':
    unittest.main()
