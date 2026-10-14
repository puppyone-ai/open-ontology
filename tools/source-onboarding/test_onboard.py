import importlib.util
import json
from pathlib import Path
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
            template=self.write(root/'template',dict(host='',port=1,database='',user='',password_file=''))
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
    def test_duplicate_keys(self):
        with self.assertRaisesRegex(m.Refused,'duplicate_key'):json.loads('{"a":1,"a":2}',object_pairs_hook=m.pairs)
    def test_inline_secret(self):
        with self.assertRaisesRegex(m.Refused,'inline_secret'):m.reject_secrets({'connection':{'password':'bad'}})

if __name__=='__main__':unittest.main()
