"""Run only the F20 route test against a newly owned native PG17 cluster.

No containers or existing services. Identity is an explicit TLS protocol fixture.
S0-S5 facts/drafts are synthetic retained inputs; the actual S6-S10/compile paths,
ReviewLedger executable, PG transactions and native OOS/Pack compiler run.
"""
import argparse
from copy import deepcopy
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import secrets
import shutil
import socket
import ssl
import subprocess
import sys
import tempfile
import threading
import time

SCRIPTS = Path(__file__).resolve().parents[2] / 'pg-builder/scripts'
sys.path.insert(0, str(SCRIPTS))
import library_build
import library_control
import library_selfbuild as lib
from test_f20_read_grants import fixture, pin, ledger_review

CASES = ['allowed', 'missing', 'absent_policy', 'hidden', 'expired', 'wrong_role',
         'digest', 'generation_drift', 'required_assets']


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


class IdentityFixture(BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'

    def log_message(self, *args):
        pass

    def reply(self, status, value):
        body = json.dumps(value).encode()
        self.send_response(status)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        self.send_header('Connection', 'close')
        self.end_headers()
        self.wfile.write(body)
        self.close_connection = True

    def do_GET(self):
        cookie = self.headers.get('Cookie')
        if self.path != '/control/v1/identity/current' or cookie not in {
                '__Host-oo-session=' + 's' * 43, '__Host-oo-session=' + 'b' * 43}:
            return self.reply(401, {'error': {'code': 'unauthenticated'}})
        capabilities = ['business.record'] if cookie.endswith('b' * 43) else [
            'project.admin', 'review.decide', 'business.record']
        self.reply(200, {
            'contract': 'oo.control-identity/v1', 'operation': 'getIdentityCurrent',
            'subject': 'f20-reviewer', 'issuer': 'https://f20.invalid', 'config_commit': 'a' * 64,
            'revision': 1, 'install_state': 'initialized',
            'projects': [{'project_id': 'f20_' + c, 'role': 'admin', 'capabilities': capabilities}
                         for c in CASES],
            'csrf_token': 'c' * 43, 'session': {'idle_expires_at': '2099-01-01T00:00:00Z',
                                              'absolute_expires_at': '2099-01-01T00:00:00Z', 'revoked': False},
            'authority': 'management_session', 'session_class': 'management_cookie',
            'data_grants': 'none', 'owner_certification': 'not_issued',
            'agent_session': 'not_issued', 'harness_session': 'not_issued',
            'gateway_token': 'not_issued', 'customer_integration': 'not_claimed'})

    def do_POST(self):
        self.rfile.read(int(self.headers.get('Content-Length', '0')))
        if self.headers.get('X-OO-CSRF') != 'c' * 43:
            return self.reply(403, {'error': {'code': 'csrf.rejected'}})
        if self.headers.get('Cookie') not in {
                '__Host-oo-session=' + 's' * 43, '__Host-oo-session=' + 'b' * 43}:
            return self.reply(401, {'error': {'code': 'unauthenticated'}})
        self.reply(403, {'error': {'code': 'bootstrap.unknown'}})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', required=True)
    parser.add_argument('--ledger', required=True)
    parser.add_argument('--test-binary', required=True)
    parser.add_argument('--pg-bin', default='/opt/homebrew/bin')
    args = parser.parse_args()
    root = Path(args.output).resolve()
    root.mkdir(mode=0o700, parents=True, exist_ok=False)
    os.umask(0o077)
    log = (root / 'setup.log').open('w')

    def run(command, **kw):
        result = subprocess.run([str(c) for c in command], stdout=log, stderr=log, **kw)
        if result.returncode:
            raise RuntimeError(f'owned_setup_exit_{result.returncode}: {command[0]}')

    pg = Path(args.pg_bin)
    private = root / 'private'
    private.mkdir(mode=0o700)
    ca, cert, key = private / 'ca.pem', private / 'server.pem', private / 'server.key'
    run(['openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-days', '2',
         '-subj', '/CN=F20-owned-CA', '-keyout', private / 'ca.key', '-out', ca])
    run(['openssl', 'req', '-newkey', 'rsa:2048', '-nodes', '-subj', '/CN=localhost',
         '-keyout', key, '-out', private / 'server.csr'])
    ext = private / 'server.ext'
    ext.write_text('subjectAltName=IP:127.0.0.1,DNS:localhost\nbasicConstraints=CA:FALSE\n'
                   'keyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n')
    run(['openssl', 'x509', '-req', '-in', private / 'server.csr', '-CA', ca,
         '-CAkey', private / 'ca.key', '-CAcreateserial', '-out', cert, '-days', '2', '-extfile', ext])
    password = secrets.token_hex(24)
    pw = private / 'pg.password'
    pw.write_text(password)
    credential = private / 'reviewer.credential'
    credential.write_text(secrets.token_hex(32))
    ledger = root / 'fixed-review-ledger'
    shutil.copyfile(args.ledger, ledger)
    ledger.chmod(0o700)
    lib.write(root / 'binary-pins.json', {'ledger': sha(ledger),
              'test_binary': sha(args.test_binary), 'postgres': sha(pg / 'postgres')})
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        port = sock.getsockname()[1]
    data = root / 'pgdata'
    sockets = Path(tempfile.mkdtemp(prefix='oo-f20-pg-'))
    started = False
    server = None
    try:
        run([pg / 'initdb', '-D', data, '-U', 'f20_owner', '--auth-local=trust',
             '--auth-host=scram-sha-256', '--pwfile', pw, '--encoding=UTF8', '--no-locale'])
        with (data / 'postgresql.conf').open('a') as f:
            f.write(f"\nlisten_addresses='127.0.0.1'\nport={port}\nunix_socket_directories='{sockets}'\n"
                    f"ssl=on\nssl_cert_file='{cert}'\nssl_key_file='{key}'\nmax_connections=40\n")
        run([pg / 'pg_ctl', '-D', data, '-l', root / 'postgres.log', '-w', 'start'])
        started = True
        sql = (f"CREATE ROLE oo_f20_selfbuild_api_admin LOGIN PASSWORD '{password}' NOSUPERUSER;\n"
               f"CREATE ROLE f20_ledger LOGIN PASSWORD '{password}' NOSUPERUSER;\n"
               "CREATE ROLE oo_review_ledger_runtime NOLOGIN;\n")
        for name in CASES:
            sql += (f'CREATE DATABASE f20_api_{name} OWNER oo_f20_selfbuild_api_admin;\n'
                    f'CREATE DATABASE f20_ledger_{name} OWNER f20_ledger;\n')
        run([pg / 'psql', '-h', sockets, '-p', port, '-U', 'f20_owner', '-d', 'postgres',
             '-X', '-v', 'ON_ERROR_STOP=1'], input=sql, text=True)
        server = ThreadingHTTPServer(('127.0.0.1', 0), IdentityFixture)
        tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        tls.load_cert_chain(cert, key)
        server.socket = tls.wrap_socket(server.socket, server_side=True)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        origin = f'https://127.0.0.1:{server.server_port}'

        def endpoint(database, user):
            return {'host': '127.0.0.1', 'server_name': '127.0.0.1', 'port': port,
                    'database': database, 'user': user, 'password_file': str(pw), 'ca_file': str(ca)}

        for name in CASES:
            case = root / name
            facts, receipts, policy, config = fixture(case, 'f20_' + name)
            expected = ledger_review(policy)
            lib.write(case / 'expected-review.json', expected)
            if name == 'missing':
                policy['review']['columns']['app.items.phone'].pop('explicit_allow')
            elif name == 'allowed':
                for column in policy['review']['columns'].values():
                    if column['pii'] != 'none':
                        column['read_grant_expires_at'] = int(time.time()) + 30
            elif name == 'hidden':
                policy['review']['hidden_columns'].append('app.items.phone')
            elif name == 'expired':
                policy['review']['columns']['app.items.phone']['read_grant_expires_at'] = 1
            elif name == 'wrong_role':
                policy['review']['role'] = 'oo_r_other'
            pin(case, config, policy)
            if name == 'absent_policy':
                config.pop('column_review_input')
            elif name == 'required_assets':
                config['required_assets']['examples'] = ['unavailable-independent-evidence']
            if name == 'digest':
                config['column_review_input']['sha256'] = '0' * 64
            lib.write(case / 'config.json', config)
            roles = case / 'role-policy.json'
            lib.write(roles, {'role': 'oo_r_f20', 'principal': 'f20_reader'})
            work = case / 'work'
            work.mkdir()
            api = {
                'contract': 'oo.selfbuild-control-api.config/v1', 'deployment_origins': [origin],
                'budgets': {'role_pack': {'max_file_bytes': 4_194_304,
                                         'max_total_bytes': 67_108_864, 'max_entries': 16384}},
                'listen': {'bind': '127.0.0.1:1', 'cert_file': str(cert), 'key_file': str(key)},
                'identity': {'origin': origin, 'ca_file': str(ca)}, 'work_dir': str(work),
                'postgres': endpoint('f20_api_' + name, 'oo_f20_selfbuild_api_admin'),
                'ledger': endpoint('f20_ledger_' + name, 'f20_ledger'),
                'binaries': {k: str(ledger) if k == 'review_ledger' else '/usr/bin/false'
                             for k in ('catalog', 'pg_builder', 'rules', 'suggestions', 'review_ledger')},
                'projects': [{
                    'project_id': 'f20_' + name, 'config_commit': 'a' * 64, 'source_ref': 'f20',
                    'allowance_id': 'f20', 'llm_secret_ref': 'secret://f20/unused',
                    'source_secret_ref': 'secret://f20/unused',
                    'source': {'host': '127.0.0.1', 'port': 1, 'database': 'never_contacted',
                               'username': 'never_contacted', 'password_file': str(pw), 'ca_file': str(ca),
                               'tables': [{'schema': 'app', 'table': 'items'}]},
                    'sample_policy': 'none', 'rules_auto_accept': 'none',
                    'llm': {'bearer_file': '/nonexistent/f20', 'origin': 'http://127.0.0.1:8317',
                            'model_id': 'grok-4.7'},
                    'role_policy_file': str(roles), 'hidden_columns': ['app.items.hidden'],
                    'visible_columns': sorted(expected['columns']),
                    'library': {'python': str(Path(sys.executable).resolve()),
                                'script': str(SCRIPTS / 'library_control.py'),
                                'script_sha256': sha(SCRIPTS / 'library_control.py'),
                                'config': str(case / 'config.json'), 'config_sha256': sha(case / 'config.json'),
                                'manifest': str(case / 'manifest.json'), 'manifest_sha256': sha(case / 'manifest.json')}
                }],
                'principals': [{'subject': 'f20-reviewer', 'ledger_role': 'owner', 'principal_id': 'f20.reviewer',
                                'reviewer_label': 'f20-reviewer', 'credential_file': str(credential),
                                'object_scope': ['app.items', 'app.items.hidden'] + sorted(expected['columns'])}]
            }
            lib.write(case / 'api-config.json', api)
            if name == 'allowed':
                local = case / 'local'
                local.mkdir()
                for file in ('facts.json', 'drafts.json', 'merged.json'):
                    shutil.copyfile(case / file, local / file)
                lib.write(local / 'ledger-review.json', library_build.pinned_review(config, facts))
                library_control.run({'stage': 'compile', 'config': str(case / 'config.json'),
                                     'manifest': str(case / 'manifest.json'), 'work_dir': str(local)})
        lib.write(root / 'cases.json', CASES)
        env = dict(os.environ, OO_F20_FIXTURE_ROOT=str(root))
        with (root / 'route-test.log').open('w') as output:
            result = subprocess.run([args.test_binary, '--ignored', '--exact',
                                     'owned_pg_ledger_router_and_local_pack_parity', '--nocapture'],
                                    env=env, stdout=output, stderr=subprocess.STDOUT)
        lib.write(root / 'test-exit.json', {'exit': result.returncode, 'fixture': str(root),
                  'customer_http': False, 'real_owned_pg': True, 'real_review_ledger': True,
                  'identity': 'pinned_TLS_contract_fixture', 'source_or_model_calls': 0})
        print((root / 'route-test.log').read_text())
        return result.returncode
    finally:
        if server:
            server.shutdown()
            server.server_close()
        if started:
            run([pg / 'pg_ctl', '-D', data, '-m', 'fast', '-w', 'stop'])
        shutil.rmtree(sockets)
        log.close()


if __name__ == '__main__':
    raise SystemExit(main())
