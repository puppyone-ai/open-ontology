import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

spec=importlib.util.spec_from_file_location('onboard',Path(__file__).with_name('onboard.py'))
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)

class RunnerTests(unittest.TestCase):
    def write(self,p,value):
        p.write_text(json.dumps(value));p.chmod(0o600);return str(p)
    def test_real_process_resume_drift_and_secret_boundary(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d); secret=root/'secret';secret.write_text('DO_NOT_PRINT');secret.chmod(0o600)
            template=self.write(root/'template',dict(host='localhost',port=3306,database='demo',user='reader',password_file=''))
            script=root/'driver.py';script.write_text("import json,sys,pathlib\nc=json.load(open(sys.argv[1]));assert c['host']=='localhost'\npathlib.Path(sys.argv[2]).write_text(json.dumps({'driver':'actual-process','rows':5}))\n")
            recipe=dict(protocol='oo.source-onboarding.recipe/v1',engine='mysql',owned_fixture=False,mode='metadata_sample',read_only=True,max_rows=5,config_template=template,connection_map={f'/{k}':k for k in ('host','port','database','user','password_file')},pins=[dict(path=sys.executable,sha256=m.digest(sys.executable)),dict(path=str(script),sha256=m.digest(script))],steps=[dict(name='probe',argv=[sys.executable,str(script),'{config}','{run}/receipt.json'],artifacts=[dict(path='receipt.json')])])
            rp=self.write(root/'recipe',recipe);reg=self.write(root/'registry',{'secret://test':str(secret)})
            inp=dict(protocol='oo.source-onboarding/v1',connection=dict(type='mysql',database='demo',host='localhost',port=3306,user='reader',secret_ref='secret://test'),secret_registry=reg,recipe=rp,out=str(root/'run'))
            ip=self.write(root/'input',inp)
            self.assertEqual(m.run(ip)['state'],'recipe_completed')
            self.assertEqual(m.run(ip,True)['mode'],'metadata_sample')
            self.assertNotIn('DO_NOT_PRINT',(root/'run/journal.json').read_text())
            (root/'run/receipt.json').write_text('changed')
            with self.assertRaisesRegex(m.Refused,'resume_artifact_drift'):m.run(ip,True)
    def test_failed_attempt_is_retained_without_repeating_success(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);secret=root/'secret';secret.write_text('private');secret.chmod(0o600)
            template=self.write(root/'template',dict(host='localhost',port=3306,database='demo',user='reader',password_file=''))
            script=root/'driver.py';script.write_text("import pathlib,sys\np=pathlib.Path(sys.argv[1]);p.write_text('actual')\nif len(sys.argv)>2 and not pathlib.Path(sys.argv[2]).exists():sys.exit(3)\n")
            steps=[dict(name='first',argv=[sys.executable,str(script),'{run}/first'],artifacts=[dict(path='first')]),dict(name='second',argv=[sys.executable,str(script),'{run}/second',str(root/'allow')],artifacts=[dict(path='second')])]
            recipe=dict(protocol='oo.source-onboarding.recipe/v1',engine='mysql',owned_fixture=True,mode='source_capture',config_template=template,connection_map={f'/{k}':k for k in ('host','port','database','user','password_file')},pins=[dict(path=sys.executable,sha256=m.digest(sys.executable)),dict(path=str(script),sha256=m.digest(script))],steps=steps)
            rp=self.write(root/'recipe',recipe);reg=self.write(root/'registry',{'secret://test':str(secret)})
            ip=self.write(root/'input',dict(protocol='oo.source-onboarding/v1',connection=dict(type='mysql',database='demo',host='localhost',port=3306,user='reader',secret_ref='secret://test'),secret_registry=reg,recipe=rp,out=str(root/'run')))
            with self.assertRaisesRegex(m.Refused,'stage_failed'):m.run(ip)
            before=(root/'run/first').stat().st_mtime_ns
            (root/'allow').touch();self.assertEqual(m.run(ip,True)['state'],'recipe_completed')
            self.assertEqual((root/'run/first').stat().st_mtime_ns,before)
            journal=json.loads((root/'run/journal.json').read_text())
            self.assertEqual([x['state'] for x in journal['steps']],['succeeded','failed','succeeded'])
    def test_plain_connection_uses_installed_recipe_and_refuses_incomplete_exit(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);old=m.INSTALL_ROOT;m.INSTALL_ROOT=root/'install';m.INSTALL_ROOT.mkdir(mode=0o700)
            try:
                secret=root/'secret';secret.write_text('private');secret.chmod(0o600)
                template=self.write(root/'template',dict(host='localhost',port=3306,database='demo',user='reader',password_file=''))
                script=root/'driver.py';script.write_text("import pathlib,sys\npathlib.Path(sys.argv[1]).write_text('{\"full_product_chain_complete\":false}')\n")
                recipe=dict(protocol='oo.source-onboarding.recipe/v1',engine='mysql',owned_fixture=True,mode='whole_source',completion_artifact='whole.json',config_template=template,connection_map={f'/{k}':k for k in ('host','port','database','user','password_file')},pins=[dict(path=sys.executable,sha256=m.digest(sys.executable)),dict(path=str(script),sha256=m.digest(script))],steps=[dict(name='capture',argv=[sys.executable,str(script),'{run}/whole.json'],artifacts=[dict(path='whole.json')])])
                rp=self.write(root/'recipe',recipe);reg=self.write(root/'registry',{'secret://test':str(secret)})
                self.write(m.INSTALL_ROOT/'engines.json',{'protocol':'oo.source-onboarding.engines/v1','engines':{'mysql':dict(recipe=rp,recipe_sha256=m.digest(rp),secret_registry=reg)}})
                ip=self.write(root/'connection.json',dict(type='mysql',database='demo',host='localhost',port=3306,user='reader',secret_ref='secret://test'))
                with self.assertRaisesRegex(m.Refused,'whole_chain_incomplete'):m.run(ip)
                pointer=json.loads((root/'connection.json.run.json').read_text())
                self.assertTrue((Path(pointer['out'])/'whole.json').is_file())
                recipe['engine']='oracle';self.write(root/'recipe',recipe)
                with self.assertRaisesRegex(m.Refused,'installed_recipe_drift'):m.run(ip,True)
            finally:m.INSTALL_ROOT=old
    def test_capture_and_boolean_cannot_claim_whole(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);secret=root/'secret';secret.write_text('private');secret.chmod(0o600)
            template=self.write(root/'template',dict(host='localhost',port=3306,database='demo',user='reader',password_file=''))
            script=root/'driver.py';script.write_text("import pathlib,sys\npathlib.Path(sys.argv[1]).write_text(sys.argv[2])\n")
            body=json.dumps({'full_product_chain_complete':True,'periodic_insert_update_delete_verified':True,'recovery_verified':True,'product_receipt':'product.json','dialogue_verdict':'dialogue.json'})
            pins=[dict(path=sys.executable,sha256=m.digest(sys.executable)),dict(path=str(script),sha256=m.digest(script))]
            common=dict(protocol='oo.source-onboarding.recipe/v1',engine='mysql',owned_fixture=True,config_template=template,connection_map={f'/{k}':k for k in ('host','port','database','user','password_file')},pins=pins,steps=[dict(name='write',argv=[sys.executable,str(script),'{run}/whole.json',body],artifacts=[dict(path='whole.json')])])
            reg=self.write(root/'registry',{'secret://test':str(secret)})
            capture=dict(common,mode='source_capture',completion_artifact='whole.json')
            rp=self.write(root/'capture-recipe',capture)
            ip=self.write(root/'capture-input',dict(protocol='oo.source-onboarding/v1',connection=dict(type='mysql',database='demo',host='localhost',port=3306,user='reader',secret_ref='secret://test'),secret_registry=reg,recipe=rp,out=str(root/'capture-run')))
            with self.assertRaisesRegex(m.Refused,'capture_cannot_claim_whole'):m.run(ip)
            whole=dict(common,mode='whole_source',completion_artifact='whole.json')
            wr=self.write(root/'whole-recipe',whole)
            ip=self.write(root/'whole-input',dict(protocol='oo.source-onboarding/v1',connection=dict(type='mysql',database='demo',host='localhost',port=3306,user='reader',secret_ref='secret://test'),secret_registry=reg,recipe=wr,out=str(root/'whole-run')))
            with self.assertRaisesRegex(m.Refused,'shared_program_unpinned'):m.run(ip)
            with self.assertRaisesRegex(m.Refused,'capture_cannot_claim_whole'):m.install_engine('mysql',rp,reg)
            proc=subprocess.run([sys.executable,str(Path(__file__).with_name('register_engine.py')),'--engine','mysql','--recipe',rp,'--secret-registry',reg],capture_output=True,text=True)
            self.assertEqual(proc.returncode,2);self.assertNotIn('private',proc.stderr)
    def test_registration_accepts_only_bound_whole_proof(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);root.chmod(0o700);run=root/'run';run.mkdir(mode=0o700)
            product=run/'product';product.mkdir();oos=product/'oos';oos.mkdir();dialogue=run/'dialogue';dialogue.mkdir()
            catalog=b'{"tables":[]}\n';(product/'catalog.json').write_bytes(catalog)
            effective=b'{"protocol":"oo.builder.effective-oos/v1","datasets":[]}\n';(product/'effective.json').write_bytes(effective)
            (oos/'ontology.yaml').write_text('{"name":"fixture"}\n');(oos/'effective-oos.yaml').write_text('fixture: true\n')
            (oos/'source.json').write_text(json.dumps({'actual_core_effective_sha256':m.digest(product/'effective.json'),'publication':'not_claimed'}))
            receipt={'protocol':'oo.source-onboarding.product/v1','fixture_review':True,'source_fresh':True,'catalog_sha256':m.digest(product/'catalog.json'),'producer_sha256':m.digest(m.PRODUCT_FLOW),'effective_oos_sha256':m.digest(product/'effective.json'),'stages':[{'stage':f'S{i}','status':'succeeded'} for i in range(11)],'s5':{'outcome':'validated','builder_spawned':True,'structural_draft':False,'cache_hit':False,'llm_calls':1,'model':'gpt-6.1-sol','input_sha256':'ab'*32,'evidence_hash':m.digest(product/'catalog.json')},'standard_oos':{name:m.digest(oos/name) for name in ('ontology.yaml','effective-oos.yaml','source.json')}}
            (product/'product-receipt.json').write_text(json.dumps(receipt))
            (dialogue/'verdict.json').write_text(json.dumps({'model':'gpt-6.1-sol','reasoning_effort':'low','turns':2,'tools':['describe','freshness','knowledge_cat','query'],'query_ids':['qid-1'],'audit_sha256':'cd'*32,'truth_sha256':'ef'*32,'outcome':'verified'}))
            (dialogue/'transcript.json').write_text(json.dumps([{'tool':'query','query_id':'qid-1'}]))
            whole={'full_product_chain_complete':True,'periodic_insert_update_delete_verified':True,'recovery_verified':True,'product_receipt':'product/product-receipt.json','dialogue_verdict':'dialogue/verdict.json'}
            (run/'whole.json').write_text(json.dumps(whole))
            secret=root/'secret';secret.write_text('private');secret.chmod(0o600)
            template=self.write(root/'template',dict(host='localhost',port=3306,database='demo',user='reader',password_file=''))
            pins=[dict(path=p,sha256=m.digest(p)) for p in (sys.executable,str(m.PRODUCT_FLOW),str(m.DIALOGUE))]
            recipe=dict(protocol='oo.source-onboarding.recipe/v1',engine='mysql',owned_fixture=True,mode='whole_source',completion_artifact='whole.json',config_template=template,connection_map={f'/{k}':k for k in ('host','port','database','user','password_file')},pins=pins,steps=[dict(name='product',argv=[sys.executable,str(m.PRODUCT_FLOW)]),dict(name='dialogue',argv=[sys.executable,str(m.DIALOGUE)])])
            rp=self.write(root/'recipe',recipe);reg=self.write(root/'registry',{'secret://test':str(secret)})
            artifacts={'product/product-receipt.json':m.digest(product/'product-receipt.json'),'dialogue/verdict.json':m.digest(dialogue/'verdict.json'),'dialogue/transcript.json':m.digest(dialogue/'transcript.json')}
            journal={'state':'recipe_completed','identity':{'recipe_sha256':m.digest(rp)},'steps':[{'name':'product','state':'succeeded','artifacts':[{'path':'product/product-receipt.json','sha256':artifacts['product/product-receipt.json']}]},{'name':'dialogue','state':'succeeded','artifacts':[{'path':'dialogue/verdict.json','sha256':artifacts['dialogue/verdict.json']},{'path':'dialogue/transcript.json','sha256':artifacts['dialogue/transcript.json']}]}]}
            self.write(run/'journal.json',journal)
            self.assertTrue(m.prove_whole(run,recipe,journal)['full_product_chain_complete'])
            receipt['s5']['structural_draft']=True;(product/'product-receipt.json').write_text(json.dumps(receipt))
            journal['steps'][0]['artifacts'][0]['sha256']=m.digest(product/'product-receipt.json');self.write(run/'journal.json',journal)
            with self.assertRaisesRegex(m.Refused,'not_real_s5'):m.prove_whole(run,json.loads(Path(rp).read_text()),journal)
            receipt['s5']['structural_draft']=False;(product/'product-receipt.json').write_text(json.dumps(receipt))
            journal['steps'][0]['artifacts'][0]['sha256']=m.digest(product/'product-receipt.json');self.write(run/'journal.json',journal)
            old=m.INSTALL_ROOT;m.INSTALL_ROOT=root/'install'
            try:
                record=m.install_engine('mysql',rp,reg,whole_run=str(run))
                self.assertTrue(record['whole'])
                meta=dict(recipe,engine='postgresql',mode='metadata_sample',owned_fixture=False,read_only=True,max_rows=5);meta.pop('completion_artifact')
                mp=self.write(root/'meta',meta)
                self.assertFalse(m.install_engine('postgresql',mp,reg)['whole'])
                with self.assertRaisesRegex(m.Refused,'actual_whole_required'):m.install_engine('mysql',rp,reg)
            finally:m.INSTALL_ROOT=old
    def test_duplicate_keys(self):
        with self.assertRaisesRegex(m.Refused,'duplicate_key'):json.loads('{"a":1,"a":2}',object_pairs_hook=m.pairs)
    def test_inline_secret(self):
        with self.assertRaisesRegex(m.Refused,'inline_secret'):m.reject_secrets({'connection':{'password':'bad'}})

if __name__=='__main__':unittest.main()
