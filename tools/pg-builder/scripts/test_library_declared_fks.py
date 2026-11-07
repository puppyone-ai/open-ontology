"""Declared relationships survive observation failures of the constraint."""
import copy
import unittest

import library_selfbuild as lib


def tables():
    columns = [dict(name=n, physical_name=n, data_type='bigint', nullable=False) for n in ('id', 'ref')]
    parent = dict(dataset='app.parent', source_id='fixture', source_generation=1, target_generation=1,
                  primary_key=['id'], columns=columns, physical='"app"."parent"')
    child = dict(copy.deepcopy(parent), dataset='app.child', physical='"app"."child"',
                 foreign_keys=[dict(name='child_fk', columns=['ref'], referenced_dataset='app.parent',
                                    referenced_columns=['id'])])
    return [parent, child]


class DeclaredForeignKeys(unittest.TestCase):
    def test_failed_containment_still_has_a_declared_relationship(self):
        facts = lib.build_facts(tables(), lambda *args: dict(non_null=10, matched=2, parent_unique=True))
        declared = [r for r in facts['relationships'] if r['provenance'] == 'declared']
        self.assertEqual(len(declared), 1)
        self.assertEqual(declared[0]['evidence']['containment'], '0.200000')
        self.assertEqual(declared[0]['status'], 'candidate')
        self.assertFalse(facts['business_owner_signed'])

    def test_declared_edge_is_not_also_inferred(self):
        inventory = tables()
        inventory[1]['foreign_keys'][0]['columns'] = ['id']
        facts = lib.build_facts(inventory, lambda *args: dict(non_null=2, matched=2, parent_unique=True))
        edges = [r for r in facts['relationship_candidates']
                 if r['from'] == dict(dataset='app.child', columns=['id'])
                 and r['to'] == dict(dataset='app.parent', columns=['id'])]
        self.assertEqual(len(edges), 1)
        self.assertEqual(edges[0]['provenance'], 'declared')

    def test_composite_observation_respects_budget_and_order(self):
        class Db:
            def __init__(self):
                self.queries = []
            def query(self, sql):
                self.queries.append(sql)
                return (dict(unique=True) if "'unique'" in sql else
                        dict(non_null=2, matched=1, non_null_rows=3, matched_rows=1))
        db = Db()
        observe = lib.database_containment(db, max_checks=1)
        parent, child = tables()
        result = observe(child, child['columns'], parent, list(reversed(parent['columns'])))
        self.assertEqual(result['method'], 'exact_composite_containment')
        self.assertIn('ROW("id","ref")', db.queries[1])
        self.assertIn('ROW("ref","id")', db.queries[1])
        with self.assertRaisesRegex(ValueError, 'relationship_check_budget_exhausted'):
            observe(child, child['columns'], parent, parent['columns'])
        self.assertEqual(len(db.queries), 2)

    def test_missing_endpoint_refuses_instead_of_losing_a_constraint(self):
        inventory = tables()
        inventory[1]['foreign_keys'][0]['referenced_columns'] = ['absent']
        with self.assertRaisesRegex(ValueError, 'declared_fk_mapping'):
            lib.build_facts(inventory, lambda *args: self.fail('unmapped key observed'))

    def test_query_error_is_not_a_complete_observation(self):
        def failed(*args):
            raise RuntimeError('source_unavailable')
        facts = lib.build_facts(tables(), failed)
        self.assertTrue(any(r['provenance'] == 'declared' for r in facts['relationships']))
        with self.assertRaisesRegex(ValueError, 'relationship_observation_incomplete'):
            lib.require_complete_relationship_observations(facts)


if __name__ == '__main__':
    unittest.main()
