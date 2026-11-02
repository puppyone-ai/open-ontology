"""Installed Pi SDK + synthetic loopback model + synthetic CLI; never a real-model test."""
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
import threading
import unittest
from unittest.mock import patch

from test_publication_binding import make_fixture, run_fixture


def installed_pi_run(test, scenario, retry):
    fixture = make_fixture(test, scenario, retry)
    wire = []

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_POST(self):
            if self.path != '/v1/chat/completions':
                self.send_error(404)
                return
            payload = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            messages = payload['messages']
            tools = [json.loads(message['content']) for message in messages if message['role'] == 'tool']
            system = next(message['content'] for message in messages if message['role'] == 'system')
            wire.append({'tool_history_count': len(tools), 'system_prompt_sha256':
                         hashlib.sha256(system.encode()).hexdigest()})
            if len(tools) < 3:
                args = ({'operation': 'tables'} if not tools else {'operation': 'query',
                        'input': "SELECT COUNT(*) FROM app.items WHERE part='" + ('a' if len(tools) == 1 else 'b') + "'"})
                delta = {'role': 'assistant', 'tool_calls': [
                    {'index': 0, 'id': f'call_{len(tools)}', 'type': 'function',
                     'function': {'name': 'oo', 'arguments': json.dumps(args)}}]}
                stop = 'tool_calls'
            else:
                queries = [output for output in tools if output.get('query_id') and not output.get('code')]
                delta = {'role': 'assistant', 'content': json.dumps({
                    'answer': 'synthetic only', 'conclusion': 'grouped',
                    'rows': [row for output in queries for row in output['rows']],
                    'query_ids': [output['query_id'] for output in queries]})}
                stop = 'stop'
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.end_headers()
            for chunk in [
                {'choices': [{'index': 0, 'delta': delta, 'finish_reason': None}]},
                {'choices': [{'index': 0, 'delta': {}, 'finish_reason': stop}],
                 'usage': {'prompt_tokens': 20, 'completion_tokens': 20, 'total_tokens': 40}},
            ]:
                chunk.update(id='offline-f22', object='chat.completion.chunk', created=1, model='offline-f22')
                self.wfile.write(('data: ' + json.dumps(chunk) + '\n\n').encode())
            self.wfile.write(b'data: [DONE]\n\n')
            self.wfile.flush()

    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    fixture.config.update(demoDir=os.environ['EVAL_OFFLINE_DEMO_DIR'], model='offline-f22',
                          modelEndpoint=f'http://127.0.0.1:{server.server_port}/v1',
                          apiKeyEnv='OFFLINE_F22_KEY')
    fixture.config.pop('bearerFile')
    try:
        with patch.dict(os.environ, {'OFFLINE_F22_KEY': 'synthetic-only'}):
            status, response, calls = run_fixture(fixture)
        return fixture, status, response, calls, wire
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


@unittest.skipUnless(os.environ.get('EVAL_OFFLINE_DEMO_DIR'), 'set EVAL_OFFLINE_DEMO_DIR to installed Pi packages')
class InstalledPiPublicationTests(unittest.TestCase):
    def test_data_only_drift_restarts_all_components_with_cold_wire_history(self):
        fixture, status, response, calls, wire = installed_pi_run(self, 'once', True)
        self.assertEqual(status, 0, response.get('error'))
        self.assertEqual([item['tool_history_count'] for item in wire], [0, 1, 2, 0, 1, 2, 3])
        self.assertEqual(len({item['system_prompt_sha256'] for item in wire}), 1)
        self.assertEqual(wire[0]['system_prompt_sha256'], response['prompt_binding']['system_prompt_sha256'])
        self.assertEqual(response['final']['query_ids'], ['q_2_a', 'q_2_b'])
        self.assertEqual(response['final']['rows'], [[20], [30]])
        self.assertEqual(sum(call['expected'] == 'current' for call in calls), 2)
        self.assertEqual(response['attempts'][0]['error'], 'PUBLICATION_DRIFT')
        self.assertTrue((fixture.root / 'run/0001-attempt-01/trace.json').exists())

    def test_default_no_retry_and_repeated_drift_never_start_a_third_agent(self):
        for scenario, retry, histories in [('once', False, [0, 1, 2]),
                                          ('always', True, [0, 1, 2, 0, 1, 2])]:
            with self.subTest(scenario=scenario):
                _, status, response, _, wire = installed_pi_run(self, scenario, retry)
                self.assertEqual(status, 2)
                self.assertEqual(response['error'], 'PUBLICATION_DRIFT')
                self.assertEqual([item['tool_history_count'] for item in wire], histories)
                self.assertNotIn('final', response)

    def test_auth_input_and_missing_receipt_never_cause_an_automatic_reacquisition(self):
        for scenario in ('auth', 'usage', 'missing'):
            with self.subTest(scenario=scenario):
                _, status, response, calls, wire = installed_pi_run(self, scenario, True)
                self.assertEqual(status, 2)
                self.assertEqual(len(response['attempts']), 1)
                self.assertEqual(sum(c['expected'] == 'current' for c in calls), 1)
                self.assertEqual([w['tool_history_count'] for w in wire], [] if scenario == 'missing' else [0, 1, 2])


if __name__ == '__main__':
    unittest.main()
