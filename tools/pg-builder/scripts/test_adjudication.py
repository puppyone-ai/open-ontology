"""Boundary rules and the shared graph contract. No source connection."""
import copy
import tempfile
import hashlib
import json
from pathlib import Path
import unittest

import adjudication as a
import retained_build
from test_retained_build import Db, cut, facts


class Adjudication(unittest.TestCase):
    def test_open_discovery_is_not_limited_to_model_candidates(self):
        tables = [
            dict(dataset='app.orders', primary_key=['id'], columns=[
                dict(name='id', data_type='bigint'), dict(name='customer_id', data_type='bigint')]),
            dict(dataset='app.customers', primary_key=['id'], columns=[
                dict(name='id', data_type='bigint')])]
        command = {'draft': {'graph': {'edges': []}}}
        a.discover_candidates(command, tables)
        edges = copy.deepcopy(command['draft']['graph']['edges'])
        self.assertTrue(any(e['from'] == 'app.orders' and
                            e['columns']['from'] == ['customer_id'] for e in edges))
        a.discover_candidates(command, tables)
        self.assertEqual(edges, command['draft']['graph']['edges'])

    def test_shared_tui_hash_vector(self):
        vector = json.loads((Path(__file__).resolve().parents[3] /
                            'crates/oo-builder/tests/adjudication-hash-vector.json').read_text())
        self.assertEqual(a.summary_hash(vector['document']), vector['expected_sha256'])
        self.assertEqual(hashlib.sha256(vector['preimage'].encode()).hexdigest(),
                         vector['expected_sha256'])

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
        for field in a.FORMAT_FIELDS:
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


def column(name, data_type='text', **profile):
    base = dict(non_null=10, distinct=10, min_length=2, max_length=4, blank_rows=0,
                digits_rows=0, leading_zero_rows=0)
    return dict(name=name, physical_name=name, data_type=data_type, profile=dict(base, **profile))


def table(dataset, columns, primary_key=(), **extra):
    return dict(dataset=dataset, physical='"' + dataset.replace('.', '"."') + '"',
                primary_key=list(primary_key), columns=columns, **extra)


class MeasuredDb:
    """Dispatches on SQL text; measurements are keyed by (child, column, parent)."""
    def __init__(self, tables, measurements, rows=10):
        self.tables, self.measurements, self.rows, self.measured = tables, measurements, rows, []

    def query(self, sql):
        if 'pg_views' in sql:
            return []
        if 'pg_extension' in sql:
            return None
        if "'entropy'" in sql:
            return dict(rows=self.rows, non_null=self.rows, distinct=self.rows, max_length=4, readable=0,
                        phone=0, id_card=0, email=0, bank_card=0, entropy=0)
        if "'distinct',count(distinct ROW(" in sql:
            return dict(rows=self.rows, non_null=self.rows, distinct=self.rows, null_rows=0, empty_rows=0)
        if sql.startswith('select (with c as'):
            for (child, col, parent), value in self.measurements.items():
                if (sql.index(self.tables[child]['physical']) < sql.index(' p as (')
                        and self.tables[parent]['physical'] in sql.split(' p as (')[1].split(' m as')[0]
                        and '"' + col + '"' in sql.split(' p as (')[0]):
                    self.measured.append((child, col, parent))
                    return copy.deepcopy(value)
            return measurement(matched=0, matched_distinct=0)
        raise AssertionError('unexpected query ' + sql[:80])


def measurement(rows=10, matched=None, distinct=10, matched_distinct=None, parent=10,
                parent_distinct=10, duplicates=0, fanout=1, normalized=None, empty=0, scope=None):
    matched = rows if matched is None else matched
    matched_distinct = distinct if matched_distinct is None else matched_distinct
    return dict(child_rows=rows + empty, empty_rows=empty, child_distinct=distinct, non_blank_rows=rows,
                matched_distinct=matched_distinct, matched_rows=matched, max_fanout=fanout if matched else 0,
                parent_rows=parent, parent_empty_rows=0, parent_distinct=parent_distinct,
                parent_duplicate_keys=duplicates,
                parent_normalized_distinct=parent_distinct if normalized is None else normalized,
                scope=scope or {})


def command_for(tables, edges=()):
    command = retained_build.review_input(tables, {'graph': {'edges': list(edges)}}, 'test')
    a.discover_candidates(command, tables)
    return command


class Relationships(unittest.TestCase):
    base = dict(containment=1.0, distinct_containment=1.0, parent_unique=True, name_similarity=0,
                join_cooccurrence=0, type_compatible=True, length_compatible=True, rows_checked=100,
                max_fanout=1, normalized_collisions=0, scope_columns=[], kind='inferred_id')

    def decide(self, **patch):
        return a.relation_decision(dict(self.base, **patch), a.thresholds())

    def test_structural_rules_block_adoption_before_containment(self):
        self.assertEqual(self.decide(), ('adopted', 'relation.containment_strict'))
        # The confirmed defect: 0.95 + name match + non-unique parent was adopted.
        self.assertEqual(self.decide(containment=.95, name_similarity=1, parent_unique=False),
                         ('rejected', 'relation.parent_not_unique'))
        self.assertEqual(self.decide(parent_unique=False, scope_columns=['valid_from']),
                         ('rejected', 'relation.scope_required'))
        self.assertEqual(self.decide(max_fanout=2), ('rejected', 'relation.parent_not_unique'))
        self.assertEqual(self.decide(normalized_collisions=1), ('rejected', 'relation.normalized_collision'))
        # Open recall needs a name, declaration or query signal; model proposals keep the strict path.
        self.assertEqual(self.decide(source='discovery'), ('rejected', 'relation.no_signal'))
        self.assertEqual(self.decide(source='discovery', name_similarity=1)[0], 'adopted')
        self.assertEqual(self.decide(source='model')[0], 'adopted')

    def test_distinct_containment_is_also_a_gate(self):
        # Frequent keys match, the long tail does not.
        self.assertEqual(self.decide(containment=.99, distinct_containment=.5)[0], 'rejected')
        self.assertEqual(self.decide(containment=.95, distinct_containment=.95, name_similarity=1)[0], 'adopted')
        self.assertEqual(self.decide(containment=.95, distinct_containment=.89, name_similarity=1)[0], 'rejected')
        self.assertEqual(self.decide(containment=.95, distinct_containment=.95, kind='declared_fk')[0], 'adopted')

    def test_scope_reason_names_the_filter(self):
        reason = a.relation_reason('relation.scope_required', {'scope_columns': ['period']})
        self.assertIn('period', reason)

    def test_key_family_excludes_amounts_times_floats_and_booleans(self):
        for kind in ['numeric(12,2)', 'double precision', 'real', 'timestamp without time zone', 'date',
                     'boolean', 'numeric']:
            self.assertIsNone(a.key_family(column('x', kind)), kind)
        for kind, family in [('bigint', 'integer'), ('numeric(10,0)', 'integer'), ('numeric(18)', 'integer'),
                             ('character varying(20)', 'text'), ('text', 'text')]:
            self.assertEqual(a.key_family(column('x', kind)), family, kind)
        self.assertEqual(a.key_family(column('x', 'numeric', digits_rows=10)), 'integer')
        self.assertTrue(a.keylike('customer_code') and a.keylike('order_id') and a.keylike('kh_bh'))
        self.assertFalse(a.keylike('amount') or a.keylike('paid') or a.keylike('created_at'))

    def test_unique_amount_or_timestamp_is_never_an_entity_key(self):
        tables = [table('app.ledger', [column('amount', 'numeric(12,2)'),
                                       column('booked_at', 'timestamp without time zone'),
                                       column('note', 'text')])]
        result = a.collect(MeasuredDb({}, {}), command_for(tables), tables, 'a' * 64)
        key = next(i for i in result['items'] if i['class'] == 'entity_key')
        self.assertEqual((key['decision'], key['rule_id'], key['evidence']['key']),
                         ('rejected', 'entity_key.no_candidate', []))
        self.assertEqual(result['graph']['entities'][0]['key'], [])

    def test_entity_key_tiers_declared_pk_unique_then_observed(self):
        observed = [table('app.c', [column('amount', 'numeric(12,2)'), column('customer_code')])]
        unique = [table('app.c', [column('customer_code'), column('ref_no')],
                        constraints_sql=['UNIQUE KEY `uk` (`ref_no`)'])]
        declared = [table('app.c', [column('id', 'bigint'), column('customer_code')], primary_key=['id'])]
        for tables, key, rule in [(observed, ['customer_code'], 'entity_key.observed_unique'),
                                  (unique, ['ref_no'], 'entity_key.declared_unique'),
                                  (declared, ['id'], 'entity_key.declared_pk')]:
            result = a.collect(MeasuredDb({}, {}), command_for(tables), tables, 'a' * 64)
            item = next(i for i in result['items'] if i['class'] == 'entity_key')
            self.assertEqual((item['evidence']['key'], item['rule_id']), (key, rule))
            self.assertIn('null_rows', item['evidence'])
            self.assertIn('empty_rows', item['evidence'])

    def test_name_mapping_role_and_bucket_recall_without_hard_coding(self):
        regions = table('app.regions', [column('id', 'bigint', digits_rows=10), column('name')],
                        primary_key=['id'])
        shipments = table('app.shipments', [
            column('id', 'bigint', digits_rows=10),
            column('origin_region_id', 'bigint', digits_rows=10),
            column('dest_region_id', 'bigint', digits_rows=10),
            column('region_name', distinct=3),
            column('batch_code', leading_zero_rows=10, digits_rows=10)], primary_key=['id'])
        codes = table('app.batches', [column('batch_code', digits_rows=10)], primary_key=['batch_code'])
        command = command_for([regions, shipments, codes])
        edges = {(e['from'], tuple(e['columns']['from']), e['to'], tuple(e['columns']['to'])): e
                 for e in command['draft']['graph']['edges']}
        mapping = edges[('app.shipments', ('region_name',), 'app.regions', ('name',))]
        self.assertEqual(mapping['provenance'], 'name_mapping')
        self.assertIn(('app.shipments', ('origin_region_id',), 'app.regions', ('id',)), edges)
        self.assertIn(('app.shipments', ('dest_region_id',), 'app.regions', ('id',)), edges)
        # Child values with leading zeros cannot be contained in a parent that has none.
        self.assertNotIn(('app.shipments', ('batch_code',), 'app.batches', ('batch_code',)), edges)
        self.assertEqual(a.role_name(['origin_region_id']), 'origin_region')
        self.assertEqual(a.role_name(['ship_wh_bh']), 'ship_wh')

    def scenario(self, measurements, budget=65536, cache=None):
        regions = table('app.regions', [column('id', 'bigint', digits_rows=10), column('name')],
                        primary_key=['id'])
        shipments = table('app.shipments', [
            column('id', 'bigint', digits_rows=10),
            column('origin_region_id', 'bigint', digits_rows=10),
            column('dest_region_id', 'bigint', digits_rows=10),
            column('region_name', distinct=3)], primary_key=['id'])
        tables = [regions, shipments]
        db = MeasuredDb({t['dataset']: t for t in tables}, measurements)
        command = command_for(tables)
        result = a.collect(db, command, tables, 'a' * 64, budget=budget, cache_path=cache)
        return result, db

    def full(self):
        return {('app.shipments', c, 'app.regions'): measurement(distinct=3 if c == 'region_name' else 10)
                for c in ('origin_region_id', 'dest_region_id', 'region_name')}

    def test_two_roles_to_one_parent_stay_apart_and_name_mapping_is_adopted(self):
        result, _ = self.scenario(self.full())
        relations = {tuple(i['evidence']['from_columns']): i for i in result['items']
                     if i['class'] == 'relation'}
        roles = {k: v['evidence']['role'] for k, v in relations.items()}
        self.assertEqual(roles[('origin_region_id',)], 'origin_region')
        self.assertEqual(roles[('dest_region_id',)], 'dest_region')
        self.assertEqual(relations[('region_name',)]['evidence']['kind'], 'snapshot_name_mapping')
        self.assertTrue(all(i['decision'] == 'adopted' for i in relations.values()))
        self.assertEqual(result['discovery']['status'], 'complete')
        self.assertEqual(roles[('region_name',)], 'region')
        self.assertEqual({e['cardinality'] for e in result['graph']['edges']}, {'one_to_one', 'many_to_one'})

    def test_duplicate_parent_collision_and_scope_are_rejected_with_evidence(self):
        m = self.full()
        m['app.shipments', 'origin_region_id', 'app.regions'] = measurement(
            parent=12, parent_distinct=10, duplicates=2, fanout=2, scope={'valid_from': True})
        m['app.shipments', 'dest_region_id', 'app.regions'] = measurement(parent=12, parent_distinct=10,
                                                                          duplicates=2, fanout=2)
        m['app.shipments', 'region_name', 'app.regions'] = measurement(distinct=3, normalized=9)
        result, _ = self.scenario(m)
        rules = {tuple(i['evidence']['from_columns']): (i['decision'], i['rule_id'], i['evidence']['kind'])
                 for i in result['items'] if i['class'] == 'relation'}
        self.assertEqual(rules[('origin_region_id',)], ('rejected', 'relation.scope_required', 'scoped'))
        self.assertEqual(rules[('dest_region_id',)], ('rejected', 'relation.parent_not_unique', 'inferred_id'))
        self.assertEqual(rules[('region_name',)][:2], ('rejected', 'relation.normalized_collision'))

    def test_long_tail_and_blank_keys_are_reported_not_matched(self):
        m = self.full()
        m['app.shipments', 'origin_region_id', 'app.regions'] = measurement(
            rows=100, matched=99, distinct=50, matched_distinct=30, empty=7)
        result, _ = self.scenario(m)
        item = next(i for i in result['items'] if i['evidence'].get('from_columns') == ['origin_region_id'])
        e = item['evidence']
        self.assertEqual((e['containment'], e['distinct_containment'], e['unmatched_rows'], e['empty_rows']),
                         (.99, .6, 1, 7))
        self.assertEqual(item['decision'], 'rejected')

    def test_budget_exhaustion_is_incomplete_and_a_rerun_resumes(self):
        with tempfile.TemporaryDirectory() as tmp:
            cache = Path(tmp) / 'adjudication-measurements.json'
            first, db = self.scenario(self.full(), budget=1, cache=cache)
            self.assertEqual(len(db.measured), 1)
            self.assertEqual((first['discovery']['status'], first['discovery']['remaining']), ('incomplete', 2))
            self.assertEqual(len(first['graph']['edges']), 1)
            ids = {u['id'] for u in first['discovery']['unchecked']}
            self.assertFalse(ids & {e['relation'] for e in first['graph']['edges']})
            second, db = self.scenario(self.full(), budget=1, cache=cache)
            self.assertEqual(second['discovery']['remaining'], 1)
            third, db = self.scenario(self.full(), budget=1, cache=cache)
            self.assertEqual(third['discovery']['status'], 'complete')
            again, db = self.scenario(self.full(), budget=1, cache=cache)
            self.assertEqual((len(db.measured), again['summary_sha256']), (0, third['summary_sha256']))
            full, _ = self.scenario(self.full())
            self.assertEqual(third['summary_sha256'], full['summary_sha256'])
            self.assertEqual(oct(cache.stat().st_mode & 0o777), '0o600')
            schema = json.loads((Path(__file__).resolve().parents[3] /
                                 'crates/oo-builder/tests/adjudication.schema.json').read_text())
            import jsonschema
            for document in (first, third):
                jsonschema.validate(document, schema)


class KeyQualityBoundary(unittest.TestCase):
    def test_uniqueness_change_invalidates_but_row_growth_does_not(self):
        import library_semantics as semantics
        old = facts(cut(1))
        grown = facts(cut(2, rows=5), rows=5, distinct=5)
        duplicated = facts(cut(2, rows=5), rows=5, distinct=4)
        self.assertEqual(semantics.data_only_digest(old), semantics.data_only_digest(grown))
        self.assertNotEqual(semantics.data_only_digest(old), semantics.data_only_digest(duplicated))
        self.assertEqual(semantics.relation_keys.key_quality(duplicated)['app.items']['amount']['duplicates'],
                         True)

    def test_adopted_relationship_is_remeasured_on_the_new_cut(self):
        r = Relationships()
        document, _ = r.scenario(r.full())
        tables = [t for t in (table('app.regions', [column('id', 'bigint', digits_rows=10), column('name')],
                                    primary_key=['id']),
                              table('app.shipments', [column('id', 'bigint', digits_rows=10),
                                                      column('origin_region_id', 'bigint', digits_rows=10),
                                                      column('dest_region_id', 'bigint', digits_rows=10),
                                                      column('region_name', distinct=3)], primary_key=['id']))]
        inventory = {t['dataset']: t for t in tables}
        same = r.full()
        grown = {k: measurement(rows=40, distinct=v['child_distinct'], parent=20, parent_distinct=20)
                 for k, v in same.items()}
        self.assertEqual(a.relationship_quality_drift(MeasuredDb(inventory, grown), document, tables), [])
        changed = dict(same)
        changed['app.shipments', 'origin_region_id', 'app.regions'] = measurement(
            parent=12, parent_distinct=10, duplicates=2, fanout=2)
        drift = a.relationship_quality_drift(MeasuredDb(inventory, changed), document, tables)
        self.assertEqual(len(drift), 1)
        tail = dict(same)
        tail['app.shipments', 'region_name', 'app.regions'] = measurement(distinct=3, matched_distinct=2,
                                                                          matched=9)
        self.assertEqual(len(a.relationship_quality_drift(MeasuredDb(inventory, tail), document, tables)), 1)


BUILDER_FIXTURE = (Path(__file__).resolve().parents[3] /
                   'crates/oo-builder/tests/fixtures/adjudication_relations.json')


def builder_fixture(budget=65536):
    """Envelope for oo-builder tests: exact input bytes plus the measured document."""
    regions = table('app.regions', [column('id', 'bigint', digits_rows=10), column('name'),
                                    column('valid_from', 'date')], primary_key=['id'])
    shipments = table('app.shipments', [
        column('id', 'bigint', digits_rows=10),
        column('origin_region_id', 'bigint', digits_rows=10),
        column('dest_region_id', 'bigint', digits_rows=10),
        column('region_name', distinct=3)], primary_key=['id'])
    tables = [regions, shipments]
    measurements = {
        ('app.shipments', 'origin_region_id', 'app.regions'): measurement(),
        ('app.shipments', 'dest_region_id', 'app.regions'): measurement(
            parent=12, parent_distinct=10, duplicates=2, fanout=2, scope={'valid_from': True}),
        ('app.shipments', 'region_name', 'app.regions'): measurement(
            rows=100, matched=95, distinct=20, matched_distinct=19)}
    command = retained_build.review_input(tables, {'graph': {'edges': []}}, 'fixture')
    a.discover_candidates(command, tables)
    raw = retained_build.lib.canonical(command)
    document = a.collect(MeasuredDb({t['dataset']: t for t in tables}, measurements), command, tables,
                         hashlib.sha256(raw.encode()).hexdigest(), budget=budget)
    return {'adjudication_input': raw, 'adjudication': document}


class BuilderFixture(unittest.TestCase):
    def test_builder_relations_fixture_is_reproducible(self):
        import os
        expected = {'complete': builder_fixture(), 'incomplete': builder_fixture(budget=1)}
        text = json.dumps(expected, indent=1, sort_keys=True, ensure_ascii=False) + '\n'
        if os.environ.get('OO_UPDATE_FIXTURES') == '1':
            BUILDER_FIXTURE.write_text(text)
        self.assertEqual(BUILDER_FIXTURE.read_text(), text)
        complete = expected['complete']['adjudication']
        rules = sorted(i['rule_id'] for i in complete['items'] if i['class'] == 'relation')
        self.assertEqual(rules, ['relation.containment_signal', 'relation.containment_strict',
                                 'relation.scope_required'])
