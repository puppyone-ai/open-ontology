import copy
import hashlib
import json
import os
import subprocess
from pathlib import Path
import tempfile
import unittest

import library_assets as assets
import library_selfbuild as lib
from test_library_metrics import fixture


def example(draft):
    draft['examples'] = [{'dataset': 'app.shipments', 'id': 'app-shipments-count',
        'question': 'How many visible records?', 'sql': 'SELECT count(*) FROM app.shipments',
        'uses': ['dataset:app.shipments']}]


class SemanticAssetTest(unittest.TestCase):
    def test_release_versions_bind_both_outputs_without_granting_columns(self):
        facts, draft = fixture()
        effective, visibility, _ = lib.effective_documents(facts, [draft])
        before = copy.deepcopy(visibility['datasets'])
        versions = {'ontology': 'reviewed-20261023', 'catalog': 'snapshot-1',
                    'policy': 'restricted-2', 'knowledge': 'business-2'}
        assets.bind_release_versions(effective, visibility, versions)
        for key in ('ontology_version', 'catalog_version', 'policy', 'knowledge'):
            self.assertEqual(effective[key], visibility[key])
        self.assertEqual(effective['knowledge']['version'], 'business-2')
        self.assertEqual(before, visibility['datasets'])
        with self.assertRaisesRegex(ValueError, 'release_versions_invalid'):
            assets.bind_release_versions(effective, visibility, {'knowledge': '2'})

    def test_examples_are_candidates_until_bound_execution_review(self):
        facts, draft = fixture()
        example(draft)
        effective, _, audit = lib.effective_documents(facts, [draft])
        self.assertEqual(effective['examples'], [])
        candidate = audit['example_candidates'][0]
        checked = dict(candidate['definition'], verified_by='fixture/test', verified_at='2026-10-22',
            last_checked={'at': '2026-10-22', 'status': 'ok', 'result_hash': 'a'*64})
        review = {'examples': {'app-shipments-count': {'candidate_sha256': candidate['candidate_sha256'],
            'reviewer': 'fixture/test', 'evidence_sha256': 'a'*64, 'definition': checked}}}
        projected = assets.project(facts, [draft], effective, review)
        self.assertEqual(effective['examples'], [checked])
        self.assertEqual(projected['examples'][0]['state'], 'projected')
        from oos_tree import source_tree
        generated, _, _ = lib.effective_documents(facts, [draft])
        files, _ = source_tree(generated, effective)
        self.assertIn('curated/examples/part-0000.yaml', files)
        self.assertNotIn('generated/examples/part-0000.yaml', files)
        for field, value in [('sql', 'SELECT 1'), ('verified_by', 'other'),
                             ('last_checked', {'status': 'failed'})]:
            bad = copy.deepcopy(review)
            bad['examples']['app-shipments-count']['definition'][field] = value
            with self.assertRaisesRegex(ValueError, 'example_review_incomplete'):
                assets.project(facts, [draft], generated, bad)

    @unittest.skipUnless(os.environ.get('OO_LIBRARY_BUILDER_BIN'), 'real builder required')
    def test_reviewed_example_survives_actual_core_yaml_and_role_pack(self):
        facts, draft = fixture()
        example(draft)
        candidate = assets.example_candidates(facts['tables'], [draft])[0]
        definition = dict(candidate['definition'], verified_by='fixture/test', verified_at='2026-10-22',
                          last_checked={'at': '2026-10-22', 'status': 'ok', 'result_hash': 'a'*64})
        with tempfile.TemporaryDirectory() as root:
            root = Path(root)
            review = root / 'review-input.json'
            review.write_text(json.dumps({'protocol': 'oo.library.semantic-review/v1', 'project': 'library',
                'facts_sha256': facts['facts_sha256'], 'reviews': {'examples': {
                    'app-shipments-count': {'candidate_sha256': candidate['candidate_sha256'],
                        'reviewer': 'fixture/test', 'definition': definition, 'evidence_sha256': 'a'*64}}}}))
            ceilings = root / 'ceilings.json'
            ceilings.write_text(json.dumps({'max_file_bytes': 4194304,
                'max_total_bytes': 67108864, 'max_entries': 16384}))
            config = {'builder_binary': os.environ['OO_LIBRARY_BUILDER_BIN'],
                      'pack_ceilings_file': str(ceilings), 'fixture_review': True,
                      'required_assets': {'examples': ['app-shipments-count']},
                      'semantic_review_input': {'path': str(review),
                           'sha256': hashlib.sha256(review.read_bytes()).hexdigest()}}
            pack = lib.compile_pack(facts, [{'draft': draft}], config, root)
            self.assertTrue(any(f['path'] == 'examples/verified.sql' and 'app-shipments-count' in f['text']
                                for f in pack['artifact']['files']))
            tree = Path(config['builder_binary']).with_name('oos-tree')
            result = subprocess.run([str(tree), str(root / 'ontology'), str(root / 'oos-tree-binding.json'),
                                     str(root / 'visibility.json'), str(ceilings), str(root / 'roundtrip.json')],
                                    capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            compiled = json.loads((root / 'roundtrip.json').read_text())
            self.assertEqual(compiled['effective_oos']['examples'], [definition])

    def test_model_cannot_claim_execution_and_duplicate_definitions_conflict(self):
        facts, draft = fixture()
        example(draft)
        bad = copy.deepcopy(draft)
        bad['examples'][0]['verified_by'] = 'model'
        with self.assertRaisesRegex(ValueError, 's5_example_fields'):
            lib.merge_drafts(facts, [bad])
        other = copy.deepcopy(draft)
        other['examples'][0]['sql'] = 'SELECT sum(weight) FROM app.shipments'
        with self.assertRaisesRegex(ValueError, 's5_example_conflict'):
            assets.example_candidates(facts['tables'], [draft, other])

    def test_full_build_rejects_partial_old_and_duplicate_batch_receipts(self):
        facts, draft = fixture()
        draft['datasets'] = [{'dataset': t['dataset'], 'columns': [{'name': c['name']} for c in t['columns']]}
                             for t in facts['tables']]
        receipt = {'draft': draft, 'draft_contract': assets.DRAFT_CONTRACT,
                   'facts_sha256': facts['facts_sha256']}
        assets.require_draft_coverage(facts, [receipt])
        for invalid in ([], [receipt, receipt], [dict(receipt, draft_contract='v2')],
                        [dict(receipt, facts_sha256='old')]):
            with self.assertRaisesRegex(ValueError, 'semantic_draft_'):
                assets.require_draft_coverage(facts, invalid)

    def test_entity_generation_does_not_depend_on_pre_review_visibility(self):
        facts, _ = fixture()
        facts['tables'][0]['columns'][1]['classification_candidate'] = {
            'semantic_type': 'entity_name', 'pii': 'none', 'evidence': 'name_comment_heuristic'}
        generated, visibility, _ = lib.effective_documents(facts, [])
        self.assertEqual(visibility['datasets'], [])
        self.assertEqual(len(generated['entities']), 1)
        self.assertEqual(generated['datasets'][0]['entity'], generated['entities'][0]['entity'])

    def test_entity_order_matches_yaml_path_order_for_exact_roundtrip(self):
        facts, _ = fixture()
        prototype = facts['tables'][0]
        prototype['columns'][1]['classification_candidate'] = {
            'semantic_type': 'entity_name', 'pii': 'none', 'evidence': 'name_comment_heuristic'}
        facts['tables'] = [dict(copy.deepcopy(prototype), dataset='app.t_' + str(i)) for i in range(10)]
        generated, _, _ = lib.effective_documents(facts, [])
        from oos_tree import documents, assembled
        reconstructed = assembled(documents(generated), True)
        self.assertEqual(generated['entities'], reconstructed['entities'])

    def test_review_input_is_bound_to_project_facts_and_content(self):
        facts, _ = fixture()
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'reviews.json'
            path.write_text(json.dumps({'protocol': 'oo.library.semantic-review/v1',
                'project': 'library', 'facts_sha256': facts['facts_sha256'], 'reviews': {}}))
            config = {'semantic_review_input': {'path': str(path),
                      'sha256': hashlib.sha256(path.read_bytes()).hexdigest()}}
            self.assertEqual(assets.load_reviews(config, facts, 'library'), {})
            with self.assertRaisesRegex(ValueError, 'binding'):
                assets.load_reviews(config, facts, 'other')
            path.write_text('{}')
            with self.assertRaisesRegex(ValueError, 'digest'):
                assets.load_reviews(config, facts, 'library')

    def test_required_assets_check_actual_role_output_not_input_counts(self):
        artifact = {'files': [{'path': 'availability.json', 'text': json.dumps({
            'protocol': 'oo.knowledge.asset-availability/v1', 'available': {'examples': ['checked']}})}]}
        assets.require_pack_assets(artifact, {'examples': ['checked']})
        with self.assertRaisesRegex(ValueError, 'unavailable'):
            assets.require_pack_assets(artifact, {'examples': ['unreviewed']})
        with self.assertRaisesRegex(ValueError, 'invalid'):
            assets.requirements({'required_assets': {'examples': []}})

    def test_glossary_review_cannot_change_meaning(self):
        facts, draft = fixture()
        draft['glossary'] = [{'term': 'Shipments', 'definition': 'Visible shipment records',
                              'dataset': 'app.shipments'}]
        effective, _, _ = lib.effective_documents(facts, [draft])
        term = effective['glossary'][0]
        review = {'glossary': {term['term']: {'candidate_sha256': assets.digest(term),
            'reviewer': 'fixture/test', 'evidence_sha256': 'c'*64,
            'definition': dict(term, status='approved', owner='fixture/test')}}}
        assets.project(facts, [draft], effective, review)
        self.assertEqual(effective['glossary'][0]['status'], 'approved')
        baseline, _, _ = lib.effective_documents(facts, [draft])
        review['glossary']['Shipments']['definition']['definition'] = 'All company history'
        with self.assertRaisesRegex(ValueError, 'drift'):
            assets.project(facts, [draft], baseline, review)

    def test_enum_review_retains_unknown_codes_and_exact_evidence(self):
        facts, draft = fixture()
        facts['enum_candidates'] = [{'enum': 'app.shipments.id', 'dictionary_candidates': [],
            'values': [{'code': '1', 'rows': 2}, {'code': '9', 'rows': 1}]}]
        effective, _, _ = lib.effective_documents(facts, [draft])
        definition = copy.deepcopy(effective['enums'][0])
        definition.update(owner='fixture/test', coverage='0.5', unexplained_values=['9'])
        definition['classes']['normal'] = 'A reviewed example category'
        definition['values'][0].update(label='Normal', status='approved', **{'class': 'normal'},
            source='dictionary_table', review={'state': 'approved', 'by': 'fixture/test'})
        review = {'enums': {'app.shipments.id': {'candidate_sha256': assets.digest(facts['enum_candidates'][0]),
            'reviewer': 'fixture/test', 'evidence_sha256': 'd'*64, 'definition': definition}}}
        assets.project(facts, [draft], effective, review)
        self.assertEqual(effective['enums'][0]['unexplained_values'], ['9'])
        for mutate, error in [
            (lambda d: d['values'].pop(), 'coverage'),
            (lambda d: d['values'][0]['evidence'].update(rows=999), 'evidence_drift'),
            (lambda d: d.update(coverage='1'), 'coverage'),
            (lambda d: d['values'][1].update(status='approved'), 'incomplete'),
        ]:
            baseline, _, _ = lib.effective_documents(facts, [draft])
            bad = copy.deepcopy(review)
            mutate(bad['enums']['app.shipments.id']['definition'])
            with self.assertRaisesRegex(ValueError, error):
                assets.project(facts, [draft], baseline, bad)


if __name__ == '__main__':
    unittest.main()
