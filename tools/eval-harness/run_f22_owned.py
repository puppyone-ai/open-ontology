"""Installed Pi + real CLI/Gateway/owned PG; the model is a deterministic SSE fixture."""
import argparse
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
import threading

import harness


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--runtime', required=True)
    parser.add_argument('--output', required=True)
    parser.add_argument('--demo-dir', required=True)
    parser.add_argument('--skill', required=True)
    parser.add_argument('--scenario', choices=['same', 'once', 'no_retry', 'repeated', 'auth', 'input'],
                        required=True)
    args = parser.parse_args()
    os.umask(0o077)
    output = harness.private_output(args.output)
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    runtime_path = Path(args.runtime).resolve()
    runtime = json.loads(runtime_path.read_text())
    if runtime.get('protocol') != 'oo.f22.owned-runtime/v1':
        raise ValueError('owned_runtime_required')
    product = Path(__file__).resolve().parents[2]
    hook = product / 'crates/oo-gateway/tests/f22_runtime.py'

    def control(action, *extra):
        call = subprocess.run(['python3', str(hook), action, str(runtime_path), *map(str, extra)],
                              text=True, capture_output=True, timeout=30)
        if call.returncode:
            raise RuntimeError('owned_runtime_control_failed')
        return json.loads(call.stdout)

    state = control('status')
    if not state['pg_running'] or not state['gateway_running']:
        raise ValueError('owned_runtime_not_ready')
    cli = Path(runtime['cli_binary'])
    if digest(cli) != runtime['cli_binary_sha256']:
        raise ValueError('owned_cli_drift')
    control('data-only', 10)
    manifest_call = subprocess.run(
        [str(cli), 'knowledge', 'cat', 'manifest.json', '--json', '--expected-publication-id',
         'current', *runtime['connection_args']], text=True, capture_output=True, timeout=35)
    if manifest_call.returncode:
        raise RuntimeError('owned_manifest_failed')
    manifest = json.loads(manifest_call.stdout)
    manifest_sha = hashlib.sha256(manifest['text'].encode()).hexdigest()
    if manifest['sha256'] != manifest_sha or not manifest.get('publication_binding'):
        raise ValueError('owned_manifest_binding')
    (output / 'initial-manifest.json').write_text(json.dumps(manifest))

    skill = Path(args.skill).resolve()
    skill_bytes = skill.read_bytes()
    for part in ['delivery', 'installed']:
        (output / part).mkdir(mode=0o700)
        (output / part / 'SKILL.md').write_bytes(skill_bytes)
    token = output / 'reader.token'
    token.write_bytes(Path(runtime['token_file']).read_bytes())
    wire, mutations, server_errors = [], [], []
    scenario = args.scenario

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_POST(self):
            if self.path != '/v1/chat/completions':
                self.send_error(404)
                return
            try:
                payload = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                tools = [json.loads(m['content']) for m in payload['messages'] if m['role'] == 'tool']
                system = next(m['content'] for m in payload['messages']
                              if m['role'] in ['system', 'developer'])
                wire.append({'tool_history_count': len(tools),
                             'system_prompt_sha256': hashlib.sha256(system.encode()).hexdigest()})
                if len(tools) < 3:
                    if len(tools) == 2:
                        if scenario in ['once', 'no_retry', 'repeated'] and (
                                scenario == 'repeated' or not mutations):
                            mutations.append(control('data-only', 20 + len(mutations) * 10))
                        elif scenario == 'auth':
                            token.write_text('intentionally-invalid-owned-fixture-token')
                    command = ({'operation': 'tables'} if not tools else
                               {'operation': 'query', 'input': 'SELECT amount FROM app.items'})
                    if len(tools) == 2 and scenario == 'input':
                        command['input'] = 'SELECT FROM'
                    delta = {'role': 'assistant', 'tool_calls': [
                        {'index': 0, 'id': f'call_{len(tools)}', 'type': 'function',
                         'function': {'name': 'oo', 'arguments': json.dumps(command)}}]}
                    stop = 'tool_calls'
                else:
                    queries = [t for t in tools if t.get('query_id') and 'rows' in t]
                    delta = {'role': 'assistant', 'content': json.dumps({
                        'answer': 'Owned fixture consistency check only.', 'conclusion': 'grouped',
                        'rows': [row for q in queries for row in q['rows']],
                        'query_ids': [q['query_id'] for q in queries]})}
                    stop = 'stop'
                self.send_response(200)
                self.send_header('Content-Type', 'text/event-stream')
                self.end_headers()
                for item in [
                    {'choices': [{'index': 0, 'delta': delta, 'finish_reason': None}]},
                    {'choices': [{'index': 0, 'delta': {}, 'finish_reason': stop}],
                     'usage': {'prompt_tokens': 20, 'completion_tokens': 20, 'total_tokens': 40}},
                ]:
                    item.update(id='f22-owned-runtime', object='chat.completion.chunk',
                                created=1, model='f22-fixture')
                    self.wfile.write(('data: ' + json.dumps(item) + '\n\n').encode())
                self.wfile.write(b'data: [DONE]\n\n')
                self.wfile.flush()
            except Exception as error:
                server_errors.append(type(error).__name__)
                self.send_error(500)

    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    retry = scenario in ['once', 'repeated', 'auth', 'input']
    config = {
        'demoDir': str(Path(args.demo_dir).resolve()), 'cliCommand': [str(cli)],
        'gateway': runtime['origin'], 'transport': 'local-development', 'tokenFile': str(token),
        'model': 'f22-fixture', 'modelEndpoint': f'http://127.0.0.1:{server.server_port}/v1',
        'apiKeyEnv': 'OO_F22_SYNTHETIC_MODEL_KEY', 'queryAccess': 'gateway_readonly',
        'gatewayManifestSha256': manifest_sha,
        'publicationBinding': {'mode': 'f22', 'allowWholeQuestionRetry': retry},
        'agentSkill': {'source': str(skill), 'delivery': str(output / 'delivery/SKILL.md'),
                       'installed': str(output / 'installed/SKILL.md'),
                       'sha256': hashlib.sha256(skill_bytes).hexdigest(),
                       'cli': {'declaredPath': '/opt/data/bin/oo', 'sha256': digest(cli)}},
    }
    questions = {'questions': [{
        'id': 'OWNED-F22-01', 'kind': 'grouped', 'expected_tables': ['app.items'],
        'text': 'Run both component reads against one published version; owned fixture only.',
        'expected_rows': [['20'], ['20']] if scenario == 'once' else [['10'], ['10']],
    }]}
    for name, data in [('config', config), ('questions', questions)]:
        (output / (name + '.json')).write_text(json.dumps(data, ensure_ascii=False, indent=2) + '\n')
    try:
        env = dict(os.environ, OO_F22_SYNTHETIC_MODEL_KEY='synthetic-fixture-only')
        with (output / 'harness.stdout').open('w') as out, (output / 'harness.stderr').open('w') as err:
            process = subprocess.run(
                ['python3', str(Path(__file__).with_name('harness.py')), 'run',
                 '--config', str(output / 'config.json'), '--questions', str(output / 'questions.json'),
                 '--output', str(output / 'run'), '--concurrency', '1'],
                env=env, stdout=out, stderr=err, timeout=180)
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)
        token.write_bytes(Path(runtime['token_file']).read_bytes())
        (output / 'wire.json').write_text(json.dumps(wire, indent=2) + '\n')
        (output / 'mutations.json').write_text(json.dumps(mutations, indent=2) + '\n')
    response = json.loads((output / 'run/0001-trace.json').read_text())
    attempts = response['attempts']
    expected_exit = 0 if scenario in ['same', 'once'] else 2
    expected_attempts = 2 if scenario in ['once', 'repeated'] else 1
    checks = {
        'harness_exit': process.returncode == expected_exit,
        'attempt_count': len(attempts) == expected_attempts,
        'no_server_errors': not server_errors,
        'actual_skill_on_wire': bool(wire) and all(w['system_prompt_sha256'] ==
            response['prompt_binding']['system_prompt_sha256'] for w in wire),
    }
    if scenario in ['same', 'once']:
        final = response['final']
        checks['rows'] = harness.canonical(final['rows']) == harness.canonical(
            questions['questions'][0]['expected_rows'])
        successful = [c['output'] for c in response['trace']
                      if c.get('operation') == 'query' and not c.get('error')]
        checks['two_actual_queries'] = len(successful) == 2 and len({q['query_id'] for q in successful}) == 2
        checks['qid_grounding'] = final['query_ids'] == [q['query_id'] for q in successful]
        checks['same_publication'] = len({json.dumps(q['publication_binding'], sort_keys=True)
                                          for q in successful}) == 1
    elif scenario == 'input':
        # A SQL syntax error can be repaired within the same bound question.
        # It must not trigger whole-question retry or certify a partial answer.
        results = json.loads((output / 'run/results.json').read_text())
        checks['failed_input_preserved'] = any(c.get('error') and
            c.get('output', {}).get('code') == 'SQL_PARSE_ERROR' for c in response['trace'])
        checks['partial_not_accepted_as_complete'] = results['results'][0]['answer'] != 'correct'
    else:
        checks['no_final_answer'] = 'final' not in response
    if expected_attempts == 2:
        first = json.loads((output / 'run/0001-attempt-01/trace.json').read_text())
        second = json.loads((output / 'run/0001-attempt-02/trace.json').read_text())
        checks['cold_recompute'] = [w['tool_history_count'] for w in wire].count(0) == 2
        checks['new_binding'] = first['runtime_binding']['publication_binding'] != second[
            'runtime_binding']['publication_binding']
        old_ids = {c['output']['query_id'] for c in first['trace']
                   if c.get('operation') == 'query' and not c.get('error') and c['output'].get('query_id')}
        checks['old_results_not_delivered'] = bool(old_ids) and not old_ids & set(
            response.get('final', {}).get('query_ids', []))
        checks['data_only_keeps_semantic_versions'] = all(
            first['runtime_binding'][key] == second['runtime_binding'][key]
            for key in ['policy_version', 'knowledge_version'])
    receipt = {
        'scenario': scenario, 'passed': all(checks.values()), 'checks': checks,
        'harness_exit': process.returncode, 'actual_cli_gateway_pg': True,
        'model_kind': 'deterministic SSE fixture, not real LLM',
        'customer_source_access': False, 'runtime': str(runtime_path),
        'cli_sha256': digest(cli), 'hook_sha256': digest(hook), 'runner_sha256': digest(__file__),
        'attempt_errors': [a['error'] for a in attempts], 'model_requests': len(wire),
    }
    (output / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps(receipt))
    return 0 if receipt['passed'] else 2


if __name__ == '__main__':
    raise SystemExit(main())
