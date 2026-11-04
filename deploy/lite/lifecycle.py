"""Owned compose lifecycle. Foundations are not full product-stack acceptance."""
import hashlib
import http.client
import ipaddress
import json
import os
from pathlib import Path
import re
import ssl
import stat
import subprocess
import sys
import urllib.parse


def private_dir(path):
    path.mkdir(mode=0o700,exist_ok=True)
    st=path.lstat()
    if not stat.S_ISDIR(st.st_mode) or st.st_uid!=os.getuid() or st.st_mode & 0o777 != 0o700:
        raise ValueError('LITE_PRIVATE_DIRECTORY')


def read_private(path):
    fd=os.open(path,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK)
    with os.fdopen(fd,'rb') as stream:
        st=os.fstat(stream.fileno())
        if not stat.S_ISREG(st.st_mode) or st.st_uid!=os.getuid() or st.st_nlink!=1 or st.st_mode & 0o777!=0o600 or st.st_size>1048576:
            raise ValueError('LITE_PRIVATE_FILE')
        return stream.read(1048577)


def write(path,data):
    tmp=path.with_suffix('.tmp-'+str(os.getpid()))
    fd=os.open(tmp,os.O_CREAT|os.O_EXCL|os.O_WRONLY|os.O_NOFOLLOW,0o600)
    with os.fdopen(fd,'wb') as stream:
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(tmp,path)


def docker(argv,env):
    p=subprocess.run(['docker',*argv],capture_output=True,env=env,timeout=180)
    if p.returncode:
        raise ValueError('LITE_DOCKER_FAILED')
    return p.stdout


def subnet(project,env):
    ids=docker(['network','ls','-q'],env).decode().split()
    used=[]
    if ids:
        raw=docker(['network','inspect','--format','{{json .IPAM.Config}}',*ids],env)
        for line in raw.decode().splitlines():
            for config in json.loads(line) or []:
                if config.get('Subnet'):used.append(ipaddress.ip_network(config['Subnet']))
    start=int(hashlib.sha256(project.encode()).hexdigest()[:4],16)%4096
    for offset in range(4096):
        n=(start+offset)%4096
        candidate=ipaddress.ip_network(f'10.{240+n//256}.{n%256}.0/24')
        if not any(candidate.overlaps(other) for other in used):
            return str(candidate)
    raise ValueError('LITE_SUBNET_UNAVAILABLE')


def health(root,stack):
    client=root/'health-client.json'
    if not client.exists():
        return {'status':'unmeasured','nearest_credential_expiry_ms':None}
    config=json.loads(read_private(client))
    url=urllib.parse.urlsplit(stack['gateway']['public_url'])
    if url.scheme!='https' or not url.hostname or url.username or url.password or url.query or url.fragment:
        raise ValueError('LITE_HEALTH_URL')
    # Health credentials must belong to this deployment, never arbitrary paths.
    for name in ('token','ca'):
        if not re.fullmatch(r'[a-zA-Z0-9][a-zA-Z0-9_.-]{0,127}',config[name]):
            raise ValueError('LITE_HEALTH_REFERENCE')
    token=read_private(root/'pki'/config['token']).decode().strip()
    ca=read_private(root/'pki'/config['ca']).decode()
    if not token or any(c.isspace() for c in token):
        raise ValueError('LITE_HEALTH_TOKEN')
    conn=http.client.HTTPSConnection(url.hostname,url.port or 443,
        context=ssl.create_default_context(cadata=ca),timeout=5)
    try:
        conn.request('GET',url.path.rstrip('/')+'/v1/health',headers={'Authorization':'Bearer '+token})
        response=conn.getresponse()
        if response.status!=200:
            raise ValueError('LITE_HEALTH_REFUSED')
        data=json.loads(response.read(65537))
        deadlines=[data[k].get('expires_at_ms') for k in ('token_expiry','internal_mtls_certificate_expiry')]
        if any(d is not None and (isinstance(d,bool) or not isinstance(d,int)) for d in deadlines):
            raise ValueError('LITE_HEALTH_PROTOCOL')
        measured=[d for d in deadlines if d is not None]
        safe={k:{'expires_at_ms':d} for k,d in zip(('token_expiry','internal_mtls_certificate_expiry'),deadlines)}
        return {'status':'measured','nearest_credential_expiry_ms':min(measured) if measured else None,
                'basis':'/v1/health',**safe}
    finally:
        conn.close()


def main(request):
    root=Path(request['root'])
    action=request['action']
    for p in (root.parent,root): private_dir(p)
    env={k:v for k,v in os.environ.items() if k in ('PATH','HOME','DOCKER_HOST','DOCKER_CONTEXT','DOCKER_CONFIG','TMPDIR')}
    project='oo-a-lite-'+hashlib.sha256(str(root.resolve()).encode()).hexdigest()[:16]
    owner=root/'owner.json'
    if action!='up' and not owner.exists():
        return {'status':'not_started','gateway_health':{'status':'unmeasured'},'services':[]}
    image=request['image'] if action=='up' else json.loads(read_private(owner))['image']
    if not re.fullmatch(r'(?:[a-zA-Z0-9./:_-]+@)?sha256:[0-9a-f]{64}',image):
        raise ValueError('LITE_IMAGE_DIGEST_REQUIRED')
    env.update(OO_LITE_STATE=str(root),OO_LITE_RUNTIME_IMAGE=image,OO_LITE_UID=str(os.getuid()),OO_LITE_GID=str(os.getgid()))
    saved=json.loads(read_private(owner)) if owner.exists() else {}
    network=saved.get('subnet')
    if not network:network=subnet(project,env)
    if not ipaddress.ip_network(network).is_private:raise ValueError('LITE_SUBNET')
    env['OO_LITE_SUBNET']=network
    argv=['compose','--project-name',project,'--file',str(root/'compose.yaml')]
    ids=docker(['ps','-aq','--filter','label=com.docker.compose.project='+project],env).decode().split()
    for cid in ids:
        labels=json.loads(docker(['inspect','--format','{{json .Config.Labels}}',cid],env))
        if labels.get('oo.lane')!='a': raise ValueError('LITE_OWNERSHIP')
    for kind in ('network','volume'):
        names=docker([kind,'ls','-q','--filter','label=com.docker.compose.project='+project],env).decode().split()
        for name in names:
            labels=json.loads(docker([kind,'inspect','--format','{{json .Labels}}',name],env))
            if labels.get('oo.lane')!='a':raise ValueError('LITE_OWNERSHIP')
    if action=='up':
        if owner.exists() and json.loads(read_private(owner))['image']!=image:
            raise ValueError('LITE_IMAGE_CHANGE_REQUIRES_EXPLICIT_MIGRATION')
        for name in ('work','home'):private_dir(root/name)
        for sub in ('home/.config','home/.config/open-ontology','home/.config/open-ontology/secrets'):
            private_dir(root/sub)
        refs=[request['stack']['model']['key']['secret_ref']]
        endpoint=request['stack']['model']['base_url']
        if isinstance(endpoint,dict):refs.append(endpoint['secret_ref'])
        for source in request['stack']['sources']:
            refs.append(source['password']['secret_ref'])
            if source.get('tls_ca'):refs.append(source['tls_ca']['secret_ref'])
        home=Path.home()/'.config/open-ontology/secrets'
        for name in set(refs):
            if not re.fullmatch(r'[a-zA-Z0-9][a-zA-Z0-9_.-]{0,127}',name):raise ValueError('SECRET_NAME')
            data=read_private(home/name)
            dest=root/'home/.config/open-ontology/secrets'/name
            if dest.exists():
                if read_private(dest)!=data:raise ValueError('LITE_SECRET_CHANGE_REQUIRES_EXPLICIT_ROTATION')
            else:write(dest,data)
        for name in ('compose.yaml','postgres.sh','scheduler.py'):
            dest=root/('work' if name=='scheduler.py' else '')/name
            write(dest,request['templates'][name].encode())
        write(root/'work/stack.json',json.dumps(request['stack']).encode())
        write(owner,json.dumps({'project':project,'image':image,'lane':'a','subnet':network}).encode())
        docker(argv+['up','--detach','--wait','--wait-timeout','120'],env)
    elif action=='down':
        # Keep database volumes and credentials. No prune, orphan or volume flags.
        docker(argv+['down'],env)
    elif action!='status':raise ValueError('LITE_ACTION')
    raw=docker(argv+['ps','--all','--format','json'],env).decode().strip()
    rows=json.loads(raw) if raw.startswith('[') else [json.loads(line) for line in raw.splitlines()]
    services=[{k:r.get(k) for k in ('Service','State','Health','ExitCode')} for r in rows]
    gateway={'status':'unmeasured','nearest_credential_expiry_ms':None}
    if action!='down':
        try:gateway=health(root,request['stack'])
        except Exception:gateway={'status':'unavailable','nearest_credential_expiry_ms':None}
    history={'last_success':None,'last_failure':None,'next_due':None}
    if (root.parent/'run.json').exists():
        build=json.loads(read_private(root.parent/'run.json'))
        history.update({k:build.get(k) for k in ('last_success','last_failure')})
    if (root/'work/scheduler-status.json').exists():
        schedule=json.loads(read_private(root/'work/scheduler-status.json'))
        history.update({k:schedule.get(k) for k in history})
    return {'status':'stopped' if action=='down' else 'foundations_only',
            'services':services,'gateway_health':gateway,**history,
            'schedule_basis':'not_integrated','pending':['worker admission/migrations','scheduler bindings',
                'Builder Agent DB load','reviewed Pack and Gateway operator config'],
            'publication':'not_published','healthy':False}

if __name__=='__main__':
    os.umask(0o077)
    try:
        report=main(json.loads(sys.stdin.buffer.read(2097153)))
        print(json.dumps(report))
        raise SystemExit(1 if report.get('status')=='foundations_only' else 0)
    except Exception:
        print(json.dumps({'status':'failed','error':'LITE_LIFECYCLE_FAILED','healthy':False}))
        raise SystemExit(1)
