import hashlib
import json
import unittest
from unittest.mock import patch

import collect
import report

ROOT = collect.ROOT
HASH = 'a' * 64
OTHER = 'b' * 64


class AdapterTests(unittest.TestCase):
    def text(self, path):
        return (ROOT / path).read_text()

    def test_direct_json_verdicts_and_hashes(self):
        data = {'runs': [{'version_actual': '21.3.0.0.0', 'run_id': 'r1',
                         'T1': 'passed', 'T8': 'failed', 'execution': 'emulated',
                         'artifacts_sha256': {'t1-t8.json': HASH, 'resource-receipt.json': OTHER}}]}
        rows = collect.json_suites('testing/engines/oracle/example.json', data, 'c' * 64)
        self.assertEqual([(r['suite'], r['result'], r['sha256']) for r in rows],
                         [('T1', 'pass', HASH), ('T8', 'fail', OTHER)])
        self.assertTrue(all(r['emulated'] for r in rows))
        self.assertEqual(rows[0]['version'], '21.3.0.0.0')

    def test_missing_explicit_fields_never_inherits_pass(self):
        for rec in ({'version': '21.3', 'status': 'passed', 'run_id': 'r'},
                    {'version': '21.3', 'T1': 'passed'},
                    {'run_id': 'r', 'T1': 'passed'},
                    {'version': '21.3', 'run_id': 'r', 'T5': {'tables': 1000}}):
            self.assertEqual(collect.json_suites('testing/engines/oracle/x.json',
                                                {'status': 'passed', 'runs': [rec]}, HASH), [])

    def test_oracle_component_downgrade(self):
        data = json.loads(self.text('testing/engines/oracle/evidence-summary.json'))
        rows = collect.json_suites('testing/engines/oracle/evidence-summary.json', data, HASH)
        self.assertTrue(rows)
        self.assertEqual({r['result'] for r in rows}, {'skip'})

    def test_oracle_t7_boolean_and_raw_hash(self):
        path = 'testing/engines/oracle/s2c-evidence-summary.json'
        data = json.loads(self.text(path))
        rows = collect.json_suites(path, data, HASH)
        self.assertEqual(rows[0]['sha256'], data['versions'][0]['evidence_sha256']['audit-result.json'])
        self.assertEqual(rows[0]['result'], 'fail')
        self.assertEqual(rows[0]['variant'], 'c5')
        self.assertFalse(any(r['suite'] == 'T2' for r in rows))

    def test_opengauss_nested_versions_do_not_promote_component_tests(self):
        path = 'testing/engines/opengauss/s4b-evidence-summary.json'
        data = json.loads(self.text(path))
        rows = collect.json_suites(path, data, HASH)
        self.assertEqual(len(rows), 6)
        self.assertEqual({r['result'] for r in rows}, {'skip'})
        self.assertEqual(rows[0]['sha256'], data['sha256']['d-s4b-ee-final3/5.0.3/T0.json'])

    def test_ssh_acceptance(self):
        path = 'crates/oo-connectors/tests/ssh/evidence-summary.json'
        rows = collect.json_suites(path, json.loads(self.text(path)), HASH)
        self.assertTrue(any(r['target'] == 'postgres-selfmanaged' and r['suite'] == 'T1' for r in rows))
        self.assertFalse(any(r['suite'] == 'T3' for r in rows))

    def test_explicit_rows_validation(self):
        r = collect.row('mysql', '5.7.44', 'T1', 'pass', 'r1', HASH)
        self.assertEqual(collect.explicit_rows([r]), [r])
        self.assertEqual(collect.explicit_rows([{'status': 'passed'}]), [])
        for change in ({'sha256': 'bad'}, {'result': 'unknown'}, {'emulated': 'false'}, {'suite': 'T9'}):
            with self.assertRaises(ValueError):
                collect.explicit_rows([dict(r, **change)])

    def test_pins_fixture_not_product_doctor(self):
        rows = collect.pins(self.text('tools/oo-test/PINS-VALIDATION.md'), HASH)
        self.assertEqual(len(rows), 11)
        self.assertEqual({r['result'] for r in rows}, {'skip'})
        self.assertEqual(sum(r['emulated'] for r in rows), 1)
        self.assertTrue(all(r['sha256'] != HASH for r in rows))

    def test_original_fixture_validation(self):
        rows = collect.fixture_t0(self.text('tools/oo-test/VALIDATION.md'), HASH)
        self.assertEqual(len(rows), 13)
        self.assertEqual(sum(r['emulated'] for r in rows), 3)
        self.assertEqual({r['result'] for r in rows}, {'skip'})
        self.assertTrue(all(r['sha256'] != HASH for r in rows))

    def test_mysql_t1_exact_versions(self):
        rows = collect.mysql_t1(self.text('tools/oo-test/VALIDATION-M-S1E.md'), HASH)
        self.assertEqual(len(rows), 8)
        self.assertEqual({r['suite'] for r in rows}, {'T1', 'T8'})
        self.assertTrue(all(r['sha256'] != HASH for r in rows))
        self.assertEqual(sum(r['emulated'] for r in rows), 2)

    def test_olap_variant_and_no_guess(self):
        text = self.text('testing/engines/olap/SUMMARY.md')
        rows = collect.olap(text, HASH)
        self.assertEqual(len(rows), 12)
        self.assertEqual({r['variant'] for r in rows if r['suite'] == 'T2'}, {'c5'})
        self.assertEqual(collect.olap(text.replace('passed', 'unknown'), HASH), [])

    def test_family_cycles_not_inferred_as_pass(self):
        rows = collect.family_acceptance(self.text('testing/engines/mariadb/C-S3c-SUMMARY.md'), HASH)
        self.assertEqual(len(rows), 21)
        self.assertFalse(any(r['suite'] == 'T2' for r in rows))
        self.assertEqual(rows[0]['target'], 'mariadb-lts')

    def test_source_adapter_tables(self):
        for path, count in [('testing/engines/mysql/SUMMARY.md', 4),
                            ('testing/engines/postgres/RESULTS.md', 12)]:
            rows = collect.adapter_tables(path, self.text(path), HASH)
            self.assertEqual(len(rows), count)
            self.assertTrue(all(r['result'] == 'skip' for r in rows if r['suite'] == 'T1'))

    def test_sqlserver_joins_verdicts_not_overall_author_complete(self):
        text = self.text('testing/engines/sqlserver/S3B-VALIDATION.md')
        data = json.loads(self.text('testing/engines/sqlserver/S3B-RESULTS.json'))
        rows = collect.sqlserver(text, HASH, data)
        self.assertEqual(len(rows), 18)
        self.assertTrue(all(r['emulated'] and r['sha256'] != HASH for r in rows))
        self.assertEqual(collect.sqlserver(text.replace('pass, run', 'unknown, run'), HASH, data), [])

    def test_s5_smoke_and_nightly_counts_not_passes(self):
        rows = collect.s5_validation(self.text('tools/oo-test/S5-VALIDATION.md'), HASH)
        self.assertEqual(len(rows), 1)
        self.assertEqual((rows[0]['version'], rows[0]['suite'], rows[0]['result']), ('17', 'T7', 'fail'))

    def test_only_committed_blobs_and_no_self_ingestion(self):
        names = '\0'.join(['execution/v0.2/evidence-index.json', 'testing/engines/x/evidence.json',
                           'tools/source-support/examples/evidence/manifest.json', ''])
        with patch('collect.subprocess.check_output', side_effect=[names.encode(), b'{}']) as git:
            self.assertEqual(list(collect.committed(ROOT)), [('testing/engines/x/evidence.json', b'{}')])
            self.assertEqual(git.call_args_list[-1].args[0], ['git', 'show', 'HEAD:testing/engines/x/evidence.json'])

    def test_summary_hash_fallback_and_repeatability(self):
        blob = json.dumps({'runs': [{'version': '21.3', 'run_id': 'r', 'T1': 'pass'}]}).encode()
        with patch('collect.committed', return_value=[('testing/engines/oracle/evidence.json', blob)]):
            a, inventory = collect.collect(ROOT)
            b, _ = collect.collect(ROOT)
        self.assertEqual(a, b)
        self.assertEqual(a[0]['sha256'], hashlib.sha256(blob).hexdigest())
        self.assertEqual(inventory[0]['rows'], 1)

    def test_conflicting_verdicts_fail_closed(self):
        record = {'version': '21.3', 'run_id': 'r', 'T1': 'pass'}
        blob = json.dumps({'runs': [record, dict(record, T1='fail')]}).encode()
        with patch('collect.committed', return_value=[('testing/engines/oracle/evidence.json', blob)]):
            with self.assertRaisesRegex(ValueError, 'conflicting verdicts'):
                collect.collect(ROOT)

    def test_report_does_not_transfer_patch_evidence(self):
        rows = [collect.row('postgres', '17.11', 'T1', 'pass', 'r', HASH),
                collect.row('postgres', '17.10', 'T0', 'skip', 'r', HASH)]
        text = report.report(rows, {'targets': []})
        self.assertIn('| postgres-selfmanaged | 17.10 | T1, T2, T3', text)
        self.assertIn('| postgres-selfmanaged | 17.11 | T0, T2, T3', text)
        self.assertIn('| oracle-19c-selfmanaged | 19c | T0, T1, T2', text)

    def test_live_collection_matches_generator_contract_and_target_ids(self):
        import importlib.util
        spec = importlib.util.spec_from_file_location('generator', ROOT / 'specs/scripts/gen_support_matrix.py')
        generator = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(generator)
        rows, inventory = collect.collect(ROOT)
        self.assertEqual(generator.validate(rows), [])
        ids = set(collect.re.findall(r'id: ([\w-]+)', self.text('docs/database-support/support-matrix.yaml')))
        self.assertTrue(all(r['target'] in ids for r in rows))
        self.assertFalse(any(r['suite'] in ('T3', 'T4', 'T5', 'T6') and r['result'] == 'pass' for r in rows))
        self.assertGreater(len(inventory), 50)


if __name__ == '__main__':
    unittest.main()
