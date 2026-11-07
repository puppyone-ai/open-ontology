import io
import json
import os
import unittest
import urllib.error
from unittest.mock import patch

import model_provider as provider


class ModelProviderTest(unittest.TestCase):
    config = {"endpoint": "https://model.example/v1", "model": "fixture", "api_key_env": "TEST_MODEL_KEY"}

    def test_explicit_quota_cooldown_and_auth_errors_do_not_blindly_retry(self):
        for token in ['usage_limit_reached', 'insufficient_quota', 'model_cooldown']:
            error = provider.http_failure(429, json.dumps({
                'error': {'type': token, 'message': 'private-account-detail'}}).encode())
            self.assertFalse(error.retryable)
            self.assertTrue(error.stops_run)
            self.assertEqual(str(error), 's5_provider_' + token)
            self.assertNotIn('private-account', str(error))
        for status in [401, 403]:
            error = provider.http_failure(status, b'private credential details')
            self.assertFalse(error.retryable)
            self.assertTrue(error.stops_run)
        rate_limit = provider.http_failure(429, b'{"error":{"type":"temporary"}}')
        self.assertFalse(rate_limit.retryable)
        self.assertTrue(rate_limit.stops_run)
        for status in [408, 500, 503]:
            error = provider.http_failure(status, b'{"error":{"type":"temporary"}}')
            self.assertTrue(error.retryable)
        for body in [b'[]', b'{"error":[]}', b'{"error":{"code":[]}}', b'not-json']:
            self.assertEqual(str(provider.http_failure(400, body)), 's5_provider_http_400')

    def test_actual_s5_http_error_classification_reaches_the_runner(self):
        import library_selfbuild as lib
        error = urllib.error.HTTPError('https://model.example', 429, 'private', {},
                                      io.BytesIO(b'{"error":{"type":"usage_limit_reached"}}'))
        with patch.object(lib, 'draft_request', return_value=({'model': 'fixture'}, b'{}')), \
                patch.object(provider, 'credential', return_value='private-test-key'), \
                patch.object(provider, 'open_request', side_effect=error):
            with self.assertRaises(provider.ProviderFailure) as caught:
                lib.call_model({'tables': [{'dataset': 'app.items'}]}, self.config)
        self.assertFalse(caught.exception.retryable)
        self.assertEqual(str(caught.exception), 's5_provider_usage_limit_reached')

    def test_stream_quota_failure_retains_partial_bytes_and_stops_retry(self):
        raw = (b'data: {"choices":[{"delta":{"content":"partial"}}]}\n\n'
               b'data: {"error":{"code":"model_cooldown","message":"private-detail"}}\n\n')
        with self.assertRaises(provider.CompletionStreamError) as caught:
            provider.read_completion_stream(io.BytesIO(raw))
        self.assertEqual(str(caught.exception), 's5_provider_model_cooldown')
        self.assertEqual(caught.exception.raw_response, raw)
        self.assertFalse(caught.exception.retryable)
        self.assertTrue(caught.exception.stops_run)
        self.assertNotIn('private-detail', str(caught.exception))

    def test_sse_requires_a_terminal_frame_and_bounded_bytes(self):
        frames = [
            {'model': 'fixture', 'choices': [{'index': 0, 'delta': {'role': 'assistant'}}]},
            {'choices': [{'index': 0, 'delta': {'content': '{"datasets":'}}]},
            {'choices': [{'index': 0, 'delta': {'content': '[]}'}}]},
            {'choices': [{'index': 0, 'delta': {}, 'finish_reason': 'stop'}]},
            {'choices': [], 'usage': {'completion_tokens': 10}},
        ]
        body = b': keepalive\n\n' + b''.join(
            b'data: ' + json.dumps(frame).encode() + b'\n\n' for frame in frames) + b'data: [DONE]\n\n'
        result, raw = provider.read_completion_stream(io.BytesIO(body))
        self.assertEqual(result['choices'][0]['message']['content'], '{"datasets":[]}')
        self.assertEqual(result['usage']['completion_tokens'], 10)
        self.assertEqual(raw, body)
        eof, _ = provider.read_completion_stream(io.BytesIO(body.replace(b'data: [DONE]\n\n', b'')))
        self.assertEqual(eof['stream_terminal'], 'finish_frame_eof')
        for invalid in [b'data: {"choices":[{"delta":{"content":"partial"}}]}\n\n',
                        b'data: [DONE]\n\n',
                        b'data: {"error":{"message":"secret"}}\n\n',
                        b'data: {"choices":[{"delta":{"tool_calls":[{}]}}]}\n\n',
                        b'data: {"choices":[{"delta":{"content":12}}]}\n\n']:
            with self.assertRaisesRegex(ValueError, 's5_stream_'):
                provider.read_completion_stream(io.BytesIO(invalid))
        with self.assertRaisesRegex(ValueError, 's5_response_budget'):
            provider.read_completion_stream(io.BytesIO(body), max_bytes=10)

    def test_explicit_alias_probe_can_verify_generation_without_a_listing_entry(self):
        replies = [io.BytesIO(b'{"data":[{"id":"other"}]}'),
                   io.BytesIO(b'{"choices":[{"finish_reason":"stop","message":{"content":"OK"}}]}')]
        with patch.dict(os.environ, {"TEST_MODEL_KEY": "private-test"}), \
                patch.object(provider, "open_request", side_effect=replies) as request:
            report = provider.probe(dict(self.config, require_model_listing=False))
        self.assertTrue(report['ready'])
        self.assertFalse(report['checks'][0]['ok'])
        self.assertEqual(report['checks'][0]['code'], 'model_not_listed')
        self.assertEqual(request.call_count, 2)
        with patch.dict(os.environ, {"TEST_MODEL_KEY": "private-test"}), \
                patch.object(provider, "open_request", return_value=io.BytesIO(b'{"data":[]}')) as request:
            self.assertFalse(provider.probe(self.config)['ready'])
            self.assertEqual(request.call_count, 1)
        error = urllib.error.HTTPError('https://model.example', 401, 'private', {},
                                      io.BytesIO(b'{}'))
        with patch.dict(os.environ, {"TEST_MODEL_KEY": "private-test"}), \
                patch.object(provider, "open_request", side_effect=error) as request:
            self.assertFalse(provider.probe(dict(self.config, require_model_listing=False))['ready'])
            self.assertEqual(request.call_count, 1)

    def test_timeout_is_bounded_distinct_and_never_retries(self):
        for timeout in (0, 601, True, '180'):
            with self.assertRaisesRegex(ValueError, 'timeout'):
                provider.probe(dict(self.config, request_timeout_seconds=timeout))
        with patch.dict(os.environ, {"TEST_MODEL_KEY": "private-test"}), \
                patch.object(provider, "open_request", side_effect=TimeoutError('private-test')) as request:
            result = provider.probe(dict(self.config, request_timeout_seconds=180))
        self.assertEqual(request.call_args.kwargs['timeout'], 180)
        self.assertEqual(request.call_count, 1)
        self.assertEqual(result['checks'][0]['code'], 'provider_timeout')
        self.assertNotIn('private-test', json.dumps(result))

    def test_environment_key_never_falls_back(self):
        with patch.dict(os.environ, {}, clear=True), patch.object(provider.Path, "read_text") as read:
            with self.assertRaises(ValueError):
                provider.credential(dict(self.config, bearer_file="fallback"))
            read.assert_not_called()
        with patch.dict(os.environ, {"TEST_MODEL_KEY": "private-test"}):
            self.assertEqual(provider.credential(self.config), "private-test")
        with self.assertRaises(ValueError):
            provider.credential(dict(self.config, api_key_env="invalid-name"))

    def test_requires_https_except_loopback_and_disables_redirects(self):
        for endpoint in ["http://model.example", "https://key@model.example", "https://model.example/?key=secret"]:
            with self.assertRaises(ValueError):
                provider.endpoint({"endpoint": endpoint})
        self.assertEqual(provider.endpoint({"endpoint": "http://127.0.0.1:8317/v1/"}), "http://127.0.0.1:8317/v1")
        self.assertIsNone(provider.NoRedirect().redirect_request(None, None, 302, "", {}, "https://other.example"))

    def test_actual_generation_required_no_secret_or_reply_in_report(self):
        responses = [io.BytesIO(b'{"data":[{"id":"fixture"}]}'),
                     io.BytesIO(b'{"choices":[{"finish_reason":"stop","message":{"content":"OK"}}]}')]
        with patch.dict(os.environ, {"TEST_MODEL_KEY": "private-test"}), \
                patch.object(provider, "open_request", side_effect=responses) as request:
            report = provider.probe(self.config)
        self.assertTrue(report["ready"])
        self.assertEqual(request.call_count, 2)
        self.assertNotIn("private-test", json.dumps(report))

    def test_ip_block_is_distinguished_and_no_retry_or_generation(self):
        error = urllib.error.HTTPError("https://model.example", 403, "private", {},
                                      io.BytesIO(b'{"error":{"code":"IP_BLOCKED","message":"private-test"}}'))
        with patch.dict(os.environ, {"TEST_MODEL_KEY": "private-test"}), \
                patch.object(provider, "open_request", side_effect=error) as request:
            report = provider.probe(self.config)
        self.assertFalse(report["ready"])
        self.assertEqual(report["checks"][0]["code"], "IP_BLOCKED")
        self.assertEqual(request.call_count, 1)
        self.assertNotIn("private-test", json.dumps(report))

    def test_tool_probe_requires_call_and_result_roundtrip(self):
        def responses(arguments='{"fixture_id":"readiness"}'):
            return [io.BytesIO(json.dumps(v).encode()) for v in [
                {"data": [{"id": "fixture"}]},
                {"choices": [{"finish_reason": "stop", "message": {"content": "OK"}}]},
                {"choices": [{"finish_reason": "tool_calls", "message": {"tool_calls": [
                    {"id": "test-call", "type": "function",
                     "function": {"name": "read_acceptance_fixture", "arguments": arguments}}]}}]},
                {"choices": [{"finish_reason": "stop", "message": {"content": "READY42"}}]},
            ]]
        with patch.dict(os.environ, {"TEST_MODEL_KEY": "private-test"}), \
                patch.object(provider, "open_request", side_effect=responses()) as request:
            report = provider.probe(dict(self.config, require_tools=True))
        self.assertTrue(report["ready"])
        self.assertEqual(request.call_count, 4)
        sent = json.loads(request.call_args.args[0].data)
        self.assertEqual(sent["messages"][-1]["role"], "tool")
        self.assertNotIn("private-test", json.dumps(report))
        with patch.dict(os.environ, {"TEST_MODEL_KEY": "private-test"}), \
                patch.object(provider, "open_request", side_effect=responses('{"shell":"bad"}')) as request:
            report = provider.probe(dict(self.config, require_tools=True))
        self.assertFalse(report["ready"])
        self.assertEqual(request.call_count, 3)
        with self.assertRaises(ValueError):
            provider.probe(dict(self.config, require_tools="true"))


if __name__ == "__main__":
    unittest.main()
