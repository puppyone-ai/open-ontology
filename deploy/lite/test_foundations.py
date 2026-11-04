#!/usr/bin/env python3
"""Owned live compose test. Exit 1/foundations_only is required, not full acceptance."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import subprocess

ROOT=Path(__file__).resolve().parents[2]

def main():
    p=argparse.ArgumentParser();p.add_argument('--run-id',required=True);p.add_argument('--image',required=True);p.add_argument('--installed',action='store_true');a=p.parse_args()
    if not re.fullmatch('[a-z0-9-]+',a.run_id) or not re.fullmatch('sha256:[0-9a-f]{64}',a.image):p.error('invalid input')
    out=ROOT/'evidence'/a.run_id;out.mkdir(mode=0o700)
    home=out/'home';store=home/'.config/open-ontology/secrets';store.mkdir(parents=True,mode=0o700)
    password,key=(secrets.token_hex(24) for _ in range(2))
    for name,value in [('source',password),('model',key),('endpoint','http://127.0.0.1:9/v1')]:
        (store/name).touch(mode=0o600);(store/name).write_text(value)
    config={'version':'oo.stack/v1','sources':[{'id':'fixture','engine':'mysql','host':'localhost','port':3306,
            'database':'fixture','user':'reader','password':{'secret_ref':'source'},'scope':{'include':['fixture.events']}}],
            'model':{'provider':'openai-compatible','base_url':{'secret_ref':'endpoint'},'model':'fixture-model','key':{'secret_ref':'model'}}}
    path=out/'stack.json';path.write_text(json.dumps(config));path.chmod(0o600)
    env=dict(os.environ,HOME=str(home),OO_LITE_RUNTIME_IMAGE=a.image,
             DOCKER_CONFIG=os.environ.get('DOCKER_CONFIG',str(Path.home()/'.docker')))
    import shutil
    binary=shutil.which('docker')
    shim=out/'bin';shim.mkdir()
    tracer=shim/'docker'
    tracer.write_text('#!/usr/bin/env python3\nimport pathlib,subprocess,sys\n'
        'p=subprocess.run(['+repr(binary)+',*sys.argv[1:]],capture_output=True)\n'
        'with pathlib.Path('+repr(str(out/'docker-trace.log'))+').open("ab") as f: f.write(p.stdout+p.stderr)\n'
        'sys.stdout.buffer.write(p.stdout);sys.stderr.buffer.write(p.stderr);sys.exit(p.returncode)\n')
    tracer.chmod(0o700)
    env['PATH']=str(shim)+os.pathsep+env['PATH']
    if a.installed:
        assert (ROOT/'target/debug/oo-lite-image').read_text().strip()==a.image
        env.pop('OO_LITE_RUNTIME_IMAGE',None)
    project='oo-a-lite-'+hashlib.sha256(str(out/'.oo/lite').encode()).hexdigest()[:16]
    def command(action,case,expected):
        proc=subprocess.run([str(ROOT/'target/debug/oo'),action,'--config',str(path),'--json'],env=env,capture_output=True,timeout=300)
        raw=proc.stdout+proc.stderr
        assert all(s.encode() not in raw for s in (password,key))
        (out/(case+'.json')).write_bytes(raw);(out/(case+'.exit')).write_text(str(proc.returncode))
        assert proc.returncode==expected,case
        return json.loads(proc.stdout)
    try:
        up=command('up','up',1)
        assert up['status']=='foundations_only' and not up['healthy']
        assert {r['Service'] for r in up['services']}=={'agent-db','control-db','model-egress'}
        assert all(r['Health']=='healthy' for r in up['services'])
        pki=out/'.oo/lite/pki'
        before={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in pki.iterdir()}
        again=command('up','up-again',1)
        assert before=={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in pki.iterdir()}
        status=command('status','status',1)
        assert status['gateway_health']['status']=='unmeasured'
        assert status['bootstrap_credentials']['token_expiry_measured'] is False
        assert status['bootstrap_credentials']['nearest_credential_expiry']
        assert status['last_success'] is None and status['next_due'] is None
        down=command('down','down',0)
        assert down['status']=='stopped' and down['services']==[]
        reports=b''.join((out/(name+'.json')).read_bytes() for name in ('up','up-again','status','down'))
        for secret in pki.iterdir():
            if secret.name.endswith('-key') or secret.name=='lite-admin-password':
                assert secret.read_bytes() not in reports
        (out/'result.json').write_text(json.dumps({'result':'passed','scope':'three-service foundations ONLY',
            'full_stack_acceptance':False,'installed_pointer':a.installed,'healthy_foundation_services':3,'credential_reuse':'unchanged',
            'gateway_health':'unmeasured','containers_left':0},indent=2))
        return 0
    finally:
        ids=subprocess.check_output(['docker','ps','-aq','--filter','label=com.docker.compose.project='+project],text=True).split()
        logs=[]
        for cid in ids:
            labels=json.loads(subprocess.check_output(['docker','inspect','--format','{{json .Config.Labels}}',cid]))
            assert labels.get('oo.lane')=='a'
            captured=subprocess.run(['docker','logs',cid],capture_output=True)
            raw=captured.stdout+captured.stderr
            for value in (password,key):raw=raw.replace(value.encode(),b'[REDACTED]')
            logs.append({'service':labels.get('com.docker.compose.service'),'logs':raw.decode(errors='replace')})
        (out/'service-logs.json').write_text(json.dumps(logs))
        subprocess.run([str(ROOT/'target/debug/oo'),'down','--config',str(path),'--json'],env=env,capture_output=True,timeout=180)
        # The test removes only its labelled, generated database volumes.
        for kind in ['container','volume','network']:
            args=['docker','ps','-aq'] if kind=='container' else ['docker',kind,'ls','-q']
            names=subprocess.check_output(args+['--filter','label=com.docker.compose.project='+project],text=True).split()
            for name in names:
                labels=json.loads(subprocess.check_output(['docker',kind,'inspect','--format',
                      '{{json .Config.Labels}}' if kind=='container' else '{{json .Labels}}',name]))
                assert labels.get('oo.lane')=='a'
                assert labels.get('com.docker.compose.project')==project
                subprocess.run(['docker',kind,'rm',*(['-f'] if kind=='container' else []),name],capture_output=True,check=True)
        # Remove only credentials generated by this test; preserve raw receipts.
        import shutil
        shutil.rmtree(home)
        for relative in ['.oo/lite/pki','.oo/lite/home']:
            directory=out/relative
            if directory.exists():shutil.rmtree(directory)

if __name__=='__main__':raise SystemExit(main())
