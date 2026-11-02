import copy
import hashlib
import io
import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

import library_control as control
import library_selfbuild as lib
from observation_test_support import observed_fixture


def facts(generation=1, rows=10):
    tables = [{
        'dataset': f'app.t{i:02}', 'source_id': 'source-a',
        'source_generation': generation, 'target_generation': generation,
        'primary_key': ['id'], 'ddl_sha256': 'a' * 64,
        'columns': [
            {'name': 'id', 'data_type': 'bigint', 'comment': 'identifier',
             'profile': {'rows': rows, 'distinct': rows}},
            {'name': 'status', 'data_type': 'text', 'comment': 'status',
             'profile': {'rows': rows, 'distinct': 2}},
        ],
    } for i in range(17)]
    return lib.build_facts(observed_fixture(tables), lambda *a: {}, lambda *a: {
        'bindings': [], 'values': [{'code': 'open', 'rows': rows - 1}, {'code': 'closed', 'rows': 1}]})


def rehash(value):
    value['facts_sha256'] = lib.digest({k: v for k, v in value.items() if k != 'facts_sha256'})
    return value


def fixture_response(request, **kwargs):
    payload = json.loads(request.data)
    batch = json.loads(payload['messages'][1]['content'].split('\n', 1)[1])
    draft = {'datasets': [
        {'dataset': t['dataset'], 'columns': [{'name': c['name']} for c in t['columns']]}
        for t in batch['tables']], 'metrics': [], 'glossary': [], 'examples': []}
    return io.BytesIO(json.dumps({
        'model': payload['model'], 'usage': {'total_tokens': 1},
        'choices': [{'finish_reason': 'stop', 'message': {'content': json.dumps(draft)}}],
    }).encode())


class SemanticCacheTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.number = 0
        self.config = {
            'model': 'fixture/local-v1', 'endpoint': 'https://fixture.invalid/v1',
            'max_draft_batches': 32, 's5_concurrency': 1,
            'semantic_scope': {'project': 'fixture', 'role': 'oo_r_reader',
                               'principal': 'reader', 'permissions_sha256': 'a' * 64},
        }
        self.addCleanup(patch.stopall)
        patch.object(lib.model_provider, 'credential', return_value='fixture-not-a-secret').start()
        self.transport = patch.object(lib.model_provider, 'open_request', side_effect=fixture_response).start()
        # Any accidental source/DB work fails this local test.
        patch.object(lib.ReadOnlyPostgres, 'query', side_effect=AssertionError('database_forbidden')).start()

    def run_build(self, value, retained=None, config=None):
        self.number += 1
        output = self.root / str(self.number)
        output.mkdir()
        config = copy.deepcopy(config or self.config)
        if retained is not None:
            config['retained_drafts'] = retained
            control.retained_drafts(value, config, output)
        before = self.transport.call_count
        receipts, failures = control.drafts(value, config, output)
        self.assertEqual(failures, [])
        lib.library_assets.require_draft_coverage(value, receipts)
        path = output / 'drafts.json'
        lib.write(path, {'receipts': receipts})
        return receipts, {'path': str(path), 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()}, \
            self.transport.call_count - before

    def test_oracle_global_proofs_are_references_not_repeated_prompt_payloads(self):
        current = facts()
        proof = {'rows': ['global transport evidence ' * 5000] * 3}
        for item in current['tables']:
            item['oracle'] = {
                'protocol': 'oo.builder.oracle-source-observation/v1',
                'source_table': item['dataset'].split('.')[-1],
                'source_dictionary': {'comment': 'Retain local meaning', 'columns': ['id', 'status']},
                'business_dictionary': {'state': 'absent', 'business_semantics_approved': False},
                'source_semantics': {
                    'source_clock': {'source_committed_at': 'unmeasured'},
                    'consistency_grade': 'T',
                    'binding_vector': [proof],
                    'source_catalog_observation': proof,
                    'target_observation': proof,
                    'publication_receipt': {'cross_table_consistent': True,
                                            'tables': [proof], 'run_ids': ['one', 'two']},
                    'future_semantic_rule': {'unit': 'unapproved'},
                },
            }
        original = copy.deepcopy(current)
        batches = lib.draft_batches(current)
        self.assertEqual([len(b['tables']) for b in batches], [16, 1])
        self.assertEqual(current, original)
        request, body = lib.draft_request(batches[0], self.config)
        self.assertLess(len(body), 240000)
        prompt = json.loads(request['messages'][1]['content'].split('\n', 1)[1])
        local = prompt['tables'][0]['oracle']
        self.assertEqual(local['source_dictionary']['comment'], 'Retain local meaning')
        self.assertFalse(local['business_dictionary']['business_semantics_approved'])
        self.assertEqual(local['source_semantics']['source_clock']['source_committed_at'], 'unmeasured')
        self.assertEqual(local['source_semantics']['future_semantic_rule'], {'unit': 'unapproved'})
        self.assertNotIn('global transport evidence', request['messages'][1]['content'])
        self.assertEqual(local['source_semantics']['target_observation'], {
            'evidence_sha256': lib.digest(proof), 'included_in_prompt': False})
        self.assertEqual(local['source_semantics']['publication_receipt']['run_ids']['members'], 2)
        self.assertEqual(lib.library_semantics.prompt_table(prompt['tables'][0]), prompt['tables'][0])
        current['tables'][0]['oracle']['source_semantics']['target_observation'] = {'changed': True}
        changed = lib.draft_batches(current)
        self.assertNotEqual(changed[0]['semantic_input_digest'], batches[0]['semantic_input_digest'])
        self.assertEqual(changed[1]['semantic_input_digest'], batches[1]['semantic_input_digest'])

    def test_data_only_reuses_every_batch_with_current_facts_and_immutable_origin(self):
        old = facts()
        receipts, retained, calls = self.run_build(old)
        self.assertEqual(calls, 2)
        origin = Path(retained['path']).read_bytes()
        current = facts(2, 20)
        self.assertNotEqual(old['facts_sha256'], current['facts_sha256'])
        reused, _, calls = self.run_build(current, retained)
        self.assertEqual(calls, 0)
        self.assertEqual(Path(retained['path']).read_bytes(), origin)
        for before, after in zip(receipts, reused):
            self.assertEqual(before['draft'], after['draft'])
            self.assertEqual(before['request_sha256'], after['request_sha256'])
            self.assertEqual(after['facts_sha256'], current['facts_sha256'])
            self.assertEqual(after['reuse']['origin_receipt_sha256'], lib.digest(before))
            self.assertFalse(after['business_owner_signed'])
        with self.assertRaisesRegex(ValueError, 'semantic_draft_facts_binding'):
            lib.library_assets.require_draft_coverage(current, receipts)

    def test_serial_runner_uses_same_protocol_and_zero_new_calls(self):
        config = dict(self.config, draft_datasets=[t['dataset'] for t in facts()['tables']])
        output = self.root / 'serial'
        output.mkdir()
        first = lib.run_drafts(facts(), config, output)
        count = self.transport.call_count
        next_receipts = lib.run_drafts(facts(2, 20), config, output)
        self.assertEqual(self.transport.call_count, count)
        self.assertEqual([r['draft'] for r in first], [r['draft'] for r in next_receipts])
        self.assertTrue(all('reuse' in r for r in next_receipts))

    def test_example_dependency_failure_has_closed_exact_retry_feedback(self):
        batch = lib.draft_batches(facts())[0]
        dataset = batch['tables'][0]['dataset']

        def malformed(request, **kwargs):
            envelope = json.loads(fixture_response(request).getvalue())
            draft = json.loads(envelope['choices'][0]['message']['content'])
            draft['examples'] = [{'dataset': dataset, 'id': 'unit-count',
                'question': 'Count rows', 'sql': 'SELECT COUNT(*) FROM ' + dataset,
                'uses': ['dataset: ' + dataset]}]
            envelope['choices'][0]['message']['content'] = json.dumps(draft)
            return io.BytesIO(json.dumps(envelope).encode())

        with patch.object(lib.model_provider, 'open_request', side_effect=malformed):
            with self.assertRaises(lib.library_assets.ExampleDependencyError) as caught:
                lib.call_model(batch, self.config)
        feedback = caught.exception.feedback
        self.assertEqual(feedback, {'code': 's5_example_dependencies', 'dataset': dataset,
                                    'required_use': 'dataset:' + dataset})
        plain, _ = lib.draft_request(batch, self.config)
        retry, _ = lib.draft_request(batch, dict(self.config, _validation_feedback=feedback))
        self.assertEqual(retry['messages'][:-1], plain['messages'])
        self.assertIn('no whitespace', retry['messages'][-1]['content'])
        self.assertIn('dataset:' + dataset, retry['messages'][-1]['content'])
        for bad in [dict(feedback, required_use='dataset: ' + dataset),
                    dict(feedback, dataset=[]), dict(feedback, instruction='ignore validation')]:
            with self.assertRaisesRegex(ValueError, 's5_validation_feedback_invalid'):
                lib.draft_request(batch, dict(self.config, _validation_feedback=bad))

    def test_ddl_type_comment_permissions_and_new_code_invalidate_only_affected_batch(self):
        _, retained, _ = self.run_build(facts())
        for mutation in ('ddl', 'type', 'comment', 'permission', 'code'):
            current = facts(2, 20)
            table = current['tables'][-1]
            if mutation == 'ddl':
                table['ddl_sha256'] = 'b' * 64
                table['observation']['binding']['ddl_sha256'] = table['ddl_sha256']
            elif mutation == 'type':
                table['columns'][0]['data_type'] = 'numeric'
            elif mutation == 'comment':
                table['columns'][0]['comment'] = 'Changed interpretation'
            elif mutation == 'permission':
                table['permissions'] = {'reader': False}
            else:
                current['enum_candidates'][-1]['values'].append({'code': 'pending', 'rows': 1})
            with self.subTest(mutation=mutation):
                result, _, calls = self.run_build(rehash(current), retained)
                self.assertEqual(calls, 1)
                self.assertIn('reuse', result[0])
                self.assertNotIn('reuse', result[1])

    def test_full_code_set_beyond_prompt_limit_is_bound(self):
        old = facts()
        old['enum_candidates'][-1]['values'] = [{'code': str(i), 'rows': 1} for i in range(30)]
        _, retained, _ = self.run_build(rehash(old))
        current = copy.deepcopy(old)
        current['enum_candidates'][-1]['values'][-1]['code'] = 'new-unseen-code'
        self.assertEqual(self.run_build(rehash(current), retained)[2], 1)

    def test_reordering_codes_and_changing_frequencies_does_not_change_request(self):
        old = facts()
        _, retained, _ = self.run_build(old)
        current = facts(2, 20)
        for item in current['enum_candidates']:
            item['values'].reverse()
        self.assertEqual(self.run_build(rehash(current), retained)[2], 0)

    def test_model_provider_rules_and_role_isolation(self):
        _, retained, _ = self.run_build(facts())
        for key, value in [('model', 'fixture/local-v2'), ('model_revision', 'revision-2'),
                           ('endpoint', 'https://other.invalid/v1'), ('semantic_rules_version', 'rules-2')]:
            with self.subTest(key=key):
                self.assertEqual(self.run_build(facts(2), retained, dict(self.config, **{key: value}))[2], 2)
        for key in ('project', 'role', 'principal', 'permissions_sha256'):
            config = copy.deepcopy(self.config)
            config['semantic_scope'][key] = 'b' * 64 if key.endswith('sha256') else 'other'
            with self.subTest(key=key):
                self.assertEqual(self.run_build(facts(2), retained, config)[2], 2)
        current = facts(2)
        current['tables'][-1]['source_id'] = 'source-b'
        current['tables'][-1]['observation']['binding']['source_id'] = 'source-b'
        current['binding_vector'][-1]['source_id'] = 'source-b'
        self.assertEqual(self.run_build(rehash(current), retained)[2], 1)
        with patch.object(lib.library_semantics, 'rules_digest', return_value='changed-implementation'):
            self.assertEqual(self.run_build(facts(2), retained)[2], 2)

    def test_missing_scope_is_not_cross_generation_authorization(self):
        config = {k: v for k, v in self.config.items() if k != 'semantic_scope'}
        _, retained, _ = self.run_build(facts(), config=config)
        self.assertEqual(self.run_build(facts(2), retained, config)[2], 2)
        bad = dict(self.config, semantic_scope={'project': 'fixture'})
        with self.assertRaisesRegex(ValueError, 'semantic_scope_invalid'):
            self.run_build(facts(), config=bad)

    def test_dictionary_addition_wakes_missing_binding_without_rebuilding_siblings(self):
        _, retained, _ = self.run_build(facts())
        current = facts(2)
        current['enum_candidates'][-1]['dictionary_candidates'] = [{
            'dataset': 'app.t00', 'discriminator': 'status', 'code_column': 'status',
            'type_column': 'id', 'containment': '1.000000', 'matched': 2,
            'dictionary_distinct': 2, 'method': 'exact_type_scoped_code_overlap',
        }]
        self.assertEqual(self.run_build(rehash(current), retained)[2], 1)
        # The formerly missing dictionary's schema is also a dependency.
        _, bound, _ = self.run_build(current)
        current['tables'][0]['columns'][0]['data_type'] = 'text'
        self.assertEqual(self.run_build(rehash(current), bound)[2], 2)

    def test_pinned_dictionary_document_addition_and_meaning_change(self):
        from test_dictionary_evidence import fixture
        tables, document = fixture()
        other = copy.deepcopy(tables[0])
        other['dataset'] = 'app.unrelated'
        other['physical'] = '"fixture"."unrelated"'
        tables += observed_fixture([other])
        base = lib.build_facts(tables, lambda *a: {'non_null': 0, 'matched': 0, 'parent_unique': False})
        config = dict(self.config, max_batch_context_bytes=1)
        _, retained, _ = self.run_build(base, config=config)

        def bind(description):
            document['candidates'][0]['description'] = description
            document['evidence_sha256'] = lib.digest({k: v for k, v in document.items()
                                                     if k != 'evidence_sha256'})
            path = self.root / (lib.digest(document) + '.json')
            lib.write(path, document)
            pin = {'path': str(path), 'sha256': hashlib.sha256(path.read_bytes()).hexdigest(),
                   **{k: document[k] for k in ('dataset', 'dictionary_sha256', 'ddl_sha256')}}
            return lib.dictionary_evidence.enrich(base, pin), dict(config, dictionary_evidence_input=pin)

        enriched, config = bind('Independent dictionary description, still pending')
        receipts, retained, calls = self.run_build(enriched, retained, config)
        self.assertEqual(calls, 1)
        self.assertNotIn('reuse', receipts[0])
        revised, config = bind('Revised meaning, still pending')
        self.assertEqual(self.run_build(revised, retained, config)[2], 1)
        self.assertFalse(revised['dictionary_evidence']['document']['business_semantics_approved'])

    def test_complete_declared_dictionary_reuses_generation_but_rejects_new_label(self):
        from test_declared_enums import fixture
        tables, adapter, declaration, value = fixture()
        config = dict(self.config, declared_enum_adapter=adapter, max_batch_context_bytes=1)

        def observed(generation, label):
            current = copy.deepcopy(tables)
            for table in current:
                table.update(source_generation=generation, target_generation=generation)
                for column in table['columns']:
                    column['profile'] = {'rows': generation * 2, 'distinct': 2}
            observed_fixture(current)
            declared = lib.declared_enums.discover(
                Mock(query=Mock(side_effect=[[declaration], [dict(value, label=label)]])), current, adapter)
            return lib.build_facts(current, lambda *a: {}, declared=declared)

        old = observed(1, 'paid')
        _, retained, _ = self.run_build(old, config=config)
        self.assertEqual(self.run_build(observed(2, 'paid'), retained, config)[2], 0)
        self.assertEqual(self.run_build(observed(2, 'unpaid'), retained, config)[2], 1)

    def test_overlap_only_dictionary_requires_reobservation_on_generation_change(self):
        old = facts()
        old['enum_candidates'][-1]['dictionary_candidates'] = [{
            'dataset': 'app.t00', 'code_column': 'status', 'containment': '1',
            'method': 'exact_type_scoped_code_overlap'}]
        _, retained, _ = self.run_build(rehash(old))
        current = facts(2)
        current['enum_candidates'][-1]['dictionary_candidates'] = copy.deepcopy(
            old['enum_candidates'][-1]['dictionary_candidates'])
        self.assertEqual(self.run_build(rehash(current), retained)[2], 1)

    def test_incomplete_or_tampered_observation_never_reaches_model(self):
        old = facts()
        _, retained, _ = self.run_build(old)
        for mutation in ('facts_sha', 'incomplete_enum', 'incomplete_column'):
            current = facts(2)
            if mutation == 'facts_sha':
                current['facts_sha256'] = old['facts_sha256']
            elif mutation == 'incomplete_enum':
                current['enum_candidates'][0]['observation_error'] = 'failed'
                rehash(current)
            else:
                current['tables'][0]['observation']['state'] = 'failed'
                rehash(current)
            before = self.transport.call_count
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                self.run_build(current, retained)
            self.assertEqual(self.transport.call_count, before)

    def test_absent_relation_wakes_and_neighbor_schema_invalidates(self):
        old = facts()
        _, retained, _ = self.run_build(old)
        current = facts(2)
        relation = {'name': 'rel_status', 'from': {'dataset': 'app.t16', 'columns': ['status']},
                    'to': {'dataset': 'app.t00', 'columns': ['id']},
                    'confidence': '0.50', 'status': 'candidate',
                    'evidence': {'non_null': 10, 'matched': 0, 'parent_unique': True,
                                 'containment': '0', 'row_containment': '0'}}
        current['relationship_candidates'].append(relation)
        self.assertEqual(self.run_build(rehash(current), retained)[2], 2)
        _, retained, _ = self.run_build(current)
        relation['evidence'].update(matched=10, containment='1', row_containment='1')
        relation['confidence'] = '0.99'
        self.assertEqual(self.run_build(rehash(current), retained)[2], 2)

    def test_relation_counts_reuse_but_null_unmatched_and_fanout_changes_invalidate(self):
        old = facts()
        relation = {'name': 'rel_status', 'from': {'dataset': 'app.t16', 'columns': ['status']},
                    'to': {'dataset': 'app.t00', 'columns': ['id']},
                    'confidence': '0.99', 'status': 'candidate',
                    'evidence': {'non_null': 10, 'matched': 10, 'parent_unique': True,
                                 'containment': '1', 'row_containment': '1'}}
        old['relationship_candidates'].append(relation)
        _, retained, _ = self.run_build(rehash(old))
        current = facts(2, 20)
        current['relationship_candidates'] = copy.deepcopy(old['relationship_candidates'])
        current['relationship_candidates'][0]['evidence'].update(non_null=20, matched=20)
        self.assertEqual(self.run_build(rehash(current), retained)[2], 0)
        for key, value in [('parent_unique', False), ('row_containment', '0.95')]:
            changed = copy.deepcopy(current)
            changed['relationship_candidates'][0]['evidence'][key] = value
            self.assertEqual(self.run_build(rehash(changed), retained)[2], 2)
        changed = copy.deepcopy(current)
        column = changed['tables'][-1]['columns'][0]
        column['profile'] = lib.observation.profiles({'rows': 20, 'columns': [{
            'name': column['name'], 'non_null': 19, 'distinct': 19, 'blank_rows': 0,
            'digits_rows': 0, 'leading_zero_rows': 0, 'min_length': 1, 'max_length': 3,
        }]}, [column['name']])[column['name']]
        batch = lib.draft_batches(changed)[-1]
        prompt = json.loads(lib.draft_request(batch, self.config)[0]['messages'][1]['content'].split('\n', 1)[1])
        self.assertTrue(prompt['tables'][0]['columns'][0]['profile']['null_present'])
        self.assertNotIn('null_frac', prompt['tables'][0]['columns'][0]['profile'])
        self.assertEqual(self.run_build(rehash(changed), retained)[2], 2)

    def test_unrelated_inventory_addition_keeps_existing_partitions(self):
        _, retained, _ = self.run_build(facts())
        current = facts(2)
        extra = copy.deepcopy(current['tables'][0])
        extra['dataset'] = 'app.a_new_table'
        extra['physical'] = '"fixture"."a_new_table"'
        observed_fixture([extra])
        current['tables'].append(extra)
        current['binding_vector'].append({k: extra[k] for k in
                                         ('dataset', 'source_id', 'source_generation', 'target_generation')})
        result, _, calls = self.run_build(rehash(current), retained)
        self.assertEqual(calls, 1, 'only the newly added table needs a draft')
        self.assertTrue(all('reuse' in r for r in result[:2]))

    def test_new_mapping_table_reruns_real_discovery_and_wakes_only_consumer_batch(self):
        tables = facts()['tables']
        tables[-1]['columns'].append({'name': 'owner_id', 'data_type': 'bigint', 'comment': '',
                                     'profile': {'rows': 10, 'distinct': 2}})
        observe = lambda *a: {'non_null': 10, 'matched': 10, 'parent_unique': True}
        old = lib.build_facts(observed_fixture(tables), observe)
        self.assertEqual(old['relationship_candidates'], [])
        _, retained, _ = self.run_build(old)
        tables.append({'dataset': 'app.owner', 'source_id': 'source-a', 'source_generation': 2,
                       'target_generation': 2, 'primary_key': ['id'], 'columns': [
                           {'name': 'id', 'data_type': 'bigint', 'comment': 'identifier',
                            'profile': {'rows': 10, 'distinct': 10}}]})
        current = lib.build_facts(observed_fixture(tables), observe)
        self.assertEqual(len(current['relationships']), 1)
        receipts, _, calls = self.run_build(current, retained)
        self.assertEqual(calls, 2, 'new parent and the old missing-mapping consumer, not unrelated tables')
        self.assertNotIn('reuse', receipts[-2])
        self.assertEqual(receipts[0]['request_sha256'],
                         json.loads(Path(retained['path']).read_text())['receipts'][0]['request_sha256'])

    def test_tampered_cache_and_legacy_cross_generation_are_refused(self):
        receipts, retained, _ = self.run_build(facts())
        batch = lib.draft_batches(facts())[0]
        changed = copy.deepcopy(receipts[0])
        changed['draft']['datasets'][0]['description'] = 'tampered'
        with self.assertRaisesRegex(ValueError, 'draft_cache_binding'):
            lib.reuse_draft(changed, batch, self.config)
        legacy = [{'facts_sha256': r['facts_sha256'], 'model': r['model'],
                   'draft_contract': r['draft_contract'], 'draft': r['draft']} for r in receipts]
        path = self.root / 'legacy.json'
        lib.write(path, {'receipts': legacy})
        pin = {'path': str(path), 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()}
        with self.assertRaisesRegex(ValueError, 'retained_draft_binding'):
            self.run_build(facts(2), pin)

    @unittest.skipUnless(os.environ.get('OO_LIBRARY_BUILDER_BIN'), 'requires local Rust compiler')
    def test_reused_s5_through_s8_review_yaml_and_real_pack_matches_fresh_build(self):
        _, retained, _ = self.run_build(facts())
        current = facts(2, 20)
        config = dict(self.config, psql_command=['false'], retained_drafts=retained)
        output = self.root / 'stages'
        output.mkdir()
        lib.write(output / 'facts.json', current)
        lib.write(self.root / 'config.json', config)
        lib.write(self.root / 'manifest.json', {})
        request = {'work_dir': str(output), 'config': str(self.root / 'config.json'),
                   'manifest': str(self.root / 'manifest.json'), 'llm': {}}
        count = self.transport.call_count
        for stage in ('S5', 'S6', 'S7', 'S8'):
            result = control.run(dict(request, stage=stage))
            self.assertFalse(result['published'])
        self.assertEqual(self.transport.call_count, count)
        review = {'project': 'fixture', 'role': 'oo_r_reader', 'principal': 'reader',
                  'columns': {t['dataset'] + '.id': {
                      'pii': 'none', 'semantic_type': 'identifier', 'reviewer': 'fixture/test'}
                      for t in current['tables']},
                  'hidden_columns': [t['dataset'] + '.status' for t in current['tables']],
                  'datasets': {}, 'labels': {}, 'relationships': []}
        lib.write(output / 'ledger-review.json', review)
        control.run(dict(request, stage='compile'))
        effective = json.loads((output / 'effective-oos.json').read_text())
        visibility = json.loads((output / 'visibility.json').read_text())
        ceilings = output / 'ceilings.json'
        lib.write(ceilings, visibility['limits'])
        binary = Path(os.environ['OO_LIBRARY_BUILDER_BIN'])
        direct = subprocess.run([str(binary), 'library-role-pack', str(ceilings)],
                                input=(output / 'compiler-request.json').read_text(),
                                text=True, capture_output=True)
        self.assertEqual(direct.returncode, 0, direct.stderr)
        lib.write(output / 'direct-pack.json', json.loads(direct.stdout))
        tree = subprocess.run([str(binary.with_name('oos-tree')), str(output / 'ontology'),
                               str(output / 'oos-tree-binding.json'), str(output / 'visibility.json'),
                               str(ceilings), str(output / 'roundtrip.json')], capture_output=True, text=True)
        self.assertEqual(tree.returncode, 0, tree.stderr)
        compiled = json.loads((output / 'roundtrip.json').read_text())
        self.assertEqual(compiled['effective_oos'], effective)
        self.assertEqual(compiled['pack_digest'], json.loads(direct.stdout)['pack_digest'])
        self.assertFalse(compiled['authenticated'])
        self.assertFalse(compiled['published'])
        self.assertFalse(compiled['business_owner_signed'])
        fresh, _, _ = self.run_build(current)
        expected, expected_visibility, _ = control.reviewed_documents(current, fresh, review)
        self.assertEqual(expected, effective)
        self.assertEqual(expected_visibility, visibility)
        self.assertTrue(all(t['columns'] == ['id'] for t in visibility['datasets']))
        with self.assertRaisesRegex(ValueError, 'required_.*unavailable'):
            lib.library_assets.require_pack_assets(compiled['artifact'], {'examples': ['missing-independent-gold']})
        if os.environ.get('OO_SEMANTIC_EVIDENCE_DIR'):
            shutil.copytree(self.root, Path(os.environ['OO_SEMANTIC_EVIDENCE_DIR']) / 'roundtrip-fixture')


if __name__ == '__main__':
    unittest.main()
