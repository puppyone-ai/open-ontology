#!/usr/bin/env python3
"""First occurrence must capture the configured connection; retry keeps its bytes."""
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

spec=importlib.util.spec_from_file_location('producer',Path(__file__).with_name('native-producer.py'))
p=importlib.util.module_from_spec(spec);spec.loader.exec_module(p)

class FreshConnection(unittest.TestCase):
    def test_first_capture_and_same_occurrence_replay(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);driver=root/'driver';driver.write_bytes(b'fixed')
            capture_config=root/'capture-config.json'
            capture_config.write_text(json.dumps(dict(source_id='new_mysql',mysql={'host':'approved-new-host'},tables=[{'table':'hubs'}])))
            c=dict(capture_config=str(capture_config),driver=dict(path=str(driver),sha256=hashlib.sha256(driver.read_bytes()).hexdigest()),admission=dict(path=str(driver),sha256=hashlib.sha256(driver.read_bytes()).hexdigest()),state_dir=str(root/'state'),table='hubs',prepare='prepare',secret_ref='secret://new/mysql',target={},registry_template=str(root/'registered/{binding}/{pin}/registry.json'))
            cfg=root/'producer.json';cfg.write_text(json.dumps(c))
            o=dict(protocol='oo.jobs.native-producer/v1',source_id='new_mysql',method='C5',binding='00000000-0000-0000-0000-000000000001',pin=1,acknowledged_source_generation=None)
            calls=[]
            def call(argv,log):
                calls.append(argv)
                if argv[1]=='capture-native':
                    observed=json.loads(Path(argv[3]).read_text());self.assertEqual(observed['mysql']['host'],'approved-new-host')
                    dest=Path(argv[5]);dest.mkdir();(dest/'capturer.json').write_text(json.dumps({'tables':[{'name':'hubs','schema':'approved_db','arrow_path':'fresh.arrow'}]}))
                elif argv[1]=='seal-native':
                    dest=Path(argv[-1]);dest.mkdir();b=dict(source_id='new_mysql',source_generation=1,fields=[],run_id='new-run',admission_receipt_path='fresh-receipt',snapshot={},capture_receipt_sha256='a'*64,config_digest='b'*64,schema_digest='c'*64,type_digest='d'*64,input_digest='e'*64)
                    (dest/'registry.json').write_text(json.dumps({'bindings':[b]}))
            for attempt in range(2):
                stdout=io.StringIO()
                with patch.object(sys,'argv',['producer','--config',str(cfg)]),patch.object(sys,'stdin',io.StringIO(json.dumps(o))),patch.object(sys,'stdout',stdout),patch.object(p,'call',call):p.main()
                r=json.loads(stdout.getvalue());self.assertEqual(r['position_lineage'],'new_mysql.hubs');self.assertEqual(r['arrow_path'],'fresh.arrow')
            self.assertEqual(len(calls),3)

if __name__=='__main__':unittest.main()
