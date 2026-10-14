import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import connection_capture as capture

class ConnectionCaptureTest(unittest.TestCase):
    def test_actual_connection_consumed_and_secret_never_in_argv(self):
        with tempfile.TemporaryDirectory() as d:
            secret=Path(d)/'secret';secret.write_text(json.dumps({'username':'APP_READER','password':'test-marker'}));secret.chmod(0o600)
            cfg={'type':'oracle','host':'other.example','port':1522,'database':'OTHERDB','user':'APP_READER','password_file':str(secret)}
            sql,password=capture.prepare(cfg)
            for fragment in ['HOST=other.example','PORT=1522','SERVICE_NAME=OTHERDB','CONNECT APP_READER/','ROWNUM <= 5']:
                self.assertIn(fragment,sql)
            self.assertNotIn('OO04_FIX',sql)
            with patch('connection_capture.subprocess.run') as run:
                run.return_value.returncode=0;run.return_value.stdout=b'CONNECTED|APP_READER\nSAMPLE_DONE\n';run.return_value.stderr=b''
                self.assertEqual(capture.run(cfg,Path(d)/'capture'),0)
                self.assertNotIn(password,str(run.call_args.args[0]))
                receipt=json.loads((Path(d)/'capture/receipt.json').read_text())
                self.assertEqual(receipt['s0_s10'],'not_run')
    def test_invalid_identifier_before_secret_io(self):
        with patch.object(Path,'read_text',side_effect=AssertionError('secret read')):
            with self.assertRaises(ValueError):
                capture.prepare({'type':'oracle','database':'a)bad','user':'reader','host':'host','port':1521,'password_file':'/no-secret'})
    def test_secret_identity_must_match(self):
        with tempfile.TemporaryDirectory() as d:
            secret=Path(d)/'secret';secret.write_text(json.dumps({'username':'other','password':'test'}));secret.chmod(0o600)
            with self.assertRaisesRegex(ValueError,'identity mismatch'):
                capture.prepare({'type':'oracle','database':'DB','user':'reader','host':'host','port':1521,'password_file':str(secret)})

if __name__=='__main__': unittest.main()
