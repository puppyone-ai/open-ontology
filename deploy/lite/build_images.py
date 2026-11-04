#!/usr/bin/env python3
"""Build serially, retain exact image IDs, install only a local digest pointer."""
import argparse
import hashlib
import json
import platform
from pathlib import Path
import re
import shutil
import subprocess

ROOT=Path(__file__).resolve().parents[2]

def main():
    p=argparse.ArgumentParser();p.add_argument('--run-id',required=True)
    p.add_argument('--platforms',default='linux/arm64,linux/amd64');a=p.parse_args()
    if not re.fullmatch('[a-z0-9-]+',a.run_id):p.error('invalid run id')
    platforms=a.platforms.split(',')
    if not platforms or any(v not in ['linux/arm64','linux/amd64'] for v in platforms):p.error('unsupported platform')
    out=ROOT/'evidence'/a.run_id;out.mkdir()
    images={}
    for target in platforms:
        if shutil.disk_usage(ROOT).free < 8*1024**3:raise RuntimeError('less than 8 GiB free; refusing build')
        arch=target.split('/')[1];tag='oo-lane-a-lite:'+a.run_id+'-'+arch
        if subprocess.run(['docker','image','inspect',tag],capture_output=True).returncode==0:
            raise RuntimeError('refusing to replace an existing image tag')
        iid=out/(arch+'.id')
        with (out/(arch+'.log')).open('wb') as log:
            subprocess.run(['docker','build','--platform',target,'--label','oo.lane=a',
                '--iidfile',str(iid),'-f','deploy/lite/Dockerfile','-t',tag,'.'],cwd=ROOT,stdout=log,stderr=subprocess.STDOUT,check=True)
        image=iid.read_text().strip()
        info=json.loads(subprocess.check_output(['docker','image','inspect',image]))[0]
        assert info['Config']['Labels']['oo.lane']=='a'
        assert info['Os']+'/'+info['Architecture']==target
        with (out/(arch+'-oo-load-help.log')).open('wb') as log:
            subprocess.run(['docker','run','--rm','--platform',target,
                '--label','oo.lane=a','--entrypoint','oo-load',image,'--help'],
                stdout=log,stderr=subprocess.STDOUT,check=True)
        images[target]={'image':image,'tag':tag,'dockerfile_sha256':hashlib.sha256((ROOT/'deploy/lite/Dockerfile').read_bytes()).hexdigest()}
        (out/'images.json').write_text(json.dumps(images,indent=2)+'\n')
        native='linux/'+('arm64' if platform.machine() in ['arm64','aarch64'] else 'amd64')
        if target==native:
            (ROOT/'target/debug').mkdir(parents=True,exist_ok=True)
            (ROOT/'target/debug/oo-lite-image').write_text(image+'\n')
        print(target,image,flush=True)
    return 0

if __name__=='__main__':raise SystemExit(main())
