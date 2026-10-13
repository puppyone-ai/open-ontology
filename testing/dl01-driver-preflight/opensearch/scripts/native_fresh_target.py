#!/usr/bin/env python3
"""Create own isolated SCRAM target; migrations and source trust remain product work."""
import json
import os
from pathlib import Path
import secrets
import subprocess

ROOT = Path(__file__).resolve().parents[4]
BASE = ROOT / 'evidence/bc6cf794-2d14-4e98-bcb6-f7a9b3533a88/DL01/opensearch-driver-preflight/native-fresh-20261012-r2'
OUT = BASE / 'target'
PRIVATE = Path('/tmp/oo-dl01w1-44-native-fresh-private-r2')
NAME = 'oo-dl01w1-44-native-fresh-pg'
DB = 'oo_dl01w1_44_native_fresh'
VOL = 'oo-dl01w1-44-native-fresh-pgdata'
IMAGE = 'docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb'


def phase(name, cmd, data=None):
    dest = OUT / name
    dest.mkdir(parents=True, exist_ok=False)
    p = subprocess.run(cmd, input=data, capture_output=True)
    (dest / 'stdout').write_bytes(p.stdout)
    (dest / 'stderr').write_bytes(p.stderr)
    (dest / 'exit').write_text(str(p.returncode) + '\n')
    if p.returncode:
        raise SystemExit(f'{name}: exit{p.returncode}, raw retained')


def main():
    OUT.mkdir(parents=True, exist_ok=False)
    pw = PRIVATE / 'native-admin.pw'
    if pw.exists():
        raise SystemExit('admin credential exists; refusing replacement')
    pw.write_text(secrets.token_hex(32) + '\n')
    os.chmod(pw, 0o600)
    phase('volume-create', ['docker', 'volume', 'create', '--label', 'owner=oo-dl01w1-44', VOL])
    phase('create', ['docker', 'run', '-d', '--pull', 'never', '--name', NAME,
        '--network', 'none', '--label', 'owner=oo-dl01w1-44', '--label', 'task=DL01-W1-44-native',
        '--env', f'POSTGRES_DB={DB}', '--env', 'POSTGRES_PASSWORD_FILE=/private/admin.pw',
        '--env', 'POSTGRES_INITDB_ARGS=--auth-host=scram-sha-256',
        '-v', f'{pw}:/private/admin.pw:ro', '-v', f'{VOL}:/var/lib/postgresql/data', IMAGE])
    phase('inspect', ['docker', 'inspect', '--format', '{{.Id}} {{.State.Status}} {{.Image}} {{.HostConfig.NanoCpus}} {{.HostConfig.NetworkMode}}', NAME])
    (OUT / 'resources.json').write_text(json.dumps({'target': NAME, 'database': DB, 'volume': VOL,
        'host_port': None, 'network': 'none', 'tls': 'pending_configuration',
        'product_migrations_installed': False, 'source_acceptance': 'not_claimed'}, indent=2) + '\n')
    print('TARGET_CREATED: migrations, runtime grants and trusted load pending')


if __name__ == '__main__':
    main()
