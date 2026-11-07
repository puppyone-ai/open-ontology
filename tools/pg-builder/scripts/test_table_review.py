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
                         'c412c1956e8d7cef50b63d143c729966eb44243b6f8f896ba3966066338df87f')
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
