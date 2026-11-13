import copy
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import library_control as control
import oos_tree
import test_library_control as fixtures


class OosTreeTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.binary = Path(os.environ.get('OO_OOS_TREE_BIN',
            Path(__file__).resolve().parents[3] / 'target/debug/oos-tree'))
        if not cls.binary.is_file():
            raise RuntimeError('Build first: cargo build --locked -p oo-builder --bin oos-tree')

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        facts = fixtures.LibraryControlTest().facts()
        self.generated, _, _ = control.lib.effective_documents(facts, [])
        self.effective, visibility, _ = control.reviewed_documents(facts, [], {
            'project': 'fixture', 'role': 'oo_r_reader', 'principal': 'reader',
            'columns': {'app.items.id': {
                'pii': 'none', 'semantic_type': 'identifier',
                'reviewer': 'fixture/test', 'label_approved': True}},
            'hidden_columns': ['app.items.phone'], 'labels': {}, 'relationships': [],
        })
        binding = oos_tree.write_source_tree(self.root / 'ontology', self.generated, self.effective)
        for name, value in [('binding', binding), ('visibility', visibility), ('ceilings', visibility['limits'])]:
            (self.root / (name + '.json')).write_text(json.dumps(value))

    def compile(self):
        return subprocess.run([str(self.binary), str(self.root / 'ontology'),
                               str(self.root / 'binding.json'), str(self.root / 'visibility.json'),
                               str(self.root / 'ceilings.json'), str(self.root / 'result.json')],
                              capture_output=True, text=True)

    def test_roundtrip_uses_core_merge_and_compiler_without_inventing_approval(self):
        result = self.compile()
        self.assertEqual(result.returncode, 0, result.stderr)
        data = json.loads((self.root / 'result.json').read_text())
        self.assertEqual(data['effective_oos'], self.effective)
        self.assertFalse(data['published'])
        self.assertFalse(data['business_owner_signed'])
        self.assertEqual(self.generated['datasets'][0]['columns'][0]['classification']['state'], 'pending')
        self.assertNotEqual(self.compile().returncode, 0, 'output must not be replaced')

    def test_repeated_export_is_idempotent_and_does_not_overwrite_curated(self):
        tree = self.root / 'ontology'
        oos_tree.write_source_tree(tree, self.generated, self.effective)
        p = tree / 'curated/datasets/app/items.yaml'
        p.write_text(p.read_text() + '# user edit\n')
        with self.assertRaisesRegex(ValueError, 'oos_tree_drift'):
            oos_tree.write_source_tree(tree, self.generated, self.effective)
        self.assertTrue(p.read_text().endswith('# user edit\n'))

    def test_unknown_yaml_field_is_rejected_by_core_schema(self):
        p = self.root / 'ontology/generated/datasets/app/items.yaml'
        p.write_text(p.read_text() + 'unknown_field: true\n')
        self.assertNotEqual(self.compile().returncode, 0)

    def test_exact_preview_companions_do_not_change_compilation(self):
        tree = self.root / 'ontology'
        (tree / 'explorer.html').write_text('<!doctype html>' + ' ' * 1048576)
        (tree / 'preview.puppyoneapp').write_text('{"type":"puppyone.app"}')
        (tree / 'ontology-preview.puppyoneapp').write_text('{"type":"puppyone.app"}')
        (tree / '.agent').write_text('# instructions\n')
        (tree / 'AGENTS.md').write_text('# instructions\n')
        (tree / '.cli').mkdir()
        (tree / '.cli/oo').write_text('#!/bin/sh\n')
        oos_tree.write_source_tree(tree, self.generated, self.effective)
        self.assertEqual((tree / 'preview.puppyoneapp').read_text(), '{"type":"puppyone.app"}')
        self.assertEqual((tree / 'ontology-preview.puppyoneapp').read_text(), '{"type":"puppyone.app"}')
        result = self.compile()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads((self.root / 'result.json').read_text())['effective_oos'],
                         self.effective)

    def test_other_html_is_not_silently_ignored(self):
        (self.root / 'ontology' / 'other.html').write_text('<html></html>')
        self.assertIn('tree.unsupported_path', self.compile().stderr)

    def test_arbitrary_preview_name_is_not_silently_ignored(self):
        (self.root / 'ontology' / 'other.puppyoneapp').write_text('{}')
        self.assertIn('tree.unsupported_path', self.compile().stderr)

    def test_preview_symlink_is_rejected(self):
        (self.root / 'ontology' / 'explorer.html').symlink_to(self.root / 'binding.json')
        self.assertNotEqual(self.compile().returncode, 0)

    def test_finder_metadata_is_not_parsed_as_yaml(self):
        (self.root / 'ontology/generated/.DS_Store').write_bytes(b'\x00\xff')
        self.assertEqual(self.compile().returncode, 0)

    def test_finder_metadata_symlink_is_rejected(self):
        (self.root / 'ontology/.DS_Store').symlink_to(self.root / 'binding.json')
        self.assertNotEqual(self.compile().returncode, 0)

    def test_duplicate_yaml_key_is_rejected(self):
        p = self.root / 'ontology/generated/datasets/app/items.yaml'
        p.write_text(p.read_text() + 'dataset: app.items\n')
        self.assertNotEqual(self.compile().returncode, 0)

    def test_legacy_onboarding_export_cli_keeps_its_contract(self):
        effective = self.root / 'effective.json'
        effective.write_text(json.dumps(self.effective))
        out = self.root / 'legacy'
        result = subprocess.run([sys.executable, str(oos_tree._exporter),
                                 '--effective', str(effective), '--out', str(out)],
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((out / 'datasets/app.items.yaml').is_file())
        self.assertTrue((out / 'effective-oos.yaml').is_file())
        self.assertEqual(json.loads((out / 'source.json').read_text())['publication'], 'not_claimed')

    def test_unhandled_oos_fields_are_not_dropped(self):
        changed = copy.deepcopy(self.effective)
        changed['calendar'] = {}
        with self.assertRaisesRegex(ValueError, 'unsupported_oos_field'):
            oos_tree.source_tree(self.generated, changed)

    def test_generated_fact_change_invalidates_review_binding(self):
        p = self.root / 'ontology/generated/datasets/app/items.yaml'
        p.write_text(p.read_text().replace('"description": "identifier"', '"description": "Changed source description"'))
        result = self.compile()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('review.stale_or_unbound_overlay', result.stderr)
        self.assertFalse((self.root / 'result.json').exists())

    def test_valid_curated_edit_requires_rebinding_review(self):
        p = self.root / 'ontology/curated/datasets/app/items.yaml'
        p.write_text(p.read_text().replace('"fixture/test"', '"other-reviewer"'))
        self.assertIn('review.stale_or_unbound_overlay', self.compile().stderr)

    def test_publish_policy_change_requires_rebinding(self):
        p = self.root / 'ontology/ontology.yaml'
        changed = copy.deepcopy(self.effective['ontology'])
        changed['publish']['include_unreviewed'] = ['relationships']
        changed_text = oos_tree.yaml(changed) + '\n'
        self.assertNotEqual(p.read_text(), changed_text)
        p.write_text(changed_text)
        self.assertIn('review.stale_or_unbound_overlay', self.compile().stderr)

    def test_duplicate_identity_is_refused(self):
        p = self.root / 'ontology/generated/datasets/app/items.yaml'
        (p.parent / 'other.yaml').write_bytes(p.read_bytes())
        self.assertNotEqual(self.compile().returncode, 0)

    def test_symlinks_are_refused(self):
        p = self.root / 'ontology/generated/datasets/app/items.yaml'
        saved = self.root / 'saved.yaml'
        saved.write_bytes(p.read_bytes())
        p.unlink()
        p.symlink_to(saved)
        self.assertNotEqual(self.compile().returncode, 0)
        with self.assertRaisesRegex(ValueError, 'oos_symlink'):
            oos_tree.write_source_tree(self.root / 'ontology', self.generated, self.effective)

    def test_membership_change_is_not_silently_reinterpreted_as_overlay(self):
        changed = copy.deepcopy(self.effective)
        changed['datasets'] = []
        with self.assertRaisesRegex(ValueError, 'review_changed_oos_membership'):
            oos_tree.source_tree(self.generated, changed)

    def test_library_compile_stage_emits_source_tree_and_binding(self):
        work = self.root / 'work'
        work.mkdir()
        facts = fixtures.LibraryControlTest().facts()
        review = {'project': 'fixture', 'role': 'oo_r_reader', 'principal': 'reader',
                  'columns': {}, 'hidden_columns': [], 'labels': {}, 'relationships': []}
        receipt = {'facts_sha256': facts['facts_sha256'], 'draft_contract': control.library_metrics.DRAFT_CONTRACT,
                   'draft': {'datasets': [{'dataset': t['dataset'], 'columns': [{'name': c['name']} for c in t['columns']]}
                                          for t in facts['tables']], 'metrics': [], 'glossary': [], 'examples': []}}
        for name, value in [('facts', facts), ('drafts', {'receipts': [receipt]}),
                            ('ledger-review', review)]:
            (work / (name + '.json')).write_text(json.dumps(value))
        (self.root / 'config.json').write_text(json.dumps({'psql_command': ['false']}))
        (self.root / 'manifest.json').write_text('{}')
        request = {'stage': 'compile', 'work_dir': str(work),
                   'config': str(self.root / 'config.json'), 'manifest': str(self.root / 'manifest.json')}
        first = control.run(request)
        self.assertEqual(first, control.run(request))
        self.assertTrue((work / 'ontology/ontology.yaml').is_file())
        self.assertTrue((work / 'ontology/generated/datasets/app/items.yaml').is_file())
        self.assertTrue((work / 'oos-tree-binding.json').is_file())
        self.assertFalse(first['business_owner_signed'])

    def test_fragmentation_stays_inside_core_file_budget(self):
        generated = copy.deepcopy(self.generated)
        generated['relationships'] = [{'name': f'r{i}'} for i in range(300)]
        fragments = oos_tree.documents(generated)
        self.assertEqual(len([p for p in fragments if p.startswith('relationships/')]), 3)
        self.assertEqual(len(oos_tree.assembled(fragments, True)['relationships']), 300)


if __name__ == '__main__':
    unittest.main()


class RelationshipEvidenceRoundTrip(unittest.TestCase):
    """Confirmed adjudication -> effective with evidence -> OOS tree -> same Pack."""
    @classmethod
    def setUpClass(cls):
        OosTreeTest.setUpClass.__func__(cls)

    def test_evidence_survives_the_yaml_round_trip_without_drift(self):
        import selfbuild_budget
        from test_adjudication import BUILDER_FIXTURE
        envelope = json.loads(BUILDER_FIXTURE.read_text())['complete']
        summary = envelope['adjudication']['summary_sha256']
        overrides = {'protocol': 'oo.adjudication-override/v1', 'summary_sha256': summary, 'items': {}}
        envelope.update(overrides=overrides, confirmation={
            'protocol': 'oo.publish-confirm/v1', 'summary_sha256': summary,
            'overrides_sha256': oos_tree.digest(overrides), 'reason': 'Round trip',
            'confirmed_at': '2026-11-11T12:00:00Z'})
        compiled = selfbuild_budget.bridge('reviewed', command=envelope)
        effective = compiled['effective']
        self.assertIn('relationship_evidence', effective)
        generated = {k: effective.get(k, []) for k in oos_tree.COLLECTIONS}
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            binding = oos_tree.write_source_tree(root / 'ontology', generated, effective)
            (root / 'binding.json').write_text(json.dumps(binding))
            (root / 'visibility.json').write_text(json.dumps(compiled['visibility']))
            (root / 'ceilings.json').write_text('{}')
            run = lambda out: subprocess.run(
                [str(self.binary), str(root / 'ontology'), str(root / 'binding.json'),
                 str(root / 'visibility.json'), str(root / 'ceilings.json'), str(root / out)],
                capture_output=True, text=True)
            result = run('result.json')
            self.assertEqual(result.returncode, 0, result.stderr)
            data = json.loads((root / 'result.json').read_text())
            self.assertEqual(data['effective_oos'], effective)
            self.assertEqual(data['pack_digest'], compiled['producer']['pack_digest'])
            evidence = root / 'ontology/evidence/relationships.json'
            evidence.write_text(evidence.read_text().replace('"inner"', '"left"', 1))
            self.assertNotEqual(run('tampered.json').returncode, 0)
