#!/usr/bin/env python3
"""Cancel an actual fresh Pi process, then capture close and released refusal."""
import pathlib, subprocess, time, json, sys
root=pathlib.Path(__file__).resolve().parents[2];out=root/'evidence/gpt-minimal-closure'/(sys.argv[1] if len(sys.argv)>1 else 'cancel-live-3');out.mkdir(exist_ok=True)
base=['docker','exec','oo-a419-product-run','/opt/oo/bin/minimal-product-loop']
config=['--config','/var/lib/oo/config/product.json']
with (out/'ask.stdout').open('wb') as stdout,(out/'ask.stderr').open('wb') as stderr:
    ask=subprocess.Popen(base+['ask']+config+['--question','How many rows are in public.tracking_events? Write the SQL yourself and run it with oo query.'],stdout=stdout,stderr=stderr)
    deadline=time.monotonic()+15
    while time.monotonic()<deadline:
        state=subprocess.run(['docker','exec','oo-a419-product-run','python3','-c','import json;print(json.load(open("/var/lib/oo/work/state.json")).get("pi_pid"))'],capture_output=True,check=True)
        if state.stdout.strip()!=b'None':break
        if ask.poll() is not None:raise SystemExit('ask finished before live cancel')
        time.sleep(.2)
    else: raise SystemExit('no live Pi pid within deadline')
    cancel=subprocess.run(base+['cancel']+config,capture_output=True)
    for suffix,value in [('stdout',cancel.stdout),('stderr',cancel.stderr),('exit',(str(cancel.returncode)+'\n').encode())]: (out/f'cancel.{suffix}').write_bytes(value)
    result=ask.wait(timeout=150);(out/'ask.exit').write_text(str(result)+'\n')
    print('cancel',cancel.returncode,'ask',result,flush=True)
    if cancel.returncode or result==0:raise SystemExit(1)
for command in ['close','ask']:
    args=base+[command]+config
    if command=='ask':args+=['--question','How many rows are in public.tracking_events?']
    r=subprocess.run(args,capture_output=True)
    for suffix,value in [('stdout',r.stdout),('stderr',r.stderr),('exit',(str(r.returncode)+'\n').encode())]:(out/f'{command}-after.{suffix}').write_bytes(value)
    print(command,r.returncode,flush=True)
