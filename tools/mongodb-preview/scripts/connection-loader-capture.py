#!/usr/bin/python3
"""Preserve actual loader invocation bytes and exits for a private owned run."""
import pathlib,subprocess,sys,uuid,os,json
p=pathlib.Path('/tmp/oo-mongodb-connection-whole/loader-calls')/str(uuid.uuid4());p.mkdir(parents=True);body=sys.stdin.buffer.read();(p/'environment.json').write_text(json.dumps({k:v for k,v in os.environ.items() if k.startswith('OO_NATIVE_')}));(p/'request.json').write_bytes(body);r=subprocess.run(['/tmp/oo-mongodb-connection-whole/bin/pipeline-current']+sys.argv[1:],input=body,capture_output=True);(p/'stdout').write_bytes(r.stdout);(p/'stderr').write_bytes(r.stderr);(p/'exit').write_text(str(r.returncode));sys.stdout.buffer.write(r.stdout);sys.stderr.buffer.write(r.stderr);sys.exit(r.returncode)
