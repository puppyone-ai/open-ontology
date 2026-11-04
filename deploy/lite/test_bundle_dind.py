#!/usr/bin/env python3
"""Offline bundle smoke plus fixture-provisioned cycles in one disposable dind.

The outer daemon has network=none. This never changes binfmt or sysctl and
never mounts the host Docker socket. Run architectures serially (two DBs max).
Foundation-only first up is reported separately from fixture-assisted cycles.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import subprocess
import sys
import time

import bundle

ROOT = Path(__file__).resolve().parents[2]
DIND = {
    'arm64': 'docker.io/library/docker@sha256:ad9e28ccfdf76cf44248258090e07d8353b9628725d8a716bab176bd71200d7d',
    'amd64': 'docker.io/library/docker@sha256:f649ef046008ca7f926a2571c32b0ac22e5c59eb61b959617f9acc2a4c638cf5',
}


def foundation(root, path, trust):
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
            '--manifest-sha256', trust, '--config', str(config), '--json'],
            env=env, capture_output=True, timeout=360)
        (out / (action + '.json')).write_bytes(result.stdout)
        (out / (action + '.stderr')).write_bytes(result.stderr)
        (out / (action + '.exit')).write_text(str(result.returncode))
        return result
    try:
        first = run('up')
        report = json.loads(first.stdout)
        assert report['status'] == 'foundations_only' and first.returncode == 1, report
        assert len(report['services']) == 3 and all(row['Healthy'] for row in report['services'])
        status = run('status')
        assert status.returncode == 1 and not json.loads(status.stdout)['healthy']
        (root / 'first-up.json').write_text(json.dumps({
            'offline_install': True, 'healthy_foundations': 3, 'healthy_stack': False,
            'reason': 'operator publication and scheduler configuration are not installed by oo up'}))
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
    installed = bundle.install(path, args.manifest_sha256)
    (out / 'loaded.json').write_text(json.dumps(installed))
    foundation(out, path, args.manifest_sha256)
    from test_scheduled_compose import ComposeScheduled, retained
    postgres = installed['images']['postgres']['id']

    class OfflineScheduled(ComposeScheduled):
        def docker(self, argv, timeout=180):
            raw = super().docker(argv, timeout)
            # The old fixture's registry digest is not retained by classic docker load.
            if argv[-3:] == ['config', '--format', 'json']:
                config = json.loads(raw)
                for name in ('agent-db', 'control-db'):
                    config['services'][name].update(image=postgres, pull_policy='never')
                return json.dumps(config).encode()
            return raw

    scheduled = out / 'scheduled'
    scheduled.mkdir(mode=0o700)
    os.environ['OO_SELFBUILD_BUDGET_BIN'] = str(path / 'bin/selfbuild-budget')
    live = OfflineScheduled(scheduled, installed['images']['runtime']['id'])
    try:
        live.start()
        live.cycles()
    finally:
        live.close()
        retained.cycles.checksums(scheduled)


def host(args):
    assert re.fullmatch('[a-z0-9-]+', args.run_id)
    path = args.bundle.resolve()
    manifest = bundle.verify(path, args.manifest_sha256)
    arch = manifest['platform'].split('/')[1]
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
        assert result.returncode == 0, 'dind operation; see docker.log'
        return result.stdout

    created_volume = created = False
    try:
        run(['volume', 'create', '--label', 'oo.lane=m', volume])
        created_volume = True
        run(['run', '-d', '--name', name, '--platform', manifest['platform'],
             '--label', 'oo.lane=m', '--privileged', '--cgroupns', 'private', '--network', 'none',
             '--mount', f'type=volume,src={volume},dst=/var/lib/docker',
             '--mount', f'type=bind,src={ROOT},dst={ROOT}',
             '--entrypoint', 'dind', DIND[arch], 'dockerd', '--host=unix:///var/run/docker.sock'])
        created = True
        for _ in range(90):
            ready = subprocess.run([docker, 'exec', name, 'docker', 'info'], capture_output=True)
            if ready.returncode == 0:
                break
            time.sleep(1)
        else:
            raise RuntimeError('DIND_NOT_READY')
        assert not run(['exec', name, 'docker', 'image', 'ls', '-q']).strip()
        (out / 'isolation.json').write_bytes(run(['inspect', '--format',
            '{{json .HostConfig.NetworkMode}}', name]))
        # Route the shared verifier/loader to this private, initially empty daemon.
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
        # Only installer tooling is copied from dind; product binaries come from the bundle.
        tools = out / 'docker-tools'
        tools.mkdir()
        plugins = tools / 'cli-plugins'
        plugins.mkdir()
        run(['cp', name + ':/usr/local/bin/docker', str(tools / 'docker')])
        run(['cp', name + ':/usr/local/libexec/docker/cli-plugins/docker-compose',
             str(plugins / 'docker-compose')])
        runtime = installed['images']['runtime']['id']
        argv = ['exec', name, 'docker', 'run', '--rm', '--label', 'oo.lane=m',
                '--network', 'host', '--mount', f'type=bind,src={ROOT},dst={ROOT}',
                '--mount', f'type=bind,src={path / "bin"},dst={ROOT / "target/debug"},readonly',
                '--mount', 'type=bind,src=/var/run/docker.sock,dst=/var/run/docker.sock',
                '--env', f'PATH={tools}:/usr/local/bin:/usr/bin:/bin',
                '--env', f'DOCKER_CONFIG={tools}', '--entrypoint', 'python3', runtime,
                str(Path(__file__).resolve()), '--inside', '--bundle', str(path),
                '--manifest-sha256', args.manifest_sha256, '--run-id', args.run_id]
        run(argv, timeout=1200)
    finally:
        if created:
            logs = subprocess.run([docker, 'logs', name], capture_output=True)
            (out / 'dind.log').write_bytes(logs.stdout + logs.stderr)
            labels = json.loads(subprocess.check_output([docker, 'inspect', '--format',
                                                        '{{json .Config.Labels}}', name]))
            assert labels.get('oo.lane') == 'm'
            run(['rm', '-f', name])
        if created_volume:
            run(['volume', 'rm', volume])


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bundle', type=Path, required=True)
    parser.add_argument('--manifest-sha256', required=True)
    parser.add_argument('--run-id', required=True)
    parser.add_argument('--inside', action='store_true')
    args = parser.parse_args()
    os.umask(0o077)
    (inside if args.inside else host)(args)
