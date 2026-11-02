import copy
import json
import unittest
from unittest.mock import patch

import library_metrics
import library_selfbuild as lib
import report_semantics as reports
from test_library_metrics import fixture


class ReportSemanticTest(unittest.TestCase):
    def facts(self, sql=None):
        facts, _ = fixture()
        facts['report_definitions_observation'] = {
            'dataset': 'app.report_config', 'scope': {'tenant_id': 'example'},
            'definitions': [{
                'active': '1', 'module_name': 'Operations report',
                'sql_text': sql or """SELECT date_format(event_date, '%Y-%m-%d') AS eventDate,
                    SUM(s.weight - s.id) AS netWeight, COUNT(*) AS records
                    FROM shipments AS s LEFT JOIN other ON s.id=other.id""",
                'data_list_config': json.dumps([
                    {'prop': 'netWeight', 'label': 'Net measure'},
                    {'prop': 'records', 'label': 'Visible record count'},
                    {'prop': 'eventDate', 'label': 'Event date'}]),
            }],
        }
        return facts

    def test_report_label_and_formula_survive_as_uncertified_bound_proposals(self):
        facts = self.facts()
        derived = reports.derive(facts)
        self.assertEqual(len(derived['metrics']), 2)
        net = next(m for m in derived['metrics'] if m['label'] == 'Net measure')
        self.assertEqual(net['sql'], 'SELECT SUM(weight - id) AS ' + net['metric'] + ' FROM app.shipments')
        self.assertEqual(net['time_dimension'], 'app.shipments.event_date')
        self.assertNotIn('other', net['sql'])
        merged = lib.merge_drafts(facts, [])
        self.assertEqual(len(merged['metric_candidates']), 2)
        self.assertTrue(all(not c['definition']['certified'] for c in merged['metric_candidates']))
        self.assertEqual(derived['audit'][0]['state'], 'candidates_extracted')
        self.assertEqual(len(derived['audit']), 1)
        self.assertEqual(derived['audit'][0]['source_field'], 'sql_text')
        row = facts['report_definitions_observation']['definitions'][0]
        self.assertEqual(derived['audit'][0]['report_sha256'], reports.digest(row))
        expected_name = 'report_' + reports.digest({
            'dataset': 'app.shipments', 'alias': 'netWeight', 'formula': 'SUM(weight - id)',
            'module': 'Operations report', 'label': 'Net measure'})[:20]
        self.assertEqual(net['metric'], expected_name)

    def test_only_total_sql_is_extracted_with_bound_source_evidence(self):
        for primary in (None, ''):
            with self.subTest(primary=primary):
                facts = self.facts()
                row = facts['report_definitions_observation']['definitions'][0]
                row['total_sql_text'], row['sql_text'] = row['sql_text'], primary
                original = copy.deepcopy(facts)
                derived = reports.derive(facts)
                self.assertEqual(len(derived['metrics']), 2)
                audit = {a['source_field']: a for a in derived['audit']}
                self.assertEqual(audit['sql_text']['reason'], 'missing_sql_definition')
                self.assertEqual(audit['total_sql_text']['state'], 'candidates_extracted')
                self.assertEqual(audit['total_sql_text']['report_sha256'], reports.digest(row))
                self.assertEqual({p['metric'] for p in audit['total_sql_text']['proposed']},
                                 {m['metric'] for m in derived['metrics']})
                merged = lib.merge_drafts(facts, [])
                self.assertEqual(merged['report_semantics'], derived['audit'])
                self.assertTrue(all(not c['definition']['certified']
                                    for c in merged['metric_candidates']))
                self.assertEqual(facts, original)

    def test_duplicate_primary_and_total_share_metrics_but_keep_both_sources(self):
        facts = self.facts()
        expected = reports.derive(facts)['metrics']
        row = facts['report_definitions_observation']['definitions'][0]
        row['total_sql_text'] = row['sql_text']
        derived = reports.derive(facts)
        self.assertEqual(derived['metrics'], expected)
        self.assertEqual([a['source_field'] for a in derived['audit']],
                         ['sql_text', 'total_sql_text'])
        for entry in derived['audit']:
            self.assertEqual(entry['report_sha256'], reports.digest(row))
            self.assertEqual(entry['state'], 'candidates_extracted')
            self.assertEqual(entry['omitted'], [])
            self.assertEqual({p['metric'] for p in entry['proposed']},
                             {m['metric'] for m in expected})
        self.assertEqual(len(lib.merge_drafts(facts, [])['metric_candidates']), 2)

    def test_total_does_not_inherit_primary_time_dimension_or_conflict(self):
        facts = self.facts()
        primary = reports.derive(facts)['metrics']
        row = facts['report_definitions_observation']['definitions'][0]
        row['total_sql_text'] = 'SELECT SUM(s.weight - s.id) AS netWeight FROM shipments s'
        derived = reports.derive(facts)
        self.assertEqual(len(derived['metrics']), 3)
        for metric in primary:
            self.assertIn(metric, derived['metrics'])
        total = derived['audit'][1]
        self.assertEqual(total['omitted'], [])
        total_metric = next(m for m in derived['metrics']
                            if m['metric'] == total['proposed'][0]['metric'])
        self.assertNotIn('time_dimension', total_metric)
        self.assertEqual(total_metric['sql'], 'SELECT SUM(weight - id) AS ' +
                         total_metric['metric'] + ' FROM app.shipments')
        self.assertEqual(len(lib.merge_drafts(facts, [])['metric_candidates']), 3)

    def test_different_total_formula_with_same_alias_is_not_deduplicated(self):
        facts = self.facts()
        row = facts['report_definitions_observation']['definitions'][0]
        row['total_sql_text'] = 'SELECT SUM(weight) AS netWeight FROM shipments'
        derived = reports.derive(facts)
        self.assertEqual(len(derived['metrics']), 3)
        self.assertEqual(derived['audit'][1]['proposed'][0]['formula'], 'SUM(weight)')
        self.assertEqual(derived['audit'][1]['omitted'], [])

    def test_unparseable_branch_does_not_discard_the_other_branch(self):
        valid = 'SELECT COUNT(*) AS records FROM shipments'
        for source_field in ('sql_text', 'total_sql_text'):
            other = 'total_sql_text' if source_field == 'sql_text' else 'sql_text'
            for invalid in ('SELECT SUM(', 'SELECT SUM(weight) AS netWeight',
                            'SUM(weight) AS netWeight', 'not SQL', 123, {'sql': valid}):
                with self.subTest(source_field=source_field, invalid=invalid):
                    facts = self.facts(valid)
                    row = facts['report_definitions_observation']['definitions'][0]
                    row.update({source_field: invalid, other: valid})
                    derived = reports.derive(facts)
                    self.assertEqual(len(derived['metrics']), 1)
                    audit = {a['source_field']: a for a in derived['audit']}
                    self.assertEqual(audit[source_field]['state'], 'unsupported_report')
                    self.assertEqual(audit[source_field]['report_sha256'], reports.digest(row))
                    self.assertEqual(audit[other]['state'], 'candidates_extracted')

    def test_total_alias_requires_exact_ui_label_binding(self):
        facts = self.facts()
        row = facts['report_definitions_observation']['definitions'][0]
        row['sql_text'] = None
        row['total_sql_text'] = 'SELECT SUM(weight) AS net_weight FROM shipments'
        derived = reports.derive(facts)
        self.assertEqual(derived['metrics'], [])
        self.assertEqual(derived['audit'][1]['state'], 'no_supported_aggregates')
        self.assertEqual(derived['audit'][1]['omitted'][0]['reason'], 'aggregate_label_missing')

    def test_untrusted_report_sql_is_not_executed_or_copied_as_a_query(self):
        for sql in [
            'SELECT SUM(weight) AS netWeight FROM shipments; DELETE FROM shipments',
            'SELECT SUM(weight) AS netWeight INTO stolen FROM shipments',
            'SELECT SUM(other.weight) AS netWeight FROM shipments s JOIN other ON s.id=other.id',
            'SELECT SUM(abs(weight)) AS netWeight FROM shipments',
            'SELECT SUM(missing) AS netWeight FROM shipments',
            "SELECT 'SUM(weight) AS netWeight' AS payload FROM shipments",
            'SELECT SUM(weight) AS netWeight FROM private_table',
            'SELECT SUM(weight) AS netWeight FROM shipments UNION SELECT 1',
            'SELECT SUM(weight) AS netWeight FROM shipments /*! INTO stolen */',
        ]:
            for source_field in ('sql_text', 'total_sql_text'):
                with self.subTest(sql=sql, source_field=source_field), \
                        patch.object(lib.ReadOnlyPostgres, 'query',
                                     side_effect=AssertionError('SQL must not execute')) as query:
                    facts = self.facts()
                    row = facts['report_definitions_observation']['definitions'][0]
                    row['sql_text'] = None
                    row[source_field] = sql
                    self.assertEqual(reports.derive(facts)['metrics'], [])
                    query.assert_not_called()

    def test_inactive_and_conflicting_labels_are_not_silently_approved(self):
        facts = self.facts()
        row = facts['report_definitions_observation']['definitions'][0]
        row.update(active='0', total_sql_text=row['sql_text'])
        self.assertEqual(reports.derive(facts)['metrics'], [])
        facts = self.facts()
        row = facts['report_definitions_observation']['definitions'][0]
        row['total_sql_text'] = row['sql_text']
        row['data_list_config'] = json.dumps([
            {'prop': 'netWeight', 'label': 'Net measure'}, {'prop': 'netWeight', 'label': 'Other meaning'}])
        self.assertEqual(reports.derive(facts)['metrics'], [])

    def test_amendment_creates_new_candidate_and_requires_fresh_separate_approval(self):
        facts = self.facts()
        items = lib.merge_drafts(facts, [])['metric_candidates']
        original = items[0]
        definition = {k: v for k, v in original['definition'].items() if k not in library_metrics.CERTIFICATION}
        definition['unit'] = 'verified fixture unit'
        change = {definition['metric']: {
            'base_candidate_sha256': original['candidate_sha256'], 'definition': definition,
            'by': 'fixture/operator', 'reason': 'Independent fixture metadata verified unit.',
            'evidence_sha256': 'f'*64}}
        amended, audit = library_metrics.amend_candidates(facts['tables'], items, change)
        revised = next(c for c in amended if c['definition']['metric'] == definition['metric'])
        self.assertNotEqual(revised['candidate_sha256'], original['candidate_sha256'])
        self.assertFalse(revised['definition']['certified'])
        self.assertEqual(revised['definition']['status'], 'draft')
        self.assertFalse(audit[0]['certified'])
        self.assertNotIn('unit', original['definition'])
        for mutate in [
            lambda c: c.update(base_candidate_sha256='0'*64),
            lambda c: c['definition'].update(certified=True),
            lambda c: c['definition'].update(owner='pretend-reviewer'),
        ]:
            bad = copy.deepcopy(change)
            mutate(bad[definition['metric']])
            with self.assertRaises(ValueError):
                library_metrics.amend_candidates(facts['tables'], items, bad)


if __name__ == '__main__':
    unittest.main()
