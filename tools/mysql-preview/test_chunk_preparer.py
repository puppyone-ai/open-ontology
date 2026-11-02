import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


class ChunkPreparerTest(unittest.TestCase):
    def test_grant_policy_requires_an_explicit_operator_choice(self):
        with tempfile.TemporaryDirectory(prefix='oo-wf-chunk-preparer-') as directory:
            root = Path(directory)
            capture = root / 'capture'
            capture.mkdir()
            driver = root / 'driver'
            driver.write_bytes(b'owned-fixture-driver')
            facts = dict(source_id='fixture', source_generation=1, snapshot={'kind':'mysql_consistent_snapshot','value':'run'}, run_id='run', observed_privileges=['SELECT'], max_rows=4, source_audit={'source':'mysql.chunk_audits/v1','chunks':[{'source':'grant_enforced'}]}, tables=[dict(name='items', schema='fixture', consistency='B', arrow='table-0.arrow')])
            (capture/'capturer.json').write_text(json.dumps(facts))
            (capture/'driver.json').write_text(json.dumps(dict(executable_path=str(driver), executable_sha256=hashlib.sha256(driver.read_bytes()).hexdigest())))
            base = [sys.executable, str(Path(__file__).with_name('prepare-native.py')), '--capture',str(capture),'--secret-ref','secret://fixture/reader']
            for policy in [None,'grant_enforced']:
                output = root / ('default' if policy is None else 'explicit')
                args = base + ['--out',str(output)]
                if policy:
                    args += ['--source-audit-policy',policy]
                result = subprocess.run(args,capture_output=True,text=True)
                self.assertEqual(result.returncode,0,result.stderr)
                config = json.loads((output/'items'/'config.json').read_text())
                self.assertEqual(config.get('source_audit_policy'),policy)
                report = json.loads((output/'items'/'report.json').read_text())
                self.assertEqual(report['capture_methods'][0]['consistency'],'B')


if __name__ == '__main__':
    unittest.main()
