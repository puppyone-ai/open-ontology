#!/usr/bin/env python3
"""Ask unmodified natural questions through the product; keep raw exit receipts."""
import pathlib, subprocess, sys, json
root=pathlib.Path(__file__).resolve().parents[2]
out=root/'evidence/gpt-minimal-closure'/ (sys.argv[1] if len(sys.argv)>1 else 'six-rerun-2')
out.mkdir(parents=True,exist_ok=True)
questions=(root/'deploy/logistics-demo/questions.txt').read_text().splitlines()
for index, question in enumerate(q for q in questions if q.strip()):
    result=subprocess.run(['docker','exec','oo-a419-product-run','/opt/oo/bin/minimal-product-loop','ask','--config','/var/lib/oo/config/product.json','--question',question],capture_output=True)
    stem=out/f'{index+1:02}'
    stem.with_suffix('.stdout').write_bytes(result.stdout)
    stem.with_suffix('.stderr').write_bytes(result.stderr)
    stem.with_suffix('.exit').write_text(str(result.returncode)+'\n')
    if result.stdout:
        try:
            record_id=json.loads(result.stdout)['question_id']
            subprocess.run(['docker','cp',f'oo-a419-product-run:/var/lib/oo/work/asks/{record_id}.json',str(stem.with_suffix('.ask.json'))],check=True,stdout=subprocess.DEVNULL)
        except (ValueError,KeyError): pass
    print(index+1,result.returncode,flush=True)
    if result.returncode: raise SystemExit(result.returncode)
