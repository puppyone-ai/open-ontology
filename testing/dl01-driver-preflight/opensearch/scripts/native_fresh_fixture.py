#!/usr/bin/env python3
"""Create an isolated original-version fixture without restarting retained resources.

The approved exact index stays unchanged but lives in a distinct cluster/network.
Only the source container receives certificates and hashed users. Raw credentials
remain host-private and are mounted only by the approved reader runner.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[4]
SRC = ROOT / 'testing/dl01-driver-preflight/opensearch'
OUT = ROOT / 'evidence/bc6cf794-2d14-4e98-bcb6-f7a9b3533a88/DL01/opensearch-driver-preflight/native-fresh-20261012-r2'
PRIVATE = Path('/tmp/oo-dl01w1-44-native-fresh-private-r2')
NAME = 'oo-dl01w1-44-native-fresh-source'
NET = 'oo-dl01w1-44-native-fresh-net'
VOL = 'oo-dl01w1-44-native-fresh-data'
IMAGE = 'opensearchproject/opensearch@sha256:cf05462a958be93fe2172aea2c6757debc7d3a1e4a6838d39e56af591a3dcdbb'


def run(name, argv):
    dest = OUT / name
    dest.mkdir(parents=True, exist_ok=False)
    proc = subprocess.run(argv, capture_output=True)
    (dest / 'stdout').write_bytes(proc.stdout)
    (dest / 'stderr').write_bytes(proc.stderr)
    (dest / 'exit').write_text(str(proc.returncode) + '\n')
    (dest / 'argv.json').write_text(json.dumps(argv) + '\n')
    if proc.returncode:
        raise SystemExit(f'{name} failed: {proc.returncode}; raw retained')
    return proc.stdout


def main():
    OUT.mkdir(parents=True, exist_ok=False)
    if PRIVATE.exists():
        raise SystemExit('private destination exists; refusing overwrite')
    old = Path('/tmp/oo-dl01w1-44-private')
    PRIVATE.mkdir(mode=0o700)
    for name in ('certs', 'security'):
        shutil.copytree(old / name, PRIVATE / name)
        for p in (PRIVATE / name).rglob('*'):
            os.chmod(p, 0o755 if p.is_dir() else 0o644)
    for name in ('identities.env', 'opensearch.yml'):
        shutil.copyfile(old / name, PRIVATE / name)
        os.chmod(PRIVATE / name, 0o600 if name == 'identities.env' else 0o644)
    (PRIVATE / 'endpoint.url').write_text('https://oo-dl01w1-44-opensearch:9200\n')
    os.chmod(PRIVATE / 'endpoint.url', 0o600)
    run('network-create', ['docker', 'network', 'create', '--subnet', '10.44.144.0/24', '--label', 'owner=oo-dl01w1-44', NET])
    run('volume-create', ['docker', 'volume', 'create', '--label', 'owner=oo-dl01w1-44', VOL])
    run('source-create', ['docker', 'run', '-d', '--name', NAME, '--pull', 'never',
        '--network', NET, '--network-alias', 'oo-dl01w1-44-opensearch',
        '--publish', '127.0.0.1:19344:9200', '--memory', '2g', '--memory-swap', '2g',
        '--label', 'owner=oo-dl01w1-44', '--label', 'task=DL01-W1-44-native',
        '--env', 'DISABLE_INSTALL_DEMO_CONFIG=true', '--env', 'OPENSEARCH_JAVA_OPTS=-Xms512m -Xmx512m',
        '-v', f'{PRIVATE}/opensearch.yml:/usr/share/opensearch/config/opensearch.yml:ro',
        '-v', f'{PRIVATE}/certs:/usr/share/opensearch/config/certs:ro',
        '-v', f'{PRIVATE}/security:/usr/share/opensearch/config/opensearch-security:ro',
        '-v', f'{VOL}:/usr/share/opensearch/data', IMAGE])
    run('resource-inspect', ['docker', 'inspect', '--format', '{{.Id}} {{.State.Status}} {{.Image}} {{.HostConfig.NanoCpus}}', NAME])
    (OUT / 'resources.json').write_text(json.dumps({'source': NAME, 'network': NET, 'volume': VOL,
        'private_path': str(PRIVATE), 'original_resources_restarted': False,
        'source_version_expected': '3.9.0', 'source_acceptance': 'not_claimed'}, indent=2) + '\n')
    print('FRESH_CREATED: startup is not product closure')


if __name__ == '__main__':
    main()
