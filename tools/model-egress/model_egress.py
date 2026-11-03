#!/usr/bin/env python3
"""Owner-local model egress. No third-party runtime dependencies or retry loop."""
import argparse
from collections import deque
import http.client
from http.server import BaseHTTPRequestHandler, HTTPServer
import ipaddress
import json
import os
from pathlib import Path
import re
import socket
import ssl
import stat
import threading
import time
from urllib.parse import urlsplit

MAX_BYTES = 4 * 1024 * 1024
NAME = re.compile(r"[A-Za-z0-9][A-Za-z0-9_-]{0,63}\Z")


class Refusal(Exception):
    def __init__(self, code, status=502):
        self.code, self.status = code, status
        super().__init__(code)


def private_open(path, flags=os.O_RDONLY, mode=0o600, directory=False):
    """Traverse using directory fds: no symlink or parent-replacement race."""
    path = Path(os.path.abspath(path))
    fd = os.open('/', os.O_RDONLY | os.O_DIRECTORY)
    try:
        for part in path.parts[1:-1]:
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=fd)
            os.close(fd)
            fd = child
        leaf = os.open(path.name, flags | os.O_NOFOLLOW | os.O_NONBLOCK, mode, dir_fd=fd)
    finally:
        os.close(fd)
    info = os.fstat(leaf)
    valid_type = stat.S_ISDIR(info.st_mode) if directory else stat.S_ISREG(info.st_mode)
    if (not valid_type or info.st_uid != os.geteuid()
            or stat.S_IMODE(info.st_mode) != (0o700 if directory else 0o600)
            or (not directory and info.st_nlink != 1)):
        os.close(leaf)
        raise Refusal('SECRET_PERMISSIONS', 500)
    return leaf


def read_key(name, root=None):
    if not isinstance(name, str) or not NAME.fullmatch(name):
        raise Refusal('SECRET_REFERENCE', 500)
    root = Path(root) if root is not None else Path.home() / '.config/open-ontology/secrets'
    directory = private_open(root, directory=True)
    try:
        fd = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=directory)
        with os.fdopen(fd, 'rb') as file:
            info = os.fstat(file.fileno())
            if (not stat.S_ISREG(info.st_mode) or info.st_uid != os.geteuid()
                    or stat.S_IMODE(info.st_mode) != 0o600 or info.st_nlink != 1):
                raise Refusal('SECRET_PERMISSIONS', 500)
            raw = file.read(4098)
    finally:
        os.close(directory)
    raw = raw.removesuffix(b'\n')
    if not raw or len(raw) > 4096 or any(b < 33 or b > 126 for b in raw):
        raise Refusal('SECRET_CONTENT', 500)
    return raw.decode('ascii')


def validate_url(value):
    if (not isinstance(value, str) or len(value) > 2048
            or any(ord(c) <= 32 or ord(c) >= 127 for c in value)
            or any(c in value for c in '@?#\\%')):
        raise Refusal('CONFIG_ENDPOINT', 500)
    try:
        url = urlsplit(value)
        loopback = url.hostname == 'localhost'
        try:
            loopback = loopback or ipaddress.ip_address(url.hostname).is_loopback
        except ValueError:
            pass
        if (not url.hostname or url.username or url.password or url.query or url.fragment
                or (url.scheme != 'https' and not (url.scheme == 'http' and loopback))
                or url.port == 0):
            raise ValueError()
    except (ValueError, TypeError):
        raise Refusal('CONFIG_ENDPOINT', 500) from None
    return url


def error_code(status, body):
    # Never relay error messages: providers sometimes echo headers/request bodies.
    error = body.get('error', {}) if isinstance(body, dict) else {}
    labels = ' '.join(str(error.get(k, '')) for k in ('code', 'type', 'message')) if isinstance(error, dict) else ''
    labels = labels.lower()
    if any(s in labels for s in ('usage_limit', 'insufficient_quota', 'credit balance', 'quota_exceeded')):
        return 'USAGE_LIMIT'
    if 'cooldown' in labels or 'overloaded' in labels or status == 529:
        return 'COOLDOWN'
    if status == 429 or 'rate_limit' in labels:
        return 'RATE_LIMITED'
    if status in (401, 403):
        return 'UPSTREAM_AUTH'
    return 'UPSTREAM_ERROR'


def usage(value):
    value = value if isinstance(value, dict) else {}
    def integer(*names):
        for name in names:
            v = value.get(name)
            if type(v) is int and 0 <= v <= 2**63 - 1:
                return v
        return None
    return {'prompt_tokens': integer('prompt_tokens', 'input_tokens'),
            'completion_tokens': integer('completion_tokens', 'output_tokens')}


def to_anthropic(request):
    allowed = {'model', 'messages', 'stream', 'stream_options', 'max_tokens',
               'max_completion_tokens', 'temperature', 'top_p', 'stop'}
    if set(request) - allowed:
        raise Refusal('UNSUPPORTED_PARAMETER', 400)
    result = {'model': request['model'], 'messages': [],
              'max_tokens': request.get('max_completion_tokens', request.get('max_tokens', 1024)),
              'stream': request.get('stream', False)}
    system = []
    for message in request['messages']:
        if set(message) != {'role', 'content'} or not isinstance(message['content'], str):
            raise Refusal('UNSUPPORTED_MESSAGE', 400)
        if message['role'] == 'system':
            system.append(message['content'])
        elif message['role'] in ('user', 'assistant'):
            result['messages'].append(message)
        else:
            raise Refusal('UNSUPPORTED_MESSAGE', 400)
    if system:
        result['system'] = '\n\n'.join(system)
    for key in ('temperature', 'top_p'):
        if key in request:
            result[key] = request[key]
    if 'stop' in request:
        result['stop_sequences'] = [request['stop']] if isinstance(request['stop'], str) else request['stop']
    return result


def stop_reason(reason):
    if reason == 'max_tokens':
        return 'length'
    if reason == 'tool_use':
        return 'tool_calls'
    return 'stop'


def from_anthropic(body):
    content = body.get('content')
    if not isinstance(content, list) or any(b.get('type') != 'text' for b in content):
        raise Refusal('UNSUPPORTED_RESPONSE')
    counts = usage(body.get('usage'))
    total = None if None in counts.values() else sum(counts.values())
    return {'id': body.get('id', 'egress'), 'object': 'chat.completion',
            'model': body.get('model'), 'choices': [{'index': 0,
                'message': {'role': 'assistant', 'content': ''.join(b['text'] for b in content)},
                'finish_reason': stop_reason(body.get('stop_reason'))}],
            'usage': {**counts, 'total_tokens': total}}


def events(response):
    """Bounded SSE parser, including multi-line data and split transport reads."""
    data, size = [], 0
    while True:
        line = response.readline(MAX_BYTES + 1)
        size += len(line)
        if size > MAX_BYTES:
            raise Refusal('RESPONSE_BUDGET')
        if not line:
            if data:
                yield b'\n'.join(data)
            return
        line = line.rstrip(b'\r\n')
        if not line:
            if data:
                yield b'\n'.join(data)
                data = []
        elif line.startswith(b'data:'):
            data.append(line[5:].lstrip(b' '))


class Egress:
    def __init__(self, model, meter, rpm=30, secret_root=None, timeout=45):
        if (not isinstance(model, dict) or set(model) - {'provider', 'base_url', 'model', 'key', 'context_window'}
                or model.get('provider') not in ('openai-compatible', 'anthropic')
                or not isinstance(model.get('model'), str)
                or not re.fullmatch(r'[A-Za-z0-9/._+:-]{1,128}', model['model'])
                or not isinstance(model.get('key'), dict) or set(model['key']) != {'secret_ref'}
                or not (model.get('context_window', 'auto') == 'auto'
                        or (type(model.get('context_window')) is int and model['context_window'] > 0))
                or type(rpm) is not int or not 1 <= rpm <= 10000):
            raise Refusal('CONFIG_SCHEMA', 500)
        self.url = validate_url(model.get('base_url'))
        self.model = model
        self.key = read_key(model['key']['secret_ref'], secret_root)
        self.timeout, self.rpm = timeout, rpm
        self.calls, self.block_until, self.block_code = deque(), 0, None
        self.lock = threading.Lock()
        self.meter = os.fdopen(private_open(meter, os.O_WRONLY | os.O_APPEND | os.O_CREAT), 'a')

    def close(self):
        self.meter.close()

    def admit(self):
        with self.lock:
            now = time.monotonic()
            if now < self.block_until:
                raise Refusal(self.block_code, 429)
            while self.calls and now - self.calls[0] >= 60:
                self.calls.popleft()
            if len(self.calls) >= self.rpm:
                raise Refusal('RATE_LIMITED', 429)
            self.calls.append(now)

    def block(self, code):
        if code in ('RATE_LIMITED', 'USAGE_LIMIT', 'COOLDOWN'):
            self.block_code = code
            self.block_until = time.monotonic() + (300 if code == 'USAGE_LIMIT' else 60)

    def connect(self, suffix, body=None):
        cls = http.client.HTTPSConnection if self.url.scheme == 'https' else http.client.HTTPConnection
        kwargs = {'timeout': self.timeout}
        if self.url.scheme == 'https':
            kwargs['context'] = ssl.create_default_context()
        conn = cls(self.url.hostname, self.url.port, **kwargs)
        headers = {'Content-Type': 'application/json', 'Accept-Encoding': 'identity'}
        if self.model['provider'] == 'anthropic':
            headers.update({'x-api-key': self.key, 'anthropic-version': '2023-06-01'})
        else:
            headers['Authorization'] = 'Bearer ' + self.key
        try:
            conn.request('GET' if body is None else 'POST', self.url.path.rstrip('/') + suffix,
                         body=None if body is None else json.dumps(body).encode(), headers=headers)
            response = conn.getresponse()
            if 300 <= response.status < 400:
                raise Refusal('UPSTREAM_REDIRECT')
            return conn, response
        except Exception:
            conn.close()
            raise

    def safe(self, value):
        encoded = json.dumps(value, separators=(',', ':')).encode()
        if self.key.encode() in encoded:
            raise Refusal('SECRET_ECHO')
        return encoded

    def call(self, request):
        """Yield JSON objects, or None for [DONE]. Meter even partial/failing calls."""
        started, counts, code, conn = time.monotonic(), usage(None), 'OK', None
        stream = isinstance(request, dict) and request.get('stream') is True
        try:
            self.admit()
            if (not isinstance(request, dict) or request.get('model') != self.model['model']
                    or not isinstance(request.get('messages'), list) or not request['messages']
                    or len(request['messages']) > 1000
                    or any(not isinstance(m, dict) for m in request['messages'])
                    or type(request.get('stream', False)) is not bool):
                raise Refusal('REQUEST_SCHEMA', 400)
            for name in ('max_tokens', 'max_completion_tokens'):
                if name in request and (type(request[name]) is not int or not 1 <= request[name] <= 128000):
                    raise Refusal('REQUEST_BUDGET', 400)
            anthropic = self.model['provider'] == 'anthropic'
            outbound = to_anthropic(request) if anthropic else dict(request)
            if stream and not anthropic:
                outbound['stream_options'] = {'include_usage': True}
            conn, response = self.connect('/messages' if anthropic else '/chat/completions', outbound)
            if response.status != 200 or not stream:
                raw = response.read(MAX_BYTES + 1)
                if len(raw) > MAX_BYTES:
                    raise Refusal('RESPONSE_BUDGET')
                try:
                    body = json.loads(raw)
                except ValueError:
                    raise Refusal('UPSTREAM_PROTOCOL') from None
                if response.status != 200 or not isinstance(body, dict) or 'error' in body:
                    code = error_code(response.status, body)
                    raise Refusal(code, 429 if code in ('RATE_LIMITED', 'USAGE_LIMIT', 'COOLDOWN') else 502)
                if stream:
                    raise Refusal('UPSTREAM_PROTOCOL')
                result = from_anthropic(body) if anthropic else body
                if not isinstance(result.get('choices'), list):
                    raise Refusal('UPSTREAM_PROTOCOL')
                counts = usage(result.get('usage'))
                self.safe(result)
                yield result
                return
            if 'text/event-stream' not in response.getheader('Content-Type', ''):
                raise Refusal('UPSTREAM_PROTOCOL')
            finished = False
            for raw in events(response):
                if raw == b'[DONE]' and not anthropic:
                    finished = True
                    yield None
                    break
                try:
                    event = json.loads(raw)
                except ValueError:
                    raise Refusal('UPSTREAM_PROTOCOL') from None
                if not isinstance(event, dict):
                    raise Refusal('UPSTREAM_PROTOCOL')
                if 'error' in event or event.get('type') == 'error':
                    raise Refusal(error_code(200, event))
                if not anthropic:
                    if event.get('usage'):
                        counts = usage(event['usage'])
                    self.safe(event)
                    yield event
                    continue
                kind = event.get('type')
                delta, finish = {}, None
                if kind == 'message_start':
                    counts = usage(event.get('message', {}).get('usage'))
                    delta = {'role': 'assistant'}
                elif kind == 'content_block_start':
                    if event.get('content_block', {}).get('type') != 'text':
                        raise Refusal('UNSUPPORTED_RESPONSE')
                    continue
                elif kind == 'content_block_delta':
                    if event.get('delta', {}).get('type') != 'text_delta':
                        raise Refusal('UNSUPPORTED_RESPONSE')
                    delta = {'content': event['delta']['text']}
                elif kind == 'message_delta':
                    finish = stop_reason(event.get('delta', {}).get('stop_reason'))
                    counts['completion_tokens'] = usage(event.get('usage'))['completion_tokens']
                elif kind == 'message_stop':
                    finished = True
                    yield {'object': 'chat.completion.chunk', 'choices': [], 'usage': counts}
                    yield None
                    break
                else:
                    continue
                result = {'object': 'chat.completion.chunk', 'model': self.model['model'],
                          'choices': [{'index': 0, 'delta': delta, 'finish_reason': finish}]}
                self.safe(result)
                yield result
            if not finished:
                raise Refusal('STREAM_INCOMPLETE')
        except Refusal as error:
            code = error.code
            self.block(code)
            raise
        except (TimeoutError, socket.timeout):
            code = 'UPSTREAM_TIMEOUT'
            raise Refusal(code, 504) from None
        except (OSError, ValueError, KeyError, TypeError, http.client.HTTPException):
            code = 'UPSTREAM_PROTOCOL'
            raise Refusal(code) from None
        except GeneratorExit:
            code = 'CLIENT_DISCONNECTED'
            raise
        finally:
            if conn:
                conn.close()
            record = {'protocol': 'oo.model-usage/v1', 'time_unix_ms': int(time.time() * 1000),
                      'duration_ms': int((time.monotonic() - started) * 1000),
                      'stream': stream, 'code': code, **counts,
                      'usage_measured': None not in counts.values()}
            with self.lock:
                self.meter.write(json.dumps(record) + '\n')
                self.meter.flush()

    def probe(self):
        result = {'protocol': 'oo.model-capabilities/v1', 'models_status': 'unavailable',
                  'context_window': None, 'max_output_tokens': None, 'json_mode': 'unknown',
                  'proxy_injected_prompt_tokens': None,
                  'overhead_basis': 'minimal_prompt_total_not_isolated_injection',
                  'limits_basis': 'unmeasured'}
        conn = None
        try:
            self.admit()
            conn, response = self.connect('/models')
            raw = response.read(MAX_BYTES + 1)
            if response.status == 200 and len(raw) <= MAX_BYTES:
                body = json.loads(raw)
                selected = next((m for m in body.get('data', [])
                                 if isinstance(m, dict) and m.get('id') == self.model['model']), None)
                result['models_status'] = 'present' if selected else 'model_not_listed'
                if selected:
                    for dest, fields in [('context_window', ('context_window', 'context_length')),
                                         ('max_output_tokens', ('max_output_tokens', 'max_completion_tokens'))]:
                        for field in fields:
                            if type(selected.get(field)) is int and selected[field] > 0:
                                result[dest] = selected[field]
                                result['limits_basis'] = 'provider_metadata_not_boundary_tested'
                                break
            elif response.status not in (404, 405):
                raise Refusal(error_code(response.status, json.loads(raw)))
        except (ValueError, OSError, http.client.HTTPException):
            result['models_status'] = 'unavailable'
        finally:
            if conn:
                conn.close()
        request = {'model': self.model['model'], 'messages': [{'role': 'user', 'content': '.'}],
                   'max_tokens': 1}
        reply = list(self.call(request))[0]
        result['proxy_injected_prompt_tokens'] = usage(reply.get('usage'))['prompt_tokens']
        if self.model['provider'] == 'openai-compatible':
            request['messages'][0]['content'] = 'Return a JSON object.'
            request['response_format'] = {'type': 'json_object'}
            try:
                list(self.call(request))
                result['json_mode'] = 'accepted_parameter_output_not_verified_at_one_token'
            except Refusal as error:
                if error.code in ('RATE_LIMITED', 'USAGE_LIMIT', 'COOLDOWN', 'UPSTREAM_AUTH'):
                    raise
                result['json_mode'] = 'probe_failed'
        override = self.model.get('context_window', 'auto')
        if type(override) is int and override > 0:
            result['context_window'] = override
            result['limits_basis'] = 'operator_override'
        self.safe(result)
        return result


def server(egress, host, port):
    if not ipaddress.ip_address(host).is_loopback:
        raise Refusal('LISTEN_NOT_LOOPBACK', 500)

    class Handler(BaseHTTPRequestHandler):
        protocol_version = 'HTTP/1.1'

        def log_message(self, *_args):
            pass

        def setup(self):
            super().setup()
            self.connection.settimeout(egress.timeout)

        def send_json(self, status, body):
            data = egress.safe(body)
            self.send_response(status)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Content-Length', str(len(data)))
            self.send_header('Connection', 'close')
            self.end_headers()
            self.wfile.write(data)
            self.close_connection = True

        def do_POST(self):
            iterator, streaming = None, False
            try:
                # A browser origin must not spend the owner's credentials. No CORS.
                if self.headers.get('Origin') or self.headers.get('Transfer-Encoding'):
                    raise Refusal('REQUEST_REJECTED', 400)
                if self.path != '/v1/chat/completions':
                    raise Refusal('NOT_FOUND', 404)
                host_header = self.headers.get('Host', '')
                if host_header not in (f'{host}:{self.server.server_port}',
                                        f'[{host}]:{self.server.server_port}'):
                    raise Refusal('HOST_REJECTED', 400)
                lengths = self.headers.get_all('Content-Length', [])
                if len(lengths) != 1 or not lengths[0].isdigit() or not 0 < int(lengths[0]) <= MAX_BYTES:
                    raise Refusal('REQUEST_BUDGET', 413)
                request = json.loads(self.rfile.read(int(lengths[0])))
                iterator = egress.call(request)
                first = next(iterator)  # Refusals before headers keep their HTTP status.
                if not request.get('stream', False):
                    # Exhaust now, so metering finishes before acknowledging the call.
                    list(iterator)
                    self.send_json(200, first)
                    return
                self.send_response(200)
                self.send_header('Content-Type', 'text/event-stream')
                self.send_header('Cache-Control', 'no-cache')
                self.send_header('Connection', 'close')
                self.end_headers()
                streaming = True
                self.send_event(first)
                for event in iterator:
                    self.send_event(event)
            except Refusal as error:
                body = {'error': {'code': error.code, 'message': error.code}}
                if streaming:
                    self.send_event(body)
                else:
                    self.send_json(error.status, body)
            except (ValueError, StopIteration):
                self.send_json(400, {'error': {'code': 'REQUEST_SCHEMA'}})
            except (OSError, TimeoutError):
                pass
            finally:
                if iterator is not None:
                    iterator.close()
                self.close_connection = True

        def send_event(self, event):
            self.wfile.write(b'data: ' + (b'[DONE]' if event is None else egress.safe(event)) + b'\n\n')
            self.wfile.flush()

        def do_GET(self):
            self.send_json(404, {'error': {'code': 'NOT_FOUND'}})

    class LocalServer(HTTPServer):
        address_family = socket.AF_INET6 if ':' in host else socket.AF_INET

        def handle_error(self, _request, _address):
            # Never print traceback/request/headers containing upstream data.
            pass

    return LocalServer((host, port), Handler)


class SafeParser(argparse.ArgumentParser):
    def error(self, _message):
        raise Refusal('ARGUMENT_ERROR', 400)


def main():
    parser = SafeParser(description=__doc__)
    parser.add_argument('--config', required=True, help='JSON stack config; key must be a secret_ref')
    parser.add_argument('--meter', required=True, help='owner-only usage JSONL file')
    parser.add_argument('--listen', default='127.0.0.1')
    parser.add_argument('--port', default=8317, type=int)
    parser.add_argument('--rpm', default=30, type=int)
    parser.add_argument('--probe', action='store_true')
    egress = None
    try:
        args = parser.parse_args()
        # Refuse non-loopback before reading the credential.
        if not ipaddress.ip_address(args.listen).is_loopback or not 0 <= args.port <= 65535:
            raise Refusal('LISTEN_NOT_LOOPBACK', 500)
        with open(args.config, 'rb') as file:
            raw = file.read(MAX_BYTES + 1)
        if len(raw) > MAX_BYTES:
            raise Refusal('CONFIG_BUDGET', 500)
        config = json.loads(raw)
        egress = Egress(config['model'], args.meter, args.rpm)
        if args.probe:
            print(json.dumps(egress.probe()))
        else:
            with server(egress, args.listen, args.port) as httpd:
                httpd.serve_forever()
        return 0
    except Refusal as error:
        print(json.dumps({'error': {'code': error.code}}))
        return 2
    except (OSError, ValueError, KeyError, TypeError):
        print(json.dumps({'error': {'code': 'CONFIG_OR_TRANSPORT_ERROR'}}))
        return 2
    except KeyboardInterrupt:
        return 0
    finally:
        if egress:
            egress.close()


if __name__ == '__main__':
    raise SystemExit(main())
