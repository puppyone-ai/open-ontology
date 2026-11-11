#!/usr/bin/env python3
"""Catalog discovery against throwaway TLS PostgreSQL containers.

One labelled container (oo.lane=connector-discovery) per version on a
loopback port, removed afterwards. Only containers this script started are
touched. Secrets stay in a private temp directory, never in argv or logs.

    python3 -I testing/engines/postgres/discovery.py [--versions 13,14,15,16,17,18]
"""
import argparse
import os
from pathlib import Path
import secrets
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[3]
PINS = {13: '13.23', 14: '14.24', 15: '15.19', 16: '16.15', 17: '17.11', 18: '18.6'}

FIXTURE = """
CREATE DATABASE analytics;
CREATE DATABASE restricted;
REVOKE CONNECT ON DATABASE restricted FROM PUBLIC;
\\connect logistics
CREATE SCHEMA sales;
CREATE SCHEMA ref;
CREATE TABLE ref.regions (code text PRIMARY KEY);
CREATE TABLE sales.customers (id bigint PRIMARY KEY, region text);
CREATE TABLE sales.orders (id bigint PRIMARY KEY,
  customer_id bigint REFERENCES sales.customers, region text REFERENCES ref.regions);
CREATE TABLE sales.notes (id bigint NOT NULL UNIQUE, body text);
CREATE TABLE sales.events (at date NOT NULL, kind text) PARTITION BY RANGE (at);
CREATE TABLE sales.events_2025 PARTITION OF sales.events FOR VALUES FROM ('2025-01-01') TO ('2026-01-01');
CREATE TABLE sales.hidden (id int);
INSERT INTO sales.customers SELECT i, 'r' FROM generate_series(1, 300) i;
ANALYZE sales.customers;
GRANT USAGE ON SCHEMA sales, ref TO oo_reader;
GRANT SELECT ON sales.customers, sales.orders, sales.notes, sales.events, sales.events_2025, ref.regions TO oo_reader;
"""


def openssl(*args):
    subprocess.run(['openssl', *args], check=True, capture_output=True)


def certificates(d: Path):
    (d / 'leaf.cnf').write_text(
        '[req]\ndistinguished_name=dn\nprompt=no\n[dn]\nCN=localhost\n'
        '[v3]\nbasicConstraints=critical,CA:FALSE\nsubjectAltName=DNS:localhost,IP:127.0.0.1\n'
        'keyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n')
    openssl('req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-days', '2', '-keyout', str(d / 'ca.key'),
            '-out', str(d / 'ca.pem'), '-subj', '/CN=oo discovery CA')
    openssl('req', '-newkey', 'rsa:2048', '-nodes', '-keyout', str(d / 'key.pem'), '-out', str(d / 'leaf.csr'),
            '-config', str(d / 'leaf.cnf'))
    openssl('x509', '-req', '-in', str(d / 'leaf.csr'), '-CA', str(d / 'ca.pem'), '-CAkey', str(d / 'ca.key'),
            '-CAcreateserial', '-out', str(d / 'cert.pem'), '-days', '2', '-extfile', str(d / 'leaf.cnf'),
            '-extensions', 'v3')
    os.chmod(d / 'key.pem', 0o644)  # copied and re-owned inside the container


def one(major: int, d: Path, run_id: str) -> int:
    admin = secrets.token_hex(12)
    reader = secrets.token_hex(12)
    (d / 'reader').write_text(reader)
    script = ('cp /certs/cert.pem /certs/key.pem /tmp/ && chown postgres /tmp/cert.pem /tmp/key.pem '
              '&& chmod 600 /tmp/key.pem && exec docker-entrypoint.sh postgres -c ssl=on '
              '-c ssl_cert_file=/tmp/cert.pem -c ssl_key_file=/tmp/key.pem')
    name = f'oo-discovery-{major}-{secrets.token_hex(4)}'
    subprocess.run(
        ['docker', 'run', '-d', '--rm', '--name', name, '--label', 'oo.lane=connector-discovery',
         '--label', f'oo.run_id={run_id}', '--memory', '1g', '-e', 'POSTGRES_USER=admin',
         '-e', f'POSTGRES_PASSWORD={admin}', '-e', 'POSTGRES_DB=logistics', '-p', '127.0.0.1::5432',
         '-v', f'{d}:/certs:ro', '--entrypoint', 'sh', f'postgres:{PINS[major]}-bookworm', '-c', script],
        check=True, capture_output=True)
    try:
        for _ in range(90):
            if subprocess.run(['docker', 'exec', name, 'pg_isready', '-U', 'admin', '-d', 'logistics',
                               '-h', '127.0.0.1'], capture_output=True).returncode == 0:
                break
            time.sleep(1)
        else:
            raise RuntimeError(f'pg{major} did not start')
        setup = (f"CREATE ROLE oo_reader LOGIN PASSWORD '{reader}' NOSUPERUSER NOCREATEDB NOCREATEROLE;\n"
                 + FIXTURE)
        subprocess.run(['docker', 'exec', '-i', name, 'psql', '-q', '-U', 'admin', '-d', 'logistics',
                        '-v', 'ON_ERROR_STOP=1'], input=setup.encode(), check=True, capture_output=True)
        port = subprocess.run(['docker', 'port', name, '5432/tcp'], check=True, capture_output=True,
                              text=True).stdout.strip().splitlines()[0].rsplit(':', 1)[1]
        env = dict(os.environ, OO_PG_DISCOVERY_DIR=str(d), OO_PG_DISCOVERY_PORT=port,
                   OO_PG_DISCOVERY_MAJOR=str(major))
        p = subprocess.run(['cargo', 'test', '-p', 'oo-connectors', '--test', 'pg_discovery_live', '--',
                            '--ignored', '--nocapture'], cwd=ROOT, env=env)
        return p.returncode
    finally:
        subprocess.run(['docker', 'rm', '-f', '-v', name], capture_output=True)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--versions', default='17')
    a = ap.parse_args()
    run_id = secrets.token_hex(4)
    failed = []
    for major in map(int, a.versions.split(',')):
        with tempfile.TemporaryDirectory(prefix=f'oo-discovery-{major}-') as tmp:
            d = Path(tmp)
            certificates(d)
            code = one(major, d, run_id)
            print(f'pg{major}: {"ok" if code == 0 else "FAILED"}', flush=True)
            if code:
                failed.append(major)
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main())
