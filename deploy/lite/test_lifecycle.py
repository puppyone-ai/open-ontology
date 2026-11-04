import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch, MagicMock

HERE=Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location('lifecycle',HERE/'lifecycle.py')
lifecycle=importlib.util.module_from_spec(spec)
spec.loader.exec_module(lifecycle)

class Tests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory(dir=HERE.parents[1]/'evidence')
        self.root=Path(self.tmp.name)/'lite'
        self.root.mkdir(mode=0o700)
        self.request={'root':str(self.root),'action':'down','stack':{'gateway':{'public_url':'auto'}}}
        lifecycle.write(self.root/'owner.json',json.dumps({'image':'sha256:'+'a'*64,'subnet':'10.253.0.0/24'}).encode())

    def tearDown(self):
        self.tmp.cleanup()

    def test_down_never_removes_volumes_or_orphans(self):
        calls=[]
        def docker(args,env):
            calls.append(args)
            return b'[]' if 'ps' in args and '--format' in args else b''
        with patch.object(lifecycle,'docker',side_effect=docker):
            result=lifecycle.main(self.request)
        self.assertEqual(result['status'],'stopped')
        self.assertTrue(any(a[-1]=='down' for a in calls))
        self.assertFalse(any('-v' in a or '--volumes' in a or '--remove-orphans' in a or 'prune' in a for a in calls))
        self.assertFalse(result['healthy'])

    def test_wrong_owner_stops_before_down(self):
        calls=[]
        def docker(args,env):
            calls.append(args)
            return b'other' if args[0]=='ps' else b'{"oo.lane":"other"}'
        with patch.object(lifecycle,'docker',side_effect=docker):
            with self.assertRaisesRegex(ValueError,'LITE_OWNERSHIP'):
                lifecycle.main(self.request)
        self.assertFalse(any('down' in a for a in calls))

    def test_absent_health_credentials_never_imply_healthy(self):
        result=lifecycle.health(self.root,self.request['stack'])
        self.assertEqual(result['status'],'unmeasured')
        self.assertIsNone(result['nearest_credential_expiry_ms'])

    def test_status_never_uses_local_receipts_as_job_times(self):
        self.request['action']='status'
        lifecycle.write(self.root.parent/'run.json',b'{"last_success":"synthetic-time","last_failure":null}')
        with patch.object(lifecycle,'docker',return_value=b''):
            result=lifecycle.main(self.request)
        self.assertIsNone(result['last_success'])
        self.assertIsNone(result['next_due'])
        self.assertEqual(result['schedule_status'],'not_configured')
        self.assertEqual(result['gateway_health']['status'],'unmeasured')

    def schedule(self,project='00000000-0000-0000-0000-000000000001'):
        (self.root/'work/operator').mkdir(parents=True)
        lifecycle.write(self.root/'work/operator/schedule.json',
                        json.dumps({'project':project}).encode())

    def test_control_status_queries_owned_db_read_only(self):
        self.schedule()
        times={'last_success':'2026-11-04T01:02:03.000Z','last_failure':None,
               'next_due':'2026-11-04T01:02:33.000Z'}
        with patch.object(lifecycle,'docker',return_value=json.dumps(times).encode()) as call:
            result=lifecycle.control_status(self.root,['compose'],{})
        self.assertEqual(result,{**times,'schedule_basis':'control_db','schedule_status':'measured'})
        args=call.call_args.args[0]
        self.assertEqual(args[1:5],['exec','-T','control-db','psql'])
        self.assertIn('READ ONLY',args[-1])
        self.assertIn('SET LOCAL ROLE oo_jobs_runtime',args[-1])

    def test_control_errors_do_not_fall_back_to_saved_success(self):
        self.schedule()
        for raw in (b'',b'{}',b'{"last_success":"invented","last_failure":null,"next_due":null}'):
            with patch.object(lifecycle,'docker',return_value=raw):
                result=lifecycle.control_status(self.root,[],{})
            self.assertEqual(result['schedule_status'],'unavailable')
            self.assertIsNone(result['last_success'])
        with patch.object(lifecycle,'docker',side_effect=ValueError('private error')):
            self.assertNotIn('private error',json.dumps(lifecycle.control_status(self.root,[],{})))

    def test_schedule_project_is_validated_before_sql(self):
        self.schedule("x'; SELECT 1; --")
        with patch.object(lifecycle,'docker') as call:
            self.assertEqual(lifecycle.control_status(self.root,[],{})['schedule_status'],'unavailable')
        call.assert_not_called()

    def test_subnet_avoids_existing_networks_without_deletion(self):
        with patch.object(lifecycle,'docker',return_value=b''):
            chosen=lifecycle.subnet('project',{})
        def docker(args,env):
            return b'owned' if args[1]=='ls' else json.dumps([{'Subnet':chosen}]).encode()
        with patch.object(lifecycle,'docker',side_effect=docker):
            self.assertNotEqual(lifecycle.subnet('project',{}),chosen)

    def test_health_uses_earliest_api_deadline_without_echoing_other_fields(self):
        (self.root/'pki').mkdir()
        lifecycle.write(self.root/'pki/token',b'synthetic-private-token')
        lifecycle.write(self.root/'pki/ca',b'synthetic-ca')
        lifecycle.write(self.root/'health-client.json',b'{"token":"token","ca":"ca"}')
        conn=MagicMock()
        conn.getresponse.return_value.status=200
        conn.getresponse.return_value.read.return_value=json.dumps({
            'token_expiry':{'expires_at_ms':200,'unexpected_secret':'never-echo'},
            'internal_mtls_certificate_expiry':{'expires_at_ms':100}}).encode()
        with patch.object(lifecycle.ssl,'create_default_context'), patch.object(lifecycle.http.client,'HTTPSConnection',return_value=conn):
            result=lifecycle.health(self.root,{'gateway':{'public_url':'https://localhost'}})
        self.assertEqual(result['nearest_credential_expiry_ms'],100)
        self.assertNotIn('never-echo',json.dumps(result))
        self.assertEqual(conn.request.call_args.args[:2],('GET','/v1/health'))

    def test_private_root_rejects_symlink(self):
        alias=self.root.parent/'alias'
        alias.symlink_to(self.root)
        with self.assertRaises(ValueError):
            lifecycle.private_dir(alias)

    def test_health_client_certificate_pair_stays_private_and_local(self):
        (self.root/'pki').mkdir()
        for name in ('token','ca','cert','key'):
            lifecycle.write(self.root/'pki'/name,b'synthetic')
        config={'token':'token','ca':'ca','cert':'cert','key':'key'}
        lifecycle.write(self.root/'health-client.json',json.dumps(config).encode())
        connection=MagicMock()
        connection.getresponse.return_value.status=200
        connection.getresponse.return_value.read.return_value=b'{"token_expiry":{},"internal_mtls_certificate_expiry":{}}'
        with patch.object(lifecycle.ssl,'create_default_context') as context, patch.object(
                lifecycle.http.client,'HTTPSConnection',return_value=connection):
            lifecycle.health(self.root,{'gateway':{'public_url':'https://localhost'}})
            context.return_value.load_cert_chain.assert_called_once_with(
                self.root/'pki/cert',self.root/'pki/key')
            (self.root/'pki/key').chmod(0o644)
            with self.assertRaisesRegex(ValueError,'LITE_PRIVATE_FILE'):
                lifecycle.health(self.root,{'gateway':{'public_url':'https://localhost'}})
            config['key']='../outside'
            lifecycle.write(self.root/'health-client.json',json.dumps(config).encode())
            with self.assertRaisesRegex(ValueError,'LITE_HEALTH_REFERENCE'):
                lifecycle.health(self.root,{'gateway':{'public_url':'https://localhost'}})

    def test_up_copies_endpoint_reference_without_expanding_stack(self):
        home=self.root.parent/'source-home'
        store=home/'.config/open-ontology/secrets'
        store.mkdir(parents=True,mode=0o700)
        for name,data in [('endpoint',b'https://private.example/v1'),('model',b'synthetic-key')]:
            lifecycle.write(store/name,data)
        self.request.update(action='up',image='sha256:'+'a'*64,
            templates={name:'' for name in ('compose.yaml','postgres.sh','scheduler.py')},
            stack={'sources':[],'gateway':{'public_url':'auto'},
                   'model':{'key':{'secret_ref':'model'},'base_url':{'secret_ref':'endpoint'}}})
        with patch.object(lifecycle.Path,'home',return_value=home), patch.object(lifecycle,'docker',return_value=b''), patch.object(lifecycle,'operator_bootstrap'):
            lifecycle.main(self.request)
        self.assertEqual(lifecycle.read_private(self.root/'home/.config/open-ontology/secrets/endpoint'),
                         b'https://private.example/v1')
        self.assertNotIn(b'private.example',lifecycle.read_private(self.root/'work/stack.json'))
        lifecycle.write(store/'endpoint',b'https://changed.example/v1')
        with patch.object(lifecycle.Path,'home',return_value=home), patch.object(lifecycle,'docker',return_value=b''):
            with self.assertRaisesRegex(ValueError,'LITE_SECRET_CHANGE_REQUIRES_EXPLICIT_ROTATION'):
                lifecycle.main(self.request)

    def test_operator_bootstrap_is_reserved_idempotent_and_file_only(self):
        pki=self.root/'pki'
        pki.mkdir(mode=0o700)
        calls=[]
        def docker(args,env):
            calls.append(args)
            if 'issue' in args:
                lifecycle.write(pki/'operator-token',b'ooct_fixture')
            return b''
        with patch.object(lifecycle,'docker',side_effect=docker), patch.object(
                lifecycle.subprocess,'run',return_value=MagicMock(returncode=0)) as sql:
            for _ in range(2):
                lifecycle.operator_bootstrap(self.root,['compose'],{},self.request['stack'])
        self.assertEqual(sum('issue' in args for args in calls),1)
        grant=json.loads(lifecycle.read_private(pki/'operator-grant.json'))
        self.assertEqual(grant['role'],'lite_operator')
        self.assertEqual(grant['knowledge_version'],'unpublished')
        self.assertEqual(grant['claims'],[])
        for name in ('token-admin-password','token-runtime-password','operator-token'):
            secret=lifecycle.read_private(pki/name).decode()
            self.assertNotIn(secret,json.dumps(calls))
            self.assertNotIn(secret,json.dumps(sql.call_args.args))
        self.request['stack']['gateway']['public_url']='https://changed.invalid'
        with self.assertRaisesRegex(ValueError,'LITE_OPERATOR_CHANGE'):
            lifecycle.operator_bootstrap(self.root,['compose'],{},self.request['stack'])

    def test_operator_bootstrap_does_not_issue_after_role_failure(self):
        (self.root/'pki').mkdir(mode=0o700)
        with patch.object(lifecycle,'docker') as docker, patch.object(
                lifecycle.subprocess,'run',return_value=MagicMock(returncode=1)):
            with self.assertRaisesRegex(ValueError,'LITE_OPERATOR_ROLES'):
                lifecycle.operator_bootstrap(self.root,['compose'],{},self.request['stack'])
        docker.assert_not_called()

if __name__=='__main__':unittest.main()
