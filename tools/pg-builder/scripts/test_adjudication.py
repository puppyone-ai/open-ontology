"""Boundary rules and the shared graph contract. No source connection."""
import copy
import hashlib
import json
from pathlib import Path
import unittest

import adjudication as a
import retained_build
from test_retained_build import Db, cut, facts


class Adjudication(unittest.TestCase):
    def test_optional_query_history_failure_is_not_empty_measured_history(self):
        class Db:
            calls = 0
            def query(self, sql):
                self.calls += 1
                if self.calls == 1:
                    return ['SELECT a.id FROM app.a JOIN app.b ON app.a.id=app.b.id']
                if self.calls == 2:
                    return {'schema':'public', 'readable':True}
                raise RuntimeError('read_only_aggregate_query_failed:55000')
        definitions, status = a.join_sources(Db())
        self.assertEqual(len(definitions), 1)
        self.assertEqual(status, 'query_failed')

    def test_code_coverage_boundary_is_not_vacuously_complete(self):
        self.assertEqual(a.code_decision(100, 99, 3, a.thresholds())[0], 'complete')
        self.assertEqual(a.code_decision(100, 98, 3, a.thresholds())[0], 'incomplete')
        self.assertEqual(a.code_decision(0, 0, 0, a.thresholds())[0], 'incomplete')
        self.assertEqual(a.code_decision(100, 100, 1001, a.thresholds())[0], 'incomplete')

    def test_view_join_aliases_require_real_equality_not_literals(self):
        pairs = [('app.orders.customer_id', 'app.customers.id')]
        query = 'SELECT o.id FROM app.orders o JOIN app.customers c ON o.customer_id = c.id'
        self.assertTrue(a.qualified_joins(query, pairs))
        self.assertFalse(a.qualified_joins(query.replace('o.customer_id = c.id', 'o.id = c.id'), pairs))
        self.assertFalse(a.qualified_joins(
            "SELECT 'app.orders.customer_id = app.customers.id' FROM app.orders", pairs))
        self.assertFalse(a.qualified_joins(
            "SELECT $$app.orders.customer_id = app.customers.id$$ FROM app.orders", pairs))

    def test_relation_thresholds_use_row_containment_and_measured_cardinality(self):
        base = dict(containment=.98, parent_unique=True, name_similarity=0,
                    join_cooccurrence=0, type_compatible=True, length_compatible=True,
                    rows_checked=100)
        for patch, expected in [({}, 'adopted'), ({'containment':.979}, 'rejected'),
                ({'containment':.9, 'name_similarity':.5}, 'adopted'),
                ({'containment':.9, 'join_cooccurrence':1}, 'adopted'),
                ({'containment':.899, 'join_cooccurrence':10}, 'rejected'),
                ({'parent_unique':False}, 'rejected'),
                ({'type_compatible':False}, 'rejected'),
                ({'length_compatible':False}, 'rejected'), ({'rows_checked':0}, 'rejected')]:
            self.assertEqual(a.relation_decision(dict(base, **patch), a.thresholds())[0], expected)

    def test_negative_name_only_profile_is_visible_but_unknown_is_not(self):
        profile = dict(rows=100, non_null=100, distinct=2, phone=0, id_card=0, email=0, bank_card=0)
        self.assertEqual(a.sensitive_decision(profile, True, a.thresholds())[0], 'visible')
        for field in ('phone', 'id_card', 'email', 'bank_card'):
            self.assertEqual(a.sensitive_decision(dict(profile, **{field:50}), False,
                                                 a.thresholds())[0], 'hidden')
        self.assertEqual(a.sensitive_decision(dict(profile, non_null=0, distinct=0), True,
                                             a.thresholds())[0], 'hidden')

    def test_full_scan_output_validates_against_json_schema(self):
        import jsonschema
        tables = facts(cut(1))['tables']
        command = retained_build.review_input(tables, {'graph':{'edges':[]}}, 'test')
        raw = json.dumps(command).encode()
        result = a.collect(Db(cut(1)), command, tables, hashlib.sha256(raw).hexdigest())
        schema = json.loads((Path(__file__).resolve().parents[3] /
                             'crates/oo-builder/tests/adjudication.schema.json').read_text())
        jsonschema.Draft202012Validator.check_schema(schema)
        jsonschema.validate(result, schema)
        self.assertEqual(result['summary_sha256'], a.summary_hash(result))
        self.assertEqual(result['graph']['entities'][0]['key'], ['amount'])
        for mutate in [lambda x:x.pop('graph'), lambda x:x['items'][0].update(decision='hidden'),
                       lambda x:x['graph']['entities'][0].update(rows=-1)]:
            bad = copy.deepcopy(result)
            mutate(bad)
            with self.assertRaises(jsonschema.ValidationError):
                jsonschema.validate(bad, schema)

    def test_invalid_entity_becomes_plain_dataset_and_metric_becomes_example(self):
        tables = facts(cut(1, rows=2), rows=2, distinct=1)['tables']
        command = retained_build.review_input(tables, {'graph':{'edges':[]}}, 'test')
        command['effective']['metrics'] = [{'metric':'unreconciled', 'sql':'not executable'}]
        result = a.collect(Db(cut(1, rows=2), rows=2, distinct=1), command, tables, 'a'*64)
        self.assertEqual(result['graph']['entities'][0]['key'], [])
        self.assertEqual(next(i for i in result['items'] if i['class']=='metric')['decision'], 'demoted')


if __name__ == '__main__':
    unittest.main()
