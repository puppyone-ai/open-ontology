#!/usr/bin/env python3
"""Ordinary onboarding's private MySQL C1 capture stage; native code owns data."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys


def load(path):
    return json.loads(Path(path).read_text())


def save(path, value):
    path = Path(path)
    with path.open('w') as f:
        os.chmod(path, 0o600)
        json.dump(value, f, separators=(',', ':'))
        f.write('\n')


def run(config, out, phase="capture"):
    config, out = Path(config), Path(out)
    c = load(config)
    operator = c['onboarding']
    root = Path(operator['prepared_root'])
    root.mkdir(mode=0o700, parents=True, exist_ok=True)
    if root.is_symlink() or root.stat().st_mode & 0o077:
        raise ValueError('private_output')
    registry = load(operator['secret_registry'])
    if registry[c['secret_ref']] != c['password_file']:
        raise ValueError('secret_binding')
    private_root = Path(operator['host_secret_root'])
    password = Path(c['password_file'])
    if password.stat().st_mode & 0o077 or not password.is_relative_to(private_root):
        raise ValueError('private_secret')
    if phase in ('follow','serve'):
        from follow import Follow
        original=load(root/'driver-config.json')
        if any(original.get(key)!=c.get(key) for key in ('host','port','database','user','secret_ref')):
            raise ValueError('connection_drift')
        if phase=='follow':
            flow=Follow(c,out);generation,catalog,result=flow.maintain();native=flow.publish(generation,catalog,result);flow.gateway(generation,catalog,native)
        else:
            import time
            seconds=operator['serve_seconds']
            if not isinstance(seconds,int) or not 1<=seconds<=86400:
                raise ValueError('serve_seconds')
            identity=load(root/'prepared/identity.json')
            worker=[operator['worker'],'--config',operator['worker_config']]
            proc=subprocess.Popen(['docker','exec',operator['container'],*worker,'native-serve','--project',identity['project_id'],'--worker','mysql-follow','--seconds',str(seconds)],stdout=(out/'worker.stdout').open('wb'),stderr=(out/'worker.stderr').open('wb'))
            previous=operator['after_generation'];cuts=[]
            try:
                while True:
                    raw=subprocess.run(['docker','exec',operator['container'],*worker,'native-state','--binding',identity['binding_id']],capture_output=True,check=True)
                    state=json.loads(raw.stdout)
                    current=state.get('source_generation',0)
                    if state.get('applied') and current>previous:
                        flow=Follow(c,out/('generation-'+str(current)))
                        generation,catalog,result=flow.maintain();native=flow.publish(generation,catalog,result);gateway=flow.gateway(generation,catalog,native)
                        cuts.append({'source_generation':generation,'gateway_private':str(gateway),'ready_sha256':hashlib.sha256((flow.out/'gateway-ready.json').read_bytes()).hexdigest()})
                        previous=current
                    if proc.poll() is not None:
                        break
                    time.sleep(1)
            finally:
                code=proc.wait()
                (out/'worker.exit').write_text(str(code)+'\n')
            if code or not cuts:
                raise RuntimeError('no_complete_followup')
            save(out/'follow-receipt.json',{'protocol':'oo.mysql.product-follow/v1','source_id':identity['source_id'],'binding':identity['binding_id'],'cuts':cuts,'current_generation':previous,'worker_exit':code,'full_product_chain_complete':False,'agent_dialogue_pending':True})
        return
    native = dict(c)
    native['password_file'] = '/secrets/' + str(password.relative_to(private_root))
    prepared = root/'prepared'
    driver = root/'driver-config.json'
    if driver.exists():
        if load(driver) != native:
            raise ValueError('connection_drift')
    else:
        save(driver, native)
    number = len(list(root.glob('command-*.exit.json')))

    def call(label, argv):
        nonlocal number
        number += 1
        prefix = root/f'command-{number:04d}-{label}'
        result = subprocess.run(['docker', 'exec', operator['container'], *argv], capture_output=True)
        for suffix, data in [('stdout', result.stdout), ('stderr', result.stderr)]:
            p = prefix.with_suffix('.'+suffix)
            p.write_bytes(data)
            p.chmod(0o600)
        save(prefix.with_suffix('.exit.json'), {'exit': result.returncode})
        if result.returncode:
            raise RuntimeError('native_'+label)
        return result.stdout.decode()

    if not prepared.exists():
        call('prepare', [operator['producer'], 'c1-onboarding-prepare', '--config', str(driver), '--out', str(prepared)])
    worker = [operator['worker'], '--config', operator['worker_config']]
    identity = load(prepared/'identity.json')
    if phase == 'prepare':
        save(out/'identity.json', identity)
        save(out/'prepare-receipt.json', {'prepared_root':str(prepared),'source_id':identity['source_id'],'source_config_sha256':identity['source_config_sha256'],'bound':False,'full_product_chain_complete':False})
        return
    if phase != 'capture':
        raise ValueError('phase')
    if not (root/'put.json').exists():
        text = call('put', [*worker, 'native-put', '--manifest', str(prepared/'native-manifest.json')])
        save(root/'put.json', {'stdout': text})
    if not (root/'fire.json').exists():
        text = call('fire', [*worker, 'native-fire', '--project', identity['project_id']])
        save(root/'fire.json', {'stdout': text})
    state = json.loads(call('state', [*worker, 'native-state', '--binding', identity['binding_id']]))
    if not state.get('applied'):
        call('step', [*worker, 'native-step', '--project', identity['project_id'], '--worker', 'mysql-onboarding'])
        state = json.loads(call('state', [*worker, 'native-state', '--binding', identity['binding_id']]))
    if not state.get('applied') or state.get('load_state') != 'awaiting_publish':
        raise RuntimeError('capture_pending')
    receipt = {'protocol': 'oo.mysql.connection-capture/v1', 'state': 'admitted',
               'source_id': identity['source_id'], 'binding': identity['binding_id'],
               'project_id': identity['project_id'], 'native_state': state,
               'prepared_root': str(prepared), 'fresh_source_capture': True,
               'full_product_chain_complete': False,
               'source_config_sha256': hashlib.sha256((prepared/'source-config.json').read_bytes()).hexdigest(),
               'manifest_sha256': hashlib.sha256((prepared/'native-manifest.json').read_bytes()).hexdigest()}
    save(out/'capture-receipt.json', receipt)
    save(out/'identity.json', identity)
    print(json.dumps({'state': 'captured', 'source_id': identity['source_id'], 'full_product_chain_complete': False}))


if __name__ == '__main__':
    run(*sys.argv[1:])
