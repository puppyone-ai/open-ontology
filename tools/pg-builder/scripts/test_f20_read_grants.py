"""F20 synthetic evidence; no source, model, publication or customer access."""
from copy import deepcopy
import hashlib
import json
from pathlib import Path
import tempfile
import time
import unittest

import library_build
import library_control as control
import library_read_grants as grants
import library_selfbuild as lib
from observation_test_support import observed_fixture


def fixture(root, project='f20'):
    root = Path(root)
    root.mkdir(parents=True, exist_ok=True)
    columns = [
        ('id', 'bigint', 'identifier', 'none'),
        ('phone', 'text', 'phone', 'personal'),
        ('bank_account', 'text', 'identifier', 'sensitive'),
        ('note', 'text', 'free_text', 'unknown'),
        ('hidden', 'text', 'free_text', 'sensitive'),
    ]
    tables = [{'dataset': 'app.items', 'source_id': 'f20', 'source_generation': 1,
               'target_generation': 1, 'primary_key': ['id'], 'columns': [
                   {'name': name, 'data_type': dtype, 'comment': 'bank account' if name == 'hidden' else name,
                    'profile': {'rows': 3, 'distinct': 3}}
                   for name, dtype, _, _ in columns]}]
    facts = lib.build_facts(observed_fixture(tables), lambda *args: {})
    lib.require_complete_facts(facts)
    draft = {'datasets': [{'dataset': 'app.items', 'columns': [
        {'name': c['name']} for c in facts['tables'][0]['columns']]}],
        'metrics': [], 'glossary': [], 'examples': []}
    receipts = [{'facts_sha256': facts['facts_sha256'],
                 'draft_contract': control.library_assets.DRAFT_CONTRACT, 'draft': draft}]
    review = {'project': project, 'role': 'oo_r_f20', 'principal': 'f20_reader',
              'hidden_columns': ['app.items.hidden'], 'columns': {}, 'labels': {},
              'datasets': {'app.items': {'label': False, 'description': False,
                                        'grain': False, 'reviewer': 'f20-reviewer'}},
              'relationships': [], 'reviewer': 'rule_high_confidence', 'business_owner_signed': False}
    for name, _, semantic, pii in columns[:-1]:
        decision = {'pii': 'sensitive' if pii == 'unknown' else pii,
                    'semantic_type': semantic, 'reviewer': 'f20-reviewer',
                    'label_approved': True, 'approved_fields': {'label': False, 'description': False}}
        if pii != 'none':
            decision.update(explicit_allow=True, read_grant_expires_at=int(time.time()) + 3600)
        if pii == 'unknown':
            decision['reason'] = 'Explicit scoped read; conservatively retain unknown as sensitive.'
        review['columns']['app.items.' + name] = decision
    policy = {'protocol': 'oo.library.column-review/v1', 'project': project,
              'facts_sha256': facts['facts_sha256'], 'review': review}
    config = {'project': project, 'psql_command': ['false'], 'required_assets': {'datasets': ['app.items']}}
    pin(root, config, policy)
    lib.write(root / 'facts.json', facts)
    lib.write(root / 'drafts.json', {'receipts': receipts, 'failures': []})
    lib.write(root / 'merged.json', lib.merge_drafts(facts, [draft]))
    lib.write(root / 'manifest.json', {})
    lib.write(root / 'config.json', config)
    return facts, receipts, policy, config


def pin(root, config, policy):
    path = Path(root) / 'operator-columns.json'
    lib.write(path, policy)
    config['column_review_input'] = {'path': str(path),
                                    'sha256': hashlib.sha256(path.read_bytes()).hexdigest()}


def ledger_review(policy):
    review = deepcopy(policy['review'])
    review['review_origin'] = 'review_ledger'
    for column in review['columns'].values():
        for key in ('explicit_allow', 'read_grant_expires_at', 'reason'):
            column.pop(key, None)
    return review


class F20ReadGrants(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='oo-f20-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.facts, self.receipts, self.policy, self.config = fixture(self.root)
        self.http = ledger_review(self.policy)

    def projected(self, review=None):
        value, audit = grants.authorize(self.config, self.facts, review or self.http)
        return (*control.reviewed_documents(self.facts, self.receipts, value)[:2], audit)

    def test_same_pinned_grant_local_and_ledger_preserve_semantics_and_visibility(self):
        local = library_build.pinned_review(self.config, self.facts)
        self.assertEqual(self.projected(local)[:2], self.projected()[:2])
        effective, visibility, auth = self.projected()
        self.assertEqual(visibility['datasets'][0]['columns'], ['bank_account', 'id', 'note', 'phone'])
        self.assertEqual({c['name']: c['pii'] for c in effective['datasets'][0]['columns']},
                         {'id': 'none', 'phone': 'personal', 'bank_account': 'sensitive',
                          'note': 'sensitive', 'hidden': 'sensitive'})
        self.assertEqual(len(auth['grants']), 3)
        self.assertEqual(auth['input_sha256'], self.config['column_review_input']['sha256'])

    def test_management_classification_or_forged_explicit_allow_is_not_read_authority(self):
        self.config.pop('column_review_input')
        self.http['columns']['app.items.phone']['explicit_allow'] = True
        with self.assertRaisesRegex(ValueError, 'pii_read_grant_required'):
            self.projected()

    def test_grant_pin_facts_and_project_cannot_drift(self):
        for change in ('bytes', 'facts', 'project'):
            with self.subTest(change=change):
                config, facts = deepcopy(self.config), deepcopy(self.facts)
                if change == 'bytes':
                    config['column_review_input']['sha256'] = '0' * 64
                elif change == 'facts':
                    facts['facts_sha256'] = '0' * 64
                else:
                    config['project'] = 'another'
                with self.assertRaises(ValueError):
                    grants.authorize(config, facts, self.http)

    def test_exact_column_scope_hidden_role_and_principal(self):
        for change in ('column', 'hidden', 'role', 'principal'):
            with self.subTest(change=change):
                policy = deepcopy(self.policy)
                if change == 'column':
                    policy['review']['columns'].pop('app.items.phone')
                elif change == 'hidden':
                    policy['review']['hidden_columns'].append('app.items.phone')
                else:
                    policy['review'][change] = 'other'
                pin(self.root, self.config, policy)
                with self.assertRaisesRegex(ValueError, 'read_grant_(scope|hidden)'):
                    self.projected()

    def test_expiry_is_required_and_fail_closed_at_boundary(self):
        for expiry in (None, False, '9999999999', 0, int(time.time()) - 1):
            with self.subTest(expiry=expiry):
                self.policy['review']['columns']['app.items.phone']['read_grant_expires_at'] = expiry
                pin(self.root, self.config, self.policy)
                with self.assertRaisesRegex(ValueError, 'read_grant_expired'):
                    self.projected()

    def test_explicit_allow_is_boolean_and_never_inferred(self):
        for allow in (None, False, 1, 'true'):
            with self.subTest(allow=allow):
                self.policy['review']['columns']['app.items.phone']['explicit_allow'] = allow
                pin(self.root, self.config, self.policy)
                with self.assertRaisesRegex(ValueError, 'pii_read_grant_required'):
                    self.projected()

    def test_unknown_needs_operator_reason_and_known_classes_cannot_be_downgraded(self):
        self.policy['review']['columns']['app.items.note'].pop('reason')
        pin(self.root, self.config, self.policy)
        self.http['columns']['app.items.note']['reason'] = 'untrusted classification body'
        with self.assertRaisesRegex(ValueError, 'pii_conservative_review_required'):
            self.projected()
        self.policy['review']['columns']['app.items.note']['reason'] = 'operator decision'
        self.policy['review']['columns']['app.items.phone'].update(pii='none', explicit_allow=False)
        self.http['columns']['app.items.phone']['pii'] = 'none'
        pin(self.root, self.config, self.policy)
        with self.assertRaisesRegex(ValueError, 'classification_conflict'):
            self.projected()

    def test_grant_reviewer_does_not_replace_ledger_classifier(self):
        self.policy['review']['columns']['app.items.phone']['reviewer'] = 'grant-operator'
        pin(self.root, self.config, self.policy)
        effective, _, auth = self.projected()
        phone = next(c for c in effective['datasets'][0]['columns'] if c['name'] == 'phone')
        self.assertEqual(phone['classification']['by'], 'f20-reviewer')
        self.assertEqual(auth['grants'][0]['authorized_by'], 'grant-operator')

    def test_symlink_grant_and_unpinned_local_review_are_refused(self):
        path = self.root / 'alias.json'
        path.symlink_to(self.config['column_review_input']['path'])
        self.config['column_review_input']['path'] = str(path)
        with self.assertRaisesRegex(ValueError, 'column_review_file'):
            self.projected()

    def test_compile_stage_rechecks_policy_and_merged_input_even_after_success(self):
        lib.write(self.root / 'ledger-review.json', self.http)
        request = {'stage': 'compile', 'work_dir': str(self.root),
                   'config': str(self.root / 'config.json'), 'manifest': str(self.root / 'manifest.json')}
        control.run(request)
        self.assertTrue((self.root / 'compiler-request.json').is_file())
        (self.root / 'operator-columns.json').write_text('{}')
        with self.assertRaisesRegex(ValueError, 'column_review_digest'):
            control.run(request)
        pin(self.root, self.config, self.policy)
        lib.write(self.root / 'merged.json', {})
        with self.assertRaisesRegex(ValueError, 'review_generation_drift'):
            control.run(request)

    def test_http_review_cannot_omit_s6_membership_document(self):
        lib.write(self.root / 'ledger-review.json', self.http)
        (self.root / 'merged.json').unlink()
        with self.assertRaisesRegex(ValueError, 'review_generation_drift'):
            control.run({'stage': 'compile', 'work_dir': str(self.root),
                         'config': str(self.root / 'config.json'),
                         'manifest': str(self.root / 'manifest.json')})


if __name__ == '__main__':
    unittest.main()
