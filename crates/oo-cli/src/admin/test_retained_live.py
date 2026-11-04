#!/usr/bin/env python3
"""Owned A3 fixture: real oo build, printed enrollment, native CLI and data-only cycle."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shlex
import signal
import ssl
import subprocess
import sys
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[4]
os.environ['OO_TEST_LANE'] = 'm'  # Import the shared helper, then give our resources our own label.
spec = importlib.util.spec_from_file_location('cycles', ROOT / 'tools/oo-load/test-gateway-cycles.py')
cycles = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cycles)
import t1
import runner
t1.LANE = 'a3'
sys.path.insert(0, str(ROOT / 'tools/release-coordinator/tests'))
import retained_fixture
BIN = ROOT / 'target/debug'

class Live(cycles.Live):
    def load(self, *args):
        pass  # Only oo build may ingest in this test.

    def publish(self, *args):
        pass  # Only oo build may review/publish in this test.

    def start_gateway(self):
        # Provision neutral H data with the fixture admin, before product execution.
        if self.engine == 'postgres':
            sql = 'CREATE SCHEMA fixture; SET search_path=fixture;\n' + ''.join(runner.generator.generate('postgres', '17', 120, 0))
            self.source_sql(sql)
            self.source_sql('GRANT USAGE ON SCHEMA fixture TO reader; GRANT SELECT ON ALL TABLES IN SCHEMA fixture TO reader;')
            self.source_version = self.db.sql('SHOW server_version').strip()
            assert self.source_version.split()[0] == '17.11'
            scope = ['fixture.customers', 'fixture.orders']
        else:
            self.source.args.scale, self.source.args.tables = 120, 0
            self.source.seed()
            if self.engine == 'mysql':
                self.source.sql("GRANT SELECT,SHOW VIEW ON fixture.* TO 'reader'@'%';", admin=True)
                scope = ['fixture.customers', 'fixture.orders']
            else:
                self.source.sql('GRANT SELECT,FLASHBACK ON fixture.orders TO reader;\n'
                                'GRANT SELECT,FLASHBACK ON fixture.customers TO reader;', admin=True)
                scope = ['FIXTURE.CUSTOMERS', 'FIXTURE.ORDERS']
        stack = json.loads(self.stack.read_text())
        stack['sources'][0]['scope']['include'] = scope
        model = {'provider': 'openai-compatible', 'base_url': {'secret_ref': 'model-endpoint-9r'},
                 'model': 'gpt-6.1-sol', 'key': {'secret_ref': 'model-key-9r-sol'}}
        stack['model'] = model
        cycles.private(self.stack, json.dumps(stack))
        cycles.private(self.root / 'model.json', json.dumps({'model': model}))
        target = json.loads(self.target.read_text())
        target.pop('local_fixture')
        target['tls_ca'] = {'secret_ref': 'source-ca'}
        cycles.private(self.target, json.dumps(target))
        self.db.sql("CREATE ROLE oo_r_owner NOLOGIN NOINHERIT; GRANT oo_r_owner TO oo_f22_login WITH INHERIT FALSE, SET TRUE;"
                    "GRANT USAGE ON SCHEMA _oo,oo_publish_target TO oo_f22_login;"
                    "GRANT SELECT ON ALL TABLES IN SCHEMA _oo,oo_publish_target TO oo_f22_login;"
                    "GRANT EXECUTE ON FUNCTION _oo.lookup_physical_mapping(text,text,bigint,text,text,text,text) TO oo_f22_login;"
                    "ALTER ROLE oo_f22_login SET default_transaction_read_only=on;"
                    "SELECT format('CREATE ROLE oo_token_admin LOGIN PASSWORD %L',pg_read_file('/tmp/pw')) \\gexec\n"
                    "GRANT CONNECT,CREATE ON DATABASE postgres TO oo_token_admin;"
                    "GRANT EXECUTE ON FUNCTION pg_catalog.pg_advisory_xact_lock(bigint),"
                    "pg_catalog.pg_advisory_xact_lock(integer,integer) TO oo_token_admin;")
        # Exact migrations and manifests, owned by the real lifecycle admin.
        for name, manifest in [('oo_token_control_v1.sql', 'INSERT INTO oo_token_control.install_manifest VALUES(true,\'%s\');'),
                               ('oo_token_control_v2.sql', 'INSERT INTO oo_token_lifecycle.migration VALUES(2,\'%s\');'),
                               ('oo_token_issue_retry_v1.sql', 'INSERT INTO oo_token_lifecycle.issue_retry_manifest VALUES(true,\'%s\');')]:
            raw = (ROOT / 'tools/gateway-token-store/migrations' / name).read_bytes()
            self.db.sql('SET ROLE oo_token_admin;\n' + raw.decode() + '\n' + manifest % hashlib.sha256(raw).hexdigest())
        for name in ['packs', 'wal', 'collector']:
            (self.runtime / name).mkdir(mode=0o700)
        self.fixture.write(self.runtime / 'endpoint.json', {'host': '127.0.0.1', 'port': self.db.port, 'database': 'postgres'})
        for src, dst in [('password', 'password'), ('source-ca', 'ca.crt'), ('server-key', 'tls.key'), ('server-cert', 'tls.crt')]:
            self.fixture.write(self.runtime / dst, (self.db.directory / src).read_bytes())
        self.command(['openssl', 'req', '-newkey', 'rsa:2048', '-nodes', '-subj', '/CN=lane-a3-client',
                      '-keyout', self.root / 'client.key', '-out', self.root / 'client.csr'], 'client-csr')
        (self.root / 'client.ext').write_text('basicConstraints=critical,CA:FALSE\nkeyUsage=digitalSignature\nextendedKeyUsage=clientAuth\nsubjectAltName=URI:urn:oo:workload:lane-a3\n')
        self.command(['openssl', 'x509', '-req', '-in', self.root / 'client.csr', '-CA', self.db.directory / 'source-ca',
                      '-CAkey', self.db.directory / 'ca-key', '-CAcreateserial', '-days', '1', '-extfile', self.root / 'client.ext',
                      '-out', self.root / 'client.crt'], 'client-sign')
        cycles.private(self.root / 'client.pem', (self.root / 'client.crt').read_text() + (self.root / 'client.key').read_text())
        self.origin = 'https://127.0.0.1:' + str(self.fixture.free_port())
        config = {'listen': self.origin.removeprefix('https://'), 'transport': 'tls', 'audience': self.origin,
                  'runtime_user': 'oo_token_runtime', 'password_file': 'password', 'ca_file': 'ca.crt', 'endpoint_file': 'endpoint.json',
                  'pack_dir': 'packs', 'body_limit': 1048576, 'request_timeout_ms': 30000, 'global_concurrency': 8,
                  'tls_key_file': 'tls.key', 'tls_chain_file': 'tls.crt', 'client_ca_file': 'ca.crt', 'handshake_timeout_ms': 10000,
                  'data_endpoint_file': 'endpoint.json', 'data_password_file': 'password', 'data_ca_file': 'ca.crt',
                  'data_user': 'oo_f22_login', 'wal_dir': 'wal', 'reviewed_dir': 'reviewed', 'audit_collector_dir': 'collector',
                  'token_admin_password_file': 'password', 'token_admin_workload': 'urn:oo:workload:lane-a3'}
        self.fixture.write(self.runtime / 'config.json', config)
        cycles.private(self.root / 'publisher.json', json.dumps({'host': '127.0.0.1', 'port': self.db.port, 'database': 'postgres',
            'user': 'postgres', 'password_file': str(self.db.directory / 'password'), 'ca_file': str(self.db.directory / 'source-ca')}))
        self.runtime_config = self.root / 'build-runtime.json'
        cycles.private(self.runtime_config, json.dumps({'protocol': 'oo.build-runtime/v1', 'source_id': 'owned', 'library_id': 'owned',
            'secrets_dir': str(self.db.directory), 'builder_script': str(ROOT / 'tools/pg-builder/scripts/retained_build.py'),
            'builder': {'project': 'owned', 'psql_command': ['docker', 'exec', '-i', self.db.name, 'psql', '-U', 'postgres'],
                        'model': {'stack': str(self.root / 'model.json'), 'egress': str(ROOT / 'tools/model-egress/model_egress.py'), 'call_limit': 6}},
            'publisher_config': str(self.root / 'publisher.json'), 'gateway_login': 'oo_f22_login',
            'gateway': {'runtime_dir': str(self.runtime), 'gateway_url': self.origin, 'ca_file': str(self.runtime / 'ca.crt'),
                        'client_pem': str(self.root / 'client.pem'), 'first_user_grant': {'project_id': 'owned', 'principal_id': 'owner',
                            'role': 'oo_r_owner', 'claims': {}, 'policy_version': 'draft-v1', 'knowledge_version': 'draft-v1',
                            'audience_url': self.origin, 'ttl_ms': 86400000, 'purpose': 'build'}}}))

    def build(self, extra, name):
        result = subprocess.run([str(BIN / 'oo'), 'build', '--config', str(self.stack), '--agent-target', str(self.target),
                                 '--runtime-config', str(self.runtime_config), *extra], capture_output=True, timeout=2000)
        # Output includes one-use credentials. Keep it private, not in test logs.
        (self.root / (name + '.stdout')).write_bytes(result.stdout)
        (self.out / (name + '.stderr')).write_bytes(result.stderr)
        state_path = self.root / '.oo' / ('cycles/cycle-1' if '--cycle' in extra else '') / 'chain.json'
        state = json.loads(state_path.read_text()) if state_path.exists() else {}
        cycles.write_json(self.out / (name + '-state.json'), state)
        return result, state

    def exercise(self):
        first, state = self.build([], 'initial')
        assert first.returncode == 1 and state['stages'][3]['status'] == 'blocked', ('expected explicit owner review', state)
        self.db.sql('GRANT EXECUTE ON FUNCTION _oo.native_library_retained_table(text) TO oo_f22_login;')
        source = self.root / '.oo/review-input.json'
        decisions = self.root / 'decisions.json'
        cycles.private(decisions, json.dumps(retained_fixture.accept_all(json.loads(source.read_text()))))
        second, state = self.build(['--resume', '--review-file', str(decisions)], 'reviewed')
        assert second.returncode == 0 and state['status'] == 'complete', ('build failed', state)
        prompt = second.stdout.decode()
        bind = next(line.split('2. Bind: ', 1)[1] for line in prompt.splitlines() if line.startswith('2. Bind: '))
        prefix, command = bind.split(' | ', 1)
        code = shlex.split(prefix)[2]
        login = shlex.split(command)
        assert login[:2] == ['oo', 'login'] and '--code-stdin' in login
        consumer = self.root / 'consumer'
        consumer.mkdir(mode=0o700)
        env = dict(os.environ, OO_CONFIG_DIR=str(consumer))
        result = subprocess.run([str(BIN / 'oo'), *login[1:]], input=code.encode(), capture_output=True, env=env)
        (self.out / 'login.stderr').write_bytes(result.stderr)
        login_ok = result.returncode == 0
        cycles.write_json(self.out / 'login-result.json', {'exit': result.returncode, 'passed': login_ok})
        for name, args in ([('whoami', ['whoami']), ('tables', ['tables']), ('query', ['query', self.query_sql()])] if login_ok else []):
            result = subprocess.run([str(BIN / 'oo'), *args], capture_output=True, env=env)
            (self.out / (name + '.stdout')).write_bytes(result.stdout)
            (self.out / (name + '.stderr')).write_bytes(result.stderr)
            assert result.returncode == 0, name
        result = {'engine': self.engine, 'version': self.source_version, 'neutral_fixture_tables': 2,
                  'build': 'complete', 'printed_login': 'passed' if login_ok else 'blocked', 'whoami': 'passed' if login_ok else 'not_run', 'query': 'passed' if login_ok else 'not_run',
                  'model_calls': state['model_calls']}
        # A direct mTLS query isolates publication/data correctness from consumer
        # CLI negotiation failures. It never counts as native CLI acceptance.
        result['explicit_native_query'] = self.native_query('explicit-query')
        result['direct_mtls_query'] = self.direct_query('initial-query')
        replay, replay_state = self.build(['--resume'], 'replay')
        assert replay.returncode == 0 and replay.stdout == second.stdout
        assert replay_state == state, 'completed resume changed checkpoints'
        result['completed_resume'] = 'passed'
        if self.engine == 'postgres':
            self.source_sql('UPDATE fixture.orders SET amount=1001.1250 WHERE id=1;')
            cycle, updated = self.build(['--resume', '--cycle', 'cycle-1'], 'cycle')
            assert cycle.returncode == 0 and updated['model_calls'] == 0, updated
            output = self.root / '.oo/cycles/cycle-1/builder'
            assert not (output / 'usage.jsonl').exists()
            assert not (output / 'model-call-count.json').exists()
            assert (output.parent / 'c5-snapshot-completion.json').exists(), 'unchanged cut not completed'
            query = self.direct_query('cycle-query')
            served = (query['status'] == 200 and query['body'].get('rows') == [['1001.1250']]
                      and query.get('publication') != result['direct_mtls_query'].get('publication'))
            result['data_only'] = {'model_calls': 0, 'unchanged_cut_snapshot_completion': True,
                                   'new_data_served': served,
                                   'direct_mtls_query': query,
                                   'explicit_native_query': self.native_query('cycle-native-query')}
        cycles.write_json(self.out / 'acceptance.json', result)
        assert result['direct_mtls_query']['status'] == 200, 'baseline Gateway query failed'
        assert self.engine != 'postgres' or result['data_only']['new_data_served'], 'new data not served'
        assert login_ok, 'consumer CLI login verification blocked; retained cycle evidence recorded separately'
        assert result['explicit_native_query']['passed'], 'native CLI query failed'

    def native_query(self, name):
        env = dict(os.environ, OO_CONFIG_DIR=str(self.root / 'unbound-consumer'))
        query = subprocess.run([str(BIN / 'oo'), 'query', '--json', '--auth', 'external',
            '--transport', 'production', '--gateway', self.origin,
            '--ca-file', str(self.runtime / 'ca.crt'), '--client-pem', str(self.root / 'client.pem'),
            '--token-file', str(self.root / '.oo/first-user.token'), self.query_sql()],
            capture_output=True, env=env, timeout=40)
        (self.out / (name + '.stdout')).write_bytes(query.stdout)
        (self.out / (name + '.stderr')).write_bytes(query.stderr)
        return {'exit': query.returncode, 'passed': query.returncode == 0}

    def direct_query(self, name):
        context = ssl.create_default_context(cafile=str(self.runtime / 'ca.crt'))
        context.load_cert_chain(self.root / 'client.pem')
        request = urllib.request.Request(self.origin + '/v1/query',
            data=json.dumps({'sql': self.query_sql(), 'format': 'json'}).encode(),
            headers={'Authorization': 'Bearer ' + (self.root / '.oo/first-user.token').read_text(),
                     'Content-Type': 'application/json', 'x-oo-expected-publication-id': 'current'})
        try:
            with urllib.request.urlopen(request, context=context, timeout=30) as response:
                value = {'status': response.status, 'body': json.loads(response.read()),
                         'publication': response.headers.get('x-oo-publication-id')}
        except urllib.error.HTTPError as error:
            value = {'status': error.code, 'body': json.loads(error.read())}
        cycles.write_json(self.out / (name + '.json'), value)
        if value['status'] == 200:
            assert len(value['body']['rows']) == 1, value
        return value

    def query_sql(self):
        if self.engine == 'oracle':
            return 'SELECT q_414d4f554e54 FROM q_46495854555245.q_4f5244455253 WHERE q_4944 = \'1\''
        return 'SELECT amount FROM fixture.orders WHERE id=1'

    def close(self):
        # oo build owns its detached Gateway; kill only an exact argv match to our private directory.
        rows = subprocess.check_output(['ps', '-axo', 'pid=,command='], text=True)
        for line in rows.splitlines():
            match = re.match(r'\s*(\d+)\s+(.*)', line)
            if match and match[2] == str(BIN / 'oo-gateway') + ' ' + str(self.runtime):
                os.kill(int(match[1]), signal.SIGTERM)
        # Preserve private checkpoints for diagnosis, excluding generated credentials from hashes.
        archive = self.out / 'private-artifacts'
        if self.root.exists():
            import shutil
            shutil.copytree(self.root, archive)
        super().close()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--run-id', required=True)
    parser.add_argument('--engine', choices=['postgres', 'mysql', 'oracle'], default='postgres')
    args = parser.parse_args()
    assert re.fullmatch('[a-z0-9-]+', args.run_id)
    os.umask(0o077)
    out = ROOT / 'evidence' / args.run_id
    out.mkdir()
    live = Live(out, args.engine)
    status = 'failed'
    try:
        live.start()
        live.exercise()
        status = 'passed'
    finally:
        live.close()
        remaining = subprocess.check_output(
            ['docker', 'ps', '-aq', '--filter', 'label=oo.lane=a3'], text=True).split()
        cycles.write_json(out / 'result.json', {'status': status, 'containers_left': len(remaining)})
        assert not remaining, 'A3 containers remain'

if __name__ == '__main__':
    main()
