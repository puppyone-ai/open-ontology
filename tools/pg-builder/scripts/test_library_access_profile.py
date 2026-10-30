"""Offline synthetic profile tests; no customer source, model or database."""
from copy import deepcopy
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

import library_access_profile as profiles
import library_build
import library_control as control
import library_read_grants as grants
import library_selfbuild as lib
from observation_test_support import observed_fixture
from test_f20_read_grants import fixture, ledger_review, pin


class AccessProfileTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='oo-access-profile-test-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.source = self.root / 'source'
        self.facts, self.receipts, self.policy, self.config = fixture(self.source)
        self.expires = int(time.time()) + 3600
        for method in ('call_model', 'ReadOnlyPostgres.query'):
            guard = patch('library_selfbuild.' + method, side_effect=AssertionError('external IO forbidden'))
            guard.start()
            self.addCleanup(guard.stop)

    def configure(self, profile='full', config=None, name=None, **kwargs):
        output = self.root / (name or profile)
        report = profiles.configure(
            config or self.source / 'config.json', self.source / 'facts.json',
            self.source / 'drafts.json', profile, kwargs.get('reviewer', 'access-operator'),
            kwargs.get('expires_at', self.expires), output)
        return output, report

    def generated(self, output):
        config = json.loads((output / 'config.json').read_text())
        review = library_build.pinned_review(config, self.facts)
        return config, review

    def compile(self, output, name='compiled', http=False, **request_fields):
        config, review = self.generated(output)
        work = self.root / name
        work.mkdir()
        for file in ('facts.json', 'drafts.json', 'merged.json', 'manifest.json'):
            (work / file).write_bytes((self.source / file).read_bytes())
        if http:
            review = ledger_review({'review': review})
            review.pop('access_profile', None)
        lib.write(work / 'ledger-review.json', review)
        request = dict(stage='compile', work_dir=str(work), config=str(output / 'config.json'),
                       manifest=str(work / 'manifest.json'), **request_fields)
        control.run(request)
        return work, request

    def test_full_explicit_grants_counts_and_original_inputs_unchanged(self):
        before = {p.name: p.read_bytes() for p in self.source.iterdir()}
        output, report = self.configure()
        self.assertEqual(before, {p.name: p.read_bytes() for p in self.source.iterdir()})
        config, review = self.generated(output)
        self.assertEqual({k: v for k, v in config.items() if k != 'column_review_input'},
                         {k: v for k, v in self.config.items() if k != 'column_review_input'})
        self.assertEqual(report['counts'], {
            'tables_total': 1, 'tables_visible': 1, 'columns_total': 5, 'columns_visible': 4,
            'columns_hidden': 1, 'system_columns': 0, 'protected_columns': 1,
            'explicit_pii_grants': 3, 'visible_sensitive_including_unknown': 2})
        self.assertTrue(report['coverage']['all_eligible_columns_visible'])
        self.assertFalse(report['coverage']['all_business_columns_visible'])
        self.assertEqual(report['hidden_columns'], [{'column': 'app.items.hidden', 'reason': 'operator_protected'}])
        self.assertEqual(report['source_classification']['unknown'], {'total': 1, 'visible': 1, 'hidden': 0})
        self.assertEqual(report['read_grants_valid_until'], self.expires)
        note = review['columns']['app.items.note']
        self.assertEqual(note['pii'], 'sensitive')
        self.assertIn('unknown', note['reason'])
        for name, decision in review['columns'].items():
            self.assertEqual(decision['label_approved'], self.policy['review']['columns'][name]['label_approved'])
            self.assertEqual(decision['reviewer'], 'f20-reviewer')
            if decision['pii'] != 'none':
                self.assertIs(decision['explicit_allow'], True)
                self.assertEqual(decision['read_grant_expires_at'], self.expires)
                self.assertEqual(decision['read_grant_reviewer'], 'access-operator')
        self.assertEqual(review['access_profile']['name'], 'full')
        self.assertFalse(report['published'])
        self.assertFalse(report['runtime_verified'])
        self.assertEqual(output.stat().st_mode & 0o777, 0o700)
        for path in output.iterdir():
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)
        self.assertEqual(config['column_review_input']['sha256'],
                         hashlib.sha256((output / 'column-review.json').read_bytes()).hexdigest())

    def test_restricted_removes_pii_instead_of_reclassifying_it(self):
        output, report = self.configure('restricted')
        config, review = self.generated(output)
        self.assertEqual(set(review['columns']), {'app.items.id'})
        self.assertEqual(report['counts']['columns_visible'], 1)
        self.assertEqual(report['counts']['columns_hidden'], 4)
        self.assertEqual(report['counts']['explicit_pii_grants'], 0)
        self.assertIsNone(report['read_grants_valid_until'])
        for pii in ('personal', 'sensitive', 'unknown'):
            self.assertEqual(report['source_classification'][pii]['visible'], 0)
        effective, _, _ = control.reviewed_documents(self.facts, self.receipts, review)
        phone = next(c for c in effective['datasets'][0]['columns'] if c['name'] == 'phone')
        self.assertEqual(phone['pii'], 'personal')
        self.assertEqual(profiles.check(config, self.facts, self.receipts)['counts'], report['counts'])

    def test_switching_back_to_full_preserves_only_original_isolation(self):
        full, first = self.configure()
        restricted, _ = self.configure('restricted', full / 'config.json')
        restored, last = self.configure('full', restricted / 'config.json', 'full-again')
        self.assertEqual(first['counts'], last['counts'])
        self.assertEqual(first['hidden_columns'], last['hidden_columns'])
        self.assertEqual(self.generated(full)[1], self.generated(restored)[1])

    def test_existing_reviewed_full_needs_no_bypass_and_can_be_checked(self):
        self.policy['review']['hidden_columns'] = []
        self.policy['review']['columns']['app.items.hidden'] = deepcopy(
            self.policy['review']['columns']['app.items.bank_account'])
        pin(self.source, self.config, self.policy)
        report = profiles.check(self.config, self.facts, self.receipts)
        self.assertEqual(report['profile'], 'reviewed')
        self.assertTrue(report['coverage']['all_business_columns_visible'])
        self.assertEqual(report['counts']['columns_hidden'], 0)
        self.assertEqual(report['counts']['columns_visible'], 5)

    def test_new_known_fields_get_exact_grants_but_facts_cannot_drift(self):
        self.policy['review']['columns'].pop('app.items.phone')
        pin(self.source, self.config, self.policy)
        lib.write(self.source / 'config.json', self.config)
        output, _ = self.configure()
        self.assertIn('app.items.phone', self.generated(output)[1]['columns'])
        changed = deepcopy(self.facts)
        changed['facts_sha256'] = '0' * 64
        lib.write(self.source / 'facts.json', changed)
        with self.assertRaisesRegex(ValueError, 'facts_digest_mismatch'):
            self.configure(name='drift')

    def test_grant_operator_is_independent_of_semantic_review_and_ledger_claims(self):
        output, _ = self.configure()
        config, policy = self.generated(output)
        review = ledger_review({'review': policy})
        review['columns']['app.items.phone']['read_grant_reviewer'] = 'forged-client'
        projected, audit = grants.authorize(config, self.facts, review)
        self.assertEqual(projected['columns']['app.items.phone']['reviewer'], 'f20-reviewer')
        self.assertEqual(projected['columns']['app.items.phone']['read_grant_reviewer'], 'access-operator')
        self.assertEqual({g['authorized_by'] for g in audit['grants']}, {'access-operator'})
        doc = json.loads((output / 'column-review.json').read_text())
        doc['review']['columns']['app.items.phone']['read_grant_reviewer'] = ''
        pin(output, config, doc)
        with self.assertRaisesRegex(ValueError, 'read_grant_reviewer'):
            profiles.check(config, self.facts, self.receipts)

    def test_metadata_system_columns_and_physical_mapping_do_not_leak(self):
        tables = deepcopy(self.facts['tables'])
        tables[0]['physical'] = '"_oo"."pt_business"'
        for name, source in [('_oo_offset', '_oo_offset'), ('ctid', 'ctid'), ('alias', '_oo_receipt')]:
            tables[0]['columns'].append({'name': name, 'source_name': source, 'data_type': 'bigint',
                                         'profile': {'rows': 3, 'distinct': 3}})
        tables[0]['columns'].append({'name': 'amount', 'data_type': 'numeric',
                                     'profile': {'rows': 3, 'distinct': 3}})
        for schema in ('_oo', 'pg_catalog', 'information_schema', 'app__stage'):
            tables.append({'dataset': schema + '.metadata', 'source_id': 'f20',
                           'source_generation': 1, 'target_generation': 1, 'primary_key': ['id'],
                           'columns': [{'name': 'id', 'data_type': 'bigint',
                                        'profile': {'rows': 3, 'distinct': 3}}]})
        self.facts = lib.build_facts(observed_fixture(tables), lambda *args: {})
        draft = {'datasets': [{'dataset': t['dataset'], 'columns': [{'name': c['name']} for c in t['columns']]}
                              for t in self.facts['tables']], 'metrics': [], 'glossary': [], 'examples': []}
        self.receipts = [{'facts_sha256': self.facts['facts_sha256'],
                          'draft_contract': control.library_assets.DRAFT_CONTRACT, 'draft': draft}]
        self.policy['facts_sha256'] = self.facts['facts_sha256']
        pin(self.source, self.config, self.policy)
        for name, data in [('config', self.config), ('facts', self.facts),
                           ('drafts', {'receipts': self.receipts, 'failures': []})]:
            lib.write(self.source / (name + '.json'), data)
        output, report = self.configure()
        config, review = self.generated(output)
        self.assertEqual(report['counts']['system_columns'], 7)
        self.assertEqual(report['counts']['columns_visible'], 5)
        self.assertEqual(review['columns']['app.items.amount']['semantic_type'], 'quantity')
        self.assertNotIn('app.items.alias', review['columns'])
        self.assertEqual(report['counts']['tables_visible'], 1)
        policy = json.loads((output / 'column-review.json').read_text())
        policy['review']['access_profile']['protected_columns'].remove('_oo.metadata.id')
        pin(output, config, policy)
        with self.assertRaisesRegex(ValueError, 'access_profile_system_isolation'):
            profiles.check(config, self.facts, self.receipts)

    def test_compile_reports_exact_visibility_and_keeps_runtime_boundary(self):
        self.config['release_versions'] = dict(ontology='test', catalog='test', policy='test', knowledge='test')
        lib.write(self.source / 'config.json', self.config)
        output, report = self.configure()
        work, request = self.compile(output)
        compiled_report = json.loads((work / 'access-profile-report.json').read_text())
        self.assertEqual(compiled_report, {k: v for k, v in report.items() if k != 'input_files_sha256'})
        visibility = json.loads((work / 'visibility.json').read_text())
        self.assertEqual(compiled_report['visibility_sha256'], lib.digest(visibility))
        self.assertEqual(visibility['datasets'][0]['row_security'], 'none')
        before = {p: p.read_bytes() for p in work.rglob('*') if p.is_file()}
        with patch('library_read_grants.time.time', return_value=self.expires):
            with self.assertRaisesRegex(ValueError, 'read_grant_expired'):
                library_build.stage(request)
        self.assertEqual(before, {p: p.read_bytes() for p in work.rglob('*') if p.is_file()})

    def test_ledger_cannot_choose_full_in_request_or_omit_profile_members(self):
        output, _ = self.configure('restricted')
        work, request = self.compile(output, http=True, access_profile='full')
        report = json.loads((work / 'access-profile-report.json').read_text())
        self.assertEqual(report['profile'], 'restricted')
        self.assertEqual(report['counts']['columns_visible'], 1)
        config, review = self.generated(output)
        review['review_origin'] = 'review_ledger'
        review['access_profile']['name'] = 'full'
        with self.assertRaisesRegex(ValueError, 'access_profile_operator_input_required'):
            grants.authorize(config, self.facts, review)
        full, _ = self.configure()
        config, review = self.generated(full)
        review['review_origin'] = 'review_ledger'
        review['columns'].pop('app.items.phone')
        with self.assertRaisesRegex(ValueError, 'access_profile_coverage'):
            grants.authorize(config, self.facts, review)

    def test_withheld_semantic_reviews_are_not_hidden_read_grants(self):
        output, _ = self.configure('restricted')
        config, review = self.generated(output)
        withheld = review['access_profile']['withheld_reviews']
        self.assertIn('app.items.phone', withheld)
        self.assertNotIn('explicit_allow', withheld['app.items.phone'])
        review['review_origin'] = 'review_ledger'
        review['columns']['app.items.phone'] = dict(withheld['app.items.phone'], explicit_allow=True)
        with self.assertRaisesRegex(ValueError, 'read_grant_hidden'):
            grants.authorize(config, self.facts, review)

    def test_customer_profile_without_operator_pin_is_not_authority(self):
        review = ledger_review(self.policy)
        review['columns'] = {'app.items.id': review['columns']['app.items.id']}
        review['access_profile'] = {'name': 'full', 'protected_columns': []}
        with self.assertRaisesRegex(ValueError, 'access_profile_operator_input_required'):
            grants.authorize({'project': 'f20'}, self.facts, review)
        with self.assertRaisesRegex(ValueError, 'access_profile_operator_input_required'):
            grants.authorize(self.config, self.facts, review)

    def test_profile_scope_pin_expiry_and_classification_negatives(self):
        output, _ = self.configure()
        config, review = self.generated(output)
        for change in ('digest', 'facts', 'project', 'role', 'principal', 'column', 'hidden',
                       'expiry', 'unknown_downgrade', 'allow'):
            with self.subTest(change=change):
                cfg, facts, req = deepcopy(config), deepcopy(self.facts), deepcopy(review)
                req['review_origin'] = 'review_ledger'
                policy = json.loads((output / 'column-review.json').read_text())
                if change == 'digest':
                    cfg['column_review_input']['sha256'] = '0' * 64
                elif change == 'facts':
                    facts['facts_sha256'] = '0' * 64
                elif change == 'project':
                    cfg['project'] = 'another-tenant'
                elif change in ('role', 'principal'):
                    req[change] = 'another'
                else:
                    decision = policy['review']['columns']['app.items.note']
                    if change == 'column':
                        policy['review']['columns'].pop('app.items.id')
                    elif change == 'hidden':
                        policy['review']['hidden_columns'].append('app.items.phone')
                    elif change == 'expiry':
                        decision['read_grant_expires_at'] = 0
                    elif change == 'unknown_downgrade':
                        decision.update(pii='none', explicit_allow=False)
                        req['columns']['app.items.note']['pii'] = 'none'
                    elif change == 'allow':
                        decision['explicit_allow'] = 'true'
                    pin(output, cfg, policy)
                with self.assertRaises(ValueError):
                    authorized, _ = grants.authorize(cfg, facts, req)
                    control.reviewed_documents(facts, self.receipts, authorized)

    def test_future_fields_do_not_inherit_old_full_grants(self):
        output, _ = self.configure()
        config, review = self.generated(output)
        review['review_origin'] = 'review_ledger'
        review['columns']['app.items.new_secret'] = deepcopy(review['columns']['app.items.phone'])
        with self.assertRaisesRegex(ValueError, 'read_grant_scope'):
            grants.authorize(config, self.facts, review)

    def test_refuse_bad_operator_inputs_and_do_not_overwrite_evidence(self):
        for params in ({'reviewer': ''}, {'expires_at': 0}, {'expires_at': True}):
            with self.subTest(params=params), self.assertRaises(ValueError):
                self.configure(**params)
        self.assertFalse((self.root / 'full').exists())
        output, _ = self.configure()
        before = {p.name: p.read_bytes() for p in output.iterdir()}
        with self.assertRaises(FileExistsError):
            self.configure()
        self.assertEqual(before, {p.name: p.read_bytes() for p in output.iterdir()})

    def test_output_under_git_or_symlinked_git_parent_is_rejected(self):
        repo = self.root / 'repo'
        repo.mkdir()
        (repo / '.git').write_text('synthetic-worktree-marker')
        alias = self.root / 'alias'
        alias.symlink_to(repo, target_is_directory=True)
        for output in (repo / 'result', alias / 'result'):
            with self.assertRaisesRegex(ValueError, 'access_profile_output_must_be_outside_git'):
                self.configure(name=str(output))
            self.assertFalse(output.exists())

    def test_pin_symlink_and_failed_drafts_are_refused(self):
        alias = self.source / 'alias-review.json'
        alias.symlink_to(self.config['column_review_input']['path'])
        self.config['column_review_input']['path'] = str(alias)
        lib.write(self.source / 'config.json', self.config)
        with self.assertRaisesRegex(ValueError, 'column_review_file'):
            self.configure()
        drafts = {'receipts': self.receipts, 'failures': [{'reason': 'historical_failure'}]}
        lib.write(self.source / 'drafts.json', drafts)
        with self.assertRaisesRegex(ValueError, 'access_profile_drafts_failed'):
            self.configure()

    def test_missing_draft_members_and_stale_classification_cannot_claim_full(self):
        draft = deepcopy(self.receipts)
        draft[0]['draft']['datasets'][0]['columns'].pop()
        lib.write(self.source / 'drafts.json', {'receipts': draft, 'failures': []})
        with self.assertRaisesRegex(ValueError, 'semantic_draft_coverage'):
            self.configure()
        facts = deepcopy(self.facts)
        phone = next(c for c in facts['tables'][0]['columns'] if c['name'] == 'phone')
        phone['classification_candidate']['pii'] = 'none'
        facts['facts_sha256'] = lib.digest({k: v for k, v in facts.items() if k != 'facts_sha256'})
        lib.write(self.source / 'facts.json', facts)
        with self.assertRaisesRegex(ValueError, 'retained_classification_rule_drift'):
            self.configure()

    def test_expired_grants_can_only_be_renewed_by_generating_new_operator_input(self):
        output, _ = self.configure()
        config, _ = self.generated(output)
        with patch('library_read_grants.time.time', return_value=self.expires):
            with self.assertRaisesRegex(ValueError, 'read_grant_expired'):
                profiles.check(config, self.facts, self.receipts)
            renewed, _ = self.configure(config=output / 'config.json', name='renewed',
                                        expires_at=self.expires + 3600)
            self.assertNotEqual(self.generated(renewed)[0]['column_review_input']['sha256'],
                                config['column_review_input']['sha256'])

    def test_cli_help_configure_check_and_sanitized_failures(self):
        entry = Path(control.__file__).resolve()
        def cli(*args):
            return subprocess.run([sys.executable, str(entry), *map(str, args)],
                                  capture_output=True, text=True, timeout=30)
        self.assertIn('access-profile', cli('--help').stdout)
        help_text = cli('access-profile', 'configure', '--help').stdout
        self.assertIn('{full,restricted}', help_text)
        self.assertIn('JWT', help_text)
        output = self.root / 'cli'
        args = ('--config', self.source / 'config.json', '--facts', self.source / 'facts.json',
                '--drafts', self.source / 'drafts.json')
        result = cli('access-profile', 'configure', *args, '--profile', 'full',
                     '--reviewer', 'test-operator', '--expires-at', self.expires, '--output', output)
        self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
        report = json.loads(result.stdout)
        checked = cli('access-profile', 'check', '--config', output / 'config.json', *args[2:])
        self.assertEqual(checked.returncode, 0, checked.stderr + checked.stdout)
        self.assertEqual(json.loads(checked.stdout)['counts'], report['counts'])
        result = cli('access-profile', 'check', '--config', self.root / 'secret-not-in-error', *args[2:])
        self.assertEqual(result.returncode, 2)
        self.assertNotIn('secret-not-in-error', result.stdout + result.stderr)

    def test_normal_oos_tree_compiler_seals_only_profile_visible_fields(self):
        binary = Path(__file__).resolve().parents[3] / 'target/debug/oos-tree'
        if not binary.is_file():
            self.skipTest('local oos-tree binary unavailable; build oo-builder --bin oos-tree first')
        for profile in profiles.PROFILES:
            with self.subTest(profile=profile):
                output, _ = self.configure(profile)
                work, _ = self.compile(output, name='compile-' + profile)
                visibility = json.loads((work / 'visibility.json').read_text())
                lib.write(work / 'ceilings.json', visibility['limits'])
                result = subprocess.run(
                    [str(binary), str(work / 'ontology'), str(work / 'oos-tree-binding.json'),
                     str(work / 'visibility.json'), str(work / 'ceilings.json'), str(work / 'pack.json')],
                    capture_output=True, text=True, timeout=30)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                pack = json.loads((work / 'pack.json').read_text())
                lib.verify_pack_artifact(pack['artifact'])
                self.assertEqual(pack['effective_oos'], json.loads((work / 'effective-oos.json').read_text()))
                table = next(f['text'] for f in pack['artifact']['files'] if f['path'] == 'tables/app.items.md')
                self.assertIn('`id`', table)
                self.assertNotIn('`hidden`', table)
                for name in ('phone', 'bank_account', 'note'):
                    self.assertEqual('`' + name + '`' in table, profile == 'full')
                self.assertFalse(pack['published'])


if __name__ == '__main__':
    unittest.main()
