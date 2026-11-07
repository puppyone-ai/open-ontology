"""Unit tests for provision.py (stdlib unittest). No Docker, no network, no customer data."""
import argparse
import importlib.util
import json
import os
import re
import socket
import stat
import subprocess
import sys
import tempfile
import threading
import unittest
from pathlib import Path
from unittest import mock

spec = importlib.util.spec_from_file_location('provision', Path(__file__).with_name('provision.py'))
P = importlib.util.module_from_spec(spec)
spec.loader.exec_module(P)

PASSWORD = b'S3cr3t-Value-XYZ-42'
GOOD_SIX = {'type': 'mysql', 'host': 'fixture.internal', 'port': 3306, 'database': 'ob_fixture',
            'user': 'oo_cdc', 'secret_ref': 'secret://owned-test-ref'}
HEX = 'ab' * 32


def private_tmp():
    d = Path(tempfile.mkdtemp(prefix='oo-prov-test-'))
    os.chmod(d, 0o700)
    return d


def site_for(secret_root):
    return {'protocol': 'oo.source-onboarding.site/v1', 'secret_root': str(secret_root),
            'pg_identity_container': 'no-such-container-test', 'egress': {
                'ssh_host': '192.0.2.20', 'ssh_user': 'oo', 'ssh_identity': '/nonexistent/key',
                'producer_tls_server_name': False}, 'binaries': {}}


def write_file(path, data, mode=0o600):
    Path(path).write_bytes(data.encode('utf-8') if isinstance(data, str) else data)
    os.chmod(path, mode)
    return Path(path)


def make_run(tmp, password=PASSWORD, label='fixture-test'):
    secret_root = tmp / 'secret-root'
    secret_root.mkdir(mode=0o700)
    site = site_for(secret_root)
    run, out = P.create_run(site, dict(GOOD_SIX), password, 'v1', 'direct', None, label, tmp / 'RUN')
    return site, run, out


def phase_ok(out, name, data=None):
    P.journal_append(out, name, 'started')
    P.record_phase(out, name, P.now(), data or {})
    P.journal_append(out, name, 'succeeded')


def overlay(columns, build='b_test'):
    return {'build_id': build, 'columns': columns}


def col(dataset, name, fact, state='pending'):
    return {'dataset': dataset, 'name': name, 'data_type': 'bigint', 'sealed_data_type': 'bigint',
            'fact_sha256': fact, 'classification_state': state, 'label': '', 'pii': 'none'}


class SixFieldTests(unittest.TestCase):
    def test_valid_six_fields_are_accepted_and_normalised(self):
        self.assertEqual(P.validate_six(dict(GOOD_SIX)), GOOD_SIX)

    def test_extra_key_is_refused(self):
        doc = dict(GOOD_SIX, recipe='x')
        with self.assertRaises(P.Refused) as cm:
            P.validate_six(doc)
        self.assertEqual(cm.exception.code, 'six_fields_invalid')

    def test_missing_key_is_refused(self):
        doc = dict(GOOD_SIX)
        del doc['user']
        with self.assertRaises(P.Refused) as cm:
            P.validate_six(doc)
        self.assertEqual(cm.exception.code, 'six_fields_invalid')

    def test_inline_password_key_is_refused_with_specific_code(self):
        for key in ('password', 'Password', 'pwd', 'token'):
            with self.subTest(key=key):
                with self.assertRaises(P.Refused) as cm:
                    P.validate_six(dict(GOOD_SIX, **{key: 'x'}))
                self.assertEqual(cm.exception.code, 'inline_password_refused')

    def test_wrong_types_and_ranges(self):
        bad = [dict(GOOD_SIX, type='postgresql'), dict(GOOD_SIX, port='3306'), dict(GOOD_SIX, port=True),
               dict(GOOD_SIX, port=0), dict(GOOD_SIX, port=70000), dict(GOOD_SIX, host='bad host'),
               dict(GOOD_SIX, database="x';DROP"), dict(GOOD_SIX, user='a b'),
               dict(GOOD_SIX, secret_ref='plain-text-password'), dict(GOOD_SIX, secret_ref='secret://../x')]
        for doc in bad:
            with self.subTest(doc=doc):
                with self.assertRaises(P.Refused) as cm:
                    P.validate_six(doc)
                self.assertEqual(cm.exception.code, 'six_fields_invalid')

    def test_replica_shape(self):
        self.assertEqual(P.validate_replica({'host': 'replica.internal', 'port': 5432, 'database': 'ob_x'})['port'], 5432)
        with self.assertRaises(P.Refused):
            P.validate_replica({'host': 'r', 'port': 5432})
        with self.assertRaises(P.Refused) as cm:
            P.validate_replica({'host': 'r', 'port': 5432, 'database': 'd', 'password': 'x'})
        self.assertEqual(cm.exception.code, 'inline_password_refused')

    def test_format_switch_accepts_v1_and_v2_and_refuses_unknown(self):
        self.assertEqual(P.choose_format(None), 'v1')
        self.assertEqual(P.choose_format('v1'), 'v1')
        self.assertEqual(P.choose_format('chunked-typed/v2'), 'chunked-typed/v2')
        with self.assertRaises(P.Refused) as cm:
            P.choose_format('v3')
        self.assertEqual(cm.exception.code, 'format_invalid')

    def test_label_validation(self):
        self.assertEqual(P.validate_label(None, GOOD_SIX), 'fixture.internal/ob_fixture')
        with self.assertRaises(P.Refused):
            P.validate_label('line\nbreak', GOOD_SIX)
        with self.assertRaises(P.Refused):
            P.validate_label('x' * 121, GOOD_SIX)


class PrivacyTests(unittest.TestCase):
    def setUp(self):
        self.tmp = private_tmp()

    def test_password_file_0600_is_read_and_trailing_newline_stripped(self):
        pw = write_file(self.tmp / 'pw', PASSWORD + b'\n', 0o600)
        self.assertEqual(P.read_password(pw), PASSWORD)

    def test_password_file_with_group_or_other_permissions_is_refused(self):
        for mode in (0o640, 0o604, 0o644):
            with self.subTest(mode=oct(mode)):
                pw = write_file(self.tmp / ('pw%o' % mode), PASSWORD, mode)
                with self.assertRaises(P.Refused) as cm:
                    P.read_password(pw)
                self.assertEqual(cm.exception.code, 'private_file')

    def test_symlinked_password_file_is_refused(self):
        real = write_file(self.tmp / 'real', PASSWORD, 0o600)
        link = self.tmp / 'link'
        os.symlink(real, link)
        with self.assertRaises(P.Refused) as cm:
            P.read_password(link)
        self.assertEqual(cm.exception.code, 'private_file')

    def test_empty_multiline_or_oversized_password_is_refused(self):
        for data in (b'', b'a\nb', b'a\x00b', b'x' * (P.MAX_PASSWORD_BYTES + 1)):
            with self.subTest(len=len(data)):
                pw = write_file(self.tmp / ('p%d' % len(data)), data, 0o600)
                with self.assertRaises(P.Refused) as cm:
                    P.read_password(pw)
                self.assertEqual(cm.exception.code, 'password_file_invalid')

    def test_input_file_must_be_private(self):
        inp = write_file(self.tmp / 'six.json', json.dumps(GOOD_SIX), 0o644)
        with self.assertRaises(P.Refused) as cm:
            P.load_private_json(inp)
        self.assertEqual(cm.exception.code, 'private_file')

    def test_duplicate_json_keys_are_refused(self):
        inp = write_file(self.tmp / 'dup.json', '{"type":"mysql","type":"mysql"}', 0o600)
        with self.assertRaises(P.Refused) as cm:
            P.load_private_json(inp)
        self.assertEqual(cm.exception.code, 'six_fields_invalid')


class CliRefusalTests(unittest.TestCase):
    """Refusals happen before any RUN directory is created."""

    def setUp(self):
        self.tmp = private_tmp()
        self.inp = write_file(self.tmp / 'six.json', json.dumps(GOOD_SIX), 0o600)
        self.pw = write_file(self.tmp / 'pw', PASSWORD + b'\n', 0o600)

    def run_cli(self, *args):
        return subprocess.run([sys.executable, str(Path(P.__file__)), *args], capture_output=True, timeout=60)

    def test_inline_password_argv_is_refused_and_not_echoed(self):
        r = self.run_cli('start', '--input', str(self.inp), '--password', PASSWORD.decode(), '--out', str(self.tmp / 'RUN'))
        self.assertEqual(r.returncode, 2)
        self.assertIn(b'inline_password_refused', r.stderr)
        self.assertNotIn(PASSWORD, r.stdout + r.stderr)
        self.assertFalse((self.tmp / 'RUN').exists())

    def test_wrong_password_permissions_refused_before_run(self):
        os.chmod(self.pw, 0o644)
        r = self.run_cli('start', '--input', str(self.inp), '--password-file', str(self.pw), '--out', str(self.tmp / 'RUN'))
        self.assertEqual(json.loads(r.stderr)['error_code'], 'private_file')
        self.assertFalse((self.tmp / 'RUN').exists())

    def test_unknown_argument_is_reported_without_echoing_its_value(self):
        r = self.run_cli('start', '--input', str(self.inp), '--password-file', str(self.pw), '--out', str(self.tmp / 'RUN'),
                         '--bogus', 'hunter2-value')
        self.assertEqual(r.returncode, 2)
        self.assertNotIn(b'hunter2-value', r.stderr)
        self.assertEqual(json.loads(r.stderr)['error_code'], 'argv_invalid')


class JournalTests(unittest.TestCase):
    def setUp(self):
        self.tmp = private_tmp()
        self.out = self.tmp / 'RUN'
        self.out.mkdir(mode=0o700)
        (self.out / 'phases').mkdir(mode=0o700)

    def test_journal_is_append_only_and_private(self):
        P.journal_append(self.out, 'validate', 'started')
        P.journal_append(self.out, 'validate', 'succeeded')
        events = P.read_journal(self.out)
        self.assertEqual([e['event'] for e in events], ['started', 'succeeded'])
        self.assertEqual(stat.S_IMODE(os.stat(self.out / 'journal.jsonl').st_mode), 0o600)

    def test_phase_completion_needs_both_journal_success_and_evidence(self):
        P.journal_append(self.out, 'validate', 'started')
        P.journal_append(self.out, 'validate', 'succeeded')
        self.assertFalse(P.phase_complete(self.out, 'validate', P.phase_summary(P.read_journal(self.out))))
        P.record_phase(self.out, 'validate', P.now(), {})
        self.assertTrue(P.phase_complete(self.out, 'validate', P.phase_summary(P.read_journal(self.out))))

    def test_failed_phase_is_not_complete_and_keeps_error_code(self):
        P.journal_append(self.out, 'egress', 'started')
        P.journal_append(self.out, 'egress', 'failed', error_code='ssh_forward_failed')
        summary = P.phase_summary(P.read_journal(self.out))
        self.assertEqual(summary['egress']['state'], 'failed')
        self.assertEqual(summary['egress']['error_code'], 'ssh_forward_failed')
        self.assertFalse(P.phase_complete(self.out, 'egress', summary))

    def test_record_phase_writes_wall_seconds(self):
        started = P.now()
        P.record_phase(self.out, 'validate', started, {'x': 1})
        doc = json.loads((self.out / 'phases' / 'validate.json').read_text())
        self.assertIsInstance(doc['wall_seconds'], float)
        self.assertEqual(doc['data'], {'x': 1})


class RunnerTests(unittest.TestCase):
    def setUp(self):
        self.tmp = private_tmp()
        self.site, self.run, self.out = make_run(self.tmp)

    def handlers(self, calls, fail=None, awaiting_ok=True):
        def make(name):
            def h(ctx):
                calls.append(name)
                if fail and fail[0] == name and not fail[1].get('done'):
                    fail[1]['done'] = True
                    raise P.Refused(fail[2])
                return {'phase': name}
            return h
        return {name: make(name) for name in P.PHASES}

    def test_runner_stops_at_awaiting_review_with_all_prior_phases_done(self):
        calls = []
        state = P.runner(self.out, self.handlers(calls))
        self.assertEqual(state, 'awaiting_review')
        self.assertEqual(calls, P.PHASES[:P.PHASES.index('awaiting_review')])
        self.assertEqual(P.run_state(P.read_journal(self.out))[0], 'awaiting_review')

    def test_failure_records_stable_code_and_resume_skips_verified_phases(self):
        calls = []
        fail = ('egress', {}, 'ssh_forward_failed')
        state = P.runner(self.out, self.handlers(calls, fail=fail))
        self.assertEqual(state, 'failed:ssh_forward_failed')
        self.assertEqual(calls, ['validate', 'egress'])
        self.assertEqual(P.run_state(P.read_journal(self.out)), ('failed', 'ssh_forward_failed'))
        calls2 = []
        state = P.runner(self.out, self.handlers(calls2, fail=fail))
        self.assertEqual(state, 'awaiting_review')
        self.assertEqual(calls2[0], 'egress')
        self.assertNotIn('validate', calls2)

    def test_identity_phase_fails_closed_when_alias_unresolved(self):
        # Unit tests must never reach Docker: alias resolution is mocked.
        handlers = dict(P.HANDLERS)
        handlers['validate'] = lambda ctx: {}
        handlers['egress'] = lambda ctx: {}
        with mock.patch.object(P, 'resolve_alias', side_effect=P.Refused('alias_unresolved:oo-dl01w1-71-pg')):
            self.assertEqual(P.runner(self.out, handlers), 'failed:alias_unresolved:oo-dl01w1-71-pg')

    def test_second_runner_refuses_while_locked(self):
        import fcntl
        lock = os.open(self.out / 'runner.lock', os.O_RDWR | os.O_CREAT, 0o600)
        fcntl.flock(lock, fcntl.LOCK_EX)
        try:
            self.assertEqual(P.runner(self.out, self.handlers([])), 'runner_active')
        finally:
            os.close(lock)

    def test_status_shape_and_phase_order(self):
        calls = []
        P.runner(self.out, self.handlers(calls))
        status = P.build_status(self.out)
        self.assertEqual(status['protocol'], 'oo.source-onboarding.provision-status/v1')
        self.assertEqual(status['state'], 'awaiting_review')
        self.assertIsNone(status['error_code'])
        self.assertEqual([p['name'] for p in status['phases']], P.PHASES)
        for phase in status['phases'][:P.PHASES.index('awaiting_review')]:
            self.assertEqual(phase['state'], 'succeeded')
            self.assertIsNotNone(phase['started_at'])
            self.assertIsNotNone(phase['finished_at'])
            self.assertIsInstance(phase['wall_seconds'], float)
        self.assertEqual(set(status['source']), {'type', 'host', 'port', 'database', 'user', 'egress'})
        self.assertEqual(set(status['capture']), {'format', 'tables', 'rows', 'bytes', 'bytes_per_second', 'reconnects', 'generation'})
        self.assertEqual(status['capture']['format'], 'v1')
        self.assertEqual(len(status['build']['stages']), 11)
        self.assertEqual(status['build']['stages'][0], {'stage': 'S0', 'status': 'succeeded'})
        self.assertEqual(status['build']['stages'][10], {'stage': 'S10', 'status': 'pending'})
        self.assertEqual(status['review'], {'pending': 0, 'submitted': 0, 'withheld': 0})
        self.assertIsNone(status['scope'])
        self.assertIn('wall_seconds_total', status['timing'])

    def test_status_reports_runner_exited_when_no_live_runner(self):
        P.journal_append(self.out, '_run', 'state', state='running')
        status = P.build_status(self.out)
        self.assertEqual((status['state'], status['error_code']), ('failed', 'runner_exited'))

    def test_cancel_marks_run_canceled(self):
        self.assertEqual(P.cancel(self.out), {'state': 'canceled'})
        self.assertEqual(P.build_status(self.out)['state'], 'canceled')


class ReviewDecideTests(unittest.TestCase):
    def setUp(self):
        self.tmp = private_tmp()
        self.site, self.run, self.out = make_run(self.tmp)
        self.product = self.out / 'product'
        self.cols = [col('ob.orders', 'id', 'a' * 64), col('ob.orders', 'total', 'b' * 64),
                     col('ob.customers', 'name', 'c' * 64)]
        write_json_file(self.product / 'prepared-overlay.json', overlay(self.cols))
        P.write_withheld(self.out)
        for name in P.PHASES[:P.PHASES.index('awaiting_review')]:
            phase_ok(self.out, name)
        P.journal_append(self.out, '_run', 'state', state='awaiting_review')

    def decisions(self, reviews=None, confirm=None, extra=None):
        reviews = reviews if reviews is not None else [
            {'name': 'ob.orders.id', 'binds_fact': 'a' * 64, 'decision': 'confirmed'},
            {'name': 'ob.orders.total', 'binds_fact': 'b' * 64, 'decision': 'confirmed'},
            {'name': 'ob.customers.name', 'binds_fact': 'c' * 64, 'decision': 'confirmed'}]
        confirm = confirm if confirm is not None else ['ob.orders.id', 'ob.orders.total', 'ob.customers.name']
        doc = {'reviews': reviews, 'confirm_columns': confirm}
        doc.update(extra or {})
        path = self.tmp / 'decisions.json'
        write_file(path, json.dumps(doc), 0o600)
        return path

    def test_review_document_shape_and_pending_items(self):
        doc = P.review_document(self.out)
        self.assertEqual(doc['protocol'], 'oo.source-onboarding.provision-review/v1')
        self.assertEqual(doc['build_id'], 'b_test')
        self.assertEqual(doc['stage_states'], {'S9': 'awaiting', 'S10': 'awaiting'})
        self.assertEqual(len(doc['items']), 3)
        item = doc['items'][0]
        for key in ('name', 'dataset', 'column', 'data_type', 'semantic_type', 'label', 'pii', 'binds_fact',
                    'classification_state', 'decision'):
            self.assertIn(key, item)
        self.assertEqual(item['name'], 'ob.orders.id')
        self.assertIsNone(item['decision'])

    def test_incomplete_decisions_are_refused(self):
        path = self.decisions(reviews=[{'name': 'ob.orders.id', 'binds_fact': 'a' * 64, 'decision': 'confirmed'}],
                              confirm=['ob.orders.id'])
        with self.assertRaises(P.Refused) as cm:
            P.decide(self.out, path)
        self.assertEqual(cm.exception.code, 'decisions_incomplete')

    def test_binds_fact_must_match_model_output_hash(self):
        path = self.decisions(reviews=[
            {'name': 'ob.orders.id', 'binds_fact': 'f' * 64, 'decision': 'confirmed'},
            {'name': 'ob.orders.total', 'binds_fact': 'b' * 64, 'decision': 'confirmed'},
            {'name': 'ob.customers.name', 'binds_fact': 'c' * 64, 'decision': 'confirmed'}])
        with self.assertRaises(P.Refused) as cm:
            P.decide(self.out, path)
        self.assertEqual(cm.exception.code, 'binds_fact_mismatch')

    def test_invalid_decision_value_unknown_and_duplicate_names(self):
        base = self.decisions()
        doc = json.loads(base.read_text())
        cases = [
            ('decision_invalid', dict(doc, reviews=[dict(doc['reviews'][0], decision='maybe')] + doc['reviews'][1:])),
            ('review_unknown_item', dict(doc, reviews=doc['reviews'] + [{'name': 'ob.x.y', 'binds_fact': HEX, 'decision': 'confirmed'}])),
            ('review_duplicate', dict(doc, reviews=doc['reviews'] + [doc['reviews'][0]])),
            ('binds_fact_invalid', dict(doc, reviews=[dict(doc['reviews'][0], binds_fact='XYZ')] + doc['reviews'][1:])),
            ('review_shape', dict(doc, reviews=[dict(doc['reviews'][0], extra=1)] + doc['reviews'][1:])),
            ('decisions_shape', dict(doc, role='oo_r_other')),
            ('decisions_shape', {'reviews': doc['reviews']}),
        ]
        for code, bad in cases:
            with self.subTest(code=code):
                path = self.tmp / ('bad-%s.json' % code)
                write_file(path, json.dumps(bad), 0o600)
                with self.assertRaises(P.Refused) as cm:
                    P.decide(self.out, path)
                self.assertEqual(cm.exception.code, code)

    def test_confirm_columns_must_equal_confirmed_set(self):
        path = self.decisions(confirm=['ob.orders.id'])
        with self.assertRaises(P.Refused) as cm:
            P.decide(self.out, path)
        self.assertEqual(cm.exception.code, 'confirm_columns_mismatch')

    def test_all_rejected_is_refused_as_requiring_a_rebuild(self):
        reviews = [{'name': n, 'binds_fact': f, 'decision': 'rejected'} for n, f in
                   (('ob.orders.id', 'a' * 64), ('ob.orders.total', 'b' * 64), ('ob.customers.name', 'c' * 64))]
        path = self.decisions(reviews=reviews, confirm=[])
        with self.assertRaises(P.Refused) as cm:
            P.decide(self.out, path)
        self.assertEqual(cm.exception.code, 'rejection_requires_rebuild')

    def test_any_rejected_decision_is_refused_before_anything_is_recorded(self):
        reviews = [{'name': 'ob.orders.id', 'binds_fact': 'a' * 64, 'decision': 'confirmed'},
                   {'name': 'ob.orders.total', 'binds_fact': 'b' * 64, 'decision': 'confirmed'},
                   {'name': 'ob.customers.name', 'binds_fact': 'c' * 64, 'decision': 'rejected'}]
        path = self.decisions(reviews=reviews, confirm=['ob.orders.id', 'ob.orders.total'])
        with self.assertRaises(P.Refused) as cm:
            P.decide(self.out, path)
        self.assertEqual(cm.exception.code, 'rejection_requires_rebuild')
        self.assertFalse((self.out / 'review' / 'decisions.json').exists())
        self.assertFalse((self.out / 'private' / 'product-decisions.json').exists())

    def write_overlay(self, columns):
        write_json_file(self.product / 'prepared-overlay.json', overlay(columns))
        P.write_withheld(self.out)

    def test_withheld_pii_is_reported_but_never_required_or_sent(self):
        self.write_overlay([col('ob.orders', 'id', 'a' * 64), col('ob.orders', 'total', 'b' * 64),
                            dict(col('ob.customers', 'name', 'c' * 64), pii='personal')])
        doc = P.review_document(self.out)
        name = [i for i in doc['items'] if i['name'] == 'ob.customers.name'][0]
        self.assertFalse(name['reviewable'])
        self.assertEqual(name['withheld_reason'], 'pii_personal')
        reviews = [{'name': 'ob.orders.id', 'binds_fact': 'a' * 64, 'decision': 'confirmed'},
                   {'name': 'ob.orders.total', 'binds_fact': 'b' * 64, 'decision': 'confirmed'}]
        path = self.tmp / 'pii-ok.json'
        write_file(path, json.dumps({'reviews': reviews, 'confirm_columns': ['ob.orders.id', 'ob.orders.total']}), 0o600)
        result = P.decide(self.out, path)
        self.assertEqual((result['reviews'], result['withheld']), (2, 1))
        sent = json.loads((self.out / 'private' / 'product-decisions.json').read_text())
        self.assertEqual(sorted(r['name'] for r in sent['reviews']), ['ob.orders.id', 'ob.orders.total'])
        self.assertNotIn('ob.customers.name', sent['confirm_columns'])

    def test_submitting_a_withheld_item_is_refused(self):
        self.write_overlay([col('ob.orders', 'id', 'a' * 64),
                            dict(col('ob.customers', 'name', 'c' * 64), pii='personal')])
        path = self.tmp / 'pii-bad.json'
        write_file(path, json.dumps({'reviews': [
            {'name': 'ob.orders.id', 'binds_fact': 'a' * 64, 'decision': 'confirmed'},
            {'name': 'ob.customers.name', 'binds_fact': 'c' * 64, 'decision': 'confirmed'}],
            'confirm_columns': ['ob.orders.id', 'ob.customers.name']}), 0o600)
        with self.assertRaises(P.Refused) as cm:
            P.decide(self.out, path)
        self.assertEqual(cm.exception.code, 'withheld_item_submitted')

    def test_unknown_pii_blocks_the_whole_decision(self):
        self.write_overlay([col('ob.orders', 'id', 'a' * 64), dict(col('ob.customers', 'name', 'c' * 64), pii='unknown')])
        path = self.tmp / 'pii-unknown.json'
        write_file(path, json.dumps({'reviews': [{'name': 'ob.orders.id', 'binds_fact': 'a' * 64, 'decision': 'confirmed'}],
                                     'confirm_columns': ['ob.orders.id']}), 0o600)
        with self.assertRaises(P.Refused) as cm:
            P.decide(self.out, path)
        self.assertEqual(cm.exception.code, 'pii_unknown')

    def test_already_accepted_reviews_are_not_resubmitted(self):
        write_json_file(self.product / '0001-review.response.json', {'status': 200, 'request': {'name': 'ob.orders.id'}})
        result = P.decide(self.out, self.decisions())
        self.assertEqual(result['to_submit'], 2)
        sent = json.loads((self.out / 'private' / 'product-decisions.json').read_text())
        self.assertEqual(sorted(r['name'] for r in sent['reviews']), ['ob.customers.name', 'ob.orders.total'])
        self.assertEqual(sorted(sent['confirm_columns']), ['ob.customers.name', 'ob.orders.id', 'ob.orders.total'])

    def test_status_counts_pending_and_submitted_reviews(self):
        self.assertEqual(P.build_status(self.out)['review'], {'pending': 3, 'submitted': 0, 'withheld': 0})
        P.decide(self.out, self.decisions())
        # Submitted but not yet accepted by the API: still pending.
        self.assertEqual(P.build_status(self.out)['review'], {'pending': 3, 'submitted': 0, 'withheld': 0})
        for index, name in enumerate(['ob.orders.id', 'ob.orders.total', 'ob.customers.name'], start=1):
            write_json_file(self.product / ('%04d-review.response.json' % index),
                            {'status': 200, 'request': {'name': name}})
        self.assertEqual(P.build_status(self.out)['review'], {'pending': 0, 'submitted': 3, 'withheld': 0})

    def test_valid_decisions_are_recorded_with_tool_filled_role_and_fixture_flag(self):
        result = P.decide(self.out, self.decisions())
        self.assertEqual((result['state'], result['reviews'], result['confirmed'], result['to_submit']), ('decisions_recorded', 3, 3, 3))
        product_doc = json.loads((self.out / 'private' / 'product-decisions.json').read_text())
        self.assertEqual(set(product_doc), {'reviews', 'confirm_columns', 'role', 'fixture_authorized'})
        self.assertEqual(product_doc['role'], self.run['names']['role'])
        self.assertIs(product_doc['fixture_authorized'], True)
        self.assertEqual(product_doc['confirm_columns'], ['ob.customers.name', 'ob.orders.id', 'ob.orders.total'])
        self.assertEqual(stat.S_IMODE(os.stat(self.out / 'private' / 'product-decisions.json').st_mode), 0o600)
        self.assertEqual(P.phase_summary(P.read_journal(self.out))['awaiting_review']['state'], 'succeeded')
        review = P.review_document(self.out)
        self.assertEqual({i['decision'] for i in review['items']}, {'confirmed'})
        self.assertEqual(review['stage_states'], {'S9': 'submitted', 'S10': 'submitted'})

    def test_resubmitting_same_decisions_is_idempotent_and_different_ones_drift(self):
        path = self.decisions()
        P.decide(self.out, path)
        self.assertEqual(P.decide(self.out, path)['decisions'], 'already_recorded')
        # Every valid submission is the same canonical set, so a different stored record is the drift to refuse.
        write_json_file(self.out / 'review' / 'decisions.json', {'reviews': [], 'confirm_columns': []})
        with self.assertRaises(P.Refused) as cm:
            P.decide(self.out, path)
        self.assertEqual(cm.exception.code, 'decisions_drift')

    def test_decide_refused_before_awaiting_review(self):
        other = make_run(private_tmp())[2]
        with self.assertRaises(P.Refused) as cm:
            P.decide(other, self.decisions())
        self.assertEqual(cm.exception.code, 'not_awaiting_review')

    def test_runner_continues_after_decide_and_skips_awaiting_review(self):
        P.decide(self.out, self.decisions())
        calls = []
        handlers = {name: (lambda n: (lambda ctx: calls.append(n) or {}))(name) for name in P.PHASES}
        state = P.runner(self.out, handlers)
        self.assertEqual(state, 'ready')
        self.assertEqual(calls, [n for n in P.PHASES[P.PHASES.index('review'):] if n != 'oracle_rebuild'])


def write_json_file(path, value):
    path.parent.mkdir(mode=0o700, exist_ok=True)
    write_file(path, json.dumps(value).encode('utf-8'), 0o600)


class SealedBundleBytesTests(unittest.TestCase):
    """The API pins catalog.json by the SHA-256 of its raw bytes (native_source_rebind)."""

    def test_bundle_catalog_is_written_in_the_producer_canonical_bytes(self):
        tmp = private_tmp()
        catalog = {'protocol': 'oo.pipeline.sealed-import-collection/v1', 'collection_sha256': 'ab' * 32,
                   'binding_vector': [{'dataset': 'ob_fixture.customers', 'input_digest': 'cd' * 32}],
                   'entries': [{'tables': [{'logical_schema': 'ob_fixture', 'logical_table': 'customers'}]}]}
        path = tmp / 'catalog.json'
        P.write_canonical_json(path, catalog)
        raw = path.read_bytes()
        producer = (json.dumps(catalog, sort_keys=True, separators=(',', ':'), ensure_ascii=False) + '\n').encode()
        self.assertEqual(raw, producer)
        self.assertEqual(raw.count(b'\n'), 1)

    def test_pretty_printed_catalog_is_not_the_pinned_form(self):
        tmp = private_tmp()
        catalog = {'protocol': 'p', 'entries': [{'a': 1}]}
        P.write_json(tmp / 'pretty.json', catalog)
        P.write_canonical_json(tmp / 'compact.json', catalog)
        self.assertNotEqual((tmp / 'pretty.json').read_bytes(), (tmp / 'compact.json').read_bytes())


class TeardownTests(unittest.TestCase):
    def test_teardown_closes_recorded_forward_and_is_idempotent(self):
        tmp = private_tmp()
        _, _, out = make_run(tmp)
        P.write_json(out / 'private' / 'ssh-forward.json', {'control': '/x/ssh.sock', 'target': 'oo@h', 'pid': 999999})
        calls = []
        with mock.patch.object(P, 'run_cmd', side_effect=lambda o, label, argv, **kw: calls.append(argv) or (0, b'')):
            P.teardown_forward(out)
            P.teardown_forward(out)
        self.assertEqual(calls, [['ssh', '-S', '/x/ssh.sock', '-O', 'exit', 'oo@h']])
        self.assertFalse((out / 'private' / 'ssh-forward.json').exists())

    def test_cancel_without_forward_only_journals(self):
        tmp = private_tmp()
        _, _, out = make_run(tmp)
        self.assertEqual(P.cancel(out), {'state': 'canceled'})
        self.assertEqual(P.build_status(out)['state'], 'canceled')


class EgressTests(unittest.TestCase):
    def setUp(self):
        self.tmp = private_tmp()
        self.site, self.run, self.out = make_run(self.tmp)
        self.ctx = P.Ctx(self.out, self.site)

    def test_forward_is_added_explicitly_after_clearing_config_forwards(self):
        calls = []

        class FakeProc:
            pid = 4242

            def poll(self):
                return None

        def fake_popen(argv, **kwargs):
            calls.append(('master', argv, kwargs))
            (self.out / 'private' / 'ssh.sock').write_bytes(b'')
            return FakeProc()

        def fake_run_cmd(out, label, argv, timeout=900, check=True, stdin=None):
            calls.append((label, argv, None))
            return 0, b''

        with mock.patch.object(P.subprocess, 'Popen', side_effect=fake_popen), \
                mock.patch.object(P, 'run_cmd', side_effect=fake_run_cmd), \
                mock.patch.object(P, 'free_port', return_value=40123):
            forward = P.open_forward(self.ctx, dict(GOOD_SIX))
        label, master, kwargs = calls[0]
        self.assertEqual(master[0], 'ssh')
        self.assertNotIn('-f', master)
        self.assertNotIn('-L', master)
        self.assertIn('ClearAllForwardings=yes', master)
        self.assertIn('ExitOnForwardFailure=yes', master)
        self.assertEqual(master[-1], 'oo@192.0.2.20')
        self.assertTrue(kwargs['start_new_session'])
        control = master[master.index('-S') + 1]
        self.assertTrue(control.startswith(str(self.out / 'private')))
        add_label, add, _ = calls[1]
        self.assertEqual(add_label, 'ssh-forward-add')
        self.assertEqual(add[add.index('-O') + 1], 'forward')
        self.assertEqual(add[add.index('-L') + 1], '127.0.0.1:40123:fixture.internal:3306')
        self.assertEqual(forward['local_port'], 40123)
        self.assertNotIn(PASSWORD.decode(), ' '.join(master + add))

    def test_forward_uses_the_trusted_site_proxy_not_a_source_field(self):
        proxy = 'ssh -S /operator/master.sock -W 127.0.0.1:22 oo@192.0.2.20'
        self.site['egress']['ssh_proxy_command'] = proxy

        def fake_popen(argv, **kwargs):
            (self.out / 'private' / 'ssh.sock').write_bytes(b'')
            return mock.Mock(pid=4242, poll=lambda: None)

        with mock.patch.object(P.subprocess, 'Popen', side_effect=fake_popen) as popen, \
                mock.patch.object(P, 'run_cmd', return_value=(0, b'')), \
                mock.patch.object(P, 'free_port', return_value=40123):
            P.open_forward(self.ctx, dict(GOOD_SIX, ssh_proxy_command='untrusted-source-command'))
        argv = popen.call_args.args[0]
        self.assertEqual(argv[-3:], ['-o', 'ProxyCommand=' + proxy, 'oo@192.0.2.20'])
        self.assertNotIn('untrusted-source-command', ' '.join(argv))

    def test_forward_add_failure_is_refused_and_master_closed(self):
        class FakeProc:
            pid = 4242

            def poll(self):
                return None

        def fake_popen(argv, **kwargs):
            (self.out / 'private' / 'ssh.sock').write_bytes(b'')
            return FakeProc()

        closed = []
        with mock.patch.object(P.subprocess, 'Popen', side_effect=fake_popen), \
                mock.patch.object(P, 'run_cmd', return_value=(255, b'')), \
                mock.patch.object(P, 'close_forward', side_effect=lambda ctx, f: closed.append(f)):
            with self.assertRaises(P.Refused) as cm:
                P.open_forward(self.ctx, dict(GOOD_SIX))
        self.assertEqual(cm.exception.code, 'ssh_forward_failed')
        self.assertEqual(len(closed), 1)

    def test_control_socket_path_length_is_bounded(self):
        import types
        ctx = types.SimpleNamespace(out=self.tmp / ('x' * 120), site=self.site)
        with self.assertRaises(P.Refused) as cm:
            P.open_forward(ctx, dict(GOOD_SIX))
        self.assertEqual(cm.exception.code, 'control_path_too_long')

    def fake_server(self, payload_bytes):
        srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        srv.bind(('127.0.0.1', 0))
        srv.listen(1)

        def serve():
            conn, _ = srv.accept()
            conn.sendall(payload_bytes)
            conn.close()
            srv.close()
        threading.Thread(target=serve, daemon=True).start()
        return srv.getsockname()[1]

    def handshake(self, version):
        payload = b'\x0a' + version + b'\x00' + b'\x01\x00\x00\x00' + b'\x00' * 9
        return len(payload).to_bytes(3, 'little') + b'\x00' + payload

    def test_mysql_greeting_parses_protocol_v10_and_rejects_other_data(self):
        port = self.fake_server(self.handshake(b'5.7.44-log'))
        self.assertEqual(P.mysql_greeting(port, timeout=5), '5.7.44-log')
        port = self.fake_server(b'\x05\x00\x00\x00\xff\x00\x00hi\x00')
        self.assertIsNone(P.mysql_greeting(port, timeout=5))

    def test_eip_refuses_when_producer_lacks_tls_server_name(self):
        port = self.fake_server(self.handshake(b'5.7.44-log'))
        closed = []
        fake = mock.Mock()
        fake.poll.return_value = None
        with mock.patch.object(P, 'open_forward', return_value={'local_port': port, 'control': Path('/x'), 'target': 't', 'proc': fake}), \
                mock.patch.object(P, 'close_forward', side_effect=lambda ctx, f: closed.append(f)):
            with self.assertRaises(P.Refused) as cm:
                P.egress_eip(self.ctx, dict(GOOD_SIX))
        self.assertEqual(cm.exception.code, 'producer_tls_server_name_unsupported')
        self.assertEqual(len(closed), 1)

    def test_eip_refuses_when_source_not_reachable_through_forward(self):
        port = self.fake_server(b'')
        fake = mock.Mock()
        fake.poll.return_value = None
        with mock.patch.object(P, 'open_forward', return_value={'local_port': port, 'control': Path('/x'), 'target': 't', 'proc': fake}), \
                mock.patch.object(P, 'close_forward'), mock.patch.object(P, 'GREETING_WAIT_SECONDS', 1):
            with self.assertRaises(P.Refused) as cm:
                P.egress_eip(self.ctx, dict(GOOD_SIX))
        self.assertEqual(cm.exception.code, 'forward_source_unreachable')


class LiveHelperTests(unittest.TestCase):
    def test_lenenc_int_decodes_all_widths(self):
        self.assertEqual(P._lenenc_int(b'\x05', 0), (5, 1))
        self.assertEqual(P._lenenc_int(b'\xfc\x01\x02', 0), (0x0201, 3))
        self.assertEqual(P._lenenc_int(b'\xfd\x01\x02\x03', 0), (0x030201, 4))
        self.assertEqual(P._lenenc_int(b'\xfe' + (2**40).to_bytes(8, 'little'), 0), (2**40, 9))

    def test_native_scramble_is_20_bytes_and_empty_for_empty_password(self):
        self.assertEqual(P._native_scramble(b'', b'x' * 20), b'')
        self.assertEqual(len(P._native_scramble(b'pw', b'x' * 20)), 20)
        self.assertNotEqual(P._native_scramble(b'pw', b'x' * 20), P._native_scramble(b'pw2', b'x' * 20))

    def test_discover_selects_only_single_integer_primary_key_tables(self):
        rows = [
            ['orders', 'id', '1', 'PRI', 'bigint'], ['orders', 'total', '2', '', 'decimal'],
            ['pairs', 'a', '1', 'PRI', 'int'], ['pairs', 'b', '2', 'PRI', 'int'],
            ['codes', 'code', '1', 'PRI', 'varchar'], ['codes', 'name', '2', '', 'varchar'],
        ]

        class FakeClient:
            def __init__(self, *a, **k):
                pass

            def query(self, sql):
                return rows

            def close(self):
                pass

        with mock.patch.object(P, 'MySQLClient', FakeClient):
            result = P.discover_schema('127.0.0.1', 1, 'oo_cdc', 'pw', 'ob_fixture', '/ca', 'host')
        self.assertEqual(result['tables'], ['ob_fixture.orders'])
        self.assertEqual(result['columns'], {'ob_fixture.orders': ['id', 'total']})
        self.assertEqual(result['rejected'], {'codes': 'needs_single_integer_primary_key', 'pairs': 'needs_single_integer_primary_key'})

    def test_discover_refuses_when_nothing_is_supported(self):
        class FakeClient:
            def __init__(self, *a, **k):
                pass

            def query(self, sql):
                return [['t', 'c', '1', '', 'varchar']]

            def close(self):
                pass

        with mock.patch.object(P, 'MySQLClient', FakeClient):
            with self.assertRaises(P.Refused) as cm:
                P.discover_schema('h', 1, 'u', 'p', 'd', '/ca', 'h')
        self.assertEqual(cm.exception.code, 'discovery_no_supported_tables')

    def test_resolve_alias_requires_exactly_one_container(self):
        net = json.dumps([{'Containers': {'a': {'Name': 'pg-one'}, 'b': {'Name': 'other'}}}])
        def fake_quiet(argv, timeout=60):
            name = argv[-1]
            aliases = ['oo-dl01w1-71-pg'] if name == 'pg-one' else ['unrelated']
            return 0, json.dumps({'net': {'Aliases': aliases}}).encode()
        out = private_tmp()
        with mock.patch.object(P, 'run_cmd', return_value=(0, net.encode())), \
                mock.patch.object(P, 'run_cmd_quiet', side_effect=fake_quiet):
            self.assertEqual(P.resolve_alias(out, 'net', 'oo-dl01w1-71-pg'), 'pg-one')
            with self.assertRaises(P.Refused) as cm:
                P.resolve_alias(out, 'net', 'missing-alias')
        self.assertEqual(cm.exception.code, 'alias_unresolved:missing-alias')

    def test_new_database_is_refused_unless_this_run_created_it(self):
        out = private_tmp()
        with mock.patch.object(P, 'run_cmd', return_value=(0, b'1\n')):
            with self.assertRaises(P.Refused) as cm:
                P.ensure_new_database(out, 'pg', 'admin', 'ob_x_identity_v1', 'admin')
        self.assertEqual(cm.exception.code, 'database_exists:ob_x_identity_v1')
        (out / 'evidence').mkdir()
        (out / 'evidence' / '0001-createdb-ob_x_identity_v1.exit.json').write_text('{"exit": 0}')
        with mock.patch.object(P, 'run_cmd', return_value=(0, b'1\n')) as run:
            P.ensure_new_database(out, 'pg', 'admin', 'ob_x_identity_v1', 'admin')
        self.assertEqual(run.call_count, 1)

    def test_public_ca_file_must_not_be_group_or_world_writable(self):
        tmp = private_tmp()
        ca = write_file(tmp / 'ca.pem', b'CERT', 0o644)
        self.assertEqual(P.owned_public_file(ca), ca)
        bad = write_file(tmp / 'bad.pem', b'CERT', 0o664)
        with self.assertRaises(P.Refused) as cm:
            P.owned_public_file(bad)
        self.assertEqual(cm.exception.code, 'ca_file_invalid')


class SecretHygieneTests(unittest.TestCase):
    def test_password_appears_only_in_its_0600_secret_file(self):
        tmp = private_tmp()
        secret = b'Zq9-unique-hygiene-probe-7731'
        site, run, out = make_run(tmp, password=secret)
        P.journal_append(out, 'validate', 'started')
        P.record_phase(out, 'validate', P.now(), {'secret_ref': GOOD_SIX['secret_ref']})
        P.build_status(out)
        status_text = json.dumps(P.build_status(out))
        self.assertNotIn(secret.decode(), status_text)
        leaks = []
        for root in (out, site['secret_root']):
            for dirpath, _, files in os.walk(root):
                for name in files:
                    path = Path(dirpath) / name
                    if path.name == 'password.pass':
                        self.assertEqual(stat.S_IMODE(os.stat(path).st_mode), 0o600)
                        continue
                    if secret in path.read_bytes():
                        leaks.append(str(path))
        self.assertEqual(leaks, [])
        pw_file = Path(site['secret_root']) / 'provision' / run['run_id'] / 'password.pass'
        self.assertEqual(pw_file.read_bytes(), secret + b'\n')
        self.assertEqual(stat.S_IMODE(os.stat(pw_file.parent).st_mode), 0o700)
        registry = json.loads((out / 'private' / 'secret-registry.json').read_text())
        self.assertEqual(registry, {GOOD_SIX['secret_ref']: str(pw_file)})

    def test_seed_semantics_do_not_claim_the_typed_oracle_was_not_run(self):
        src = Path(P.__file__).read_text()
        self.assertNotIn("'typed_oracle': 'not_run'", src)
        self.assertNotIn('Typed oracle not run', src)

    def test_status_and_review_never_include_secret_material(self):
        tmp = private_tmp()
        secret = b'Another-Probe-Value-0098'
        _, _, out = make_run(tmp, password=secret)
        self.assertNotIn(secret.decode(), json.dumps(P.build_status(out)))


class ProfileCredentialTests(unittest.TestCase):
    SIX = {'type': 'mysql', 'host': 'h', 'port': 3306, 'database': 'ob_fixture', 'user': 'oo_cdc',
           'secret_ref': 'secret://owned-test-ref'}

    def test_profile_input_has_exactly_user_and_secret_ref(self):
        good = {'user': 'oo_profile', 'secret_ref': 'secret://owned-test-profile'}
        self.assertEqual(P.validate_profile(dict(good), self.SIX), good)
        for bad in (dict(good, password='x'), {'user': 'oo_profile'}, dict(good, host='h')):
            with self.subTest(bad=sorted(bad)):
                with self.assertRaises(P.Refused):
                    P.validate_profile(bad, self.SIX)

    def test_profile_must_be_a_different_account_than_the_cdc_account(self):
        with self.assertRaises(P.Refused) as cm:
            P.validate_profile({'user': 'oo_cdc', 'secret_ref': 'secret://owned-test-profile'}, self.SIX)
        self.assertEqual(cm.exception.code, 'profile_same_as_six')
        with self.assertRaises(P.Refused) as cm:
            P.validate_profile({'user': 'oo_profile', 'secret_ref': 'secret://owned-test-ref'}, self.SIX)
        self.assertEqual(cm.exception.code, 'profile_same_as_six')

    def test_create_run_registers_profile_secret_and_six_for_follow(self):
        tmp = private_tmp()
        site = site_for(tmp / 'secret-root')
        (tmp / 'secret-root').mkdir(mode=0o700)
        profile = {'user': 'oo_profile', 'secret_ref': 'secret://owned-test-profile'}
        run, out = P.create_run(site, dict(self.SIX), PASSWORD, 'v1', 'direct', None, 'x', tmp / 'RUN',
                                profile=profile, profile_password=b'profile-pass-value')
        registry = json.loads((out / 'private' / 'secret-registry.json').read_text())
        self.assertEqual(set(registry), {self.SIX['secret_ref'], profile['secret_ref']})
        self.assertFalse((out / 'private' / 'profile-six.json').exists())
        self.assertEqual(json.loads((out / 'run.json').read_text())['profile'], profile)
        self.assertNotIn('profile-pass-value', (out / 'run.json').read_text())
        self.assertEqual(run['profile'], profile)

    def test_pinned_source_onboarding_and_producer_are_the_p12_and_p11_binaries(self):
        site = P.load_site()
        self.assertEqual(site['binaries']['source_onboarding']['sha256'],
                         '362c2f0c5dd71dcf770e26c7444af19f9fc29db83472c52533686b9d3f8138ca')
        self.assertEqual(site['binaries']['producer']['sha256'],
                         'ff1e7f801bb8da6ae611633b888919658f30b034cb8cde20fa6887a36101abf3')
        self.assertEqual(site['product']['source_access'], 'network')


class S0S8RetryTests(unittest.TestCase):
    def setUp(self):
        self.tmp = private_tmp()
        self.site, self.run, self.out = make_run(self.tmp)
        self.product = self.out / 'product'
        self.product.mkdir(mode=0o700, exist_ok=True)
        write_json_file(self.product / 'build.json', {'build_id': 'b_failed', 'config_sha256': 'x'})
        self.ctx = P.Ctx.__new__(P.Ctx)
        self.ctx.out, self.ctx.run, self.ctx.site = self.out, self.run, self.site

    def test_failed_attempt_with_a_build_is_archived_for_a_new_build(self):
        P.journal_append(self.out, 's0_s8', 'started')
        P.journal_append(self.out, 's0_s8', 'failed', error_code='command_failed:product-flow-prepare')
        self.assertEqual(P.archive_failed_s0_s8(self.ctx), 1)
        self.assertTrue((self.out / 'product-attempt-1' / 'build.json').is_file())
        self.assertTrue(self.product.is_dir())
        self.assertFalse((self.product / 'build.json').exists())

    def test_the_runner_started_event_after_a_failure_still_archives(self):
        P.journal_append(self.out, 's0_s8', 'started')
        P.journal_append(self.out, 's0_s8', 'failed', error_code='command_failed:product-flow-prepare')
        P.journal_append(self.out, 's0_s8', 'started')
        self.assertEqual(P.archive_failed_s0_s8(self.ctx), 1)

    def test_nothing_is_archived_without_a_journaled_failure(self):
        P.journal_append(self.out, 's0_s8', 'started')
        self.assertIsNone(P.archive_failed_s0_s8(self.ctx))
        self.assertTrue((self.product / 'build.json').is_file())

    def test_a_prepared_attempt_is_never_archived(self):
        write_json_file(self.product / 'prepared.json', {'build_id': 'b_failed'})
        P.journal_append(self.out, 's0_s8', 'failed', error_code='x')
        self.assertIsNone(P.archive_failed_s0_s8(self.ctx))


class PiiPolicyTests(unittest.TestCase):
    """Withheld set = effective pii != none of the CURRENT build's overlay; decide() refuses any drift."""

    def setUp(self):
        self.tmp = private_tmp()
        self.site, self.run, self.out = make_run(self.tmp)
        self.product = self.out / 'product'
        self.product.mkdir(mode=0o700, exist_ok=True)

    def set_overlay(self, columns, build='b_test'):
        write_json_file(self.product / 'prepared-overlay.json', overlay(columns, build))

    def test_withheld_uses_effective_pii_from_builder_output_over_overlay(self):
        self.set_overlay([col('ob.orders', 'id', 'a' * 64), col('ob.customers', 'name', 'c' * 64)])
        semantics = {'ob.customers.name': {'dataset': 'ob.customers', 'column': 'name', 'pii': 'personal',
                                           'semantic_type': 'person_name'}}
        with mock.patch.object(P, 'model_semantics', return_value=semantics):
            doc = P.write_withheld(self.out)
        self.assertEqual(doc['withheld'], ['ob.customers.name'])
        self.assertEqual(doc['build_id'], 'b_test')

    def test_unknown_pii_is_withheld_and_listed_so_decide_can_block_it(self):
        self.set_overlay([col('ob.orders', 'id', 'a' * 64), dict(col('ob.customers', 'name', 'c' * 64), pii='unknown')])
        self.assertEqual(P.write_withheld(self.out)['withheld'], ['ob.customers.name'])

    def test_withheld_follows_the_current_build_not_an_older_one(self):
        self.set_overlay([col('ob.customers', 'name', 'c' * 64)], build='b_old')
        with mock.patch.object(P, 'model_semantics', return_value={}):
            self.assertEqual(P.write_withheld(self.out)['withheld'], [])
        self.set_overlay([dict(col('ob.customers', 'name', 'c' * 64), pii='personal')], build='b_new')
        with mock.patch.object(P, 'model_semantics', return_value={}):
            doc = P.write_withheld(self.out)
        self.assertEqual((doc['build_id'], doc['withheld']), ('b_new', ['ob.customers.name']))

    def test_decide_refuses_when_policy_was_never_applied(self):
        self.set_overlay([col('ob.orders', 'id', 'a' * 64)])
        with mock.patch.object(P, 'model_semantics', return_value={}):
            with self.assertRaises(P.Refused) as cm:
                P.assert_withheld_current(self.out)
        self.assertEqual(cm.exception.code, 'policy_not_applied')

    def test_decide_refuses_when_overlay_changed_after_the_policy(self):
        self.set_overlay([col('ob.orders', 'id', 'a' * 64)])
        with mock.patch.object(P, 'model_semantics', return_value={}):
            P.write_withheld(self.out)
            self.set_overlay([col('ob.orders', 'id', 'a' * 64), dict(col('ob.x', 'y', 'd' * 64), pii='personal')])
            with self.assertRaises(P.Refused) as cm:
                P.assert_withheld_current(self.out)
        self.assertEqual(cm.exception.code, 'withheld_drift')


class OracleRoundTests(unittest.TestCase):
    """Typed-oracle round: the follow's own oracle value goes into a new sealed seed; review and publish run again."""

    def setUp(self):
        self.tmp = private_tmp()
        self.site, self.run, self.out = make_run(self.tmp)
        self.site['secret_root'] = str(self.tmp / 'secret-root')
        self.ctx = P.Ctx.__new__(P.Ctx)
        self.ctx.out, self.ctx.run, self.ctx.site = self.out, self.run, self.site
        self.rid = self.run['run_id']
        self.shared_root = self.tmp / 'shared-root'
        patcher = mock.patch.object(P, 'SHARED_PROVISION', self.shared_root)
        patcher.start()
        self.addCleanup(patcher.stop)
        self.product = self.out / 'product'
        self.product.mkdir(mode=0o700, exist_ok=True)
        write_json_file(self.out / 'phases' / 'artifacts.json', {'data': {
            'bundle': str(self.tmp / 'bundle'), 'source_id': 'mysql_t', 'generation': 1, 'catalog_sha256': HEX}})
        hidden = ['ob_fixture.customers.name']
        P.save_pii_state(self.out, {'protocol': P.PROTOCOL_PII, 'passes': [
            {'pass': 1, 'bundle': 'bundle-static-v1', 'hidden': [], 'build_id': 'b_1', 'decision': 'rebuild'},
            {'pass': 2, 'bundle': 'bundle-static-v1-pii2', 'hidden': hidden, 'build_id': 'b_2', 'decision': 'clean'}]})
        write_json_file(self.product / 'build.json', {'build_id': 'b_2', 'config_sha256': 'x'})
        write_json_file(self.product / 'prepared-overlay.json', overlay([col('ob_fixture.orders', 'id', 'a' * 64)], 'b_2'))
        write_json_file(self.out / 'review' / 'decisions.json', {'reviews': [], 'confirm_columns': []})
        write_json_file(self.out / 'private' / 'product-decisions.json', {'reviews': []})

    def write_oracle(self, cells, carried=None, protocol='oo.mysql.full-typed-oracle/v1'):
        base = self.shared_root / self.rid / 'follow' / 'follow'
        base.mkdir(mode=0o700, parents=True, exist_ok=True)
        write_json_file(base / 'typed-oracle.json', {'protocol': protocol, 'different_cells': cells})
        quality = {'columns': 23, 'rows': 11500}
        if carried is not None or cells is not None:
            quality['full_typed_cell_differences'] = cells if carried is None else carried
        write_json_file(base / 'source-semantics.json', {'quality': quality})

    def fake_build(self, calls):
        def build(ctx, bundle_name, hidden, artifacts, oracle_cells=None):
            calls.append({'bundle': bundle_name, 'hidden': sorted(hidden), 'oracle_cells': oracle_cells})
            build_id = 'b_%d' % (2 + len(calls))
            write_json_file(self.product / 'build.json', {'build_id': build_id, 'config_sha256': 'y'})
            write_json_file(self.product / 'prepared-overlay.json', overlay(
                [col('ob_fixture.orders', 'id', 'a' * 64)], build_id))
            return {'build_id': build_id, 'overlay_sha256': 'f' * 64, 'bundle': bundle_name, 'hidden': sorted(hidden),
                    'api_restarted': True, 's5': {'llm_calls': 1, 'cache_hit': False}}
        return build

    def test_typed_oracle_result_is_read_from_the_follows_own_output(self):
        self.write_oracle(0)
        self.assertEqual(P.typed_oracle_result(self.rid), 0)

    def test_missing_or_drifting_oracle_output_is_refused(self):
        with self.assertRaises(P.Refused) as cm:
            P.typed_oracle_result(self.rid)
        self.assertEqual(cm.exception.code, 'typed_oracle_missing')
        self.write_oracle(0, carried=2)
        with self.assertRaises(P.Refused) as cm:
            P.typed_oracle_result(self.rid)
        self.assertEqual(cm.exception.code, 'typed_oracle_drift')

    def test_oracle_differences_stop_the_run_before_any_rebuild(self):
        self.write_oracle(3)
        with mock.patch.object(P, 'run_build') as run:
            with self.assertRaises(P.Refused) as cm:
                P.handler_oracle_rebuild(self.ctx)
        self.assertEqual(cm.exception.code, 'typed_oracle_mismatch')
        run.assert_not_called()
        self.assertEqual(len(P.load_pii_state(self.out)['passes']), 2)

    def test_a_seed_that_already_carries_the_value_is_not_rebuilt(self):
        state = P.load_pii_state(self.out)
        state['passes'][-1]['oracle_cells'] = 0
        state['passes'][-1]['seed_version'] = P.SEED_CONTENT_VERSION
        P.save_pii_state(self.out, state)
        self.write_oracle(0)
        with mock.patch.object(P, 'run_build') as run:
            with self.assertRaises(P.Refused) as cm:
                P.handler_oracle_rebuild(self.ctx)
        self.assertEqual(cm.exception.code, 'publish_refused_after_oracle')
        run.assert_not_called()

    def test_an_older_seed_content_version_is_rebuilt_even_with_the_same_oracle_value(self):
        state = P.load_pii_state(self.out)
        state['passes'][-1]['oracle_cells'] = 0
        P.save_pii_state(self.out, state)
        self.write_oracle(0)
        calls = []
        with mock.patch.object(P, 'run_build', side_effect=self.fake_build(calls)), \
                mock.patch.object(P, 'model_semantics', return_value={}):
            P.handler_oracle_rebuild(self.ctx)
        self.assertEqual(calls[0]['bundle'], 'bundle-static-v1-oracle1')
        self.assertEqual(P.load_pii_state(self.out)['passes'][-1]['seed_version'], P.SEED_CONTENT_VERSION)


    def test_oracle_round_builds_a_new_seed_with_the_value_and_keeps_the_round_one_review(self):
        self.write_oracle(0)
        calls = []
        with mock.patch.object(P, 'run_build', side_effect=self.fake_build(calls)), \
                mock.patch.object(P, 'model_semantics', return_value={}):
            result = P.handler_oracle_rebuild(self.ctx)
        self.assertEqual(calls, [{'bundle': 'bundle-static-v1-oracle1', 'hidden': ['ob_fixture.customers.name'],
                                  'oracle_cells': 0}])
        state = P.load_pii_state(self.out)
        last = state['passes'][-1]
        self.assertEqual((last['kind'], last['bundle'], last['oracle_cells'], last['build_id']),
                         ('oracle', 'bundle-static-v1-oracle1', 0, 'b_3'))
        self.assertEqual(result['build_id'], 'b_3')
        self.assertFalse((self.out / 'review' / 'decisions.json').exists())
        self.assertTrue((self.out / 'review' / 'decisions-before-oracle-1.json').is_file())
        self.assertTrue((self.out / 'private' / 'product-decisions-before-oracle-1.json').is_file())
        self.assertTrue((self.out / 'product-pass-2.1' / 'build.json').is_file())
        self.assertEqual(json.loads((self.out / 'private' / 'withheld.json').read_text())['build_id'], 'b_3')
        events = [(e['phase'], e['event']) for e in P.read_journal(self.out) if e['phase'] == 'oracle_rebuild']
        self.assertEqual(events, [('oracle_rebuild', 'declared'), ('oracle_rebuild', 'built')])

    def test_oracle_rebuilds_are_bounded(self):
        state = P.load_pii_state(self.out)
        state['passes'].append({'pass': 3, 'kind': 'oracle', 'bundle': 'bundle-static-v1-oracle1', 'hidden': [],
                                'build_id': 'b_3', 'oracle_cells': 5})
        state['passes'].append({'pass': 4, 'kind': 'oracle', 'bundle': 'bundle-static-v1-oracle2', 'hidden': [],
                                'build_id': 'b_4', 'oracle_cells': 5})
        P.save_pii_state(self.out, state)
        self.write_oracle(0)
        with mock.patch.object(P, 'run_build') as run:
            with self.assertRaises(P.Refused) as cm:
                P.handler_oracle_rebuild(self.ctx)
        self.assertEqual(cm.exception.code, 'oracle_rebuild_exhausted')
        run.assert_not_called()

    def test_runner_routes_an_oracle_refused_publish_through_the_oracle_round(self):
        calls = []
        handlers = {name: (lambda n: (lambda ctx: calls.append(n) or {}))(name) for name in P.PHASES}
        for name in P.PHASES[:P.PHASES.index('publish')]:
            handlers[name] = (lambda n: (lambda ctx: {}))(name)
        journal = self.out / 'journal.jsonl'
        for name in P.PHASES[:P.PHASES.index('publish')]:
            P.journal_append(self.out, name, 'started')
            P.record_phase(self.out, name, P.now(), {})
            P.journal_append(self.out, name, 'succeeded')
        P.journal_append(self.out, 'publish', 'started')
        P.journal_append(self.out, 'publish', 'failed', error_code='follow_failed:actual_api_refused')
        P.journal_append(self.out, '_run', 'state', state='failed', error_code='follow_failed:actual_api_refused')
        self.write_oracle(0)
        handlers['oracle_rebuild'] = lambda ctx: calls.append('oracle_rebuild') or {'build_id': 'b_3'}
        state = P.runner(self.out, handlers)
        self.assertEqual(state, 'awaiting_review')
        self.assertEqual(calls, ['oracle_rebuild'])
        summary = P.phase_summary(P.read_journal(self.out))
        self.assertNotIn('publish', summary)
        self.assertNotIn('review', summary)
        self.assertEqual(P.build_status(self.out)['state'], 'awaiting_review')

    def test_phase_summary_resets_the_tail_only_after_an_oracle_round(self):
        P.journal_append(self.out, 'review', 'succeeded')
        P.journal_append(self.out, 'publish', 'failed', error_code='follow_failed:actual_api_refused')
        summary = P.phase_summary(P.read_journal(self.out))
        self.assertEqual(summary['publish']['state'], 'failed')
        P.journal_append(self.out, 'oracle_rebuild', 'succeeded')
        summary = P.phase_summary(P.read_journal(self.out))
        self.assertNotIn('publish', summary)
        self.assertNotIn('review', summary)
        self.assertEqual(summary['oracle_rebuild']['state'], 'succeeded')


class TwoPassPiiTests(unittest.TestCase):
    """Two-pass PII build: hidden declaration, sealed bundle without hidden columns, API sync, journaled passes."""

    def setUp(self):
        self.tmp = private_tmp()
        self.site, self.run, self.out = make_run(self.tmp)
        self.site['secret_root'] = str(self.tmp / 'secret-root')
        self.product = self.out / 'product'
        self.product.mkdir(mode=0o700, exist_ok=True)
        self.ctx = P.Ctx.__new__(P.Ctx)
        self.ctx.out, self.ctx.run, self.ctx.site = self.out, self.run, self.site
        self.rid = self.run['run_id']
        self.names = self.run['names']
        self.artifacts = self.tmp / 'artifacts-bundle'
        self.artifacts.mkdir(mode=0o700)
        self.native = self.tmp / 'native'
        self.native.mkdir(mode=0o700)
        write_json_file(self.out / 'phases' / 'artifacts.json', {'data': {'bundle': str(self.artifacts),
                                                                         'source_id': 'mysql_t', 'generation': 1,
                                                                         'catalog_sha256': HEX}})

    def catalog(self):
        def table(name, columns):
            return {'logical_schema': 'ob_fixture', 'logical_table': name, 'stored_namespace': 'ns',
                    'stored_physical': 'p_' + name, 'columns': [{'name': c} for c in columns]}
        return {'protocol': 'oo.pipeline.sealed-import-collection/v1', 'binding_vector': [],
                'entries': [{'tables': [table('customers', ['id', 'name', 'credit_limit'])]},
                            {'tables': [table('orders', ['id', 'total'])]}]}

    def write_capture_inputs(self):
        (self.artifacts / 'catalog.json').write_bytes(
            (json.dumps(self.catalog(), sort_keys=True, separators=(',', ':')) + '\n').encode())
        write_json_file(self.artifacts / 'profile.json', {'tables': []})
        write_json_file(self.native / 'request.json', {'entries': [{'schema': 'ob_fixture'}],
                                                      'baseline_scope': {}})
        write_json_file(self.native / 'load.json', {'entries': [{'row_count': 2}], 'target_generation': 1,
                                                    'source_epoch': 1, 'applied_pos': 'x'})
        write_json_file(self.native / 'ack.json', {'ok': True})
        write_json_file(self.out / 'private' / 'discovery.json', {
            'tables': ['ob_fixture.customers', 'ob_fixture.orders'],
            'types': {'ob_fixture.customers': {'id': 'bigint', 'name': 'varchar', 'credit_limit': 'decimal'},
                      'ob_fixture.orders': {'id': 'bigint', 'total': 'decimal'}}})

    def ensure(self, bundle, hidden):
        return P.ensure_bundle_complete(self.ctx, {'bundle': str(self.artifacts), 'source_id': 'mysql_t',
                                                   'generation': 1, 'catalog_sha256': HEX},
                                        bundle=bundle, hidden=hidden, native=self.native)

    def test_hidden_columns_leave_role_visibility_and_native_model_but_not_capture_facts(self):
        self.write_capture_inputs()
        bundle = self.tmp / 'bundle-pii2'
        bundle.mkdir(mode=0o700)
        catalog_before = (self.artifacts / 'catalog.json').read_bytes()
        (bundle / 'catalog.json').write_bytes(catalog_before)
        write_json_file(bundle / 'profile.json', {'tables': []})
        info = self.ensure(bundle, {'ob_fixture.customers.name', 'ob_fixture.orders.total'})
        role = self.names['role']
        roles = json.loads((bundle / 'roles.json').read_text())['roles'][role]
        self.assertEqual(sorted(roles), ['ob_fixture.customers.credit_limit', 'ob_fixture.customers.id',
                                         'ob_fixture.orders.id'])
        model = json.loads((bundle / 'native-model.json').read_text())
        visible = sorted('%s.%s' % (d['dataset'], c) for d in model['visibility']['datasets'] for c in d['columns'])
        self.assertEqual(visible, sorted(roles))
        rules = json.loads((bundle / 'rules.json').read_text())['columns']
        self.assertEqual([r['column'] for r in rules], ['ob_fixture.customers.credit_limit'])
        semantics = json.loads((bundle / 'source_semantics.json').read_text())
        self.assertEqual(semantics['quality']['columns'], 5)
        self.assertEqual((bundle / 'catalog.json').read_bytes(), catalog_before)
        self.assertEqual(info['hidden'], ['ob_fixture.customers.name', 'ob_fixture.orders.total'])
        self.assertIn('roles.json', json.loads((bundle / 'manifest.json').read_text())['files'])

    def test_sealed_inputs_are_never_rewritten_after_a_build_recorded_them(self):
        self.write_capture_inputs()
        bundle = self.tmp / 'bundle-pii2'
        bundle.mkdir(mode=0o700)
        (bundle / 'catalog.json').write_bytes((self.artifacts / 'catalog.json').read_bytes())
        write_json_file(bundle / 'profile.json', {'tables': []})
        first = self.ensure(bundle, {'ob_fixture.customers.name'})
        again = self.ensure(bundle, {'ob_fixture.customers.name'})
        self.assertEqual(first['manifest_sha256'], again['manifest_sha256'])
        with self.assertRaises(P.Refused) as cm:
            self.ensure(bundle, set())
        self.assertEqual(cm.exception.code, 'sealed_inputs_rewrite')

    def test_the_first_bundle_keeps_every_column_when_nothing_is_hidden(self):
        self.write_capture_inputs()
        bundle = self.artifacts
        self.ensure(bundle, set())
        roles = json.loads((bundle / 'roles.json').read_text())['roles'][self.names['role']]
        self.assertEqual(len(roles), 5)

    def test_pass_bundle_names_and_pass_bundle_copies_only_capture_bytes(self):
        self.assertEqual(P.pass_bundle_name(1), 'bundle-static-v1')
        self.assertEqual(P.pass_bundle_name(2), 'bundle-static-v1-pii2')
        self.write_capture_inputs()
        target = P.make_pass_bundle(self.ctx, {'bundle': str(self.artifacts)}, 'bundle-static-v1-pii2')
        self.assertEqual((target / 'catalog.json').read_bytes(), (self.artifacts / 'catalog.json').read_bytes())
        self.assertEqual(stat.S_IMODE(os.stat(target).st_mode), 0o700)

    def test_sync_api_bundle_points_at_the_pass_bundle_and_restarts_only_when_changed(self):
        secret_root = Path(P.expand(self.site['secret_root']))
        api_dir = secret_root / 'provision' / self.rid / 'api'
        api_dir.mkdir(mode=0o700, parents=True)
        write_json_file(api_dir / 'api-runtime.json', {
            'projects': [{'sealed_bundle': '/owned/x/bundle-static-v1', 'role_policy_file': '/owned/x/roles.json',
                          'hidden_columns': []}],
            'principals': [{'object_scope': ['ob_fixture.customers.id', 'ob_fixture.customers.name']},
                           {'ledger_role': 'owner'}]})
        write_json_file(self.out / 'phases' / 'product_runtime.json', {'data': {'api_port': 19999}})
        pass_dir = P.pass_bundle_host(self.ctx, 'bundle-static-v1-pii2')
        pass_dir.mkdir(mode=0o700, parents=True)
        write_json_file(pass_dir / 'roles.json', {'roles': {self.names['role']: ['ob_fixture.customers.id']}})
        with mock.patch.object(P, 'stop_api_serve') as stop, mock.patch.object(P, 'serve_api') as serve, \
                mock.patch.object(P, 'container_binary', return_value='/run-current/bin/api'):
            self.assertTrue(P.sync_api_bundle(self.ctx, 'bundle-static-v1-pii2', {'ob_fixture.customers.name'}))
            runtime = json.loads((api_dir / 'api-runtime.json').read_text())
            project = runtime['projects'][0]
            self.assertEqual(project['sealed_bundle'], '/owned/provision/%s/bundle-static-v1-pii2' % self.rid)
            self.assertEqual(project['role_policy_file'],
                             '/owned/provision/%s/bundle-static-v1-pii2/roles.json' % self.rid)
            self.assertEqual(project['hidden_columns'], ['ob_fixture.customers.name'])
            self.assertEqual(runtime['principals'][0]['object_scope'], ['ob_fixture.customers.id'])
            self.assertNotIn('object_scope', runtime['principals'][1])
            self.assertEqual((stop.call_count, serve.call_count), (1, 1))
            self.assertFalse(P.sync_api_bundle(self.ctx, 'bundle-static-v1-pii2', {'ob_fixture.customers.name'}))
            self.assertEqual((stop.call_count, serve.call_count), (1, 1))

    def test_seed_position_lineage_is_the_capture_lineage(self):
        self.write_capture_inputs()
        request = json.loads((self.native / 'request.json').read_text())
        request['baseline_scope'] = {'lineage': 'mysql.abc', 'applied_pos': '65'}
        write_json_file(self.native / 'request.json', request)
        bundle = self.tmp / 'bundle-lineage'
        bundle.mkdir(mode=0o700)
        (bundle / 'catalog.json').write_bytes((self.artifacts / 'catalog.json').read_bytes())
        write_json_file(bundle / 'profile.json', {'tables': []})
        P.ensure_bundle_complete(self.ctx, {'bundle': str(self.artifacts), 'source_id': 'mysql_t', 'generation': 1,
                                            'catalog_sha256': HEX}, bundle=bundle, native=self.native)
        self.assertEqual(json.loads((bundle / 'source_semantics.json').read_text())['position_lineage'], 'mysql.abc')

    def test_oracle_value_is_written_into_the_source_semantics_quality(self):
        self.write_capture_inputs()
        bundle = self.tmp / 'bundle-oracle1'
        bundle.mkdir(mode=0o700)
        (bundle / 'catalog.json').write_bytes((self.artifacts / 'catalog.json').read_bytes())
        write_json_file(bundle / 'profile.json', {'tables': []})
        self.ensure_with = lambda **kw: P.ensure_bundle_complete(self.ctx, {
            'bundle': str(self.artifacts), 'source_id': 'mysql_t', 'generation': 1, 'catalog_sha256': HEX},
            bundle=bundle, native=self.native, **kw)
        self.ensure_with(hidden={'ob_fixture.customers.name'}, oracle_cells=0)
        quality = json.loads((bundle / 'source_semantics.json').read_text())['quality']
        self.assertEqual(quality['full_typed_cell_differences'], 0)
        self.assertEqual(quality['columns'], 5)

    def test_declared_hidden_columns_are_withheld_items_even_when_the_overlay_says_none(self):
        write_json_file(self.product / 'prepared-overlay.json', overlay(
            [col('ob_fixture.orders', 'id', 'a' * 64), dict(col('ob_fixture.customers', 'name', 'c' * 64), pii='none')]))
        P.declare_hidden(self.out, 'b_1', [{'name': 'ob_fixture.customers.name', 'dataset': 'ob_fixture.customers',
                                            'column': 'name', 'data_type': 'varchar', 'semantic_type': 'person_name',
                                            'pii': 'personal'}])
        with mock.patch.object(P, 'model_semantics', return_value={}):
            _, items = P.review_items(self.out)
            by = {item['name']: item for item in items}
            self.assertFalse(by['ob_fixture.customers.name']['reviewable'])
            self.assertEqual(by['ob_fixture.customers.name']['withheld_reason'], 'pii_personal')
            self.assertTrue(by['ob_fixture.orders.id']['reviewable'])
            doc = P.write_withheld(self.out)
            self.assertEqual(doc['withheld'], ['ob_fixture.customers.name'])
            review = P.review_document(self.out)
        declaration = review['hidden_declaration']
        self.assertEqual(declaration['columns'], ['ob_fixture.customers.name'])
        self.assertEqual(declaration['signer_class'], 'control_plane_test')
        self.assertIs(declaration['business_owner_signed'], False)

    def test_declaration_is_control_plane_test_and_records_the_build_that_classified_it(self):
        P.declare_hidden(self.out, 'b_1', [{'name': 'ob_fixture.customers.name', 'dataset': 'ob_fixture.customers',
                                            'column': 'name', 'pii': 'personal'}])
        doc = json.loads((self.out / 'private' / 'hidden-declaration.json').read_text())
        self.assertEqual((doc['declared_by'], doc['signer_class'], doc['business_owner_signed']),
                         ('control_plane_test', 'control_plane_test', False))
        self.assertEqual(doc['columns'][0]['classified_by_build'], 'b_1')

    def fake_builds(self, overlays, first_build=2):
        calls = []
        self.oracle_args = []

        def build(ctx, bundle_name, hidden, artifacts, oracle_cells=None):
            index = len(calls)
            calls.append((bundle_name, sorted(hidden)))
            self.oracle_args.append(oracle_cells)
            build_id = 'b_%d' % (first_build + index)
            write_json_file(self.product / 'build.json', {'build_id': build_id, 'config_sha256': 'x'})
            write_json_file(self.product / 'prepared-overlay.json', overlay(overlays[index], build_id))
            return {'build_id': build_id, 'overlay_sha256': 'f' * 64, 'bundle': bundle_name,
                    'hidden': sorted(hidden), 'api_restarted': False,
                    's5': {'llm_calls': 1, 'usage': {'total_tokens': 10}}}
        return calls, build

    def seed_pass_one(self, overlay_one):
        write_json_file(self.product / 'build.json', {'build_id': 'b_1', 'config_sha256': 'x'})
        write_json_file(self.product / 'prepared-overlay.json', overlay(overlay_one, 'b_1'))
        P.save_pii_state(self.out, {'protocol': P.PROTOCOL_PII, 'passes': [
            {'pass': 1, 'bundle': 'bundle-static-v1', 'hidden': [], 'build_id': 'b_1', 'overlay_sha256': 'f' * 64,
             'api_restarted': False, 's5': {'llm_calls': 1}}]})

    def test_pii_rebuild_builds_a_second_pass_on_a_bundle_without_the_visible_pii_column(self):
        name_pii = dict(col('ob_fixture.customers', 'name', 'c' * 64), pii='personal')
        self.seed_pass_one([name_pii, col('ob_fixture.orders', 'id', 'a' * 64)])
        # pass 2: the hidden column may still appear in the overlay, but it is no longer visible
        calls2, build2 = self.fake_builds([[name_pii, col('ob_fixture.orders', 'id', 'a' * 64)]])
        with mock.patch.object(P, 'model_semantics', return_value={}), \
                mock.patch.object(P, 'run_build', side_effect=build2):
            result = P.handler_pii_rebuild(self.ctx)
        self.assertEqual(calls2, [('bundle-static-v1-pii2', ['ob_fixture.customers.name'])])
        self.assertEqual(result['builds'], 2)
        self.assertEqual(result['final_build_id'], 'b_2')
        state = P.load_pii_state(self.out)
        self.assertEqual([p.get('decision') for p in state['passes']], ['rebuild', 'clean'])
        self.assertEqual(state['passes'][0]['visible_pii'], [{'name': 'ob_fixture.customers.name', 'pii': 'personal'}])
        self.assertTrue((self.out / 'product-pass-1.1' / 'build.json').is_file())
        events = [(e['phase'], e['event']) for e in P.read_journal(self.out) if e['phase'] == 'pii_rebuild']
        self.assertEqual(events, [('pii_rebuild', 'classified'), ('pii_rebuild', 'declared'),
                                  ('pii_rebuild', 'built'), ('pii_rebuild', 'classified')])
        withheld = json.loads((self.out / 'private' / 'withheld.json').read_text())
        self.assertEqual(withheld['withheld'], ['ob_fixture.customers.name'])
        self.assertEqual(withheld['build_id'], 'b_2')

    def test_pii_rebuild_stops_with_pii_unstable_after_three_builds_and_keeps_the_evidence(self):
        name_pii = dict(col('ob_fixture.customers', 'name', 'c' * 64), pii='personal')
        profile_pii = dict(col('ob_fixture.customers', 'profile', 'd' * 64), pii='personal')
        email_pii = dict(col('ob_fixture.customers', 'email', 'e' * 64), pii='personal')
        self.seed_pass_one([name_pii])
        _, build = self.fake_builds([[profile_pii], [email_pii]])
        with mock.patch.object(P, 'model_semantics', return_value={}), \
                mock.patch.object(P, 'run_build', side_effect=build):
            with self.assertRaises(P.Refused) as cm:
                P.handler_pii_rebuild(self.ctx)
        self.assertEqual(cm.exception.code, 'pii_unstable')
        state = P.load_pii_state(self.out)
        self.assertEqual(len(state['passes']), 3)
        self.assertEqual(state['passes'][-1]['visible_pii'], [{'name': 'ob_fixture.customers.email', 'pii': 'personal'}])
        self.assertEqual([c['name'] for c in P.load_hidden_declaration(self.out)['columns']],
                         ['ob_fixture.customers.name', 'ob_fixture.customers.profile'])

    def test_a_declared_pass_without_a_build_is_built_on_resume(self):
        name_pii = dict(col('ob_fixture.customers', 'name', 'c' * 64), pii='personal')
        self.seed_pass_one([name_pii])
        P.declare_hidden(self.out, 'b_1', [{'name': 'ob_fixture.customers.name', 'dataset': 'ob_fixture.customers',
                                            'column': 'name', 'pii': 'personal'}])
        P.save_pii_state(self.out, {'protocol': P.PROTOCOL_PII, 'passes': [
            {'pass': 1, 'bundle': 'bundle-static-v1', 'hidden': [], 'build_id': 'b_1', 'overlay_sha256': 'f' * 64,
             'api_restarted': False, 's5': {}},
            {'pass': 2, 'bundle': 'bundle-static-v1-pii2', 'hidden': ['ob_fixture.customers.name'], 'build_id': None}]})
        calls, build = self.fake_builds([[name_pii]])
        with mock.patch.object(P, 'model_semantics', return_value={}), \
                mock.patch.object(P, 'run_build', side_effect=build):
            result = P.handler_pii_rebuild(self.ctx)
        self.assertEqual(calls, [('bundle-static-v1-pii2', ['ob_fixture.customers.name'])])
        self.assertEqual(result['final_build_id'], 'b_2')
        self.assertEqual(len(P.load_pii_state(self.out)['passes']), 2)

    def test_pii_rebuild_refuses_without_a_recorded_first_build(self):
        with self.assertRaises(P.Refused) as cm:
            P.handler_pii_rebuild(self.ctx)
        self.assertEqual(cm.exception.code, 'pii_state_missing')

    def test_first_build_records_pass_one_and_applies_no_post_build_rewrite(self):
        build = {'build_id': 'b_1', 'overlay_sha256': 'f' * 64, 'bundle': 'bundle-static-v1', 'hidden': [],
                 'api_restarted': False, 's5': {'llm_calls': 1}}
        with mock.patch.object(P, 'run_build', return_value=build) as run, \
                mock.patch.object(P, 'ensure_bundle_complete') as rewrite:
            data = P.handler_s0_s8(self.ctx)
        self.assertEqual(run.call_args[0][1], 'bundle-static-v1')
        rewrite.assert_not_called()
        self.assertEqual(data['pass'], 1)
        self.assertEqual(P.load_pii_state(self.out)['passes'][0]['build_id'], 'b_1')


class ReleaseControlTests(unittest.TestCase):
    def setUp(self):
        self.tmp = private_tmp()
        self.site, self.run, self.out = make_run(self.tmp)
        self.site['product'] = {'network': 'net-test', 'cache_dir': str(self.tmp / 'cache'), 'image': 'img@sha',
                                'standby_ca': '/x/ca.crt'}
        self.site['target'] = {'network': 'net-test', 'pg_alias': 'pg-alias', 'admin_user': 'oo_admin'}
        self.site['standby_container'] = 'standby-test'
        self.site['binaries'] = {'release_control_api': {'path': '/tmp/oo-mysql-connection-whole/rc', 'sha256': 'x'}}
        (self.out / 'private').mkdir(mode=0o700, exist_ok=True)
        write_json_file(self.out / 'private' / 'discovery.json', {'tables': ['ob_fixture.customers', 'ob_fixture.orders']})
        (self.tmp / 'cache').mkdir(mode=0o700)

    def test_release_install_renders_a_complete_config_and_runs_official_install(self):
        calls = []

        def fake_run(out, label, argv, **kw):
            calls.append((label, argv))
            return 0, b''

        ctx = P.Ctx.__new__(P.Ctx)
        ctx.out, ctx.run, ctx.site = self.out, self.run, self.site
        with mock.patch.object(P, 'resolve_alias', return_value='pg-container'), \
                mock.patch.object(P, 'ensure_new_database') as ensure, \
                mock.patch.object(P, 'dexec') as dexec, \
                mock.patch.object(P, 'run_cmd', side_effect=fake_run), \
                mock.patch.object(P, 'free_ports', return_value=[19362]), \
                mock.patch.object(P, 'expand', side_effect=lambda p: str(p).replace('~', str(self.tmp))):
            result = P.install_release_control(ctx, 19447, '/owned/provision/%s' % self.run['run_id'])
        db = self.run['names']['db_prefix'] + 'release_control_v1'
        ensure.assert_called_once()
        self.assertEqual(ensure.call_args[0][3], db)
        self.assertEqual(ensure.call_args[0][4], 'oo_release_api_admin')
        self.assertIn('GRANT CONNECT,CREATE ON DATABASE %s TO oo_release_admin' % db, dexec.call_args[1]['stdin'].decode())
        runtime = json.loads((self.tmp / 'secret-root' / 'provision' / self.run['run_id'] / 'release' /
                              'runtime.json').read_text())
        self.assertEqual(runtime['api_pg']['database'], db)
        self.assertEqual(runtime['listen_port'], 19362)
        self.assertEqual(runtime['identity_origin'], 'https://127.0.0.1:19447')
        self.assertEqual(runtime['target_pg']['database'], self.run['names']['db_prefix'] + 'target_v1')
        self.assertEqual(runtime['target_relation'], 'customers')
        self.assertEqual(runtime['target_role'], self.run['names']['role'])
        self.assertEqual(runtime['pack_dir'], '/owned/provision/%s/release/control/packs' % self.run['run_id'])
        self.assertIsNone(re.search(r'"\{[A-Z_]+\}"', json.dumps(runtime)))
        container = self.run['names']['container_prefix'] + '-release'
        self.assertEqual(result['container'], container)
        self.assertEqual(result['origin'], 'https://127.0.0.1:19362')
        install = [argv for label, argv in calls if label == 'release-install'][0]
        self.assertEqual(install, ['docker', 'exec', container, '/run-current/rc', 'install', '--config',
                                   '/owned/provision/%s/release/runtime.json' % self.run['run_id']])
        create = [argv for label, argv in calls if label == 'release-create'][0]
        self.assertIn('--network', create)
        self.assertEqual(create[create.index('--network') + 1], 'net-test')
        self.assertIn('127.0.0.1:19362:19362', create)


class PublishTests(unittest.TestCase):
    """follow launch, operator contents, error codes, and the verification phases (no Docker)."""

    def setUp(self):
        self.tmp = private_tmp()
        self.site, self.run, self.out = make_run(self.tmp)
        rid = self.run['run_id']
        self.shared = self.tmp / 'shared'
        self.shared.mkdir(mode=0o700)
        self.publish_dir = self.shared / rid
        self.publish_dir.mkdir(mode=0o700)
        self.site['secret_root'] = str(self.tmp / 'secret-root')
        self.site['runtime_container'] = 'runtime-test'
        self.site['follow_deadline_seconds'] = 1
        self.site['standby_container'] = 'standby-test'
        self.site['target'] = {'network': 'net-test', 'pg_alias': 'pg-alias', 'admin_user': 'oo_admin'}
        self.site['product'] = {'network': 'net-test', 'cache_dir': str(self.tmp / 'cache'), 'image': 'img@sha',
                                'standby_ca': str(self.tmp / 'standby-ca.crt'), 'token_seed': str(self.tmp / 'seed'),
                                'pg_client_image': 'postgres:17', 'gateway_networks': ['net-test', 'net-two'],
                                'fixed_bin_root': str(self.tmp / 'fixed'), 'source_container': 'src',
                                'source_client_file': '/run/secrets/x.cnf'}
        self.site['binaries'] = {name: {'path': '/tmp/oo-mysql-connection-whole/bin/%s' % name, 'sha256': 'x'}
                                 for name in ('source_onboarding', 'selfbuild_control_api', 'cli',
                                              'release_coordinator', 'provision_mapping_reader', 'gateway',
                                              'issue_token', 'release_control_api')}
        self.site['binaries']['cli']['path'] = '/tmp/oo-mysql-connection-whole/gw/fb8/oo'
        (self.tmp / 'standby-ca.crt').write_text('ca')
        write_file(self.tmp / 'token-admin-source', 'SHARED-TOKEN-ADMIN-VALUE\n', 0o600)
        self.site['product']['token_admin_source'] = str(self.tmp / 'token-admin-source')
        (self.tmp / 'seed').mkdir(mode=0o700)
        for name in ('ca.crt', 'runtime.pass', 'endpoint.json'):
            write_file(self.tmp / 'seed' / name, 'x', 0o600)
        write_file(self.tmp / 'seed' / 'config.json', json.dumps({'data_user': 'old', 'audience': 'a'}), 0o600)
        write_file(self.tmp / 'secret-root' / 'provision' / rid / 'gateway-reader.pass', 'R' * 64 + '\n', 0o600)
        bundle = self.tmp / 'bundle'
        bundle.mkdir(mode=0o700)
        write_json_file(bundle / 'catalog.json', {'entries': []})
        (self.out / 'product').mkdir(mode=0o700, exist_ok=True)
        write_json_file(self.out / 'product' / 'product-receipt.json', {'build_id': 'b_x'})
        pass_roles = self.tmp / 'secret-root' / 'provision' / rid / 'bundle-static-v1-pii2'
        pass_roles.mkdir(mode=0o700, parents=True)
        write_json_file(pass_roles / 'roles.json', {'roles': {self.run['names']['role']: [
            'ob_fixture.customers.id', 'ob_fixture.customers.region', 'ob_fixture.orders.id']}})
        P.save_pii_state(self.out, {'protocol': P.PROTOCOL_PII, 'passes': [
            {'pass': 1, 'bundle': 'bundle-static-v1', 'hidden': [], 'build_id': 'b_x'},
            {'pass': 2, 'bundle': 'bundle-static-v1-pii2', 'hidden': ['ob_fixture.customers.name'], 'build_id': 'b_y'}]})
        write_json_file(self.out / 'private' / 'operator-capture.json',
                        {'onboarding': {'prepared_root': str(self.tmp / 'prep'), 'product': {}},
                         'database': self.run['source']['database'], 'user': 'oo_cdc',
                         'secret_ref': 'secret://owned-test-ref',
                         'password_file': str(self.tmp / 'secret-root' / 'provision' / rid / 'password.pass')})
        phases = {'product_runtime': {'api_container': 'api-c', 'release': {
                      'container': 'rel-c', 'origin': 'https://127.0.0.1:19362', 'private': str(self.tmp / 'rp'),
                      'runtime': str(self.tmp / 'rr.json'), 'binary_container': '/run-current/rc'}},
                  'artifacts': {'bundle': str(bundle), 'rows_by_table': {'ob_fixture.customers': 2000,
                                                                       'ob_fixture.orders': 3000},
                                'source_id': 'mysql_x'},
                  's0_s8': {'build_id': 'b_x'}}
        for name, data in phases.items():
            write_json_file(self.out / 'phases' / (name + '.json'), {'phase': name, 'data': data})
        self.patches = [mock.patch.object(P, 'SHARED_PROVISION', self.shared)]
        for patch in self.patches:
            patch.start()
        self.addCleanup(lambda: [p.stop() for p in self.patches])
        self.run['profile'] = {'user': 'oo_profile', 'secret_ref': 'secret://owned-test-profile'}
        self.ctx = P.Ctx.__new__(P.Ctx)
        self.ctx.out, self.ctx.run, self.ctx.site = self.out, self.run, self.site

    def operator_doc(self):
        return json.loads((self.out / 'private' / 'operator-static-ready.json').read_text())

    def test_preflight_uses_a_visible_non_id_column_with_a_row_limit(self):
        bundle = P.pass_bundle_host(self.ctx, 'bundle-static-v1-pii2')
        write_json_file(bundle / 'roles.json', {'roles': {self.run['names']['role']: [
            'bench_logistics.shipments.status', 'bench_logistics.shipments.shipment_id']}})
        self.assertEqual(P.preflight_query(self.ctx, 'bench_logistics'),
                         'SELECT shipment_id FROM bench_logistics.shipments LIMIT 1')

    def test_preflight_refuses_columns_from_a_different_schema(self):
        with self.assertRaises(P.Refused) as cm:
            P.preflight_query(self.ctx, 'bench_logistics')
        self.assertEqual(cm.exception.code, 'preflight_query_missing')

    def test_preflight_does_not_interpolate_unsafe_fallback_identifiers(self):
        bundle = P.pass_bundle_host(self.ctx, 'bundle-static-v1-pii2')
        for column in ['bench_logistics.shipments.bad-name',
                       'bench_logistics.bad-table.shipment_id',
                       'bench_logistics.shipments.extra.shipment_id']:
            with self.subTest(column=column):
                write_json_file(bundle / 'roles.json',
                                {'roles': {self.run['names']['role']: [column]}})
                with self.assertRaises(P.Refused) as cm:
                    P.preflight_query(self.ctx, 'bench_logistics')
                self.assertEqual(cm.exception.code, 'preflight_query_missing')

    def test_static_operator_keeps_the_cdc_binding_and_adds_profile_access(self):
        with mock.patch.object(P, 'container_binary', side_effect=lambda s, n: '/run-current/' + n):
            op = json.loads(P.build_static_operator(self.ctx).read_text())
        # The six-field binding is the CDC account, exactly as capture admitted it.
        self.assertEqual(op['user'], 'oo_cdc')
        self.assertEqual(op['secret_ref'], 'secret://owned-test-ref')
        self.assertTrue(op['password_file'].endswith('/provision/%s/password.pass' % self.run['run_id']))
        self.assertEqual(op['database'], 'ob_fixture')
        product = op['onboarding']['product']
        self.assertEqual(product['source_access'], 'network')
        self.assertEqual(product['profile_access'], {'user': 'oo_profile', 'secret_ref': 'secret://owned-test-profile'})
        self.assertEqual(set(product['profile_access']), {'user', 'secret_ref'})

    def test_static_operator_without_profile_credential_is_refused(self):
        self.run['profile'] = None
        with self.assertRaises(P.Refused) as cm:
            P.build_static_operator(self.ctx)
        self.assertEqual(cm.exception.code, 'profile_credential_required')

    def test_publish_retry_archives_the_follow_rust_bundle_copies_with_the_attempt(self):
        secret = Path(P.expand(self.site['secret_root'])) / 'provision' / self.run['run_id']
        secret.mkdir(mode=0o700, parents=True, exist_ok=True)
        (secret / 'rust-bundle-g1-abc').mkdir(mode=0o700)
        write_file(secret / 'rust-bundle-g1-abc' / 'roles.json', '{}', 0o600)
        write_file(secret / 'rust-bundle-g1-abc.runtime.json', '{}', 0o600)
        shared = self.shared
        (shared / 'follow').mkdir(mode=0o700)
        log = shared / 'publish'
        log.with_name('publish.exit').write_text('1')
        P.archive_publish_attempt(shared, log, secret)
        self.assertFalse((secret / 'rust-bundle-g1-abc').exists())
        self.assertTrue((secret / 'publish-attempt-1' / 'rust-bundle-g1-abc' / 'roles.json').is_file())
        self.assertTrue((secret / 'publish-attempt-1' / 'rust-bundle-g1-abc.runtime.json').is_file())
        self.assertTrue((shared / 'follow-attempt-1').is_dir())

    def test_token_admin_is_the_shared_store_credential_copied_not_generated(self):
        self.assertEqual(P.token_admin_credential(self.site), b'SHARED-TOKEN-ADMIN-VALUE\n')
        write_file(self.tmp / 'token-admin-open', 'x\n', 0o644)
        self.site['product']['token_admin_source'] = str(self.tmp / 'token-admin-open')
        with self.assertRaises(P.Refused):
            P.token_admin_credential(self.site)

    def test_static_operator_without_pass_state_is_refused(self):
        (self.out / 'private' / 'pii-rebuild.json').unlink()
        with mock.patch.object(P, 'container_binary', side_effect=lambda s, n: '/run-current/' + n):
            with self.assertRaises(P.Refused) as cm:
                P.operator_product(self.ctx)
        self.assertEqual(cm.exception.code, 'pii_state_missing')

    def test_static_operator_has_follow_product_block_and_no_secret_values(self):
        with mock.patch.object(P, 'container_binary', side_effect=lambda s, n: '/run-current/' + n):
            path = P.build_static_operator(self.ctx)
        op = json.loads(path.read_text())
        product = op['onboarding']['product']
        self.assertTrue(product['seed_bundle'].endswith('/bundle-static-v1-pii2'))
        self.assertEqual(product['preflight_sql'], ['SELECT id FROM ob_fixture.customers WHERE id = 1'])
        self.assertNotIn('name', product['preflight_sql'][0])
        for key in ('api_binary', 'api_container', 'api_runtime', 'bundle_parent', 'cli_binary',
                    'coordinator_binary', 'coordinator_config_container', 'fixed_bin_root', 'gateway_binary',
                    'gateway_container_prefix', 'gateway_data_ca', 'gateway_data_endpoint', 'gateway_image',
                    'gateway_login', 'gateway_networks', 'mapping_binary', 'mapping_template', 'preflight_sql',
                    'previous_gateway', 'product_config', 'release_binary', 'release_binary_container',
                    'release_container', 'release_origin', 'release_private', 'release_runtime', 'reuse_product',
                    'reuse_receipt', 'seed_bundle', 'token_admin_password_file', 'token_binary'):
            self.assertIn(key, product)
        self.assertEqual(op['onboarding']['after_generation'], 0)
        self.assertEqual(product['reuse_receipt'], P.digest_file(self.out / 'product' / 'product-receipt.json'))
        self.assertEqual(product['gateway_networks'], ['net-test', 'net-two'])
        self.assertEqual(product['gateway_data_endpoint']['host'], 'standby-test')
        self.assertNotIn('R' * 64, path.read_text())

    def test_write_release_helpers_points_reader_at_the_scope_reader_only(self):
        P.write_release_helpers(self.ctx)
        run_secret = self.tmp / 'secret-root' / 'provision' / self.run['run_id']
        seed = json.loads((run_secret / 'gateway-seed' / 'config.json').read_text())
        self.assertEqual(seed['data_user'], self.run['names']['reader_login'])
        coord = json.loads((run_secret / 'release' / 'target-coordinator.json').read_text())
        self.assertEqual(coord['reader_password_file'], '/secrets/provision/%s/gateway-reader.pass' % self.run['run_id'])
        self.assertTrue((run_secret / 'gateway-seed' / 'data.pass').is_file())
        native = json.loads((run_secret / 'gateway-seed' / 'reviewed' / 'mysql_full' / 'native-input.json').read_text())
        self.assertEqual(native['catalog'], {'entries': []})

    def test_follow_failure_is_reported_as_a_stable_code_not_raw_text(self):
        def fake_run(out, label, argv, **kw):
            if label == 'follow-launch':
                (self.publish_dir / 'publish.stderr').write_text('source-onboarding: something\nowned_fixture_only\n')
                (self.publish_dir / 'publish.exit').write_text('1\n')
            return 0, b''

        with mock.patch.object(P, 'run_cmd', side_effect=fake_run), \
                mock.patch.object(P, 'container_binary', side_effect=lambda s, n: '/run-current/' + n):
            with self.assertRaises(P.Refused) as cm:
                P.handler_publish(self.ctx)
        self.assertEqual(cm.exception.code, 'follow_failed:owned_fixture_only')

    def test_follow_success_requires_a_ready_gateway_record_for_this_role(self):
        ready = {'ready': True, 'readonly_replica': True, 'role': 'oo_r_other', 'gateway_container': 'g',
                 'private_root': str(self.tmp), 'pack_digest': 'p', 'source_generation': 1, 'token_sha256': 'h'}

        def fake_run(out, label, argv, **kw):
            if label == 'follow-launch':
                # follow writes its ready record under <--out>/follow/ while it runs
                (self.publish_dir / 'follow' / 'follow').mkdir(parents=True, exist_ok=True)
                write_json_file(self.publish_dir / 'follow' / 'follow' / 'gateway-ready.json', ready)
                (self.publish_dir / 'publish.exit').write_text('0\n')
            return 0, b''

        with mock.patch.object(P, 'run_cmd', side_effect=fake_run), \
                mock.patch.object(P, 'container_binary', side_effect=lambda s, n: '/run-current/' + n):
            with self.assertRaises(P.Refused) as cm:
                P.handler_publish(self.ctx)
        self.assertEqual(cm.exception.code, 'gateway_not_ready')

    def test_replica_check_requires_standby_in_recovery_read_only_and_reader_scoped(self):
        network_json = json.dumps({'net-test': {}}).encode()
        login = self.run['names']['reader_login']
        for observed, code in (('t|on|%s|t|t' % login, None), ('f|on|%s|t|t' % login, 'replica_check_failed')):
            def fake_run(out, label, argv, observed=observed, **kw):
                if label == 'standby-inspect':
                    return 0, network_json
                return 0, (observed + '\n').encode()
            with self.subTest(observed=observed), mock.patch.object(P, 'run_cmd', side_effect=fake_run):
                if code is None:
                    result = P.handler_replica_check(self.ctx)
                    self.assertEqual(result['result'], 't|on|%s|t|t' % login)
                    self.assertIn('in_recovery', result['checks'])
                else:
                    with self.assertRaises(P.Refused) as cm:
                        P.handler_replica_check(self.ctx)
                    self.assertEqual(cm.exception.code, code)

    def write_phase(self, name, data):
        write_json_file(self.out / 'phases' / (name + '.json'), {'phase': name, 'data': data})

    def test_gateway_requires_exactly_fixed_and_run_current_mounts(self):
        root = self.tmp / 'gw'
        root.mkdir(mode=0o700)
        write_file(root / 'listen.addr', '127.0.0.1:20001\n')
        self.write_phase('publish', {'gateway_container': 'g', 'private_root': str(root)})
        good = [{'State': {'Running': True}, 'Mounts': [{'Destination': '/fixed'}, {'Destination': '/run-current'}],
                 'NetworkSettings': {'Networks': {'net-test': {}, 'net-two': {}}}}]
        bad = [dict(good[0], Mounts=good[0]['Mounts'] + [{'Destination': '/extra'}])]
        for doc, code in ((good, None), (bad, 'gateway_mounts')):
            with self.subTest(code=code), mock.patch.object(P, 'run_cmd', return_value=(0, json.dumps(doc).encode())):
                if code is None:
                    self.assertEqual(P.handler_gateway(self.ctx)['listen_addr'], '127.0.0.1:20001')
                else:
                    with self.assertRaises(P.Refused) as cm:
                        P.handler_gateway(self.ctx)
                    self.assertEqual(cm.exception.code, code)

    def test_token_must_match_ready_hash_and_ttl(self):
        root = self.tmp / 'gw'
        root.mkdir(mode=0o700, exist_ok=True)
        write_file(root / 'current-client.token', 'tok', 0o600)
        write_file(root / 'grant.json', json.dumps({'client_ttl_ms': 3600000, 'role': 'oo_r_x'}), 0o600)
        sha = P.digest_file(root / 'current-client.token')
        self.write_phase('publish', {'private_root': str(root), 'token_sha256': sha})
        self.assertEqual(P.handler_token(self.ctx)['client_ttl_ms'], 3600000)
        self.write_phase('publish', {'private_root': str(root), 'token_sha256': 'f' * 64})
        with self.assertRaises(P.Refused) as cm:
            P.handler_token(self.ctx)
        self.assertEqual(cm.exception.code, 'token_hash_mismatch')

    def test_smoke_checks_tables_counts_and_refused_withheld_columns(self):
        write_json_file(self.out / 'private' / 'withheld.json', {'withheld': ['ob_fixture.customers.email']})
        self.write_phase('publish', {'gateway_container': 'g', 'private_root': str(self.tmp)})
        self.write_phase('gateway', {'listen_addr': '127.0.0.1:20001'})
        self.write_phase('token', {'token_file_name': 'current-client.token'})
        answers = {'freshness': {'publication': 'live', 'sources': [{'source': 's'}]},
                   'tables': {'tables': [{'name': 'ob_fixture.customers'}, {'name': 'ob_fixture.orders'}]}}

        def fake_run(out, label, argv, **kw):
            if label == 'smoke-freshness':
                return 0, json.dumps(answers['freshness']).encode()
            if label == 'smoke-tables':
                return 0, json.dumps(answers['tables']).encode()
            if label.startswith('smoke-count-'):
                n = '2000' if 'customers' in label else '3000'
                return 0, json.dumps({'rows': [[n]]}).encode()
            if label.startswith('smoke-withheld-'):
                return 1, b'refused'
            raise AssertionError(label)

        with mock.patch.object(P, 'run_cmd', side_effect=fake_run):
            result = P.handler_smoke(self.ctx)
        self.assertEqual(result['counts'], {'ob_fixture.customers': 2000, 'ob_fixture.orders': 3000})
        self.assertEqual(result['withheld_refused'], ['ob_fixture.customers.email'])
        self.assertEqual(result['rejected_refused'], [])

        def wrong_count(out, label, argv, **kw):
            if label.startswith('smoke-count-'):
                return 0, json.dumps({'rows': [['1']]}).encode()
            return fake_run(out, label, argv, **kw)

        with mock.patch.object(P, 'run_cmd', side_effect=wrong_count):
            with self.assertRaises(P.Refused) as cm:
                P.handler_smoke(self.ctx)
        self.assertEqual(cm.exception.code, 'smoke_count')

    def test_smoke_also_requires_rejected_columns_to_be_refused(self):
        write_json_file(self.out / 'private' / 'withheld.json', {'withheld': []})
        write_json_file(self.out / 'review' / 'decisions.json', {'reviews': [
            {'name': 'ob_fixture.customers.name', 'binds_fact': 'a' * 64, 'decision': 'rejected'},
            {'name': 'ob_fixture.orders.id', 'binds_fact': 'b' * 64, 'decision': 'confirmed'}], 'confirm_columns': []})
        self.write_phase('publish', {'gateway_container': 'g', 'private_root': str(self.tmp)})
        self.write_phase('gateway', {'listen_addr': '127.0.0.1:20001'})
        self.write_phase('token', {'token_file_name': 'current-client.token'})
        ran = []

        def fake_run(out, label, argv, **kw):
            ran.append(label)
            if label == 'smoke-freshness':
                return 0, json.dumps({'publication': 'live', 'sources': [{'source': 's'}]}).encode()
            if label == 'smoke-tables':
                return 0, json.dumps({'tables': [{'name': 'ob_fixture.customers'}, {'name': 'ob_fixture.orders'}]}).encode()
            if label.startswith('smoke-count-'):
                return 0, json.dumps({'rows': [['2000' if 'customers' in label else '3000']]}).encode()
            if label == 'smoke-withheld-name':
                return 1, b'column not visible'
            raise AssertionError(label)

        with mock.patch.object(P, 'run_cmd', side_effect=fake_run):
            result = P.handler_smoke(self.ctx)
        self.assertIn('smoke-withheld-name', ran)
        self.assertNotIn('smoke-withheld-id', ran)
        self.assertEqual(result['rejected_refused'], ['ob_fixture.customers.name'])
        self.assertEqual(result['withheld_refused'], [])

    def test_ready_writes_contract_scope_record_and_alias(self):
        self.write_phase('publish', {'gateway_container': 'g', 'private_root': str(self.tmp)})
        self.write_phase('token', {'token_file_name': 'current-client.token'})
        scope = P.handler_ready(self.ctx)
        for key in ('source_id', 'label', 'input_path', 'operator_path', 'alias_path', 'ready_record', 'cli_binary',
                    'container', 'role', 'build_id', 'product_config', 'token_file_name'):
            self.assertIn(key, scope)
        self.assertEqual(scope['container'], 'g')
        self.assertTrue(scope['ready_record'].endswith('rust-current-gateway.json'))
        alias = json.loads((self.out / 'oo-current.json').read_text())
        self.assertEqual({alias['ready_record'], alias['cli_binary']}, {scope['ready_record'], scope['cli_binary']})
        self.assertEqual(json.loads((self.out / 'scope.json').read_text()), scope)

    def test_eip_ready_exports_transport_without_changing_the_customer_input(self):
        self.write_phase('publish', {'gateway_container': 'g', 'private_root': str(self.tmp)})
        self.write_phase('token', {'token_file_name': 'current-client.token'})
        self.ctx.run['egress'] = 'eip'
        P.write_json(self.out / 'private' / 'producer-six.json',
                     dict(GOOD_SIX, host='host.docker.internal', port=40123))
        scope = P.handler_ready(self.ctx)
        self.assertEqual(scope['input_path'], str(self.out / 'six.json'))
        transport = json.loads(Path(scope['transport_input_path']).read_text())
        self.assertEqual(transport, dict(GOOD_SIX, host='host.docker.internal', port=40123))

    def test_follow_error_code_extracts_only_a_code(self):
        path = self.tmp / 'err.txt'
        write_file(path, 'something long\nowned_fixture_only\n')
        self.assertEqual(P.follow_error_code(path), 'owned_fixture_only')
        write_file(path, 'Secret pass Value-123 leaked here\n')
        self.assertEqual(P.follow_error_code(path), 'unknown')


    def test_follow_error_code_reads_the_source_onboarding_prefix_form(self):
        path = self.tmp / 'err.txt'
        write_file(path, 'source-onboarding: owned_fixture_only; private evidence retained\n')
        self.assertEqual(P.follow_error_code(path), 'owned_fixture_only')

    def test_follow_gets_the_same_six_field_input_as_capture(self):
        seen = []

        def fake_run(out, label, argv, **kw):
            if label == 'follow-launch':
                seen.append(argv)
                (self.publish_dir / 'publish.stderr').write_text('source-onboarding: owned_fixture_only\n')
                (self.publish_dir / 'publish.exit').write_text('2\n')
            return 0, b''

        with mock.patch.object(P, 'run_cmd', side_effect=fake_run), \
                mock.patch.object(P, 'container_binary', side_effect=lambda s, n: '/run-current/' + n):
            with self.assertRaises(P.Refused):
                P.handler_publish(self.ctx)
        argv = seen[0]
        self.assertEqual(argv[argv.index('--input') + 1], str(self.out / 'six.json'))
        self.assertEqual(json.loads((self.out / 'six.json').read_text())['user'], 'oo_cdc')

    def test_failed_publish_is_archived_and_can_be_retried(self):
        def fake_run(out, label, argv, **kw):
            if label == 'follow-launch':
                (self.publish_dir / 'publish.stderr').write_text('source-onboarding: owned_fixture_only; x\n')
                (self.publish_dir / 'publish.exit').write_text('2\n')
            return 0, b''

        with mock.patch.object(P, 'run_cmd', side_effect=fake_run), \
                mock.patch.object(P, 'container_binary', side_effect=lambda s, n: '/run-current/' + n):
            for _ in range(2):
                with self.assertRaises(P.Refused) as cm:
                    P.handler_publish(self.ctx)
                self.assertEqual(cm.exception.code, 'follow_failed:owned_fixture_only')
        self.assertTrue((self.publish_dir / 'publish-attempt-1.exit').is_file())
        self.assertTrue((self.publish_dir / 'publish-attempt-1.stderr').is_file())
        self.assertTrue((self.publish_dir / 'publish.exit').is_file())


V2_CHANGED_PINS = {'source_onboarding', 'producer', 'oo_pipeline', 'job_worker', 'oo_source_admission', 'selfbuild_control_api'}


class FormatAndRouteTests(unittest.TestCase):
    """--format chunked-typed/v2 (site profile + operator field) and --egress eip (producer endpoint, TLS name)."""

    def setUp(self):
        self.tmp = private_tmp()
        _, self.run, self.out = make_run(self.tmp)
        self.site = dict(P.load_site(), secret_root=str(self.tmp / 'secret-root'))
        self.site['product'] = dict(self.site['product'], token_admin_source=str(self.tmp / 'token-admin-source'))
        write_file(self.tmp / 'token-admin-source', 'SHARED-TOKEN\n', 0o600)
        (self.tmp / 'prep').mkdir(mode=0o700)

    def ctx_for(self, egress='direct', fmt='v1', forward_port=None):
        P.write_json(self.out / 'run.json', dict(self.run, egress=egress, format=fmt))
        if forward_port is not None:
            P.write_json(self.out / 'private' / 'eip-local-port.json', {'local_port': forward_port})
            P.write_json(self.out / 'private' / 'ssh-forward.json',
                         {'control': '/x', 'target': 'oo@h', 'pid': 1, 'local_port': forward_port})
        return P.Ctx(self.out, self.site)

    def operator(self, ctx):
        with mock.patch.object(P, 'prepared_root', return_value=self.tmp / 'prep'):
            path = P.build_operator(ctx, 'binding-uuid', 'project-uuid', 4242, {'tables': [], 'columns': {}})
        return json.loads(path.read_text(encoding='utf-8'))

    def test_v2_profile_replaces_only_the_v2_pins_and_v1_pins_stay(self):
        site = P.load_site()
        v2 = P.site_for_format(site, 'chunked-typed/v2')
        self.assertIs(P.site_for_format(site, 'v1'), site)
        changed = {name for name in site['binaries'] if v2['binaries'][name] != site['binaries'][name]}
        self.assertEqual(changed, V2_CHANGED_PINS)
        self.assertEqual(v2['binaries']['producer']['sha256'],
                         'd1eee194c5393e473c7da638678239c6ce5330e25da4e45ac7122fd4013c280d')
        self.assertEqual(v2['binaries']['source_onboarding']['sha256'],
                         'eeb19cf0aa4269d0799fa43c906e56765e5d325030142feb217d3feffd4b6cd7')
        self.assertEqual(site['binaries']['producer']['sha256'],
                         'ff1e7f801bb8da6ae611633b888919658f30b034cb8cde20fa6887a36101abf3')
        self.assertEqual(site['binaries']['source_onboarding']['sha256'],
                         '362c2f0c5dd71dcf770e26c7444af19f9fc29db83472c52533686b9d3f8138ca')
        self.assertEqual(site['binaries']['oo_source_admission']['sha256'],
                         '85cf8ad37a10ec91113dde35b1a51702fda90b1a65041df709dce89aab0ae480')

    def test_v2_installs_current_jobs_expansions_without_changing_v1_deployment(self):
        for fmt, expected in [
                ('v1', self.site['target']['deploy_script']),
                ('chunked-typed/v2', str(P.HERE.parents[1] / 'crates/oo-jobs/tools/deploy.py'))]:
            with self.subTest(fmt=fmt):
                ctx = self.ctx_for(fmt=fmt)
                with mock.patch.object(P, 'resolve_alias', return_value='fixture-pg'), \
                        mock.patch.object(P, 'ensure_new_database'), \
                        mock.patch.object(P, 'run_cmd', side_effect=P.Refused('stop_test')) as run:
                    with self.assertRaises(P.Refused):
                        P.handler_target(ctx)
                self.assertEqual(run.call_args.args[1], 'jobs-deploy')
                self.assertEqual(run.call_args.args[2][1], expected)

    def test_missing_profile_is_format_unavailable(self):
        with mock.patch.dict(P.FORMAT_PROFILES, {'chunked-typed/v2': self.tmp / 'missing.json'}):
            with self.assertRaises(P.Refused) as cm:
                P.site_for_format(P.load_site(), 'chunked-typed/v2')
        self.assertEqual(cm.exception.code, 'format_unavailable')

    def test_profile_naming_an_unknown_pin_or_another_format_is_refused(self):
        entry = {'path': '/tmp/x', 'sha256': 'ab' * 32}
        cases = [{'protocol': P.PROTOCOL_PROFILE, 'format': 'chunked-typed/v2', 'binaries': {'not_a_pin': entry}},
                 {'protocol': P.PROTOCOL_PROFILE, 'format': 'v9', 'binaries': {}},
                 {'protocol': 'other', 'format': 'chunked-typed/v2', 'binaries': {}}]
        for doc in cases:
            with self.subTest(doc=doc):
                path = write_file(self.tmp / 'profile.json', json.dumps(doc), 0o600)
                with mock.patch.dict(P.FORMAT_PROFILES, {'chunked-typed/v2': path}):
                    with self.assertRaises(P.Refused) as cm:
                        P.site_for_format(P.load_site(), 'chunked-typed/v2')
                self.assertEqual(cm.exception.code, 'site_config_invalid')

    def test_cmd_start_refuses_v2_without_profile_before_creating_run(self):
        six = write_file(self.tmp / 'six-start.json', json.dumps(GOOD_SIX), 0o600)
        pw = write_file(self.tmp / 'pw-start', PASSWORD + b'\n', 0o600)
        args = argparse.Namespace(input=str(six), password_file=str(pw), out=str(self.tmp / 'RUN-V2'),
                                  format='chunked-typed/v2', egress=None, read_replica=None, label=None,
                                  profile_input=None, profile_password_file=None, ca_file=None,
                                  discover_endpoint=None)
        with mock.patch.dict(P.FORMAT_PROFILES, {'chunked-typed/v2': self.tmp / 'missing.json'}):
            with self.assertRaises(P.Refused) as cm:
                P.cmd_start(args)
        self.assertEqual(cm.exception.code, 'format_unavailable')
        self.assertFalse((self.tmp / 'RUN-V2').exists())

    def test_cmd_start_refuses_discover_endpoint_under_eip(self):
        six = write_file(self.tmp / 'six-eip.json', json.dumps(GOOD_SIX), 0o600)
        pw = write_file(self.tmp / 'pw-eip', PASSWORD + b'\n', 0o600)
        args = argparse.Namespace(input=str(six), password_file=str(pw), out=str(self.tmp / 'RUN-EIP'),
                                  format=None, egress='eip', read_replica=None, label=None,
                                  profile_input=None, profile_password_file=None, ca_file=None,
                                  discover_endpoint='127.0.0.1:3306')
        with self.assertRaises(P.Refused) as cm:
            P.cmd_start(args)
        self.assertEqual(cm.exception.code, 'egress_invalid')
        self.assertFalse((self.tmp / 'RUN-EIP').exists())

    def test_v1_direct_operator_is_unchanged(self):
        op = self.operator(self.ctx_for())
        self.assertEqual((op['host'], op['port']), ('fixture.internal', 3306))
        self.assertNotIn('tls_server_name', op)
        self.assertNotIn('c1_baseline_format', op['onboarding'])
        self.assertNotIn('child_deadlines_ms', op['onboarding'])
        self.assertNotIn('child_deadline_ms', op['onboarding'])
        self.assertTrue(op['onboarding']['producer'].endswith('/ff1e7f801bb8da6ae611633b888919658f30b034cb8cde20fa6887a36101abf3/mysql-preview'))
        self.assertTrue(op['onboarding']['admission'].endswith('/85cf8ad37a10ec91113dde35b1a51702fda90b1a65041df709dce89aab0ae480/oo-source-admission'))

    def test_v2_operator_carries_the_format_and_the_v2_pins(self):
        op = self.operator(self.ctx_for(fmt='chunked-typed/v2'))
        self.assertEqual(op['onboarding']['c1_baseline_format'], 'chunked-typed/v2')
        self.assertEqual(op['onboarding']['child_deadlines_ms'], {'native-step': 21600000})
        self.assertTrue(op['onboarding']['producer'].endswith('/d1eee194c5393e473c7da638678239c6ce5330e25da4e45ac7122fd4013c280d/mysql-preview'))
        self.assertEqual(op['onboarding']['loader'],
                         P.site_for_format(self.site, 'chunked-typed/v2')['binaries']['oo_pipeline']['path'])
        self.assertTrue(op['onboarding']['admission'].endswith('/40f52d923aab6a75d67ad0d796320614d98efd390dbf453d75d557ba40cd6982/oo-source-admission'))
        self.assertNotIn('tls_server_name', op)

    def test_v2_artifact_request_names_each_tables_schema_digest(self):
        request = {'source_id': 'mysql_t', 'source_generation': 1,
                   'target': {'database': 'd', 'owner': 'o', 'shard': 'x', 'host': 'h'},
                   'baseline_scope': {'config_digest': 'c' * 64, 'schema_digest': 'a' * 64, 'l_end': 'u:1',
                                      'tables': [{'schema': 'ob_fixture', 'table': 'orders',
                                                  'schema_sha256': 'b' * 64}]},
                   'tables': [{'schema': 'ob_fixture', 'table': 'orders', 'type_digest': 't' * 64,
                               'input_digest': 'i' * 64,
                               'admission': {'registry_path': '/r', 'registry_sha256': 'd' * 64}}],
                   'entries': [{'schema': 'ob_fixture', 'table': 'orders'}]}
        native = P.v2_artifact_request(request)['native_requests'][0]
        self.assertEqual(native['schema_digest'], 'b' * 64)
        self.assertEqual(native['config_digest'], 'c' * 64)
        self.assertEqual(native['type_digest'], 't' * 64)
        self.assertEqual(native['input_digest'], 'i' * 64)

    def test_eip_operator_routes_the_producer_through_the_forward(self):
        op = self.operator(self.ctx_for(egress='eip', forward_port=40123))
        self.assertEqual((op['host'], op['port']), ('host.docker.internal', 40123))
        self.assertEqual(op['tls_server_name'], 'fixture.internal')
        self.assertEqual((op['user'], op['database']), ('oo_cdc', 'ob_fixture'))
        self.assertNotIn('c1_baseline_format', op['onboarding'])

    def test_eip_producer_six_and_host_endpoints_follow_the_forward(self):
        ctx = self.ctx_for(egress='eip', forward_port=40123)
        path = P.producer_six_path(ctx)
        self.assertEqual(path, self.out / 'private' / 'producer-six.json')
        self.assertEqual(json.loads(path.read_text()), dict(GOOD_SIX, host='host.docker.internal', port=40123))
        self.assertEqual(P.host_endpoint(ctx, GOOD_SIX), ('127.0.0.1', 40123))
        self.assertEqual(stat.S_IMODE(os.stat(path).st_mode), 0o600)

    def test_direct_producer_six_and_host_endpoints_are_unchanged(self):
        ctx = self.ctx_for()
        self.assertEqual(P.producer_six_path(ctx), self.out / 'six.json')
        self.assertEqual(P.host_endpoint(ctx, GOOD_SIX), ('fixture.internal', 3306))

    def test_eip_without_a_recorded_forward_refuses(self):
        ctx = self.ctx_for(egress='eip')
        with self.assertRaises(P.Refused) as cm:
            P.producer_endpoint(ctx, dict(GOOD_SIX))
        self.assertEqual(cm.exception.code, 'ssh_forward_failed')

    def test_egress_eip_records_the_local_port_for_later_phases(self):
        ctx = self.ctx_for(egress='eip')
        ctx.site['egress']['producer_tls_server_name'] = True
        fake = mock.Mock(pid=4321)
        fake.poll.return_value = None
        forward = {'local_port': 40123, 'control': self.tmp / 'ctl', 'target': 'oo@h', 'proc': fake}
        with mock.patch.object(P, 'open_forward', return_value=forward), \
                mock.patch.object(P, 'wait_greeting', return_value='5.7.44-log'):
            result = P.egress_eip(ctx, dict(GOOD_SIX))
        self.assertEqual(result['local_port'], 40123)
        meta = json.loads((self.out / 'private' / 'ssh-forward.json').read_text())
        self.assertEqual(meta['local_port'], 40123)
        self.assertEqual(json.loads((self.out / 'private' / 'eip-local-port.json').read_text())['local_port'], 40123)

    def test_a_torn_down_eip_forward_is_reopened_on_the_same_port(self):
        ctx = self.ctx_for(egress='eip', forward_port=40123)
        (self.out / 'private' / 'ssh-forward.json').unlink()
        calls = []
        with mock.patch.object(P, 'egress_eip', side_effect=lambda c, six, local_port=None: calls.append(local_port)):
            self.assertEqual(P.eip_forward_port(ctx), 40123)
        self.assertEqual(calls, [40123])
        with mock.patch.object(P, 'egress_eip') as live:
            self.assertEqual(P.eip_forward_port(self.ctx_for(egress='eip', forward_port=40123)), 40123)
        live.assert_not_called()

    def test_open_forward_clears_a_stale_control_socket_before_starting_the_master(self):
        ctx = self.ctx_for(egress='eip')
        control = self.out / 'private' / 'ssh.sock'
        control.write_text('stale')
        seen = []

        def fake_popen(argv, **kwargs):
            seen.append(control.exists())
            proc = mock.Mock(pid=7)
            proc.poll.return_value = 1  # the master exits at once: the forward must be refused, not trusted
            return proc

        with mock.patch.object(P.subprocess, 'Popen', side_effect=fake_popen), \
                mock.patch.object(P, 'close_forward'):
            with self.assertRaises(P.Refused) as cm:
                P.open_forward(ctx, dict(GOOD_SIX), local_port=40123)
        self.assertEqual(cm.exception.code, 'ssh_forward_failed')
        self.assertEqual(seen, [False])


class ProfileTimeoutTests(unittest.TestCase):
    def test_profile_has_a_separate_budget_and_restores_the_socket_timeout(self):
        client = mock.Mock()
        client.sock.gettimeout.return_value = 30
        with mock.patch.object(P, '_profile_source', return_value={'ok': True}):
            self.assertEqual(P.profile_source(client, 's', 1, [], {}, None), {'ok': True})
        client.query.assert_called_once_with('SET SESSION max_execution_time=540000')
        self.assertEqual(client.sock.settimeout.call_args_list, [mock.call(540), mock.call(30)])

    def test_profile_restores_timeout_on_failure(self):
        client = mock.Mock()
        client.sock.gettimeout.return_value = 30
        with mock.patch.object(P, '_profile_source', side_effect=TimeoutError):
            with self.assertRaises(TimeoutError):
                P.profile_source(client, 's', 1, [], {}, None)
        self.assertEqual(client.sock.settimeout.call_args_list[-1], mock.call(30))


if __name__ == '__main__':
    unittest.main()
