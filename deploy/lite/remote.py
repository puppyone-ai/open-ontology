"""Remote lite agent, run over SSH by `oo up|down|status --target ssh`.

The CLI sends this file on stdin through a fixed `python3 -c` loader, then one
JSON request line, then (for `load`) a `docker save` stream. Secrets arrive only
on stdin and are written as 0600 files; they never reach argv, the environment
of other processes or shell history. Everything is written under
~/.open-ontology plus one systemd timer/service pair; `down` removes exactly
what the manifest records. The agent never runs sudo.
"""
import fcntl
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import time

HOME = Path(os.path.expanduser('~'))
BASE = HOME / '.open-ontology'
ROOT = BASE / 'lite'
SOURCE_HOME = BASE / 'source-home'
SECRETS = SOURCE_HOME / '.config/open-ontology/secrets'
BIN = BASE / 'bin'
MANIFEST = BASE / 'manifest.json'
CYCLE_STATUS = BASE / 'cycle-status.json'
UNIT = 'open-ontology-lite-cycle'
NAME = re.compile(r'[a-zA-Z0-9][a-zA-Z0-9_.-]{0,127}')
PROTOCOL = 'oo.lite-remote/v1'


class Refused(Exception):
    """A stable, value-free error code."""


def run(argv, timeout=180, **kw):
    return subprocess.run(argv, capture_output=True, timeout=timeout, **kw)


def ok(argv, code, timeout=180, **kw):
    p = run(argv, timeout, **kw)
    if p.returncode:
        raise Refused(code)
    return p.stdout


def private_dir(path):
    path.mkdir(mode=0o700, exist_ok=True)
    st = path.lstat()
    if not path.is_dir() or path.is_symlink() or st.st_uid != os.getuid():
        raise Refused('REMOTE_PRIVATE_DIRECTORY')
    if st.st_mode & 0o777 != 0o700:
        os.chmod(path, 0o700)


def read_private(path):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, 'rb') as stream:
        st = os.fstat(stream.fileno())
        if (not (st.st_mode & 0o170000 == 0o100000) or st.st_uid != os.getuid()
                or st.st_nlink != 1 or st.st_mode & 0o777 != 0o600):
            raise Refused('REMOTE_PRIVATE_FILE')
        return stream.read(4 * 1048576)


def put(path, data, changed, immutable=False):
    """Write 0600 atomically. Immutable files (credentials) are never replaced."""
    if path.exists() or path.is_symlink():
        if read_private(path) == data:
            return
        if immutable:
            raise Refused('REMOTE_CREDENTIAL_CHANGE_REQUIRES_EXPLICIT_ROTATION')
    tmp = path.with_name('.' + path.name + '.tmp-' + str(os.getpid()))
    fd = os.open(tmp, os.O_CREAT | os.O_EXCL | os.O_WRONLY | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, 'wb') as stream:
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(tmp, path)
    changed.append(str(path.relative_to(HOME)) if path.is_relative_to(HOME) else str(path))


def load_manifest():
    if MANIFEST.exists():
        return json.loads(read_private(MANIFEST))
    return {'protocol': PROTOCOL, 'images': [], 'units': [], 'scope': None}


def save_manifest(manifest):
    put(MANIFEST, json.dumps(manifest, sort_keys=True).encode(), [])


def lock():
    private_dir(BASE)
    fd = os.open(BASE / '.lock', os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
    fcntl.flock(fd, fcntl.LOCK_EX)
    return fd


def image_info(ref, platform=None):
    select = ['--platform', platform] if platform else []
    p = run(['docker', 'image', 'inspect', *select, '--format',
             '{{json .Id}} {{json .Os}} {{json .Architecture}} {{json .RootFS.Layers}}', ref])
    if p.returncode:
        return None
    ident, system, arch, layers = p.stdout.decode().split(' ', 3)
    return {'id': json.loads(ident), 'os': json.loads(system), 'arch': json.loads(arch),
            'layers': json.loads(layers)}

def load_verified(stdin, expect):
    """Shared remote/offline loader: config identity includes entrypoint and env."""
    p = run(['docker', 'load'], 1800, stdin=stdin)
    if p.returncode:
        raise Refused('REMOTE_IMAGE_LOAD')
    loaded = [line.split(':', 1)[1].strip() for line in p.stdout.decode().splitlines()
              if line.startswith(('Loaded image ID:', 'Loaded image:'))]
    if len(loaded) != 1:
        raise Refused('REMOTE_IMAGE_LOAD_PROTOCOL')
    info = image_info(loaded[0])
    # Legacy SSH callers only know the sender's store-dependent ID. Bundles also
    # supply the verified config digest, so they can enforce identity across stores.
    if (not info or any(info[key] != expect[key] for key in ('os', 'arch', 'layers'))
            or ('config_id' in expect and info['id'] not in
                (expect['id'], expect['config_id'], expect.get('source_id')))):
        raise Refused('REMOTE_IMAGE_CONTENT_MISMATCH')
    return info, loaded[0]


def systemctl(scope):
    return ['systemctl'] + (['--user'] if scope == 'user' else [])


def unit_dir(scope):
    if scope == 'system':
        return Path('/etc/systemd/system')
    return HOME / '.config/systemd/user'


def probe(_request, _stdin):
    machine = platform.machine()
    arch = {'x86_64': 'amd64', 'amd64': 'amd64', 'aarch64': 'arm64', 'arm64': 'arm64'}.get(machine)
    missing = []
    report = {'uid': os.getuid(), 'arch': arch, 'python': platform.python_version(),
              'python3': shutil.which('python3') or sys.executable,
              'docker': None, 'compose': None, 'systemd': None}
    if not arch:
        missing.append('linux_amd64_or_arm64')
    if platform.system() != 'Linux':
        missing.append('linux')
    if not shutil.which('docker'):
        missing.append('docker')
    else:
        p = run(['docker', 'version', '--format', '{{.Server.Version}}'], 30)
        if p.returncode:
            missing.append('docker_daemon_access')
        else:
            report['docker'] = p.stdout.decode().strip()
            c = run(['docker', 'compose', 'version', '--short'], 30)
            if c.returncode:
                missing.append('docker_compose_plugin')
            else:
                report['compose'] = c.stdout.decode().strip()
            info = run(['docker', 'info', '--format', '{{.DockerRootDir}}'], 30)
            root = info.stdout.decode().strip() if not info.returncode else '/'
            try:
                report['docker_root_free_bytes'] = shutil.disk_usage(root).free
            except OSError:
                report['docker_root_free_bytes'] = shutil.disk_usage('/').free
            if report['docker_root_free_bytes'] < 4 * 1024 ** 3:
                missing.append('docker_root_4gib_free')
    scope = 'system' if os.getuid() == 0 else 'user'
    if not (Path('/run/systemd/system').is_dir() and shutil.which('systemctl')):
        missing.append('systemd')
    else:
        p = run(systemctl(scope) + ['show-environment'], 30)
        if p.returncode:
            missing.append('systemd_user_manager' if scope == 'user' else 'systemd')
        else:
            v = run(['systemctl', '--version'], 30).stdout.decode().split()
            report['systemd'] = v[1] if len(v) > 1 else 'unknown'
    report.update(scope=scope, missing=missing, ready=not missing)
    if missing:
        report['guidance'] = (
            'Install Docker Engine with the compose plugin and systemd on the target, give the '
            'SSH user Docker access (or use root), and for non-root users enable lingering '
            '(loginctl enable-linger). oo never runs sudo.')
    report['images'] = {}
    return report


def images(request, _stdin):
    return {'images': {ref: image_info(ref) for ref in request['refs']}}


def load(request, stdin):
    """Load a `docker save` stream from stdin, verify content, tag deterministically."""
    tag, expect = request['tag'], request['expect']
    if not re.fullmatch(r'open-ontology/lite-(runtime|postgres):[0-9a-f]{12}-(amd64|arm64)', tag):
        raise Refused('REMOTE_IMAGE_TAG')
    with lock_held():
        manifest = load_manifest()
        info, loaded = load_verified(stdin, expect)
        ok(['docker', 'image', 'tag', info['id'], tag], 'REMOTE_IMAGE_TAG')
        if loaded != info['id'] and loaded != tag:
            # A tag carried inside the archive is ours too; never keep it silently.
            manifest['images'].append(loaded)
        manifest['images'].append(tag)
        save_manifest(manifest)
    return {'image': info['id'], 'tag': tag}


def pull(request, _stdin):
    ref = request['ref']
    if not re.fullmatch(r'[a-z0-9./_-]+@sha256:[0-9a-f]{64}', ref):
        raise Refused('REMOTE_IMAGE_REFERENCE')
    if image_info(ref):
        return {'image': ref, 'pulled': False}
    with lock_held():
        manifest = load_manifest()
        if run(['docker', 'pull', '--quiet', ref], 1800).returncode or not image_info(ref):
            return {'image': ref, 'pulled': False, 'error': 'REMOTE_IMAGE_PULL'}
        manifest['images'].append(ref)
        save_manifest(manifest)
    return {'image': ref, 'pulled': True}


class lock_held:
    def __enter__(self):
        self.fd = lock()

    def __exit__(self, *_):
        os.close(self.fd)


def project_containers(project):
    raw = run(['docker', 'ps', '-aq', '--no-trunc', '--filter',
               'label=com.docker.compose.project=' + project]).stdout.decode().split()
    result = {}
    for cid in raw:
        p = run(['docker', 'inspect', '--format', '{{json .Name}} {{json .State.StartedAt}}', cid])
        if not p.returncode:
            result[cid] = p.stdout.decode().strip()
    return result


def lifecycle(action, request=None):
    """Run the same lite lifecycle as local `oo up`, rooted in ~/.open-ontology."""
    env = {k: v for k, v in os.environ.items() if k in ('PATH', 'DOCKER_HOST', 'DOCKER_CONTEXT', 'TMPDIR')}
    # Secret references resolve from the deployment's private source home;
    # Docker CLI configuration (plugins, contexts) stays the user's own.
    env['HOME'] = str(SOURCE_HOME)
    env['DOCKER_CONFIG'] = os.environ.get('DOCKER_CONFIG', str(HOME / '.docker'))
    env.setdefault('PATH', '/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin')
    body = dict(request or {}, action=action, root=str(ROOT))
    if action != 'up':
        body.setdefault('image', '')
        body.setdefault('templates', {})
        stack = BASE / 'stack.json'
        body.setdefault('stack', json.loads(read_private(stack)) if stack.exists() else {})
    p = run([sys.executable, str(BIN / 'lifecycle.py')], 900, input=json.dumps(body).encode(), env=env)
    try:
        report = json.loads(p.stdout)
    except ValueError:
        report = {'status': 'failed', 'error': 'LITE_LIFECYCLE_PROTOCOL', 'healthy': False}
    return report, p.returncode


def units(python, interval):
    service = (f'[Unit]\nDescription=Open Ontology lite scheduled cycle\n'
               f'After=docker.service\n\n[Service]\nType=oneshot\n'
               f'ExecStart={python} {BIN / "oo-remote.py"} cycle\n')
    timer = (f'[Unit]\nDescription=Open Ontology lite scheduled cycle timer\n\n'
             f'[Timer]\nOnActiveSec={interval}s\nOnUnitActiveSec={interval}s\n'
             f'Unit={UNIT}.service\n\n[Install]\nWantedBy=timers.target\n')
    return {UNIT + '.service': service, UNIT + '.timer': timer}


def timer_status(scope):
    if not scope:
        return {'installed': False}
    path = unit_dir(scope) / (UNIT + '.timer')
    p = run(systemctl(scope) + ['show', UNIT + '.timer', '--property',
                                'UnitFileState,ActiveState,NextElapseUSecRealtime,'
                                'NextElapseUSecMonotonic,LastTriggerUSec'], 30)
    props = dict(line.split('=', 1) for line in p.stdout.decode().splitlines() if '=' in line)
    cycle = json.loads(read_private(CYCLE_STATUS)) if CYCLE_STATUS.exists() else None
    listed = run(systemctl(scope) + ['list-timers', '--all', '--no-pager', '--no-legend',
                                     UNIT + '.timer'], 30).stdout.decode().split()
    # Monotonic timers have no NextElapseUSecRealtime; list-timers shows wall time.
    wall = ' '.join(listed[:4]) if len(listed) > 4 and listed[0][:3].isalpha() else None
    return {'installed': path.exists(), 'scope': scope, 'unit': UNIT + '.timer',
            'enabled': props.get('UnitFileState') == 'enabled',
            'active': props.get('ActiveState') == 'active',
            'next_elapse': props.get('NextElapseUSecRealtime') or wall,
            'next_elapse_monotonic': props.get('NextElapseUSecMonotonic') or None,
            'last_trigger': props.get('LastTriggerUSec') or None,
            'last_cycle': cycle}


def install(request, _stdin):
    changed = []
    with lock_held():
        manifest = load_manifest()
        for path in (BASE, ROOT, ROOT / 'pki', SOURCE_HOME, SOURCE_HOME / '.config',
                     SOURCE_HOME / '.config/open-ontology', SECRETS, BIN):
            private_dir(path)
        for group, directory in (('pki', ROOT / 'pki'), ('secrets', SECRETS)):
            for name, data in sorted(request[group].items()):
                if not NAME.fullmatch(name):
                    raise Refused('REMOTE_SECRET_NAME')
                put(directory / name, bytes.fromhex(data), changed, immutable=True)
        put(BIN / 'lifecycle.py', request['lifecycle'].encode(), changed)
        put(BIN / 'oo-remote.py', request['agent'].encode(), changed)
        put(BASE / 'stack.json', json.dumps(request['stack'], sort_keys=True).encode(), changed)
        owner = ROOT / 'owner.json'
        project = json.loads(read_private(owner))['project'] if owner.exists() else None
        before = project_containers(project) if project else {}
        body = {'stack': request['stack'], 'image': request['image'], 'templates': request['templates']}
        # D16: the CLI asks for the retained build chain; prompt paths are its own.
        for key in ('retained', 'jobs', 'cycle_events', 'prompt', 'prompt_npm_registry'):
            if key in request:
                body[key] = request[key]
        report, code = lifecycle('up', body)
        project = json.loads(read_private(owner))['project'] if owner.exists() else None
        after = project_containers(project) if project else {}
        if before != after:
            changed.append('containers')
        scope = request['scope']
        if report.get('status') != 'failed':
            directory = unit_dir(scope)
            directory.mkdir(parents=True, exist_ok=True, mode=0o755)
            written = False
            for name, text in units(request['python3'], int(request['interval_seconds'])).items():
                path = directory / name
                if not path.exists() or path.read_text() != text:
                    tmp = path.with_name('.' + name + '.tmp')
                    tmp.write_text(text)
                    os.chmod(tmp, 0o644)
                    os.replace(tmp, path)
                    changed.append(str(path))
                    written = True
                if str(path) not in manifest['units']:
                    manifest['units'].append(str(path))
            manifest['scope'] = scope
            save_manifest(manifest)
            if written:
                ok(systemctl(scope) + ['daemon-reload'], 'REMOTE_SYSTEMD_RELOAD', 60)
            state = timer_status(scope)
            if not (state['enabled'] and state['active']):
                ok(systemctl(scope) + ['enable', '--now', UNIT + '.timer'], 'REMOTE_TIMER_ENABLE', 60)
                changed.append('timer_state')
            elif written:
                ok(systemctl(scope) + ['restart', UNIT + '.timer'], 'REMOTE_TIMER_ENABLE', 60)
    return {'lifecycle': report, 'lifecycle_exit': code, 'changed': changed,
            'timer': timer_status(manifest.get('scope'))}


def status(_request, _stdin):
    if not (ROOT / 'owner.json').exists():
        return {'lifecycle': {'status': 'not_started', 'services': [], 'healthy': False,
                              'gateway_health': {'status': 'unmeasured'}},
                'lifecycle_exit': 0, 'timer': {'installed': False}, 'installed': BASE.exists()}
    with lock_held():
        report, code = lifecycle('status')
        manifest = load_manifest()
    return {'lifecycle': report, 'lifecycle_exit': code, 'timer': timer_status(manifest.get('scope')),
            'installed': True}


def owned(kind, name, project):
    fmt = '{{json .Config.Labels}}' if kind == 'container' else '{{json .Labels}}'
    p = run(['docker', kind, 'inspect', '--format', fmt, name])
    labels = json.loads(p.stdout) if not p.returncode else None
    return bool(labels) and labels.get('oo.lane') == 'a' and labels.get(
        'com.docker.compose.project') == project


def leftovers(project):
    found = {}
    if not project:
        return found
    for kind in ('container', 'volume', 'network'):
        args = ['docker', 'ps', '-aq'] if kind == 'container' else ['docker', kind, 'ls', '-q']
        names = run(args + ['--filter', 'label=com.docker.compose.project=' + project]).stdout.decode().split()
        if names:
            found[kind] = len(names)
    return found


def down(_request, _stdin):
    removed = {'units': [], 'images': [], 'volumes': 0, 'networks': 0, 'containers': 0}
    if not BASE.exists():
        return {'status': 'absent', 'removed': removed, 'remaining': {}}
    fd = lock()
    manifest = load_manifest()
    scope = manifest.get('scope')
    if scope:
        run(systemctl(scope) + ['disable', '--now', UNIT + '.timer'], 60)
        run(systemctl(scope) + ['stop', UNIT + '.service'], 120)
    for unit in manifest['units']:
        path = Path(unit)
        if path.parent == unit_dir(scope) and path.name.startswith(UNIT) and path.exists():
            path.unlink()
            removed['units'].append(unit)
    if scope and removed['units']:
        run(systemctl(scope) + ['daemon-reload'], 60)
        run(systemctl(scope) + ['reset-failed', UNIT + '.service', UNIT + '.timer'], 60)
    owner = ROOT / 'owner.json'
    project = json.loads(read_private(owner))['project'] if owner.exists() else None
    report = None
    if project:
        report, _ = lifecycle('down')
        for kind in ('container', 'volume', 'network'):
            args = ['docker', 'ps', '-aq'] if kind == 'container' else ['docker', kind, 'ls', '-q']
            names = run(args + ['--filter', 'label=com.docker.compose.project=' + project]).stdout.decode().split()
            for name in names:
                if not owned(kind, name, project):
                    raise Refused('REMOTE_OWNERSHIP')
                rm = ['docker', 'rm', '-f', name] if kind == 'container' else ['docker', kind, 'rm', name]
                ok(rm, 'REMOTE_REMOVE_' + kind.upper())
                removed[kind + 's'] += 1
    for ref in reversed(manifest['images']):
        if image_info(ref) and not run(['docker', 'image', 'rm', ref]).returncode:
            removed['images'].append(ref)
    shutil.rmtree(BASE)
    os.close(fd)
    remaining = leftovers(project)
    if BASE.exists():
        remaining['files'] = True
    remaining_images = [ref for ref in manifest['images'] if image_info(ref)]
    if remaining_images:
        remaining['images'] = remaining_images
    if any((unit_dir(scope) / (UNIT + suffix)).exists() for suffix in ('.service', '.timer')) if scope else False:
        remaining['units'] = True
    return {'status': 'removed', 'removed': removed, 'remaining': remaining,
            'lifecycle': report}


def cycle():
    """One scheduled cycle through the existing scheduler dispatch, never invented success."""
    fd = lock()
    try:
        schedule = ROOT / 'work/operator/schedule.json'
        at = time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())
        if not schedule.exists():
            result = {'status': 'not_configured', 'at': at,
                      'reason': 'work/operator/schedule.json is not installed'}
            put(CYCLE_STATUS, json.dumps(result).encode(), [])
            return 0
        owner = json.loads(read_private(ROOT / 'owner.json'))
        env = {k: v for k, v in os.environ.items() if k in ('PATH', 'DOCKER_HOST', 'DOCKER_CONTEXT', 'TMPDIR')}
        env.update(HOME=str(HOME), OO_LITE_STATE=str(ROOT), OO_LITE_RUNTIME_IMAGE=owner['image'],
                   OO_LITE_UID=str(os.getuid()), OO_LITE_GID=str(os.getgid()),
                   OO_LITE_SUBNET=owner['subnet'])
        env.setdefault('PATH', '/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin')
        # One enqueue and one claim through the scheduler's own functions; the
        # worker's volume lock keeps this from overlapping the worker service.
        code = ('import json,sys;sys.path.insert(0,"/work");import scheduler;'
                'c=json.load(open("/work/operator/schedule.json"));'
                'scheduler.fire(c);r=scheduler.work(c);print(json.dumps(r));'
                'sys.exit(1 if r.get("stage") in ("failed","review_required") else 0)')
        p = run(['docker', 'compose', '--project-name', owner['project'], '--file', str(ROOT / 'compose.yaml'),
                 'run', '--rm', '--no-deps', '--entrypoint', 'python3', 'scheduler', '-c', code], 3600, env=env)
        result = {'status': 'dispatched' if p.returncode == 0 else 'failed', 'at': at,
                  'exit_code': p.returncode,
                  'note': 'dispatch success is not job completion; see control DB task events'}
        put(CYCLE_STATUS, json.dumps(result).encode(), [])
        return 0 if p.returncode == 0 else 1
    finally:
        os.close(fd)


BUILD_SECONDS = 4 * 3600


def service_id(project, service):
    raw = run(['docker', 'ps', '-q', '--filter', 'label=com.docker.compose.project=' + project,
               '--filter', 'label=com.docker.compose.service=' + service,
               '--filter', 'status=running']).stdout.decode().split()
    return raw[0] if len(raw) == 1 else None


def build_state(cycle):
    state = ROOT / 'work/build/.oo'
    chain = (state / 'cycles' / cycle / 'chain.json') if cycle else state / 'chain.json'
    return state, chain


def build(request, _stdin):
    """One phase of the runtime build (D16), the same container as local lite."""
    cycle = request.get('cycle')
    if cycle is not None and not re.fullmatch('[A-Za-z0-9-]{1,100}', cycle):
        raise Refused('REMOTE_BUILD_CYCLE')
    if not (ROOT / 'work/operator/build-runtime.json').exists():
        raise Refused('BUILD_LITE_NOT_PROVISIONED')
    owner = json.loads(read_private(ROOT / 'owner.json'))
    project, image, lane = owner['project'], owner['image'], owner.get('lane', 'a')
    state, chain = build_state(cycle)
    if request.get('fresh') and chain.exists():
        raise Refused('BUILD_EXISTS_USE_RESUME')
    if request.get('run', True):
        name = project + '-build'
        # A disconnected client leaves its phase running; never start a second.
        p = run(['docker', 'inspect', '--format', '{{.State.Running}}', name])
        if p.returncode == 0:
            if p.stdout.startswith(b'true'):
                run(['docker', 'wait', name], BUILD_SECONDS)
            run(['docker', 'rm', '-f', name])
        if not service_id(project, 'agent-db'):
            raise Refused('BUILD_LITE_SERVICE_NOT_RUNNING')
        gateway = service_id(project, 'gateway')
        argv = ['docker', 'run', '--rm', '--name', name, '--label', 'oo.lane=' + lane,
                '--label', 'oo.lite.project=' + project,
                '--network', 'container:' + gateway if gateway else project + '_default',
                '--user', f'{os.getuid()}:{os.getgid()}', '--read-only', '--tmpfs', '/tmp',
                '--cap-drop', 'ALL', '--security-opt', 'no-new-privileges:true',
                '--memory', '2g', '--pids-limit', '512',
                '--volume', f'{ROOT}/work:/work', '--volume', f'{ROOT}/home:/home/oo:ro',
                '--volume', f'{ROOT}/pki:/pki:ro', '--env', 'HOME=/home/oo',
                '--env', 'PYTHONDONTWRITEBYTECODE=1', '--entrypoint', 'oo', image,
                'build', '--config', '/work/build/stack.json', '--json',
                '--agent-target', '/work/operator/target.json',
                '--runtime-config', '/work/operator/build-runtime.json']
        if not request.get('fresh'):
            argv.append('--resume')
        if cycle:
            argv += ['--cycle', cycle]
        p = run(argv, BUILD_SECONDS)
        if p.returncode > 1:
            raise Refused('BUILD_RUNTIME_FAILED')
    reply = {'chain': json.loads(read_private(chain)), 'image': image}
    if reply['chain'].get('status') == 'complete':
        receipt = state / 'share_prompt-receipt.json'
        reply['prompt'] = json.loads(read_private(receipt))['result']['prompt']
    elif not cycle and (state / 'review-input.json').exists() and not (state / 'reviewed-registry.json').exists():
        reply['review_input'] = json.loads(read_private(state / 'review-input.json'))
    return reply


def handoff(request, _stdin):
    """Accept the owner's locally reviewed registry; the runtime verifies it."""
    state, _ = build_state(None)
    data = json.dumps(request['reviewed']).encode()
    path = state / 'reviewed-registry.json'
    if path.exists():
        if json.loads(read_private(path)) != request['reviewed']:
            raise Refused('BUILD_REVIEW_INPUT_CHANGED')
    else:
        put(path, data, [])
    owner = json.loads(read_private(ROOT / 'owner.json'))
    agent = service_id(owner['project'], 'agent-db')
    if not agent:
        raise Refused('BUILD_LITE_SERVICE_NOT_RUNNING')
    sql = b'GRANT EXECUTE ON FUNCTION _oo.native_library_retained_table(text) TO "oo_gateway_login";\n'
    p = run(['docker', 'exec', '-i', agent, 'psql', '-X', '-q', '-v', 'ON_ERROR_STOP=1', '-U', 'postgres'],
            60, input=sql)
    if p.returncode:
        raise Refused('BUILD_LITE_RETAINED_GRANT')
    return {'handed_off': True}


ACTIONS = {'probe': probe, 'images': images, 'load': load, 'pull': pull,
           'install': install, 'status': status, 'down': down, 'build': build, 'handoff': handoff}


def serve():
    stdin = sys.stdin.buffer.raw
    line = bytearray()
    while not line.endswith(b'\n'):
        chunk = stdin.read(1)
        if not chunk:
            break
        line += chunk
        if len(line) > 16 * 1048576:
            raise Refused('REMOTE_REQUEST_BUDGET')
    request = json.loads(line)
    if request.get('protocol') != PROTOCOL or request.get('action') not in ACTIONS:
        raise Refused('REMOTE_PROTOCOL')
    return ACTIONS[request['action']](request, stdin)


def main():
    os.umask(0o077)
    if sys.argv[1:] == ['cycle']:
        return cycle()
    try:
        result = {'protocol': PROTOCOL, 'ok': True, **serve()}
        code = 0
    except Refused as error:
        result = {'protocol': PROTOCOL, 'ok': False, 'error': str(error)}
        code = 3
    except Exception as error:  # never relay messages: they may contain paths or values
        result = {'protocol': PROTOCOL, 'ok': False, 'error': 'REMOTE_AGENT_FAILED',
                  'detail': type(error).__name__}
        code = 3
    sys.stdout.write(json.dumps(result) + '\n')
    sys.stdout.flush()
    return code


if __name__ == '__main__':
    raise SystemExit(main())
