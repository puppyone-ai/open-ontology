"""D16 provisioning: `oo up` derives the runtime build chain from stack.yaml."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('lifecycle', HERE / 'lifecycle.py')
lifecycle = importlib.util.module_from_spec(spec)
spec.loader.exec_module(lifecycle)


def stack(host='127.0.0.1', url='auto'):
    return {'sources': [{'id': 'erp', 'engine': 'postgres', 'host': host, 'port': 5432,
                         'password': {'secret_ref': 'erp-password'}}],
            'model': {'provider': 'openai-compatible', 'base_url': {'secret_ref': 'endpoint'},
                      'model': 'm', 'key': {'secret_ref': 'key'}},
            'gateway': {'public_url': url}}


class Tests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(dir=HERE.parents[1] / 'evidence')
        self.root = Path(self.tmp.name) / 'lite'
        for path in (self.root, self.root / 'pki', self.root / 'work', self.root / 'home',
                     self.root / 'home/.config', self.root / 'home/.config/open-ontology',
                     self.root / 'home/.config/open-ontology/secrets'):
            path.mkdir(mode=0o700)
        for name in ('token-runtime-password', 'token-admin-password', 'lite-admin-password'):
            lifecycle.write(self.root / 'pki' / name, b'a' * 64)
        for name in ('lite-ca-cert', 'lite-gateway-key', 'lite-gateway-cert', 'lite-admin-cert',
                     'lite-admin-key', 'lite-consumer-cert', 'lite-consumer-key'):
            lifecycle.write(self.root / 'pki' / name, name.encode())

    def tearDown(self):
        self.tmp.cleanup()

    def test_gateway_port_is_local_https_and_stable(self):
        self.assertEqual(lifecycle.gateway_port(stack(url='https://localhost:9443'), {}), 9443)
        self.assertEqual(lifecycle.gateway_port(stack(), {'gateway_port': 4443}), 4443)
        self.assertEqual(lifecycle.gateway_port(stack(), {'image': 'old'}), 8443)
        self.assertGreater(lifecycle.gateway_port(stack(), {}), 0)
        for url in ('http://localhost:1', 'https://gateway.example'):
            with self.assertRaisesRegex(ValueError, 'LITE_PUBLIC_URL_LOCAL_HTTPS'):
                lifecycle.gateway_port(stack(url=url), {})

    def test_runtime_stack_redirects_only_loopback_sources(self):
        self.assertEqual(lifecycle.runtime_stack(stack())['sources'][0]['host'], 'host.docker.internal')
        self.assertEqual(lifecycle.runtime_stack(stack('db.internal'))['sources'][0]['host'], 'db.internal')

    def test_provision_writes_runtime_configuration_without_secret_values(self):
        calls = []
        with patch.object(lifecycle, 'compose_input', side_effect=lambda *a, **k: calls.append(a) or b''), \
                patch.object(lifecycle, 'docker', side_effect=lambda *a: calls.append(a) or b''):
            for _ in range(2):  # Idempotent repeat.
                lifecycle.provision_retained(self.root, ['compose'], {}, {'stack': stack()}, 4443)
        operator = self.root / 'work/operator'
        runtime = json.loads(lifecycle.read_private(operator / 'build-runtime.json'))
        self.assertEqual(runtime['gateway']['gateway_url'], 'https://localhost:4443')
        self.assertEqual(runtime['gateway']['first_user_grant']['audience_url'], 'https://localhost:4443')
        self.assertEqual(runtime['gateway']['prompt_client_pem'], str(self.root / 'consumer/client.pem'))
        self.assertEqual(runtime['source_id'], 'erp')
        gateway = json.loads(lifecycle.read_private(operator / 'gateway/config.json'))
        self.assertEqual(gateway['listen'], '0.0.0.0:4443')
        self.assertEqual(gateway['token_admin_workload'], 'urn:oo:workload:admin')
        build_stack = json.loads(lifecycle.read_private(self.root / 'work/build/stack.json'))
        self.assertEqual(build_stack['sources'][0]['host'], 'host.docker.internal')
        service = lifecycle.read_private(self.root / 'home/.pg_service.conf').decode()
        self.assertIn('sslmode=verify-full', service)
        self.assertIn('channel_binding=disable', service)
        secret = (b'a' * 64).decode()
        for path in [operator / 'build-runtime.json', operator / 'target.json', operator / 'publisher.json',
                     operator / 'gateway/config.json', self.root / 'work/build/stack.json']:
            self.assertNotIn(secret, lifecycle.read_private(path).decode())
        self.assertNotIn(secret, json.dumps([list(map(str, c[:3])) for c in calls]))
        # The Gateway waits for the first reviewed publication.
        self.assertFalse(any('gateway' in c[0] for c in calls if isinstance(c[0], list)))

    def test_schedule_waits_for_the_first_build_and_reuses_its_review(self):
        request = {'stack': stack(), 'cycle_events': 'synthetic', 'jobs': {name: '-- ' + name for name in lifecycle.JOBS}}
        self.assertFalse(lifecycle.provision_schedule(self.root, [], {}, request))
        state = self.root / 'work/build/.oo'
        state.mkdir(parents=True, mode=0o700)
        (self.root / 'work/operator').mkdir(mode=0o700)
        (self.root / 'consumer').mkdir(mode=0o700)
        lifecycle.write(self.root / 'consumer/ca.crt', b'ca')
        lifecycle.write(self.root / 'owner.json', json.dumps({'gateway_port': 4443}).encode())
        for name in ('facts.json', 'reviewed-registry.json'):
            lifecycle.write(state / name, name.encode())
        lifecycle.write(state / 'chain.json', b'{"status":"blocked"}')
        self.assertFalse(lifecycle.provision_schedule(self.root, [], {}, request))
        lifecycle.write(state / 'chain.json', b'{"status":"complete"}')
        sql = []
        with patch.object(lifecycle, 'compose_input',
                          side_effect=lambda argv, env, args, data, code, *a: sql.append(data) or b'f'):
            self.assertTrue(lifecycle.provision_schedule(self.root, [], {}, request))
        installed = sql[1].decode()
        self.assertIn('RESET ROLE; GRANT CONNECT ON DATABASE postgres TO oo_token_admin', installed)
        self.assertLess(installed.index('-- 0000_bootstrap_roles.sql'), installed.index('SET LOCAL ROLE oo_jobs_owner'))
        config = json.loads(lifecycle.read_private(self.root / 'work/operator/schedule.json'))
        self.assertEqual(config['interval_seconds'], 8 * 3600)
        self.assertEqual(config['cycle']['gateway_probe']['url'], 'https://gateway:4443')
        self.assertEqual(config['cycle']['builder']['origin']['reviewed_registry']['path'],
                         '/work/build/.oo/reviewed-registry.json')
        self.assertIn(config['project'], sql[3].decode())
        with self.assertRaisesRegex(ValueError, 'LITE_JOBS_TEMPLATES'):
            lifecycle.provision_schedule(self.root, [], {}, {'stack': stack()})

    def test_sync_interval_bounds(self):
        self.assertEqual(lifecycle.interval_seconds('30m'), 1800)
        for value in ('10s', '2d', 'soon'):
            with self.assertRaisesRegex(ValueError, 'LITE_SYNC_INTERVAL'):
                lifecycle.interval_seconds(value)

    def test_staged_npm_registry_reaches_the_share_prompt_configuration(self):
        request = {'stack': stack(), 'prompt_npm_registry': 'http://localhost:4873/'}
        with patch.object(lifecycle, 'compose_input', return_value=b''), \
                patch.object(lifecycle, 'docker', return_value=b''):
            lifecycle.provision_retained(self.root, ['compose'], {}, request, 4443)
        runtime = json.loads(lifecycle.read_private(self.root / 'work/operator/build-runtime.json'))
        self.assertEqual(runtime['gateway']['prompt_npm_registry'], 'http://localhost:4873/')

    def test_more_than_one_source_is_refused(self):
        two = stack()
        two['sources'].append(dict(two['sources'][0], id='crm'))
        with self.assertRaisesRegex(ValueError, 'LITE_BUILD_ONE_SOURCE'):
            lifecycle.provision_retained(self.root, [], {}, {'stack': two}, 4443)


if __name__ == '__main__':
    unittest.main()
