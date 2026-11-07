#!/usr/bin/env python3
"""Serial lane-A product doctor checks using oo-test's pinned MySQL fixtures."""
import argparse
import json
import os
from pathlib import Path
import secrets
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(ROOT / 'tools/oo-test'))
sys.path.insert(0, str(ROOT / 'tools/model-egress/tests'))
import runner
from runner import Run, checksums

# Card-requested patches predate the shared fixture lock; keep the override local.
shared_resolve = runner.resolve
card_pins = json.loads((ROOT / "deploy/lite/mysql-doctor/images.json").read_text())
runner.resolve = lambda engine, version: card_pins[version] if engine == "mysql" and version in card_pins else shared_resolve(engine, version)
from test_oracle_doctor import Fixture as OracleFixture
from test_egress import Replay

class Fixture(Run):
    cleanup = OracleFixture.cleanup

    def command(self, argv, **kwargs):
        argv = ['oo.lane=a' if s == 'oo.lane=h' else s for s in argv]
        if argv[:2] == ['docker', 'create']:
            argv[2:2] = ['-p', '127.0.0.1::3306']
        return super().command(argv, **kwargs)

    def tls(self):
        p = self.private
        self.command(['openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-nodes',
                      '-keyout', str(p/'ca-key.pem'), '-out', str(p/'ca.pem'),
                      '-days', '2', '-subj', '/CN=OO owned fixture CA'])
        self.command(['openssl', 'req', '-newkey', 'rsa:2048', '-nodes',
                      '-keyout', str(p/'server-key.pem'), '-out', str(p/'server.csr'),
                      '-subj', '/CN=localhost'])
        (p/'ext').write_text('basicConstraints=critical,CA:FALSE\nsubjectAltName=DNS:localhost,IP:127.0.0.1\nextendedKeyUsage=serverAuth\n')
        self.command(['openssl', 'x509', '-req', '-in', str(p/'server.csr'),
                      '-CA', str(p/'ca.pem'), '-CAkey', str(p/'ca-key.pem'), '-CAcreateserial',
                      '-out', str(p/'server-cert.pem'), '-days', '2', '-extfile', str(p/'ext')])
        self.command(['docker', 'exec', self.name, 'sh', '-c',
                      'cp /run/oo/ca.pem /run/oo/server-cert.pem /run/oo/server-key.pem /var/lib/mysql/; '
                      'chown mysql:mysql /var/lib/mysql/*pem; chmod 600 /var/lib/mysql/*key.pem'])
        self.command(['docker', 'restart', self.name])
        for _ in range(120):
            code, _ = self.sql('SELECT 1;', admin=True, required=False)
            if code == 0:
                return
            time.sleep(1)
        raise RuntimeError('TLS fixture restart timeout')

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--run-id', required=True)
    parser.add_argument('--versions', default='5.7.44,8.0.44,8.4.7,9.5.0')
    args = parser.parse_args()
    if not args.run_id or any(c not in 'abcdefghijklmnopqrstuvwxyz0123456789-' for c in args.run_id):
        parser.error('invalid run id')
    base = ROOT/'evidence'/args.run_id
    base.mkdir(exist_ok=False)
    results = []
    for version in args.versions.split(','):
        out = base/('mysql-'+version)
        out.mkdir()
        fixture = Fixture(out, 'mysql', version, argparse.Namespace(
            platform=None, scale=1000, tables=3, health_timeout=300))
        fixture.name = 'oo-a-'+fixture.id
        fixture.volume = fixture.name+'-data'
        replay = Replay()
        result = {'version': version, 'status': 'failed', 'model': 'synthetic replay'}
        try:
            fixture.setup()
            fixture.seed()
            fixture.verify_shapes()
            fixture.gold()
            fixture.tls()
            # Host-forwarded reader is distinct from the seed-admin connection.
            fixture.sql("CREATE USER 'fixture_reader'@'%' IDENTIFIED BY '"+fixture.reader_secret+"' REQUIRE SSL;\n"
                        "GRANT SELECT, SHOW VIEW ON fixture.* TO 'fixture_reader'@'%';\n"
                        'ANALYZE TABLE events, order_lines, string_keys;', admin=True)
            home = fixture.private/'home'
            store = home/'.config/open-ontology/secrets'
            store.mkdir(parents=True, mode=0o700)
            key = secrets.token_hex(24)
            for name, value in [('reader',fixture.reader_secret),('model',key),('ca',(fixture.private/'ca.pem').read_text())]:
                path = store/name
                path.touch(mode=0o600)
                path.write_text(value)
            _, address = fixture.command(['docker','port',fixture.name,'3306/tcp'])
            source = {'id':'fixture','engine':'mysql-'+version,'host':'localhost',
                      'port':int(address.strip().rsplit(':',1)[1]),'database':'fixture','user':'fixture_reader',
                      'password':{'secret_ref':'reader'},'tls_ca':{'secret_ref':'ca'},
                      'scope':{'include':['fixture.events','fixture.order_lines','fixture.string_keys']}}
            config = {'version':'oo.stack/v1','sources':[source],
                      'model':{'provider':'openai-compatible',
                               'base_url':f'http://127.0.0.1:{replay.httpd.server_port}/v1',
                               'model':'fixture-model','key':{'secret_ref':'model'}}}
            path = out/'stack.json'
            def doctor(name, expected):
                path.write_text(json.dumps(config))
                proc = subprocess.run([str(ROOT/'target/debug/oo'),'doctor','--config',str(path),'--json'],
                                      env=dict(os.environ,HOME=str(home)),capture_output=True,timeout=180)
                raw = proc.stdout+proc.stderr
                assert all(s.encode() not in raw for s in [fixture.secret,fixture.reader_secret,key])
                (out/(name+'.json')).write_bytes(raw)
                (out/(name+'.exit')).write_text(str(proc.returncode))
                assert proc.returncode == expected, name
                report = json.loads(proc.stdout)
                assert report['model']['status'] == 'ok'
                return report['sources'][0]
            good = doctor('healthy',0)
            v = good['probe']['version']
            assert (v['major'],v['minor'],v['patch']) == tuple(map(int,version.split('.')))
            assert len(good['chunk_plans']) == 3
            # The integrated MySQL adapter inherits estimate_bytes -> None. Do not
            # invent physical bytes from row counts; retain the unknown basis.
            assert all(p['estimated_bytes'] is None and p['bytes_basis']=='unknown'
                       for p in good['chunk_plans'])
            assert good['dba_requirements']
            source['sync'] = {'mode':'C1'}
            doctor('c1-missing-grants',1)
            source.pop('sync')
            source.pop('tls_ca')
            assert doctor('missing-ca',1)['error'] == 'SOURCE_TLS_CA_REQUIRED'
            result.update(status='passed',source=good,execution=fixture.receipt['execution'])
        except Exception as error:
            result['error'] = type(error).__name__+': '+str(error)
        finally:
            replay.close()
            result['cleanup_failures'] = fixture.cleanup()
            if result['cleanup_failures']:
                result['status'] = 'failed'
            (out/'result.json').write_text(json.dumps(result,indent=2))
            checksums(out)
            results.append(result)
            print(version,result['status'],result.get('error',''),flush=True)
    (base/'results.json').write_text(json.dumps(results,indent=2))
    return int(any(r['status']!='passed' for r in results))

if __name__ == '__main__':
    raise SystemExit(main())
