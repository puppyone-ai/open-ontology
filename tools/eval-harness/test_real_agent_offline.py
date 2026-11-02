"""Opt-in installed Pi SDK integration; the HTTP endpoint is a local synthetic SSE fixture."""
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
import threading
import unittest

import test_skill_binding as fixtures
from harness import inspect_binding, score


@unittest.skipUnless(os.environ.get('EVAL_OFFLINE_DEMO_DIR'), 'set EVAL_OFFLINE_DEMO_DIR to installed Pi packages')
class RealAgentOfflineTests(unittest.TestCase):
    def test_installed_pi_agent_sends_selected_system_prompt_and_executes_pinned_cli(self):
        fixture = fixtures.SelectedSkillTests()
        fixture.setUp()
        self.addCleanup(fixture.doCleanups)
        payloads = []

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_args):
                pass

            def do_POST(self):
                if self.path != '/v1/chat/completions':
                    self.send_error(404)
                    return
                payload = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                payloads.append(payload)
                turn = len(payloads)
                if turn <= 2:
                    args = ({'operation': 'tables'} if turn == 1 else
                            {'operation': 'query', 'input': 'SELECT COUNT(*) FROM app.items'})
                    delta = {'role': 'assistant', 'tool_calls': [
                        {'index': 0, 'id': f'call_{turn}', 'type': 'function',
                         'function': {'name': 'oo', 'arguments': json.dumps(args)}}]}
                    stop = 'tool_calls'
                else:
                    delta = {'role': 'assistant', 'content': json.dumps({
                        'answer': '3', 'conclusion': 'numeric', 'rows': [[3]], 'query_ids': ['q_actual']})}
                    stop = 'stop'
                chunks = [
                    {'choices': [{'index': 0, 'delta': delta, 'finish_reason': None}]},
                    {'choices': [{'index': 0, 'delta': {}, 'finish_reason': stop}],
                     'usage': {'prompt_tokens': 20, 'completion_tokens': 20, 'total_tokens': 40}},
                ]
                self.send_response(200)
                self.send_header('Content-Type', 'text/event-stream')
                self.end_headers()
                for chunk in chunks:
                    chunk.update(id=f'offline_{turn}', object='chat.completion.chunk',
                                 created=1, model='offline-fixture')
                    self.wfile.write(('data: ' + json.dumps(chunk) + '\n\n').encode())
                self.wfile.write(b'data: [DONE]\n\n')
                self.wfile.flush()

        server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            config = fixture.config
            config.update(demoDir=os.environ['EVAL_OFFLINE_DEMO_DIR'], model='offline-fixture',
                          modelEndpoint=f'http://127.0.0.1:{server.server_port}/v1',
                          apiKeyEnv='OFFLINE_FIXTURE_MODEL_KEY')
            config.pop('bearerFile')
            binding = inspect_binding(config)
            process = subprocess.run(
                ['node', str(Path(__file__).with_name('agent.mjs'))],
                input=json.dumps({'config': config, 'text': 'Count synthetic fixture items',
                                  'expected_binding': binding}),
                env={**os.environ, 'OFFLINE_FIXTURE_MODEL_KEY': 'synthetic-not-a-secret'},
                capture_output=True, text=True, timeout=30)
            self.assertEqual(process.returncode, 0, 'offline Agent process failed')
            response = json.loads(process.stdout)
            self.assertIsNone(response.get('error'), response.get('error'))
            self.assertEqual(len(payloads), 3)
            for payload in payloads:
                system = [message for message in payload['messages'] if message['role'] == 'system']
                self.assertEqual(len(system), 1)
                self.assertTrue(system[0]['content'].endswith(fixture.skill_text))
                self.assertEqual(hashlib.sha256(system[0]['content'].encode()).hexdigest(),
                                 response['prompt_binding']['system_prompt_sha256'])
            self.assertEqual(response['final']['query_ids'], ['q_actual'])
            self.assertEqual(score(fixture.questions['questions'][0], response)['answer'], 'correct')
            self.assertTrue(all(request.get('system_prompt_sha256') == binding['system_prompt_sha256']
                                for request in response['model_requests']))
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=5)


if __name__ == '__main__':
    unittest.main()
