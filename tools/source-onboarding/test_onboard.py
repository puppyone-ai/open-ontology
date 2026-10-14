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
            receipt={'protocol':'oo.source-onboarding.product/v1','fixture_review':True,'source_fresh':True,'catalog_sha256':m.digest(product/'catalog.json'),'producer':'tools/source-onboarding/product_flow.py','producer_sha256':m.digest(m.PRODUCT_FLOW),'effective_oos_sha256':m.digest(product/'effective.json'),'stages':[{'stage':f'S{i}','status':'succeeded'} for i in range(11)],'s5':{'outcome':'validated','builder_spawned':True,'structural_draft':False,'cache_hit':False,'llm_calls':1,'model':'gpt-6.1-sol','input_sha256':'ab'*32,'evidence_hash':m.digest(product/'catalog.json')},'standard_oos':{name:m.digest(oos/name) for name in ('ontology.yaml','effective-oos.yaml','source.json')}}
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
            observed=json.loads(json.dumps(journal))
            observed['steps'][0]['observed_programs']=[str(m.PRODUCT_FLOW)]
            observed['steps'][1]['observed_programs']=[str(m.DIALOGUE)]
            wrapped=json.loads(json.dumps(recipe))
            wrapped['steps'][0]['argv']=[sys.executable,str(root/'driver.py')]
            wrapped['steps'][1]['argv']=[sys.executable,str(root/'driver.py')]
            self.assertTrue(m.prove_whole(run,wrapped,observed)['full_product_chain_complete'])
            observed['steps'][0]['observed_programs']=[]
            with self.assertRaisesRegex(m.Refused,'historical_completion_attached'):m.prove_whole(run,wrapped,observed)
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
                pending=m.install_engine('mysql',rp,reg)
                self.assertFalse(pending['whole']);self.assertTrue(pending['awaiting_run'])
            finally:m.INSTALL_ROOT=old
    def test_selected_engine_freeze_ignores_unrelated_registry_entries(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);old=m.INSTALL_ROOT;m.INSTALL_ROOT=root/'install';m.INSTALL_ROOT.mkdir(mode=0o700)
            try:
                secret=root/'secret';secret.write_text('private');secret.chmod(0o600)
                other=root/'other';other.write_text('other');other.chmod(0o600)
                template=self.write(root/'template',dict(host='localhost',port=3306,database='demo',user='reader',password_file=''))
                script=root/'driver.py';script.write_text("import pathlib,sys\npathlib.Path(sys.argv[1]).write_text('ok')\n")
                recipe=dict(protocol='oo.source-onboarding.recipe/v1',engine='mysql',owned_fixture=False,mode='metadata_sample',read_only=True,max_rows=5,config_template=template,connection_map={f'/{k}':k for k in ('host','port','database','user','password_file')},pins=[dict(path=sys.executable,sha256=m.digest(sys.executable)),dict(path=str(script),sha256=m.digest(script))],steps=[dict(name='probe',argv=[sys.executable,str(script),'{run}/receipt.json'],artifacts=[dict(path='receipt.json')])])
                rp=self.write(root/'recipe',recipe);reg=self.write(root/'registry',{'secret://test':str(secret)})
                same=self.write(root/'registry-same',{'secret://test':str(secret)})
                engines={'protocol':'oo.source-onboarding.engines/v1','engines':{'mysql':dict(recipe=rp,recipe_sha256=m.digest(rp),secret_registry=reg,whole=False,awaiting_run=True)}}
                registry=m.INSTALL_ROOT/'engines.json';self.write(registry,engines)
                ip=self.write(root/'connection.json',dict(type='mysql',database='demo',host='localhost',port=3306,user='reader',secret_ref='secret://test'))
                self.assertEqual(m.run(ip)['state'],'recipe_completed')
                pointer=json.loads((root/'connection.json.run.json').read_text());run=Path(pointer['out']);journal_path=run/'journal.json'
                journal=json.loads(journal_path.read_text());frozen=journal['identity']['engine_registry_sha256'];selected=journal['identity']['selected_engine_sha256']
                engines['engines']['oracle']=dict(recipe=rp,recipe_sha256=m.digest(rp),secret_registry=reg,whole=False)
                self.write(registry,engines)
                self.assertNotEqual(m.digest(registry),frozen)
                self.assertEqual(m.run(ip,True)['state'],'recipe_completed')
                kept=json.loads(journal_path.read_text())
                self.assertEqual(kept['identity']['engine_registry_sha256'],frozen)
                self.assertEqual(kept['identity']['selected_engine_sha256'],selected)
                engines['engines']['mysql']['whole']=True;engines['engines']['mysql']['awaiting_run']=False;self.write(registry,engines)
                self.assertEqual(m.run(ip,True)['state'],'recipe_completed')
                engines['engines']['mysql']['secret_registry']=same;self.write(registry,engines)
                with self.assertRaisesRegex(m.Refused,'resume_drift'):m.run(ip,True)
                engines['engines']['mysql']['secret_registry']=reg;engines['engines']['mysql']['whole']=False;engines['engines']['mysql']['awaiting_run']=True;self.write(registry,engines)
                legacy=json.loads(journal_path.read_text());legacy['identity'].pop('selected_engine_sha256');journal_path.write_text(json.dumps(legacy))
                engines['engines']['clickhouse']=dict(recipe=rp,recipe_sha256=m.digest(rp),secret_registry=reg)
                self.write(registry,engines)
                self.assertEqual(m.run(ip,True)['state'],'recipe_completed')
                migrated=json.loads(journal_path.read_text())
                self.assertNotIn('selected_engine_sha256',migrated['identity'])
                self.assertEqual(migrated['identity']['engine_registry_sha256'],frozen)
                moved=self.write(root/'registry-moved',{'secret://test':str(other)})
                engines['engines']['mysql']['secret_registry']=moved;self.write(registry,engines)
                with self.assertRaisesRegex(m.Refused,'resume_drift'):m.run(ip,True)
                engines['engines']['mysql']['secret_registry']=reg;self.write(registry,engines)
                recipe['max_rows']=4;self.write(root/'recipe',recipe)
                engines['engines']['mysql']['recipe_sha256']=m.digest(rp);self.write(registry,engines)
                with self.assertRaisesRegex(m.Refused,'resume_drift'):m.run(ip,True)
                recipe['max_rows']=5;self.write(root/'recipe',recipe);engines['engines']['mysql']['recipe_sha256']=m.digest(rp);self.write(registry,engines)
                conn=json.loads((root/'connection.json').read_text());conn['port']=3307;self.write(root/'connection.json',conn)
                with self.assertRaisesRegex(m.Refused,'resume_drift'):m.run(ip,True)
            finally:m.INSTALL_ROOT=old
    def test_completed_product_reuses_same_config(self):
        import importlib.util
        sys.path.insert(0,str(Path(__file__).parent))
        spec=importlib.util.spec_from_file_location('product_flow',Path(__file__).with_name('product_flow.py'))
        pf=importlib.util.module_from_spec(spec);spec.loader.exec_module(pf)
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);root.chmod(0o700);out=root/'out';out.mkdir(mode=0o700)
            catalog='ab'*32
            config={'fixture_authorized':True,'operator_sha256':m.digest(pf.OPERATOR),'catalog_sha256':catalog,'pins':['pin-a']}
            cp=root/'config.json';cp.write_text(json.dumps(config));cp.chmod(0o600)
            c=m.load(cp);c['_sha256']=m.digest(cp)
            flow=pf.Flow(c,out);build='b_same'
            m.save(out/'build.json',{'build_id':build,'config_sha256':c['_sha256']})
            m.save(out/'product-receipt.json',{'protocol':'oo.source-onboarding.product/v1','build_id':build,'catalog_sha256':catalog,'fixture_review':True})
            result=flow.run();self.assertTrue(result['reused_completed']);self.assertEqual(result['build_id'],build)
            self.assertEqual(list(out.glob('*.response.json')),[])
            receipt=json.loads((out/'product-receipt.json').read_text());receipt['build_id']='b_other';(out/'product-receipt.json').write_text(json.dumps(receipt))
            with self.assertRaisesRegex(pf.Refused,'product_resume_drift'):flow.run()
            self.assertTrue((out/'product-receipt.json').is_file())
            receipt['build_id']=build;(out/'product-receipt.json').write_text(json.dumps(receipt))
            config['pins']=['pin-b'];cp.write_text(json.dumps(config));cp.chmod(0o600)
            c2=m.load(cp);c2['_sha256']=m.digest(cp)
            with self.assertRaisesRegex(pf.Refused,'product_resume_drift'):pf.Flow(c2,out).run()
            self.assertTrue((out/'product-receipt.json').is_file());self.assertEqual(list(out.glob('*.response.json')),[])
    def test_reused_receipt_proves_whole_and_keeps_producer(self):
        import importlib.util
        spec=importlib.util.spec_from_file_location('product_flow',Path(__file__).with_name('product_flow.py'))
        pf=importlib.util.module_from_spec(spec);spec.loader.exec_module(pf)
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);root.chmod(0o700);run=root/'run';run.mkdir(mode=0o700)
            product=run/'product';product.mkdir(mode=0o700);oos=product/'oos';oos.mkdir();dialogue=run/'dialogue';dialogue.mkdir()
            (product/'catalog.json').write_bytes(b'{"tables":[]}\n')
            (product/'effective.json').write_bytes(b'{"protocol":"oo.builder.effective-oos/v1","datasets":[]}\n')
            (oos/'ontology.yaml').write_text('{"name":"fixture"}\n');(oos/'effective-oos.yaml').write_text('fixture: true\n')
            (oos/'source.json').write_text(json.dumps({'actual_core_effective_sha256':m.digest(product/'effective.json'),'publication':'not_claimed'}))
            catalog_sha=m.digest(product/'catalog.json');old_producer='b2'*32
            self.assertNotEqual(old_producer,m.digest(m.PRODUCT_FLOW))
            receipt={'protocol':'oo.source-onboarding.product/v1','build_id':'b_same','fixture_review':True,'source_fresh':True,'catalog_sha256':catalog_sha,'producer':'tools/source-onboarding/product_flow.py','producer_sha256':old_producer,'effective_oos_sha256':m.digest(product/'effective.json'),'stages':[{'stage':f'S{i}','status':'succeeded'} for i in range(11)],'s5':{'outcome':'validated','builder_spawned':True,'structural_draft':False,'cache_hit':True,'llm_calls':0,'model':'grok-4.7','input_sha256':'ab'*32,'evidence_hash':catalog_sha},'standard_oos':{name:m.digest(oos/name) for name in ('ontology.yaml','effective-oos.yaml','source.json')}}
            raw=json.dumps(receipt).encode();(product/'product-receipt.json').write_bytes(raw)
            (dialogue/'verdict.json').write_text(json.dumps({'model':'grok-4.7','reasoning_effort':'low','turns':2,'tools':['describe','freshness','knowledge_cat','query'],'query_ids':['qid-1'],'audit_sha256':'cd'*32,'truth_sha256':'ef'*32,'outcome':'verified'}))
            (dialogue/'transcript.json').write_text(json.dumps([{'tool':'query','query_id':'qid-1'}]))
            (run/'whole.json').write_text(json.dumps({'full_product_chain_complete':True,'periodic_insert_update_delete_verified':True,'recovery_verified':True,'product_receipt':'product/product-receipt.json','dialogue_verdict':'dialogue/verdict.json'}))
            config={'fixture_authorized':True,'operator_sha256':m.digest(pf.OPERATOR),'catalog_sha256':catalog_sha}
            cp=root/'config.json';cp.write_text(json.dumps(config));cp.chmod(0o600)
            c=m.load(cp);c['_sha256']=m.digest(cp)
            m.save(product/'build.json',{'build_id':'b_same','config_sha256':c['_sha256']})
            reused=pf.Flow(c,product).run()
            self.assertTrue(reused['reused_completed']);self.assertEqual(reused['build_id'],'b_same')
            self.assertEqual((product/'product-receipt.json').read_bytes(),raw)
            pins=[dict(path=p,sha256=m.digest(p)) for p in (sys.executable,str(m.PRODUCT_FLOW),str(m.DIALOGUE))]
            recipe=dict(protocol='oo.source-onboarding.recipe/v1',engine='mysql',owned_fixture=True,mode='whole_source',completion_artifact='whole.json',pins=pins,steps=[dict(name='product',argv=[sys.executable,str(root/'reuse.py')]),dict(name='dialogue',argv=[sys.executable,str(m.DIALOGUE)])])
            journal={'state':'failed','steps':[{'name':'product','state':'succeeded','observed_programs':[str(m.PRODUCT_FLOW)],'artifacts':[{'path':'product/product-receipt.json','sha256':m.digest(product/'product-receipt.json')}]},{'name':'dialogue','state':'succeeded','observed_programs':[str(m.DIALOGUE)],'artifacts':[{'path':'dialogue/verdict.json','sha256':m.digest(dialogue/'verdict.json')},{'path':'dialogue/transcript.json','sha256':m.digest(dialogue/'transcript.json')}]}]}
            self.assertTrue(m.prove_whole(run,recipe,journal)['full_product_chain_complete'])
            self.assertEqual((product/'product-receipt.json').read_bytes(),raw)
            bad_pins=json.loads(json.dumps(recipe));bad_pins['pins']=[dict(path=p,sha256='00'*32) for p in (sys.executable,str(m.PRODUCT_FLOW),str(m.DIALOGUE))]
            with self.assertRaisesRegex(m.Refused,'shared_program_unpinned'):m.prove_whole(run,bad_pins,journal)
            unrun=json.loads(json.dumps(journal));unrun['steps'][0]['observed_programs']=[];unrun['steps'][1]['observed_programs']=[]
            unrun_recipe=json.loads(json.dumps(recipe));unrun_recipe['steps'][1]['argv']=[sys.executable,str(root/'reuse.py')]
            with self.assertRaisesRegex(m.Refused,'historical_completion_attached'):m.prove_whole(run,unrun_recipe,unrun)
            copied=json.loads(raw);copied['producer_sha256']='ab'*32;(product/'product-receipt.json').write_bytes(json.dumps(copied).encode())
            with self.assertRaisesRegex(m.Refused,'historical_completion_attached'):m.prove_whole(run,recipe,journal)
            (product/'product-receipt.json').write_bytes(raw)
            foreign=json.loads(raw);foreign['producer']='tools/other.py';(product/'product-receipt.json').write_bytes(json.dumps(foreign).encode())
            forged=json.loads(json.dumps(journal));forged['steps'][0]['artifacts'][0]['sha256']=m.digest(product/'product-receipt.json')
            with self.assertRaisesRegex(m.Refused,'source_not_fresh'):m.prove_whole(run,recipe,forged)
            (product/'product-receipt.json').write_bytes(raw)
            config['catalog_sha256']='cd'*32;cp.write_text(json.dumps(config));cp.chmod(0o600)
            c2=m.load(cp);c2['_sha256']=m.digest(cp)
            with self.assertRaisesRegex(pf.Refused,'product_resume_drift'):pf.Flow(c2,product).run()
            self.assertEqual((product/'product-receipt.json').read_bytes(),raw)
            (product/'catalog.json').write_bytes(b'{"tables":[1]}\n')
            with self.assertRaisesRegex(m.Refused,'source_not_fresh'):m.prove_whole(run,recipe,journal)
    def test_duplicate_keys(self):
        with self.assertRaisesRegex(m.Refused,'duplicate_key'):json.loads('{"a":1,"a":2}',object_pairs_hook=m.pairs)
    def test_inline_secret(self):
        with self.assertRaisesRegex(m.Refused,'inline_secret'):m.reject_secrets({'connection':{'password':'bad'}})

if __name__=='__main__':unittest.main()
