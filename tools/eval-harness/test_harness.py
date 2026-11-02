import unittest
import json
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch
import subprocess
import tempfile
from harness import score, summarize, generate, map_oracle_sql, oracle_script, merge_metadata, decode_final, execute_oracles, published_scope


class EvaluationTests(unittest.TestCase):
    def test_scalar_answer_can_project_a_real_numeric_column_with_extra_check_columns(self):
        from copy import deepcopy
        question = {'kind': 'numeric', 'expected_rows': [['65']]}
        output = {'query_id': 'q_real', 'rows': [['65', '65']],
                  'columns': [{'name': 'total_rows', 'type': 'int8'},
                              {'name': 'distinct_ids', 'type': 'int8'}],
                  'returned_rows': 1, 'truncated': False}
        run = {'trace': [{'operation': 'query', 'output': output}],
               'final': {'rows': [['65']], 'query_ids': ['q_real']}}
        result = score(question, run)
        self.assertEqual(result['answer'], 'correct')
        self.assertEqual(result['failures'], [])
        self.assertEqual(result['numeric_projection'],
                         {'query_id': 'q_real', 'column_index': 0, 'column': 'total_rows'})
        for patch in (
            {'rows': [['64', '64']]},
            {'rows': [['65', '65'], ['65', '65']], 'returned_rows': 2},
            {'columns': [{'name': 'a', 'type': 'text'}, {'name': 'b', 'type': 'text'}]},
            {'columns': []}, {'returned_rows': True}, {'truncated': True},
            {'truncated': None}, {'code': 'FAILED'},
        ):
            bad = deepcopy(run)
            bad['trace'][0]['output'].update(patch)
            self.assertNotEqual(score(question, bad)['answer'], 'correct', patch)
        bad = deepcopy(run)
        bad['final']['query_ids'] = ['q_forged']
        self.assertFalse(score(question, bad)['citations_valid'])
        self.assertEqual(score({'kind': 'numeric', 'expected_rows': [['66']]}, run)['answer'], 'wrong')
        self.assertEqual(score({'kind': 'grouped', 'expected_rows': [['65']]}, run)['answer'], 'wrong')

    def test_catalog_q_prefixed_identifiers_are_not_inline_query_citations(self):
        column = 'q_414d4f554e54'
        table = 'q_415050.q_4954454d53'
        rows = [['-400000000000015975914.246917581']]
        run = {'trace': [
            {'operation': 'describe', 'input': table, 'output': {'columns': [column]}},
            {'operation': 'query', 'output': {'query_id': 'q_real', 'rows': rows}}],
            'final': {'rows': rows, 'query_ids': ['q_real'],
                      'answer': f'{table} column {column} total is exact; query q_real'}}
        question = {'kind': 'numeric', 'expected_rows': rows}
        self.assertEqual(score(question, run)['answer'], 'correct')
        run['final']['query_ids'] = [column]
        self.assertFalse(score(question, run)['citations_valid'])
        run['final']['query_ids'] = ['q_real']
        run['final']['answer'] += ' and q_forged'
        self.assertFalse(score(question, run)['citations_valid'])
        run['final']['answer'] = column
        run['trace'][0]['error'] = True
        self.assertFalse(score(question, run)['citations_valid'])

    def test_frozen_runner_outputs_are_allowed_but_git_worktrees_and_symlinks_are_not(self):
        from harness import private_output
        with tempfile.TemporaryDirectory() as root:
            root = Path(root)
            repo = root / 'repo'
            repo.mkdir()
            (repo / '.git').mkdir()
            with self.assertRaisesRegex(ValueError, 'outside Git'):
                private_output(repo / 'migration/results')
            frozen = root / 'frozen/tools/eval-harness'
            frozen.mkdir(parents=True)
            self.assertEqual(private_output(root / 'evidence/results'), (root / 'evidence/results').resolve())
            link = root / 'alias'
            link.symlink_to(repo)
            with self.assertRaisesRegex(ValueError, 'outside Git'):
                private_output(link / 'results')

    def test_business_discovery_never_grades_a_safe_gap_or_raw_sum_as_delivery(self):
        for conclusion, delivery in [('semantic_gap', 'blocked'),
                                     ('clarification_needed', 'blocked'),
                                     ('partial', 'blocked'), ('no_data', 'blocked'),
                                     ('execution_failure', 'failed'), ('numeric', 'unverified')]:
            run = {'trace': [{'operation': 'query', 'output': {
                'query_id': 'q_real', 'rows': [[3]], 'truncated': False}}],
                'final': {'conclusion': conclusion, 'query_ids': ['q_real'],
                          'rows': [[3]] if conclusion == 'numeric' else []}}
            result = score({'kind': 'business', 'expected_rows': [[3]]}, run)
            self.assertEqual(result['answer'], 'unverified')
            self.assertEqual(result['business']['delivery'], delivery)
            self.assertEqual(result['business']['successful_queries'], 1)
            self.assertIsNone(result['refusal_correct'])
        result = score({'kind': 'business'}, {'error': 'AGENT_TIMEOUT'})
        self.assertEqual(result['business']['delivery'], 'failed')
        self.assertEqual(summarize([result], {})['business_delivery_counts'], {'failed': 1})

    def test_error_receipt_cannot_ground_a_successful_answer(self):
        for code in ('SCHEMA_INVALID', 'UNAUTHENTICATED', 'MODEL_REQUEST_TIMEOUT'):
            for error in ({'error': {'code': code}},
                          {'error': {'code': code}, 'code': None},
                          {'code': code}):
                for value in (0, 3):
                    with self.subTest(error=error, value=value):
                        run = {'trace': [{'operation': 'query', 'error': False, 'output': {
                            'query_id': 'q_failed', 'rows': [[value]], **error}}],
                            'final': {'rows': [[value]], 'query_ids': ['q_failed']}}
                        result = score({'kind': 'numeric', 'expected_rows': [[value]]}, run)
                        self.assertEqual(result['answer'], 'wrong')
                        self.assertFalse(result['citations_valid'])

    def test_successful_empty_query_and_zero_count_are_grounded_but_timeouts_are_not(self):
        for kind, rows in [('grouped', []), ('numeric', [[0]])]:
            with self.subTest(kind=kind):
                question = {'kind': kind, 'expected_rows': rows}
                run = {'trace': [{'operation': 'query', 'output': {
                    'query_id': 'q_empty', 'rows': rows, 'truncated': False,
                    'returned_rows': len(rows)}}],
                    'final': {'rows': rows, 'query_ids': ['q_empty']}}
                self.assertEqual(score(question, run)['answer'], 'correct')
                for failure in ('AGENT_TIMEOUT', 'MODEL_REQUEST_TIMEOUT'):
                    failed = score(question, dict(run, error=failure))
                    self.assertEqual(failed['answer'], 'unverified')
                    self.assertIn(failure, failed['failures'])

    def test_nonempty_sum_and_netting_can_return_zero_without_being_no_data(self):
        for expression, values in [('SUM(amount)', [0, 0]),
                                   ('SUM(amount)', [25, -25]),
                                   ('SUM(received - refunded)', [25 - 25, 10 - 10])]:
            with self.subTest(expression=expression, values=values):
                rows = [[sum(values)]]
                run = {'trace': [
                    {'operation': 'query', 'input': 'SELECT COUNT(*) FROM app.receipts',
                     'output': {'query_id': 'q_count', 'rows': [[len(values)]]}},
                    {'operation': 'query', 'input': f'SELECT {expression} FROM app.receipts',
                     'output': {'query_id': 'q_sum', 'rows': rows, 'truncated': False}}],
                    'final': {'conclusion': 'numeric', 'rows': rows, 'query_ids': ['q_sum']}}
                self.assertEqual(score({'kind': 'numeric', 'expected_rows': [[0]]}, run)['answer'],
                                 'correct')

    def test_null_and_empty_response_cannot_ground_a_zero_answer(self):
        for output in ({'query_id': 'q_null', 'rows': [[None]]}, {}):
            with self.subTest(output=output):
                run = {'trace': [{'operation': 'query', 'output': output}],
                       'final': {'rows': [[0]], 'query_ids': [output.get('query_id', 'q_missing')]}}
                self.assertEqual(score({'kind': 'numeric', 'expected_rows': [[0]]}, run)['answer'],
                                 'wrong')

    def test_generation_uses_published_edges_and_visible_columns_only(self):
        import hashlib
        relation = {'name': 'edge', 'from': {'dataset': 'app.a', 'columns': ['id']},
                    'to': {'dataset': 'app.b', 'columns': ['id']}}
        oos = {'datasets': [{'dataset': n, 'columns': [{'name': 'id'}, {'name': 'secret'}]}
                            for n in ['app.a', 'app.b']],
               'relationships': [relation, dict(relation, name='unpublished')]}
        texts = {'tables/app.a.md': '| `id` | integer |',
                 'tables/app.b.md': '| `id` | integer |',
                 'relationships.md': '## edge\n\n`app.a.id` -> `app.b.id`\n'}
        pack = {'files': [{'path': p, 'text': t, 'sha256': hashlib.sha256(t.encode()).hexdigest()}
                          for p, t in texts.items()]}
        scoped = published_scope(oos, pack)
        self.assertEqual(scoped['relationships'], [relation])
        self.assertEqual(scoped['datasets'][0]['columns'], [{'name': 'id'}])
        pack['files'][-1]['path'] = 'relationships/page-0001.md'
        self.assertEqual(published_scope(oos, pack)['relationships'], [relation])
        pack['files'][0]['text'] += 'tampered'
        with self.assertRaisesRegex(ValueError, 'pack_file_digest'):
            published_scope(oos, pack)

    def test_generated_time_predicates_use_matching_explicit_types(self):
        for dtype, literal in [('date', "DATE '2000-01-01'"),
                               ('timestamp without time zone', "TIMESTAMP '2000-01-01 00:00:00'"),
                               ('timestamp with time zone', "TIMESTAMPTZ '2000-01-01 00:00:00+00:00'")]:
            questions = generate({'datasets': [{'dataset': 'app.a', 'columns': [
                {'name': 'created_at', 'data_type': dtype}]}]})
            question = next(q for q in questions if q['category'] == 'time_range')
            self.assertIn(literal, question['oracle_sql'])
            self.assertIn('created_at', question['text'])

    def test_recall_only_from_discovery_outputs(self):
        q = {'expected_tables': ['app.items'], 'kind': 'numeric', 'expected_rows': [[3]]}
        trace = [{'operation': 'query', 'input': 'SELECT COUNT(*) FROM app.items',
                  'output': {'query_id': 'q_real', 'rows': [[3]]}}]
        r = score(q, {'trace': trace, 'final': {'rows': [[3]], 'query_ids': ['q_real']}})
        self.assertEqual(r['table_recall'], 0)
        self.assertEqual(r['answer'], 'correct')

    def test_successful_describe_is_discovery_without_redundant_table_listing(self):
        question = {'expected_tables': ['app.items'], 'kind': 'numeric', 'expected_rows': [[3]]}
        call = {'operation': 'describe', 'input': 'app.items', 'output': {'columns': ['id']}}
        run = {'trace': [call], 'final': {'rows': [[3]], 'query_ids': []}}
        result = score(question, run)
        self.assertEqual(result['table_recall'], 1)
        self.assertEqual(result['first_hit_calls'], 1)
        for output in ({}, {'columns': []}, {'columns': ['id'], 'code': 'NOT_FOUND'},
                       {'columns': ['id'], 'error': 'failed'}):
            call['output'] = output
            self.assertEqual(score(question, run)['table_recall'], 0)
        call['output'] = {'columns': ['id']}
        call['error'] = True
        self.assertEqual(score(question, run)['table_recall'], 0)

    def test_complete_query_dependency_receipt_avoids_redundant_discovery(self):
        question = {'expected_tables': ['app.items'], 'kind': 'numeric', 'expected_rows': [[3]]}
        output = {'query_id': 'q_real', 'rows': [[3]], 'returned_rows': 1,
                  'truncated': False, 'freshness': [{'table': 'app.items', 'state': 'live'}]}
        call = {'operation': 'query', 'input': 'SELECT COUNT(*) FROM app.items',
                'process_exit_code': 0, 'output': output}
        run = {'trace': [call], 'final': {'rows': [[3]], 'query_ids': ['q_real']}}
        result = score(question, run)
        self.assertEqual((result['table_recall'], result['first_hit_calls']), (1, 1))
        self.assertEqual(result['answer'], 'correct')
        for patch in ({'truncated': True}, {'truncated': None}, {'returned_rows': 2},
                      {'returned_rows': True}, {'query_id': ''}, {'rows': [3]},
                      {'freshness': None}, {'freshness': ['app.items']},
                      {'freshness': [{'table': 'app.other'}]},
                      {'freshness': [{'table': 'app.items extra'}]},
                      {'code': 'TIMEOUT'}, {'error': 'failed'}):
            call['output'] = {**output, **patch}
            self.assertEqual(score(question, run)['table_recall'], 0, patch)
        call['output'] = output
        for patch in ({'error': True}, {'process_exit_code': 2}, {'limited': True}):
            run['trace'] = [{**call, **patch}]
            self.assertEqual(score(question, run)['table_recall'], 0, patch)
        run['trace'] = [{**call, 'process_exit_code': 2}]
        self.assertEqual(score(question, run)['answer'], 'wrong')
        # A successful empty result still proves the table was resolved.
        run['trace'] = [{**call, 'output': {**output, 'rows': [], 'returned_rows': 0}}]
        self.assertEqual(score(question, run)['table_recall'], 1)

    def test_failed_discovery_does_not_credit_table_names_in_error_messages(self):
        question = {'expected_tables': ['app.items'], 'kind': 'numeric', 'expected_rows': [[3]]}
        for op in ('tables', 'knowledge_cat', 'knowledge_search'):
            run = {'trace': [{'operation': op, 'output': {
                'code': 'NOT_FOUND', 'message': 'app.items not found'}}],
                'final': {'rows': [[3]], 'query_ids': []}}
            self.assertEqual(score(question, run)['table_recall'], 0)

    def test_missing_oracle_is_not_correct(self):
        self.assertEqual(score({'kind': 'numeric'}, {'final': {'rows': [[3]]}})['answer'], 'unverified')

    def test_forged_citation_and_numbers_fail(self):
        r = score({'kind': 'numeric', 'expected_rows': [[3]]},
                  {'final': {'rows': [[4]], 'query_ids': ['q_fake']}})
        self.assertEqual(r['answer'], 'wrong')
        self.assertFalse(r['citations_valid'])
        self.assertIn('invalid_citation', r['failures'])

    def test_first_hit_and_irrelevant_exploration(self):
        q = {'kind': 'refusal', 'expected_tables': ['app.items']}
        run = {'trace': [
            {'operation': 'tables', 'output': {'tables': [{'name': 'app.items'}, {'name': 'app.other'}]}},
            {'operation': 'describe', 'input': 'app.other', 'output': {}}],
            'final': {'conclusion': 'refusal', 'query_ids': []}}
        r = score(q, run)
        self.assertEqual(r['first_hit_calls'], 1)
        self.assertEqual(r['unrelated_describes'], 1)
        self.assertEqual(r['retrieved_extra_tables'], 1)

    def test_refusal_requires_evidence(self):
        r = score({'kind': 'refusal'}, {'final': {'conclusion': 'refusal'}})
        self.assertNotEqual(r['answer'], 'correct')

    def test_freshness_requires_all_terms(self):
        q = {'kind': 'freshness', 'required_terms': ['unmeasured', 'CROSS_TABLE_SKEW']}
        r = score(q, {'trace': [{'operation': 'freshness', 'output': {}}],
                      'final': {'answer': 'unmeasured'}})
        self.assertFalse(r['freshness_correct'])

    def test_unknowns_remain_in_denominator(self):
        s = summarize([{'answer': 'unverified', 'table_recall': None, 'refusal_correct': None,
                        'freshness_correct': None, 'failures': ['missing_oracle']}], {})
        self.assertEqual(s['answer_accuracy'], 0)
        self.assertIsNone(s['table_recall'])

    def test_generator_quotes_names_and_does_not_invent_joins(self):
        qs = generate({'datasets': [{'dataset': 'app.items', 'columns': []}], 'relationships': []})
        self.assertIn('"app"."items"', qs[0]['oracle_sql'])
        self.assertFalse(any(q['category'] == 'two_table_join' for q in qs))

    def test_generated_join_questions_disambiguate_multiple_roles(self):
        oos = {'datasets': [{'dataset': n, 'columns': []} for n in ['app.a', 'app.b']],
               'relationships': [{'name': role, 'from': {'dataset': 'app.a', 'columns': [role]},
                                  'to': {'dataset': 'app.b', 'columns': ['id']}}
                                 for role in ['sender', 'receiver']]}
        joins = [q for q in generate(oos) if q['category'] == 'two_table_join']
        self.assertNotEqual(joins[0]['text'], joins[1]['text'])
        self.assertIn('sender', joins[0]['text'])
        self.assertIn('receiver', joins[1]['text'])

    def test_oracle_mapping_uses_physical_columns_and_tables(self):
        metadata = {'tables': [{'logical_table': 'app.items', 'oracle_table': '_oo.replica',
                                'columns': {'category': 'c_1'}}]}
        sql = 'SELECT "category", COUNT(*) AS n FROM "app"."items" GROUP BY "category"'
        mapped = map_oracle_sql(sql, metadata)
        self.assertIn('"_oo"."replica"', mapped)
        self.assertIn('GROUP BY "c_1"', mapped)

    def test_oracle_mapping_accepts_named_aliases_and_as(self):
        metadata = {'tables': [
            {'logical_table': 'app.items', 'oracle_table': '_oo.items',
             'columns': {'id': 'item_id'}},
            {'logical_table': 'app.groups', 'oracle_table': '_oo.groups',
             'columns': {'id': 'group_id'}}]}
        sql = ('SELECT COUNT(*) FROM "app"."items" AS hdr '
               'JOIN "app"."groups" dept ON hdr."id" = dept."id"')
        self.assertEqual(map_oracle_sql(sql, metadata),
                         'SELECT COUNT(*) FROM "_oo"."items" AS hdr '
                         'JOIN "_oo"."groups" dept ON hdr."item_id" = dept."group_id"')

    def test_oracle_mapping_does_not_treat_where_as_an_alias(self):
        metadata = {'tables': [{'logical_table': 'app.items', 'oracle_table': '_oo.items',
                                'columns': {'id': 'item_id'}}]}
        self.assertIn('WHERE "item_id"', map_oracle_sql(
            'SELECT COUNT(*) FROM "app"."items" WHERE "id" > 0', metadata))

    def test_oracle_is_read_only_repeatable_and_rejects_multiple_statements(self):
        script = oracle_script([{'oracle_sql': 'SELECT COUNT(*) AS n FROM "app"."items"'}])
        self.assertIn('ISOLATION LEVEL REPEATABLE READ READ ONLY', script)
        self.assertIn('statement_timeout', script)
        with self.assertRaises(ValueError):
            oracle_script([{'oracle_sql': 'SELECT 1; DELETE FROM app.items'}])

    def test_forged_inline_citation_fails_even_when_structured_ids_are_valid(self):
        q = {'kind': 'numeric', 'expected_rows': [[3]]}
        run = {'trace': [{'operation': 'query', 'output': {'query_id': 'q_real', 'rows': [[3]]}}],
               'final': {'answer': '3 [q_fake]', 'rows': [[3]], 'query_ids': ['q_real']}}
        self.assertFalse(score(q, run)['citations_valid'])

    def test_fake_agent_and_cli_end_to_end(self):
        with tempfile.TemporaryDirectory(prefix='oo-wf-eval-') as directory:
            root = Path(directory)
            for package in ('pi-agent-core', 'pi-ai'):
                module = root / 'node_modules' / '@earendil-works' / package
                module.mkdir(parents=True)
                (module / 'package.json').write_text(json.dumps({'type': 'module', 'exports': {'./package.json': './package.json'}}))
                (module / 'dist').mkdir()
            ai = root / 'node_modules/@earendil-works/pi-ai/dist'
            (ai / 'index.js').write_text('export const Type = {Object:x=>x,String:()=>({}),Integer:()=>({}),Optional:x=>x,Union:x=>x,Literal:x=>x};')
            (ai / 'api').mkdir()
            (ai / 'api/openai-completions.js').write_text('export const streamSimple=()=>{};')
            core = root / 'node_modules/@earendil-works/pi-agent-core/dist'
            (core / 'index.js').write_text("""
import assert from 'node:assert/strict';
export class Agent {
  constructor(options) {
    const prompt = options.initialState.systemPrompt;
    assert(prompt.includes('# 单 Gateway 数据分析技能'));
    assert(prompt.includes('# 业务回答规范'));
    for (const rule of ['实际查询数据库', '实际数字或分组结果', '不是停止查询的理由',
                        '非空数据的 SUM 或正负抵销结果，不要求空集', '业务口径明确定义空集为零',
                        'NULL 本身不是零或空集证据', '执行失败、认证失败或模型超时不能作为报零依据',
                        '用户正文不出现“知识包”', '“字段映射”', '内部诊断只留在结构化审计']) {
      assert(prompt.includes(rule), rule);
    }
    this.state = {...options.initialState, messages: []};
  }
  subscribe() {}
  abort() {}
  async prompt() {
    await this.state.tools[0].execute('one', {operation:'tables'});
    await this.state.tools[0].execute('two', {operation:'query', input:'SELECT COUNT(*) FROM app.items'});
    this.state.messages.push({role:'assistant', content:[{type:'text', text:JSON.stringify({answer:'3 [q_real]', rows:[[3]], query_ids:['q_real']})}]});
  }
}
""")
            cli = root / 'cli.mjs'
            cli.write_text("console.log(JSON.stringify(process.argv.includes('tables')?{tables:[{name:'app.items'}]}:{query_id:'q_real',rows:[[3]]}));")
            payload = {'text': 'count items', 'config': {'demoDir': str(root), 'cliCommand': ['node', str(cli)],
                       'gateway': 'http://localhost:1', 'tokenFile': '/not-opened'}}
            process = subprocess.run(['node', str(Path(__file__).with_name('agent.mjs'))], input=json.dumps(payload),
                                     capture_output=True, text=True, timeout=10, check=True)
            response = json.loads(process.stdout)
            self.assertIsNone(response.get('error'))
            result = score({'kind': 'numeric', 'expected_tables': ['app.items'], 'expected_rows': [[3]]}, response)
            self.assertEqual(result['answer'], 'correct')
            self.assertEqual(result['table_recall'], 1)
            cli.write_text("console.log('not JSON');")
            malformed = subprocess.run(
                ['node', str(Path(__file__).with_name('agent.mjs'))],
                input=json.dumps(payload), capture_output=True, text=True, timeout=10, check=True)
            response = json.loads(malformed.stdout)
            self.assertTrue(all(call['error'] for call in response['trace']))
            self.assertTrue(all(call['output']['code'] == 'CLI_INVALID_JSON'
                                for call in response['trace']))
            self.assertEqual(score({'kind': 'numeric', 'expected_rows': [[3]]},
                                   response)['answer'], 'wrong')

    def test_knowledge_table_file_is_a_retrieval_hit(self):
        q = {'kind': 'numeric', 'expected_tables': ['app.items']}
        r = score(q, {'trace': [{'operation': 'knowledge_search', 'output': {'path': 'tables/app.items.md'}}]})
        self.assertEqual(r['table_recall'], 1)
        self.assertEqual(r['retrieved_extra_tables'], 0)

    def test_malformed_model_final_does_not_crash(self):
        self.assertEqual(score({'kind': 'numeric'}, {'final': []})['answer'], 'unverified')
        r = score({'kind': 'numeric', 'expected_rows': [[3]]}, {'final': {'rows': 'bad', 'query_ids': [{}]}})
        self.assertNotEqual(r['answer'], 'correct')

    def test_metadata_adds_date_facts_not_hidden_columns(self):
        oos = {'datasets': [{'dataset': 'app.items', 'columns': [{'name': 'created_at'}]}]}
        metadata = {'tables': [{'logical_table': 'app.items', 'column_facts': [
            {'name': 'created_at', 'data_type': 'date'}, {'name': 'hidden', 'data_type': 'date'}]}]}
        merged = merge_metadata(oos, metadata)
        self.assertEqual(len(merged['datasets'][0]['columns']), 1)
        qs = generate(merged)
        self.assertTrue(any(q['category'] == 'time_range' for q in qs))
        self.assertFalse(any('hidden' in q.get('oracle_sql', '') for q in qs))

    def test_replay_recovers_json_suffix_without_model_retry(self):
        self.assertEqual(decode_final('说明\n{"rows":[[3]],"query_ids":["q_real"]}')['rows'], [[3]])
        self.assertIsNone(decode_final('no JSON'))

    def test_numeric_looking_group_identifiers_keep_leading_zeroes(self):
        q = {'kind': 'grouped', 'expected_rows': [['001', 3]]}
        run = {'trace': [{'operation': 'query', 'output': {'query_id': 'q_real', 'rows': [['1', '3']]}}],
               'final': {'rows': [['1', '3']], 'query_ids': ['q_real']}}
        self.assertEqual(score(q, run)['answer'], 'wrong')

    def test_physical_mapping_does_not_cascade_replacements(self):
        metadata = {'tables': [{'logical_table': 'app.items', 'oracle_table': '_oo.replica',
                                'columns': {'id': 'c_1', 'c_1': 'c_2'}}]}
        mapped = map_oracle_sql('SELECT "id", COUNT(*) FROM "app"."items" GROUP BY "id"', metadata)
        self.assertIn('SELECT "c_1",', mapped)

    def test_oracle_decimal_precision_is_preserved(self):
        with tempfile.TemporaryDirectory(prefix='oo-wf-eval-') as directory:
            root = Path(directory)
            (root / 'questions.json').write_text(json.dumps({'questions': [{'kind': 'grouped', 'oracle_sql': 'SELECT category, SUM(amount) AS n FROM app.items GROUP BY category'}]}))
            (root / 'config.json').write_text(json.dumps({'psqlCommand': ['fake-psql']}))
            args = SimpleNamespace(questions=str(root / 'questions.json'), config=str(root / 'config.json'), metadata=None, output=str(root / 'output.json'))
            with patch('harness.subprocess.run', return_value=SimpleNamespace(returncode=0, stdout='[{"category":"001","n":0.1234567890123456789}]\n')):
                execute_oracles(args)
            question = json.loads((root / 'output.json').read_text())['questions'][0]
            self.assertEqual(question['expected_rows'], [['001', '0.1234567890123456789']])
            self.assertEqual(question['numeric_columns'], [1])

    def test_empty_numeric_result_still_needs_a_real_query_receipt(self):
        r = score({'kind': 'grouped', 'expected_rows': []}, {'final': {'rows': [], 'query_ids': []}})
        self.assertEqual(r['answer'], 'wrong')


if __name__ == '__main__':
    unittest.main()
