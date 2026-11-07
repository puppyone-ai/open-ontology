from http.server import BaseHTTPRequestHandler, HTTPServer
import http.client
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import threading
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from model_egress import Egress, Refusal, read_key, server, validate_url

FIXTURES = Path(__file__).parent / 'fixtures'


class Replay:
    def __init__(self, dialect='openai', status=200, error=None, truncate=False, wire=None, models=None):
        self.seen = []
        self.dialect, self.status, self.error, self.truncate = dialect, status, error, truncate
        replay = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass

            def do_GET(self):
                replay.seen.append((self.path, None, dict(self.headers)))
                body = json.dumps({'data': models if models is not None else [
                    {'id': 'fixture-model', 'context_window': 272000,
                     'max_output_tokens': 128000}]}).encode()
                self.send_response(200)
                self.end_headers()
                self.wfile.write(body)

            def do_POST(self):
                request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                replay.seen.append((self.path, request, dict(self.headers)))
                stream = request.get('stream', False)
                if replay.error:
                    body = json.dumps(replay.error).encode()
                else:
                    fixture = FIXTURES / (replay.dialect + ('.sse.json' if stream else '.json'))
                    body = json.loads(fixture.read_text()).encode() if stream else fixture.read_bytes()
                    if replay.truncate:
                        body = body.split(b'data: {"object":"chat.completion.chunk","choices":[{"index":0,"delta":{}')[0] if replay.dialect == 'openai' else body.split(b'event: message_delta')[0]
                    if wire is not None:
                        body = wire
                self.send_response(replay.status)
                self.send_header('Content-Type', 'text/event-stream' if stream else 'application/json')
                self.send_header('Content-Length', str(len(body)))
                self.end_headers()
                # Fragment at inconvenient transport boundaries.
                for offset in range(0, len(body), 7):
                    self.wfile.write(body[offset:offset + 7])

        self.httpd = HTTPServer(('127.0.0.1', 0), Handler)
        self.thread = threading.Thread(target=self.httpd.serve_forever, daemon=True)
        self.thread.start()

    def close(self):
        self.httpd.shutdown()
        self.httpd.server_close()
        self.thread.join()


class Tests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.secrets = self.root / 'secrets'
        self.secrets.mkdir(mode=0o700)
        self.key = 'synthetic-fixture-key-not-a-credential'
        self.path = self.secrets / 'model-key'
        self.path.write_text(self.key + '\n')
        self.path.chmod(0o600)
        self.resources = []

    def tearDown(self):
        for resource in reversed(self.resources):
            resource.close()
        self.tmp.cleanup()

    def egress(self, dialect='openai', **kwargs):
        replay = Replay(dialect, **kwargs)
        self.resources.append(replay)
        model = {'provider': 'openai-compatible' if dialect == 'openai' else 'anthropic',
                 'base_url': f'http://127.0.0.1:{replay.httpd.server_port}/workflow/v1',
                 'model': 'fixture-model', 'key': {'secret_ref': 'model-key'}}
        meter = self.root / f'meter-{len(self.resources)}.jsonl'
        egress = Egress(model, meter, secret_root=self.secrets)
        self.resources.append(egress)
        return egress, replay, meter

    def request(self, stream=False):
        return {'model': 'fixture-model', 'messages': [{'role': 'system', 'content': 'Be brief.'},
                    {'role': 'user', 'content': 'OK'}], 'max_tokens': 1, 'stream': stream}

    def test_endpoint_reference_is_private_and_validated_after_resolution(self):
        original, replay, _ = self.egress()
        endpoint = self.secrets / 'model-endpoint'
        endpoint.write_text(original.model['base_url'])
        endpoint.chmod(0o600)
        model = dict(original.model, base_url={'secret_ref': 'model-endpoint'})
        resolved = Egress(model, None, secret_root=self.secrets)
        self.resources.append(resolved)
        self.assertEqual(resolved.url.port, replay.httpd.server_port)
        self.assertEqual(resolved.model['base_url'], {'secret_ref': 'model-endpoint'})
        endpoint.write_text('http://not-loopback.example/v1')
        with self.assertRaisesRegex(Refusal, 'CONFIG_ENDPOINT'):
            Egress(model, None, secret_root=self.secrets)
        endpoint.chmod(0o644)
        with self.assertRaisesRegex(Refusal, 'SECRET_PERMISSIONS'):
            Egress(model, None, secret_root=self.secrets)

    def test_probe_cli_accepts_piped_config_without_persistent_meter(self):
        egress, _, _ = self.egress()
        home = self.root / 'home'
        store = home / '.config/open-ontology/secrets'
        store.mkdir(parents=True, mode=0o700)
        path = store / 'model-key'
        path.write_text(self.key)
        path.chmod(0o600)
        command = [sys.executable, str(Path(__file__).resolve().parents[1] / 'model_egress.py'),
                   '--config', '-', '--probe']
        result = subprocess.run(command, input=json.dumps({'model': egress.model}),
                                text=True, capture_output=True,
                                env=dict(os.environ, HOME=str(home)), timeout=10)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        report = json.loads(result.stdout)
        self.assertEqual(report['context_window'], 272000)
        self.assertEqual(report['proxy_injected_prompt_tokens'], 2503)
        self.assertNotIn(self.key, result.stdout + result.stderr)
        self.assertEqual(list(store.iterdir()), [path])
        refused = subprocess.run(command[:-1], input='{}', text=True,
                                 capture_output=True, timeout=10)
        self.assertEqual(refused.returncode, 2)
        self.assertEqual(json.loads(refused.stdout)['error']['code'], 'METER_REQUIRED')

    def test_nonstream_both_dialects_and_usage(self):
        for dialect, tokens in [('openai', 2503), ('anthropic', 17)]:
            egress, replay, meter = self.egress(dialect)
            output = list(egress.call(self.request()))[0]
            self.assertEqual(output['choices'][0]['message']['content'], 'OK')
            row = json.loads(meter.read_text())
            self.assertEqual(row['prompt_tokens'], tokens)
            self.assertTrue(row['usage_measured'])
            self.assertNotIn(self.key, meter.read_text())
            self.assertEqual(stat.S_IMODE(meter.stat().st_mode), 0o600)
            path, sent, headers = replay.seen[0]
            self.assertEqual(path, '/workflow/v1/' + ('chat/completions' if dialect == 'openai' else 'messages'))
            if dialect == 'anthropic':
                self.assertEqual(sent['system'], 'Be brief.')
                self.assertEqual(headers['x-api-key'], self.key)
                self.assertNotIn('Authorization', headers)
            else:
                self.assertEqual(headers['Authorization'], 'Bearer ' + self.key)

    def test_stream_both_dialects(self):
        for dialect in ('openai', 'anthropic'):
            egress, _, meter = self.egress(dialect)
            output = list(egress.call(self.request(True)))
            self.assertIsNone(output[-1])
            self.assertEqual(''.join(c.get('delta', {}).get('content', '')
                for event in output if event for c in event.get('choices', [])), 'OK')
            row = json.loads(meter.read_text())
            self.assertEqual(row['completion_tokens'], 1)
            self.assertEqual(row['code'], 'OK')

    def test_unified_errors_fail_fast_and_do_not_echo(self):
        for status, upstream, expected in [(429, 'rate_limit', 'RATE_LIMITED'),
                    (400, 'usage_limit_reached', 'USAGE_LIMIT'), (503, 'cooldown', 'COOLDOWN'),
                    (200, 'insufficient_quota', 'USAGE_LIMIT')]:
            egress, replay, meter = self.egress(status=status,
                    error={'error': {'code': upstream, 'message': self.key}})
            for _ in range(2):
                with self.assertRaises(Refusal) as error:
                    list(egress.call(self.request()))
                self.assertEqual(error.exception.code, expected)
            self.assertEqual(len(replay.seen), 1)
            self.assertNotIn(self.key, meter.read_text())
            self.assertEqual(len(meter.read_text().splitlines()), 2)

    def test_rate_limit_and_truncated_stream(self):
        egress, replay, _ = self.egress()
        egress.rpm = 1
        list(egress.call(self.request()))
        with self.assertRaisesRegex(Refusal, 'RATE_LIMITED'):
            list(egress.call(self.request()))
        self.assertEqual(len(replay.seen), 1)
        for dialect in ('openai', 'anthropic'):
            egress, _, meter = self.egress(dialect, truncate=True)
            with self.assertRaisesRegex(Refusal, 'STREAM_INCOMPLETE'):
                list(egress.call(self.request(True)))
            self.assertEqual(json.loads(meter.read_text())['code'], 'STREAM_INCOMPLETE')

    def test_probe_distinguishes_metadata_and_one_token_json_acceptance(self):
        egress, replay, meter = self.egress()
        output = egress.probe()
        self.assertEqual(output['context_window'], 272000)
        self.assertEqual(output['max_output_tokens'], 128000)
        self.assertEqual(output['proxy_injected_prompt_tokens'], 2503)
        self.assertEqual(output['json_mode'], 'accepted_parameter_output_not_verified_at_one_token')
        self.assertEqual(len(replay.seen), 3)
        self.assertEqual(len(meter.read_text().splitlines()), 2)

    def test_finish_frame_completes_without_terminal_marker(self):
        for dialect, marker in [('openai', b'data: [DONE]'), ('anthropic', b'event: message_stop')]:
            wire = json.loads((FIXTURES / (dialect + '.sse.json')).read_text()).encode().split(marker)[0]
            egress, _, meter = self.egress(dialect, wire=wire)
            self.assertIsNone(list(egress.call(self.request(True)))[-1])
            self.assertEqual(json.loads(meter.read_text())['code'], 'OK')

    def test_probe_exact_then_channel_minimum_and_unknown_fallbacks(self):
        channels = [
            {'id': 'cx/fixture-model', 'context_length': 1050000, 'max_completion_tokens': 128000},
            {'id': 'cb/fixture-model', 'context_window': 272000, 'max_output_tokens': 64000},
            {'id': 'cb/fixture-model-other', 'context_window': 1, 'max_output_tokens': 1}]
        for models, expected, match in [
            (channels, (272000, 64000), 'last_segment'),
            (channels + [{'id': 'fixture-model', 'context_window': 500000, 'max_output_tokens': 100000}],
             (500000, 100000), 'exact'),
            ([], (32000, 4000), 'none'),
            (channels + [{'id': 'unknown/fixture-model', 'context_window': True}],
             (32000, 4000), 'last_segment')]:
            egress, _, _ = self.egress(models=models)
            output = egress.probe()
            self.assertEqual((output['context_window'], output['max_output_tokens']), expected)
            self.assertEqual(output['model_match'], match)
            self.assertEqual(output['proxy_injected_prompt_tokens'], 2503)
            self.assertEqual(len(output['matched_models']),
                             1 if match == 'exact' else len(models) - 1 if models else 0)
        egress, _, _ = self.egress(models=[])
        egress.model['context_window'] = 16000
        output = egress.probe()
        self.assertEqual(output['context_window'], 16000)
        self.assertEqual(output['limits_basis']['context_window'], 'stack_fallback')
        self.assertEqual(output['limits_basis']['max_output_tokens'], 'conservative_default')

    def test_finish_then_separate_usage_and_missing_usage(self):
        finish = b'data: {"choices":[{"finish_reason":"length","delta":{}}]}\n\n'
        counts = b'data: {"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":2}}\n\n'
        for suffix in (b'', b'data: [DONE]\n\n'):
            egress, _, meter = self.egress(wire=finish + counts + suffix)
            self.assertIsNone(list(egress.call(self.request(True)))[-1])
            self.assertEqual(json.loads(meter.read_text())['prompt_tokens'], 10)
        egress, _, meter = self.egress(wire=finish)
        with self.assertRaisesRegex(Refusal, 'STREAM_USAGE_MISSING'):
            list(egress.call(self.request(True)))
        self.assertFalse(json.loads(meter.read_text())['usage_measured'])

    def test_secret_modes_symlinks_fifo_and_names(self):
        self.assertEqual(read_key('model-key', self.secrets), self.key)
        with patch('model_egress.os.geteuid', return_value=os.geteuid() + 1):
            with self.assertRaisesRegex(Refusal, 'SECRET_PERMISSIONS'):
                read_key('model-key', self.secrets)
        for name in ('../model-key', '/model-key', '', 'bad.name'):
            with self.assertRaises(Refusal):
                read_key(name, self.secrets)
        self.path.chmod(0o644)
        with self.assertRaises(Refusal):
            read_key('model-key', self.secrets)
        self.path.chmod(0o600)
        (self.secrets / 'link').symlink_to(self.path)
        with self.assertRaises(OSError):
            read_key('link', self.secrets)
        os.mkfifo(self.secrets / 'fifo', 0o600)
        with self.assertRaises(Refusal):
            read_key('fifo', self.secrets)
        self.secrets.chmod(0o755)
        with self.assertRaises(Refusal):
            read_key('model-key', self.secrets)

    def test_symlinked_ancestor_but_not_secret_directory_is_accepted(self):
        alias = self.root / 'alias'
        alias.symlink_to(self.root, target_is_directory=True)
        self.assertEqual(read_key('model-key', alias / 'secrets'), self.key)
        leaf = self.root / 'private-link'
        leaf.symlink_to(self.secrets, target_is_directory=True)
        with self.assertRaises(OSError):
            read_key('model-key', leaf)

    def test_url_and_listener_guards(self):
        for url in ('http://example.com/v1', 'https://user:pass@example.com',
                    'https://example.com?key=x', 'http://127.0.0.1.evil/v1'):
            with self.assertRaises(Refusal):
                validate_url(url)
        egress, _, _ = self.egress()
        with self.assertRaisesRegex(Refusal, 'LISTEN_NOT_LOOPBACK'):
            server(egress, '0.0.0.0', 0)

    def test_live_loopback_http_transport_nonstream_stream_and_browser_guard(self):
        egress, replay, _ = self.egress()
        httpd = server(egress, '127.0.0.1', 0)
        thread = threading.Thread(target=httpd.serve_forever, daemon=True)
        thread.start()
        try:
            for stream in (False, True):
                conn = http.client.HTTPConnection('127.0.0.1', httpd.server_port, timeout=3)
                conn.request('POST', '/v1/chat/completions', json.dumps(self.request(stream)),
                             {'Authorization': 'Bearer local-value-never-forwarded'})
                response = conn.getresponse()
                self.assertEqual(response.status, 200)
                body = response.read()
                self.assertNotIn(self.key.encode(), body)
                self.assertIn(b'[DONE]' if stream else b'chat.completion', body)
                conn.close()
            conn = http.client.HTTPConnection('127.0.0.1', httpd.server_port, timeout=3)
            conn.request('POST', '/v1/chat/completions', '{}', {'Origin': 'https://evil.example'})
            response = conn.getresponse()
            self.assertEqual(response.status, 400)
            response.read()
            conn.close()
            self.assertEqual(len(replay.seen), 2)
        finally:
            httpd.shutdown()
            httpd.server_close()
            thread.join()

    def test_unsupported_anthropic_features_fail_before_call(self):
        egress, replay, _ = self.egress('anthropic')
        request = self.request()
        request['tools'] = []
        with self.assertRaisesRegex(Refusal, 'UNSUPPORTED_PARAMETER'):
            list(egress.call(request))
        self.assertEqual(replay.seen, [])

    def test_anthropic_quota_and_overload_are_sanitized(self):
        for label, expected in [('overloaded_error', 'COOLDOWN'),
                                ('usage_limit_reached', 'USAGE_LIMIT')]:
            egress, replay, meter = self.egress('anthropic', status=529,
                error={'type': 'error', 'error': {'type': label, 'message': self.key}})
            with self.assertRaises(Refusal) as error:
                list(egress.call(self.request()))
            self.assertEqual(error.exception.code, expected)
            self.assertNotIn(self.key, meter.read_text())
            self.assertEqual(len(replay.seen), 1)

    def test_success_response_cannot_echo_key(self):
        egress, _, meter = self.egress()
        with self.assertRaisesRegex(Refusal, 'SECRET_ECHO'):
            egress.safe({'choices': [{'message': {'content': self.key}}]})
        self.assertNotIn(self.key, meter.read_text())


if __name__ == '__main__':
    unittest.main()
