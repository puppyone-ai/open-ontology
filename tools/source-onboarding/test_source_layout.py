import unittest
import source_layout as m


class LayoutTests(unittest.TestCase):
    def table(self, name='t_items', **kw):
        return dict(schema='sample', table=name, estimated_rows=0, exact_rows=100,
                    primary_key=[{'name': 'id', 'type': 'bigint'}], **kw)

    def test_domain_split_and_unique_ownership(self):
        tables = [self.table(f't_items_{i}', domain='catalog') for i in range(35)]
        plan = m.plan(tables)
        self.assertEqual([len(s['tables']) for s in plan['sources']], [16, 16, 3])
        self.assertEqual(len({t for s in plan['sources'] for t in s['tables']}), 35)
        self.assertEqual(len({s['source_id'] for s in plan['sources']}), 3)
        self.assertEqual(plan, m.plan(list(reversed(tables))))

    def test_empty_without_key_only_excluded_after_count(self):
        t = self.table(); t['primary_key'] = []; t['exact_rows'] = 0
        self.assertEqual(m.plan([t])['exclusions'][0]['reason'], 'empty_without_key')
        t['exact_rows'] = None
        self.assertRaisesRegex(ValueError, 'exact_count_required', m.plan, [t])

    def test_chunk_and_budget(self):
        t = self.table(); t['primary_key'].append({'name': 'day', 'type': 'date'})
        entry = m.plan([t])['manifests']['s001']['tables'][0]
        self.assertTrue(entry['chunking'])
        self.assertEqual(entry['budgets']['max_rows'], 100000)
        self.assertLessEqual(entry['budgets']['deadline_ms'], 86400000)
        t['exact_rows'] = 0
        self.assertTrue(m.plan([t])['manifests']['s001']['tables'][0]['chunking'])

    def test_unpublished_group_and_limits(self):
        ts = [self.table(f't_log_{i}', modeled=False) for i in range(40)]
        self.assertEqual([len(s['tables']) for s in m.plan(ts)['sources']], [32, 8])
        for args in [(17, 32), (16, 33), (0, 32)]:
            self.assertRaises(ValueError, m.plan, ts, *args)

    def test_invalid_and_duplicate_inputs(self):
        t = self.table()
        self.assertRaisesRegex(ValueError, 'duplicate_table', m.plan, [t, t])
        t['exact_rows'] = -1
        self.assertRaises(ValueError, m.plan, [t])
        t['exact_rows'] = True
        self.assertRaises(ValueError, m.plan, [t])

    def test_preflight_does_not_hide_blockers(self):
        t = self.table(columns=[{'name': 'body', 'type': 'longtext'}] * 1025)
        t['primary_key'] = [{'name': 'id', 'type': 'float'}]
        report = m.preflight({'session': {'sql_mode': '', 'time_zone': '+08:00',
                              'have_ssl': 'DISABLED', 'log_bin': 'OFF',
                              'privileges': ['SELECT', 'UPDATE']}, 'tables': [t]})
        self.assertFalse(report['importable'])
        for code in ['session_sql_mode', 'session_timezone', 'plaintext_approval_required',
                     'cdc_unavailable', 'write_capable_grant', 'wide_table', 'lob',
                     'unsupported_key', 'estimate_unreliable']:
            self.assertIn(code, [f['code'] for f in report['findings']])

    def test_missing_facts_are_not_pass(self):
        r = m.preflight({'tables': [self.table()]})
        self.assertFalse(r['importable'])
        self.assertIn('session_facts_missing', [f['code'] for f in r['findings']])


if __name__ == '__main__':
    unittest.main()
