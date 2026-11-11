"""Source metadata consumption, independent of the not-yet-exported engine API."""
import copy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import library_selfbuild as lib
import library_semantics as semantics
import retained_build as build
import retained_catalog as catalog
import retained_observation as observation
from test_retained_build import Db, cut, facts


def fixture():
    metadata = cut(1, rows=2, column='id')
    candidate = metadata['candidate']
    receipt = candidate['shard']['receipt']
    base = copy.deepcopy(metadata)
    candidate['data_vector'] = []
    candidate['request']['recipes'] = []
    receipt['tables'] = []
    metadata['mappings'], metadata['attributes'] = [], []
    source = dict(protocol=catalog.PROTOCOL, source_id='owned', tables=[])
    for i, (name, reference) in enumerate([
            ('accounts', None), ('invoices', 'accounts'), ('lines', 'invoices')]):
        columns = ['id'] + (['reference'] if reference else [])
        physical = 'pt_' + ('%060x' % (100 + i))
        recipe = copy.deepcopy(base['candidate']['request']['recipes'][0])
        recipe['plan']['table']['name'] = name
        recipe['definition'] = ','.join('"%s" bigint' % c for c in columns)
        recipe_sha = observation.rust_digest(recipe)
        candidate['request']['recipes'].append(recipe)
        published = dict(base['candidate']['shard']['receipt']['tables'][0],
                         table=name, storage_table=name, physical_name=physical,
                         recipe_sha256=recipe_sha)
        receipt['tables'].append(published)
        vector = dict(base['candidate']['data_vector'][0], dataset='app.' + name,
                      physical_name=physical, recipe_sha256=recipe_sha,
                      expected_columns=[dict(name=c, data_type='bigint') for c in columns])
        candidate['data_vector'].append(vector)
        for j, column in enumerate(columns):
            mapped = 'pc_' + ('%060x' % (100 * (i + 1) + j))
            metadata['mappings'].append(dict(base['mappings'][0], logical_table=name,
                logical_column=column, physical_name=mapped, scope_table_physical=physical))
            metadata['attributes'].append(dict(base['attributes'][0], relname=physical,
                                               attname=mapped, not_null=False))
        foreign_keys = [] if not reference else [dict(name=name + '_reference_fk', columns=[
            dict(name='reference', ref_schema='app', ref_table=reference, ref_name='id')])]
        source['tables'].append(dict(schema='app', name=name, comment='Source ' + name,
            columns=[dict(name=c, not_null=True, comment='Source ' + c) for c in columns],
            primary_key=[dict(name='id')], foreign_keys=foreign_keys))
    candidate['shard']['receipt_sha256'] = observation.rust_digest(receipt)
    candidate['data_vector_sha256'] = observation.rust_digest(candidate['data_vector'])
    return metadata, source


class RelatedDb(Db):
    def query(self, sql):
        if 'from pg_views' in sql:
            return []
        if 'from pg_extension' in sql:
            return None
        if "'entropy'" in sql:
            return dict(rows=2, non_null=2, distinct=2, readable=0, max_length=1,
                        phone=0, id_card=0, email=0, bank_card=0, entropy=1)
        if "'columns'," in sql and 'pg_current_snapshot' in sql:
            self.queries.append(sql)
            mappings = [m for m in self.metadata['mappings']
                        if '"' + m['scope_table_physical'] + '"' in sql]
            return dict(rows=2, snapshot='1:1:', observed_at='now', columns=[
                dict(name=m['logical_column'], non_null=2, distinct=2, min_length=1,
                     max_length=1, blank_rows=0, digits_rows=2, leading_zero_rows=0)
                for m in mappings])
        if sql.startswith('select (with c as'):
            return dict(child_rows=2, empty_rows=0, child_distinct=2, non_blank_rows=2,
                        matched_distinct=2, matched_rows=2, max_fanout=1, parent_rows=2,
                        parent_empty_rows=0, parent_distinct=2, parent_duplicate_keys=0,
                        parent_normalized_distinct=2, scope={})
        if "'non_null_rows'" in sql:
            return dict(non_null=2, matched=2, non_null_rows=2, matched_rows=2)
        if "'distinct'" in sql:
            return dict(rows=2, non_null=2, distinct=2)
        return super().query(sql)


class RetainedCatalog(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.metadata, self.source = fixture()
        self.config = dict(protocol=build.PROTOCOL, library_id='owned', generation='cycle-1',
                           source_engine='postgres', project='fixture', psql_command=['unused'])

    def pin(self, value):
        path = self.root / 'source-catalog.json'
        path.write_text(json.dumps(value))
        return dict(path=str(path), sha256=hashlib.sha256(path.read_bytes()).hexdigest())

    def observed(self, source=None):
        config = dict(self.config, source_catalog=self.pin(source or self.source))
        return build.facts_for(RelatedDb(self.metadata), config)[0]

    def test_two_declared_fks_comments_and_non_null_primary_keys(self):
        current = self.observed()
        declared = [r for r in current['relationships'] if r['provenance'] == 'declared']
        self.assertEqual(len(declared), 2)
        self.assertTrue(all(r['evidence']['containment'] == '1.000000' for r in declared))
        for table in build.inventory(current):
            self.assertEqual(table['comment'], 'Source ' + table['dataset'].split('.')[1])
            self.assertEqual(table['primary_key'], ['id'])
            self.assertTrue(all(not c['nullable'] and c['comment'] for c in table['columns']))

    def test_no_pin_keeps_the_original_inventory_and_digest(self):
        current, _ = build.facts_for(RelatedDb(self.metadata), self.config)
        self.assertTrue(all(t['comment'] is None and not t['foreign_keys']
                            for t in build.inventory(current)))
        self.assertTrue(all(c['nullable'] for t in current['tables'] for c in t['columns']))
        # Existing generation equivalence remains covered by test_retained_build.
        self.assertEqual(semantics.data_only_digest(facts(cut(1))),
                         semantics.data_only_digest(facts(cut(2))))
        # Gold values of oo.library.data-only-semantics/v2 (key quality added in V3-G-01).
        for engine, expected in {
            'postgres': '6a273dabbc86b5cc96c9e1a5a44402533c0bcaf597feb86266c9c19732592dc7',
            'mysql': '0aceda0264368de5ac0aa08f3d700aaec6cff96ccb8fefa6ea8a5cb31db205ea',
            'oracle': 'c1a652ffd78714025af4618d6fd29fab0abb0482f3285167e25f41c733a2581c',
        }.items():
            self.assertEqual(semantics.data_only_digest(facts(cut(1, engine=engine), engine)), expected)
        self.assertEqual(semantics.data_only_digest(current),
                         '2eeb6aea7955e49bda8ba0efc47164cd31c8c1bfb79ec95d5392d6da49654be6')
        self.assertNotEqual(semantics.data_only_digest(current),
                            semantics.data_only_digest(self.observed()))

    def test_metadata_changes_invalidate_reuse_but_capture_time_does_not(self):
        original = semantics.data_only_digest(self.observed())
        source = copy.deepcopy(self.source)
        source['captured_at'] = 'a later capture'
        self.assertEqual(original, semantics.data_only_digest(self.observed(source)))
        for mutate in (
                lambda s: s['tables'][0].update(comment='Changed meaning'),
                lambda s: s['tables'][1]['columns'][1].update(not_null=False),
                lambda s: s['tables'][1].update(foreign_keys=[]),
                lambda s: s['tables'][0].update(primary_key=[])):
            source = copy.deepcopy(self.source)
            mutate(source)
            self.assertNotEqual(original, semantics.data_only_digest(self.observed(source)))

    def test_bad_pin_is_refused_before_any_observation(self):
        config = dict(self.config, source_catalog=self.pin(self.source))
        config['source_catalog']['sha256'] = '0' * 64
        db = RelatedDb(self.metadata)
        with self.assertRaisesRegex(ValueError, 'retained_origin_digest'):
            build.facts_for(db, config)
        self.assertEqual(db.queries, [])

    def test_scope_identity_column_and_key_mismatches_refuse(self):
        for mutate, code in [
            (lambda s: s.update(source_id='other'), 'retained_source_catalog_binding'),
            (lambda s: s['tables'].pop(), 'retained_source_table_mismatch'),
            (lambda s: s['tables'].append(s['tables'][0]), 'retained_source_table_mismatch'),
            (lambda s: s['tables'][0]['columns'].pop(), 'retained_source_column_mismatch'),
            (lambda s: s['tables'][0].update(primary_key=[dict(name='missing')]), 'retained_source_key_mismatch'),
            (lambda s: s['tables'][0]['columns'][0].update(not_null=False), 'retained_source_key_nullable'),
            (lambda s: s['tables'][0].update(comment=dict(sha256='0' * 64, byte_len=6)),
             'retained_source_comment_unavailable'),
            (lambda s: s['tables'][1]['foreign_keys'][0]['columns'][0].update(ref_name='missing'),
             'retained_source_fk_mismatch'),
        ]:
            source = copy.deepcopy(self.source)
            mutate(source)
            with self.subTest(code=code), self.assertRaisesRegex(ValueError, code):
                self.observed(source)

    def test_out_of_scope_reference_has_an_explicit_reason(self):
        self.source['tables'][1]['foreign_keys'][0]['columns'][0]['ref_table'] = 'external'
        current = self.observed()
        invoices = next(t for t in current['tables'] if t['dataset'] == 'app.invoices')
        self.assertEqual(invoices['foreign_keys'], [])
        self.assertEqual(invoices['excluded_foreign_keys'][0]['reason'], 'referenced_table_outside_scope')

    def test_declared_fk_with_failed_containment_is_not_dropped(self):
        tables = self.observed()['tables']
        def absent(*args):
            return dict(non_null=2, matched=0, parent_unique=True, method='exact_distinct_containment')
        current = lib.build_facts(tables, absent)
        declared = [r for r in current['relationships'] if r['provenance'] == 'declared']
        self.assertEqual(len(declared), 2)
        self.assertTrue(all(r['evidence']['containment'] == '0.000000' for r in declared))

    def test_composite_fk_observes_the_ordered_tuple_and_keeps_self_reference(self):
        tables = self.observed()['tables']
        invoices = next(t for t in tables if t['dataset'] == 'app.invoices')
        invoices['foreign_keys'] = [dict(name='self_composite', columns=['id', 'reference'],
            referenced_dataset='app.invoices', referenced_columns=['reference', 'id'])]
        seen = []
        def observed(child, columns, parent, keys):
            if isinstance(columns, list):
                seen.append(([c['name'] for c in columns], [k['name'] for k in keys]))
            return dict(non_null=2, matched=1, parent_unique=True)
        current = lib.build_facts(tables, observed)
        self.assertEqual(seen, [(['id', 'reference'], ['reference', 'id'])])
        self.assertTrue(any(r['from']['dataset'] == r['to']['dataset'] for r in current['relationships']))

    def test_encoded_source_names_use_the_loader_mapping(self):
        metadata = cut(1, engine='oracle', column='amount')
        source = dict(protocol=catalog.PROTOCOL, source_id='owned', tables=[
            dict(schema='APP', name='ITEMS', comment='Source items', primary_key=[dict(name='AMOUNT')],
                 columns=[dict(name='AMOUNT', not_null=True, comment='Source amount')], foreign_keys=[])])
        config = dict(self.config, source_engine='oracle', source_catalog=self.pin(source))
        current = build.facts_for(Db(metadata), config)[0]
        self.assertEqual(current['tables'][0]['dataset'], 'q_415050.q_4954454d53')
        self.assertEqual(current['tables'][0]['primary_key'], ['q_414d4f554e54'])

    def test_run_writes_declared_evidence_into_owner_review_input(self):
        config = dict(self.config, source_catalog=self.pin(self.source))
        path = self.root / 'config.json'
        path.write_text(json.dumps(config))
        with patch.object(build.lib, 'ReadOnlyPostgres', return_value=RelatedDb(self.metadata)), \
                patch.object(build, 'draft', return_value=({'graph': {'edges': []}}, 0)):
            result = build.run(path, self.root / 'output')
        self.assertEqual(result['stage'], 'review_required')
        command = json.loads((self.root / 'output/review-input.json').read_text())
        self.assertEqual(len(command['source_inventory']), 3)
        self.assertEqual(len([r for r in command['source_relationships']
                              if r['provenance'] == 'declared']), 2)
        self.assertTrue(all(t['comment'] for t in command['source_inventory']))
        self.assertEqual((self.root / 'output/facts.json').stat().st_mode & 0o777, 0o600)

    def test_inventory_reaches_real_community_prompts(self):
        import selfbuild_budget
        from test_selfbuild_budget import Acceptance, Model
        model = Model()
        result = selfbuild_budget.Runner(Acceptance().config(), self.root / 'runner', model).run(
            build.inventory(self.observed()), [])
        self.assertEqual(result['unresolved'], [])
        self.assertEqual(len(result['graph']['edges']), 2)
        prompts = json.dumps(model.requests)
        self.assertIn('Source accounts', prompts)
        self.assertIn('Source reference', prompts)


if __name__ == '__main__':
    unittest.main()
