"""Offline integration through the real Python -> Node -> Agent caller, using SDK/CLI doubles."""
import contextlib
import copy
import hashlib
import io
import json
from pathlib import Path
import shutil
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

from harness import evaluator_digest, inspect_binding, replay, response_binding_error, run, score, runtime_evaluator_digest


def sha(data):
    return hashlib.sha256(data.encode() if isinstance(data, str) else data).hexdigest()


class SelectedSkillTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='skill-caller-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.skill_text = '# Fixture selected Skill\nPrivate marker: use actual queries, never errors as zero.\n'
        for stage in ('source', 'delivery', 'installed'):
            (self.root / f'{stage}.md').write_text(self.skill_text)
        manifest = '{"fixture":"publication-one"}'
        self.config = {
            'demoDir': str(self.root), 'gateway': 'http://127.0.0.1:1',
            'tokenFile': '/never-read', 'bearerFile': '/never-read-model-secret',
            'gatewayManifestSha256': sha(manifest), 'cliCommand': [str(self.root / 'oo')],
            'agentSkill': {
                **{stage: str(self.root / f'{stage}.md') for stage in ('source', 'delivery', 'installed')},
                'sha256': sha(self.skill_text),
                'cli': {'declaredPath': '/opt/customer/bin/oo', 'sha256': '0' * 64},
            },
        }
        self.cli_source = (
            '#!' + shutil.which('node') + '\n'
            'const args=process.argv.slice(2);\n'
            'const versions={policy_version:"p1",knowledge_version:"k1"};\n'
            f'const text={json.dumps(manifest)};\n'
            f'const hash={json.dumps(sha(manifest))};\n'
            'const output=args.includes("manifest.json")'
            '?{...versions,path:"manifest.json",text,sha256:hash}'
            ':args[0]==="tables"?{tables:[{name:"app.items"}]}'
            ':{...versions,query_id:"q_actual",rows:[[3]],truncated:false,returned_rows:1};\n'
            'console.log(JSON.stringify(output));\n')
        self.write_cli(self.cli_source)
        self.questions = {'questions': [{'id': 'fixture', 'text': 'Count the fixture items',
                                         'kind': 'numeric', 'expected_rows': [[3]],
                                         'expected_tables': ['app.items']}]}
        self.args = SimpleNamespace(config=str(self.root / 'config.json'),
                                    questions=str(self.root / 'questions.json'),
                                    output=str(self.root / 'run'), concurrency=1,
                                    answer_threshold=.8, table_threshold=.9, refusal_threshold=1)
        self.write_config()
        Path(self.args.questions).write_text(json.dumps(self.questions))
        core = self.root / 'node_modules/@earendil-works/pi-agent-core'
        ai = self.root / 'node_modules/@earendil-works/pi-ai'
        for module in (core, ai):
            (module / 'dist').mkdir(parents=True)
            (module / 'package.json').write_text('{"type":"module"}')
        (ai / 'dist/api').mkdir()
        (ai / 'dist/index.js').write_text(
            'export const Type=new Proxy({}, {get:()=>()=>({})});')
        (ai / 'dist/api/openai-completions.js').write_text("""
export function streamSimple(_model, context, options) {
  options.onPayload({messages:[{role:'system',content:context.systemPrompt},
    {role:'user',content:'synthetic question'}],tools:[],stream:true});
  return {};
}
""")
        self.core_entry = core / 'dist/index.js'
        self.core_entry.write_text("""
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
export class Agent {
  constructor(options) {
    this.options=options;
    const skill=readFileSync(fileURLToPath(new URL('../../../../installed.md',import.meta.url)),'utf8');
    const prompt=options.initialState.systemPrompt;
    assert(prompt.endsWith(skill), 'selected Skill absent from actual Agent constructor');
    for(const marker of ['query_ids','rows','/opt/customer/bin/oo','structured JSON scoring contract']) {
      assert(prompt.includes(marker), 'missing scoring or mapping contract');
    }
    this.state={...options.initialState,messages:[]};
  }
  subscribe(fn) {this.subscriber=fn;}
  abort() {}
  async prompt() {
    this.options.streamFn(this.state.model,{systemPrompt:this.state.systemPrompt},{});
    await this.state.tools[0].execute('one',{operation:'tables'});
    await this.state.tools[0].execute('two',{operation:'query',input:'SELECT COUNT(*) FROM app.items'});
    const message={role:'assistant',stopReason:'stop',usage:{input:10,output:10},
      content:[{type:'text',text:JSON.stringify({answer:'3',conclusion:'numeric',rows:[[3]],query_ids:['q_actual']})}]};
    this.state.messages.push(message);
    this.subscriber({type:'message_end',message});
  }
}
""")

    def write_cli(self, text):
        path = self.root / 'oo'
        path.write_text(text)
        path.chmod(0o700)
        self.config['agentSkill']['cli']['sha256'] = sha(path.read_bytes())

    def write_config(self):
        Path(self.args.config).write_text(json.dumps(self.config))

    def run_fixture(self):
        self.write_config()
        with contextlib.redirect_stdout(io.StringIO()), patch('harness.time.sleep'):
            status = run(self.args)
        output = Path(self.args.output)
        return status, json.loads((output / '0001-trace.json').read_text())

    def test_real_caller_injects_selected_skill_and_persists_prompt_cli_runtime_hashes(self):
        status, response = self.run_fixture()
        self.assertEqual(status, 0, response.get('error'))
        self.assertEqual(response['final']['query_ids'], ['q_actual'])
        binding = response['prompt_binding']
        self.assertEqual(binding['skill']['installed']['sha256'], sha(self.skill_text))
        self.assertEqual(binding['skill']['cli']['actual']['sha256'], self.config['agentSkill']['cli']['sha256'])
        self.assertEqual(response['model_requests'][0]['system_prompt_sha256'], binding['system_prompt_sha256'])
        self.assertEqual(response['runtime_binding'], response['runtime_binding_after'])
        self.assertNotEqual(response['evaluator_sha256'], response['runtime_evaluator_sha256'])
        self.assertEqual(response['evaluator_sha256'], evaluator_digest(self.config))
        self.assertEqual(response['runtime_data_binding'][0]['query_id'], 'q_actual')
        self.assertEqual(response['runtime_evaluator_sha256'],
                         runtime_evaluator_digest(response['evaluator_sha256'], response))
        tampered = copy.deepcopy(response)
        tampered['trace'][-1]['output']['rows'] = [[99]]
        self.assertNotEqual(response['runtime_evaluator_sha256'],
                            runtime_evaluator_digest(response['evaluator_sha256'], tampered))
        manifest = json.loads((Path(self.args.output) / 'run-binding.json').read_text())
        self.assertEqual(manifest['prompt_binding'], binding)
        for data in (manifest, response):
            self.assertNotIn('Private marker', json.dumps(data))
            self.assertNotIn('never-read-model-secret', json.dumps(data))

    def test_skill_change_invalidates_old_digest_and_old_replay_even_after_refreshing_pins(self):
        status, response = self.run_fixture()
        self.assertEqual(status, 0)
        before = response['evaluator_sha256']
        for stage in ('source', 'delivery', 'installed'):
            Path(self.config['agentSkill'][stage]).write_text(self.skill_text + 'Changed.\n')
        with self.assertRaisesRegex(ValueError, 'SKILL_SHA_DRIFT'):
            evaluator_digest(self.config)
        self.config['agentSkill']['sha256'] = sha(self.skill_text + 'Changed.\n')
        self.assertNotEqual(evaluator_digest(self.config), before)
        self.write_config()
        args = SimpleNamespace(**{**vars(self.args), 'traces': self.args.output,
                                  'output': str(self.root / 'replayed.json')})
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(replay(args), 2)
        self.assertNotEqual(json.loads(Path(args.output).read_text())['results'][0]['answer'], 'correct')
        args.config = None
        args.output = str(self.root / 'replay-without-config.json')
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(replay(args), 2)

    def test_runtime_manifest_and_cli_changes_invalidate_digest(self):
        before = evaluator_digest(self.config)
        changed = copy.deepcopy(self.config)
        changed['gatewayManifestSha256'] = 'b' * 64
        self.assertNotEqual(evaluator_digest(changed), before)
        self.write_cli(self.cli_source + '// changed executable bytes\n')
        self.assertNotEqual(evaluator_digest(self.config), before)

    def test_structured_cli_error_with_zero_exit_is_not_a_zero_or_successful_query(self):
        self.write_cli(self.cli_source.replace(
            'query_id:"q_actual",rows:[[3]],truncated:false,returned_rows:1',
            'query_id:"q_actual",rows:[[0]],error:{code:"DENIED"},truncated:false,returned_rows:1'))
        status, response = self.run_fixture()
        self.assertEqual(status, 2)
        query = response['trace'][-1]
        self.assertTrue(query['error'])
        self.assertEqual(query['output']['query_id'], 'q_actual')
        response['final']['rows'] = [[0]]
        self.assertNotEqual(score({'kind': 'numeric', 'expected_rows': [[0]]}, response)['answer'], 'correct')

    def test_wrong_query_release_is_retained_but_rejected(self):
        self.write_cli(self.cli_source.replace('query_id:"q_actual"', 'knowledge_version:"k2",query_id:"q_actual"'))
        status, response = self.run_fixture()
        self.assertEqual(status, 2)
        self.assertEqual(response['error'], 'GATEWAY_BINDING_DRIFT')
        self.assertEqual(response['trace'][-1]['output']['query_id'], 'q_actual')

    def test_missing_skill_preflight_writes_failure_without_importing_agent_or_reading_credentials(self):
        Path(self.config['agentSkill']['installed']).unlink()
        self.core_entry.write_text('throw new Error("MUST_NOT_IMPORT");')
        self.write_config()
        self.assertEqual(run(self.args), 2)
        evidence = json.loads((Path(self.args.output) / 'preflight-failure.json').read_text())
        self.assertEqual(evidence, {'error': 'SKILL_FILE_UNAVAILABLE', 'model_called': False})
        self.assertFalse((Path(self.args.output) / '0001-trace.json').exists())

    def test_drift_after_prompt_preparation_blocks_tool_execution_and_preserves_failure(self):
        source = self.core_entry.read_text().replace(
            "import {readFileSync} from 'node:fs';", "import {readFileSync,appendFileSync} from 'node:fs';")
        source = source.replace('async prompt() {', """
  async prompt() {
    appendFileSync(fileURLToPath(new URL('../../../../installed.md',import.meta.url)), 'Drift.');
""")
        self.core_entry.write_text(source)
        status, response = self.run_fixture()
        self.assertEqual(status, 2)
        self.assertIn(response['error'], ('EVALUATION_INPUT_DRIFT', 'evaluation_input_drift'))
        self.assertTrue(all(call['error'] for call in response['trace']))
        self.assertFalse(any(call['output'].get('query_id') for call in response['trace']))

    def test_missing_or_edited_prompt_receipt_never_counts_as_verified(self):
        binding = inspect_binding(self.config)
        self.assertEqual(response_binding_error({}, self.config, binding), 'evaluation_prompt_binding_mismatch')
        response = {'prompt_binding': binding, 'runtime_binding': {
            'manifest_sha256': self.config['gatewayManifestSha256'], 'policy_version': 'p1', 'knowledge_version': 'k1'}}
        response['runtime_binding_after'] = response['runtime_binding']
        self.assertEqual(response_binding_error(response, self.config, binding), 'evaluation_wire_prompt_unverified')

    def test_generic_scoring_does_not_pool_known_different_versions_or_accept_partial_as_full(self):
        response = {'trace': [
            {'operation': 'query', 'output': {'query_id': 'q_one', 'rows': [[1]], 'knowledge_version': 'k1'}},
            {'operation': 'query', 'output': {'query_id': 'q_two', 'rows': [[2]], 'knowledge_version': 'k2'}},
        ], 'final': {'rows': [[1], [2]], 'query_ids': ['q_one', 'q_two']}}
        self.assertEqual(score({'kind': 'grouped', 'expected_rows': [[1], [2]]}, response)['answer'], 'unverified')
        response['final'].update(conclusion='partial', rows=[])
        result = score({'kind': 'business', 'expected_rows': [[3]]}, response)
        self.assertNotEqual(result['answer'], 'correct')
        response = {'trace': [{'operation': 'query', 'output': {'query_id': 'q_one', 'rows': [[1]]}}],
                    'final': {'conclusion': 'partial', 'rows': [[1]], 'query_ids': ['q_one']}}
        self.assertNotEqual(score({'kind': 'numeric', 'expected_rows': [[1]]}, response)['answer'], 'correct')


if __name__ == '__main__':
    unittest.main()
