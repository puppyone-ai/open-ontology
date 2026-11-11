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
import shutil
import ssl
import stat
import subprocess
import sys
import urllib.parse
import uuid
from datetime import datetime, timezone

def publication_health(history, services_up, now=None):
    published=history.get('data_published_at')
    # An unchanged source is current data, not an overdue one: health follows
    # the newest cycle that confirmed the served publication (activation or
    # exact replay), falling back to the publication time.
    verified=history.get('data_verified_at') or published
    try:
        age=((now or datetime.now(timezone.utc))-datetime.fromisoformat(
            verified.replace('Z','+00:00'))).total_seconds()
        fresh=0 <= age <= 2*history.get('interval_seconds',3600)+history.get('grace_seconds',60)
    except (AttributeError,TypeError,ValueError):
        age=None
        fresh=False
    degraded=(history.get('cycle') or {}).get('result')=='partial' or any(
        (history.get('tables') or {}).get(key) for key in ('stale','review_pending'))
    status='healthy' if services_up and fresh else 'unhealthy'
    if status=='healthy' and degraded:status='degraded'
    # A library-scoped pending entry stops every publication until review, so
    # surface it even while the last publication is still fresh.
    library=any(isinstance(row,dict) and row.get('scope')=='library'
                for row in (history.get('tables') or {}).get('review_pending') or [])
    return {'status':status,'healthy':status=='healthy','data_age_seconds':age,
            'alert':('library_review_required' if library else None) if fresh else
            'data_publication_overdue' if published else 'data_publication_unmeasured'}

def active_publication(root,argv,env):
    """Initial builds predate the first scheduled event; consult the DB head."""
    try:
        config=json.loads(read_private(root/'work/operator/schedule.json'))
        library=config['cycle']['library'].replace("'","''")
        query=f"""BEGIN TRANSACTION READ ONLY;
SET LOCAL statement_timeout='5s';
SELECT to_char(p.published_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')
FROM _oo.native_library_publication p JOIN _oo.native_library_head h
ON h.library_id=p.library_id AND h.receipt_sha256=p.receipt_sha256
WHERE h.library_id='{library}' AND h.state='ready';
COMMIT;"""
        raw=docker(argv+['exec','-T','agent-db','psql','-X','-qAt','-v','ON_ERROR_STOP=1',
                        '-U','postgres','-d','postgres','-c',query],env).decode().strip()
        if re.fullmatch(r'\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z',raw):
            return raw
    except Exception:
        pass
    return None

def service_status(row):
    result={key:row.get(key) for key in ('Service','State','Health','ExitCode')}
    job=row.get('Service')=='builder'
    result['Kind']='job' if job else 'service'
    state=row.get('State')
    if job:
        # Exited successfully is the expected lifecycle of a one-shot Builder.
        # It proves job completion, not review, publication or Gateway readiness.
        code=row.get('ExitCode')
        measured=type(code) is int or (type(code) is str and code.isascii() and code.isdecimal())
        success=measured and int(code)==0
        result['Status']=('succeeded' if success else 'failed' if measured else 'unmeasured') if state=='exited' else (
            'running' if state=='running' else 'not_completed')
        result['Healthy']=state=='exited' and success
    else:
        result['Healthy']=state=='running' and row.get('Health')=='healthy'
        result['Status']=('healthy' if result['Healthy'] else
                          'unmeasured' if state=='running' and not row.get('Health') else
                          'unhealthy')
    return result

def control_status(root,argv,env):
    empty={'last_success':None,'last_failure':None,'next_due':None}
    split={'data_published_at':None,'data_verified_at':None,'cycle':None,'tables':{'stale':[],'review_pending':[]}}
    path=root/'work/operator/schedule.json'
    if not path.exists():
        return {**empty,**split,'schedule_basis':'control_db','schedule_status':'not_configured'}
    try:
        config=json.loads(read_private(path))
        project=config['project']
        if str(uuid.UUID(project))!=project:
            raise ValueError('LITE_SCHEDULE_PROJECT')
        task_kind=(" AND e.task_id IN (SELECT id FROM jobs.tasks WHERE kind='lite_retained')"
                   if 'cycle' in config else '')
        schedule_kind=" AND kind='lite_retained'" if 'cycle' in config else ''
        event_fields=("""
 'cycle', (SELECT jsonb_build_object('result',payload->'result','reason',payload->'reason',
   'at',occurred_at) FROM jobs.lite_cycle_events WHERE project_id='%s'
   ORDER BY event_id DESC LIMIT 1),
 'data_published_at', (SELECT payload->'data_published_at' FROM jobs.lite_cycle_events
   WHERE project_id='%s' AND payload->>'data_published_at' IS NOT NULL ORDER BY event_id DESC LIMIT 1),
 'data_verified_at', (SELECT payload->'data_verified_at' FROM jobs.lite_cycle_events
   WHERE project_id='%s' AND payload->>'data_verified_at' IS NOT NULL ORDER BY event_id DESC LIMIT 1),
 'tables', coalesce((SELECT jsonb_build_object('stale',payload->'stale_tables',
   'review_pending',payload->'review_pending') FROM jobs.lite_cycle_events
   WHERE project_id='%s' ORDER BY event_id DESC LIMIT 1),
   '{"stale":[],"review_pending":[]}'::jsonb),
""" % (project,project,project,project)) if 'cycle' in config else ''
        # The socket is inside this owned control container, never a source DB.
        # One read-only snapshot; enqueue/fire success is not job completion.
        sql=f"""
BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY;
SET LOCAL ROLE oo_jobs_runtime;
SET LOCAL statement_timeout = '5s';
SELECT json_build_object(
 {event_fields}
 'last_success', (SELECT to_char(max(e.occurred_at) AT TIME ZONE 'UTC',
   'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') FROM jobs.task_events e
   WHERE e.project_id = '{project}'::uuid AND e.to_state = 'succeeded'{task_kind}),
 'last_failure', (SELECT to_char(max(e.occurred_at) AT TIME ZONE 'UTC',
   'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') FROM jobs.task_events e
   WHERE e.project_id = '{project}'::uuid
     AND (e.to_state = 'failed' OR
          (e.from_state = 'running' AND e.to_state = 'queued' AND e.note IS NOT NULL)){task_kind}),
 'next_due', (SELECT to_char(min(next_run_at) AT TIME ZONE 'UTC',
   'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') FROM jobs.schedules
   WHERE project_id = '{project}'::uuid AND enabled{schedule_kind}));
COMMIT;
"""
        raw=docker(argv+['exec','-T','control-db','psql','-X','-qAt',
                       '-v','ON_ERROR_STOP=1','-U','postgres','-d','postgres','-c',sql],env)
        data=json.loads(raw)
        if set(data)!=set(empty) | (set(split) if 'cycle' in config else set()):
            raise ValueError('LITE_CONTROL_PROTOCOL')
        for value in (data[key] for key in empty):
            if value is not None and (not isinstance(value,str) or not re.fullmatch(
                    r'\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z',value)):
                raise ValueError('LITE_CONTROL_PROTOCOL')
        return {**split,**data,'interval_seconds':config.get('interval_seconds',3600),
                'grace_seconds':config.get('grace_seconds',60),
                'schedule_basis':'control_db','schedule_status':'measured'}
    except Exception:
        return {**empty,**split,'schedule_basis':'control_db','schedule_status':'unavailable'}


def private_dir(path):
    path.mkdir(mode=0o700,exist_ok=True)
    st=path.lstat()
    if not stat.S_ISDIR(st.st_mode) or st.st_uid!=os.getuid() or st.st_mode & 0o777 != 0o700:
        raise ValueError('LITE_PRIVATE_DIRECTORY')

def require_outside_git(path):
    for parent in (path.resolve(),*path.resolve().parents):
        if (parent/'.git').exists():
            raise ValueError('LITE_MODEL_STATE_IN_GIT')

def model_sources(root, stack):
    """Materialize input credentials outside Git; return container-only references."""
    stack=json.loads(json.dumps(stack))
    model=stack['model']
    sources=[('model-key',model['key'])]
    if isinstance(model['base_url'],dict):
        sources.append(('model-endpoint',model['base_url']))
    for index,source in enumerate(stack.get('sources',[])):
        sources.append((f'source-{index}-password',source['password']))
        if source.get('tls_ca'):
            sources.append((f'source-{index}-ca',source['tls_ca']))
    home=Path.home()
    state=home/'.config/open-ontology/deployments'/hashlib.sha256(
        str(root.resolve()).encode()).hexdigest()[:32]
    # Resolve before creating anything, including a HOME redirected into Git.
    require_outside_git(state)
    for path in (home/'.config',home/'.config/open-ontology',
                 state.parent,state):
        if path.is_symlink():
            raise ValueError('LITE_PRIVATE_DIRECTORY')
        if path in (state.parent,state):
            private_dir(path)
        else:
            path.mkdir(mode=0o700,exist_ok=True)
            meta=path.stat()
            if meta.st_uid!=os.getuid() or meta.st_mode & 0o022:
                raise ValueError('LITE_PRIVATE_DIRECTORY')
    mounts=[]
    for name,reference in sources:
        ref=reference['secret_ref']
        if ref.startswith('file:'):
            source=Path(ref[5:])
            if not source.is_absolute() or '..' in source.parts:
                raise ValueError('LITE_MODEL_SOURCE')
        elif re.fullmatch(r'[a-zA-Z0-9][a-zA-Z0-9_.-]{0,127}',ref):
            source=home/'.config/open-ontology/secrets'/ref
        else:
            raise ValueError('LITE_MODEL_SOURCE')
        dest=state/name
        data=read_private(source)
        if dest.exists() and read_private(dest)!=data:
            raise ValueError('LITE_SECRET_CHANGE_REQUIRES_EXPLICIT_ROTATION')
        if not dest.exists():write(dest,data)
        reference['secret_ref']='oo-lite-'+name
        mounts.append({'type':'bind','source':str(dest),
                       'target':'/home/oo/.config/open-ontology/secrets/oo-lite-'+name,
                       'read_only':True})
    return stack,mounts


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


def docker_command(argv,env):
    # Docker Desktop also installs Compose on PATH. An empty HOME may not
    # contain Docker's per-user plugin link; never borrow another HOME/config.
    if argv and argv[0]=='compose':
        standalone=shutil.which('docker-compose',path=env.get('PATH'))
        if standalone:
            return [standalone,*argv[1:]]
    return ['docker',*argv]

def docker(argv,env):
    p=subprocess.run(docker_command(argv,env),capture_output=True,env=env,timeout=180)
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


# Run inside the Gateway container when the host Python's TLS library cannot
# load the Ed25519 client identity (macOS LibreSSL). Material arrives on stdin
# and only the two deadlines leave; nothing is written outside its /tmp.
RUNTIME_HEALTH = r"""
import http.client,json,os,ssl,sys,tempfile
m=json.loads(sys.stdin.read())
d=tempfile.mkdtemp()
for n in ('cert','key'):
    with open(os.path.join(d,n),'w',opener=lambda p,f:os.open(p,f,0o600)) as s:s.write(m[n])
c=ssl.create_default_context(cadata=m['ca']);c.load_cert_chain(os.path.join(d,'cert'),os.path.join(d,'key'))
h=http.client.HTTPSConnection('localhost',m['port'],context=c,timeout=5)
h.request('GET','/v1/health',headers={'Authorization':'Bearer '+m['token']})
r=h.getresponse();b=json.loads(r.read(65537)) if r.status==200 else {}
print(json.dumps({'status':r.status,'deadlines':[(b.get(k) or {}).get('expires_at_ms') for k in ('token_expiry','internal_mtls_certificate_expiry')]}))
"""

def runtime_health(root,argv,env,config,port):
    material={'port':port,**{name:read_private(root/'pki'/config[name]).decode().strip()+('\n' if name!='token' else '')
                             for name in ('token','ca','cert','key')}}
    p=subprocess.run([*docker_command(argv,env),'exec','-T','gateway','python3','-c',RUNTIME_HEALTH],
                     input=json.dumps(material).encode(),capture_output=True,env=env,timeout=30)
    if p.returncode:raise ValueError('LITE_HEALTH_RUNTIME')
    reply=json.loads(p.stdout)
    if reply['status']!=200:raise ValueError('LITE_HEALTH_REFUSED')
    return reply['deadlines']

def health(root,stack,port=8443,argv=None,env=None):
    client=root/'health-client.json'
    if not client.exists():
        return {'status':'unmeasured','nearest_credential_expiry_ms':None}
    config=json.loads(read_private(client))
    public=stack['gateway']['public_url']
    url=urllib.parse.urlsplit(f'https://localhost:{port}/' if public=='auto' else public)
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
    deadlines=None
    if 'cert' in config:
        for name in ('cert','key'):
            read_private(root/'pki'/config[name])
        try:
            context.load_cert_chain(root/'pki'/config['cert'],root/'pki'/config['key'])
        except ssl.SSLError:
            if argv is None or url.hostname not in ('localhost','127.0.0.1'):
                raise
            deadlines=runtime_health(root,argv,env,config,url.port or 443)
    if deadlines is None:
        conn=http.client.HTTPSConnection(url.hostname,url.port or 443,context=context,timeout=5)
        try:
            conn.request('GET',url.path.rstrip('/')+'/v1/health',headers={'Authorization':'Bearer '+token})
            response=conn.getresponse()
            if response.status!=200:
                raise ValueError('LITE_HEALTH_REFUSED')
            data=json.loads(response.read(65537))
            deadlines=[data[k].get('expires_at_ms') for k in ('token_expiry','internal_mtls_certificate_expiry')]
        finally:
            conn.close()
    if any(d is not None and (isinstance(d,bool) or not isinstance(d,int)) for d in deadlines):
        raise ValueError('LITE_HEALTH_PROTOCOL')
    measured=[d for d in deadlines if d is not None]
    safe={k:{'expires_at_ms':d} for k,d in zip(('token_expiry','internal_mtls_certificate_expiry'),deadlines)}
    return {'status':'measured','nearest_credential_expiry_ms':min(measured) if measured else None,
            'basis':'/v1/health',**safe}


def retained_login_grant(login):
    if (not isinstance(login,str) or login=='public'
            or not re.fullmatch('[a-z_][a-z0-9_]{0,62}',login)):
        raise ValueError('LITE_GATEWAY_DATA_LOGIN')
    # This helper exposes verified retained proofs, not source access or DDL.
    return f'GRANT EXECUTE ON FUNCTION _oo.native_library_retained_table(text) TO "{login}";\n'

def provision_retained_login(root,argv,env):
    config=root/'work/operator/gateway/config.json'
    if not config.exists():
        return
    login=json.loads(read_private(config)).get('data_user')
    if login is None:
        return
    sql=retained_login_grant(login)
    result=subprocess.run([*docker_command(argv,env),'exec','-T','agent-db','psql',
        '-X','-q','-v','ON_ERROR_STOP=1','-U','postgres'],
        input=sql.encode(),capture_output=True,env=env,timeout=30)
    if result.returncode:
        raise ValueError('LITE_GATEWAY_RETAINED_GRANT')

def gateway_port(stack,saved):
    """The host loopback port for this deployment's Gateway."""
    url=stack['gateway']['public_url']
    if url!='auto':
        parts=urllib.parse.urlsplit(url)
        if parts.scheme!='https' or parts.hostname not in ('localhost','127.0.0.1'):
            raise ValueError('LITE_PUBLIC_URL_LOCAL_HTTPS')
        return parts.port or 443
    if saved.get('gateway_port'):
        return saved['gateway_port']
    if saved:
        return 8443  # Deployments created before the port was recorded.
    import socket
    with socket.socket() as sock:
        sock.bind(('127.0.0.1',0))
        return sock.getsockname()[1]

def operator_bootstrap(root,argv,env,stack,port=8443):
    """Issue only the reserved operator role, never a user's data-plane token."""
    pki=root/'pki'
    lock=os.open(pki/'operator.lock',os.O_CREAT|os.O_RDWR|os.O_NOFOLLOW,0o600)
    with os.fdopen(lock,'w') as stream:
        fcntl.flock(stream,fcntl.LOCK_EX)
        audience=stack['gateway']['public_url']
        if audience=='auto':
            audience=f'https://localhost:{port}/'
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
        result=subprocess.run([*docker_command(argv,env),'exec','-T','control-db','psql',
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

LOOPBACK={'localhost','127.0.0.1','::1'}
DATA_LOGIN='oo_gateway_login'

def copy_private(src,dest):
    data=read_private(src)
    if dest.exists():
        if read_private(dest)!=data:raise ValueError('LITE_PROVISION_CHANGED')
    else:write(dest,data)

def save_json(path,value):
    data=json.dumps(value,sort_keys=True).encode()
    if path.exists() and read_private(path)==data:return
    write(path,data)

def compose_input(argv,env,args,data,code,timeout=300):
    result=subprocess.run([*docker_command(argv,env),*args],input=data,capture_output=True,env=env,timeout=timeout)
    if result.returncode:raise ValueError(code)
    return result.stdout

def runtime_stack(stack):
    """The runtime's copy: a loopback source is this host, not the container."""
    copy=json.loads(json.dumps(stack))
    for source in copy['sources']:
        if source['host'] in LOOPBACK:source['host']='host.docker.internal'
    return copy

def provision_retained(root,argv,env,request,port):
    """Install the retained build chain's configuration inside this deployment.

    D16: `oo build` runs every stage in the runtime image; this derives what it
    needs from stack.yaml and the deployment's own credentials. Secrets are
    copied between private files or sent on stdin, never placed in argv.
    """
    stack=request['stack']
    if len(stack['sources'])!=1:raise ValueError('LITE_BUILD_ONE_SOURCE')
    source=stack['sources'][0]['id']
    pki,work,home=root/'pki',root/'work',root/'home'
    operator,gateway,consumer=work/'operator',work/'operator/gateway',root/'consumer'
    for path in (operator,work/'build',gateway,consumer,*(gateway/n for n in ('packs','wal','reviewed','collector'))):
        private_dir(path)
    store=home/'.config/open-ontology/secrets'
    # Agent DB schema: the runtime's checked installer, idempotent by design.
    compose_input(argv,env,['run','--rm','--no-deps','-T','--entrypoint','install-retained-target',
        'operator-bootstrap'],json.dumps({'protocol':'oo.pipeline.install-retained-target/v1',
        'host':'agent-db','port':5432,'database':'postgres','user':'postgres',
        'password_file':'/pki/lite-admin-password','ca_file':'/pki/lite-ca-cert'}).encode(),
        'LITE_AGENT_DB_INSTALL',600)
    if not (pki/'gateway-data-password').exists():
        write(pki/'gateway-data-password',secrets.token_hex(32).encode())
    password=read_private(pki/'gateway-data-password').decode()
    if not re.fullmatch('[0-9a-f]{64}',password):raise ValueError('LITE_PROVISION_PASSWORD')
    sql=("SET log_statement='none'; SET log_min_error_statement='panic';\n"
         f"SELECT format('CREATE ROLE {DATA_LOGIN} LOGIN NOINHERIT PASSWORD %L','{password}') "
         f"WHERE NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='{DATA_LOGIN}') \\gexec\n"
         "SELECT 'CREATE ROLE oo_r_owner NOLOGIN NOINHERIT' "
         "WHERE NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='oo_r_owner') \\gexec\n"
         f"GRANT oo_r_owner TO {DATA_LOGIN} WITH INHERIT FALSE, SET TRUE;\n"
         f"GRANT CONNECT ON DATABASE postgres TO {DATA_LOGIN};\n"
         f"GRANT USAGE ON SCHEMA _oo,oo_publish_target TO {DATA_LOGIN};\n"
         "GRANT SELECT ON _oo.physical_mappings,_oo.table_freshness,"
         "_oo.source_watermark,_oo.table_quality,_oo.apply_receipts,"
         "_oo.full_import_run,_oo.full_import_table,_oo.full_import_quality,"
         "_oo.publication_identity,_oo.native_library_source_status,"
         "_oo.native_library_head,_oo.native_library_publication,"
         f"oo_publish_target.fence TO {DATA_LOGIN};\n"
         "GRANT EXECUTE ON FUNCTION _oo.lookup_physical_mapping(text,text,bigint,text,text,text,text) "
         f"TO {DATA_LOGIN};\n"
         f"ALTER ROLE {DATA_LOGIN} SET default_transaction_read_only=on;\n")
    compose_input(argv,env,['exec','-T','agent-db','psql','-X','-q','-v','ON_ERROR_STOP=1',
                            '-U','postgres'],sql.encode(),'LITE_GATEWAY_DATA_LOGIN')
    # Gateway operator directory: tokens in control-db, data in agent-db.
    for src,dest in [('token-runtime-password','password'),('lite-ca-cert','ca.crt'),
                     ('lite-gateway-key','tls.key'),('lite-gateway-cert','tls.crt'),
                     ('gateway-data-password','data-password'),('token-admin-password','token-admin-password')]:
        copy_private(pki/src,gateway/dest)
    save_json(gateway/'endpoint.json',{'host':'control-db','port':5432,'database':'postgres'})
    save_json(gateway/'data-endpoint.json',{'host':'agent-db','port':5432,'database':'postgres'})
    public=f'https://localhost:{port}'
    save_json(gateway/'config.json',{'listen':f'0.0.0.0:{port}','transport':'tls','audience':public,
        'runtime_user':'oo_token_runtime','password_file':'password','ca_file':'ca.crt',
        'endpoint_file':'endpoint.json','pack_dir':'packs','body_limit':1048576,
        'request_timeout_ms':30000,'global_concurrency':8,'tls_key_file':'tls.key',
        'tls_chain_file':'tls.crt','client_ca_file':'ca.crt','handshake_timeout_ms':10000,
        'data_endpoint_file':'data-endpoint.json','data_password_file':'data-password',
        'data_ca_file':'ca.crt','data_user':DATA_LOGIN,'wal_dir':'wal','reviewed_dir':'reviewed',
        'audit_collector_dir':'collector','token_admin_password_file':'token-admin-password',
        'token_admin_workload':'urn:oo:workload:admin'})
    # Runtime-side identities. The admin pair issues the first user's token;
    # the owner's CLI receives a separate consumer workload certificate.
    admin=read_private(pki/'lite-admin-cert')+read_private(pki/'lite-admin-key')
    if not (pki/'lite-admin.pem').exists():write(pki/'lite-admin.pem',admin)
    copy_private(pki/'lite-ca-cert',consumer/'ca.crt')
    pem=read_private(pki/'lite-consumer-cert')+read_private(pki/'lite-consumer-key')
    if not (consumer/'client.pem').exists():write(consumer/'client.pem',pem)
    copy_private(pki/'lite-admin-password',store/'oo-lite-agent-db-password')
    copy_private(pki/'lite-ca-cert',store/'oo-lite-ca')
    admin_password=read_private(pki/'lite-admin-password').decode().strip()
    if not re.fullmatch('[0-9a-f]{64}',admin_password):raise ValueError('LITE_PROVISION_PASSWORD')
    services=''.join(f'[{name}]\nhost={name}-db\nport=5432\ndbname=postgres\nuser=postgres\n'
                     'sslmode=verify-full\nsslrootcert=/home/oo/.config/open-ontology/secrets/oo-lite-ca\n'
                     # tls-server-end-point binding has no digest for Ed25519 leaves.
                     'channel_binding=disable\n' for name in ('agent','control'))
    for path,data in [(home/'.pg_service.conf',services.encode()),
                      (home/'.pgpass',''.join(f'{name}-db:5432:postgres:postgres:{admin_password}\n'
                                             for name in ('agent','control')).encode())]:
        if not path.exists() or read_private(path)!=data:write(path,data)
    save_json(work/'build/stack.json',runtime_stack(stack))
    save_json(operator/'target.json',{'host':'agent-db','port':5432,'database':'postgres','user':'postgres',
        'password':{'secret_ref':'oo-lite-agent-db-password'},'tls_ca':{'secret_ref':'oo-lite-ca'},
        'run_id':'initial'})
    save_json(operator/'publisher.json',{'host':'agent-db','port':5432,'database':'postgres',
        # Home-relative, so the scheduled worker (no /pki mount) can publish too.
        'user':'postgres','password_file':'/home/oo/.config/open-ontology/secrets/oo-lite-agent-db-password',
        'ca_file':'/home/oo/.config/open-ontology/secrets/oo-lite-ca'})
    save_json(operator/'model.json',{'model':stack['model']})
    save_json(operator/'build-runtime.json',{'protocol':'oo.build-runtime/v1','source_id':source,
        'library_id':'library','secrets_dir':'/home/oo/.config/open-ontology/secrets',
        'builder_script':'/opt/oo/pg-builder/retained_build.py',
        'builder':{'project':'lite','psql_command':['psql','service=agent'],
                   'model':{'stack':'/work/operator/model.json','egress':'/opt/oo/model_egress.py','call_limit':6}},
        'publisher_config':'/work/operator/publisher.json','gateway_login':DATA_LOGIN,
        'gateway':{'runtime_dir':'/work/operator/gateway','gateway_url':public,
                   'ca_file':'/pki/lite-ca-cert','client_pem':'/pki/lite-admin.pem',
                   # Remote deployments name the owner's local copies instead.
                   'prompt_ca_file':(request.get('prompt') or {}).get('ca_file',str(consumer/'ca.crt')),
                   'prompt_client_pem':(request.get('prompt') or {}).get('client_pem',str(consumer/'client.pem')),
                   # Optional registry that carries this exact CLI version (share prompt step 1).
                   **({'prompt_npm_registry':request['prompt_npm_registry']}
                      if request.get('prompt_npm_registry') else {}),
                   'first_user_grant':{'project_id':'lite','principal_id':'owner','role':'oo_r_owner',
                       'claims':{},'policy_version':'draft-v1','knowledge_version':'draft-v1',
                       'audience_url':public,'ttl_ms':7776000000,'purpose':'first build'}}})
    # Gateway needs a reviewed publication; the first `oo build` publishes it
    # (verifying it inside the runtime) and then reconciles this deployment,
    # which arms the scheduled retained cycle and starts the remaining services.
    if (gateway/'reviewed/library/library.json').exists():
        services=['gateway']
        if provision_schedule(root,argv,env,request):
            services+=['builder','worker','scheduler']
        docker(argv+['--profile','integration','up','--detach','--no-deps',*services],env)

JOBS=['0000_bootstrap_roles.sql','0001_jobs_control_expand.sql',
      '0002_event_project_binding_expand.sql','0003_job_lease_operations_expand.sql']
JOBS_VERSIONS=['jobs-bootstrap/1','jobs-control/1','jobs-event-project/1','jobs-lease-operations/1']

def interval_seconds(text):
    match=re.fullmatch(r'(\d+)([smhd])',text or '8h')
    if not match:raise ValueError('LITE_SYNC_INTERVAL')
    seconds=int(match.group(1))*{'s':1,'m':60,'h':3600,'d':86400}[match.group(2)]
    if not 60<=seconds<=86400:raise ValueError('LITE_SYNC_INTERVAL')
    return seconds

def provision_schedule(root,argv,env,request):
    """After the first build: jobs schema, schedule row and schedule.json.

    The cycle reuses the build's owner-reviewed registry and facts; it never
    creates review decisions. Returns False until the first build completed.
    """
    state=root/'work/build/.oo'
    chain=state/'chain.json'
    if not chain.exists() or json.loads(read_private(chain)).get('status')!='complete':
        return False
    templates=request.get('jobs') or {}
    if sorted(templates)!=sorted(JOBS):raise ValueError('LITE_JOBS_TEMPLATES')
    operator=root/'work/operator'
    path=operator/'schedule.json'
    saved=json.loads(read_private(path)) if path.exists() else {}
    project=saved.get('project') or str(uuid.uuid4())
    if str(uuid.UUID(project))!=project:raise ValueError('LITE_SCHEDULE_PROJECT')
    installed=compose_input(argv,env,['exec','-T','control-db','psql','-X','-qAt','-U','postgres'],
        b"SELECT to_regclass('jobs.schema_migrations') IS NOT NULL;",'LITE_JOBS_SCHEMA').strip()
    if installed!=b't':
        sql=['BEGIN; SELECT pg_advisory_xact_lock(6401202601);']
        for i,name in enumerate(JOBS):
            if i==1:sql.append('SET LOCAL ROLE oo_jobs_owner;')
            sql.append(templates[name])
        for version,name in zip(JOBS_VERSIONS,JOBS):
            digest=hashlib.sha256(templates[name].encode()).hexdigest()
            sql.append(f"INSERT INTO jobs.schema_migrations(version,checksum,phase,filename) "
                       f"VALUES('{version}','{digest}','expand','{name}');")
        # jobs-bootstrap revokes PUBLIC's CONNECT on the control database; the
        # token store shares it in lite and keeps its own explicit grant.
        sql.append('RESET ROLE; GRANT CONNECT ON DATABASE postgres TO oo_token_admin, oo_token_runtime;')
        sql.append('COMMIT;')
        compose_input(argv,env,['exec','-T','control-db','psql','-X','-q','-v','ON_ERROR_STOP=1',
                                '-U','postgres'],'\n'.join(sql).encode(),'LITE_JOBS_SCHEMA')
    compose_input(argv,env,['exec','-T','control-db','psql','-X','-q','-v','ON_ERROR_STOP=1',
                            '-U','postgres'],request['cycle_events'].encode(),'LITE_CYCLE_SCHEMA')
    stack=request['stack']
    source=stack['sources'][0]
    origin={name:{'path':'/work/build/.oo/'+file,
                  'sha256':hashlib.sha256(read_private(state/file)).hexdigest()}
            for name,file in [('facts','facts.json'),('reviewed_registry','reviewed-registry.json')]}
    copy_private(root/'consumer/ca.crt',operator/'probe-ca.crt')
    copy_private(root/'pki/lite-consumer-cert',operator/'probe.crt')
    copy_private(root/'pki/lite-consumer-key',operator/'probe.key')
    port=json.loads(read_private(root/'owner.json'))['gateway_port']
    config={'project':project,'interval_seconds':interval_seconds(source.get('sync',{}).get('interval')),
        'control_psql_command':['psql','service=control'],'work_dir':'/work/cycles','cycle':{
        'stack':'/work/build/stack.json','source':source['id'],'target':'/work/operator/target.json',
        'secrets_dir':'/home/oo/.config/open-ontology/secrets','library':'library','login':DATA_LOGIN,
        'publisher_config':'/work/operator/publisher.json','retain_seconds':86400,
        'builder':{'protocol':'oo.builder.retained-build/v1','library_id':'library',
                   'source_engine':source['engine'],'project':'lite',
                   'psql_command':['psql','service=agent'],'origin':origin},
        'document_path':'/work/operator/gateway/reviewed/library/library.json',
        'pack_path':'/work/operator/gateway/packs/library.json',
        'gateway_probe':{'url':f'https://gateway:{port}','ca':'/work/operator/probe-ca.crt',
                         'cert':'/work/operator/probe.crt','key':'/work/operator/probe.key',
                         # The owner's first token; the probe reads only freshness.
                         'token_file':'/work/build/.oo/first-user.token'}}}
    first_config=state/'builder-config.json'
    if first_config.exists() and 'source_catalog' in json.loads(read_private(first_config)):
        config['cycle']['source_catalog']='export'
        # Only this additive migration is safe; all other config changes still refuse.
        if saved and 'source_catalog' not in saved.get('cycle',{}):
            saved['cycle']['source_catalog']='export'
    if saved and saved!=config:raise ValueError('LITE_SCHEDULE_CHANGE_REQUIRES_EXPLICIT_MIGRATION')
    save_json(path,config)
    compose_input(argv,env,['exec','-T','control-db','psql','-X','-q','-v','ON_ERROR_STOP=1','-U','postgres'],
        ("SET ROLE oo_jobs_runtime;\n"
         "INSERT INTO jobs.schedules(project_id,kind,config_version,artifact_digest,dedupe_prefix,next_run_at) "
         f"SELECT '{project}','lite_retained','v1','{origin['reviewed_registry']['sha256']}','lite',clock_timestamp() "
         f"WHERE NOT EXISTS(SELECT 1 FROM jobs.schedules WHERE project_id='{project}' AND kind='lite_retained');\n").encode(),
        'LITE_SCHEDULE_ROW')
    return True

def retained_function_exists(argv,env):
    result=subprocess.run([*docker_command(argv,env),'exec','-T','agent-db','psql','-X','-qAt','-U','postgres','-c',
        "SELECT to_regprocedure('_oo.native_library_retained_table(text)') IS NOT NULL"],
        capture_output=True,env=env,timeout=30)
    return result.returncode==0 and result.stdout.strip()==b't'

def main(request):
    root=Path(request['root'])
    action=request['action']
    for p in (root.parent,root): private_dir(p)
    env={k:v for k,v in os.environ.items() if k in ('PATH','HOME','DOCKER_HOST','DOCKER_CONTEXT','DOCKER_CONFIG','DOCKER_DEFAULT_PLATFORM','TMPDIR')}
    project='oo-a-lite-'+hashlib.sha256(str(root.resolve()).encode()).hexdigest()[:16]
    owner=root/'owner.json'
    if action!='up' and not owner.exists():
        return {'status':'not_started','gateway_health':{'status':'unmeasured'},'services':[],
                'data_published_at':None,'data_verified_at':None,'cycle':None,'tables':{'stale':[],'review_pending':[]}}
    image=request['image'] if action=='up' else json.loads(read_private(owner))['image']
    if not re.fullmatch(r'(?:[a-zA-Z0-9./:_-]+@)?sha256:[0-9a-f]{64}',image):
        raise ValueError('LITE_IMAGE_DIGEST_REQUIRED')
    saved=json.loads(read_private(owner)) if owner.exists() else {}
    lane=saved.get('lane','v03' if request.get('demo') else os.environ.get('OO_LITE_LANE','a'))
    if not re.fullmatch('[a-z][a-z0-9]{0,7}',lane):raise ValueError('LITE_LANE')
    env.update(OO_LITE_STATE=str(root),OO_LITE_RUNTIME_IMAGE=image,OO_LITE_UID=str(os.getuid()),
               OO_LITE_GID=str(os.getgid()),OO_LITE_LANE=lane)
    if os.environ.get('OO_LITE_POSTGRES_IMAGE'):
        env['OO_LITE_PULL_POLICY']='never'
    network=saved.get('subnet')
    if not network:network=subnet(project,env)
    if not ipaddress.ip_network(network).is_private:raise ValueError('LITE_SUBNET')
    env['OO_LITE_SUBNET']=network
    port=gateway_port(request['stack'],saved) if action=='up' else saved.get('gateway_port',8443)
    argv=['compose','--project-name',project,'--file',str(root/'compose.yaml')]
    if action=='up' and request.get('retained'):
        # The Builder job replays the completed runtime build: its exit code
        # measures that the reviewed chain still verifies, never a new draft.
        write(root/'compose.local.yaml',json.dumps({'services':{'gateway':{
            'ports':[f'127.0.0.1:{port}:{port}']},'builder':{'command':[
            'build','--config','/work/build/stack.json','--agent-target','/work/operator/target.json',
            '--runtime-config','/work/operator/build-runtime.json','--resume','--json']}}}).encode())
    if (root/'compose.local.yaml').exists():
        argv+=['--file',str(root/'compose.local.yaml')]
    ids=docker(['ps','-aq','--filter','label=com.docker.compose.project='+project],env).decode().split()
    for cid in ids:
        labels=json.loads(docker(['inspect','--format','{{json .Config.Labels}}',cid],env))
        if labels.get('oo.lane')!=lane: raise ValueError('LITE_OWNERSHIP')
    for kind in ('network','volume'):
        names=docker([kind,'ls','-q','--filter','label=com.docker.compose.project='+project],env).decode().split()
        for name in names:
            labels=json.loads(docker([kind,'inspect','--format','{{json .Labels}}',name],env))
            if labels.get('oo.lane')!=lane:raise ValueError('LITE_OWNERSHIP')
    if action=='up':
        if owner.exists() and json.loads(read_private(owner))['image']!=image:
            raise ValueError('LITE_IMAGE_CHANGE_REQUIRES_EXPLICIT_MIGRATION')
        for name in ('work','home'):private_dir(root/name)
        for sub in ('home/.config','home/.config/open-ontology','home/.config/open-ontology/secrets'):
            private_dir(root/sub)
        input_stack_sha256=hashlib.sha256(json.dumps(request['stack'],sort_keys=True,
            separators=(',',':'),ensure_ascii=False).encode()).hexdigest()
        mounts=[]
        if not request.get('demo'):
            request=dict(request)
            request['stack'],mounts=model_sources(root,request['stack'])
            # Nested file mounts under the read-only home need empty targets.
            # These placeholders contain no credential bytes.
            for mount in mounts:
                target=root/'home/.config/open-ontology/secrets'/Path(mount['target']).name
                if target.exists():
                    if read_private(target):
                        raise ValueError('LITE_SECRET_MOUNT_SHADOW')
                else:
                    write(target,b'')
        for name in ('compose.yaml','postgres.sh','scheduler.py'):
            dest=root/('work' if name=='scheduler.py' else '')/name
            template=request['templates'][name]
            if name=='compose.yaml' and mounts:
                marker='    - ${OO_LITE_STATE:?}/home:/home/oo:ro\n'
                if template.count(marker)!=1:
                    raise ValueError('LITE_MODEL_MOUNT')
                template=template.replace(marker,marker+''.join(
                    '    - '+json.dumps(mount)+'\n' for mount in mounts))
            # docker save/load retains config IDs, not registry RepoDigests.
            postgres=os.environ.get('OO_LITE_POSTGRES_IMAGE')
            if name=='compose.yaml' and postgres:
                if not re.fullmatch(r'sha256:[0-9a-f]{64}',postgres):
                    raise ValueError('LITE_IMAGE_DIGEST_REQUIRED')
                template=re.sub(r'image: docker.io/library/postgres@sha256:[0-9a-f]{64}',
                                'image: '+postgres,template)
            write(dest,template.encode())
        write(root/'work/stack.json',json.dumps(request['stack']).encode())
        write(owner,json.dumps({'project':project,'image':image,'lane':lane,'subnet':network,
                                'gateway_port':port,'input_secret_mounts':mounts,
                                'input_stack_sha256':input_stack_sha256}).encode())
        offline=['--pull','never','--no-build'] if os.environ.get('OO_LITE_POSTGRES_IMAGE') else []
        services=['agent-db','control-db'] if request.get('demo') else []
        docker(argv+['up',*offline,'--detach','--wait','--wait-timeout','120',*services],env)
        operator_bootstrap(root,argv,env,request['stack'],port)
        if request.get('retained'):
            provision_retained(root,argv,env,request,port)
        if not request.get('retained') or retained_function_exists(argv,env):
            provision_retained_login(root,argv,env)
    elif action=='down':
        # Keep database volumes and credentials. No prune, orphan or volume flags.
        docker(argv+['--profile','integration','down'],env)
    elif action!='status':raise ValueError('LITE_ACTION')
    raw=docker(argv+['ps','--all','--format','json'],env).decode().strip()
    rows=json.loads(raw) if raw.startswith('[') else [json.loads(line) for line in raw.splitlines()]
    services=[service_status(r) for r in rows]
    gateway={'status':'unmeasured','nearest_credential_expiry_ms':None}
    if action!='down':
        try:gateway=health(root,request['stack'],port,argv,env)
        except Exception:gateway={'status':'unavailable','nearest_credential_expiry_ms':None}
    history=({'last_success':None,'last_failure':None,'next_due':None,
              'data_published_at':None,'data_verified_at':None,'cycle':None,'tables':{'stale':[],'review_pending':[]},
              'schedule_basis':'control_db','schedule_status':'stopped'}
             if action=='down' else control_status(root,argv,env))
    if action!='down' and not history.get('data_published_at'):
        history['data_published_at']=active_publication(root,argv,env)
    expected={'agent-db','control-db','model-egress','worker','scheduler','builder','gateway'}
    measured={row['Service']:row for row in services if row['Service'] in expected}
    services_up=(action!='down' and set(measured)==expected
             and all(row['Healthy'] for row in measured.values())
             and gateway['status']=='measured')
    health_report=publication_health(history,services_up)
    healthy=health_report['healthy']
    status='stopped' if action=='down' else health_report['status'] if history.get('data_published_at') else 'foundations_only'
    pending=[] if status in ('healthy','degraded') else ['worker admission/migrations','scheduler bindings',
        'Builder target binding and publication','Gateway operator configuration']
    build=None
    # Right after the first build no scheduled cycle has run yet and the
    # scheduler services are only now starting: that is retained_ready, not an
    # unhealthy deployment (the first `oo build` reconciles with `up`).
    first_cycle_pending=(status=='unhealthy' and not history.get('cycle'))
    if (status=='foundations_only' or first_cycle_pending) and (root/'work/operator/build-runtime.json').exists():
        # Retained chain provisioned (D16): `oo build` runs in this runtime.
        # A completed first build is not a measured scheduled cycle.
        chain=root/'work/build/.oo/chain.json'
        try:build=json.loads(read_private(chain))['status'] if chain.exists() else 'not_started'
        except Exception:build='unavailable'
        status='retained_ready'
        pending=['oo build' if build!='complete' else 'scheduled retained cycle']
    return {**health_report,'status':status,'services':services,'gateway_health':gateway,**history,
            'model_source_configured':bool(request['stack'].get('model')) and not request.get('demo',False),
            'pending':pending,'build':build,'gateway_url':f'https://localhost:{port}' if build else None,
            'publication':'live' if services_up and history.get('data_published_at') else 'unmeasured','healthy':healthy}

if __name__=='__main__':
    os.umask(0o077)
    try:
        report=main(json.loads(sys.stdin.buffer.read(2097153)))
        print(json.dumps(report))
        raise SystemExit(1 if report.get('status') in ('foundations_only','unhealthy') else 0)
    except Exception as error:
        code=str(error)
        if not isinstance(error,ValueError) or not re.fullmatch(r'LITE_[A-Z_]+',code):
            code='LITE_LIFECYCLE_FAILED'
        print(json.dumps({'status':'failed','error':code,'healthy':False}))
        raise SystemExit(1)
