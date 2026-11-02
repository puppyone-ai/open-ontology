"""Synthetic source observations. Not captured-generation acceptance."""
import copy
import hashlib
import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import library_control as control
import library_build as build
import library_selfbuild as lib
import library_semantics as semantics
from observation_test_support import observed_fixture
from test_library_semantics import fixture_response, rehash


def facts(generation=4, rows=12):
    binding = {'run_id': str(generation), 'source_id': 'unit-oracle',
               'source_generation': generation, 'target_generation': generation,
               'config_digest': 'config', 'type_digest': 'types', 'schema_digest': 'schema',
               'input_digest': str(rows), 'stored_namespace': 'pn_' + str(generation),
               'stored_physical': 'pt_' + str(generation)}
    receipt = {
        'source_id': 'unit-oracle', 'run_id': binding['run_id'], 'run_ids': [binding['run_id']],
        'source_generation': generation, 'target_generation': generation,
        'position_codec': 'oracle-capture-sequence/v1', 'source_epoch': 1,
        'consistency_grade': 'T', 'cross_table_consistent': True, 'applied_pos': str(generation),
        'tables': [{'schema': 'APP', 'table': 'ITEMS', 'accepted_rows': rows,
                    'canonical_digest': str(rows), 'rejects': []}],
        'oracle_buffer': {'source_identity': {'dbid': 'unit'}, 'typed_schema_sha256': 'schema',
                          'commit_scn': str(rows), 'bundle_sha256': str(rows)}}
    oracle = {
        'protocol': 'oo.builder.oracle-source-observation/v1', 'source_bound': None,
        'source_schema': 'APP', 'source_table': 'ITEMS',
        'binding': dict(binding, accepted_rows=rows,
                        columns=[{'logical_name': 'ID', 'stored_physical': 'pc_' + str(generation)}],
                        column_mappings=[{'logical_column': 'id', 'source_name': 'ID',
                                          'physical_name': 'pc_' + str(generation)}]),
        'pins': {'adapter_sha256': 'adapter', 'catalog_sha256': str(generation),
                 'profile_sha256': str(rows), 'semantics_sha256': str(rows)},
        'business_dictionary': {'state': 'missing', 'business_semantics_approved': False},
        'source_dictionary': {'primary_key': ['ID'], 'default_state': 'unmeasured'},
        'source_profile': dict(binding, rows=rows, observed_at=str(rows), snapshot=str(rows),
                               columns=[{'name': 'ID', 'non_null': rows, 'distinct': rows}]),
        'source_semantics': {
            'measurement_protocol': 'oo.builder.oracle-target-observation/v1',
            'measurement_code_sha256': 'adapter', 'publication_receipt': receipt,
            'source_generation': generation, 'target_generation': generation,
            'binding_vector': [binding], 'binding_vector_sha256': str(rows),
            'collection_sha256': str(generation), 'profile_sha256': str(rows),
            'measurement_sha256': str(rows),
            'input_sha256': {'source_schema': 'schema', 'catalog': str(rows), 'publication': str(rows)},
            'source_clock': {'basis': 'watermark', 'source_committed_at': None},
            'target_observation': {'database': 'unit-target', 'principal': 'unit-observer',
                'freshness': [{'source_generation': generation, 'last_pos': generation, 'state': 'live'}],
                'metadata': {'full_import_run': [binding], 'native_publication_receipt': [receipt],
                             'source_watermark': [{'source_generation': generation, 'applied_pos': generation}],
                             'source_instance': [{'source_epoch': 1}], 'table_quality': [{'rejects_open': 0}]}}}}
    tables = observed_fixture([{
        'dataset': 'app.items', 'source_id': 'unit-oracle', 'source_engine': 'oracle',
        'source_generation': generation, 'target_generation': generation, 'primary_key': ['id'],
        'mapping_namespace': 'pn_' + str(generation), 'oracle': oracle, 'ddl_sha256': 'schema',
        'columns': [{'name': 'id', 'data_type': 'bigint', 'comment': 'identifier',
                     'profile': {'rows': rows, 'distinct': rows}},
                    {'name': 'status', 'data_type': 'text', 'comment': 'status',
                     'profile': {'rows': rows, 'distinct': 2}}]}])
    tables[0]['observation']['binding'] = lib.observation.table_binding(tables[0])
    return lib.build_facts(tables, lambda *a: {}, lambda *a: {
        'bindings': [], 'values': [{'code': 'open', 'rows': rows - 1}, {'code': 'closed', 'rows': 1}]})


class DataOnlyTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.config = {'model': 'fixture/local-v1', 'endpoint': 'https://fixture.invalid/v1',
                       'max_draft_batches': 1,
                       'release_versions': {k: 'unit-v1' for k in ('ontology', 'catalog', 'policy', 'knowledge')},
                       'semantic_scope': {'project': 'unit', 'role': 'oo_r_unit',
                                          'principal': 'unit', 'permissions_sha256': 'a' * 64}}
        self.old = facts()
        self.before_acl = {'principal': 'unit-observer', 'database': 'unit-target',
                           'grants': [{'table_schema': 'fixture', 'table_name': 't_app_items',
                                       'column_name': c['physical_name'], 'privilege_type': 'SELECT'}
                                      for c in self.old['tables'][0]['columns']]}
        self.config['semantic_scope']['permissions_sha256'] = lib.digest(self.before_acl)
        with patch.object(lib.model_provider, 'credential', return_value='unit-not-secret'), \
                patch.object(lib.model_provider, 'open_request', side_effect=fixture_response):
            receipts, failures = control.drafts(self.old, self.config, self.root)
        self.assertEqual(failures, [])
        self.receipts = receipts
        origin = {'protocol': 'oo.library.data-only-origin/v1'}
        for name, value in [('facts', self.old), ('config', self.config),
                            ('drafts', {'receipts': receipts, 'failures': []}), ('pack', {}),
                            ('effective', {}), ('visibility', {})]:
            origin[name] = self.pin(name, value)
        self.current_config = dict(self.config, data_only_origin=self.pin('origin', origin))
        self.addCleanup(patch.stopall)
        self.model = patch.object(lib, 'call_model', side_effect=AssertionError('S5_FORBIDDEN')).start()
        self.db = patch.object(lib.ReadOnlyPostgres, 'query', side_effect=AssertionError('DB_FORBIDDEN')).start()

    def pin(self, name, value):
        path = self.root / (name + '.json')
        path.write_text(lib.canonical(value))
        return {'path': str(path), 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()}

    def test_data_change_produces_separate_use_without_rewriting_s5_identity(self):
        current = facts(5, 13)
        original_bytes = (self.root / 'drafts.json').read_bytes()
        result, failures = control.drafts(current, self.current_config, self.root)
        self.assertEqual(failures, [])
        self.assertEqual(len(result), 1)
        use = result[0]
        self.assertEqual(use['facts_sha256'], current['facts_sha256'])
        self.assertEqual(use['origin_receipt'], self.receipts[0])
        self.assertEqual(use['origin_receipt_sha256'], lib.digest(self.receipts[0]))
        self.assertNotIn('request_sha256', use)
        self.assertNotIn('usage', use)
        self.assertEqual(use['new_model_calls'], 0)
        self.assertEqual((self.root / 'drafts.json').read_bytes(), original_bytes)
        self.model.assert_not_called()
        self.db.assert_not_called()

    def test_nested_metadata_scope_new_codes_and_unknown_extensions_invalidate(self):
        for change in ('adapter', 'default', 'type', 'code', 'unknown', 'null', 'epoch', 'empty'):
            current = facts(5, 13)
            table = current['tables'][0]
            oracle = table['oracle']
            if change == 'adapter':
                oracle['pins']['adapter_sha256'] = 'changed'
            elif change == 'default':
                oracle['source_dictionary']['default_state'] = 'measured'
            elif change == 'type':
                oracle['binding']['type_digest'] = 'changed'
            elif change == 'code':
                current['enum_candidates'][0]['values'].append({'code': 'new', 'rows': 1})
            elif change == 'unknown':
                oracle['source_semantics']['future_semantics'] = {'units': 'changed'}
            elif change == 'null':
                oracle['source_profile']['columns'][0]['non_null'] -= 1
            elif change == 'epoch':
                oracle['source_semantics']['publication_receipt']['source_epoch'] = 2
            else:
                oracle['source_profile']['rows'] = 0
            table['observation']['binding'] = lib.observation.table_binding(table)
            with self.subTest(change=change), self.assertRaisesRegex(ValueError, 'data_only_semantic_drift'):
                control.drafts(rehash(current), self.current_config, self.root)
        self.model.assert_not_called()

    def test_old_generation_foreign_scope_and_corrupt_pins_fail_closed(self):
        with self.assertRaisesRegex(ValueError, 'data_only_not_successor'):
            control.drafts(self.old, self.current_config, self.root)
        cfg = copy.deepcopy(self.current_config)
        cfg['semantic_scope']['role'] = 'oo_r_foreign'
        with self.assertRaisesRegex(ValueError, 'data_only_scope_changed'):
            control.drafts(facts(5, 13), cfg, self.root)
        (self.root / 'drafts.json').write_text('{}')
        with self.assertRaisesRegex(ValueError, 'data_only_origin_digest'):
            control.drafts(facts(5, 13), self.current_config, self.root)
        self.model.assert_not_called()

    def test_request_or_draft_tampering_even_with_resealed_file_is_rejected(self):
        for field in ('request_sha256', 'draft_sha256', 'cache_identity_sha256'):
            broken = copy.deepcopy(self.receipts)
            broken[0][field] = '0' * 64
            origin = json.loads((self.root / 'origin.json').read_text())
            origin['drafts'] = self.pin('drafts-' + field, {'receipts': broken, 'failures': []})
            cfg = dict(self.current_config, data_only_origin=self.pin('origin-' + field, origin))
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, 'data_only_origin_s5_binding'):
                control.drafts(facts(5, 13), cfg, self.root)
        self.model.assert_not_called()

    def test_same_meaning_differs_from_original_s5_prompt_identity(self):
        old, new = facts(), facts(5, 13)
        self.assertEqual(semantics.data_only_digest(old), semantics.data_only_digest(new))
        self.assertNotEqual(lib.draft_batches(old)[0]['semantic_input_digest'],
                            lib.draft_batches(new)[0]['semantic_input_digest'])

    def test_mysql_ddl_route_reuses_counts_but_keeps_semantic_dependencies(self):
        def mysql(generation, rows):
            value = facts(generation, rows)
            table = value['tables'][0]
            table.pop('oracle')
            table.pop('source_engine')
            table.update(ddl_sha256='a' * 64, table_options_sql='ENGINE=InnoDB',
                         constraints_sql=['PRIMARY KEY (`id`)'])
            for column in table['columns']:
                column.update(source_type=column['data_type'],
                              definition_sql=column['name'] + ' ' + column['data_type'])
            table['observation']['binding'] = lib.observation.table_binding(table)
            return rehash(value)

        old, current = mysql(1, 12), mysql(2, 13)
        expected = semantics.data_only_digest(old, mysql_ddl=True)
        self.assertEqual(expected, semantics.data_only_digest(current, mysql_ddl=True))
        with self.assertRaisesRegex(ValueError, 'data_only_source_route_required'):
            semantics.data_only_digest(current)
        for field, changed in [('ddl_sha256', 'b' * 64), ('primary_key', ['status']),
                               ('table_options_sql', 'ENGINE=InnoDB COLLATE=utf8_bin'),
                               ('unknown_meaning', {'unit': 'new'})]:
            altered = copy.deepcopy(current)
            altered['tables'][0][field] = changed
            self.assertNotEqual(expected, semantics.data_only_digest(altered, mysql_ddl=True))
        for field in ('source_type', 'definition_sql'):
            altered = copy.deepcopy(current)
            altered['tables'][0]['columns'][0].pop(field)
            with self.assertRaisesRegex(ValueError, 'data_only_mysql_ddl_required'):
                semantics.data_only_digest(altered, mysql_ddl=True)
        altered = copy.deepcopy(current)
        altered['tables'][0]['source_engine'] = 'postgresql'
        with self.assertRaisesRegex(ValueError, 'data_only_source_route_required'):
            semantics.data_only_digest(altered, mysql_ddl=True)
        self.model.assert_not_called()

    def test_pack_receipt_uses_the_validated_engine_specific_semantic_binding(self):
        current = facts(5, 13)
        uses = [{'semantic_input_digest': 'a' * 64}]
        lib.write(self.root / 'drafts.json', {'receipts': uses, 'failures': []})
        lib.write(self.root / 'visibility.json', {})
        lib.write(self.root / 'effective-review-input.json', {})
        origin = {'facts': self.old, 'pack': {'artifact': {}, 'pack_digest': 'unit',
                  'effective_sha256': lib.digest({}), 'visibility_sha256': lib.digest({})}}
        with patch.object(lib, 'data_only_origin', return_value=origin), \
                patch.object(lib, 'data_only_drafts', return_value=uses), \
                patch.object(lib, 'verify_pack_artifact'), \
                patch.object(semantics, 'data_only_digest', side_effect=AssertionError('route already validated')):
            build.verify_data_only_pack(current, self.current_config, self.root,
                                        {'artifact': {}, 'pack_digest': 'unit'}, {})
        proof = json.loads((self.root / 'data-only-reuse.json').read_text())
        self.assertEqual(proof['semantic_input_digest'], uses[0]['semantic_input_digest'])
        self.model.assert_not_called()

    def test_new_physical_acl_requires_exact_logical_permission_equivalence(self):
        current = facts(5, 13)
        table = current['tables'][0]
        table['physical'] = '"fixture"."new_items"'
        table['observation']['binding'] = lib.observation.table_binding(table)
        current = rehash(current)
        acl = copy.deepcopy(self.before_acl)
        acl['grants'] += [dict(g, table_name='new_items') for g in self.before_acl['grants']]
        config = copy.deepcopy(self.current_config)
        config['semantic_scope']['permissions_sha256'] = lib.digest(acl)
        with self.assertRaisesRegex(ValueError, 'data_only_scope_changed'):
            control.drafts(current, config, self.root)
        before = self.pin('before-acl', self.before_acl)
        config['data_only_permissions'] = self.pin('permission-proof', {
            'protocol': 'oo.library.data-only-permissions/v1', 'before': before,
            'current': self.pin('current-acl', acl)})
        uses, _ = control.drafts(current, config, self.root)
        self.assertEqual(uses[0]['permissions']['origin_scope'], self.config['semantic_scope'])
        self.assertNotEqual(uses[0]['permissions']['origin_scope'], uses[0]['permissions']['current_scope'])
        acl['grants'].append(dict(acl['grants'][0], table_name='foreign'))
        config['semantic_scope']['permissions_sha256'] = lib.digest(acl)
        config['data_only_permissions'] = self.pin('bad-permission-proof', {
            'protocol': 'oo.library.data-only-permissions/v1', 'before': before,
            'current': self.pin('bad-current-acl', acl)})
        with self.assertRaisesRegex(ValueError, 'data_only_scope_changed'):
            control.drafts(current, config, self.root)
        self.model.assert_not_called()

    @unittest.skipUnless(os.environ.get('OO_LIBRARY_BUILDER_BIN'), 'requires local Rust compiler')
    def test_new_reuse_entrypoint_runs_s5_s8_review_yaml_and_same_pack_without_model(self):
        review = {
            'project': 'unit', 'role': 'oo_r_unit', 'principal': 'unit',
            'columns': {'app.items.id': {'pii': 'none', 'semantic_type': 'identifier',
                                         'reviewer': 'unit/test'}},
            'hidden_columns': ['app.items.status'], 'datasets': {}, 'relationships': []}
        binary = Path(os.environ['OO_LIBRARY_BUILDER_BIN'])

        def compile_document(directory, value, receipts, config):
            directory.mkdir()
            lib.write(directory / 'facts.json', value)
            lib.write(directory / 'drafts.json', {'receipts': receipts, 'failures': []})
            lib.write(directory / 'config.json', dict(config, psql_command=['false']))
            lib.write(directory / 'manifest.json', {})
            lib.write(directory / 'ledger-review.json', review)
            request = {'work_dir': str(directory), 'config': str(directory / 'config.json'),
                       'manifest': str(directory / 'manifest.json'), 'llm': {}}
            if config.get('data_only_origin'):
                control.run(dict(request, stage='S5'))
            for stage in ('S6', 'S7', 'S8', 'compile'):
                control.run(dict(request, stage=stage))
            visibility = json.loads((directory / 'visibility.json').read_text())
            lib.write(directory / 'ceilings.json', visibility['limits'])
            completed = subprocess.run([str(binary.with_name('oos-tree')), str(directory / 'ontology'),
                str(directory / 'oos-tree-binding.json'), str(directory / 'visibility.json'),
                str(directory / 'ceilings.json'), str(directory / 'roundtrip.json')],
                text=True, capture_output=True)
            self.assertEqual(completed.returncode, 0, completed.stderr)
            result = json.loads((directory / 'roundtrip.json').read_text())
            effective = json.loads((directory / 'effective-oos.json').read_text())
            self.assertEqual(result['effective_oos'], effective)
            lib.verify_pack_artifact(result['artifact'])
            return result, effective, visibility

        old_result, effective, visibility = compile_document(
            self.root / 'old-pack', self.old, self.receipts, self.config)
        origin = json.loads((self.root / 'origin.json').read_text())
        origin['pack'] = self.pin('actual-unit-pack', {
            'pack_digest': old_result['pack_digest'], 'artifact': old_result['artifact'],
            'effective_sha256': lib.digest(effective), 'visibility_sha256': lib.digest(visibility)})
        origin['effective'] = self.pin('actual-unit-effective', effective)
        origin['visibility'] = self.pin('actual-unit-visibility', visibility)
        config = dict(self.current_config, data_only_origin=self.pin('complete-origin', origin))
        current = facts(5, 13)
        uses, failures = control.drafts(current, config, self.root)
        self.assertEqual(failures, [])
        result, effective, _ = compile_document(self.root / 'new-pack', current, uses, config)
        build.verify_data_only_pack(current, config, self.root / 'new-pack', result, effective)
        self.assertEqual(old_result['artifact'], result['artifact'])
        self.model.assert_not_called()
        wrong = dict(result, pack_digest='0' * 64)
        with self.assertRaisesRegex(ValueError, 'data_only_pack_drift'):
            build.verify_data_only_pack(current, config, self.root / 'new-pack', wrong, effective)

    def test_old_generation_read_approval_cannot_be_carried_forward(self):
        review = self.pin('old-review', {'protocol': 'oo.library.column-review/v1',
            'project': 'unit', 'facts_sha256': self.old['facts_sha256'],
            'review': {'project': 'unit'}})
        with self.assertRaisesRegex(ValueError, 'column_review_binding'):
            build.pinned_review(dict(self.current_config, project='unit', column_review_input=review),
                                facts(5, 13))


if __name__ == '__main__':
    unittest.main()
