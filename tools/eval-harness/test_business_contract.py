import copy
import hashlib
import unittest
from types import SimpleNamespace

from business_contract import digest
from harness import applicable_thresholds, evaluator_digest, score, summarize


class BusinessContractTests(unittest.TestCase):
    def test_business_gate_requires_all_answers_without_inventing_retrieval_or_refusal_cases(self):
        args = SimpleNamespace(answer_threshold=.8, table_threshold=.9, refusal_threshold=1.0)
        thresholds = applicable_thresholds([{'kind': 'business'}], args)
        self.assertEqual(thresholds, {'answer_accuracy': 1.0})
        question, run = self.fixture()
        results = [score(question, run)]
        self.assertTrue(all(summarize(results, thresholds)['gates'].values()))
        results.append(score({'kind': 'business'}, {}))
        self.assertFalse(all(summarize(results, thresholds)['gates'].values()))
        mixed = applicable_thresholds([{'kind': 'business'}, {'kind': 'refusal', 'expected_tables': ['a.b']}], args)
        self.assertEqual(set(mixed), {'answer_accuracy', 'table_recall', 'refusal_accuracy'})
        self.assertEqual(evaluator_digest(), evaluator_digest())
        self.assertEqual(len(evaluator_digest()), 64)

    def fixture(self):
        text = 'Reviewed fixture metric. Test evidence only.'
        context = {'metric': 'receipts', 'scope': {'tenant_id': 'fixture'},
                   'period': {'start': '2026-09-01', 'end': '2026-10-01'},
                   'unit': 'CNY', 'time_basis': 'paid_at', 'coverage': 'all approved channels'}
        rows = [['9007199254740993.01']]
        contract = {
            'approval': {'status': 'approved', 'reviewer': 'fixture/test', 'definition_sha256': 'a' * 64},
            'context': context,
            'oracle': {'kind': 'finance_report', 'reconciled': True, 'evidence_sha256': 'b' * 64,
                       'context_sha256': digest(context), 'rows_sha256': digest(rows)},
            'knowledge_files': {'metrics.md': hashlib.sha256(text.encode()).hexdigest()}}
        question = {'kind': 'business', 'expected_rows': rows, 'business_contract': contract}
        run = {'trace': [
            {'operation': 'freshness', 'output': {'publication': 'live', 'source_time': 'unmeasured'}},
            {'operation': 'knowledge_cat', 'input': 'metrics.md',
             'output': {'path': 'metrics.md', 'text': text, 'sha256': hashlib.sha256(text.encode()).hexdigest()}},
            {'operation': 'query', 'output': {'query_id': 'q_actual', 'rows': rows,
                                              'truncated': False, 'returned_rows': 1}}],
            'final': {'conclusion': 'numeric', 'rows': rows, 'query_ids': ['q_actual'],
                      'business_context': context}}
        return copy.deepcopy(question), copy.deepcopy(run)

    def test_verified_contract_has_a_positive_path_without_rounding_money(self):
        question, run = self.fixture()
        result = score(question, run)
        self.assertEqual(result['answer'], 'correct')
        self.assertEqual(result['business']['delivery'], 'verified_against_contract')
        run['final']['rows'] = [['9007199254740993.00']]
        self.assertEqual(score(question, run)['answer'], 'wrong')

    def test_replica_or_self_asserted_totals_are_not_independent_evidence(self):
        for kind in ['agent_db', 'gateway', 'model', None]:
            question, run = self.fixture()
            question['business_contract']['oracle']['kind'] = kind
            result = score(question, run)
            self.assertNotEqual(result['answer'], 'correct')
            self.assertIn('business_independent_oracle_required', result['failures'])

    def local_fixture(self):
        question, run = self.fixture()
        contract = question['business_contract']
        contract['context']['coverage'] = 'local_imported_snapshot_only'
        run['final']['business_context'] = copy.deepcopy(contract['context'])
        contract['oracle'].update(
            kind='local_snapshot_reconciliation', context_sha256=digest(contract['context']),
            local_snapshot={'import_manifest_sha256': 'c' * 64, 'oracle_execution_sha256': 'd' * 64,
                            'definition_query_sha256': 'e' * 64, 'independent_query_sha256': 'f' * 64,
                            'source_completeness_claimed': False, 'same_snapshot_verified': True})
        return question, run

    def test_local_snapshot_has_a_bounded_positive_path_without_source_access(self):
        question, run = self.local_fixture()
        self.assertEqual(score(question, run)['answer'], 'correct')

    def test_local_reconciliation_cannot_claim_source_or_reuse_its_own_query(self):
        for change in [
            {'import_manifest_sha256': None}, {'oracle_execution_sha256': None},
            {'source_completeness_claimed': True}, {'same_snapshot_verified': False},
            {'independent_query_sha256': 'e' * 64},
        ]:
            question, run = self.local_fixture()
            question['business_contract']['oracle']['local_snapshot'].update(change)
            self.assertIn('business_independent_oracle_required', score(question, run)['failures'])
        question, run = self.local_fixture()
        question['business_contract']['context']['coverage'] = 'all approved channels'
        run['final']['business_context'] = copy.deepcopy(question['business_contract']['context'])
        question['business_contract']['oracle']['context_sha256'] = digest(run['final']['business_context'])
        self.assertIn('business_independent_oracle_required', score(question, run)['failures'])

    def test_changed_or_unapproved_contract_cannot_pass(self):
        for mutation in [
                lambda q: q['business_contract']['approval'].update(status='draft'),
                lambda q: q['business_contract']['approval'].update(reviewer=''),
                lambda q: q['business_contract']['oracle'].update(reconciled=False),
                lambda q: q['business_contract']['context']['scope'].update(tenant_id='another'),
                lambda q: q.update(expected_rows=[['1']]),
                lambda q: q['business_contract']['oracle'].update(evidence_sha256='missing')]:
            question, run = self.fixture()
            mutation(question)
            self.assertNotEqual(score(question, run)['answer'], 'correct')

    def test_wrong_scope_unit_period_or_coverage_cannot_pass(self):
        for key in ['scope', 'unit', 'period', 'coverage', 'time_basis', 'metric']:
            question, run = self.fixture()
            run['final']['business_context'][key] = 'wrong'
            self.assertIn('business_context_mismatch', score(question, run)['failures'])

    def test_missing_or_changed_pack_and_freshness_are_not_verified(self):
        for mutation in [
                lambda r: r['trace'].pop(1),
                lambda r: r['trace'][1]['output'].update(text='modified pack'),
                lambda r: r['trace'][1]['output'].update(sha256='0' * 64),
                lambda r: r['trace'][0].update(error=True),
                lambda r: r['trace'][0]['output'].update(publication='stale')]:
            question, run = self.fixture()
            mutation(run)
            self.assertNotEqual(score(question, run)['answer'], 'correct')

    def test_refusal_truncation_error_and_forged_citation_remain_failures(self):
        for mutation in [
                lambda r: r['final'].update(conclusion='semantic_gap', rows=[]),
                lambda r: r['final'].update(query_ids=['q_forged']),
                lambda r: r['trace'][2]['output'].update(truncated=True),
                lambda r: r['trace'][2]['output'].pop('truncated'),
                lambda r: r['trace'][2]['output'].update(returned_rows=99),
                lambda r: r['trace'][2].update(error=True),
                lambda r: r.update(error='AGENT_TIMEOUT')]:
            question, run = self.fixture()
            mutation(run)
            self.assertNotEqual(score(question, run)['answer'], 'correct')


if __name__ == '__main__':
    unittest.main()
