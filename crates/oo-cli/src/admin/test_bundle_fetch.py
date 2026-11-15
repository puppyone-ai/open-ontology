#!/usr/bin/env python3
"""`oo up` release-bundle fallback against a local HTTP server and fake Docker.

Builds target/debug/oo first (cargo build -p oo-cli --bin oo). No real images,
registries, containers or databases: the fake `docker` refuses every pull and
records each call; the bundle is a small synthetic runtime archive.
"""
import gzip
import hashlib
import http.server
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import threading
import unittest

ROOT = Path(__file__).resolve().parents[4]
OO = ROOT / 'target/debug/oo'
RUNTIME = 'ghcr.io/example/lite@sha256:' + '1' * 64
CONFIG = json.dumps({'os': 'linux', 'architecture': 'arm64', 'rootfs': {'diff_ids': []}}).encode()
CONFIG_ID = 'sha256:' + hashlib.sha256(CONFIG).hexdigest()

FAKE_DOCKER = r'''#!PYTHON
import json, os, sys
from pathlib import Path
state = Path(os.environ['FAKE_DOCKER_STATE'])
args = sys.argv[1:]
with open(state / 'calls', 'a') as f:
    f.write(' '.join(args) + '\n')
loaded = (state / 'loaded').read_text().split() if (state / 'loaded').exists() else []
config = os.environ['FAKE_CONFIG_ID']
if args[0] == 'info':
    print(os.environ.get('FAKE_DAEMON', 'linux/aarch64')); raise SystemExit(0)
if args[0] == 'version':
    print('linux/arm64'); raise SystemExit(0)
if args[0] == 'pull':
    print('unauthorized', file=sys.stderr); raise SystemExit(1)
if args[0] == 'load':
    sys.stdin.buffer.read()
    (state / 'loaded').write_text(config)
    print('Loaded image ID: ' + config); raise SystemExit(0)
if args[:2] == ['image', 'tag']:
    raise SystemExit(0)
if args[:2] == ['image', 'inspect']:
    ref = args[-1]
    if 'postgres@sha256:' in ref:
        raise SystemExit(0)
    if ref in loaded:
        fmt = args[args.index('--format') + 1] if '--format' in args else ''
        if '{{json .Id}}' in fmt:
            print(json.dumps(config), '"linux"', '"arm64"', '[]')
        elif fmt:
            print(config + ' linux/arm64')
        raise SystemExit(0)
    raise SystemExit(1)
raise SystemExit(1)
'''


def member(tar, name, raw):
    info = tarfile.TarInfo(name)
    info.size = len(raw)
    tar.addfile(info, io.BytesIO(raw))


def fake_bundle(path, platform='linux/arm64'):
    runtime = io.BytesIO()
    with tarfile.open(fileobj=runtime, mode='w') as tar:
        member(tar, 'manifest.json', json.dumps([{'Config': 'config.json', 'RepoTags': None,
                                                  'Layers': []}]).encode())
        member(tar, 'config.json', CONFIG)
    runtime = runtime.getvalue()
    files = {'runtime.tar': hashlib.sha256(runtime).hexdigest()}
    manifest = json.dumps({'protocol': 'oo.lite-runtime-bundle/v1', 'platform': platform,
                           'images': {'runtime': {'archive': 'runtime.tar', 'id': CONFIG_ID,
                                                  'config_id': CONFIG_ID, 'os': 'linux',
                                                  'arch': 'arm64', 'layers': []}},
                           'files': files}).encode()
    sums = ''.join(f'{d}  {n}\n' for n, d in files.items()).encode()
    with path.open('wb') as raw, gzip.GzipFile(fileobj=raw, mode='wb', mtime=0) as gz, \
            tarfile.open(fileobj=gz, mode='w|') as tar:
        member(tar, 'SHA256SUMS', sums)
        member(tar, 'manifest.json', manifest)
        member(tar, 'runtime.tar', runtime)
    return hashlib.sha256(manifest).hexdigest()


class Handler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *args):
        self.server.requests.append(self.path)


class Fetch(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name).resolve()
        self.www, self.state, self.home, self.tools = (self.dir / n for n in ('www', 'state', 'home', 'tools'))
        for path in (self.www, self.state, self.home, self.tools):
            path.mkdir(mode=0o700)
        self.asset = self.www / 'oo-lite-runtime-v0.2.0-linux-arm64.tar.gz'
        self.manifest_sha = fake_bundle(self.asset)
        (self.tools / 'docker').write_text(FAKE_DOCKER.replace('PYTHON', sys.executable))
        (self.tools / 'docker').chmod(0o700)
        handler = lambda *a, **k: Handler(*a, directory=str(self.www), **k)
        self.server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), handler)
        self.server.requests = []
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.stack = self.dir / 'stack.json'
        shutil.copy2(ROOT / 'crates/oo-stack/example.stack.json', self.stack)
        self.write_pin()

    def tearDown(self):
        self.server.shutdown()
        self.tmp.cleanup()

    def write_pin(self, **override):
        entry = {'asset': self.asset.name,
                 'url': f'http://127.0.0.1:{self.server.server_port}/{self.asset.name}',
                 'size': self.asset.stat().st_size,
                 'sha256': hashlib.sha256(self.asset.read_bytes()).hexdigest(),
                 'manifest_sha256': self.manifest_sha, 'image_id': CONFIG_ID, 'config_id': CONFIG_ID}
        entry.update(override)
        self.pin = self.dir / 'oo-lite-bundle.json'
        self.pin.write_text(json.dumps({'schema': 'oo.lite-bundles/v1', 'runtime': RUNTIME,
                                        'platforms': {'linux/arm64': entry}}))

    def up(self, *extra, **env):
        environ = {'HOME': str(self.home), 'PATH': f'{self.tools}:/usr/bin:/bin:{os.path.dirname(sys.executable)}',
                   'FAKE_DOCKER_STATE': str(self.state), 'FAKE_CONFIG_ID': CONFIG_ID,
                   'OO_LITE_RUNTIME_IMAGE': RUNTIME, 'OO_LITE_BUNDLE_PIN': str(self.pin), **env}
        result = subprocess.run([str(OO), 'up', '--config', str(self.stack), '--json', *extra],
                                env=environ, capture_output=True, timeout=120)
        message = json.loads(result.stdout or b'{}').get('message', '')
        return result, message

    def calls(self):
        path = self.state / 'calls'
        return path.read_text().splitlines() if path.exists() else []

    def cache(self):
        path = self.home / '.cache/open-ontology/lite-bundles'
        return sorted(p.name for p in path.iterdir()) if path.exists() else []

    def test_failed_pull_downloads_verifies_and_loads_then_reuses(self):
        result, message = self.up()
        # Past the image gate: the next stage needs owner secrets, not images.
        self.assertIn('SECRET_', message)
        self.assertEqual(self.server.requests, ['/' + self.asset.name])
        calls = self.calls()
        self.assertTrue(any(c.startswith('pull ') for c in calls))
        self.assertEqual(sum(c == 'load' for c in calls), 1)
        self.assertTrue(any(c.startswith('image tag ' + CONFIG_ID) for c in calls))
        self.assertIn(b'downloading runtime bundle', result.stderr)
        self.assertEqual(self.cache(), [], 'nothing kept after a successful load')
        # Second up: the loaded image is found by ID; no pull, download or load.
        (self.state / 'calls').unlink()
        _, message = self.up()
        self.assertNotIn('LITE_BUNDLE', message)
        self.assertEqual(len(self.server.requests), 1)
        self.assertFalse(any(c.startswith(('pull ', 'load')) for c in self.calls()))

    def test_sha_mismatch_never_loads_and_removes_partial(self):
        self.write_pin(sha256='0' * 64)
        result, message = self.up()
        self.assertEqual(result.returncode, 1)
        self.assertIn('LITE_BUNDLE_SHA256', message)
        self.assertNotIn('load', self.calls())
        self.assertEqual(self.cache(), [])

    def test_manifest_mismatch_fails_in_the_verifier(self):
        self.write_pin(manifest_sha256='0' * 64)
        result, message = self.up()
        self.assertEqual(result.returncode, 1)
        self.assertIn('LITE_BUNDLE_VERIFY: BUNDLE_MANIFEST_CHECKSUM', message)
        self.assertNotIn('load', self.calls())

    def test_wrong_daemon_architecture(self):
        result, message = self.up(FAKE_DAEMON='linux/x86_64')
        self.assertEqual(result.returncode, 1)
        self.assertIn('LITE_BUNDLE_PLATFORM', message)
        self.assertEqual(self.server.requests, [])

    def test_offline_skips_network_and_accepts_local_file(self):
        result, message = self.up(OO_LITE_OFFLINE='1')
        self.assertIn('LITE_IMAGES_MISSING', message)
        self.assertIn('--offline-bundle', message)
        self.assertFalse(any(c.startswith('pull ') for c in self.calls()))
        self.assertEqual(self.server.requests, [])
        _, message = self.up('--offline-bundle', str(self.asset), OO_LITE_OFFLINE='1')
        self.assertNotIn('LITE_BUNDLE', message)
        self.assertNotIn('LITE_IMAGES_MISSING', message)
        self.assertEqual(self.server.requests, [])
        self.assertIn('load', self.calls())
        self.assertTrue(self.asset.exists())

    def test_offline_bundle_requires_an_existing_file(self):
        result, message = self.up('--offline-bundle', str(self.dir / 'missing.tar.gz'))
        self.assertEqual(result.returncode, 2)
        self.assertIn('--offline-bundle', message)


if __name__ == '__main__':
    unittest.main()
