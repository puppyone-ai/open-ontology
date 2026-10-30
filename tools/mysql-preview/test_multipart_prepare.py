import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

class AnchorPreparer(unittest.TestCase):
    def test_anchor_witness_never_names_combined_arrow(self):
        with tempfile.TemporaryDirectory(prefix='oo-wf-mpart-anchor-') as d:
            root=Path(d);capture=root/'capture';capture.mkdir();driver=root/'driver';driver.write_bytes(b'owned')
            import hashlib
            (capture/'driver.json').write_text(json.dumps(dict(executable_path=str(driver),executable_sha256=hashlib.sha256(driver.read_bytes()).hexdigest())))
            facts=dict(source_id='fixture',source_generation=1,run_id='run',snapshot={'kind':'mysql_consistent_snapshot','value':'run'},max_rows=6,chunk_budget={'max_rows':2},observed_privileges=['SELECT'],multipart_scope={'part_count':3},tables=[dict(name='items',schema='fixture',consistency='B',arrow='table-0.arrow',arrow_path=str(capture/'table-0.arrow'))])
            (capture/'capturer.json').write_text(json.dumps(facts))
            r=subprocess.run([sys.executable,str(Path(__file__).with_name('prepare-native.py')),'--capture',str(capture),'--out',str(root/'inputs'),'--secret-ref','secret://fixture/reader','--multipart'],capture_output=True,text=True)
            self.assertEqual(r.returncode,0,r.stderr)
            witness=json.loads((root/'inputs/items/witness.json').read_text())
            self.assertTrue(witness['multipart_anchor']);self.assertEqual(witness['arrow_path'],str(capture/'0.arrow'))
            config=json.loads((root/'inputs/items/config.json').read_text());self.assertEqual(config['budget']['hard_limit'],2)

if __name__=='__main__':unittest.main()
