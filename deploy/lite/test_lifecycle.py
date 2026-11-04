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

    def test_status_uses_saved_run_times_not_http_invention(self):
        self.request['action']='status'
        lifecycle.write(self.root.parent/'run.json',b'{"last_success":"synthetic-time","last_failure":null}')
        with patch.object(lifecycle,'docker',return_value=b''):
            result=lifecycle.main(self.request)
        self.assertEqual(result['last_success'],'synthetic-time')
        self.assertIsNone(result['next_due'])
        self.assertEqual(result['gateway_health']['status'],'unmeasured')

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

if __name__=='__main__':unittest.main()
