#!/usr/bin/env python3
"""Owned PostgreSQL 17.11 source cycles through the actual mTLS Gateway."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import ssl
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tools/oo-test'))
from t1 import AgentDB, LaneRun, private
from runner import checksums, write_json


def load_fixture():
    path = ROOT / 'tools/source-onboarding/tests/refresh_live_catalog_owned_pg.py'
    spec = importlib.util.spec_from_file_location('pack_fixture', path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    module.TABLES = ['items']
    return module


class Live:
    def __init__(self, out):
        self.out = out
        self.run = LaneRun(out, 'postgres', '17', argparse.Namespace())
        self.db = AgentDB(self.run)
        self.root = self.run.private
        self.runtime = self.root / 'runtime'
        self.runtime.mkdir()
        self.gateway = None
        self.reads = []
        self.stop = threading.Event()
        self.worker = None
        self.client_lock = threading.Lock()
        self.cuts = []
        self.baselines = {}
        self.fixture = load_fixture()

    def command(self, argv, label, data=None):
        result = subprocess.run([str(a) for a in argv], input=data, capture_output=True,
                                env=dict(os.environ, CARGO_TARGET_DIR=str(ROOT / 'target')))
        (self.out / (label + '.stdout')).write_bytes(result.stdout)
        (self.out / (label + '.stderr')).write_bytes(result.stderr)
        assert result.returncode == 0, label
        return result

    def start(self):
        self.db.start()
        pki = self.db.directory
        self.command(['openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-days', '1',
                      '-subj', '/CN=lane-m-ca', '-keyout', pki / 'ca-key', '-out', pki / 'source-ca',
                      '-addext', 'basicConstraints=critical,CA:TRUE',
                      '-addext', 'keyUsage=critical,keyCertSign,cRLSign'], 'ca')
        self.command(['openssl', 'req', '-newkey', 'rsa:2048', '-nodes', '-subj', '/CN=localhost',
                      '-keyout', pki / 'server-key', '-out', pki / 'server-csr'], 'server-csr')
        (pki / 'server-ext').write_text('basicConstraints=critical,CA:FALSE\n'
            'keyUsage=critical,digitalSignature,keyEncipherment\nsubjectKeyIdentifier=hash\n'
            'authorityKeyIdentifier=keyid,issuer\nextendedKeyUsage=serverAuth\n'
            'subjectAltName=DNS:localhost,IP:127.0.0.1\n')
        self.command(['openssl', 'x509', '-req', '-in', pki / 'server-csr', '-CA', pki / 'source-ca',
                      '-CAkey', pki / 'ca-key', '-CAcreateserial', '-days', '1', '-extfile', pki / 'server-ext',
                      '-out', pki / 'server-cert'], 'server-sign')
        for name in ['server-key', 'server-cert', 'source-ca']:
            self.run.command(['docker', 'cp', str(self.db.directory / name), self.db.name + ':/tmp/' + name])
        self.run.command(['docker', 'exec', '-u', 'root', self.db.name, 'sh', '-c',
                          'chown postgres:postgres /tmp/server-* /tmp/source-ca; chmod 600 /tmp/server-key'])
        self.db.sql("ALTER SYSTEM SET ssl='on'; ALTER SYSTEM SET ssl_cert_file='/tmp/server-cert'; "
                    "ALTER SYSTEM SET ssl_key_file='/tmp/server-key';")
        self.db.sql('SELECT pg_reload_conf();')
        self.run.command(['docker', 'exec', self.db.name, 'sh', '-c',
                          'export PGPASSWORD="$(cat /tmp/pw)"; '
                          'psql "host=127.0.0.1 user=postgres sslmode=verify-full sslrootcert=/tmp/source-ca" -c "SELECT 1"'])
        # Dedicated fixture admin creates data; the Loader source role is read-only.
        self.db.sql(r"""SELECT format('CREATE ROLE reader LOGIN PASSWORD %L',pg_read_file('/tmp/pw')) \gexec
SELECT format('CREATE ROLE oo_token_runtime LOGIN NOINHERIT PASSWORD %L',pg_read_file('/tmp/pw')) \gexec
SELECT format('CREATE ROLE oo_f22_login LOGIN NOINHERIT PASSWORD %L',pg_read_file('/tmp/pw')) \gexec
CREATE ROLE oo_r_f22 NOLOGIN NOINHERIT;
GRANT oo_r_f22 TO oo_f22_login WITH INHERIT FALSE, SET TRUE;
CREATE DATABASE source;
GRANT pg_read_all_stats TO reader;
""")
        self.source_sql("""
CREATE SCHEMA app; CREATE TABLE app.items(amount bigint PRIMARY KEY);
INSERT INTO app.items VALUES(10);
GRANT USAGE ON SCHEMA app TO reader; GRANT SELECT ON app.items TO reader;

""")
        config = self.root / 'install.json'
        config.write_text(json.dumps({'port': self.db.port, 'password_file': str(self.db.directory / 'password')}))
        env = dict(os.environ, OO_LOAD_TEST_TARGET=str(config))
        with (self.out / 'install.log').open('w') as log:
            subprocess.run(['cargo', 'test', '-p', 'oo-pipeline', '--locked', '--test',
                            'native_library_publication', 'retained_gateway_install', '--', '--ignored', '--exact'],
                           env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
        self.db.sql((ROOT / 'tools/release-coordinator/migrations/oo_publish_target_v1.sql').read_text())
        self.db.sql('GRANT CONNECT ON DATABASE postgres TO reader,oo_token_runtime,oo_f22_login;')
        self.db.sql('GRANT USAGE ON SCHEMA oo_publish_target TO oo_target_owner; GRANT SELECT ON oo_publish_target.fence TO oo_target_owner;')
        self.stack = self.root / 'stack.json'
        self.stack.write_text(json.dumps({'version': 'oo.stack/v1', 'sources': [{
            'id': 'owned', 'engine': 'postgres', 'host': '127.0.0.1', 'port': self.db.port,
            'database': 'source', 'user': 'reader', 'password': {'secret_ref': 'password'},
            'tls_ca': {'secret_ref': 'source-ca'}, 'scope': {'include': ['app.items']}}],
            'model': {'provider': 'openai-compatible', 'base_url': 'http://127.0.0.1:1',
                      'model': 'unused', 'key': {'secret_ref': 'absent-model-key'}}}))
        self.target = self.root / 'target.json'
        self.target.write_text(json.dumps({'host': '127.0.0.1', 'port': self.db.port,
            'database': 'postgres', 'user': 'postgres', 'password': {'secret_ref': 'password'},
            'local_fixture': True, 'run_id': 'baseline'}))
        self.args = argparse.Namespace(builder=str(ROOT / 'target/debug/oo-builder'))
        self.fixture.Fixture.pack(self)
        self.load([], 'baseline')
        self.publish('baseline')
        self.start_gateway()

    def source_sql(self, sql):
        return self.run.command(['docker', 'exec', '-i', self.db.name, 'psql', '-X', '-U', 'postgres',
                                 '-d', 'source', '-v', 'ON_ERROR_STOP=1', '-A', '-t'], data=sql)[1]

    def load(self, prefix, run):
        return self.command([ROOT / 'target/debug/oo-load', *prefix, '--stack', self.stack,
            '--source', 'owned', '--target', self.target, '--secrets-dir', self.db.directory,
            '--run-id', run], 'load-' + run)

    def publish(self, run):
        argv = [ROOT / 'target/debug/oo-load', 'publish', '--stack', self.stack,
                '--source', 'owned', '--target', self.target, '--secrets-dir', self.db.directory,
                '--generation', run, '--library', 'owned']
        candidate = subprocess.run([str(a) for a in argv], capture_output=True)
        (self.out / ('prepare-' + run + '.stderr')).write_bytes(candidate.stderr)
        assert candidate.returncode != 0
        cut = json.loads(candidate.stdout)
        assert cut['state'] == 'pending'
        self.cuts.append(cut)
        entry = cut['data_vector'][0]
        physical = entry['physical_name']
        assert self.db.sql(f"SELECT _oo.native_library_retained_table('{physical}') IS NULL").strip() == 't'
        column = self.db.sql("SELECT physical_name FROM _oo.physical_mappings WHERE source='owned' "
                             f"AND run_generation={cut['generation']} AND level='c'").strip()
        # Same reviewed-column grant and closed -> granted -> open fence sequence as publisher.
        self.db.sql(f"""BEGIN;
INSERT INTO oo_publish_target.fence(singleton,state,role_name,schema_name,relation_name,opened_columns,streams_stopped)
 VALUES(true,'closed','oo_r_f22','_oo','{physical}','{{}}',0);
GRANT USAGE ON SCHEMA _oo TO oo_r_f22,oo_f22_login;
GRANT SELECT (\"{column}\") ON _oo.\"{physical}\" TO oo_r_f22;
UPDATE oo_publish_target.fence SET state='granted' WHERE relation_name='{physical}';
UPDATE oo_publish_target.fence SET state='open',opened_columns=ARRAY['{column}'],
 opened_pack_digest='{self.artifact['pack_digest']}',opened_version='review-{run}',candidate_id='self-{run}'
 WHERE relation_name='{physical}'; COMMIT;""")
        result = json.loads(self.command(argv, 'publish-' + run).stdout)
        assert result['state'] == 'ready'
        if self.worker:
            # Exercise the real DB-head/reviewed-file handoff window, rather
            # than relying on a conveniently timed client to miss it.
            time.sleep(.25)
        publication = {'protocol': 'oo.pipeline.native-library-publication/v1', 'run_id': run,
                       'receipt_sha256': cut['receipt_sha256'], 'receipt': cut['receipt']}
        catalog = self.base_catalog | {'publication': publication, 'bindings': [{
            'dataset': 'app.items', 'source': 'owned', 'config_digest': cut['config_digest'],
            'generation': cut['generation'], 'type_digest': entry['recipe_sha256'],
            'input_digest': entry['recipe_sha256']}]}
        path = self.runtime / 'reviewed/library/library.json'
        path.parent.mkdir(parents=True, exist_ok=True)
        self.fixture.replace(path, self.fixture.encode(catalog))
        write_json(self.out / ('cut-' + run + '.json'), cut)
        return result

    def start_gateway(self):
        token_sql = ROOT / 'tools/gateway-token-store/migrations/oo_token_control_v1.sql'
        self.db.sql(token_sql.read_text())
        self.db.sql("INSERT INTO oo_token_control.install_manifest VALUES(true,'" + self.fixture.sha(token_sql.read_bytes()) + "');")
        self.db.sql('GRANT USAGE ON SCHEMA _oo,oo_publish_target TO oo_f22_login; '
                    'GRANT SELECT ON ALL TABLES IN SCHEMA _oo,oo_publish_target TO oo_f22_login; '
                    'GRANT EXECUTE ON FUNCTION _oo.lookup_physical_mapping(text,text,bigint,text,text,text,text) TO oo_f22_login; '
                    'GRANT EXECUTE ON FUNCTION _oo.native_library_retained_table(text) TO oo_f22_login; '
                    'ALTER ROLE oo_f22_login SET default_transaction_read_only=on;')
        for name in ['packs', 'wal', 'collector']:
            (self.runtime / name).mkdir()
        self.fixture.write(self.runtime / 'packs/library.json', self.artifact)
        endpoint = {'host': '127.0.0.1', 'port': self.db.port, 'database': 'postgres'}
        self.fixture.write(self.runtime / 'endpoint.json', endpoint)
        for src, dst in [('password', 'password'), ('source-ca', 'ca.crt'),
                         ('server-key', 'tls.key'), ('server-cert', 'tls.crt')]:
            self.fixture.write(self.runtime / dst, (self.db.directory / src).read_bytes())
        # mTLS client certificate uses the fixture CA, as in lane A's boundary harness.
        self.command(['openssl', 'req', '-newkey', 'rsa:2048', '-nodes', '-subj', '/CN=lane-m-client',
                      '-keyout', self.root / 'client.key', '-out', self.root / 'client.csr'], 'client-csr')
        (self.root / 'client.ext').write_text('basicConstraints=critical,CA:FALSE\nkeyUsage=digitalSignature\nextendedKeyUsage=clientAuth\nsubjectAltName=URI:urn:oo:workload:lane-m\n')
        self.command(['openssl', 'x509', '-req', '-in', self.root / 'client.csr', '-CA', self.db.directory / 'source-ca',
                      '-CAkey', self.db.directory / 'ca-key', '-CAcreateserial', '-days', '1', '-extfile', self.root / 'client.ext',
                      '-out', self.root / 'client.crt'], 'client-sign')
        self.origin = 'https://127.0.0.1:' + str(self.fixture.free_port())
        config = {'listen': self.origin.removeprefix('https://'), 'transport': 'tls',
                  'audience': self.origin + '/v1', 'runtime_user': 'oo_token_runtime',
                  'password_file': 'password', 'ca_file': 'ca.crt', 'endpoint_file': 'endpoint.json',
                  'pack_dir': 'packs', 'body_limit': 1048576, 'request_timeout_ms': 30000,
                  'global_concurrency': 8, 'tls_key_file': 'tls.key', 'tls_chain_file': 'tls.crt',
                  'client_ca_file': 'ca.crt', 'handshake_timeout_ms': 10000,
                  'data_endpoint_file': 'endpoint.json', 'data_password_file': 'password',
                  'data_ca_file': 'ca.crt', 'data_user': 'oo_f22_login', 'wal_dir': 'wal',
                  'reviewed_dir': 'reviewed', 'audit_collector_dir': 'collector'}
        self.fixture.write(self.runtime / 'config.json', config)
        import base64, secrets
        self.token = 'ooct_' + base64.urlsafe_b64encode(secrets.token_bytes(32)).decode().rstrip('=')
        private(self.root / 'client-token', self.token)
        now = int(time.time() * 1000)
        self.db.sql(f"INSERT INTO oo_token_control.token_record VALUES('{self.fixture.sha(self.token.encode())}',"
                    f"'client','f22','alice','oo_r_f22','{{}}','pv1','kv1','{self.origin}/v1',"
                    f"NULL,NULL,{now},NULL,{now+86400000},{now},0);")
        self.context = ssl.create_default_context(cafile=str(self.runtime / 'ca.crt'))
        self.context.load_cert_chain(self.root / 'client.crt', self.root / 'client.key')
        self.log = (self.out / 'gateway.log').open('wb')
        self.gateway = subprocess.Popen([str(ROOT / 'target/debug/oo-gateway'), str(self.runtime)],
                                        stdout=self.log, stderr=self.log)
        self.fixture.wait(lambda: (self.runtime / 'listen.addr').exists() or self.gateway.poll() is not None, 'gateway startup')
        assert self.gateway.poll() is None, 'gateway startup failed'
        unauthenticated = urllib.request.Request(self.origin + '/v1/freshness',
            headers={'Authorization': 'Bearer ' + self.token})
        without_cert = ssl.create_default_context(cafile=str(self.runtime / 'ca.crt'))
        try:
            urllib.request.urlopen(unauthenticated, context=without_cert, timeout=10)
        except urllib.error.HTTPError:
            raise AssertionError('certificate-less request reached HTTP')
        except (urllib.error.URLError, ssl.SSLError, ConnectionError):
            write_json(self.out / 'mtls.json', {'missing_client_certificate': 'refused'})
        else:
            raise AssertionError('Gateway accepted a client without mTLS')

    def http(self, route='/v1/query'):
        # One authenticated client; same-role parallel queries are deliberately
        # rejected by Gateway's existing admission limit.
        with self.client_lock:
            return self.http_request(route)

    def http_request(self, route):
        data = self.fixture.encode({'sql': 'SELECT amount FROM app.items', 'format': 'json'}) if route.endswith('query') else None
        request = urllib.request.Request(self.origin + route, data=data, headers={
            'Authorization': 'Bearer ' + self.token, 'Content-Type': 'application/json',
            'x-oo-expected-publication-id': 'current'})
        try:
            with urllib.request.urlopen(request, context=self.context, timeout=30) as response:
                return {'status': response.status, 'publication': response.headers.get('x-oo-publication-id'),
                        'vector': response.headers.get('x-oo-data-vector-sha256'), 'body': json.loads(response.read())}
        except urllib.error.HTTPError as error:
            return {'status': error.code, 'body': json.loads(error.read())}
        except Exception as error:
            return {'status': 0, 'error': type(error).__name__}

    def cycles(self):
        negative = {}
        physical = self.cuts[0]['data_vector'][0]['physical_name']
        original = self.db.sql(f"SELECT pg_get_viewdef('_oo.\"{physical}\"'::regclass,false)").strip()
        column = self.db.sql("SELECT physical_name FROM _oo.physical_mappings WHERE source='owned' "
                             f"AND run_generation={self.cuts[0]['generation']} AND level='c'").strip()
        self.db.sql(f'CREATE OR REPLACE VIEW _oo."{physical}" AS SELECT 999::bigint AS "{column}";')
        negative['changed_view'] = self.http()
        self.db.sql(f'CREATE OR REPLACE VIEW _oo."{physical}" AS {original}')
        self.db.sql(f"UPDATE oo_publish_target.fence SET state='closed',opened_pack_digest=NULL,opened_version=NULL WHERE relation_name='{physical}'")
        negative['closed_fence'] = self.http()
        self.db.sql(f"UPDATE oo_publish_target.fence SET state='open',opened_pack_digest='{self.artifact['pack_digest']}',opened_version='review-baseline' WHERE relation_name='{physical}'")
        write_json(self.out / 'negative-queries.json', negative)
        assert all(r['status'] == 409 for r in negative.values())
        initial = self.http()
        assert initial['status'] == 200 and initial['body']['rows'] == [['10']], initial
        self.baselines['10'] = (initial['publication'], initial['vector'])
        def read():
            while not self.stop.is_set():
                self.reads.append(self.http())
                time.sleep(.01)
        self.reads.append(initial)
        self.worker = threading.Thread(target=read)
        self.worker.start()
        freshness = [self.http('/v1/freshness')]
        for cycle in range(1, 4):
            self.source_sql(f'UPDATE app.items SET amount={10+cycle};')
            policy = self.root / 'c5.json'
            policy.write_text(json.dumps({'cycle': cycle, 'full_read_every': 1}))
            self.load(['sync', '--c5', policy], 'cycle-' + str(cycle))
            self.publish('cycle-' + str(cycle))
            query = self.http()
            self.reads.append(query)
            assert query['status'] == 200 and query['body']['rows'] == [[str(10 + cycle)]], query
            self.baselines[str(10 + cycle)] = (query['publication'], query['vector'])
            freshness.append(self.http('/v1/freshness'))
        self.stop.set()
        self.worker.join()
        write_json(self.out / 'queries.json', self.reads)
        write_json(self.out / 'freshness.json', freshness)
        errors = [r for r in self.reads if r['status'] != 200]
        write_json(self.out / 'live-result.json', {'cycles': 3, 'queries': len(self.reads), 'errors': len(errors),
                   'freshness_statuses': [r['status'] for r in freshness]})
        assert not errors, 'continuous authenticated queries failed; see queries.json'
        assert all(r['status'] == 200 for r in freshness)
        assert len(set(self.baselines.values())) == 4
        assert len({p for p, _ in self.baselines.values()}) == 4
        assert len({v for _, v in self.baselines.values()}) == 4
        for query in self.reads:
            rows = query['body']['rows']
            assert len(rows) == 1 and len(rows[0]) == 1
            assert (query['publication'], query['vector']) == self.baselines[rows[0][0]]
        times = []
        for response, cut in zip(freshness, self.cuts):
            body = response['body']
            assert body['source_time'] == 'unmeasured'
            assert body['publication'] == 'live'
            assert body['library_publications'][0]['receipt_sha256'] == cut['receipt_sha256']
            observation = body['target_observations'][0]
            assert observation['source_generation'] == cut['generation']
            assert observation['source_committed_at'] is None
            times.append(observation['target_applied_at'])
        assert all(a < b for a, b in zip(times, times[1:]))
        assert self.db.sql('SELECT count(*) FROM _oo.full_import_run').strip() == '0'
        assert self.db.sql('SELECT count(*) FROM _oo.apply_receipts').strip() == '0'
        replay = subprocess.run([str(ROOT / 'target/debug/oo-load'), 'publish', '--stack', str(self.stack),
            '--source', 'owned', '--target', str(self.target), '--secrets-dir', str(self.db.directory),
            '--generation', 'baseline', '--library', 'owned'], capture_output=True)
        (self.out / 'superseded-replay.stdout').write_bytes(replay.stdout)
        (self.out / 'superseded-replay.stderr').write_bytes(replay.stderr)
        assert replay.returncode != 0
        assert json.loads(replay.stderr)['error'] == 'publication_superseded'
        assert self.db.sql("SELECT receipt_sha256 FROM _oo.native_library_head WHERE library_id='owned'").strip() == self.cuts[-1]['receipt_sha256']
        write_json(self.out / 'live-result.json', {'cycles': 3, 'queries': len(self.reads), 'errors': 0,
            'header_vector_rows_consistent': True, 'freshness_advancing': True,
            'source_time': 'unmeasured', 'legacy_imports': 0, 'legacy_acks': 0,
            'negative_cases': list(negative) + ['superseded_replay'], 'handoff_delay_ms': 250,
            'engine': self.db.sql('SHOW server_version').strip()})

    def close(self):
        self.stop.set()
        if self.worker:
            self.worker.join(timeout=35)
        write_json(self.out / 'queries.json', self.reads)
        if self.gateway:
            self.gateway.terminate()
            try:
                self.gateway.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.gateway.kill()
                self.gateway.wait()
            self.log.close()
        if self.db.created:
            self.run.command(['docker', 'logs', self.db.name], required=False)
        self.db.close()
        self.run.cleanup()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--run-id', required=True)
    args = parser.parse_args()
    assert args.run_id and all(c.isalnum() or c == '-' for c in args.run_id)
    os.umask(0o077)
    out = ROOT / 'evidence' / args.run_id
    out.mkdir(exist_ok=False)
    live = Live(out)
    status = 'failed'
    try:
        live.start()
        live.cycles()
        status = 'passed'
    finally:
        live.close()
        write_json(out / 'result.json', {'status': status, 'containers_left': 0})
        checksums(out)


if __name__ == '__main__':
    main()
