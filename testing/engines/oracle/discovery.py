#!/usr/bin/env python3
"""Oracle catalog discovery against one throwaway Oracle Free container.

Labelled oo.lane=connector-discovery, loopback port, removed afterwards; only
the container this script started is touched. Passwords live in a private
temp directory and in this container's environment only.

    python3 -I testing/engines/oracle/discovery.py [--image gvenzl/oracle-free:23-slim]
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
WHENEVER SQLERROR EXIT FAILURE
CREATE TABLE regions (code VARCHAR2(10) PRIMARY KEY);
CREATE TABLE customers (id NUMBER PRIMARY KEY, region VARCHAR2(10));
CREATE TABLE orders (id NUMBER PRIMARY KEY, customer_id NUMBER REFERENCES customers,
  region VARCHAR2(10) REFERENCES regions);
CREATE TABLE notes (id NUMBER NOT NULL UNIQUE, body VARCHAR2(100));
CREATE GLOBAL TEMPORARY TABLE scratch (id NUMBER);
INSERT INTO customers SELECT level, 'r' FROM dual CONNECT BY level <= 300;
COMMIT;
EXEC DBMS_STATS.GATHER_TABLE_STATS(USER, 'CUSTOMERS');
EXIT
"""


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--image', default='gvenzl/oracle-free:23-slim')
    ap.add_argument('--service', default='FREEPDB1')
    a = ap.parse_args()
    with tempfile.TemporaryDirectory(prefix='oo-ora-discovery-') as tmp:
        app = secrets.token_hex(10)
        (Path(tmp) / 'app').write_text(app)
        name = f'oo-ora-discovery-{secrets.token_hex(4)}'
        subprocess.run(['docker', 'run', '-d', '--name', name, '--label', 'oo.lane=connector-discovery',
                        '-p', '127.0.0.1::1521', '-e', f'ORACLE_PASSWORD={secrets.token_hex(10)}',
                        '-e', 'APP_USER=oo_app', '-e', f'APP_USER_PASSWORD={app}', a.image],
                       check=True, capture_output=True)
        try:
            for _ in range(300):
                logs = subprocess.run(['docker', 'logs', name], capture_output=True, text=True)
                if 'DATABASE IS READY TO USE' in logs.stdout + logs.stderr:
                    break
                time.sleep(2)
            else:
                raise RuntimeError('oracle did not start')
            subprocess.run(['docker', 'exec', '-i', name, 'sqlplus', '-s', f'oo_app/{app}@localhost/{a.service}'],
                           input=FIXTURE.encode(), check=True, capture_output=True)
            port = subprocess.run(['docker', 'port', name, '1521/tcp'], check=True, capture_output=True,
                                  text=True).stdout.strip().splitlines()[0].rsplit(':', 1)[1]
            env = dict(os.environ, OO_ORA_DISCOVERY_PORT=port, OO_ORA_DISCOVERY_SERVICE=a.service,
                       OO_ORA_DISCOVERY_PASSWORD=str(Path(tmp) / 'app'))
            return subprocess.run(['cargo', 'test', '-p', 'oo-connectors', '--features', 'oracle', '--test',
                                   'oracle_discovery_live', '--', '--ignored', '--nocapture'], cwd=ROOT,
                                  env=env).returncode
        finally:
            subprocess.run(['docker', 'rm', '-f', '-v', name], capture_output=True)


if __name__ == '__main__':
    sys.exit(main())
