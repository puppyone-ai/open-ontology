#!/usr/bin/env python3
"""Bundle-only product runtime with a separately staged synthetic fixture harness.

arm64 uses isolated DinD; amd64 uses an emulated client and the host daemon.
Neither target receives a checkout, Cargo, Rust sources or host product binaries.
Run serially: at most two owned databases, removed even after test failures.
"""
import argparse
import json
import os
from pathlib import Path
import re
import secrets
import select
import shutil
import socket
import socketserver
import subprocess
import sys
import threading
import time
from unittest.mock import patch

import bundle

ROOT = Path(__file__).resolve().parents[2]
DIND = {
    'arm64': 'docker.io/library/docker@sha256:ad9e28ccfdf76cf44248258090e07d8353b9628725d8a716bab176bd71200d7d',
    'amd64': 'docker.io/library/docker@sha256:f649ef046008ca7f926a2571c32b0ac22e5c59eb61b959617f9acc2a4c638cf5',
}


def forward(endpoint, port=0):
    """Fixture-local TCP transport; TLS still terminates in the real service."""
    class Handler(socketserver.BaseRequestHandler):
        def handle(self):
            try:
                with socket.create_connection(endpoint, timeout=10) as upstream:
                    upstream.settimeout(None)
                    peers = (self.request, upstream)
                    while True:
                        ready, _, _ = select.select(peers, [], [], 30)
                        if not ready:
                            return
                        for peer in ready:
                            data = peer.recv(65536)
                            if not data:
                                return
                            (upstream if peer is self.request else self.request).sendall(data)
            except OSError:
                # Clients report refused connections/handshakes; never fake a reply.
                return
    class Server(socketserver.ThreadingTCPServer):
        allow_reuse_address = True
        daemon_threads = True
    server = Server(('127.0.0.1', port), Handler)
    worker = threading.Thread(target=server.serve_forever, daemon=True)
    worker.start()
    return server, worker


def foundation(root, path, trust, emulated=False):
    out = root / 'foundation'
    out.mkdir(mode=0o700)
    home = out / 'home'
    store = home / '.config/open-ontology/secrets'
    store.mkdir(parents=True, mode=0o700)
    for name in ('source', 'model'):
        secret = store / name
        secret.touch(mode=0o600)
        secret.write_text(secrets.token_hex(24))
    config = out / 'stack.json'
    config.write_text(json.dumps({'version': 'oo.stack/v1', 'sources': [{
        'id': 'fixture', 'engine': 'mysql', 'host': 'localhost', 'port': 3306,
        'database': 'fixture', 'user': 'reader', 'password': {'secret_ref': 'source'},
        'scope': {'include': ['fixture.events']}}],
        'model': {'provider': 'openai-compatible', 'base_url': 'http://127.0.0.1:9/v1',
                  'model': 'fixture', 'key': {'secret_ref': 'model'}}}))
    env = dict(os.environ, HOME=str(home), DOCKER_CONFIG=os.environ['DOCKER_CONFIG'])
    tracer = out / 'trace'
    tracer.mkdir()
    docker = shutil.which('docker')
    (tracer / 'docker').write_text(
        '#!/usr/bin/env python3\nimport json,pathlib,subprocess,sys\n'
        f'p=subprocess.run([{docker!r},*sys.argv[1:]],capture_output=True)\n'
        f'with pathlib.Path({str(out / "docker-errors.log")!r}).open("a") as f:\n'
        ' f.write(json.dumps({"argv":sys.argv[1:],"exit":p.returncode,'
        '"stderr":p.stderr.decode(errors="replace") if p.returncode else ""})+"\\n")\n'
        'sys.stdout.buffer.write(p.stdout);sys.stderr.buffer.write(p.stderr);sys.exit(p.returncode)\n')
    (tracer / 'docker').chmod(0o700)
    env['PATH'] = str(tracer) + os.pathsep + env['PATH']
    def run(action):
        result = subprocess.run([sys.executable, str(path / 'oo'), action, '--bundle', str(path),
            '--manifest-sha256', trust, '--config', str(config), '--json',
            *(['--emulated-client'] if emulated else [])],
            env=env, capture_output=True, timeout=360)
        (out / (action + '.json')).write_bytes(result.stdout)
        (out / (action + '.stderr')).write_bytes(result.stderr)
        (out / (action + '.exit')).write_text(str(result.returncode))
        return result
    try:
        first = run('up')
        report = json.loads(first.stdout)
        assert report['status'] == 'retained_ready' and first.returncode == 0, report
        assert len(report['services']) == 3 and all(row['Healthy'] for row in report['services'])
        runtime = out / '.oo/lite/work/operator/build-runtime.json'
        assert runtime.is_file() and report['build'] == 'not_started'
        assert (path / 'bin/install-retained-target').is_file()
        private = out / '.oo/lite/pki'
        assert (private / 'operator-token').read_bytes().startswith(b'ooct_')
        assert json.loads((private / 'operator-grant.json').read_text())['role'] == 'lite_operator'
        assert (out / '.oo/lite/health-client.json').is_file()
        status = run('status')
        assert status.returncode == 0 and not json.loads(status.stdout)['healthy']
        assert json.loads(status.stdout)['status'] == 'retained_ready'
        (root / 'first-up.json').write_text(json.dumps({
            'offline_install': True, 'healthy_foundations': 3, 'healthy_stack': False,
            'bundle_operator_provisioned': True, 'operator_role': 'lite_operator',
            'retained_chain_provisioned': True,
            'reason': 'owner-reviewed publication and scheduler configuration are not installed by oo up'}))
    finally:
        run('down')
        owner = out / '.oo/lite/owner.json'
        if owner.exists():
            project = json.loads(owner.read_text())['project']
            for kind in ('volume', 'network'):
                names = subprocess.check_output(['docker', kind, 'ls', '-q', '--filter',
                    'label=com.docker.compose.project=' + project], text=True).split()
                for name in names:
                    labels = json.loads(subprocess.check_output(
                        ['docker', kind, 'inspect', '--format', '{{json .Labels}}', name]))
                    assert labels.get('oo.lane') == 'm'
                    subprocess.run(['docker', kind, 'rm', name], check=True, capture_output=True)
        for directory in (home, out / '.oo/lite/pki', out / '.oo/lite/home'):
            if directory.exists():
                shutil.rmtree(directory)


def inside(args):
    path = args.bundle.resolve()
    out = ROOT / 'evidence' / args.run_id
    assert not shutil.which('cargo') and not shutil.which('rustc')
    assert not list(ROOT.rglob('Cargo.toml'))
    assert not list(ROOT.rglob('*.rs'))
    installed = bundle.install(path, args.manifest_sha256, args.host_daemon)
    (out / 'loaded.json').write_text(json.dumps(installed))
    foundation(out, path, args.manifest_sha256, args.host_daemon)
    from test_scheduled_compose import ComposeScheduled, retained
    postgres = installed['images']['postgres']['id']

    class OfflineScheduled(ComposeScheduled):
        def __init__(self, *arguments):
            super().__init__(*arguments)
            self.forwards = []
            self.client_network = None

        def start_databases(self):
            super().start_databases()
            if args.host_daemon:
                networks = json.loads(self.docker(['inspect', '--format',
                    '{{json .NetworkSettings.Networks}}', self.db.name]))
                assert len(networks) == 1
                network, address = next(iter(networks.items()))
                client = os.environ['OO_BUNDLE_TEST_CLIENT']
                labels = json.loads(self.docker(['inspect', '--format',
                                                '{{json .Config.Labels}}', client]))
                assert labels.get('oo.lane') == 'm' and client == 'oo-m-' + args.run_id + '-client'
                self.docker(['network', 'connect', network, client])
                self.client_network = network
                server, worker = forward((address['IPAddress'], 5432))
                self.forwards.append((server, worker))
                self.db.port = server.server_address[1]

        def prepare_runtime(self):
            super().prepare_runtime()
            if args.host_daemon:
                # The initial local Gateway has stopped. Keep its client URL,
                # forwarding TLS unchanged to the Gateway on the owned network.
                server, worker = forward(('gateway', 8443), self.gateway_port)
                self.forwards.append((server, worker))

        def close(self):
            try:
                if self.client_network:
                    self.docker(['network', 'disconnect', self.client_network,
                                 os.environ['OO_BUNDLE_TEST_CLIENT']])
                super().close()
            finally:
                for server, worker in self.forwards:
                    server.shutdown()
                    server.server_close()
                    worker.join(timeout=5)

        def docker(self, argv, timeout=180):
            raw = super().docker(argv, timeout)
            # The old fixture's registry digest is not retained by classic docker load.
            if argv[-3:] == ['config', '--format', 'json']:
                config = json.loads(raw)
                for name in ('agent-db', 'control-db'):
                    config['services'][name].update(image=postgres, pull_policy='never')
                for service in config['services'].values():
                    service['platform'] = installed['platform']
                if args.host_daemon:
                    config['services']['gateway'].pop('ports', None)
                return json.dumps(config).encode()
            return raw

    scheduled = out / 'scheduled'
    scheduled.mkdir(mode=0o700)
    os.environ['OO_SELFBUILD_BUDGET_BIN'] = str(path / 'bin/selfbuild-budget')
    live = OfflineScheduled(scheduled, installed['images']['runtime']['id'])
    try:
        # The upstream fixture has a hard-coded cargo test invocation. Redirect only
        # that exact administrative seam to the bundled precompiled test helper.
        # Do not ship a fake cargo executable or substitute migration SQL.
        original = subprocess.run
        migration = ['cargo', 'test', '-p', 'oo-pipeline', '--locked', '--test',
                     'native_library_publication', 'retained_gateway_install', '--', '--ignored', '--exact']
        calls = []
        def run(argv, *pos, **kw):
            if argv == migration:
                argv = [str(path / 'bin/oo-fixture-migrate'), 'retained_gateway_install',
                        '--ignored', '--exact', '--nocapture']
                calls.append('bundled precompiled retained_gateway_install')
            elif argv and argv[0] == 'cargo':
                raise AssertionError('unexpected compiler dependency')
            return original(argv, *pos, **kw)
        with patch.object(subprocess, 'run', side_effect=run):
            live.start()
        assert len(calls) == 1
        (out / 'migration-helper.json').write_text(json.dumps({'calls': calls, 'cargo_available': False}))
        live.cycles()
    finally:
        live.close()
        retained.cycles.checksums(scheduled)


def stage_harness(dest, path, out, run_id):
    """Stage fixture drivers/setup utilities; launched products and SQL are bundled."""
    files = [
        'deploy/lite/test_bundle_dind.py', 'deploy/lite/test_scheduled_compose.py',
        'deploy/lite/test_scheduled_live.py', 'deploy/lite/lifecycle.py',
        'tools/release-coordinator/tests/retained_cycle_live.py',
        'tools/release-coordinator/tests/retained_fixture.py',
        'tools/oo-load/test-gateway-cycles.py',
        'tools/source-onboarding/tests/refresh_live_catalog_owned_pg.py',
        'testing/engines/seed-common/generate.py',
    ]
    files += ['tools/oo-test/' + name + '.py' for name in
              ('t1', 'runner', 'comparison', 'oracle_t1', 'audit', 'matrix', 'verdicts')]
    for name in files:
        target = dest / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(ROOT / name, target)
    for name in bundle.MIGRATIONS:
        target = dest / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.symlink_to(path / 'support' / name)
    for name in ('bundle.py', 'remote.py', 'scheduler.py', 'compose.yaml', 'postgres.sh'):
        (dest / 'deploy/lite' / name).symlink_to(path / name)
    (dest / 'target').mkdir()
    (dest / 'target/debug').symlink_to(path / 'bin')
    (dest / 'tools/pg-builder').mkdir()
    (dest / 'tools/pg-builder/scripts').symlink_to('/opt/oo/pg-builder')
    (dest / 'evidence').mkdir()
    (dest / 'evidence' / run_id).symlink_to(out)
    assert not list(dest.rglob('Cargo.toml')) and not list(dest.rglob('*.rs'))


def cleanup_siblings(out, docker):
    """The client may time out; clean only its two recorded Compose projects."""
    for phase in ('foundation', 'scheduled'):
        owner = out / phase / '.oo/lite/owner.json'
        if not owner.exists():
            continue
        project = json.loads(owner.read_text())['project']
        assert re.fullmatch('oo-a-lite-[0-9a-f]{16}', project)
        for kind in ('container', 'volume', 'network'):
            command = [docker, kind, 'ls', '-q', '--filter',
                       'label=com.docker.compose.project=' + project]
            if kind == 'container':
                command.insert(3, '-a')
            names = subprocess.check_output(command, text=True).split()
            for name in names:
                fmt = '{{json .Config.Labels}}' if kind == 'container' else '{{json .Labels}}'
                labels = json.loads(subprocess.check_output(
                    [docker, kind, 'inspect', '--format', fmt, name]))
                assert labels.get('oo.lane') == 'm'
                assert labels.get('com.docker.compose.project') == project
                subprocess.run([docker, kind, 'rm', *(['-f'] if kind == 'container' else []), name],
                               check=True, capture_output=True)
        for private in (out / phase / 'home', out / phase / '.private',
                        out / phase / '.oo/lite/home', out / phase / '.oo/lite/pki'):
            if private.exists():
                shutil.rmtree(private)


def host(args):
    assert re.fullmatch('[a-z0-9-]+', args.run_id)
    path = args.bundle.resolve()
    manifest = bundle.verify(path, args.manifest_sha256)
    arch = manifest['platform'].split('/')[1]
    assert args.host_daemon == (arch == 'amd64'), 'use host client only for amd64'
    out = ROOT / 'evidence' / args.run_id
    out.mkdir(mode=0o700)
    name = 'oo-m-' + args.run_id
    volume = name + '-data'
    docker = shutil.which('docker')
    assert docker
    assert subprocess.run([docker, 'inspect', name], capture_output=True).returncode != 0
    assert subprocess.run([docker, 'volume', 'inspect', volume], capture_output=True).returncode != 0

    def run(argv, **kwargs):
        result = subprocess.run([docker, *argv], capture_output=True, **kwargs)
        with (out / 'docker.log').open('ab') as log:
            log.write(result.stdout + result.stderr)
        assert result.returncode == 0, 'container operation; see docker.log'
        return result.stdout

    created_volume = created = False
    client = name + '-client'
    try:
        if args.host_daemon:
            run(['create', '--name', name, '--platform', manifest['platform'],
                 '--label', 'oo.lane=m', DIND[arch]])
            created = True
            runtime = manifest['images']['runtime']['source_id']
            server = run(['version', '--format', '{{.Server.Os}}/{{.Server.Arch}}']).decode().strip()
        else:
            run(['volume', 'create', '--label', 'oo.lane=m', volume])
            created_volume = True
            run(['run', '-d', '--name', name, '--platform', manifest['platform'],
                 '--label', 'oo.lane=m', '--privileged', '--cgroupns', 'private', '--network', 'none',
                 '--mount', f'type=volume,src={volume},dst=/var/lib/docker',
                 '--mount', f'type=bind,src={out},dst={out}',
                 '--mount', f'type=bind,src={path},dst={path},readonly',
                 '--entrypoint', 'dind', DIND[arch], 'dockerd', '--host=unix:///var/run/docker.sock'])
            created = True
            for _ in range(90):
                if subprocess.run([docker, 'exec', name, 'docker', 'info'], capture_output=True).returncode == 0:
                    break
                time.sleep(1)
            else:
                raise RuntimeError('DIND_NOT_READY')
            assert not run(['exec', name, 'docker', 'image', 'ls', '-q']).strip()
            shim = out / 'shim'
            shim.mkdir()
            script = shim / 'docker'
            script.write_text('#!/bin/sh\nexec ' + docker + ' exec -i ' + name + ' docker "$@"\n')
            script.chmod(0o700)
            old_path = os.environ['PATH']
            try:
                os.environ['PATH'] = str(shim) + os.pathsep + old_path
                installed = bundle.install(path, args.manifest_sha256)
            finally:
                os.environ['PATH'] = old_path
            (out / 'loaded-host.json').write_text(json.dumps(installed))
            runtime = installed['images']['runtime']['id']
            server = run(['exec', name, 'docker', 'version', '--format',
                          '{{.Server.Os}}/{{.Server.Arch}}']).decode().strip()
        tools = out / 'docker-tools'
        tools.mkdir()
        plugins = tools / 'cli-plugins'
        plugins.mkdir()
        run(['cp', name + ':/usr/local/bin/docker', str(tools / 'docker')])
        run(['cp', name + ':/usr/local/libexec/docker/cli-plugins/docker-compose',
             str(plugins / 'docker-compose')])
        harness = out / 'harness'
        stage_harness(harness, path, out, args.run_id)
        (out / 'isolation.json').write_text(json.dumps({
            'client_platform': manifest['platform'], 'daemon_platform': server,
            'daemon': 'host Docker Desktop' if args.host_daemon else 'isolated DinD',
            'outer_network': 'bridge; client joins owned fixture network' if args.host_daemon else 'none',
            'checkout_mounted': False, 'fixture_harness': 'staged Python test drivers only',
            'cargo_available': False, 'product_binaries': 'bundle only'}))
        argv = ([] if args.host_daemon else ['exec', name, 'docker']) + [
            'run', '--rm', '--name', client, '--platform', manifest['platform'], '--label', 'oo.lane=m',
            '--network', 'bridge' if args.host_daemon else 'host',
            '--mount', f'type=bind,src={out},dst={out}',
            '--mount', f'type=bind,src={path},dst={path},readonly',
            '--mount', 'type=bind,src=/var/run/docker.sock,dst=/var/run/docker.sock',
            '--env', f'PATH={tools}:/usr/local/bin:/usr/bin:/bin',
            '--env', f'DOCKER_CONFIG={tools}', '--env', 'DOCKER_DEFAULT_PLATFORM=' + manifest['platform'],
            '--env', 'OO_BUNDLE_TEST_CLIENT=' + client,
            '--entrypoint', 'python3', runtime,
            str(harness / 'deploy/lite/test_bundle_dind.py'), '--inside', '--bundle', str(path),
            '--manifest-sha256', args.manifest_sha256, '--run-id', args.run_id,
            *(['--host-daemon'] if args.host_daemon else [])]
        run(argv, timeout=1200)
    finally:
        if args.host_daemon:
            # Timeout cleanup is scoped to this exact client, never lane-wide.
            state = subprocess.run([docker, 'inspect', '--format', '{{json .Config.Labels}}', client], capture_output=True)
            if state.returncode == 0:
                assert json.loads(state.stdout).get('oo.lane') == 'm'
                run(['rm', '-f', client])
            cleanup_siblings(out, docker)
        if created:
            logs = subprocess.run([docker, 'logs', name], capture_output=True)
            (out / 'dind.log').write_bytes(logs.stdout + logs.stderr)
            labels = json.loads(subprocess.check_output([docker, 'inspect', '--format',
                                                        '{{json .Config.Labels}}', name]))
            assert labels.get('oo.lane') == 'm'
            run(['rm', '-f', '-v', name])
        if created_volume:
            run(['volume', 'rm', volume])

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bundle', type=Path, required=True)
    parser.add_argument('--manifest-sha256', required=True)
    parser.add_argument('--run-id', required=True)
    parser.add_argument('--inside', action='store_true')
    parser.add_argument('--host-daemon', action='store_true')
    args = parser.parse_args()
    os.umask(0o077)
    (inside if args.inside else host)(args)
