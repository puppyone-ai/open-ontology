import unittest

from whole import BANK, GOLD_CELL, PRIOR_SEMANTICS, compose_current_semantics


class CurrentSemanticsTest(unittest.TestCase):
    def test_next_random_bank_has_fresh_semantics_and_live_gold_keys(self):
        ids = {item['semantic_id'] for item in BANK}
        self.assertEqual(len(ids), len(BANK))
        self.assertFalse(ids & set(PRIOR_SEMANTICS))
        self.assertEqual(set(GOLD_CELL), {'uniq_id1_label', 'part_20260422_amount',
                                          'part_20260311_amount', 'agg_m_qty',
                                          'agg_population', 'str_population'})
        self.assertTrue({item['truth_key'] for item in BANK} <= set(GOLD_CELL))

    def test_reports_mixed_current_methods_from_generation_bound_receipts(self):
        target = {'part_day': (16, 28), 'dup_events': (17, 29),
                  'uniq_acct': (18, 30), 'nz_obs': (19, 31),
                  'agg_qty': (1, 2), 'dec_exact': (1, 4), 'str_key': (1, 6)}
        profile = {'table_generations': {
            'oo_dl01w1_43.' + name: {'source_generation': source, 'target_generation': dest}
            for name, (source, dest) in target.items()}}
        receipts = {}
        for name, method, (source, dest) in (
            ('part_day', 'c4_partition', target['part_day']),
            ('dup_events', 'c5_diff', target['dup_events']),
            ('uniq_acct', 'c5_diff', target['uniq_acct']),
            ('nz_obs', 'c5_diff', target['nz_obs']),
        ):
            request = {'logical_table': name, 'source_generation': source,
                       'capture_method': method, 'full_snapshot': method == 'c5_diff',
                       'delete_detection': 'partition_replace' if method == 'c4_partition' else 'reconcile'}
            loaded = {'logical_table': name, 'source_generation': source, 'target_generation': dest}
            if name == 'part_day':
                loaded['partition_target_receipt'] = {'partition_scope': {'partition_id': 'p202604'}}
            receipts[name] = request, loaded

        result = compose_current_semantics(profile, receipts)['tables']
        self.assertEqual(result['part_day']['capture_method'], 'C4')
        self.assertEqual(result['part_day']['partition_id'], 'p202604')
        self.assertEqual({name for name, value in result.items() if value['capture_method'] == 'C5'},
                         {'dup_events', 'uniq_acct', 'nz_obs'})
        self.assertEqual({name for name, value in result.items() if value['capture_method'] == 'static'},
                         {'agg_qty', 'dec_exact', 'str_key'})

    def test_rejects_a_method_claim_without_matching_receipt(self):
        profile = {'table_generations': {
            'oo_dl01w1_43.' + name: {'source_generation': source, 'target_generation': dest}
            for name, (source, dest) in {
                'part_day': (16, 28), 'dup_events': (17, 29), 'uniq_acct': (18, 30), 'nz_obs': (19, 31),
                'agg_qty': (1, 2), 'dec_exact': (1, 4), 'str_key': (1, 6)}.items()}}
        receipts = {
            name: ({'logical_table': name, 'source_generation': source,
                    'capture_method': 'c5_diff', 'full_snapshot': True, 'delete_detection': 'reconcile'},
                   {'logical_table': name, 'source_generation': source, 'target_generation': dest})
            for name, source, dest in [('part_day', 16, 28), ('dup_events', 17, 29),
                                       ('uniq_acct', 18, 30), ('nz_obs', 19, 31)]}
        with self.assertRaisesRegex(RuntimeError, 'semantics_c4'):
            compose_current_semantics(profile, receipts)


if __name__ == '__main__':
    unittest.main()
