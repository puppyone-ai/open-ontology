"""Data-only F22 transitions through actual CLI processes and cold Node Agent callers."""
import contextlib
import copy
import io
import json
from pathlib import Path
import shutil
import unittest
from unittest.mock import patch

import test_skill_binding as skills
from harness import run, score, runtime_evaluator_digest
from publication_contract import retry_allowed


def make_fixture(test, scenario='stable', retry=False):
    fixture = skills.SelectedSkillTests()
    fixture.setUp()
    test.addCleanup(fixture.doCleanups)
    fixture.config['publicationBinding'] = {'mode': 'f22', 'allowWholeQuestionRetry': retry}
    updated = scenario in ('once', 'post-drift') and retry
    fixture.questions['questions'][0].update(kind='grouped', expected_rows=[
        [20 if updated else 10], [30 if updated else 15]])
    Path(fixture.args.questions).write_text(json.dumps(fixture.questions))
    source = r"""
import {readFileSync,writeFileSync,appendFileSync,existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
const root=fileURLToPath(new URL('.',import.meta.url));
const args=process.argv.slice(2);
const flag=name=>args.includes(name)?args[args.indexOf(name)+1]:null;
const expected=flag('--expected-publication-id'), vector=flag('--expected-data-vector-sha256');
const scenario=SCENARIO;
const file=root+'state.json';
const state=existsSync(file)?JSON.parse(readFileSync(file,'utf8')):{generation:1,acquisitions:0};
if(expected==='current') state.acquisitions++;
const binding={publication_id:'pub1_'+String(state.generation).repeat(64),
  data_vector_sha256:String(state.generation+4).repeat(64)};
appendFileSync(root+'calls.jsonl',JSON.stringify({operation:args[0],expected,vector,
  input:args.at(-1),generation:state.generation,acquisitions:state.acquisitions})+'\n');
const drift={class:'gateway',code:'CONFLICT',exit:2,retryable:false,query_id:'q_drift_'+state.acquisitions,
  hint:'reason=PUBLICATION_DRIFT; action=RECOMPUTE_WHOLE_QUESTION'};
function finish(output,code=0) {
  writeFileSync(file,JSON.stringify(state));
  console.log(JSON.stringify(output)); process.exit(code);
}
if(!expected || (expected==='current' && vector)) finish({class:'usage',code:'INVALID_ARGUMENT',exit:2},2);
if(expected==='current' && scenario==='acquire-auth') finish({...drift,code:'UNAUTHENTICATED',exit:4},4);
if(expected!=='current' && (expected!==binding.publication_id || vector!==binding.data_vector_sha256))
  finish(drift,2);
const versions={policy_version:'p1',knowledge_version:'k1',publication_binding:binding};
if(args.includes('manifest.json')) finish({...versions,path:'manifest.json',text:MANIFEST,sha256:MANIFEST_HASH,
  ...(scenario==='missing'?{publication_binding:undefined}:{})});
if(args[0]==='freshness') finish({...versions,source_time:'unmeasured',publication:'live'});
if(args[0]==='tables') finish({...versions,tables:[{name:'app.items'}]});
if(args[0]==='query') {
  const part=args.at(-1).includes("'a'")?'a':'b';
  if(part==='b' && scenario==='auth') finish({...drift,code:'UNAUTHENTICATED',exit:4},4);
  if(part==='b' && scenario==='usage') finish({...drift,class:'usage',code:'INVALID_ARGUMENT'},2);
  if(part==='b' && scenario==='forged') finish({...drift,rows:[[0]]},2);
  const output={...versions,query_id:'q_'+state.generation+'_'+part,
    rows:[[state.generation*(part==='a'?10:15)]],truncated:false,returned_rows:1};
  if(part==='b' && scenario==='mismatch') output.publication_binding={...binding,data_vector_sha256:'f'.repeat(64)};
  if(part==='b' && scenario==='missing-later') delete output.publication_binding;
  if(part==='a' && (scenario==='always' || (scenario==='once' && state.acquisitions===1))) state.generation++;
  if(part==='b' && scenario==='post-drift' && state.acquisitions===1) state.generation++;
  finish(output);
}
finish({...versions,valid:true});
"""
    source = source.replace('SCENARIO', json.dumps(scenario))
    manifest = '{"fixture":"publication-one"}'
    source = source.replace('MANIFEST_HASH', json.dumps(skills.sha(manifest))).replace('MANIFEST', json.dumps(manifest))
    fixture.write_cli('#!' + shutil.which('node') + '\n' + source)
    fixture.core_entry.write_text("""
import {readFileSync,appendFileSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {createHash} from 'node:crypto';
export class Agent {
  constructor(options) {
    this.options=options; this.state={...options.initialState,messages:[]};
    appendFileSync(fileURLToPath(new URL('../../../../agents.jsonl',import.meta.url)),
      JSON.stringify({pid:process.pid,prompt_sha256:createHash('sha256').update(this.state.systemPrompt).digest('hex')})+'\\n');
  }
  subscribe(fn){this.subscriber=fn;}
  abort(){this.aborted=true;}
  async prompt(){
    this.options.streamFn(this.state.model,{systemPrompt:this.state.systemPrompt},{});
    await this.state.tools[0].execute('tables',{operation:'tables'});
    const outputs=[];
    for(const part of ['a','b']){
      if(this.aborted) return;
      const result=await this.state.tools[0].execute(part,{operation:'query',
        input:"SELECT COUNT(*) FROM app.items WHERE part='"+part+"'"});
      if(this.aborted) return;
      outputs.push(JSON.parse(result.content[0].text));
    }
    const message={role:'assistant',stopReason:'stop',usage:{input:20,output:20},
      content:[{type:'text',text:JSON.stringify({answer:'fixture only',conclusion:'grouped',
        rows:outputs.flatMap(output=>output.rows||[]),query_ids:outputs.map(output=>output.query_id)})}]};
    this.state.messages.push(message); this.subscriber({type:'message_end',message});
  }
}
""")
    fixture.write_config()
    return fixture


def run_fixture(fixture):
    fixture.write_config()
    with contextlib.redirect_stdout(io.StringIO()), patch('harness.time.sleep'):
        status = run(fixture.args)
    root = Path(fixture.args.output)
    response = json.loads((root / '0001-trace.json').read_text())
    calls = [json.loads(line) for line in (fixture.root / 'calls.jsonl').read_text().splitlines()]
    return status, response, calls


class PublicationBindingTests(unittest.TestCase):
    def test_stable_f22_records_all_read_receipts_and_old_generic_scoring_still_works(self):
        fixture = make_fixture(self)
        status, response, calls = run_fixture(fixture)
        self.assertEqual(status, 0, response.get('error'))
        pin = response['runtime_binding']['publication_binding']
        self.assertEqual(calls[0]['expected'], 'current')
        self.assertIsNone(calls[0]['vector'])
        self.assertTrue(all(c['expected'] == pin['publication_id'] and
                            c['vector'] == pin['data_vector_sha256'] for c in calls[1:]))
        self.assertEqual(len(response['attempts']), 1)
        self.assertEqual(response['final']['query_ids'], ['q_1_a', 'q_1_b'])
        self.assertEqual(response['runtime_evaluator_sha256'],
                         runtime_evaluator_digest(response['evaluator_sha256'], response))
        bad = copy.deepcopy(response)
        del bad['trace'][-1]['output']['publication_binding']
        self.assertNotEqual(score(fixture.questions['questions'][0], bad)['answer'], 'correct')

    def test_explicit_once_recomputation_is_cold_reacquires_and_preserves_failed_attempt(self):
        fixture = make_fixture(self, 'once', True)
        status, response, calls = run_fixture(fixture)
        self.assertEqual(status, 0, response.get('error'))
        self.assertEqual(len(response['attempts']), 2)
        root = Path(fixture.args.output)
        first = json.loads((root / '0001-attempt-01/trace.json').read_text())
        self.assertEqual(first['error'], 'PUBLICATION_DRIFT')
        self.assertNotIn('final', first)
        self.assertEqual(first['trace'][-1]['output']['query_id'], 'q_drift_1')
        self.assertEqual(first['trace'][-1]['output']['retryable'], False)
        self.assertEqual(response['final']['query_ids'], ['q_2_a', 'q_2_b'])
        self.assertEqual(response['final']['rows'], [[20], [30]])
        self.assertNotEqual(first['runtime_binding']['publication_binding'],
                            response['runtime_binding']['publication_binding'])
        self.assertEqual(first['runtime_binding']['knowledge_version'], response['runtime_binding']['knowledge_version'])
        self.assertEqual(first['runtime_binding']['policy_version'], response['runtime_binding']['policy_version'])
        self.assertEqual(sum(c['expected'] == 'current' for c in calls), 2)
        for attempt in (1, 2):
            query_inputs = [c['input'] for c in calls if c['acquisitions'] == attempt and c['operation'] == 'query']
            self.assertEqual(len(query_inputs), 2)
            self.assertIn("'a'", query_inputs[0])  # Recompute the first component too; never only replay B.
        agents = [json.loads(line) for line in (fixture.root / 'agents.jsonl').read_text().splitlines()]
        self.assertEqual(len({a['pid'] for a in agents}), 2)
        self.assertEqual(len({a['prompt_sha256'] for a in agents}), 1)
        self.assertEqual(skills.sha((root / '0001-attempt-01/trace.json').read_bytes()),
                         response['attempts'][0]['trace_sha256'])
        summary = json.loads((root / 'results.json').read_text())['summary']
        self.assertEqual(summary['whole_question_recomputations'], 1)
        self.assertEqual(summary['attempt_failures'], {'PUBLICATION_DRIFT': 1})

    def test_default_no_retry_and_enabled_retry_is_capped_at_one(self):
        for scenario, allowed, count in [('once', False, 1), ('always', True, 2)]:
            with self.subTest(scenario=scenario):
                fixture = make_fixture(self, scenario, allowed)
                status, response, calls = run_fixture(fixture)
                self.assertEqual(status, 2)
                self.assertEqual(response['error'], 'PUBLICATION_DRIFT')
                self.assertEqual(len(response['attempts']), count)
                self.assertEqual(sum(c['expected'] == 'current' for c in calls), count)
                self.assertNotIn('final', response)

    def test_auth_input_protocol_and_missing_ack_failures_never_trigger_recomputation(self):
        for scenario in ['auth', 'usage', 'missing', 'missing-later', 'mismatch', 'forged', 'acquire-auth']:
            with self.subTest(scenario=scenario):
                fixture = make_fixture(self, scenario, True)
                status, response, calls = run_fixture(fixture)
                self.assertEqual(status, 2)
                self.assertEqual(len(response['attempts']), 1)
                self.assertEqual(sum(c['expected'] == 'current' for c in calls), 1)
                if scenario == 'missing':
                    self.assertFalse((fixture.root / 'agents.jsonl').exists())
                    self.assertEqual(response['error'], 'PUBLICATION_BINDING_INVALID')

    def test_drift_during_final_verification_discards_the_completed_answer_before_cold_recompute(self):
        fixture = make_fixture(self, 'post-drift', True)
        status, response, _ = run_fixture(fixture)
        self.assertEqual(status, 0, response.get('error'))
        first = json.loads((Path(fixture.args.output) / '0001-attempt-01/trace.json').read_text())
        self.assertEqual(first['error'], 'PUBLICATION_DRIFT')
        self.assertNotIn('final', first)
        self.assertNotIn('final_text', first)
        self.assertTrue(all(not call['error'] for call in first['trace']))
        self.assertEqual(first['binding_checks'][-1]['output']['query_id'], 'q_drift_1')
        self.assertEqual(response['final']['query_ids'], ['q_2_a', 'q_2_b'])

    def test_generic_f22_can_acquire_freshness_without_a_private_skill_or_manifest_pin(self):
        fixture = make_fixture(self)
        fixture.config.pop('agentSkill')
        fixture.config.pop('gatewayManifestSha256')
        status, response, calls = run_fixture(fixture)
        self.assertEqual(status, 0, response.get('error'))
        self.assertEqual(calls[0]['operation'], 'freshness')
        self.assertIsNone(response['prompt_binding']['skill'])

    def test_drift_text_cannot_authorize_retry_after_any_other_failure(self):
        fixture = make_fixture(self, 'once', False)
        _, response, _ = run_fixture(fixture)
        allowed = {'allow_whole_question_retry': True}
        self.assertTrue(retry_allowed(response, allowed, 1))
        for code in ('UNAUTHENTICATED', 'INVALID_ARGUMENT', 'CLI_INVALID_JSON', 'TIMEOUT'):
            bad = copy.deepcopy(response)
            bad['trace'].insert(0, {'operation': 'query', 'error': True, 'output': {'code': code}})
            self.assertFalse(retry_allowed(bad, allowed, 1))
        self.assertFalse(retry_allowed(response, allowed, 2))


if __name__ == '__main__':
    unittest.main()
