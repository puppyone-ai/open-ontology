"""Owned compose lifecycle. Foundations are not full product-stack acceptance."""
import hashlib
import fcntl
import http.client
import ipaddress
import json
import os
from pathlib import Path
import re
import secrets
import ssl
import stat
import subprocess
import sys
import urllib.parse
import uuid

def control_status(root,argv,env):
    empty={'last_success':None,'last_failure':None,'next_due':None}
    path=root/'work/operator/schedule.json'
    if not path.exists():
        return {**empty,'schedule_basis':'control_db','schedule_status':'not_configured'}
    try:
        project=json.loads(read_private(path))['project']
        if str(uuid.UUID(project))!=project:
            raise ValueError('LITE_SCHEDULE_PROJECT')
        # The socket is inside this owned control container, never a source DB.
        # One read-only snapshot; enqueue/fire success is not job completion.
        sql=f"""
BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY;
SET LOCAL ROLE oo_jobs_runtime;
SET LOCAL statement_timeout = '5s';
SELECT json_build_object(
 'last_success', (SELECT to_char(max(e.occurred_at) AT TIME ZONE 'UTC',
   'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') FROM jobs.task_events e
   WHERE e.project_id = '{project}'::uuid AND e.to_state = 'succeeded'),
 'last_failure', (SELECT to_char(max(e.occurred_at) AT TIME ZONE 'UTC',
   'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') FROM jobs.task_events e
   WHERE e.project_id = '{project}'::uuid
     AND (e.to_state = 'failed' OR
          (e.from_state = 'running' AND e.to_state = 'queued' AND e.note IS NOT NULL))),
 'next_due', (SELECT to_char(min(next_run_at) AT TIME ZONE 'UTC',
   'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') FROM jobs.schedules
   WHERE project_id = '{project}'::uuid AND enabled));
COMMIT;
"""
        raw=docker(argv+['exec','-T','control-db','psql','-X','-qAt',
                       '-v','ON_ERROR_STOP=1','-U','postgres','-d','postgres','-c',sql],env)
        data=json.loads(raw)
        if set(data)!=set(empty):
            raise ValueError('LITE_CONTROL_PROTOCOL')
        for value in data.values():
            if value is not None and (not isinstance(value,str) or not re.fullmatch(
                    r'\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z',value)):
                raise ValueError('LITE_CONTROL_PROTOCOL')
        return {**data,'schedule_basis':'control_db','schedule_status':'measured'}
    except Exception:
        return {**empty,'schedule_basis':'control_db','schedule_status':'unavailable'}


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
    names=['token','ca']
    if 'cert' in config or 'key' in config:
        names+=['cert','key']
    for name in names:
        if not re.fullmatch(r'[a-zA-Z0-9][a-zA-Z0-9_.-]{0,127}',config[name]):
            raise ValueError('LITE_HEALTH_REFERENCE')
    token=read_private(root/'pki'/config['token']).decode().strip()
    ca=read_private(root/'pki'/config['ca']).decode()
    if not token or any(c.isspace() for c in token):
        raise ValueError('LITE_HEALTH_TOKEN')
    context=ssl.create_default_context(cadata=ca)
    if 'cert' in config:
        for name in ('cert','key'):
            read_private(root/'pki'/config[name])
        context.load_cert_chain(root/'pki'/config['cert'],root/'pki'/config['key'])
    conn=http.client.HTTPSConnection(url.hostname,url.port or 443,context=context,timeout=5)
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


def operator_bootstrap(root,argv,env,stack):
    """Issue only the reserved operator role, never a user's data-plane token."""
    pki=root/'pki'
    lock=os.open(pki/'operator.lock',os.O_CREAT|os.O_RDWR|os.O_NOFOLLOW,0o600)
    with os.fdopen(lock,'w') as stream:
        fcntl.flock(stream,fcntl.LOCK_EX)
        audience=stack['gateway']['public_url']
        if audience=='auto':
            audience='https://localhost:8443/'
        grant={'kind':'client','project_id':'lite-operator','principal_id':'operator',
               'role':'lite_operator','claims':[],'policy_version':'bootstrap',
               'knowledge_version':'unpublished','audience_url':audience,
               'purpose':'deployment-local health and protocol negotiation'}
        grant_path=pki/'operator-grant.json'
        if grant_path.exists() and json.loads(read_private(grant_path))!=grant:
            raise ValueError('LITE_OPERATOR_CHANGE_REQUIRES_EXPLICIT_MIGRATION')
        write(grant_path,json.dumps(grant).encode())
        for name in ('token-admin-password','token-runtime-password'):
            if not (pki/name).exists():
                write(pki/name,secrets.token_hex(32).encode())
        passwords=[read_private(pki/name).decode() for name in
                   ('token-admin-password','token-runtime-password')]
        if any(not re.fullmatch('[0-9a-f]{64}',value) for value in passwords):
            raise ValueError('LITE_OPERATOR_PASSWORD')
        # Only this deployment's control DB is administered. Secrets use stdin.
        sql="SET log_statement='none'; SET log_min_error_statement='panic';\n"
        for role,password in zip(('oo_token_admin','oo_token_runtime'),passwords):
            sql+=("SELECT format('CREATE ROLE %I LOGIN PASSWORD %L', "
                  f"'{role}', '{password}') WHERE NOT EXISTS "
                  f"(SELECT 1 FROM pg_roles WHERE rolname='{role}') \\gexec\n")
        sql+='GRANT CREATE ON DATABASE postgres TO oo_token_admin;\n'
        result=subprocess.run(['docker',*argv,'exec','-T','control-db','psql',
                               '-X','-q','-v','ON_ERROR_STOP=1','-U','postgres'],
                              input=sql.encode(),capture_output=True,env=env,timeout=30)
        if result.returncode:
            raise ValueError('LITE_OPERATOR_ROLES')
        write(pki/'operator-endpoint.json',json.dumps({
            'host':'control-db','port':5432,'database':'postgres'}).encode())
        command=argv+['run','--rm','--no-deps','--entrypoint','issue-token','operator-bootstrap']
        material=['/pki/operator-endpoint.json','/pki/token-admin-password','/pki/lite-ca-cert']
        docker(command+['install',*material],env)
        token=pki/'operator-token'
        if not token.exists():
            docker(command+['issue',*material,'/pki/operator-grant.json','/pki/operator-token'],env)
        if not read_private(token).strip().startswith(b'ooct_'):
            raise ValueError('LITE_OPERATOR_TOKEN')
        write(root/'health-client.json',json.dumps({
            'token':'operator-token','ca':'lite-ca-cert',
            'cert':'lite-admin-cert','key':'lite-admin-key'}).encode())

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
        operator_bootstrap(root,argv,env,request['stack'])
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
    history=({'last_success':None,'last_failure':None,'next_due':None,
              'schedule_basis':'control_db','schedule_status':'stopped'}
             if action=='down' else control_status(root,argv,env))
    return {'status':'stopped' if action=='down' else 'foundations_only',
            'services':services,'gateway_health':gateway,**history,
            'pending':['worker admission/migrations','scheduler bindings',
                'Builder service lifecycle and target binding','Gateway operator configuration'],
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
