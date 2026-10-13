#!/usr/bin/env python3
"""Real MariaDB C5 producer for the co-located native scheduler.

Run in the Linux runtime with pinned driver/admission ELFs and private reader
file mounts. stdout is the native update request; no jobs claim or target writes.
"""
import argparse,fcntl,hashlib,json,os,shutil,subprocess,sys,uuid
from pathlib import Path


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def write(path,value):
    Path(path).write_text(json.dumps(value,indent=2)+'\n')


def call(argv,out):
    result=subprocess.run(argv,capture_output=True)
    Path(str(out)+'.stdout').write_bytes(result.stdout)
    Path(str(out)+'.stderr').write_bytes(result.stderr)
    Path(str(out)+'.exit').write_text(str(result.returncode)+'\n')
    if result.returncode:
        raise RuntimeError('source stage refused; see '+str(out))


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--config',required=True)
    cfg=json.loads(Path(parser.parse_args().config).read_text())
    for name in ['driver','admission']:
        if digest(cfg[name]['path'])!=cfg[name]['sha256']:
            raise RuntimeError(name+' identity mismatch')
    incoming=sys.stdin.read()
    occurrence=json.loads(incoming) if incoming.strip() else None
    if occurrence and (occurrence.get('protocol')!='oo.jobs.native-producer/v1' or occurrence.get('source_id')!=cfg['source_request']['source_id'] or occurrence.get('method')!='C5'):
        raise RuntimeError('occurrence identity mismatch')
    pair=(str(uuid.UUID(occurrence['binding']))+'_'+str(int(occurrence['pin']))) if occurrence else None
    if occurrence and int(occurrence['pin'])<1:raise RuntimeError('occurrence pin')
    root=Path(cfg['state_dir']);root.mkdir(parents=True,exist_ok=True)
    with (root/'producer.lock').open('a') as lock:
        fcntl.flock(lock,fcntl.LOCK_EX)
        cache=root/'occurrences'/(pair+'.json') if pair else None
        if cache and cache.exists():
            sys.stdout.write(cache.read_text());return
        previous=json.loads((root/'state.json').read_text()) if (root/'state.json').exists() else None
        acknowledged=occurrence.get('acknowledged_source_generation') if occurrence else None
        if occurrence:
            if acknowledged is not None and (not isinstance(acknowledged,int) or acknowledged<1):
                raise RuntimeError('acknowledged generation invalid')
            if previous and previous['generation'] != acknowledged:
                raise RuntimeError('previous source capture has no matching worker ACK')
            generation=(acknowledged or 0)+1
        else:
            generation=previous['generation']+1 if previous else 1
        # The sealed catalog compiler accepts a UUID plus source generation.
        # This token is produced before capture and sealed by admission.
        run=f'{uuid.uuid4()}_{generation}'
        out=root/run;out.mkdir()
        req=dict(cfg['source_request'],source_generation=generation,run_id=run,label=run,out_dir=str(out/'capture'),driver_path=cfg['driver']['path'])
        write(out/'capture-request.json',req)
        call([cfg['driver']['path'],'product-witness',str(out/'capture-request.json')],out/'driver')
        capture=out/'capture';native=json.loads((capture/'native-update.json').read_text())
        limit=int(cfg.get('max_arrow_bytes',16777216))
        if not 0<limit<=268435456 or Path(native['arrow_path']).stat().st_size>limit:
            raise RuntimeError('configured Arrow byte budget exceeded')
        native['max_arrow_bytes']=limit
        call([cfg['admission']['path'],'seal-native','--report',str(capture/'report.json'),'--config',str(capture/'config.json'),'--out',str(out/'admission')],out/'admission-call')
        native['target']=cfg['target']
        native['admission_path']=str(out/'admission'/'admission-receipt.json')
        write(out/'native-request.json',native)
        state={'generation':generation,'input_digest':native['input_digest'],'schema_digest':native['schema_digest'],'request_path':str(out/'native-request.json')}
        # Atomically publish source-local checkpoint only after real seal.
        write(root/'state.tmp',state);os.replace(root/'state.tmp',root/'state.json')
        if occurrence:
            template=cfg.get('registry_template')
            if not template:raise RuntimeError('registration output template absent')
            registered=Path(template.format(binding=occurrence['binding'],pin=occurrence['pin']))
            registered.parent.mkdir(parents=True,exist_ok=True)
            # Copy exact sealer registry bytes; no receipt/certificate is synthesized.
            registry=Path(native['admission_path']).parent/'registry.json'
            shutil.copyfile(registry,registered)
            cache.parent.mkdir(parents=True,exist_ok=True)
            cache.with_suffix('.tmp').write_text(json.dumps(native,separators=(',',':'))+'\n');os.replace(cache.with_suffix('.tmp'),cache)
        print(json.dumps(native,separators=(',',':')))


if __name__=='__main__':
    try:main()
    except Exception as error:
        print('mariadb producer: '+str(error),file=sys.stderr);sys.exit(1)
