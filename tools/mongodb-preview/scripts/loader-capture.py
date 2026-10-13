#!/usr/bin/python3
"""Byte-preserving operator capture of the pinned real loader; no result edits."""
import json,pathlib,re,subprocess,sys
root=pathlib.Path('/Users/redux/puppy-one/open-ontology/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/native-c5-20261012/native-loader-calls')
body=sys.stdin.buffer.read();request=json.loads(body);task=request.get('lease_id','none');generation=request.get('lease_generation',0)
if not re.fullmatch(r'[0-9a-f-]{36}|none',task) or not isinstance(generation,int):raise RuntimeError('lease capture identity')
out=root/task/str(generation);out.mkdir(parents=True,exist_ok=False);(out/'request.json').write_bytes(body)
result=subprocess.run(['/loader']+sys.argv[1:],input=body,capture_output=True)
(out/'stdout').write_bytes(result.stdout);(out/'stderr').write_bytes(result.stderr);(out/'exit').write_text(str(result.returncode)+'\n')
sys.stdout.buffer.write(result.stdout);sys.stderr.buffer.write(result.stderr);raise SystemExit(result.returncode)
