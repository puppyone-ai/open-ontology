#!/usr/bin/env python3
"""MySQL catalog discovery against throwaway TLS MySQL containers.

One container per version, labelled oo.lane=connector-discovery, loopback
port, removed afterwards; only containers this script started are touched.
Passwords stay in a private temp directory and the container's environment.

    python3 -I testing/engines/mysql/discovery.py [--versions 5.7.44,8.0.44,8.4.7,9.5.0]
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

FIXTURE = """
CREATE DATABASE shop; CREATE DATABASE ref; CREATE DATABASE analytics; CREATE DATABASE restricted;
CREATE TABLE ref.regions (code VARCHAR(10) PRIMARY KEY) ENGINE=InnoDB;
CREATE TABLE shop.customers (id BIGINT PRIMARY KEY, region VARCHAR(10)) ENGINE=InnoDB;
CREATE TABLE shop.orders (id BIGINT PRIMARY KEY, customer_id BIGINT, region VARCHAR(10),
  FOREIGN KEY (customer_id) REFERENCES shop.customers (id),
  FOREIGN KEY (region) REFERENCES ref.regions (code)) ENGINE=InnoDB;
CREATE TABLE shop.notes (id BIGINT NOT NULL UNIQUE, body TEXT) ENGINE=InnoDB;
CREATE VIEW shop.recent_orders AS SELECT id FROM shop.orders;
CREATE TABLE analytics.daily (d DATE PRIMARY KEY);
INSERT INTO shop.customers (id, region)
  SELECT a.n + b.n * 10 + c.n * 100 + 1, 'r' FROM
  (SELECT 0 n UNION SELECT 1 UNION SELECT 2 UNION SELECT 3 UNION SELECT 4 UNION SELECT 5 UNION SELECT 6 UNION SELECT 7 UNION SELECT 8 UNION SELECT 9) a,
  (SELECT 0 n UNION SELECT 1 UNION SELECT 2 UNION SELECT 3 UNION SELECT 4 UNION SELECT 5 UNION SELECT 6 UNION SELECT 7 UNION SELECT 8 UNION SELECT 9) b,
  (SELECT 0 n UNION SELECT 1 UNION SELECT 2) c;
ANALYZE TABLE shop.customers;
CREATE USER 'oo_reader'@'%' IDENTIFIED BY '{reader}' REQUIRE SSL;
GRANT SELECT ON shop.* TO 'oo_reader'@'%';
GRANT SELECT ON ref.* TO 'oo_reader'@'%';
GRANT SELECT ON analytics.* TO 'oo_reader'@'%';
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
    if subprocess.run(['openssl', 'rsa', '-in', str(d / 'key.pem'), '-traditional', '-out', str(d / 'key-rsa.pem')],
                      capture_output=True).returncode == 0:
        (d / 'key-rsa.pem').replace(d / 'key.pem')  # 5.7's yaSSL/OpenSSL 1.0 reads PKCS#1
    os.chmod(d / 'key.pem', 0o644)


def one(version: str, d: Path, run_id: str) -> int:
    root = secrets.token_hex(12)
    reader = secrets.token_hex(12)
    (d / 'reader').write_text(reader)
    script = ('cp /certs/ca.pem /certs/cert.pem /certs/key.pem /tmp/ && chown mysql /tmp/*.pem '
              '&& chmod 600 /tmp/key.pem && exec docker-entrypoint.sh mysqld --ssl-ca=/tmp/ca.pem '
              '--ssl-cert=/tmp/cert.pem --ssl-key=/tmp/key.pem')
    name = f'oo-my-discovery-{secrets.token_hex(4)}'
    platform = ['--platform', 'linux/amd64'] if version.startswith('5.7') else []
    subprocess.run(['docker', 'run', '-d', '--name', name, *platform, '--label', 'oo.lane=connector-discovery',
                    '--label', f'oo.run_id={run_id}', '-e', f'MYSQL_ROOT_PASSWORD={root}',
                    '-p', '127.0.0.1::3306', '-v', f'{d}:/certs:ro', '--entrypoint', 'sh',
                    f'mysql:{version}', '-c', script], check=True, capture_output=True)
    try:
        for _ in range(180):
            ok = subprocess.run(['docker', 'exec', '-e', f'MYSQL_PWD={root}', name, 'mysql', '-uroot',
                                 '-h127.0.0.1', '-e', 'SELECT 1'], capture_output=True).returncode == 0
            if ok:
                break
            time.sleep(2)
        else:
            raise RuntimeError(f'mysql {version} did not start')
        subprocess.run(['docker', 'exec', '-i', '-e', f'MYSQL_PWD={root}', name, 'mysql', '-uroot', '-h127.0.0.1'],
                       input=FIXTURE.replace('{reader}', reader).encode(), check=True, capture_output=True)
        port = subprocess.run(['docker', 'port', name, '3306/tcp'], check=True, capture_output=True,
                              text=True).stdout.strip().splitlines()[0].rsplit(':', 1)[1]
        env = dict(os.environ, OO_MY_DISCOVERY_DIR=str(d), OO_MY_DISCOVERY_PORT=port)
        return subprocess.run(['cargo', 'test', '-p', 'oo-connectors', '--features', 'mysql', '--test',
                               'mysql_discovery_live', '--', '--ignored', '--nocapture'], cwd=ROOT,
                              env=env).returncode
    finally:
        subprocess.run(['docker', 'rm', '-f', '-v', name], capture_output=True)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--versions', default='8.4.7')
    a = ap.parse_args()
    run_id = secrets.token_hex(4)
    failed = []
    for version in a.versions.split(','):
        with tempfile.TemporaryDirectory(prefix=f'oo-my-discovery-') as tmp:
            d = Path(tmp)
            certificates(d)
            code = one(version, d, run_id)
            print(f'mysql {version}: {"ok" if code == 0 else "FAILED"}', flush=True)
            if code:
                failed.append(version)
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main())
