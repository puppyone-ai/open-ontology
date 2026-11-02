import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import library_build as build


class FullBuildTest(unittest.TestCase):
    def test_column_review_pin_cannot_be_reused_for_other_facts_or_project(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'review.json'
            document = {'protocol': 'oo.library.column-review/v1', 'project': 'p',
                        'facts_sha256': 'abc', 'review': {'project': 'p', 'columns': {}}}
            path.write_text(json.dumps(document))
            config = {'project': 'p', 'column_review_input': {'path': str(path),
                      'sha256': hashlib.sha256(path.read_bytes()).hexdigest()}}
            self.assertEqual(build.pinned_review(config, {'facts_sha256': 'abc'})['review_origin'],
                             'operator_pinned_file')
            with self.assertRaisesRegex(ValueError, 'binding'):
                build.pinned_review(config, {'facts_sha256': 'changed'})
            with self.assertRaisesRegex(ValueError, 'binding'):
                build.pinned_review(dict(config, project='other'), {'facts_sha256': 'abc'})
            path.write_text('{}')
            with self.assertRaisesRegex(ValueError, 'digest'):
                build.pinned_review(config, {'facts_sha256': 'abc'})

    def test_completed_stage_resume_checks_artifact_before_skipping_execution(self):
        with tempfile.TemporaryDirectory() as root:
            root = Path(root)
            artifact = root / 'facts.json'
            artifact.write_text('{}')
            receipt = {'artifact': 'facts.json',
                       'artifact_sha256': hashlib.sha256(artifact.read_bytes()).hexdigest()}
            (root / 'S1-receipt.json').write_text(json.dumps(receipt))
            with patch.object(build.control, 'run') as runner:
                self.assertEqual(build.stage({'stage': 'S1', 'work_dir': str(root)}), receipt)
                runner.assert_not_called()
                artifact.write_text('[]')
                with self.assertRaisesRegex(ValueError, 'drift'):
                    build.stage({'stage': 'S1', 'work_dir': str(root)})
                runner.assert_not_called()

    def test_full_entry_never_accepts_fixture_review(self):
        with tempfile.TemporaryDirectory() as root:
            root = Path(root)
            config = root / 'config.json'
            config.write_text('{"fixture_review":true}')
            with self.assertRaisesRegex(ValueError, 'fixture'):
                build.run(config, root / 'build')
            self.assertFalse((root / 'build').exists())

    def test_observation_resume_revalidates_oracle_before_returning_cached_facts(self):
        from test_library_control import LibraryControlTest
        facts = LibraryControlTest().facts()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / 'build'
            output.mkdir()
            build.lib.write(output / 'facts.json', facts)
            build.lib.write(output / 's0-s2.json', facts['tables'])
            build.lib.write(root / 'manifest.json', {
                'protocol': 'oo.library.manifest/v1',
                'inventory': [{'schema': 'app', 'table': 'items'}], 'exclusions': [],
                'sources': [{'source_id': facts['tables'][0]['source_id'],
                             'schema': 'app', 'tables': ['items']}]})
            oracle_input = {'source_id': 'caller-contract-only'}
            build.lib.write(root / 'config.json', {
                'library_manifest': str(root / 'manifest.json'), 'psql_command': ['false'],
                'oracle_observation_input': oracle_input})
            with patch.object(build, 'stage'), patch.object(
                    build.lib, 'validate_retained_tables',
                    side_effect=ValueError('oracle_retained_source_drift')) as validate:
                with self.assertRaisesRegex(ValueError, 'oracle_retained_source_drift'):
                    build.run(root / 'config.json', output, through='observation', resume=True)
                self.assertIsNone(validate.call_args.args[2])
                self.assertEqual(validate.call_args.kwargs, {'oracle_input': oracle_input})


    def test_compile_resume_rechecks_unchanged_grant_expiry_without_new_model_calls(self):
        from test_f20_read_grants import fixture
        with tempfile.TemporaryDirectory() as root:
            root = Path(root)
            facts, _, policy, config = fixture(root)
            build.lib.write(root / 'ledger-review.json', build.pinned_review(config, facts))
            request = {'stage': 'compile', 'work_dir': str(root),
                       'config': str(root / 'config.json'),
                       'manifest': str(root / 'manifest.json')}
            with patch.object(build.control.lib, 'call_model', side_effect=AssertionError('unexpected S5')):
                first = build.stage(request)
                before = {p.relative_to(root): p.read_bytes() for p in root.rglob('*') if p.is_file()}
                self.assertEqual(build.stage(request), first)
                expires = min(c['read_grant_expires_at'] for c in policy['review']['columns'].values()
                              if c.get('explicit_allow'))
                with patch('library_read_grants.time.time', return_value=expires):
                    with self.assertRaisesRegex(ValueError, 'read_grant_expired'):
                        build.stage(request)
                self.assertEqual(before, {p.relative_to(root): p.read_bytes()
                                         for p in root.rglob('*') if p.is_file()})

    def test_compile_resume_refuses_revoked_pin_and_keeps_successful_receipt(self):
        from test_f20_read_grants import fixture
        with tempfile.TemporaryDirectory() as root:
            root = Path(root)
            facts, _, _, config = fixture(root)
            build.lib.write(root / 'ledger-review.json', build.pinned_review(config, facts))
            request = {'stage': 'compile', 'work_dir': str(root),
                       'config': str(root / 'config.json'),
                       'manifest': str(root / 'manifest.json')}
            build.stage(request)
            receipt = (root / 'compile-receipt.json').read_bytes()
            artifact = (root / 'compiler-request.json').read_bytes()
            Path(config['column_review_input']['path']).write_text('{}')
            with patch.object(build.control.lib, 'call_model', side_effect=AssertionError('unexpected S5')):
                with self.assertRaisesRegex(ValueError, 'column_review_digest'):
                    build.stage(request)
            self.assertEqual((root / 'compile-receipt.json').read_bytes(), receipt)
            self.assertEqual((root / 'compiler-request.json').read_bytes(), artifact)

if __name__ == '__main__':
    unittest.main()
