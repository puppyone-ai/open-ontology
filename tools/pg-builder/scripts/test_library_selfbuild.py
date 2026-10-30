import importlib.util
from pathlib import Path
import unittest
from observation_test_support import observed_fixture

spec = importlib.util.spec_from_file_location('library_selfbuild', Path(__file__).with_name('library_selfbuild.py'))
lib = importlib.util.module_from_spec(spec)
spec.loader.exec_module(lib)


class LibraryFactsTest(unittest.TestCase):
    def test_oracle_relationship_signals_use_original_names_not_pgident_hex(self):
        column = dict(name='q_554e49545f4944', source_name='UNIT_ID')
        key = dict(name='q_4944', source_name='ID')
        parent = dict(dataset='q_4f574e4552.q_554e4954', oracle={'source_table': 'UNIT'})
        self.assertEqual(lib.candidate_signal({}, column, parent, key), 'table_key_name')

    def test_oracle_dictionary_discovery_uses_source_names_and_physical_sql(self):
        from unittest.mock import Mock
        db = Mock(query=Mock(side_effect=[
            [dict(discriminator='shipment', dictionary_distinct=2, matched=2,
                  child_distinct=2, comment_label_matches=0)], [], [dict(code='A', rows=2)]]))
        dictionary = dict(dataset='q_4f574e4552.q_44494354', physical='"_oo"."pt_dict"',
                          oracle={'source_table': 'STATUS_DICT'},
                          columns=[dict(name='q_' + name.encode().hex(), source_name=name,
                                        physical_name='pc_' + str(i))
                                   for i, name in enumerate(['TYPE_CODE', 'CODE', 'NAME'])])
        table = dict(physical='"_oo"."pt_child"')
        column = dict(name='q_535441545553', source_name='STATUS', physical_name='pc_status')
        observed = lib.database_enums(db)(table, column, [dictionary])
        self.assertEqual(len(observed['bindings']), 1)
        self.assertEqual(observed['bindings'][0]['dataset'], dictionary['dataset'])
        self.assertEqual(observed['bindings'][0]['status'], 'undetermined')
        self.assertIn('"pc_status"', db.query.call_args_list[0].args[0])
        self.assertIn('"_oo"."pt_dict"', db.query.call_args_list[0].args[0])
        self.assertNotIn('"TYPE_CODE"', db.query.call_args_list[0].args[0])

    def test_identical_physical_types_keep_indexed_equality(self):
        from unittest.mock import Mock
        db = Mock()
        db.query.return_value = {'rows': 1, 'non_null': 1, 'distinct': 1, 'matched': 1}
        table = {'physical': '"app"."items"'}
        column = {'physical_name': 'id', 'data_type': 'bigint', 'profile': {'rows': 1}}
        lib.database_containment(db)(table, column, table, column)
        self.assertNotIn('::text', db.query.call_args_list[-1].args[0])
        sql = db.query.call_args_list[-1].args[0]
        self.assertIn('p.key=c.key', sql)
        self.assertIn('group by "id"', sql)
        self.assertIn('select distinct "id"', sql)
        self.assertNotIn('exists (select', sql)

    def test_retained_facts_cannot_bypass_exhausted_observation_budget(self):
        with self.assertRaisesRegex(ValueError, 'relationship_check_budget_exhausted'):
            lib.require_complete_relationship_observations({
                'relationship_candidates': [{'evidence': {'method': 'budget_unmeasured'}}]})
        facts = {'relationship_candidates': [{'evidence': {'method': 'empty_table'}}]}
        self.assertEqual(lib.require_complete_relationship_observations(facts), facts)

    def test_relationship_budget_exhaustion_fails_instead_of_silently_losing_edges(self):
        from unittest.mock import Mock
        db = Mock()
        db.query.return_value = {'rows': 1, 'non_null': 1, 'distinct': 1, 'matched': 1}
        check = lib.database_containment(db, max_checks=1)
        table = {'physical': '"app"."items"'}
        column = {'physical_name': 'id', 'profile': {'rows': 1}}
        self.assertTrue(check(table, column, table, column)['parent_unique'])
        with self.assertRaisesRegex(ValueError, 'relationship_check_budget_exhausted'):
            check(table, column, table, column)
        for invalid in (0, -1, True, '1024'):
            with self.assertRaisesRegex(ValueError, 'relationship_check_budget'):
                lib.database_containment(db, invalid)

    def fixture(self):
        tables = []
        for i in range(70):
            tables.append({'dataset': f'app.item_{i:03}', 'source_id': f'shard_{i // 16}',
                           'source_generation': 1, 'target_generation': 2,
                           'primary_key': ['id'], 'columns': [
                               {'name': 'id', 'data_type': 'bigint', 'comment': '主键', 'profile': {'rows': 100, 'distinct': 100}},
                               {'name': 'owner_unit_id', 'data_type': 'bigint', 'comment': '所属网点编号', 'profile': {'rows': 100, 'distinct': 3}},
                               {'name': 'status', 'data_type': 'text', 'comment': '状态', 'profile': {'rows': 100, 'distinct': 3}}]})
        tables.append({'dataset': 'app.unit', 'source_id': 'master', 'source_generation': 3,
                       'target_generation': 4, 'primary_key': ['unit_id'], 'columns': [
                           {'name': 'unit_id', 'data_type': 'bigint', 'comment': '网点编号', 'profile': {'rows': 3, 'distinct': 3}},
                           {'name': 'unit_name', 'data_type': 'text', 'comment': '网点名称', 'profile': {'rows': 3, 'distinct': 3}}]})
        return observed_fixture(tables)

    def test_global_relationships_are_not_limited_to_a_batch(self):
        tables = self.fixture()
        facts = lib.build_facts(tables, lambda *args: {'non_null': 100, 'matched': 100, 'parent_unique': True})
        self.assertEqual(len(facts['relationships']), 70)
        self.assertTrue(all(r['to']['dataset'] == 'app.unit' for r in facts['relationships']))
        self.assertEqual(facts, lib.build_facts(list(reversed(tables)), lambda *args: {'non_null': 100, 'matched': 100, 'parent_unique': True}))
        self.assertTrue(all(r['confidence'] == '0.99' for r in facts['relationships']))

    def test_no_containment_or_nonunique_target_is_not_a_proven_relation(self):
        tables = self.fixture()
        facts = lib.build_facts(tables, lambda *args: {'non_null': 100, 'matched': 30, 'parent_unique': True})
        self.assertEqual(facts['relationships'], [])
        facts = lib.build_facts(tables, lambda *args: {'non_null': 100, 'matched': 100, 'parent_unique': False})
        self.assertEqual(facts['relationships'], [])

    def test_distinct_overlap_cannot_override_failed_row_containment(self):
        facts = lib.build_facts(self.fixture(), lambda *args: {
            'non_null': 100, 'matched': 99, 'parent_unique': True,
            'non_null_rows': 10000, 'matched_rows': 300,
        })
        self.assertEqual(facts['relationships'], [])
        self.assertTrue(facts['relationship_candidates'])
        self.assertTrue(all(r['confidence'] == '0.50' for r in facts['relationship_candidates']))

    def test_classification_and_low_cardinality_do_not_invent_meanings(self):
        self.assertEqual(lib.semantic('phone', 'text', '联系电话')['pii'], 'personal')
        self.assertEqual(lib.semantic('unit_name', 'text', '网点名称')['pii'], 'none')
        self.assertEqual(lib.semantic('region', 'text', '省市区域')['pii'], 'none')
        self.assertEqual(lib.semantic('event_date', 'varchar(20)', '发生日期')['semantic_type'], 'date')
        self.assertEqual(lib.semantic('parent_id', 'bigint', '')['semantic_type'], 'identifier')
        facts = lib.build_facts(self.fixture(), lambda *args: {'non_null': 100, 'matched': 100, 'parent_unique': True})
        self.assertTrue(facts['enum_candidates'])
        self.assertTrue(all(e['status'] == 'undetermined' for e in facts['enum_candidates']))

class DictionaryFactsTest(unittest.TestCase):
    def test_dictionary_candidates_preserve_type_scoped_evidence(self):
        tables = LibraryFactsTest().fixture()
        facts = lib.build_facts(tables, lambda *a: {'non_null': 1, 'matched': 1, 'parent_unique': True},
                                lambda *a: [{'dataset': 'app.codebook', 'type_column': 'kind',
                                             'code_column': 'code', 'containment': '1.000000', 'matched': 3}])
        self.assertEqual(len(facts['enum_candidates']), 70)
        self.assertEqual(facts['enum_candidates'][0]['dictionary_candidates'][0]['matched'], 3)

class ManifestTest(unittest.TestCase):
    def test_manifest_coverage_is_exact_and_exclusions_are_not_silent(self):
        tables = LibraryFactsTest().fixture()
        manifest = {'protocol': 'oo.library.manifest/v1', 'inventory': [{'schema': 'app', 'table': 'unit'}, {'schema': 'app', 'table': 'retired'}],
                    'exclusions': [{'schema': 'app', 'table': 'retired', 'reason': 'not modeled'}]}
        self.assertEqual([t['dataset'] for t in lib.select_library(tables, manifest)], ['app.unit'])
        manifest['exclusions'] = []
        with self.assertRaises(ValueError):
            lib.select_library(tables, manifest)

class BatchTest(unittest.TestCase):
    def test_batches_carry_global_edges_and_neighbours(self):
        facts = lib.build_facts(LibraryFactsTest().fixture(), lambda *a: {'non_null': 100, 'matched': 100, 'parent_unique': True})
        batches = lib.draft_batches(facts)
        self.assertEqual([len(b['tables']) for b in batches], [16, 16, 16, 16, 7])
        self.assertIn('app.unit', [n['dataset'] for n in batches[0]['neighbors']])
        self.assertEqual(len(batches[0]['relationships']), 16)
        self.assertEqual(len(batches[0]['enum_candidates']), 16)

    def test_drafts_cannot_override_column_facts(self):
        facts = lib.build_facts(LibraryFactsTest().fixture(), lambda *a: {'non_null': 100, 'matched': 100, 'parent_unique': True})
        draft = {'datasets': [{'dataset': 'app.item_000', 'description': 'Draft only', 'columns': [
            {'name': 'owner_unit_id', 'data_type': 'text', 'description': 'Owner unit'}]}]}
        merged = lib.merge_drafts(facts, [draft])
        column = next(c for c in merged['tables'][0]['columns'] if c['name'] == 'owner_unit_id')
        self.assertEqual(column['data_type'], 'bigint')
        self.assertEqual(len(merged['review_queue']), 1)
        self.assertEqual(column['description_candidate'], 'Owner unit')

class EffectiveTest(unittest.TestCase):
    def test_fixture_review_and_sensitive_columns_are_explicit(self):
        facts = lib.build_facts(LibraryFactsTest().fixture(), lambda *a: {'non_null': 100, 'matched': 100, 'parent_unique': True})
        effective, visibility, review = lib.effective_documents(facts, [], fixture_review=True)
        self.assertEqual(len(effective['datasets']), 71)
        self.assertEqual(len(effective['relationships']), 70)
        self.assertEqual(len(visibility['datasets']), 71)
        self.assertFalse(review['business_owner_signed'])
        self.assertEqual(review['reviewer'], 'fixture/test')
        self.assertTrue(effective['enums'])
        self.assertTrue(all(r['review']['by'] == 'fixture/test' for r in effective['relationships']))

class CompleteFixtureTest(unittest.TestCase):
    def test_multisource_wide_library_compiles_one_pack(self):
        import os
        import tempfile
        import json
        tables = LibraryFactsTest().fixture()
        tables[0]['columns'].extend({'name': f'measure_{i:03}', 'data_type': 'bigint', 'comment': '数量',
                                   'profile': {'rows': 100, 'distinct': 100}} for i in range(499))
        observed_fixture(tables)
        facts = lib.build_facts(tables, lambda *a: {'non_null': 100, 'matched': 100, 'parent_unique': True},
                                lambda *a: {'bindings': [], 'values': [{'code': '', 'rows': 1}, {'code': '1', 'rows': 99}]})
        drafts = []
        for batch in lib.draft_batches(facts):
            drafts.append({'datasets': [{'dataset': t['dataset'], 'description': 'Anonymous fixture draft',
                                        'columns': [{'name': c['name'], 'description': 'Fixture description'} for c in t['columns']]}
                                       for t in batch['tables']],
                           'glossary': [{'term': 'Fixture domain ' + str(batch['index']), 'definition': 'Anonymous test term',
                                         'dataset': batch['tables'][0]['dataset']}]})
        with tempfile.TemporaryDirectory(prefix='oo-wf-bld-fixture-') as root:
            root = Path(root)
            ceilings = {'max_datasets': 4096, 'max_columns': 1024, 'max_effective_columns': 1024,
                        'max_relationships': 32768, 'max_enums': 32768, 'max_entities': 4096,
                        'max_entries': 16384, 'max_file_bytes': 4194304, 'max_total_bytes': 67108864,
                        'json_scan': {'max_bytes': 67108864, 'max_nodes': 4000000, 'max_depth': 32}}
            lib.write(root / 'ceilings.json', ceilings)
            pack = lib.compile_pack(facts, [{'draft': draft} for draft in drafts],
                                    {'fixture_review': True, 'builder_binary': os.environ['OO_LIBRARY_BUILDER_BIN'],
                                     'pack_ceilings_file': str(root / 'ceilings.json')}, root)
            files = {item['path']: item['text'] for item in pack['artifact']['files']}
            self.assertEqual(len([name for name in files if name.startswith('tables/')]), 71)
            relationship_pages = [text for name, text in files.items()
                                  if name.startswith('relationships/')]
            self.assertTrue(relationship_pages)
            self.assertIn('relationships/page-0001.md', files['relationships.md'])
            self.assertIn('app.unit', '\n'.join(relationship_pages))
            self.assertIn('Fixture domain', files['glossary.md'])
            self.assertIn('Domain map', files['README.md'])
            review = json.loads((root / 'review.json').read_text())
            self.assertFalse(review['business_owner_signed'])
            self.assertTrue(any(d['kind'] == 'unrepresentable_enum_code' for d in review['decisions']))
            self.assertEqual(len(facts['binding_vector']), 71)
            self.assertGreater(len({v['source_id'] for v in facts['binding_vector']}), 1)

class ObservationBoundaryTest(unittest.TestCase):
    def test_enum_observation_failure_is_audited_not_approved(self):
        def failed(*args):
            raise RuntimeError('read_only_aggregate_query_failed:57014')
        facts = lib.build_facts(LibraryFactsTest().fixture(),
                                lambda *a: {'non_null': 100, 'matched': 100, 'parent_unique': True}, failed)
        self.assertEqual(len(facts['relationships']), 70)
        self.assertTrue(all(e['observation_error'].endswith('57014') for e in facts['enum_candidates']))
        self.assertTrue(all(not e['dictionary_candidates'] for e in facts['enum_candidates']))

    def test_glossary_is_draft_without_an_explicit_fixture_review(self):
        facts = lib.build_facts(LibraryFactsTest().fixture(), lambda *a: {'non_null': 100, 'matched': 100, 'parent_unique': True})
        draft = {'datasets': [], 'glossary': [{'term': 'Anonymous term', 'definition': 'Fixture definition', 'dataset': 'app.unit'}]}
        effective, visibility, review = lib.effective_documents(facts, [draft])
        self.assertEqual(effective['glossary'][0]['status'], 'draft')
        self.assertFalse(review['business_owner_signed'])
        self.assertFalse(visibility['datasets'])


class GlossaryMergeTest(unittest.TestCase):
    def test_conflicting_terms_are_not_silently_approved(self):
        facts = lib.build_facts(LibraryFactsTest().fixture(), lambda *a: {'non_null': 100, 'matched': 100, 'parent_unique': True})
        drafts = [{'datasets': [], 'glossary': [{'term': 'Shared term', 'definition': 'First meaning', 'dataset': 'app.unit'}]},
                  {'datasets': [], 'glossary': [{'term': 'Shared term', 'definition': 'Different meaning', 'dataset': 'app.item_000'}]}]
        effective, _, review = lib.effective_documents(facts, drafts, fixture_review=True)
        self.assertEqual(len(effective['glossary']), 1)
        self.assertEqual(effective['glossary'][0]['status'], 'draft')
        self.assertTrue(any(item['kind'] == 'glossary_conflict' for item in review['decisions']))

class BatchBudgetTest(unittest.TestCase):
    def test_large_context_splits_without_dropping_facts(self):
        tables = LibraryFactsTest().fixture()[:2]
        for table in tables:
            table['columns'][0]['comment'] = 'x' * 140000
        facts = lib.build_facts(tables, lambda *a: {'non_null': 0, 'matched': 0, 'parent_unique': False})
        batches = lib.draft_batches(facts)
        self.assertEqual([len(batch['tables']) for batch in batches], [1, 1])
        self.assertEqual(len(batches[0]['tables'][0]['columns'][0]['comment']), 140000)


class ClassificationVisibilityTest(unittest.TestCase):
    def test_fixture_review_does_not_expose_sensitive_columns(self):
        tables = LibraryFactsTest().fixture()
        sensitive = [('mobile', '手机'), ('identity_number', '身份证'), ('bank_number', '银行卡'), ('street_address', '详细地址')]
        visible = [('branch_label', '网点名称'), ('branch_type', '网点类型'), ('province', '省市区域')]
        tables[0]['columns'].extend({'name': name, 'data_type': 'text', 'comment': comment, 'profile': {'rows': 100, 'distinct': 3}}
                                   for name, comment in sensitive + visible)
        facts = lib.build_facts(tables, lambda *a: {'non_null': 100, 'matched': 100, 'parent_unique': True})
        _, visibility, _ = lib.effective_documents(facts, [], fixture_review=True)
        columns = next(t['columns'] for t in visibility['datasets'] if t['dataset'] == 'app.item_000')
        self.assertTrue(all(name not in columns for name, _ in sensitive))
        self.assertTrue(all(name in columns for name, _ in visible))


if __name__ == '__main__':
    unittest.main()
