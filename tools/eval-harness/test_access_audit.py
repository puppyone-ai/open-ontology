import unittest

from access_audit import expected_columns, inspect_table


class AccessAuditTests(unittest.TestCase):
    def test_exact_zero_row_query_checks_real_column_access(self):
        seen = []
        def invoke(args):
            seen.append(args)
            return 0, {'columns': [{'name': 'id'}, {'name': 'contact'}], 'rows': [],
                       'returned_rows': 0, 'truncated': False, 'query_id': 'q_actual',
                       'knowledge_version': 'v2', 'policy_version': 'v2'}
        result = inspect_table('app.customers', {'id', 'contact'}, invoke)
        self.assertTrue(result['passed'])
        self.assertEqual(seen, [['query', '--json', 'SELECT * FROM "app"."customers" LIMIT 0']])

    def test_hidden_columns_errors_and_accidental_rows_never_pass(self):
        base = {'columns': [{'name': 'id'}], 'rows': [], 'returned_rows': 0,
                'truncated': False, 'query_id': 'q_actual'}
        self.assertFalse(inspect_table('app.t', {'id', 'secret'}, lambda _: (0, base))['passed'])
        for change in [{'code': 'COLUMN_NOT_ALLOWED'}, {'truncated': True},
                       {'rows': [['private-value']], 'returned_rows': 1}, {'query_id': None}]:
            result = inspect_table('app.t', {'id'}, lambda _: (0, dict(base, **change)))
            self.assertFalse(result['passed'])
            self.assertNotIn('private-value', str(result))

    def test_expected_scope_cannot_be_empty_or_duplicated(self):
        for tables in [[], [{'dataset': 'app.t', 'columns': []}],
                       [{'dataset': 'app.t', 'columns': [{'name': 'id'}, {'name': 'id'}]}]]:
            with self.assertRaises(ValueError):
                expected_columns({'tables': tables})


if __name__ == '__main__':
    unittest.main()
