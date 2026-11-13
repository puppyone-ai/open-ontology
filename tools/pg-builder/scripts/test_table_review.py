import copy
import unittest

import library_semantics as semantics
import retained_build as builder
import retained_catalog as catalog
from test_retained_build import cut, facts
from test_retained_catalog import fixture, RelatedDb


class TableReview(unittest.TestCase):
    def test_legacy_library_hash_and_table_hash_survive_new_generation(self):
        old, new = facts(cut(1)), facts(cut(2))
        self.assertEqual(semantics.data_only_digest(old),
                         '6a273dabbc86b5cc96c9e1a5a44402533c0bcaf597feb86266c9c19732592dc7')
        self.assertEqual(semantics.table_data_only_digests(old),
                         semantics.table_data_only_digests(new))
        self.assertEqual(semantics.table_review_pending(old, new), [])

    def observed(self, source):
        metadata, _ = fixture()
        db = RelatedDb(metadata)
        tables, _ = builder.retained_observation.observe(db, 'owned', 'cycle-1', 'postgres', source)
        return builder.lib.build_facts(tables, builder.lib.database_containment(db, 65536),
                                       builder.lib.database_enums(db, .5))

    def test_metadata_only_marks_the_declaring_table(self):
        _, source = fixture()
        old = self.observed(source)
        for key in ('comment', 'column_comment', 'foreign_keys', 'new_column'):
            changed = copy.deepcopy(source)
            table = changed['tables'][1]
            if key == 'comment':
                table['comment'] = 'changed'
            elif key == 'column_comment':
                table['columns'][0]['comment'] = 'changed'
            elif key == 'foreign_keys':
                table['foreign_keys'] = []
            else:
                table['columns'].append(dict(name='new', not_null=False, comment=None))
            current = self.observed(changed)
            pending = semantics.table_review_pending(old, current)
            self.assertEqual([r['table'] for r in pending], ['app.invoices'], key)
            self.assertTrue(pending[0]['changes'])
            self.assertNotIn('new', [c['name'] for c in current['tables'][1]['columns']])

    def test_library_remainder_is_stable_for_unchanged_sources(self):
        old, new = facts(cut(1)), facts(cut(2))
        self.assertEqual(semantics.library_data_only_digest(old), semantics.library_data_only_digest(new))
        self.assertEqual(semantics.library_review_pending(old, new, 'owned'), [])
        _, source = fixture()
        first, second = self.observed(source), self.observed(copy.deepcopy(source))
        self.assertEqual(semantics.library_data_only_digest(first), semantics.library_data_only_digest(second))
        self.assertEqual(semantics.table_data_only_digests(first), semantics.table_data_only_digests(second))
        # Counts are evidence, not semantics: row-count drift keeps the digest.
        counted = copy.deepcopy(first)
        for item in counted['relationship_candidates']:
            item['evidence']['matched'] = item['evidence']['non_null'] = 99
        self.assertEqual(semantics.library_data_only_digest(first), semantics.library_data_only_digest(counted))

    def test_relationship_candidate_change_requires_library_review(self):
        _, source = fixture()
        old = self.observed(source)
        for mutate in (lambda f: f['relationship_candidates'].pop(),
                       lambda f: f['relationship_candidates'][-1].update(cardinality='one_to_one'),
                       lambda f: f['relationship_candidates'][-1]['evidence'].update(containment='0.5')):
            current = copy.deepcopy(old)
            self.assertEqual(current['relationship_candidates'][-1]['provenance'], 'inferred')
            mutate(current)
            self.assertEqual(semantics.table_review_pending(old, current), [])
            self.assertEqual(semantics.library_review_pending(old, current, 'owned'), [
                {'table': 'library:owned', 'scope': 'library', 'changes': ['relationship_candidates']}])
        # An observed source change that drops an inferred edge is library-scoped too.
        changed = copy.deepcopy(source)
        changed['tables'][0]['columns'][0]['comment'] = 'changed'
        self.assertTrue(semantics.library_review_pending(old, self.observed(changed), 'owned'))

    def test_report_change_requires_library_review_only(self):
        _, source = fixture()
        old = self.observed(source)
        old['report_definitions_observation'] = {'dataset': 'app.accounts', 'definitions': [
            {'active': '1', 'module_name': 'Accounts', 'sql_text': 'SELECT COUNT(*) FROM accounts'}]}
        current = copy.deepcopy(old)
        current['report_definitions_observation']['definitions'][0]['sql_text'] = 'SELECT 1 FROM accounts'
        self.assertEqual(semantics.table_review_pending(old, current), [])
        self.assertEqual(semantics.library_review_pending(old, current, 'owned'), [
            {'table': 'library:owned', 'scope': 'library', 'changes': ['reports']}])
        self.assertNotEqual(semantics.data_only_digest(old), semantics.data_only_digest(current))

    def test_one_table_comment_marks_only_that_table(self):
        _, source = fixture()
        old = self.observed(source)
        for index, path in ((1, None), (1, 1), (2, None)):
            changed = copy.deepcopy(source)
            target = changed['tables'][index] if path is None else changed['tables'][index]['columns'][path]
            target['comment'] = 'changed'
            current = self.observed(changed)
            self.assertEqual(semantics.library_review_pending(old, current, 'owned'), [])
            self.assertEqual([r['table'] for r in semantics.table_review_pending(old, current)],
                             ['app.' + source['tables'][index]['name']])

    def test_reviewed_projection_allows_extra_columns_but_not_missing_or_retyped(self):
        current = facts(cut(1))
        reviewed = dict(protocol='oo.owner-reviewed-registry/v1', status='reviewed',
                        review_decision={'mode': 'self'}, compiled={'effective': {'datasets': [
                            {'dataset': 'app.items', 'columns': [{'name': 'amount', 'data_type': 'bigint'}]}]}})
        current['tables'][0]['columns'].append({'name': 'extra', 'data_type': 'text'})
        builder.reviewed_shape(reviewed, current)
        for columns in ([{'name': 'extra', 'data_type': 'text'}],
                        [{'name': 'amount', 'data_type': 'text'}]):
            changed = copy.deepcopy(current)
            changed['tables'][0]['columns'] = columns
            with self.assertRaisesRegex(ValueError, 'shape_drift'):
                builder.reviewed_shape(reviewed, changed)
        reviewed['compiled']['effective']['datasets'] = []
        with self.assertRaisesRegex(ValueError, 'shape_drift'):
            builder.reviewed_shape(reviewed, current)

    def test_unpublished_fk_never_becomes_a_published_relationship(self):
        metadata, source = fixture()
        source['tables'][1]['columns'].append(dict(name='new', not_null=False, comment=None))
        source['tables'][1]['foreign_keys'][0]['columns'][0]['name'] = 'new'
        current = self.observed(source)
        table = current['tables'][1]
        self.assertEqual(table['foreign_keys'], [])
        self.assertEqual(table['excluded_foreign_keys'][0]['reason'], 'unreviewed_column')
        source['tables'][0]['columns'].pop()
        with self.assertRaisesRegex(ValueError, 'column_mismatch'):
            self.observed(source)

    def test_stale_source_drift_uses_old_metadata_without_blocking_fresh_tables(self):
        metadata, source = fixture()
        old = self.observed(source)
        source['tables'][1]['columns'].pop()
        source['tables'][0]['comment'] = 'changed'
        tables, _ = builder.retained_observation.observe(
            RelatedDb(metadata), 'owned', 'cycle-1', 'postgres', source,
            previous=old, stale_tables=['app.invoices'])
        projection = lambda columns: [
            {k: c[k] for k in ('name', 'data_type', 'nullable', 'comment')} for c in columns]
        self.assertEqual(projection(tables[1]['columns']), projection(old['tables'][1]['columns']))
        self.assertEqual(tables[0]['table_comment'], 'changed')
        source['tables'].pop(1)
        tables, _ = builder.retained_observation.observe(
            RelatedDb(metadata), 'owned', 'cycle-1', 'postgres', source,
            previous=old, stale_tables=['app.invoices'])
        self.assertEqual(tables[1]['foreign_keys'], old['tables'][1]['foreign_keys'])
        with self.assertRaisesRegex(ValueError, 'stale_origin_missing'):
            builder.retained_observation.observe(
                RelatedDb(metadata), 'owned', 'cycle-1', 'postgres', source,
                stale_tables=['app.invoices'])
