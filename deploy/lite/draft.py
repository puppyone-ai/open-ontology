"""Resumable PostgreSQL metadata-to-draft bridge; never reviews or publishes."""
import fcntl
import hashlib
import json
import os
from pathlib import Path
import signal
import re
import stat
import subprocess
import sys
import time

STAGES = ('preflight', 'catalog', 'profile', 'comments', 'seal', 'cards', 'generation')

def load_community_modules(sources, root):
    import types
    for name in ('model_provider', 'selfbuild_budget', 'community_adapter'):
        module = types.ModuleType(name)
        module.__file__ = str(root/'embedded/tools/pg-builder/scripts'/(name+'.py'))
        sys.modules[name] = module
        exec(compile(sources[name], module.__file__, 'exec'), module.__dict__)
        module.ROOT = root
    return sys.modules['community_adapter']


def digest(data):
    return hashlib.sha256(data).hexdigest()


def read_private(path):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, 'rb') as stream:
        st = os.fstat(stream.fileno())
        if not stat.S_ISREG(st.st_mode) or st.st_nlink != 1 or st.st_uid != os.getuid() or st.st_mode & 0o777 != 0o600 or st.st_size > 1048576:
            raise ValueError('PRIVATE_FILE')
        return stream.read(1048577)


def write_private(path, data):
    tmp = path.with_name(path.name + '.tmp-' + str(os.getpid()))
    fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    try:
        with os.fdopen(fd, 'wb') as stream:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(tmp, path)
    finally:
        tmp.unlink(missing_ok=True)


def save(path, value):
    write_private(path, (json.dumps(value, sort_keys=True) + '\n').encode())


def initial(fingerprint):
    return {'protocol': 'oo.build-progress/v1', 'config_sha256': fingerprint,
            'status': 'running', 'stages': [dict(name=s, status='pending', attempts=0,
              completed=0, total=1, elapsed_seconds=None, operations_per_second=None,
              rows_per_second=None, error=None, artifacts={}) for s in STAGES],
            'last_success': None, 'last_failure': None, 'next_due': None,
            'publication': 'not_published', 'review': 'unreviewed',
            'agent_db_load': 'pending_oo_load', 'ontology_export': None, 'pack': None,
            'gateway_url': None, 'enroll': 'pending_publication', 'share_prompt': 'pending_publication'}


def check_resume(state, fingerprint, root):
    if state.get('protocol') != 'oo.build-progress/v1' or state.get('config_sha256') != fingerprint:
        raise ValueError('RESUME_CONFIG_CHANGED')
    if [s['name'] for s in state['stages']] != list(STAGES):
        raise ValueError('RESUME_PROTOCOL')
    for stage in state['stages']:
        for name, expected in stage['artifacts'].items():
            if '/' in name or name in ('', '.', '..') or digest(read_private(root / name)) != expected:
                raise ValueError('RESUME_ARTIFACT_CHANGED')


def loaded_postgres(receipts,target,source_id,tables,secret_root,binaries):
    expected={(t['schema'],t['table']) for t in tables}
    actual=set()
    views=[]
    for receipt in receipts:
        table=receipt.get('table',{})
        identity=(table.get('schema'),table.get('name'))
        relation=receipt.get('target','')
        if (receipt.get('protocol')!='oo.load/v1' or receipt.get('source')!=source_id
                or receipt.get('run_id')!=target['run_id']
                or receipt.get('gateway_published') is not False
                or receipt.get('publication')!='local_table_view'
                or identity not in expected or identity in actual
                or not re.fullmatch(r'oo_load_data\.[a-z0-9_]+',relation)):
            raise ValueError('DRAFT_LOAD_RECEIPT')
        actual.add(identity)
        views.append({'schema':'oo_load_data','table':relation.split('.')[1]})
    if actual!=expected or len({v['table'] for v in views})!=len(views):
        raise ValueError('DRAFT_LOAD_COVERAGE')
    return {'host':target['host'],'port':target['port'],'database':target['database'],
            'username':target['user'],'password_file':str(secret_root/target['password']['secret_ref']),
            'ca_file':str(secret_root/target['tls_ca']['secret_ref']),
            'catalog_bin':str(binaries/'oo-pipeline'),'tables':views}

def main(request):
    root = Path(request['state_dir'])
    root.mkdir(mode=0o700, exist_ok=True)
    st = root.lstat()
    if not stat.S_ISDIR(st.st_mode) or st.st_uid != os.getuid() or st.st_mode & 0o777 != 0o700:
        raise ValueError('STATE_DIRECTORY')
    fd = os.open(root / 'build.lock', os.O_WRONLY | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, 'w') as lock:
        if os.fstat(lock.fileno()).st_nlink != 1:
            raise ValueError('STATE_LOCK')
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        return build(request, root)


def build(request, root):
    stack = request['stack']
    fingerprint_input = stack
    target = None
    if 'load_target' in request:
        target = json.loads(read_private(Path(request['load_target'])))
        if not target.get('tls_ca') or target.get('local_fixture'):
            raise ValueError('DRAFT_AGENT_TLS_REQUIRED')
        fingerprint_input = {'stack': stack, 'load_target': target}
    if 'builder_postgres' in request:
        fingerprint_input = {'stack': stack, 'builder_postgres': request['builder_postgres']}
    if 'community_modules' in request:
        fingerprint_input = {'input': fingerprint_input, 'builder_path': 'community-v1'}
    fingerprint = digest(json.dumps(fingerprint_input, sort_keys=True).encode())
    path = root / 'run.json'
    if request['resume']:
        state = json.loads(read_private(path))
        check_resume(state, fingerprint, root)
    else:
        if path.exists():
            raise ValueError('BUILD_EXISTS_USE_RESUME')
        state = initial(fingerprint)
    def emit():
        save(path, state)
        print(json.dumps(state), flush=True)
    state['status'] = 'running'
    emit()
    save(root / 'stack.json', stack)
    binaries = Path(request['bin_dir'])
    builder = str(binaries / 'pg-builder')
    cfg = str(root / 'builder.json')
    egress = None
    stage = None
    def invoke(argv, output, timeout=180):
        proc = subprocess.run(argv, stdin=subprocess.DEVNULL, capture_output=True, timeout=timeout)
        # Fixed error codes only. Never relay subprocess diagnostics to a TTY.
        if proc.returncode:
            raise ValueError('STAGE_COMMAND_FAILED')
        if output:
            write_private(root / output, proc.stdout)
    try:
        for stage in state['stages']:
            name = stage['name']
            if stage['status'] == 'complete' and name != 'preflight':
                continue
            stage.update(status='running', attempts=stage['attempts']+1, error=None, completed=0)
            started = time.monotonic()
            emit()
            artifacts = []
            if name == 'preflight':
                invoke([str(binaries/'oo'), 'doctor', '--config', str(root/'stack.json'), '--json'], 'doctor.json')
                report = json.loads(read_private(root/'doctor.json'))
                if report.get('status') != 'ok':
                    raise ValueError('PREFLIGHT_BLOCKED')
                if len(stack['sources']) != 1 or (target is None and 'builder_postgres' not in request
                        and stack['sources'][0]['engine'].split('-')[0] not in ('postgres','postgresql')):
                    raise ValueError('DRAFT_ENGINE_PENDING_OO_LOAD')
                source = stack['sources'][0]
                secret_root = Path.home()/'.config/open-ontology/secrets'
                tables = []
                for table in source['scope']['include']:
                    schema, sep, table_name = table.partition('.')
                    if not sep or '*' in table or '?' in table:
                        raise ValueError('DRAFT_EXPLICIT_SCOPE_REQUIRED')
                    if any(e in ('*', table, schema, schema+'.*') for e in source['scope']['exclude']):
                        continue
                    tables.append({'schema':schema, 'table':table_name})
                # The bridge is bounded; it does not silently truncate a project.
                if not 1 <= len(tables) <= 16:
                    raise ValueError('DRAFT_BRIDGE_TABLE_BUDGET')
                postgres = request.get('builder_postgres')
                if target is not None:
                    save(root/'agent-target.json',target)
                    invoke([str(binaries/'oo-load'),'--stack',str(root/'stack.json'),
                            '--source',source['id'],'--tables',
                            ','.join(t['schema']+'.'+t['table'] for t in tables),
                            '--target',str(root/'agent-target.json'),'--secrets-dir',str(secret_root)],
                           'load-receipts.jsonl',timeout=600)
                    receipts=[json.loads(line) for line in read_private(root/'load-receipts.jsonl').splitlines()]
                    postgres=loaded_postgres(receipts,target,source['id'],tables,secret_root,binaries)
                    state.update(agent_db_load='local_table_views',consistency='B',source_time='unmeasured')
                if postgres is None:
                    postgres={'host':source['host'],'port':source['port'],'database':source['database'],
                              'username':source['user'],'password_file':str(secret_root/source['password']['secret_ref']),
                              'ca_file':str(secret_root/source['tls_ca']['secret_ref']),
                              'catalog_bin':str(binaries/'oo-pipeline'),'tables':tables}
                config = {'protocol':'oo.pg-builder.config/v1', 'adapter':'loopback_http_local_dev',
                          'namespace':'pg-builder', 'purpose':'generate', 'sample_policy':'none',
                          'temperature':0, 'reasoning_effort':'low',
                          'endpoint':{'origin':'http://127.0.0.1:8317','path':'/v1/chat/completions','model_id':stack['model']['model']},
                          'secret':{'mode':'model-egress'}, 'declared_price':{'status':'unknown'},
                          'budget':{'deadline_ms':180000,'max_completion_tokens':8192,'max_in_flight':1,
                                    'max_prompt_tokens':50000,'max_request_bytes':262144,'max_response_bytes':262144,
                                    'max_tables':16,'max_total_tokens':60000},
                          'postgres':postgres}
                save(root/'builder.json', config)
                artifacts = ['doctor.json']
                if target is not None:
                    artifacts += ['load-receipts.jsonl','agent-target.json']
            elif name == 'catalog':
                source = stack['sources'][0]
                invoke([builder,'catalog','--config',cfg,'--source-id',source['id'],
                        '--secret-ref','secret://'+source['password']['secret_ref'],'--out',str(root/'catalog.json')],None)
                artifacts = ['catalog.json']
            elif name in ('profile','comments'):
                invoke([builder,name,'--config',cfg,'--out',str(root/(name+'.json'))],None)
                artifacts = [name+'.json']
            elif name == 'seal':
                invoke([builder,'seal','--config',cfg,'--catalog',str(root/'catalog.json'),'--out',str(root/'seal.json')],None)
                artifacts = ['seal.json']
            elif name == 'cards':
                argv = [builder,'cards','--config',cfg,'--out',str(root/'cards.json')]
                for item in ('catalog','profile','comments','seal'):
                    argv += ['--'+item,str(root/(item+'.json'))]
                invoke(argv,None)
                # This is permission to send metadata, NOT approval of model output.
                invoke([builder,'approve','--cards',str(root/'cards.json'),'--reviewer-label','owner-metadata-generation',
                        '--reviewer-at',time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'--out',str(root/'generation-permission.json')],None)
                artifacts = ['cards.json','generation-permission.json']
            else:
                cards = json.loads(read_private(root/'cards.json'))
                calls = sum((len(t['columns'])+7)//8 for t in cards['tables'])
                if 'community_modules' not in request and not 1 <= calls <= 2:
                    raise ValueError('DRAFT_BRIDGE_CALL_BUDGET')
                # Refuse a pre-existing listener: never send a draft to somebody else's proxy.
                import socket
                with socket.socket() as sock:
                    try:
                        sock.bind(('127.0.0.1',8317))
                    except OSError:
                        raise ValueError('EGRESS_PORT_IN_USE_USE_LITE_NETWORK_NAMESPACE') from None
                egress_path = root/'model-egress.py'
                write_private(egress_path, request['egress_code'].encode())
                egress = subprocess.Popen([sys.executable,str(egress_path),'--config',str(root/'stack.json'),
                                           '--meter',str(root/'usage.jsonl')],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
                for _ in range(100):
                    if egress.poll() is not None:
                        raise ValueError('EGRESS_START_FAILED')
                    try:
                        with socket.create_connection(('127.0.0.1',8317),timeout=0.1):
                            break
                    except OSError:
                        time.sleep(0.05)
                else:
                    raise ValueError('EGRESS_START_TIMEOUT')
                if 'community_modules' in request:
                    adapter = load_community_modules(request['community_modules'], root)
                    os.environ['OO_SELFBUILD_BUDGET_BIN'] = str(binaries/'selfbuild-budget')
                    adapter.generate(root,stack,binaries,
                        lambda p: json.loads(read_private(p)), save)
                    artifacts = ['draft.json','review-input.json']
                else:
                    invoke([builder,'generate','--config',cfg,'--seal',str(root/'seal.json'),
                            '--cards',str(root/'cards.json'),'--approval',str(root/'generation-permission.json'),
                            '--cache',str(root/'cache'),'--out',str(root/'draft.json')],None,timeout=600)
                    draft = json.loads((root/'draft.json').read_bytes())
                    if draft.get('outcome') != 'validated':
                        raise ValueError('DRAFT_NOT_VALIDATED')
                    artifacts = ['draft.json']
            stage['artifacts'] = {}
            for name_out in artifacts:
                # Existing builder writers run under this process's owner-only umask.
                stage['artifacts'][name_out] = digest(read_private(root/name_out))
            elapsed = time.monotonic()-started
            stage.update(status='complete',completed=1,elapsed_seconds=elapsed,operations_per_second=1/max(elapsed,0.000001))
            emit()
        state.update(status='draft', last_success=time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()), draft='draft.json')
        emit()
        return 0
    except (Exception, KeyboardInterrupt) as error:
        code = str(error) if isinstance(error, ValueError) else 'BUILD_INTERRUPTED' if isinstance(error,(InterruptedError,KeyboardInterrupt)) else 'BUILD_EXECUTION_FAILED'
        if stage:
            stage.update(status='failed',error=code,elapsed_seconds=time.monotonic()-started)
        state.update(status='failed',last_failure=time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()))
        emit()
        return 1
    finally:
        if egress and egress.poll() is None:
            egress.terminate()
            try:
                egress.wait(timeout=5)
            except subprocess.TimeoutExpired:
                egress.kill()
                egress.wait()


if __name__ == '__main__':
    os.umask(0o077)
    def interrupted(_sig, _frame):
        raise InterruptedError()
    signal.signal(signal.SIGTERM, interrupted)
    try:
        request = json.loads(sys.stdin.buffer.read(2097153))
        raise SystemExit(main(request))
    except (Exception, KeyboardInterrupt):
        print(json.dumps({'error':'BUILD_STATE_OR_CONFIGURATION'}),flush=True)
        raise SystemExit(1)
