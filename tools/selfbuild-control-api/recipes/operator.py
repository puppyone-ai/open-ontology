#!/usr/bin/env python3
"""Call actual authenticated Selfbuild. Never synthesizes authority or stage results."""
import argparse,json,pathlib,ssl,urllib.request,urllib.error,stat,os

def private(path):
    p=pathlib.Path(path);s=p.lstat()
    if not stat.S_ISREG(s.st_mode) or s.st_mode & 0o077 or s.st_uid!=os.getuid(): raise ValueError('private owner-only regular credential required')
    return p.read_text().strip()

p=argparse.ArgumentParser();p.add_argument('--endpoint',required=True);p.add_argument('--origin',required=True);p.add_argument('--ca',required=True);p.add_argument('--cookie-file',required=True);p.add_argument('--csrf-file');p.add_argument('--identity-origin');p.add_argument('--identity-ca');p.add_argument('--path',required=True);p.add_argument('--body-file');p.add_argument('--out',required=True);a=p.parse_args()
if not a.path.startswith('/control/v1/') or '\n' in a.path: raise ValueError('public control API path required')
headers={'Cookie':'__Host-oo-session='+private(a.cookie_file),'Origin':a.origin,'Accept':'application/json'}
body=None
if a.body_file:
    body=pathlib.Path(a.body_file).read_bytes();json.loads(body);headers['Content-Type']='application/json'
    if a.identity_origin:
        current=urllib.request.Request(a.identity_origin.rstrip('/')+'/control/v1/identity/current',headers={'Cookie':headers['Cookie']})
        with urllib.request.urlopen(current,context=ssl.create_default_context(cafile=a.identity_ca),timeout=30) as response:
            actual=json.load(response);headers['X-OO-CSRF']=actual['csrf_token']
    else: headers['X-OO-CSRF']=private(a.csrf_file)
req=urllib.request.Request(a.endpoint.rstrip('/')+a.path,data=body,headers=headers)
try:
    with urllib.request.urlopen(req,context=ssl.create_default_context(cafile=a.ca),timeout=30) as response: status=response.status;raw=response.read()
except urllib.error.HTTPError as error: status=error.code;raw=error.read()
out=pathlib.Path(a.out)
if out.exists(): raise ValueError('refusing to overwrite actual evidence')
out.write_text(json.dumps({'status':status,'request':json.loads(body) if body else None,'response':json.loads(raw)},indent=2)+'\n')
print('actual_http_status',status)
raise SystemExit(0 if status<400 else 1)
