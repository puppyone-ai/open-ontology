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

class InstalledPolicy(unittest.TestCase):
    def test_policy_is_installed_not_inferred_from_capture(self):
        with tempfile.TemporaryDirectory() as d:
            path=Path(d)/'config.json';path.write_text('{}')
            p.installed_policy({},path)
            self.assertNotIn('source_audit_policy',json.loads(path.read_text()))
            p.installed_policy({'source_audit_policy':'grant_enforced'},path)
            self.assertEqual(json.loads(path.read_text())['source_audit_policy'],'grant_enforced')
            with self.assertRaisesRegex(RuntimeError,'audit policy'):
                p.installed_policy({'source_audit_policy':'anything'},path)

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

class FullSnapshot(unittest.TestCase):
    def test_full_capture_restarts_failed_attempt_then_replays_only_success(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);driver=root/'driver';driver.write_bytes(b'fixed')
            capture_config=root/'capture-config.json'
            capture_config.write_text(json.dumps(dict(protocol='oo.mysql-full/v1',source_id='full_mysql',secret_ref='secret://full/reader',table={'table':'items'},budgets={'max_bytes':1048576})))
            identity=dict(path=str(driver),sha256=hashlib.sha256(driver.read_bytes()).hexdigest())
            c=dict(capture_config=str(capture_config),driver=identity,admission=identity,state_dir=str(root/'state'),table='items',prepare='prepare',secret_ref='secret://full/reader',target={},registry_template=str(root/'registered/{binding}/{pin}/registry.json'))
            cfg=root/'producer.json';cfg.write_text(json.dumps(c))
            o=dict(protocol='oo.jobs.native-producer/v1',source_id='full_mysql',method='C5',binding='00000000-0000-0000-0000-000000000002',pin=1,acknowledged_source_generation=None)
            captures=[]
            def call(argv,log):
                if argv[1]=='capture-full':
                    dest=Path(argv[5]);captures.append(dest);dest.mkdir()
                    if len(captures)==1:raise RuntimeError('interrupted snapshot')
                    (dest/'capturer.json').write_text(json.dumps({'tables':[{'name':'items','schema':'sample','arrow_path':'real.arrow','consistency':'S0'}]}))
                elif argv[1]=='seal-native':
                    dest=Path(argv[-1]);dest.mkdir();b=dict(source_id='full_mysql',source_generation=1,fields=[dict(name='id',arrow_type='utf8',nullable=False,key=True)],run_id='run',admission_receipt_path='receipt',snapshot={'kind':'mysql_consistent_snapshot','value':'run'},capture_receipt_sha256='a'*64,config_digest='b'*64,schema_digest='c'*64,type_digest='d'*64,input_digest='e'*64)
                    (dest/'registry.json').write_text(json.dumps({'bindings':[b]}))
            def invoke():
                stdout=io.StringIO()
                with patch.object(sys,'argv',['producer','--config',str(cfg)]),patch.object(sys,'stdin',io.StringIO(json.dumps(o))),patch.object(sys,'stdout',stdout),patch.object(p,'call',call):p.main()
                return stdout.getvalue()
            with self.assertRaisesRegex(RuntimeError,'interrupted'):invoke()
            self.assertEqual(list((root/'state').glob('*/native-request.json')),[])
            result=invoke();self.assertEqual(invoke(),result)
            request=json.loads(result)
            self.assertEqual(request['consistency_grade'],'S0');self.assertTrue(request['full_snapshot'])
            self.assertEqual(request['position_codec'],'snapshot-zero/v1');self.assertEqual(request['key_columns'],['id'])
            self.assertEqual(request['max_arrow_bytes'],1048576)
            self.assertEqual(len(captures),2);self.assertNotEqual(captures[0],captures[1])

class ChunkResume(unittest.TestCase):
    def test_interrupted_chunk_capture_keeps_attempt_owner_and_config(self):
        self.run_retry(False)

    def test_changed_config_refuses_checkpoint_before_source_call(self):
        self.run_retry(True)

    def test_multiple_checkpoints_refuse_before_source_call(self):
        self.run_retry(False, ambiguous=True)

    def run_retry(self, change_config, ambiguous=False):
        with tempfile.TemporaryDirectory(prefix='oo-wf-mpart-producer-') as d:
            root=Path(d);driver=root/'driver';driver.write_bytes(b'fixed')
            capture_config=root/'capture-config.json'
            original=dict(protocol='oo.mysql-full/v1',source_id='full_mysql',secret_ref='secret://full/reader',table={'table':'items'},budgets={'max_bytes':1048576},chunking=True)
            capture_config.write_text(json.dumps(original))
            identity=dict(path=str(driver),sha256=hashlib.sha256(driver.read_bytes()).hexdigest())
            c=dict(capture_config=str(capture_config),driver=identity,admission=identity,state_dir=str(root/'state'),table='items',prepare='prepare',secret_ref='secret://full/reader',target={},registry_template=str(root/'registered/{binding}/{pin}/registry.json'))
            cfg=root/'producer.json';cfg.write_text(json.dumps(c))
            o=dict(protocol='oo.jobs.native-producer/v1',source_id='full_mysql',method='C5',binding='00000000-0000-0000-0000-000000000003',pin=1,acknowledged_source_generation=4)
            captures=[];configs=[]
            def call(argv,log):
                self.assertEqual(argv[1],'capture-full')
                dest=Path(argv[5]);captures.append(dest)
                configs.append(Path(argv[3]).read_bytes())
                if len(captures)==1:
                    dest.mkdir();(dest/'chunk-binding.json').write_text('{}')
                    (dest/'chunks.json').write_text('[]')
                    (dest/'0.arrow').write_bytes(b'checkpoint bytes')
                    raise RuntimeError('interrupted snapshot')
                self.assertEqual(dest,captures[0])
                self.assertEqual(configs[0],configs[1])
                self.assertEqual((dest/'0.arrow').read_bytes(),b'checkpoint bytes')
                raise RuntimeError('resumed same checkpoint')
            def invoke():
                with patch.object(sys,'argv',['producer','--config',str(cfg)]),patch.object(sys,'stdin',io.StringIO(json.dumps(o))),patch.object(p,'call',call):p.main()
            with self.assertRaisesRegex(RuntimeError,'interrupted'):invoke()
            if ambiguous:
                occurrence=next((root/'state').iterdir())
                extra=occurrence/'other'/'capture';extra.mkdir(parents=True)
                (extra/'chunk-binding.json').write_text('{}')
                with self.assertRaisesRegex(RuntimeError,'ambiguous capture checkpoints'):invoke()
                self.assertEqual(len(captures),1)
            elif change_config:
                original['budgets']['max_bytes']=524288
                capture_config.write_text(json.dumps(original))
                with self.assertRaisesRegex(RuntimeError,'configuration changed'):invoke()
                self.assertEqual(len(captures),1)
            else:
                with self.assertRaisesRegex(RuntimeError,'resumed same checkpoint'):invoke()
                self.assertEqual(json.loads(configs[0])['generation'],5)
            self.assertEqual(list((root/'state').glob('*/native-request.json')),[])

if __name__=='__main__':unittest.main()
