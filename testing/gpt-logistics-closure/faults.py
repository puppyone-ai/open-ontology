#!/usr/bin/env python3
"""Create isolated configs with file refs only and retain actual product exits."""
import pathlib, subprocess
root=pathlib.Path(__file__).resolve().parents[2]
out=root/'evidence/gpt-minimal-closure/faults'
setup='''import json,pathlib,os
base=pathlib.Path('/var/lib/oo/config/product.json')
d=json.loads(base.read_text())
p=pathlib.Path('/var/lib/oo/fault-bad-connect');p.mkdir(exist_ok=True)
d['database']['host']='127.0.0.1';d['database']['port']=1
d['paths']['work']=str(p/'work');d['paths']['knowledge']=str(p/'knowledge')
(p/'config.json').write_text(json.dumps(d))
'''
subprocess.run(['docker','exec','oo-a419-product-run','python3','-c',setup],check=True)
r=subprocess.run(['docker','exec','oo-a419-product-run','/opt/oo/bin/minimal-product-loop','run','--config','/var/lib/oo/fault-bad-connect/config.json'],capture_output=True)
(out/'bad-connection.stdout').write_bytes(r.stdout);(out/'bad-connection.stderr').write_bytes(r.stderr);(out/'bad-connection.exit').write_text(str(r.returncode)+'\n')
if r.returncode==0: raise SystemExit('Bad connection was accepted')
print('bad_connection_exit',r.returncode)
