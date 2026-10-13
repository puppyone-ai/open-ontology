#!/usr/bin/env python3
"""Fixture DBA TLS and restricted loader login bootstrap, without product receipts."""
import importlib.util
import os
from pathlib import Path
import secrets

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('target', HERE / 'native_fresh_target.py')
t = importlib.util.module_from_spec(spec)
spec.loader.exec_module(t)


def main():
    runtime = t.PRIVATE / 'native-runtime.pw'
    if runtime.exists():
        raise SystemExit('runtime credential exists; refusing replacement')
    password = secrets.token_hex(32)
    runtime.write_text(password + '\n')
    os.chmod(runtime, 0o600)
    t.phase('copy-tls-cert', ['docker', 'cp', str(t.PRIVATE / 'certs/node.pem'), t.NAME + ':/var/lib/postgresql/data/server.crt'])
    t.phase('copy-tls-key', ['docker', 'cp', str(t.PRIVATE / 'certs/node-key.pem'), t.NAME + ':/var/lib/postgresql/data/server.key'])
    t.phase('tls-key-ownership', ['docker', 'exec', '--user', '0', t.NAME, 'chown', 'postgres:postgres', '/var/lib/postgresql/data/server.crt', '/var/lib/postgresql/data/server.key'])
    t.phase('tls-key-mode', ['docker', 'exec', '--user', '0', t.NAME, 'chmod', '600', '/var/lib/postgresql/data/server.key'])
    sql = ("ALTER SYSTEM SET ssl='on';\nALTER SYSTEM SET ssl_cert_file='server.crt';\n"
        "ALTER SYSTEM SET ssl_key_file='server.key';\nSELECT pg_reload_conf();\n"
        "CREATE ROLE oo_dl01w1_44_runtime LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION PASSWORD '" + password + "';\n"
        "GRANT CONNECT ON DATABASE oo_dl01w1_44_native_fresh TO oo_dl01w1_44_runtime;\n"
        "SELECT rolname,rolsuper,rolcreatedb,rolcreaterole,rolbypassrls,rolreplication FROM pg_roles WHERE rolname='oo_dl01w1_44_runtime';\n")
    t.phase('secure-bootstrap', ['docker', 'exec', '-i', t.NAME, 'psql', '-X', '-U', 'postgres', '-d', t.DB, '-v', 'ON_ERROR_STOP=1'], sql.encode())
    t.phase('tls-check', ['docker', 'exec', t.NAME, 'psql', '-X', '-U', 'postgres', '-d', t.DB, '-At', '-c', 'SHOW ssl;'])
    print('TLS_AND_RESTRICTED_RUNTIME_CREATED: no product migration or claim synthesized')


if __name__ == '__main__':
    main()
