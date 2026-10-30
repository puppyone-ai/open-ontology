import copy
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import library_control as control
import library_metrics as metrics
import library_selfbuild as lib
from observation_test_support import observed_fixture


def fixture():
    tables = [{'dataset': 'app.shipments', 'source_id': 'fixture',
               'source_generation': 1, 'target_generation': 1, 'primary_key': ['id'],
               'columns': [{'name': 'id', 'data_type': 'bigint', 'comment': 'identifier',
                            'profile': {'rows': 3, 'distinct': 3}},
                           {'name': 'weight', 'data_type': 'numeric', 'comment': 'weight in kg',
                            'profile': {'rows': 3, 'distinct': 3}},
                           {'name': 'event_date', 'data_type': 'timestamp', 'comment': 'record date',
                            'profile': {'rows': 3, 'distinct': 3}}]}]
    facts = lib.build_facts(observed_fixture(tables), lambda *a: {})
    draft = {'datasets': [], 'glossary': [], 'examples': [], 'metrics': [{
        'dataset': 'app.shipments', 'metric': 'shipment_weight', 'label': 'Recorded weight',
        'unit': 'kg', 'sql': 'SELECT sum(weight) FROM app.shipments',
        'grain': ['Visible shipment records'],
        'caveats': ['Local imported snapshot only; not complete source history.']}]}
    return facts, draft


def approval(facts, draft):
    candidate = metrics.candidates(facts['tables'], [draft])[0]
    definition = dict(candidate['definition'], owner='fixture/test', certified=True,
        status='active', default_anchor='month_start', tests=[{'params': {}, 'expect': {'rows': 1}}],
        control_total={'as_of': 'month_start', 'source': 'fixture',
                       'sql': 'SELECT sum(weight) FROM app.shipments', 'tolerance': {'abs': '0'}})
    return {'shipment_weight': {'candidate_sha256': candidate['candidate_sha256'],
                               'reviewer': 'fixture/test', 'definition': definition}}


class MetricPipelineTest(unittest.TestCase):
    def test_coverage_failure_supplies_bounded_missing_names_for_equivalent_retry(self):
        facts, draft = fixture()
        batch = lib.draft_batches(facts)[0]
        draft['datasets'] = [{'dataset': t['dataset'], 'columns': [{'name': c['name']} for c in t['columns'][:-1]]}
                             for t in batch['tables']]
        raw = json.dumps({'choices': [{'finish_reason': 'stop',
                          'message': {'content': json.dumps(draft)}}]}).encode()
        with patch.object(lib.model_provider, 'credential', return_value='fixture-key'), \
                patch.object(lib.model_provider, 'endpoint', return_value='https://fixture.invalid/v1'), \
                patch.object(lib.model_provider, 'open_request', return_value=io.BytesIO(raw)):
            with self.assertRaises(lib.DraftValidationError) as raised:
                lib.call_model(batch, {})
        feedback = raised.exception.feedback
        self.assertEqual(feedback['missing_columns'], [batch['tables'][0]['columns'][-1]['name']])
        request, _ = lib.draft_request(batch, {'_validation_feedback': feedback})
        self.assertIn('return the entire JSON object', request['messages'][-1]['content'])
        self.assertIn(feedback['missing_columns'][0], request['messages'][-1]['content'])
        bad = dict(feedback, missing_columns=['private.not_in_input'])
        with self.assertRaisesRegex(ValueError, 'feedback_invalid'):
            lib.draft_request(batch, {'_validation_feedback': bad})

    def test_partial_stream_evidence_is_preserved_privately_without_credentials(self):
        facts, _ = fixture()
        batch = lib.draft_batches(facts)[0]
        with tempfile.TemporaryDirectory() as root:
            archive = Path(root) / 'evidence'
            error = lib.model_provider.CompletionStreamError('s5_stream_incomplete',
                                                           b'data: private-test-key\n\n')
            with patch.object(lib.model_provider, 'credential', return_value='private-test-key'), \
                    patch.object(lib.model_provider, 'endpoint', return_value='https://fixture.invalid/v1'), \
                    patch.object(lib.model_provider, 'open_request', return_value=io.BytesIO(b'')), \
                    patch.object(lib.model_provider, 'read_completion_stream', side_effect=error):
                with self.assertRaisesRegex(ValueError, 's5_stream_incomplete'):
                    lib.call_model(batch, {'model_stream': True, 'model_evidence_directory': str(archive)})
            files = list(archive.iterdir())
            self.assertEqual(len(files), 2)
            self.assertTrue(any(p.suffix == '.sse' for p in files))
            self.assertTrue(all(p.stat().st_mode & 0o077 == 0 for p in files))
            self.assertTrue(all(b'private-test-key' not in p.read_bytes() for p in files))

    def test_candidates_survive_merge_effective_and_yaml_without_fixture_certification(self):
        facts, draft = fixture()
        merged = lib.merge_drafts(facts, [draft])
        self.assertEqual(len(merged['metric_candidates']), 1)
        effective, _, audit = lib.effective_documents(facts, [draft], fixture_review=True)
        self.assertEqual(effective['metrics'][0]['status'], 'draft')
        self.assertFalse(effective['metrics'][0]['certified'])
        self.assertNotIn('owner', effective['metrics'][0])
        self.assertEqual(audit['metric_projection'][0]['state'], 'pending')
        self.assertEqual(audit['metric_generation']['candidate_count'], 1)
        from oos_tree import source_tree
        files, _ = source_tree(effective, effective)
        self.assertEqual(files['generated/metrics/part-0000.yaml']['metrics'], effective['metrics'])
        self.assertFalse(any(path.startswith('curated/metrics/') for path in files))

    def test_model_cannot_approve_invent_bindings_or_silently_replace_conflicting_metrics(self):
        facts, draft = fixture()
        for field, value in [('certified', True), ('owner', 'attacker'), ('status', 'active'),
                             ('control_total', {}), ('tests', [])]:
            bad = copy.deepcopy(draft)
            bad['metrics'][0][field] = value
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, 's5_metric_fields'):
                lib.merge_drafts(facts, [bad])
        for field, value in [('dataset', 'secret.other'), ('time_dimension', 'app.shipments.absent')]:
            bad = copy.deepcopy(draft)
            bad['metrics'][0][field] = value
            with self.assertRaisesRegex(ValueError, 'binding'):
                lib.merge_drafts(facts, [bad])
        other = copy.deepcopy(draft)
        other['metrics'][0]['sql'] = 'SELECT count(*) FROM app.shipments'
        with self.assertRaisesRegex(ValueError, 's5_metric_conflict'):
            lib.merge_drafts(facts, [draft, other])
        self.assertEqual(len(metrics.candidates(facts['tables'], [draft, draft])), 1)

    def test_local_time_column_resolves_only_inside_explicit_dataset_without_mutating_response(self):
        facts, draft = fixture()
        draft['metrics'][0]['time_dimension'] = 'event_date'
        candidate = metrics.candidates(facts['tables'], [draft])[0]
        self.assertEqual(candidate['definition']['time_dimension'], 'app.shipments.event_date')
        self.assertEqual(draft['metrics'][0]['time_dimension'], 'event_date')
        draft['metrics'][0]['time_dimension'] = 'unknown_time'
        with self.assertRaisesRegex(ValueError, 's5_metric_time_binding'):
            metrics.candidates(facts['tables'], [draft])

    def test_empty_optional_hints_mean_absent_without_changing_model_evidence(self):
        facts, draft = fixture()
        for value in ('', None):
            original = copy.deepcopy(draft)
            original['metrics'][0].update(unit=value, time_dimension=value)
            before = copy.deepcopy(original)
            candidate = metrics.candidates(facts['tables'], [original])[0]
            self.assertNotIn('unit', candidate['definition'])
            self.assertNotIn('time_dimension', candidate['definition'])
            self.assertEqual(original, before)
            self.assertFalse(candidate['definition']['certified'])
        for value in (False, 0, [], {}, ' '):
            bad = copy.deepcopy(draft)
            bad['metrics'][0]['time_dimension'] = value
            with self.subTest(value=value), self.assertRaisesRegex(ValueError, 'time_dimension_invalid'):
                metrics.candidates(facts['tables'], [bad])

    def test_explicit_review_is_hash_bound_and_does_not_grant_columns(self):
        facts, draft = fixture()
        review = {'project': 'fixture', 'role': 'reader', 'principal': 'reader',
                  'columns': {}, 'hidden_columns': [], 'metrics': approval(facts, draft)}
        effective, visibility, audit = control.reviewed_documents(facts, [{'draft': draft}], review)
        self.assertTrue(effective['metrics'][0]['certified'])
        self.assertEqual(visibility['datasets'], [])
        self.assertEqual(audit['metric_projection'][0]['state'], 'projected')
        self.assertFalse(audit['business_owner_signed'])
        from oos_tree import source_tree
        generated, _, _ = lib.effective_documents(facts, [draft])
        files, _ = source_tree(generated, effective)
        self.assertFalse(files['generated/metrics/part-0000.yaml']['metrics'][0]['certified'])
        self.assertTrue(files['curated/metrics/part-0000.yaml']['metrics'][0]['certified'])
        for mutate, error in [
            (lambda r: r['shipment_weight'].update(candidate_sha256='0'*64), 'stale'),
            (lambda r: r['shipment_weight']['definition'].update(sql='SELECT count(*) FROM app.shipments'), 'drift'),
            (lambda r: r['shipment_weight']['definition'].pop('tests'), 'incomplete'),
            (lambda r: r.update(unknown=r['shipment_weight']), 'unknown'),
        ]:
            broken = copy.deepcopy(review)
            mutate(broken['metrics'])
            with self.assertRaisesRegex(ValueError, error):
                control.reviewed_documents(facts, [{'draft': draft}], broken)

    def test_operator_input_requires_pin_project_and_facts_and_never_executes_sql(self):
        facts, draft = fixture()
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'review.json'
            document = {'protocol': 'oo.library.metric-review/v1', 'project': 'fixture',
                        'facts_sha256': facts['facts_sha256'], 'reviews': approval(facts, draft)}
            path.write_text(json.dumps(document))
            config = {'metric_review_input': {'path': str(path), 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()}}
            self.assertEqual(metrics.load_reviews(config, facts, 'fixture'), document['reviews'])
            with self.assertRaisesRegex(ValueError, 'binding'):
                metrics.load_reviews(config, facts, 'other')
            with self.assertRaisesRegex(ValueError, 'binding'):
                metrics.load_reviews(config, dict(facts, facts_sha256='0'*64), 'fixture')
            path.write_text('{}')
            with self.assertRaisesRegex(ValueError, 'digest'):
                metrics.load_reviews(config, facts, 'fixture')

    def test_new_drafting_contract_asks_for_metrics_and_missing_field_is_not_success(self):
        facts, draft = fixture()
        batch = lib.draft_batches(facts)[0]
        self.assertEqual(batch['draft_contract'], metrics.DRAFT_CONTRACT)
        draft['datasets'] = [{'dataset': t['dataset'], 'columns': [{'name': c['name']} for c in t['columns']]}
                             for t in batch['tables']]
        requests = []
        def provider(request, **kwargs):
            requests.append(json.loads(request.data))
            body = json.dumps({'choices': [{'finish_reason': 'stop',
                'message': {'content': json.dumps(draft)}}]}).encode()
            return io.BytesIO(body)
        with patch.object(lib.model_provider, 'credential', return_value='fixture-key'), \
                patch.object(lib.model_provider, 'endpoint', return_value='https://fixture.invalid/v1'), \
                patch.object(lib.model_provider, 'open_request', side_effect=provider):
            receipt = lib.call_model(batch, {})
            self.assertEqual(receipt['draft_contract'], metrics.DRAFT_CONTRACT)
            self.assertIn('metrics:', requests[0]['messages'][0]['content'])
            self.assertIn('Do not rewrite existing source comments', requests[0]['messages'][1]['content'])
            self.assertIn('Still list every column name exactly once', requests[0]['messages'][1]['content'])
            draft.pop('metrics')
            with self.assertRaisesRegex(ValueError, 's5_metrics_missing'):
                lib.call_model(batch, {})

    def test_old_drafts_are_visible_in_audit_but_cannot_be_reused_as_new_s5(self):
        facts, _ = fixture()
        _, _, audit = lib.effective_documents(facts, [{'datasets': []}])
        self.assertEqual(audit['metric_generation']['drafts_without_metrics_contract'], 1)
        self.assertFalse(metrics.current_draft({'draft_contract': metrics.DRAFT_CONTRACT, 'draft': {}}))
        self.assertFalse(metrics.current_draft({'draft_contract': metrics.DRAFT_CONTRACT, 'draft': {'metrics': None}}))
        self.assertFalse(metrics.current_draft({'draft_contract': metrics.DRAFT_CONTRACT, 'draft': {'metrics': []}}))
        self.assertTrue(metrics.current_draft({'draft_contract': metrics.DRAFT_CONTRACT,
                                              'draft': {k: [] for k in ('metrics', 'datasets', 'glossary', 'examples')}}))
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'drafts.json'
            path.write_text(json.dumps({'receipts': [{'facts_sha256': facts['facts_sha256'],
                'model': 'fixture', 'draft': {'datasets': []}}]}))
            config = {'model': 'fixture', 'retained_drafts': {
                'path': str(path), 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()}}
            with self.assertRaisesRegex(ValueError, 'retained_draft_contract'):
                control.retained_drafts(facts, config, Path(root))

    def test_control_merge_exposes_bound_candidates_at_existing_candidates_endpoint(self):
        facts, draft = fixture()
        draft['datasets'] = [{'dataset': t['dataset'], 'columns': [{'name': c['name']} for c in t['columns']]}
                             for t in facts['tables']]
        receipt = {'draft': draft, 'facts_sha256': facts['facts_sha256'],
                   'draft_contract': metrics.DRAFT_CONTRACT}
        with tempfile.TemporaryDirectory() as root:
            root = Path(root)
            for name, value in [
                ('facts.json', facts), ('drafts.json', {'receipts': [receipt]}),
                ('config.json', {'psql_command': ['/never-execute']}), ('manifest.json', {}),
            ]:
                (root / name).write_text(json.dumps(value))
            control.run({'stage': 'S6', 'work_dir': str(root),
                         'config': str(root / 'config.json'), 'manifest': str(root / 'manifest.json')})
            suggestions = json.loads((root / 'suggestions.json').read_text())
            self.assertEqual(suggestions['facts_sha256'], facts['facts_sha256'])
            self.assertEqual(suggestions['metric_candidates'], metrics.candidates(facts['tables'], [draft]))
            self.assertFalse(suggestions['certified'])
            control.run({'stage': 'S8', 'work_dir': str(root),
                         'config': str(root / 'config.json'), 'manifest': str(root / 'manifest.json')})
            self.assertTrue((root / 'generated-ontology/generated/metrics/part-0000.yaml').is_file())
            generated = json.loads((root / 'generated-oos.json').read_text())
            self.assertFalse(generated['metrics'][0]['certified'])
            self.assertFalse((root / 'pack.json').exists())

    def test_required_metrics_fail_for_empty_candidate_only_or_missing_inventory(self):
        for invalid in ('all', ['x', 'x'], [True], ['bad.name']):
            with self.assertRaisesRegex(ValueError, 'required_metrics_invalid'):
                metrics.requirements({'required_metrics': invalid})
        for available in ([], ['another']):
            artifact = {'files': [{'path': 'metrics/availability.json', 'text': json.dumps({
                'protocol': 'oo.knowledge.metric-availability/v1',
                'certified_metrics': available, 'candidate_metrics': ['shipment_weight']})}]}
            with self.assertRaisesRegex(ValueError, 'required_metrics_unavailable'):
                metrics.require_pack_metrics(artifact, ['shipment_weight'])
        with self.assertRaisesRegex(ValueError, 'metric_availability_missing'):
            metrics.require_pack_metrics({'files': []}, ['shipment_weight'])

    @unittest.skipUnless(os.environ.get('OO_LIBRARY_BUILDER_BIN'), 'real builder not configured')
    def test_real_compiler_only_publishes_visible_explicit_reviews_and_reports_pending(self):
        facts, draft = fixture()
        with tempfile.TemporaryDirectory() as root:
            root = Path(root)
            (root / 'ceilings.json').write_text(json.dumps({
                'max_file_bytes': 4_194_304, 'max_total_bytes': 67_108_864, 'max_entries': 16384}))
            config = {'fixture_review': True, 'builder_binary': os.environ['OO_LIBRARY_BUILDER_BIN'],
                      'pack_ceilings_file': str(root / 'ceilings.json'),
                      'include_candidate_metrics': True}
            candidate_output = root / 'candidate'
            candidate_output.mkdir()
            pack = lib.compile_pack(facts, [{'draft': draft}], config, candidate_output)
            files = {f['path']: f['text'] for f in pack['artifact']['files']}
            self.assertIn('### Candidate: shipment_weight', files['metrics.md'])
            self.assertIn('NOT CERTIFIED', files['metrics.md'])
            rejected_output = root / 'rejected'
            rejected_output.mkdir()
            with self.assertRaisesRegex(ValueError, 'required_metrics_unavailable'):
                lib.compile_pack(facts, [{'draft': draft}], dict(config, required_metrics=['shipment_weight']), rejected_output)
            path = root / 'metric-review-input.json'
            path.write_text(json.dumps({'protocol': 'oo.library.metric-review/v1', 'project': 'library',
                'facts_sha256': facts['facts_sha256'], 'reviews': approval(facts, draft)}))
            config.update(required_metrics=['shipment_weight'], metric_review_input={
                'path': str(path), 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()})
            pack = lib.compile_pack(facts, [{'draft': draft}], config, root)
            files = {f['path']: f['text'] for f in pack['artifact']['files']}
            self.assertIn('## shipment_weight', files['metrics.md'])
            self.assertEqual(json.loads(files['metrics/availability.json'])['certified_metrics'], ['shipment_weight'])
            from oos_tree import write_source_tree
            generated, _, _ = lib.effective_documents(facts, [draft])
            effective = json.loads((root / 'effective-oos.json').read_text())
            visibility = json.loads((root / 'visibility.json').read_text())
            binding = write_source_tree(root / 'ontology', generated, effective)
            (root / 'binding.json').write_text(json.dumps(binding))
            tree = Path(config['builder_binary']).with_name('oos-tree')
            result = subprocess.run([str(tree), str(root / 'ontology'), str(root / 'binding.json'),
                str(root / 'visibility.json'), str(root / 'ceilings.json'), str(root / 'tree-result.json')],
                capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads((root / 'tree-result.json').read_text())['effective_oos'], effective)
            self.assertTrue(visibility['datasets'])
            # A metric review never authorizes its underlying columns.
            hidden_output = root / 'hidden'
            hidden_output.mkdir()
            with self.assertRaisesRegex(ValueError, 'required_metrics_unavailable'):
                lib.compile_pack(facts, [{'draft': draft}], dict(config, fixture_review=False), hidden_output)


if __name__ == '__main__':
    unittest.main()
