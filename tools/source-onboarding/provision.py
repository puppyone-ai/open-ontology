#!/usr/bin/env python3
"""Provision one MySQL source into a conversational scope.

Contract: specs/chunked-baseline/ONBOARDING-API.md (sections 1-4).
Commands: start | status | review | decide | resume | cancel  (+ internal _runner).

Every phase is journaled in RUN/journal.jsonl. A phase is skipped on resume only when
its last journal event is `succeeded` AND RUN/phases/<name>.json exists. S9/S10 are never
approved automatically: `start` stops at awaiting_review, `decide` needs decisions that
cover every pending item.

Passwords: copied once into the trusted container's secret mount
(<secret_root>/provision/<run_id>/password.pass, 0600), registered in
RUN/private/secret-registry.json (secret_ref -> path). Never in argv, logs, journal or status.
"""
import argparse
import datetime
import fcntl
import hashlib
import json
import os
import re
import secrets
import signal
import socket
import stat
import struct
import subprocess
import sys
import time
import uuid
from pathlib import Path

HERE = Path(__file__).resolve().parent
SITE_PATH = HERE / 'provision' / 'site-v1.json'
PRODUCT_FLOW = HERE / 'product_flow.py'

SHARED_PROVISION = Path('/tmp/oo-mysql-connection-whole/provision')

PROTOCOL_STATUS = 'oo.source-onboarding.provision-status/v1'
PROTOCOL_REVIEW = 'oo.source-onboarding.provision-review/v1'
PROTOCOL_RUN = 'oo.source-onboarding.provision-run/v1'
PROTOCOL_HIDDEN = 'oo.source-onboarding.hidden-declaration/v1'
PROTOCOL_PII = 'oo.source-onboarding.pii-rebuild/v1'
MAX_PII_BUILDS = 3
MAX_ORACLE_BUILDS = 2
SEED_CONTENT_VERSION = 2  # 2: position_lineage from baseline_scope.lineage, quality from the typed oracle
ORACLE_RETRY_CODES = ('follow_failed:actual_api_refused',)
ROUND_TAIL = ('awaiting_review', 'review', 'pack', 'publish', 'replica_check', 'gateway', 'token', 'smoke', 'ready')

PHASES = ['validate', 'egress', 'identity', 'target', 'capture', 'artifacts', 'product_runtime',
          's0_s8', 'pii_rebuild', 'awaiting_review', 'review', 'pack', 'publish', 'oracle_rebuild', 'replica_check', 'gateway',
          'token', 'smoke', 'ready']
STAGES = ['S%d' % i for i in range(11)]
SIX_KEYS = ('type', 'host', 'port', 'database', 'user', 'secret_ref')
FORBIDDEN_KEYS = {'password', 'passwd', 'pass', 'pwd', 'secret', 'token', 'api_key', 'authorization'}
FORMATS = ('v1', 'chunked-typed/v2')
FORMAT_PROFILES = {'chunked-typed/v2': HERE / 'provision' / 'site-v2.json'}
PROTOCOL_PROFILE = 'oo.source-onboarding.site-profile/v1'
PRODUCER_FORWARD_HOST = 'host.docker.internal'
EGRESS_MODES = ('direct', 'eip')
PENDING_STATES = {'pending'}
HEX64 = re.compile(r'^[0-9a-f]{64}$')
HOST_RE = re.compile(r'^[A-Za-z0-9][A-Za-z0-9.\-]{0,252}$')
IDENT_RE = re.compile(r'^[A-Za-z0-9_]{1,64}$')
USER_RE = re.compile(r'^[A-Za-z0-9_.\-]{1,64}$')
REF_RE = re.compile(r'^secret://[A-Za-z0-9._/\-]{1,128}$')
DECISION_VALUES = {'confirmed', 'rejected'}
MAX_PASSWORD_BYTES = 4096


class Refused(Exception):
    """A stable, public error code. Never carries input values."""

    def __init__(self, code):
        super().__init__(code)
        self.code = code


def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def digest_file(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def write_private(path, data):
    """Create a new 0600 file (replace an existing one). Data is bytes or str."""
    path = Path(path)
    if path.is_symlink():
        raise Refused('private_file')
    tmp = path.with_name(path.name + '.tmp-%d' % os.getpid())
    if tmp.exists():
        tmp.unlink()
    fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, 'wb') as handle:
        handle.write(data if isinstance(data, bytes) else data.encode('utf-8'))
        handle.flush()
        os.fsync(handle.fileno())
    os.replace(tmp, path)
    os.chmod(path, 0o600)


def write_json(path, value):
    write_private(path, (json.dumps(value, sort_keys=True, indent=2, ensure_ascii=False) + '\n').encode('utf-8'))


def write_canonical_json(path, value):
    """Compact sorted JSON + LF: the bytes the producer writes. The API pins catalog.json by raw SHA-256."""
    write_private(path, (json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False) + '\n').encode('utf-8'))


def private_file(path):
    p = Path(os.path.abspath(str(path)))
    try:
        st = os.lstat(p)
    except OSError:
        raise Refused('private_file')
    if not stat.S_ISREG(st.st_mode) or st.st_uid != os.getuid() or st.st_mode & 0o077:
        raise Refused('private_file')
    return p


def owned_public_file(path):
    """Public material (e.g. a CA certificate): regular, owned by us, not symlinked, not group/other-writable."""
    p = Path(os.path.abspath(str(path)))
    try:
        st = os.lstat(p)
    except OSError:
        raise Refused('ca_file_invalid')
    if not stat.S_ISREG(st.st_mode) or st.st_uid != os.getuid() or st.st_mode & 0o022:
        raise Refused('ca_file_invalid')
    return p


def load_private_json(path, code='six_fields_invalid'):
    raw = private_file(path).read_bytes()
    if len(raw) > 1 << 20:
        raise Refused(code)
    try:
        return json.loads(raw.decode('utf-8'), object_pairs_hook=_no_duplicate_keys)
    except (UnicodeError, ValueError):
        raise Refused(code)


def _no_duplicate_keys(pairs):
    out = {}
    for key, value in pairs:
        if key in out:
            raise Refused('six_fields_invalid')
        out[key] = value
    return out


def read_password(path):
    """Return the password bytes (one trailing newline stripped). Never print the result."""
    data = private_file(path).read_bytes()
    if data.endswith(b'\r\n'):
        data = data[:-2]
    elif data.endswith(b'\n'):
        data = data[:-1]
    if not data or len(data) > MAX_PASSWORD_BYTES or any(b in (0x00, 0x0a, 0x0d) for b in data):
        raise Refused('password_file_invalid')
    return data


def validate_six(doc):
    if not isinstance(doc, dict):
        raise Refused('six_fields_invalid')
    if any(str(key).lower() in FORBIDDEN_KEYS for key in doc):
        raise Refused('inline_password_refused')
    if set(doc) != set(SIX_KEYS):
        raise Refused('six_fields_invalid')
    if doc['type'] != 'mysql':
        raise Refused('six_fields_invalid')
    port = doc['port']
    if not isinstance(port, int) or isinstance(port, bool) or not 1 <= port <= 65535:
        raise Refused('six_fields_invalid')
    for key, rx in (('host', HOST_RE), ('database', IDENT_RE), ('user', USER_RE), ('secret_ref', REF_RE)):
        value = doc[key]
        if not isinstance(value, str) or not rx.match(value) or '..' in value:
            raise Refused('six_fields_invalid')
    return {key: doc[key] for key in SIX_KEYS}


def validate_replica(doc):
    if not isinstance(doc, dict):
        raise Refused('replica_invalid')
    if any(str(key).lower() in FORBIDDEN_KEYS for key in doc):
        raise Refused('inline_password_refused')
    allowed = {'host', 'port', 'database', 'user', 'secret_ref'}
    if not set(doc) <= allowed or not {'host', 'port', 'database'} <= set(doc):
        raise Refused('replica_invalid')
    if not isinstance(doc['host'], str) or not HOST_RE.match(doc['host']):
        raise Refused('replica_invalid')
    if not isinstance(doc['port'], int) or isinstance(doc['port'], bool) or not 1 <= doc['port'] <= 65535:
        raise Refused('replica_invalid')
    if not isinstance(doc['database'], str) or not IDENT_RE.match(doc['database']):
        raise Refused('replica_invalid')
    if 'user' in doc and (not isinstance(doc['user'], str) or not USER_RE.match(doc['user'])):
        raise Refused('replica_invalid')
    if 'secret_ref' in doc and (not isinstance(doc['secret_ref'], str) or not REF_RE.match(doc['secret_ref'])
                                or '..' in doc['secret_ref']):
        raise Refused('replica_invalid')
    return dict(doc)


PROFILE_KEYS = ('user', 'secret_ref')


def validate_profile(doc, six):
    """Optional profile credential for the source-SQL profile step (SELECT-only account; never the CDC account)."""
    if not isinstance(doc, dict) or set(doc) != set(PROFILE_KEYS):
        raise Refused('profile_invalid')
    if any(str(key).lower() in FORBIDDEN_KEYS for key in doc):
        raise Refused('inline_password_refused')
    user, ref = doc['user'], doc['secret_ref']
    if not isinstance(user, str) or not USER_RE.match(user) or '..' in user:
        raise Refused('profile_invalid')
    if not isinstance(ref, str) or not REF_RE.match(ref) or '..' in ref:
        raise Refused('profile_invalid')
    if user == six['user'] or ref == six['secret_ref']:
        raise Refused('profile_same_as_six')
    return {'user': user, 'secret_ref': ref}


def choose_format(value):
    if value in (None, 'v1'):
        return 'v1'
    if value in FORMATS:
        return value
    raise Refused('format_invalid')


def validate_label(label, six):
    if label is None:
        return '%s/%s' % (six['host'], six['database'])
    if not isinstance(label, str) or not 1 <= len(label) <= 120 or any(ord(ch) < 32 for ch in label):
        raise Refused('label_invalid')
    return label


def load_site(path=SITE_PATH):
    site = json.loads(Path(path).read_text(encoding='utf-8'))
    if site.get('protocol') != 'oo.source-onboarding.site/v1':
        raise Refused('site_config_invalid')
    return site


def site_for_format(site, fmt):
    """The site with the binary pins of `fmt`. v1 is the site file itself. Another format replaces only the
    binaries its profile names, so the v1 pins never change."""
    if fmt == 'v1':
        return site
    path = FORMAT_PROFILES.get(fmt)
    if path is None or not Path(path).is_file():
        raise Refused('format_unavailable')
    profile = json.loads(Path(path).read_text(encoding='utf-8'))
    if profile.get('protocol') != PROTOCOL_PROFILE or profile.get('format') != fmt:
        raise Refused('site_config_invalid')
    binaries = dict(site['binaries'])
    for name, entry in profile.get('binaries', {}).items():
        if name not in binaries:
            raise Refused('site_config_invalid')
        binaries[name] = entry
    return dict(site, binaries=binaries)


def expand(path):
    return os.path.expanduser(path)


def verify_site_binaries(site):
    """Each pinned binary must exist and hash to its pin (the Rust helper's binary_pin rule)."""
    verified = {}
    for name, entry in sorted(site['binaries'].items()):
        path = Path(entry['path'])
        if not path.is_file():
            raise Refused('binary_missing:%s' % name)
        if digest_file(path) != entry['sha256']:
            raise Refused('binary_pin:%s' % name)
        verified[name] = entry['sha256']
    return verified


def free_port():
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(('127.0.0.1', 0))
        return s.getsockname()[1]


# ---------------------------------------------------------------- run layout and journal

class Ctx:
    def __init__(self, out, site=None):
        self.out = Path(out)
        self.run = load_run(self.out)
        self.site = None if site is None else site_for_format(site, self.run['format'])

    @property
    def secret_root(self):
        return Path(expand(self.site['secret_root']))


def load_run(out):
    path = Path(out) / 'run.json'
    if not path.is_file():
        raise Refused('run_not_found')
    return json.loads(path.read_text(encoding='utf-8'))


def journal_append(out, phase, event, error_code=None, **extra):
    record = {'ts': now(), 'phase': phase, 'event': event, 'error_code': error_code}
    record.update(extra)
    line = (json.dumps(record, sort_keys=True) + '\n').encode('utf-8')
    fd = os.open(Path(out) / 'journal.jsonl', os.O_WRONLY | os.O_APPEND | os.O_CREAT, 0o600)
    try:
        os.write(fd, line)
        os.fsync(fd)
    finally:
        os.close(fd)


def read_journal(out):
    path = Path(out) / 'journal.jsonl'
    if not path.exists():
        return []
    events = []
    for line in path.read_text(encoding='utf-8').splitlines():
        if line.strip():
            events.append(json.loads(line))
    return events


def phase_summary(events):
    """Return {phase: {state, started_at, finished_at, error_code}} from the journal."""
    summary = {}
    for event in events:
        name = event['phase']
        if name.startswith('_'):
            continue
        entry = summary.setdefault(name, {'state': 'pending', 'started_at': None, 'finished_at': None,
                                          'error_code': None})
        kind = event['event']
        if kind == 'started':
            entry.update(state='running', started_at=entry['started_at'] or event['ts'], finished_at=None,
                         error_code=None)
        elif kind == 'succeeded':
            entry.update(state='succeeded', finished_at=event['ts'], error_code=None)
        elif kind == 'failed':
            entry.update(state='failed', finished_at=event['ts'], error_code=event.get('error_code'))
        elif kind == 'awaiting':
            entry.update(state='awaiting', finished_at=None)
        if name == 'oracle_rebuild' and kind == 'succeeded':
            for tail in ROUND_TAIL:
                summary.pop(tail, None)
    return summary


def run_state(events):
    state, code = 'running', None
    for event in events:
        if event['phase'] == '_run' and event['event'] == 'state':
            state, code = event['state'], event.get('error_code')
    return state, code


def phase_complete(out, name, summary):
    entry = summary.get(name)
    return bool(entry and entry['state'] == 'succeeded' and (Path(out) / 'phases' / (name + '.json')).is_file())


def record_phase(out, name, started_at, data):
    finished = now()
    wall = None
    if started_at:
        wall = round((parse_ts(finished) - parse_ts(started_at)).total_seconds(), 3)
    write_json(Path(out) / 'phases' / (name + '.json'),
               {'phase': name, 'started_at': started_at, 'finished_at': finished,
                'wall_seconds': wall, 'data': data})


def parse_ts(value):
    return datetime.datetime.fromisoformat(value)


# ---------------------------------------------------------------- evidence commands

def next_index(directory):
    return len([p for p in Path(directory).glob('*.started.json')]) + 1


def run_cmd(out, label, argv, timeout=900, check=True, stdin=None):
    """Run an external command with immutable evidence. argv must never contain secrets."""
    ev = Path(out) / 'evidence'
    base = ev / ('%04d-%s' % (next_index(ev), label))
    write_json(str(base) + '.started.json', {'argv': argv, 'stdin_sha256': hashlib.sha256(stdin).hexdigest() if stdin else None,
                                            'utc': now()})
    try:
        result = subprocess.run(argv, input=stdin, capture_output=True, timeout=timeout)
        code = result.returncode
        out_bytes, err_bytes = result.stdout, result.stderr
    except subprocess.TimeoutExpired:
        code, out_bytes, err_bytes = 124, b'', b'timeout'
    write_private(str(base) + '.stdout', out_bytes)
    write_private(str(base) + '.stderr', err_bytes)
    write_json(str(base) + '.exit.json', {'exit': code})
    if check and code != 0:
        raise Refused('command_failed:%s' % label)
    return code, out_bytes


def docker_running(out, name):
    code, out_bytes = run_cmd(out, 'docker-inspect', ['docker', 'inspect', '-f', '{{.State.Running}}', name],
                              check=False, timeout=60)
    return code == 0 and out_bytes.strip() == b'true'


def run_cmd_quiet(argv, timeout=60):
    try:
        r = subprocess.run(argv, capture_output=True, timeout=timeout)
        return r.returncode, r.stdout
    except (OSError, subprocess.TimeoutExpired):
        return 1, b''


# ---------------------------------------------------------------- egress

def recv_exact(sock, n):
    buf = b''
    while len(buf) < n:
        chunk = sock.recv(n - len(buf))
        if not chunk:
            raise OSError('eof')
        buf += chunk
    return buf


def mysql_greeting(port, timeout=10):
    """Read the MySQL server handshake (protocol v10) through a local port; return the version."""
    try:
        with socket.create_connection(('127.0.0.1', port), timeout=timeout) as sock:
            header = recv_exact(sock, 4)
            payload = recv_exact(sock, int.from_bytes(header[:3], 'little'))
    except OSError:
        return None
    if payload[:1] != b'\x0a':
        return None
    return payload[1:].split(b'\x00', 1)[0].decode('ascii', 'replace')


GREETING_WAIT_SECONDS = 30


def open_forward(ctx, six, local_port=None):
    """Start a supervised ssh master, then add the explicit -L through its control socket.

    ClearAllForwardings=yes also clears command-line -L, so the forward is added with
    `ssh -O forward` after the master is up. The master is not started with -f: forwards
    must be verified live, not assumed from the exit code.
    """
    eg = ctx.site['egress']
    local_port = local_port or free_port()
    control = ctx.out / 'private' / 'ssh.sock'
    if len(str(control)) >= 100:
        raise Refused('control_path_too_long')
    control.unlink(missing_ok=True)
    target = '%s@%s' % (eg['ssh_user'], eg['ssh_host'])
    spec = '127.0.0.1:%d:%s:%d' % (local_port, six['host'], six['port'])
    argv = ['ssh', '-N', '-M', '-S', str(control),
            '-o', 'ControlPersist=no', '-o', 'ClearAllForwardings=yes', '-o', 'ExitOnForwardFailure=yes',
            '-o', 'IdentitiesOnly=yes', '-o', 'BatchMode=yes', '-i', expand(eg['ssh_identity']),
            '-o', 'ServerAliveInterval=30', '-o', 'ServerAliveCountMax=3', '-o', 'StrictHostKeyChecking=yes',
            target]
    # Optional trusted operator transport, never accepted from browser/source input.
    if eg.get('ssh_proxy_command'):
        argv[-1:-1] = ['-o', 'ProxyCommand=' + eg['ssh_proxy_command']]
    ev = ctx.out / 'evidence'
    base = ev / ('%04d-ssh-master' % next_index(ev))
    write_json(str(base) + '.started.json', {'argv': argv, 'utc': now()})
    stderr_fd = os.open(str(base) + '.stderr', os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    try:
        proc = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=stderr_fd,
                                start_new_session=True, close_fds=True)
    finally:
        os.close(stderr_fd)
    forward = {'local_port': local_port, 'control': control, 'target': target, 'proc': proc}
    deadline = time.monotonic() + 30
    while not control.exists():
        if proc.poll() is not None or time.monotonic() > deadline:
            close_forward(ctx, forward)
            raise Refused('ssh_forward_failed')
        time.sleep(0.2)
    code, _ = run_cmd(ctx.out, 'ssh-forward-add', ['ssh', '-S', str(control), '-O', 'forward', '-L', spec, target],
                      check=False, timeout=30)
    if code != 0:
        close_forward(ctx, forward)
        raise Refused('ssh_forward_failed')
    return forward


def wait_greeting(forward, seconds):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if forward['proc'].poll() is not None:
            raise Refused('ssh_forward_failed')
        version = mysql_greeting(forward['local_port'], timeout=5)
        if version:
            return version
        time.sleep(0.5)
    return None


def close_forward(ctx, forward):
    if not forward:
        return
    run_cmd(ctx.out, 'ssh-exit', ['ssh', '-S', str(forward['control']), '-O', 'exit', forward['target']],
            check=False, timeout=30)
    proc = forward.get('proc')
    if proc is not None and proc.poll() is None:
        proc.terminate()
        try:
            proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            proc.kill()


def egress_eip(ctx, six, local_port=None):
    forward = open_forward(ctx, six, local_port)
    try:
        version = wait_greeting(forward, GREETING_WAIT_SECONDS)
        if not version:
            raise Refused('forward_source_unreachable')
        if not ctx.site['egress']['producer_tls_server_name']:
            raise Refused('producer_tls_server_name_unsupported')
        write_json(ctx.out / 'private' / 'eip-local-port.json', {'local_port': forward['local_port']})
        write_json(ctx.out / 'private' / 'ssh-forward.json',
                   {'control': str(forward['control']), 'target': forward['target'], 'pid': forward['proc'].pid,
                    'local_port': forward['local_port']})
        return {'mode': 'eip', 'server_version': version, 'local_port': forward['local_port']}
    except Refused:
        close_forward(ctx, forward)
        raise


def eip_forward_port(ctx):
    """Local port of the eip forward. The egress phase chooses it once and keeps it in eip-local-port.json. The
    runner tears the forward down after a failure, so a later phase opens it again on the same port: the
    producer's driver config pins that port for a re-run."""
    path = ctx.out / 'private' / 'eip-local-port.json'
    if not path.is_file():
        raise Refused('ssh_forward_failed')
    port = int(json.loads(path.read_text(encoding='utf-8'))['local_port'])
    if not (ctx.out / 'private' / 'ssh-forward.json').is_file():
        egress_eip(ctx, ctx.run['source'], local_port=port)
    return port


def producer_endpoint(ctx, six):
    """The six fields the producer-side Rust tools read. Under eip the producer dials the forward on the host
    (host.docker.internal); TLS verifies the original host through the operator's tls_server_name."""
    if ctx.run['egress'] != 'eip':
        return dict(six)
    return dict(six, host=PRODUCER_FORWARD_HOST, port=eip_forward_port(ctx))


def host_endpoint(ctx, six):
    """(host, port) for the Python tools on the host. Under eip that is the forward on 127.0.0.1; otherwise the
    discover endpoint, or the six fields when none is given."""
    if ctx.run['egress'] == 'eip':
        return '127.0.0.1', eip_forward_port(ctx)
    endpoint = ctx.run.get('discover_endpoint') or {'host': six['host'], 'port': six['port']}
    return endpoint['host'], endpoint['port']


def producer_six_path(ctx):
    """The six-field input file for the producer-side `source-onboarding` (capture, follow). Direct runs use the
    run's six.json; eip runs use a private copy that carries the producer endpoint."""
    if ctx.run['egress'] != 'eip':
        return ctx.out / 'six.json'
    six = validate_six(load_private_json(ctx.out / 'six.json'))
    path = ctx.out / 'private' / 'producer-six.json'
    write_json(path, producer_endpoint(ctx, six))
    return path


def teardown_forward(out):
    """Close a forward recorded by a successful egress phase (no-op for direct or absent)."""
    meta_path = Path(out) / 'private' / 'ssh-forward.json'
    if not meta_path.is_file():
        return
    meta = json.loads(meta_path.read_text(encoding='utf-8'))
    run_cmd(out, 'ssh-teardown', ['ssh', '-S', meta['control'], '-O', 'exit', meta['target']], check=False, timeout=30)
    try:
        os.kill(int(meta['pid']), signal.SIGTERM)
    except OSError:
        pass
    meta_path.unlink()




# ---------------------------------------------------------------- minimal MySQL client (discovery only)

def _lenenc_int(data, pos):
    first = data[pos]
    if first < 0xfb:
        return first, pos + 1
    if first == 0xfc:
        return int.from_bytes(data[pos + 1:pos + 3], 'little'), pos + 3
    if first == 0xfd:
        return int.from_bytes(data[pos + 1:pos + 4], 'little'), pos + 4
    return int.from_bytes(data[pos + 1:pos + 9], 'little'), pos + 9


class MySQLClient:
    """Stdlib MySQL client: TLS + mysql_native_password + COM_QUERY text protocol. Read-only use."""

    CAP = 0x1 | 0x8 | 0x200 | 0x800 | 0x8000 | 0x80000

    def __init__(self, host, port, user, password, database, ca_file, server_name, timeout=30):
        import ssl as _ssl
        self.sock = socket.create_connection((host, port), timeout=timeout)
        self.seq = 0
        greeting = self._read()
        if greeting[0] != 10:
            raise Refused('discovery_protocol')
        nul = greeting.index(b'\x00', 1)
        pos = nul + 1 + 4
        salt1 = greeting[pos:pos + 8]
        pos += 8 + 1
        caps_lo = int.from_bytes(greeting[pos:pos + 2], 'little')
        pos += 2 + 1 + 2
        caps_hi = int.from_bytes(greeting[pos:pos + 2], 'little')
        pos += 2
        auth_len = greeting[pos]
        pos += 1 + 10
        part2_len = max(13, auth_len - 8)
        salt2 = greeting[pos:pos + part2_len].rstrip(b'\x00')
        if not (caps_lo | (caps_hi << 16)) & 0x800:
            raise Refused('discovery_tls_required')
        salt = salt1 + salt2
        ssl_req = struct.pack('<IIB', self.CAP, 1 << 24, 45) + b'\x00' * 23
        self._write(ssl_req)
        ctx = _ssl.create_default_context(cafile=ca_file)
        ctx.check_hostname = True
        self.sock = ctx.wrap_socket(self.sock, server_hostname=server_name)
        scramble = _native_scramble(password.encode('utf-8'), salt)
        resp = struct.pack('<IIB', self.CAP, 1 << 24, 45) + b'\x00' * 23
        resp += user.encode() + b'\x00' + bytes([len(scramble)]) + scramble
        resp += database.encode() + b'\x00' + b'mysql_native_password\x00'
        self._write(resp)
        ok = self._read()
        if ok[:1] == b'\xfe':
            raise Refused('discovery_auth_switch_unsupported')
        if ok[:1] == b'\xff':
            raise Refused('discovery_auth_failed')

    def _read(self):
        header = _recv_exact(self.sock, 4)
        length = int.from_bytes(header[:3], 'little')
        self.seq = (header[3] + 1) & 0xff
        return _recv_exact(self.sock, length)

    def _write(self, payload):
        self.sock.sendall(len(payload).to_bytes(3, 'little') + bytes([self.seq]) + payload)
        self.seq = (self.seq + 1) & 0xff

    def query(self, sql):
        self.seq = 0
        self._write(b'\x03' + sql.encode('utf-8'))
        first = self._read()
        if first[:1] == b'\xff':
            raise Refused('discovery_query_failed')
        if first[:1] == b'\x00':
            return []
        ncols, pos = _lenenc_int(first, 0)
        for _ in range(ncols):
            self._read()
        eof = self._read()
        rows = []
        while True:
            packet = self._read()
            if packet[:1] == b'\xfe' and len(packet) < 9:
                break
            if packet[:1] == b'\xff':
                raise Refused('discovery_query_failed')
            row, pos = [], 0
            while pos < len(packet):
                if packet[pos] == 0xfb:
                    row.append(None)
                    pos += 1
                    continue
                n, pos = _lenenc_int(packet, pos)
                row.append(packet[pos:pos + n].decode('utf-8', 'replace'))
                pos += n
            rows.append(row)
        return rows

    def close(self):
        try:
            self.seq = 0
            self._write(b'\x01')
        except OSError:
            pass
        self.sock.close()


def _recv_exact(sock, n):
    buf = b''
    while len(buf) < n:
        chunk = sock.recv(n - len(buf))
        if not chunk:
            raise Refused('discovery_connection_closed')
        buf += chunk
    return buf


def _native_scramble(password, salt):
    if not password:
        return b''
    import hashlib as _h
    stage1 = _h.sha1(password).digest()
    stage2 = _h.sha1(stage1).digest()
    mix = _h.sha1(salt + stage2).digest()
    return bytes(a ^ b for a, b in zip(stage1, mix))


def discover_schema(endpoint_host, endpoint_port, user, password, database, ca_file, server_name):
    """Select base tables whose single primary key is an integer; return tables and full column lists."""
    client = MySQLClient(endpoint_host, endpoint_port, user, password, database, ca_file, server_name)
    try:
        rows = client.query(
            "SELECT c.TABLE_NAME, c.COLUMN_NAME, c.ORDINAL_POSITION, c.COLUMN_KEY, c.DATA_TYPE "
            "FROM information_schema.COLUMNS c JOIN information_schema.TABLES t "
            "ON t.TABLE_SCHEMA = c.TABLE_SCHEMA AND t.TABLE_NAME = c.TABLE_NAME "
            "WHERE t.TABLE_SCHEMA = '%s' AND t.TABLE_TYPE = 'BASE TABLE' "
            "ORDER BY c.TABLE_NAME, c.ORDINAL_POSITION" % database)
    finally:
        client.close()
    tables = {}
    column_types = {}
    for table, column, _pos, key, dtype in rows:
        entry = tables.setdefault(table, {'columns': [], 'pri': [], 'pri_type': None})
        entry['columns'].append(column)
        column_types.setdefault(table, {})[column] = dtype
        if key == 'PRI':
            entry['pri'].append(column)
            entry['pri_type'] = dtype
    selected, rejected = [], {}
    for table in sorted(tables):
        entry = tables[table]
        if len(entry['pri']) != 1 or entry['pri_type'] not in INTEGER_TYPES:
            rejected[table] = 'needs_single_integer_primary_key'
            continue
        selected.append(table)
    if not selected:
        raise Refused('discovery_no_supported_tables')
    return {'tables': ['%s.%s' % (database, t) for t in selected],
            'columns': {'%s.%s' % (database, t): tables[t]['columns'] for t in selected},
            'types': {'%s.%s' % (database, t): column_types[t] for t in selected},
            'rejected': rejected}


INTEGER_TYPES = {'tinyint', 'smallint', 'mediumint', 'int', 'bigint'}

# ---------------------------------------------------------------- live infrastructure

def resolve_alias(out, network, alias):
    """Resolve a Docker network alias to exactly one container (never by container name)."""
    code, raw = run_cmd(out, 'net-inspect-%s' % network, ['docker', 'network', 'inspect', network], check=False)
    if code != 0:
        raise Refused('network_missing:%s' % network)
    names = [c['Name'] for net in json.loads(raw) for c in net.get('Containers', {}).values()]
    matches = []
    for name in names:
        code, raw = run_cmd_quiet(['docker', 'inspect', '-f', '{{json .NetworkSettings.Networks}}', name])
        if code != 0:
            continue
        entry = json.loads(raw).get(network) or {}
        if alias in (entry.get('Aliases') or []):
            matches.append(name)
    if len(matches) != 1:
        raise Refused('alias_unresolved:%s' % alias)
    return matches[0]


def dexec(out, label, container, argv, stdin=None, check=True, timeout=900):
    cmd = ['docker', 'exec'] + (['-i'] if stdin is not None else []) + [container] + list(argv)
    return run_cmd(out, label, cmd, timeout=timeout, check=check, stdin=stdin)


def created_by_run(out, db):
    """True when this run's own createdb evidence succeeded (a resume of our own step)."""
    ev = Path(out) / 'evidence'
    return any(json.loads(p.read_text(encoding='utf-8')).get('exit') == 0
               for p in ev.glob('*-createdb-%s.exit.json' % db))


def ensure_new_database(out, pg, admin, db, owner):
    """Create a database that must not exist yet. Only this run's own earlier createdb may be reused."""
    _, raw = run_cmd(out, 'db-exists-%s' % db,
                     ['docker', 'exec', pg, 'psql', '-X', '-U', admin, '-d', 'postgres', '-Atc',
                      "SELECT 1 FROM pg_database WHERE datname = '%s'" % db], check=False)
    if raw.strip() == b'1':
        if created_by_run(out, db):
            return
        raise Refused('database_exists:%s' % db)
    run_cmd(out, 'createdb-%s' % db, ['docker', 'exec', pg, 'createdb', '-U', admin, '--owner=%s' % owner,
                                       '--template=template0', db])


def container_path(site, host):
    """Map a host path to the path the trusted container sees (secret root is mounted at /secrets)."""
    secret = Path(expand(site['secret_root']))
    host = Path(os.path.abspath(str(host)))
    if host == secret or secret in host.parents:
        return '/secrets/' + str(host.relative_to(secret))
    return str(host)


def free_ports(n):
    ports = []
    while len(ports) < n:
        port = free_port()
        if port not in ports:
            ports.append(port)
    return ports


def wait_for_file(out, path, seconds, name, container=None):
    deadline = time.monotonic() + seconds
    while not Path(path).exists():
        if time.monotonic() > deadline:
            raise Refused('%s_timeout' % name)
        if container:
            _, state = run_cmd_quiet(['docker', 'inspect', '-f', '{{.State.Running}}', container])
            if state.strip() != b'true' and not Path(path).exists():
                raise Refused('%s_exited' % name)
        time.sleep(5)
    return Path(path)


def handler_identity(ctx):
    I = ctx.site['identity']
    out = ctx.out
    names = ctx.run['names']
    pg = resolve_alias(out, I['network'], I['pg_alias'])
    db = names['db_prefix'] + 'identity_v1'
    ensure_new_database(out, pg, I['pg_admin_user'], db, I['pg_admin_user'])
    ident = out / 'identity'
    ident.mkdir(mode=0o700, exist_ok=True)
    ports = free_ports(3)
    name = names['container_prefix'] + '-identity'
    mainline = I['mainline_dir']
    argv = ['docker', 'run', '-d', '--name', name, '--network', I['network'],
            '-p', '127.0.0.1:%d:%d' % (ports[0], ports[0]), '-p', '127.0.0.1:%d:%d' % (ports[1], ports[1]),
            '-p', '127.0.0.1:%d:%d' % (ports[2], ports[2]),
            '--mount', 'type=volume,source=%s,target=/target,readonly' % I['volume'],
            '--mount', 'type=bind,source=%s,target=%s,readonly' % (mainline, mainline),
            '--mount', 'type=bind,source=%s,target=/private' % ident,
            '--mount', 'type=bind,source=%s,target=/secrets,readonly' % expand(I['secret_dir']),
            '--mount', 'type=bind,source=%s,target=/pgca,readonly' % expand(I['tls_dir']),
            '-e', 'OO_NATIVE_IDENTITY_PROJECT=%s' % names['logical_project'],
            '-e', 'OO_A370_LIVE=1', '-e', 'OO_NATIVE_IDENTITY_SERVICE=1', '-e', 'OO_NATIVE_IDENTITY_PRIVATE=/private',
            '-e', 'OO_A370_PG_HOST=%s' % I['pg_alias'], '-e', 'OO_A370_PG_PORT=5432',
            '-e', 'OO_A370_PG_DATABASE=%s' % db, '-e', 'OO_A370_PG_PASSWORD_FILE=/secrets/%s' % I['pg_password_name'],
            '-e', 'OO_A370_PG_CA=/pgca/ca.crt', '-e', 'OO_A370_PROOF=/private/proof.json',
            '-e', 'OO_NATIVE_IDENTITY_PORT=%d' % ports[0], '-e', 'OO_NATIVE_IDP_PORT=%d' % ports[1],
            '-e', 'OO_MAINLINE_BINDING_PORT=%d' % ports[2], '-e', 'OO_MAINLINE_BINDING_BIN=%s' % I['binding_bin'],
            I['image'], I['test_binary'], '--exact', I['test_name'], '--nocapture']
    run_cmd(out, 'identity-launch', argv)
    wait_for_file(out, ident / 'ready.json', I['ready_timeout_seconds'], 'identity_ready', container=name)
    files = sorted(p.name for p in ident.iterdir())
    return {'container': name, 'network': I['network'], 'pg_alias': I['pg_alias'], 'pg_container': pg,
            'database': db, 'ports': ports, 'files': files,
            'ready_sha256': digest_file(ident / 'ready.json')}


def cleanup_leftover_acl(out, pg, admin, db, role, login):
    """Drop reader/role objects left by this run's earlier failed ACL attempt (names are per-run)."""
    _, raw = run_cmd(out, 'acl-leftover-check', ['docker', 'exec', pg, 'psql', '-X', '-U', admin, '-d', 'postgres',
                                                 '-Atc', "SELECT rolname FROM pg_roles WHERE rolname IN ('%s','%s')"
                                                 % (role, login)], check=False)
    if not raw.strip():
        return
    dexec(out, 'acl-leftover-drop', pg, ['psql', '-X', '-v', 'ON_ERROR_STOP=1', '-U', admin, '-d', db],
          stdin=('BEGIN;\nDROP OWNED BY %s, %s;\nDROP ROLE IF EXISTS %s;\nDROP ROLE IF EXISTS %s;\nCOMMIT;\n'
                 % (login, role, login, role)).encode())


def handler_target(ctx):
    T = ctx.site['target']
    out = ctx.out
    names = ctx.run['names']
    pg = resolve_alias(out, T['network'], T['pg_alias'])
    db = names['db_prefix'] + 'target_v1'
    ensure_new_database(out, pg, T['admin_user'], db, T['admin_user'])
    deploy = (str(HERE.parents[1] / 'crates/oo-jobs/tools/deploy.py')
              if ctx.run['format'] == 'chunked-typed/v2' else T['deploy_script'])
    run_cmd(out, 'jobs-deploy', [sys.executable, deploy, '--container', pg, '--database', db,
                                 '--admin', T['admin_user']], timeout=1800)
    worker = ctx.site['binaries']['job_worker']['path']
    loader = ctx.site['binaries']['oo_pipeline']['path']
    target = {'host': T['pg_alias'], 'port': 5432, 'database': db, 'runtime_principal': T['admin_user'],
              'password_file': '/secrets/%s' % T['admin_password_name'], 'ca_file': '/secrets/%s' % T['ca_name']}
    req = json.dumps({'protocol': 'oo.pipeline.install-native-update/v1', 'target': target},
                     separators=(',', ':')).encode()
    trusted = ctx.site['trusted_container']
    for command in ('install-native-update', 'install-native-mirror-rebind', 'install-native-claim-selected'):
        dexec(out, command, trusted, [loader, command], stdin=req, timeout=1800)
    secret = Path(expand(ctx.site['secret_root']))
    run_secret = secret / 'provision' / ctx.run['run_id']
    reader_pw = run_secret / 'gateway-reader.pass'
    if not reader_pw.exists():
        write_private(reader_pw, secrets.token_hex(32) + '\n')
    login = names['reader_login']
    role = names['role']
    pw = reader_pw.read_bytes().strip().decode()
    sql = ('BEGIN;\n'
           'CREATE ROLE %(role)s NOLOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;\n'
           'CREATE ROLE %(login)s LOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS PASSWORD \'%(pw)s\';\n'
           'GRANT %(role)s TO %(login)s WITH INHERIT FALSE, SET TRUE;\n'
           'GRANT CONNECT ON DATABASE %(db)s TO mysql_native_loader,oo_target_admin,%(login)s;\n'
           'ALTER ROLE %(login)s SET default_transaction_read_only=on;\n'
           'GRANT USAGE ON SCHEMA _oo TO %(role)s;\n'
           'GRANT EXECUTE ON FUNCTION pg_catalog.pg_advisory_xact_lock(integer,integer) TO oo_target_owner;\n'
           'COMMIT;\n'
           ) % {'role': role, 'login': login, 'pw': pw, 'db': db}
    cleanup_leftover_acl(out, pg, T['admin_user'], db, role, login)
    dexec(out, 'owned-target-acl', pg, ['psql', '-X', '-v', 'ON_ERROR_STOP=1', '-U', T['admin_user'], '-d', db],
          stdin=sql.encode())
    del pw, sql
    worker_conf = out / 'private' / 'worker.conf'
    write_private(worker_conf, ('host=%s\nport=5432\ndatabase=%s\nruntime_user=%s\n'
                                'runtime_password_file=/secrets/%s\nadmin_user=%s\n'
                                'admin_password_file=/secrets/%s\nca_file=/secrets/%s\nmigrations=/unused\n')
                  % (T['pg_alias'], db, T['runtime_user'], T['runtime_password_name'], T['admin_user'],
                     T['admin_password_name'], T['ca_name']))
    dexec(out, 'native-install', trusted, [worker, '--config', container_path(ctx.site, worker_conf), 'native-install'],
          timeout=1800)
    return {'database': db, 'pg_container': pg, 'role': role, 'reader_login': login,
            'worker_config': str(worker_conf)}


def prepared_root(ctx):
    """Prepared output must sit under /tmp/oo-mysql-connection-whole: the trusted container runs the producer on it."""
    root = Path('/tmp/oo-mysql-connection-whole/provision') / ctx.run['run_id'] / 'prepared'
    root.mkdir(mode=0o700, parents=True, exist_ok=True)
    os.chmod(root, 0o700)
    return root


def discover_for_run(ctx):
    six = ctx.run['source']
    host, port = host_endpoint(ctx, six)
    password = read_password(ctx.secret_root / 'provision' / ctx.run['run_id'] / 'password.pass').decode('utf-8')
    result = discover_schema(host, port, six['user'], password, six['database'],
                             ctx.run['source_ca'], six['host'])
    del password
    write_json(ctx.out / 'private' / 'discovery.json', result)
    return result


def build_operator(ctx, binding, project_id, server_id, discovered):
    """Operator for the Rust helper: six fields + registry + product block (all per-run paths)."""
    site = ctx.site
    names = ctx.run['names']
    secret = Path(expand(site['secret_root']))
    run_secret = secret / 'provision' / ctx.run['run_id']
    token_admin = run_secret / 'token-admin.pass'
    write_private(token_admin, token_admin_credential(site))
    b = site['binaries']
    worker_conf = ctx.out / 'private' / 'worker.conf'
    op = dict(producer_endpoint(ctx, ctx.run['source']))
    if ctx.run['egress'] == 'eip':
        op['tls_server_name'] = ctx.run['source']['host']
    op['password_file'] = str(run_secret / 'password.pass')
    op['ca_file'] = container_path(site, ctx.run['source_ca'])
    op['tables'] = discovered['tables']
    op['columns'] = discovered['columns']
    op['server_id'] = server_id
    op['health_every_ms'] = 400
    op['reconcile_every_ms'] = 1200
    op['fixture'] = site['producer_fixture_marker']
    op['preserve_dir'] = str(prepared_root(ctx).parent / 'preserve')
    Path(op['preserve_dir']).mkdir(mode=0o700, exist_ok=True)
    op['onboarding'] = {
        'admission': b['oo_source_admission']['path'],
        **({} if ctx.run['format'] == 'v1' else {'c1_baseline_format': ctx.run['format'],
                                                 'child_deadlines_ms': {'native-step': 21600000}}),
        'after_generation': 0,
        'binding': binding,
        'container': site['trusted_container'],
        'host_secret_root': str(secret),
        'loader': b['oo_pipeline']['path'],
        'period_ms': 5000,
        'prepared_root': str(prepared_root(ctx)),
        'producer': b['producer']['path'],
        'project_id': project_id,
        'reconcile_every_ms': 1200,
        'health_every_ms': 400,
        'secret_registry': str(ctx.out / 'private' / 'secret-registry.json'),
        'serve_seconds': 86400,
        'worker': b['job_worker']['path'],
        'worker_config': container_path(site, worker_conf),
        'product': {
            'token_admin_password_file': str(token_admin),
            'source_access': 'network',
            'api_binary': b['selfbuild_control_api']['path'],
            'release_binary': b['release_control_api']['path'],
            'gateway_binary': b['gateway']['path'],
            'token_binary': b['issue_token']['path'],
            'cli_binary': b['cli']['path'],
            'mapping_binary': b['provision_mapping_reader']['path'],
            'coordinator_binary': b['release_coordinator']['path'],
        },
    }
    path = ctx.out / 'private' / 'operator-capture.json'
    write_json(path, op)
    return path


def handler_capture(ctx):
    out = ctx.out
    identity = json.loads((out / 'phases' / 'identity.json').read_text(encoding='utf-8'))['data']
    ids_path = out / 'private' / 'scope-ids.json'
    if not ids_path.exists():
        write_json(ids_path, {'binding': str(uuid.uuid4()), 'project_id': str(uuid.uuid4()),
                              'server_id': secrets.randbelow(3_900_000_000) + 100_000})
    ids = json.loads(ids_path.read_text(encoding='utf-8'))
    binding, project_id = ids['binding'], ids['project_id']
    discovered = discover_for_run(ctx)
    operator = build_operator(ctx, binding, project_id, ids['server_id'], discovered)
    prepared_root(ctx)
    capture_out = out / 'capture'
    binary = ctx.site['binaries']['source_onboarding']['path']
    run_cmd(out, 'source-onboarding-capture',
            ['docker', 'exec', ctx.site['runtime_container'], binary, 'capture', '--input', str(producer_six_path(ctx)),
             '--operator', str(operator), '--out', str(capture_out)], timeout=7200)
    receipt = capture_out / 'capture-receipt.json'
    if not receipt.is_file():
        raise Refused('capture_receipt_missing')
    doc = json.loads(receipt.read_text(encoding='utf-8'))
    return {'binding': binding, 'project_id': project_id, 'receipt_sha256': digest_file(receipt),
            'identity_container': identity['container'], 'native_state': doc.get('native_state')}


# ---------------------------------------------------------------- artifacts (catalog, profile, bundle)

def load_export_collection():
    import importlib.util as _ilu
    spec = _ilu.spec_from_file_location('export_native_collection', HERE / 'export_native_collection.py')
    module = _ilu.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.collection


def profile_source(client, source_id, generation, tables, columns, source_gtid_check):
    """Full, unsampled aggregates per column (same shape as oo.source.mysql.profile/v1). Read-only."""
    # Connection establishment stays short; full-table aggregates need their own bounded budget.
    previous_timeout = client.sock.gettimeout()
    client.sock.settimeout(540)
    try:
        client.query('SET SESSION max_execution_time=540000')
        return _profile_source(client, source_id, generation, tables, columns, source_gtid_check)
    finally:
        client.sock.settimeout(previous_timeout)


def _profile_source(client, source_id, generation, tables, columns, source_gtid_check):
    before = client.query('SELECT @@GLOBAL.gtid_executed')[0][0]
    out_tables = []
    for qualified in tables:
        schema, table = qualified.split('.', 1)
        meta = client.query(
            "SELECT COLUMN_NAME, DATA_TYPE, IS_NULLABLE FROM information_schema.COLUMNS "
            "WHERE TABLE_SCHEMA='%s' AND TABLE_NAME='%s' ORDER BY ORDINAL_POSITION" % (schema, table))
        fields = []
        for name, kind, nullable in meta:
            if name not in columns[qualified]:
                continue
            binary = kind in ('binary', 'varbinary', 'blob', 'tinyblob', 'mediumblob', 'longblob')
            minimum = 'HEX(MIN(`%s`))' % name if binary else 'CAST(MIN(`%s`) AS CHAR)' % name
            maximum = 'HEX(MAX(`%s`))' % name if binary else 'CAST(MAX(`%s`) AS CHAR)' % name
            raw = client.query("SELECT JSON_OBJECT('rows',COUNT(*),'nulls',COUNT(*)-COUNT(`%s`),"
                               "'distinct',COUNT(DISTINCT BINARY `%s`),'min',%s,'max',%s) FROM `%s`.`%s`"
                               % (name, name, minimum, maximum, schema, table))[0][0]
            value = json.loads(raw)
            if not (0 <= value['nulls'] <= value['rows'] and 0 <= value['distinct'] <= value['rows']):
                raise Refused('profile_inconsistent')
            value.update(null_count=value['nulls'], distinct_count=value['distinct'])
            fields.append(dict(name=name, source_type=kind, nullable=nullable == 'YES', aggregation='full_unsampled',
                               min_max_encoding='hex' if binary else 'source_text', **value))
        out_tables.append(dict(dataset=qualified, schema=schema, table=table, rows=fields[0]['rows'],
                               full_scan=True, columns=fields))
    after = client.query('SELECT @@GLOBAL.gtid_executed')[0][0]
    if before != after:
        raise Refused('source_changed_during_profile')
    return {'protocol': 'oo.source.mysql.profile/v1', 'source_id': source_id, 'source_gtid': after,
            'source_generation': generation, 'samples': 'none', 'sampled': False, 'tables': out_tables}


def sha_file(path):
    return digest_file(path)


def v2_artifact_request(request):
    """Shape a chunked-typed/v2 final-load request like the v1 one the catalog import expects.

    v2 carries the per-table admission under `tables[].admission`, the config and l_end digests at the top
    level, and each table's schema digest under `baseline_scope.tables[].schema_sha256` (the scope-level
    `schema_digest` is not the table's; the catalog import binds the table's). v1 carried `entries[]` with
    registry fields and a `native_requests[]` list.
    """
    tables = {(t['schema'], t['table']): t for t in request['tables']}
    scope_tables = {(t['schema'], t['table']): t for t in request['baseline_scope']['tables']}
    entries, natives = [], []
    for item in request['entries']:
        table = tables[(item['schema'], item['table'])]
        entries.append({'schema': item['schema'], 'table': item['table'],
                        'admission_registry_path': table['admission']['registry_path'],
                        'admission_registry_sha256': table['admission']['registry_sha256']})
        natives.append({'source_id': request['source_id'], 'source_generation': request['source_generation'],
                        'config_digest': request['baseline_scope']['config_digest'],
                        'schema_digest': scope_tables[(item['schema'], item['table'])]['schema_sha256'],
                        'type_digest': table['type_digest'], 'input_digest': table['input_digest'],
                        'snapshot': {'kind': 'mysql_cdc_first', 'value': request['baseline_scope']['l_end']},
                        'target': request['target']})
    return {'entries': entries, 'native_requests': natives}


def handler_artifacts(ctx):
    out = ctx.out
    names = ctx.run['names']
    capture = json.loads((out / 'phases' / 'capture.json').read_text(encoding='utf-8'))['data']
    binding = capture['binding']
    rid = ctx.run['run_id']
    shared = Path('/tmp/oo-mysql-connection-whole/provision') / rid
    native = shared / 'native'
    if native.exists():
        raise Refused('artifacts_exist')
    worker_conf = out / 'private' / 'worker.conf'
    worker = ctx.site['binaries']['job_worker']['path']
    trusted = ctx.site['trusted_container']
    dexec(out, 'native-artifacts', trusted, [worker, '--config', container_path(ctx.site, worker_conf),
                                              'native-artifacts', '--binding', binding, '--out', str(native)], timeout=1800)
    request = json.loads((native / 'request.json').read_text(encoding='utf-8'))
    result = json.loads((native / 'load.json').read_text(encoding='utf-8'))
    ack = json.loads((native / 'ack.json').read_text(encoding='utf-8'))
    generation = result['source_generation']
    if generation != 1 or ack['source_generation'] != generation:
        raise Refused('artifacts_generation')
    loader = ctx.site['binaries']['oo_pipeline']['path']
    admission = ctx.site['binaries']['oo_source_admission']['path']
    if 'native_requests' not in request:
        request = v2_artifact_request(request)
    entries = []
    for index, (spec, native_request, loaded) in enumerate(zip(request['entries'], request['native_requests'],
                                                                 result['entries'], strict=True)):
        if sha_file(spec['admission_registry_path']) != spec['admission_registry_sha256']:
            raise Refused('admission_registry_drift')
        query = {k: native_request[k] for k in ['source_id', 'source_generation', 'config_digest', 'schema_digest',
                                                'type_digest', 'input_digest', 'snapshot', 'target']}
        query['target'] = {k: v for k, v in query['target'].items() if k not in ['owner', 'shard']}
        query.update(protocol='oo.pipeline.native-sealed-catalog/v1', run_id=loaded['run_id'], owner=binding,
                     target_generation=result['target_generation'])
        _, raw = dexec(out, 'catalog-import-%d' % index, trusted,
                       ['env', 'OO_NATIVE_ADMISSION_BIN=' + admission,
                        'OO_NATIVE_BINDING_REGISTRY=' + spec['admission_registry_path'],
                        'OO_NATIVE_BINDING_REGISTRY_SHA256=' + spec['admission_registry_sha256'],
                        loader, 'catalog-import-native'],
                       stdin=json.dumps(query, separators=(',', ':')).encode(), timeout=1800)
        entry = json.loads(raw)
        for key in ['source_id', 'source_generation', 'target_generation', 'run_id', 'config_digest',
                    'type_digest', 'input_digest']:
            if entry[key] != query[key]:
                raise Refused('catalog_import_mismatch')
        entries.append(entry)
    collection = load_export_collection()
    catalog = collection(entries, [(v['schema'], v['table']) for v in request['entries']])
    bundle = Path(expand(ctx.site['secret_root'])) / 'provision' / rid / 'bundle-static-v1'
    bundle.mkdir(mode=0o700)
    write_canonical_json(bundle / 'catalog.json', catalog)
    discovered = json.loads((out / 'private' / 'discovery.json').read_text(encoding='utf-8'))
    client_host, client_port = host_endpoint(ctx, ctx.run['source'])
    client = MySQLClient(client_host, client_port,
                         ctx.run['source']['user'],
                         read_password(ctx.secret_root / 'provision' / rid / 'password.pass').decode('utf-8'),
                         ctx.run['source']['database'], ctx.run['source_ca'], ctx.run['source']['host'])
    try:
        profile = profile_source(client, result['source_id'], generation, discovered['tables'], discovered['columns'], None)
    finally:
        client.close()
    write_json(bundle / 'profile.json', profile)
    if profile['source_gtid'] != result.get('l_end', profile['source_gtid']):
        raise Refused('profile_source_gtid_mismatch')
    return {'binding': binding, 'source_id': result['source_id'], 'generation': generation,
            'tables': len(entries), 'catalog_sha256': sha_file(bundle / 'catalog.json'),
            'profile_sha256': sha_file(bundle / 'profile.json'), 'bundle': str(bundle),
            'rows_by_table': {t['dataset']: t['rows'] for t in profile['tables']}}


# ---------------------------------------------------------------- sealed bundle (complete) and product runtime

TEMPLATE_DIR = HERE / 'provision' / 'templates'
VISIBILITY_CONSTANTS = {'catalog_version': 'pending', 'issued_for_compiler': 'oo-builder/role-pack/v1',
                        'limits': {'max_entries': 128, 'max_file_bytes': 262144, 'max_total_bytes': 1048576},
                        'principal': 'fixture-admin', 'protocol': 'oo.builder.role-visibility/v1'}
ONTOLOGY_CONSTANTS = {'classification': {'auto_accept': 'rule_high_confidence'},
                      'compile': {'budgets': {'readme_tokens': 2000, 'table_file_tokens': 4000}, 'per_role': True,
                                  'sample_values': {'default': 'masked', 'max_per_column': 0}},
                      'default_tier_preference': ['raw'], 'lifecycle': {'deprecation_min_releases': 2},
                      'locale': 'en-US', 'publish': {'include_unreviewed': []}, 'spec_version': 'oos/v1'}


def withheld_path(out):
    return Path(out) / 'private' / 'withheld.json'


def hidden_declaration_path(out):
    return Path(out) / 'private' / 'hidden-declaration.json'


def load_hidden_declaration(out):
    path = hidden_declaration_path(out)
    if not path.is_file():
        return {'protocol': PROTOCOL_HIDDEN, 'columns': []}
    return json.loads(path.read_text(encoding='utf-8'))


def hidden_names(out):
    return {column['name'] for column in load_hidden_declaration(out)['columns']}


def declare_hidden(out, build_id, visible):
    """Automated owner declaration of the hidden set, derived from the model's PII classes of build_id.
    Labelled control_plane_test with business_owner_signed=false: it is not a business signature."""
    columns = {column['name']: column for column in load_hidden_declaration(out)['columns']}
    for item in visible:
        columns[item['name']] = {'name': item['name'], 'dataset': item['dataset'], 'column': item['column'],
                                 'data_type': item.get('data_type'), 'semantic_type': item.get('semantic_type'),
                                 'pii': item['pii'], 'classified_by_build': build_id}
    doc = {'protocol': PROTOCOL_HIDDEN, 'declared_by': 'control_plane_test', 'signer_class': 'control_plane_test',
           'business_owner_signed': False, 'basis': 'model_classification',
           'rule': 'a column whose effective PII class in a build is not none is hidden from every later build',
           'columns': [columns[name] for name in sorted(columns)]}
    write_json(hidden_declaration_path(out), doc)
    return doc


def write_withheld(out):
    """Derive the withheld set from the CURRENT build's review items (effective PII class and the hidden declaration)."""
    overlay, items = review_items(out)
    names = sorted(item['name'] for item in items if not item['reviewable'])
    doc = {'build_id': overlay.get('build_id'), 'overlay_sha256': digest_file(Path(out) / 'product' / 'prepared-overlay.json'),
           'rule': 'withheld = effective pii != none, or declared hidden', 'withheld': names}
    write_json(withheld_path(out), doc)
    return doc


def assert_withheld_current(out):
    """decide() refuses unless the stored withheld set is exactly what the current review items derive."""
    path = withheld_path(out)
    if not path.is_file():
        raise Refused('policy_not_applied')
    stored = json.loads(path.read_text(encoding='utf-8'))
    _, items = review_items(out)
    derived = sorted(item['name'] for item in items if not item['reviewable'])
    if stored.get('withheld') != derived or stored.get('overlay_sha256') != digest_file(
            Path(out) / 'product' / 'prepared-overlay.json'):
        raise Refused('withheld_drift')
    return derived


def pii_state_path(out):
    return Path(out) / 'private' / 'pii-rebuild.json'


def load_pii_state(out):
    path = pii_state_path(out)
    if not path.is_file():
        return {'protocol': PROTOCOL_PII, 'passes': []}
    return json.loads(path.read_text(encoding='utf-8'))


def save_pii_state(out, state):
    write_json(pii_state_path(out), state)


def product_build_id(out):
    path = Path(out) / 'product' / 'build.json'
    return json.loads(path.read_text(encoding='utf-8')).get('build_id') if path.is_file() else None


def visible_pii_items(out, hidden=()):
    """Items of the current build that are PII (pii != none) and not hidden: declared, or in the pass's hidden set."""
    hidden = hidden_names(out) | set(hidden)
    _, items = review_items(out)
    return [item for item in items if item['name'] not in hidden and item['pii'] != PII_REVIEWABLE]


def pass_bundle_name(number):
    return 'bundle-static-v1' if number == 1 else 'bundle-static-v1-pii%d' % number


def pass_bundle_host(ctx, name):
    return Path(expand(ctx.site['secret_root'])) / 'provision' / ctx.run['run_id'] / name


def make_pass_bundle(ctx, artifacts, name):
    """Directory for a rebuild pass. catalog.json and profile.json are the capture's own bytes, copied unchanged."""
    source = Path(artifacts['bundle'])
    target = pass_bundle_host(ctx, name)
    target.mkdir(mode=0o700, exist_ok=True)
    for filename in ('catalog.json', 'profile.json'):
        if not (target / filename).exists():
            write_private(target / filename, (source / filename).read_bytes())
    return target


def json_bytes(value):
    return (json.dumps(value, sort_keys=True, indent=2, ensure_ascii=False) + '\n').encode('utf-8')


def write_sealed(bundle, files):
    """Write-once. A sealed input that exists must already hold exactly these bytes, and no new input is written once
    the manifest exists. A bundle that a build has recorded is never rewritten."""
    manifest_path = bundle / 'manifest.json'
    for name, data in files.items():
        path = bundle / name
        if path.exists():
            if path.read_bytes() != data:
                raise Refused('sealed_inputs_rewrite')
        elif manifest_path.exists():
            raise Refused('sealed_inputs_rewrite')
        else:
            write_private(path, data)
    members = {f.name: digest_file(f) for f in sorted(bundle.iterdir()) if f.name != 'manifest.json'}
    manifest = json_bytes({'contract': 'oo.selfbuild-control-api/sealed-bundle/v1', 'files': members})
    if manifest_path.exists():
        if manifest_path.read_bytes() != manifest:
            raise Refused('sealed_inputs_rewrite')
    else:
        write_private(manifest_path, manifest)
    return digest_file(manifest_path)


def ensure_bundle_complete(ctx, artifacts, bundle=None, hidden=(), native=None, oracle_cells=None):
    """Sealed-bundle files for one build, from this run's catalog, profile and receipts. The hidden columns are absent
    from the role, the visibility and the native model, so the S-stage copy of the model is built without them.
    The capture-level facts (catalog, profile, quality.columns) keep every column. Write-once (see write_sealed)."""
    rid = ctx.run['run_id']
    bundle = Path(artifacts['bundle']) if bundle is None else Path(bundle)
    native = Path('/tmp/oo-mysql-connection-whole/provision') / rid / 'native' if native is None else Path(native)
    request = json.loads((native / 'request.json').read_text(encoding='utf-8'))
    load = json.loads((native / 'load.json').read_text(encoding='utf-8'))
    catalog = json.loads((bundle / 'catalog.json').read_text(encoding='utf-8'))
    profile = json.loads((bundle / 'profile.json').read_text(encoding='utf-8'))
    names = ctx.run['names']
    logical = names['logical_project']
    role = names['role']
    schema = request['entries'][0]['schema']
    hidden = set(hidden)
    datasets, columns, mappings = [], [], []
    total_columns = 0
    for entry in catalog['entries']:
        for table in entry['tables']:
            dataset = table['logical_schema'] + '.' + table['logical_table']
            names_ = [c.get('logical_name', c.get('name', c.get('logical_column'))) for c in table['columns']]
            if not all(names_):
                raise Refused('catalog_columns_unnamed')
            total_columns += len(names_)
            visible = [n for n in names_ if dataset + '.' + n not in hidden]
            columns += [dataset + '.' + n for n in visible]
            datasets.append({'dataset': dataset, 'columns': visible, 'global_stats': 'denied', 'row_security': 'none'})
            mappings.append({'dataset': dataset, 'actual_storage_schema': '_oo',
                             'scope_namespace': table['stored_namespace'], 'stored_physical': table['stored_physical']})
    catalog_sha = digest_file(bundle / 'catalog.json')
    # Source-owner declaration (sealed source rule), not a model output: every DECIMAL column is declared money.
    # The unit is 'unspecified' because the fixture declares no currency. The money projection requires this declaration.
    discovered = json.loads((ctx.out / 'private' / 'discovery.json').read_text(encoding='utf-8'))
    if 'types' not in discovered:
        previous = discovered['tables']
        discovered = discover_for_run(ctx)
        if discovered['tables'] != previous:
            raise Refused('discovery_drift')
    money = []
    for dataset in sorted(discovered['types']):
        for column, dtype in sorted(discovered['types'][dataset].items()):
            if dtype == 'decimal' and dataset + '.' + column not in hidden:
                money.append({'column': dataset + '.' + column, 'semantic_type': 'money',
                              'unit': 'unspecified', 'pii': 'none'})
    rules = {'columns': money, 'relationships': [],
             'declaration': {'declared_by': 'test_human_control_plane_test', 'rule': 'mysql decimal columns are money',
                             'currency_declared': False}}
    ontology_version = 'ont-%s-availability' % logical
    visibility = dict(VISIBILITY_CONSTANTS, project=logical, role=role, datasets=datasets,
                      ontology_version=ontology_version, policy={'id': 'pol-%s' % logical, 'version': '1'},
                      knowledge={'id': 'know-%s' % logical, 'version': '1'})
    model = {'dataset': datasets[0]['dataset'],
             'declared_price': {'currency': 'USD', 'per_total_token': '0.000001', 'status': 'declared_non_billing'},
             'settings': {'catalog_version': 'pending', 'knowledge': {'id': 'know-%s' % logical, 'version': '1'},
                          'ontology': dict(ONTOLOGY_CONSTANTS, include_schemas=[schema], name=schema),
                          'ontology_version': ontology_version, 'policy': {'id': 'pol-%s' % logical, 'version': '1'},
                          'project': logical},
             'visibility': visibility}
    accepted_rows = sum(e['row_count'] for e in load['entries'])
    rejects = 0  # the load receipt records no rejects; the fixture has no zero-date values
    baseline = request.get('baseline_scope', {})
    binding_vector = json.dumps(catalog['binding_vector'], sort_keys=True, separators=(',', ':')).encode()
    quality = {'rows': accepted_rows, 'tables': len(datasets), 'columns': total_columns,
               'zero_date_rejects': rejects}
    if oracle_cells is not None:
        quality['full_typed_cell_differences'] = oracle_cells
    files = {
        'rules.json': json_bytes(rules),
        'suggestions.json': json_bytes({'certified': False, 'merged': False, 'ready': False}),
        'roles.json': json_bytes({'contract': 'oo.selfbuild-control-api/sealed-roles/v1', 'roles': {role: columns}}),
        'read_logic.json': json_bytes({'contract': 'oo.selfbuild-control-api/read-logic/v1',
                                       'catalog_sha256': catalog_sha, 'executed': False, 'mappings': mappings}),
        'generation.json': json_bytes({'llm_calls': 0, 'outcome': 'structural_draft'}),
        'native-model.json': json_bytes(model),
        'source_semantics.json': json_bytes({
            'protocol': 'oo.source.mysql.audit/v1', 'version': '1', 'product': 'MySQL', 'capture_method': 'C1',
            'applied_pos': str(baseline.get('applied_pos', load.get('applied_pos', ''))),
            'binding_vector_sha256': hashlib.sha256(binding_vector).hexdigest(),
            'collection_sha256': catalog_sha,
            'consistency_grade': baseline.get('consistency_grade', 'T'),
            'cross_table_consistent': baseline.get('cross_table_consistent', True),
            'continuous_capture': {'schedule_period_ms': 5000, 'source_snapshot_on_followup': False,
                                   'status': 'static_baseline_verified_without_source_dml',
                                   'streaming_throughput_validated': False, 'target_full_shadow_rebuild': True,
                                   'tested_gtid_transactions': []},
            'delete_detection': 'native', 'position_codec': 'mysql-gtid/v1',
            'position_lineage': baseline.get('lineage'),
            'source_id': artifacts['source_id'], 'source_generation': artifacts['generation'],
            'target_generation': load.get('target_generation'), 'source_epoch': load.get('source_epoch'),
            'evidence': {k: digest_file(native / (k + '.json')) for k in ('request', 'load', 'ack')},
            # The typed oracle runs inside follow, which records full_typed_cell_differences. The seed must not claim
            # the oracle was not run: a stale 'not_run' contradicts the follow result (pack refused at maintenance).
            'quality': quality,
            'source_contract': {'binlog_cdc_proven': False, 'cross_table_consistent': True, 'reader': 'oo_cdc',
                                'write_attempts': 0,
                                'caveats': ['No source DML was executed; binlog CDC is not proven.']}}),
    }
    manifest_sha = write_sealed(bundle, files)
    return {'bundle_files': sorted(p.name for p in bundle.iterdir()), 'columns': len(columns),
            'hidden': sorted(hidden), 'datasets': len(datasets), 'money_columns': len(money),
            'manifest_sha256': manifest_sha}


def render_template(name, values):
    """Replace whole-string placeholders with typed values; a placeholder that remains is an error."""
    def walk(node):
        if isinstance(node, dict):
            return {k: walk(v) for k, v in node.items()}
        if isinstance(node, list):
            return [walk(v) for v in node]
        if isinstance(node, str) and node.startswith('{') and node.endswith('}'):
            if node[1:-1] not in values:
                raise Refused('template_placeholder:%s' % node[1:-1])
            return values[node[1:-1]]
        if isinstance(node, str) and '{' in node:
            raise Refused('template_placeholder_partial')
        return node
    return walk(json.loads((TEMPLATE_DIR / name).read_text(encoding='utf-8')))



def run_path_in_runtime(host_path):
    """Host path -> path inside the rust runtime / trusted containers (/tmp/oo-mysql-connection-whole is mounted at the same path)."""
    return str(host_path)


def to_run_current(path):
    prefix = '/tmp/oo-mysql-connection-whole/'
    if path.startswith(prefix):
        return '/run-current/' + path[len(prefix):]
    return path


def container_binary(site, name):
    return to_run_current(site['binaries'][name]['path'])


def handler_product_runtime(ctx):
    out = ctx.out
    names = ctx.run['names']
    rid = ctx.run['run_id']
    site = ctx.site
    P_ = site['product']
    artifacts = json.loads((out / 'phases' / 'artifacts.json').read_text(encoding='utf-8'))['data']
    identity = json.loads((out / 'phases' / 'identity.json').read_text(encoding='utf-8'))['data']
    bundle_info = ensure_bundle_complete(ctx, artifacts)
    pg = resolve_alias(out, site['target']['network'], site['target']['pg_alias'])
    control_db = names['db_prefix'] + 'control_v1'
    ledger_db = names['db_prefix'] + 'ledger_v1'
    ensure_new_database(out, pg, site['target']['admin_user'], control_db, 'oo_selfbuild_api_admin')
    ensure_new_database(out, pg, site['target']['admin_user'], ledger_db, 'oo_selfbuild_api_admin')
    api_port, = free_ports(1)
    identity_port = identity['ports'][0]
    roles = json.loads((Path(artifacts['bundle']) / 'roles.json').read_text(encoding='utf-8'))
    object_scope = sorted(next(iter(roles['roles'].values())))
    tables = [{'schema': t.split('.')[0], 'table': t.split('.')[1]} for t in
              json.loads((out / 'private' / 'discovery.json').read_text(encoding='utf-8'))['tables']]
    run_secret = Path(expand(site['secret_root'])) / 'provision' / rid
    api_dir = run_secret / 'api'
    api_dir.mkdir(mode=0o700, exist_ok=True)
    scope_dir = '/owned/provision/%s' % rid
    values = {
        'API_BIND': '0.0.0.0:%d' % api_port, 'IDENTITY_ORIGIN': 'https://127.0.0.1:%d' % identity_port,
        'CONTROL_DB': control_db, 'LEDGER_DB': ledger_db,
        'MODEL_CACHE_DIR': scope_dir + '/model-cache', 'WORK_DIR': scope_dir + '/work',
        'OBJECT_SCOPE': object_scope, 'PROJECT_ID': names['logical_project'],
        'ALLOWANCE_ID': names['logical_project'] + '_allow',
        'CONFIG_COMMIT': hashlib.sha256(('config|' + rid + '|' + artifacts['catalog_sha256']).encode()).hexdigest(),
        'SOURCE_REF': artifacts['source_id'], 'SOURCE_SECRET_REF': ctx.run['source']['secret_ref'],
        'LLM_SECRET_REF': site['model']['secret_ref'], 'BUNDLE': scope_dir + '/bundle-static-v1',
        'ROLE_POLICY_FILE': scope_dir + '/bundle-static-v1/roles.json',
        'TABLES': tables, 'PG_BUILDER': to_run_current(P_['pg_builder']),
        'PIN_COMMIT': site['protocol_pins']['source_commit'],
    }
    runtime = render_template('api-runtime-v1.template.json', values)
    runtime['projects'][0]['llm']['model_id'] = site['model']['name']
    runtime['llm_allowed_models'] = [site['model']['name']]
    runtime_path = api_dir / 'api-runtime.json'
    write_json(runtime_path, runtime)
    api = names['container_prefix'] + '-api'
    secret_root = Path(expand(site['secret_root']))
    repo = Path(os.path.abspath(str(HERE.parents[1])))
    argv = ['docker', 'run', '-d', '--name', api, '--network', P_['network'],
            '-p', '127.0.0.1:%d:%d' % (api_port, api_port),
            '--mount', 'type=bind,source=%s,target=/cache,readonly' % expand(P_['cache_dir']),
            '--mount', 'type=bind,source=%s,target=/model,readonly' % P_['model_dir'],
            '--mount', 'type=bind,source=%s,target=/owned' % secret_root,
            '--mount', 'type=bind,source=%s,target=/identity-mainline,readonly' % identity_dir(out),
            '--mount', 'type=bind,source=%s,target=/repo,readonly' % repo,
            '--mount', 'type=bind,source=/tmp/oo-mysql-connection-whole,target=/run-current',
            '--memory', '8g', '--cpus', '4', P_['image'], 'sleep', 'infinity']
    run_cmd(out, 'api-create', argv)
    binary = container_binary(site, 'selfbuild_control_api')
    run_cmd(out, 'api-model-loopback', ['docker', 'exec', '-d', api, 'python3', '-I',
                                       '/repo/tools/selfbuild-control-api/recipes/model_loopback_forward.py',
                                       '--port', '8317'])
    forward_src = (HERE.parents[1] / 'tools/control-identity/recipes/mysql57-forward.py').read_text(encoding='utf-8')
    if 'PORT = 18447' not in forward_src:
        raise Refused('identity_forward_template_changed')
    forward = api_dir / 'identity-forward.py'
    write_private(forward, forward_src.replace('PORT = 18447', 'PORT = %d' % identity_port))
    run_cmd(out, 'api-identity-forward', ['docker', 'exec', '-d', api, 'python3', '-I',
                                         scope_dir + '/api/identity-forward.py'])
    config_in_container = scope_dir + '/api/api-runtime.json'
    run_cmd(out, 'api-install', ['docker', 'exec', api, binary, 'install', '--config', config_in_container],
            timeout=1800)
    # Logs go to the shared tree; inside the API container that tree is mounted at /run-current.
    host_logs = Path('/tmp/oo-mysql-connection-whole/provision') / rid / 'api'
    host_logs.mkdir(mode=0o700, parents=True, exist_ok=True)
    serve_api(out, api, binary, rid, config_in_container, api_port, host_logs)
    release = install_release_control(ctx, identity_port, scope_dir)
    return {'api_container': api, 'api_port': api_port, 'identity_port': identity_port, 'control_db': control_db,
            'ledger_db': ledger_db, 'runtime_sha256': digest_file(runtime_path), 'bundle': bundle_info,
            'release': release}


def install_release_control(ctx, identity_port, scope_dir):
    """New release-control DB + grants, release container, identity forward, official `release-control-api install`."""
    out, site, names, rid = ctx.out, ctx.site, ctx.run['names'], ctx.run['run_id']
    T, P_ = site['target'], site['product']
    pg = resolve_alias(out, T['network'], T['pg_alias'])
    db = names['db_prefix'] + 'release_control_v1'
    ensure_new_database(out, pg, T['admin_user'], db, 'oo_release_api_admin')
    dexec(out, 'release-db-grants', pg, ['psql', '-X', '-v', 'ON_ERROR_STOP=1', '-U', T['admin_user'], '-d', db],
          stdin=('GRANT CONNECT,CREATE ON DATABASE %s TO oo_release_admin;\n'
                 'GRANT CONNECT ON DATABASE %s TO oo_release_reader;\n' % (db, db)).encode())
    release_dir = Path(expand(site['secret_root'])) / 'provision' / rid / 'release'
    control = release_dir / 'control'
    control.mkdir(mode=0o700, parents=True, exist_ok=True)
    (control / 'packs').mkdir(mode=0o700, exist_ok=True)
    release_port, = free_ports(1)
    discovered = json.loads((out / 'private' / 'discovery.json').read_text(encoding='utf-8'))
    relation = discovered['tables'][0].split('.', 1)[1]
    values = {'RELEASE_DB': db, 'AUTHORITY_ID': names['logical_project'] + '-provision',
              'RELEASE_ORIGIN': 'https://127.0.0.1:%d' % release_port, 'RELEASE_PORT': release_port,
              'IDENTITY_ORIGIN': 'https://127.0.0.1:%d' % identity_port, 'PACK_DIR': scope_dir + '/release/control/packs',
              'PROJECT_ID': names['logical_project'], 'TARGET_DB': names['db_prefix'] + 'target_v1',
              'STANDBY': site['standby_container'], 'SCHEMA': ctx.run['source']['database'], 'RELATION': relation,
              'ROLE': names['role'], 'READER_LOGIN': names['reader_login'],
              'READER_PASSWORD_FILE': scope_dir + '/gateway-reader.pass'}
    runtime = render_template('release-runtime-v1.template.json', values)
    runtime_path = release_dir / 'runtime.json'
    write_json(runtime_path, runtime)
    container = names['container_prefix'] + '-release'
    secret_root = Path(expand(site['secret_root']))
    identity_mount = identity_dir(out)
    run_cmd(out, 'release-create', ['docker', 'run', '-d', '--name', container, '--network', P_['network'],
                                    '-p', '127.0.0.1:%d:%d' % (release_port, release_port),
                                    '--mount', 'type=bind,source=%s,target=/cache,readonly' % expand(P_['cache_dir']),
                                    '--mount', 'type=bind,source=%s,target=/owned' % secret_root,
                                    '--mount', 'type=bind,source=%s,target=/identity-mainline,readonly' % identity_mount,
                                    '--mount', 'type=bind,source=/tmp/oo-mysql-connection-whole,target=/run-current',
                                    '--memory', '4g', '--cpus', '2', P_['image'], 'sleep', 'infinity'])
    forward = scope_dir + '/api/identity-forward.py'
    run_cmd(out, 'release-identity-forward', ['docker', 'exec', '-d', container, 'python3', '-I', forward])
    binary = container_binary(site, 'release_control_api')
    run_cmd(out, 'release-install', ['docker', 'exec', container, binary, 'install', '--config',
                                     scope_dir + '/release/runtime.json'], timeout=1800)
    return {'container': container, 'release_port': release_port, 'origin': 'https://127.0.0.1:%d' % release_port,
            'database': db, 'runtime_sha256': digest_file(runtime_path), 'private': str(control),
            'runtime': str(runtime_path), 'binary_container': binary, 'relation': relation}


def serve_api(out, api, binary, rid, config_in_container, api_port, host_logs, label='api-serve'):
    serve_prefix = '/run-current/provision/' + rid + '/api/serve'
    host_logs.mkdir(mode=0o700, parents=True, exist_ok=True)
    for stale in ('serve.exit',):
        if (host_logs / stale).exists():
            (host_logs / stale).unlink()
    shell = ('"$@" >"' + serve_prefix + '.stdout" 2>"' + serve_prefix + '.stderr"; result=$?; '
             'printf "%s\\n" "$result" >"' + serve_prefix + '.exit"; exit "$result"')
    run_cmd(out, label, ['docker', 'exec', '-d', api, 'sh', '-c', shell, 'sh', binary, 'serve', '--config',
                         config_in_container])
    wait_for_port(out, api_port, host_logs, 180)


def stop_api_serve(out, api, label='api-stop'):
    # Match the serve process without matching this shell (the pattern is written as a character class).
    script = ('for d in /proc/[0-9]*; do c=$(tr "\\0" " " < $d/cmdline 2>/dev/null); '
              'case "$c" in *"[s]elfbuild-control-api serve"*) ;; *"selfbuild-control-api serve"*) kill ${d#/proc/} ;; esac; done; sleep 2')
    run_cmd(out, label, ['docker', 'exec', api, 'sh', '-c', script], check=False)


def sync_api_bundle(ctx, bundle_name, hidden):
    """Point the API project at this pass's sealed bundle and hidden set. Restart the API only if something changed."""
    rid = ctx.run['run_id']
    runtime_path = Path(expand(ctx.site['secret_root'])) / 'provision' / rid / 'api' / 'api-runtime.json'
    runtime = json.loads(runtime_path.read_text(encoding='utf-8'))
    bundle_dir = '/owned/provision/%s/%s' % (rid, bundle_name)
    roles = json.loads((pass_bundle_host(ctx, bundle_name) / 'roles.json').read_text(encoding='utf-8'))
    scope = sorted(next(iter(roles['roles'].values())))
    project = runtime['projects'][0]
    wanted = {'sealed_bundle': bundle_dir, 'role_policy_file': bundle_dir + '/roles.json',
              'hidden_columns': sorted(hidden)}
    scoped = [p for p in runtime['principals'] if 'object_scope' in p]
    if all(project.get(key) == value for key, value in wanted.items()) and all(p['object_scope'] == scope for p in scoped):
        return False
    project.update(wanted)
    for principal in scoped:
        principal['object_scope'] = scope
    write_json(runtime_path, runtime)
    api = ctx.run['names']['container_prefix'] + '-api'
    port = json.loads((ctx.out / 'phases' / 'product_runtime.json').read_text(encoding='utf-8'))['data']['api_port']
    binary = container_binary(ctx.site, 'selfbuild_control_api')
    stop_api_serve(ctx.out, api)
    serve_api(ctx.out, api, binary, rid, '/owned/provision/%s/api/api-runtime.json' % rid, port,
              Path('/tmp/oo-mysql-connection-whole/provision') / rid / 'api', label='api-serve-sync')
    return True


def wait_for_port(out, port, logs, seconds):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if (logs / 'serve.exit').exists():
            raise Refused('api_serve_exited')
        try:
            with socket.create_connection(('127.0.0.1', port), timeout=2):
                return
        except OSError:
            time.sleep(2)
    raise Refused('api_port_timeout')


def identity_dir(out):
    return str(Path(out) / 'identity')


# ---------------------------------------------------------------- S0-S8 (real S5 through product_flow.py)

def archive_failed_s0_s8(ctx):
    """A journaled S0-S8 failure with a build but no prepared overlay: keep that attempt as evidence and start a
    new build on resume. Only after a recorded failure, so a crash mid-poll never starts a second model build."""
    product = ctx.out / 'product'
    if not (product / 'build.json').is_file() or (product / 'prepared.json').is_file():
        return None
    # The runner journals 'started' before calling this handler, so look at the last terminal event.
    terminal = [event['event'] for event in read_journal(ctx.out)
                if event['phase'] == 's0_s8' and event['event'] in ('succeeded', 'failed')]
    if not terminal or terminal[-1] != 'failed':
        return None
    index = len(list(ctx.out.glob('product-attempt-*'))) + 1
    os.replace(product, ctx.out / ('product-attempt-%d' % index))
    product.mkdir(mode=0o700)
    return index


def run_build(ctx, bundle_name, hidden, artifacts, oracle_cells=None):
    """One S0-S8 build (product_flow --prepare-only) on the sealed bundle of one pass. Returns the build record."""
    out = ctx.out
    names = ctx.run['names']
    rid = ctx.run['run_id']
    site = ctx.site
    runtime = json.loads((out / 'phases' / 'product_runtime.json').read_text(encoding='utf-8'))['data']
    identity = json.loads((out / 'phases' / 'identity.json').read_text(encoding='utf-8'))['data']
    run_secret = Path(expand(site['secret_root'])) / 'provision' / rid
    work = run_secret / 'work'
    work.mkdir(mode=0o700, exist_ok=True)
    bundle_dir = pass_bundle_host(ctx, bundle_name)
    if bundle_name != pass_bundle_name(1):
        make_pass_bundle(ctx, artifacts, bundle_name)
    ensure_bundle_complete(ctx, artifacts, bundle=bundle_dir, hidden=hidden, oracle_cells=oracle_cells)
    api_restarted = sync_api_bundle(ctx, bundle_name, hidden)
    identity_dir_host = out / 'identity'
    api_ca = expand(site['product']['cache_dir']) + '/tls/api/ca.crt'
    endpoint = 'https://127.0.0.1:%d' % runtime['api_port']
    identity_origin = 'https://127.0.0.1:%d' % identity['ports'][0]
    cookies = {role_: str(identity_dir_host / ('fixture-%s.cookie' % role_))
               for role_ in ('admin', 'reviewer', 'owner', 'auditor')}
    for path in cookies.values():
        if not Path(path).is_file():
            raise Refused('identity_cookie_missing')
    config = {
        'endpoint': endpoint, 'origin': identity_origin, 'ca': api_ca,
        'identity_origin': identity_origin, 'identity_ca': str(identity_dir_host / 'ca.pem'),
        'cookies': cookies,
        'operator_sha256': digest_file(HERE.parent / 'selfbuild-control-api' / 'recipes' / 'operator.py'),
        'export_oos_sha256': digest_file(HERE.parent / 'selfbuild-control-api' / 'recipes' / 'export_oos.py'),
        'fixture_authorized': True,
        'create': {'allowance_id': names['logical_project'] + '_allow',
                   'config_commit': hashlib.sha256(('config|' + rid + '|' + artifacts['catalog_sha256']).encode()).hexdigest(),
                   'llm_secret_ref': site['model']['secret_ref'], 'project_id': names['logical_project'],
                   'source_ref': artifacts['source_id']},
        'catalog_sha256': artifacts['catalog_sha256'],
        'work_dir_host': str(work), 'work_dir_container': '/owned/provision/%s/work' % rid,
        'sealed_bundle_host': str(bundle_dir),
        'role': names['role'], 'model_deadline_seconds': 1800,
    }
    if config['create']['config_commit'] != json.loads(
            (run_secret / 'api' / 'api-runtime.json').read_text(encoding='utf-8'))['projects'][0]['config_commit']:
        raise Refused('config_commit_drift')
    config_path = out / 'private' / 'product.json'
    write_json(config_path, config)
    run_cmd(out, 'product-flow-prepare', [sys.executable, str(PRODUCT_FLOW), '--config', str(config_path),
                                          '--out', str(out / 'product'), '--prepare-only'], timeout=7200)
    prepared = json.loads((out / 'product' / 'prepared.json').read_text(encoding='utf-8'))
    if prepared.get('s0_s8') != 'succeeded':
        raise Refused('s0_s8_incomplete')
    s5 = json.loads((out / 'product' / 'actual-s5.json').read_text(encoding='utf-8'))
    proof = json.loads((out / 'product' / 'actual-cache-proof.json').read_text(encoding='utf-8'))
    return {'build_id': prepared['build_id'], 'overlay_sha256': prepared['overlay_sha256'], 'bundle': bundle_name,
            'hidden': sorted(hidden), 'api_restarted': api_restarted,
            's5': {'model': proof['response_identity'].get('model'), 'llm_calls': s5.get('llm_calls'),
                   'cache_hit': s5.get('cache_hit'), 'usage': proof['response_identity'].get('usage'),
                   'structural_draft': s5.get('structural_draft'), 'outcome': s5.get('outcome')}}


def pass_record(number, build):
    return {'pass': number, 'bundle': build['bundle'], 'hidden': build['hidden'], 'build_id': build['build_id'],
            'overlay_sha256': build['overlay_sha256'], 'api_restarted': build['api_restarted'], 's5': build['s5']}


def pass_summary(record):
    return {'pass': record['pass'], 'build_id': record.get('build_id'), 'bundle': record['bundle'],
            'hidden_count': len(record.get('hidden') or []), 'visible_pii': len(record.get('visible_pii') or []),
            'decision': record.get('decision')}


def handler_s0_s8(ctx):
    """Pass 1: the first S0-S8 build, on the sealed bundle that has no hidden columns."""
    attempt = archive_failed_s0_s8(ctx)
    artifacts = json.loads((ctx.out / 'phases' / 'artifacts.json').read_text(encoding='utf-8'))['data']
    build = run_build(ctx, pass_bundle_name(1), frozenset(), artifacts)
    save_pii_state(ctx.out, {'protocol': PROTOCOL_PII, 'passes': [pass_record(1, build)]})
    return {'archived_attempt': attempt, 'pass': 1, 'build_id': build['build_id'],
            'overlay_sha256': build['overlay_sha256'], 's5': build['s5'], 'hidden': build['hidden']}


def build_pass(ctx, state, artifacts, phase='pii_rebuild'):
    """Build the pass recorded last in state. A partial product of that pass is archived, never resumed."""
    out = ctx.out
    current = state['passes'][-1]
    product = out / 'product'
    if product.is_dir() and any(product.iterdir()):
        index = 1
        while (out / ('product-pass-%d.%d' % (current['pass'] - 1, index))).exists():
            index += 1
        os.replace(product, out / ('product-pass-%d.%d' % (current['pass'] - 1, index)))
    product.mkdir(mode=0o700, exist_ok=True)
    started = now()
    build = run_build(ctx, current['bundle'], set(current['hidden']), artifacts, oracle_cells=current.get('oracle_cells'))
    finished = now()
    current.update(pass_record(current['pass'], build))
    current.update(started_at=started, finished_at=finished,
                   wall_seconds=round((parse_ts(finished) - parse_ts(started)).total_seconds(), 3))
    save_pii_state(out, state)
    journal_append(out, phase, 'built', pass_no=current['pass'], build_id=build['build_id'],
                   hidden=build['hidden'], s5=build['s5'])


def oracle_bundle_name(number):
    return 'bundle-static-v1-oracle%d' % number


def typed_oracle_result(rid):
    """The follow's own typed-oracle result (typed-oracle.json) and the value it wrote to source-semantics quality.
    Both must exist and agree; nothing is inferred or defaulted."""
    base = SHARED_PROVISION / rid / 'follow' / 'follow'
    oracle_path, semantics_path = base / 'typed-oracle.json', base / 'source-semantics.json'
    if not oracle_path.is_file() or not semantics_path.is_file():
        raise Refused('typed_oracle_missing')
    oracle = json.loads(oracle_path.read_text(encoding='utf-8'))
    semantics = json.loads(semantics_path.read_text(encoding='utf-8'))
    cells = oracle.get('different_cells')
    if oracle.get('protocol') != 'oo.mysql.full-typed-oracle/v1' or type(cells) is not int or cells < 0:
        raise Refused('typed_oracle_missing')
    if semantics.get('quality', {}).get('full_typed_cell_differences') != cells:
        raise Refused('typed_oracle_drift')
    return cells


def archive_review_round(out, number):
    """Decisions of the round before the oracle rebuild are kept as evidence; the new build needs its own decisions."""
    for path in (Path(out) / 'review' / 'decisions.json', Path(out) / 'private' / 'product-decisions.json'):
        if path.exists():
            os.replace(path, path.with_name('%s-before-oracle-%d%s' % (path.stem, number, path.suffix)))


def handler_oracle_rebuild(ctx):
    """Typed-oracle round, after a publish that the API refused. The follow computed the oracle from the real source
    and target; its value (0 differences required) goes into a NEW sealed seed with the same hidden set, and a new
    build runs from S0 (real S5). Review and publish then run again. Bounded: a value the seed already carries is
    refused rather than rebuilt, and at most MAX_ORACLE_BUILDS oracle builds."""
    out = ctx.out
    artifacts = json.loads((out / 'phases' / 'artifacts.json').read_text(encoding='utf-8'))['data']
    state = load_pii_state(out)
    if not state['passes'] or state['passes'][-1].get('build_id') is None:
        raise Refused('pii_state_missing')
    final = state['passes'][-1]
    observed = typed_oracle_result(ctx.run['run_id'])
    if observed != 0:
        raise Refused('typed_oracle_mismatch')
    if final.get('oracle_cells') == observed and final.get('seed_version', 1) >= SEED_CONTENT_VERSION:
        raise Refused('publish_refused_after_oracle')
    number = sum(1 for record in state['passes'] if record.get('kind') == 'oracle') + 1
    if number > MAX_ORACLE_BUILDS:
        raise Refused('oracle_rebuild_exhausted')
    archive_review_round(out, number)
    state['passes'].append({'pass': final['pass'] + 1, 'kind': 'oracle', 'bundle': oracle_bundle_name(number),
                            'hidden': list(final['hidden']), 'build_id': None, 'oracle_cells': observed,
                            'seed_version': SEED_CONTENT_VERSION})
    save_pii_state(out, state)
    journal_append(out, 'oracle_rebuild', 'declared', pass_no=state['passes'][-1]['pass'], oracle_cells=observed,
                   hidden=final['hidden'])
    build_pass(ctx, state, artifacts, phase='oracle_rebuild')
    current = state['passes'][-1]
    if product_build_id(out) != current['build_id']:
        raise Refused('pii_state_drift')
    visible = visible_pii_items(out, current['hidden'])
    if visible:
        raise Refused('oracle_build_visible_pii')
    write_withheld(out)
    return {'pass': current['pass'], 'build_id': current['build_id'], 'bundle': current['bundle'],
            'hidden': current['hidden'], 'oracle_cells': observed, 's5': current.get('s5')}


def handler_pii_rebuild(ctx):
    """Two-pass build. A build that shows a visible column as PII (pii != none) declares the column hidden (automated
    control_plane_test declaration) and is followed by a new build on a bundle without it. At most MAX_PII_BUILDS
    builds; still visible after the last one fails with pii_unstable. Every pass is journaled and kept."""
    out = ctx.out
    artifacts = json.loads((out / 'phases' / 'artifacts.json').read_text(encoding='utf-8'))['data']
    state = load_pii_state(out)
    if not state['passes'] or state['passes'][0].get('build_id') is None:
        raise Refused('pii_state_missing')
    while True:
        current = state['passes'][-1]
        if current.get('build_id') is None:
            build_pass(ctx, state, artifacts)
            continue
        if product_build_id(out) != current['build_id']:
            raise Refused('pii_state_drift')
        visible = visible_pii_items(out, current['hidden'])
        current['visible_pii'] = [{'name': item['name'], 'pii': item['pii']} for item in visible]
        current['decision'] = 'rebuild' if visible else 'clean'
        save_pii_state(out, state)
        journal_append(out, 'pii_rebuild', 'classified', pass_no=current['pass'], build_id=current['build_id'],
                       hidden=current['hidden'], visible_pii=current['visible_pii'], decision=current['decision'])
        if not visible:
            break
        if len(state['passes']) >= MAX_PII_BUILDS:
            raise Refused('pii_unstable')
        declare_hidden(out, current['build_id'], visible)
        hidden = sorted(set(current['hidden']) | {item['name'] for item in visible})
        state['passes'].append({'pass': current['pass'] + 1, 'bundle': pass_bundle_name(current['pass'] + 1),
                                'hidden': hidden, 'build_id': None})
        save_pii_state(out, state)
        journal_append(out, 'pii_rebuild', 'declared', pass_no=current['pass'] + 1, hidden=hidden)
    write_withheld(out)
    return {'builds': len(state['passes']), 'final_build_id': state['passes'][-1]['build_id'],
            'hidden': sorted(hidden_names(out)), 'passes': [pass_summary(p) for p in state['passes']]}


# ---------------------------------------------------------------- phase handlers

def handler_validate(ctx):
    six = validate_six(load_private_json(ctx.out / 'six.json'))
    registry = load_private_json(ctx.out / 'private' / 'secret-registry.json', code='registry_invalid')
    if registry.get(six['secret_ref']) is None:
        raise Refused('secret_binding')
    pw_path = Path(registry[six['secret_ref']])
    if pw_path.parent.resolve().parent != ctx.secret_root.resolve() / 'provision':
        raise Refused('secret_root')
    read_password(pw_path)
    if ctx.run.get('profile') is not None:
        if registry.get(ctx.run['profile']['secret_ref']) is None:
            raise Refused('secret_binding')
        read_password(Path(registry[ctx.run['profile']['secret_ref']]))
    verified = verify_site_binaries(ctx.site)
    return {'secret_ref': six['secret_ref'], 'format': ctx.run['format'], 'egress': ctx.run['egress'],
            'binaries_verified': sorted(verified)}


def handler_egress(ctx):
    six = validate_six(load_private_json(ctx.out / 'six.json'))
    if ctx.run['egress'] == 'direct':
        return {'mode': 'direct'}
    return egress_eip(ctx, six)


def handler_pack(ctx):
    receipt = ctx.out / 'product' / 'product-receipt.json'
    if not receipt.is_file():
        raise Refused('pack_incomplete')
    doc = json.loads(receipt.read_text(encoding='utf-8'))
    stages = {s.get('stage'): s.get('status') for s in doc.get('stages', []) if isinstance(s, dict)}
    if not all(stages.get(stage) == 'succeeded' for stage in STAGES) or not doc.get('pack'):
        raise Refused('pack_incomplete')
    return {'build_id': doc.get('build_id'), 'pack_artifact_sha256': doc.get('pack_artifact_sha256')}


# ---------------------------------------------------------------- publish, replica, gateway, token, smoke, ready

def phase_data(out, name):
    return json.loads((Path(out) / 'phases' / (name + '.json')).read_text(encoding='utf-8'))['data']


def write_release_helpers(ctx):
    """Per-scope release/mapping/coordinator configs and the gateway seed (shared token runtime material + own reader)."""
    site, names, rid, out = ctx.site, ctx.run['names'], ctx.run['run_id'], ctx.out
    P_, T = site['product'], site['target']
    run_secret = Path(expand(site['secret_root'])) / 'provision' / rid
    release_dir = run_secret / 'release'
    release_dir.mkdir(mode=0o700, parents=True, exist_ok=True)
    db = names['db_prefix'] + 'target_v1'
    login = names['reader_login']
    target_admin_pw = '/secrets/c1/standby-v1/target-admin-restored-20261015.hex'
    reader_pw = '/secrets/provision/%s/gateway-reader.pass' % rid
    write_json(release_dir / 'mapping-template.json', {
        'ca_file': '/secrets/pg-ca.crt', 'endpoint': {'database': db, 'host': T['pg_alias'], 'port': 5432,
                                                     'user': 'oo_target_admin'},
        'login': login, 'password_file': target_admin_pw, 'policies': [],
        'protocol': 'oo.gateway.provision-native-vector-mapping-reader-cli/v1'})
    write_json(release_dir / 'target-coordinator.json', {
        'ca_file': '/secrets/pg-ca.crt', 'database': db, 'host': T['pg_alias'], 'password_file': target_admin_pw,
        'port': 5432, 'reader_password_file': reader_pw, 'reader_user': login, 'user': 'oo_target_admin'})
    seed = run_secret / 'gateway-seed'
    seed.mkdir(mode=0o700, exist_ok=True)
    token_seed = Path(expand(P_['token_seed']))
    for name in ('ca.crt', 'runtime.pass', 'endpoint.json'):
        write_private(seed / name, (token_seed / name).read_bytes())
    write_private(seed / 'data.pass', (run_secret / 'gateway-reader.pass').read_bytes())
    config = json.loads((token_seed / 'config.json').read_text(encoding='utf-8'))
    config['data_user'] = login
    write_json(seed / 'config.json', config)
    (seed / 'reviewed' / 'mysql_full').mkdir(mode=0o700, parents=True, exist_ok=True)
    catalog = json.loads((Path(phase_data(out, 'artifacts')['bundle']) / 'catalog.json').read_text(encoding='utf-8'))
    write_json(seed / 'reviewed' / 'mysql_full' / 'native-input.json',
               {'catalog': catalog, 'provision_seed_only': True})
    return run_secret


def profile_access_block(ctx):
    """`onboarding.product.profile_access` (P12): exactly {user, secret_ref}. Never overrides the CDC binding."""
    profile = ctx.run.get('profile')
    if profile is None:
        raise Refused('profile_credential_required')
    return {'user': profile['user'], 'secret_ref': profile['secret_ref']}


def preflight_query(ctx, schema):
    """The follow's preflight must succeed on the role's own columns: a SELECT of a visible identifier of the source
    schema. A withheld column is refused by the gateway, and smoke checks that separately."""
    passes = load_pii_state(ctx.out)['passes']
    if not passes:
        raise Refused('pii_state_missing')
    roles = json.loads((pass_bundle_host(ctx, passes[-1]['bundle']) / 'roles.json').read_text(encoding='utf-8'))
    visible = sorted(roles['roles'].get(ctx.run['names']['role'], []))
    for column in visible:
        dataset, _, name = column.rpartition('.')
        if name == 'id' and dataset.startswith(schema + '.'):
            return 'SELECT id FROM %s WHERE id = 1' % dataset
    for column in visible:
        dataset, _, name = column.rpartition('.')
        parts = dataset.split('.')
        if len(parts) == 2 and parts[0] == schema and all(
                re.fullmatch(r'[A-Za-z_][A-Za-z0-9_]*', value) for value in [*parts, name]):
            return 'SELECT %s FROM %s LIMIT 1' % (name, dataset)
    raise Refused('preflight_query_missing')


def operator_product(ctx):
    """The `product` block that source-onboarding-linux follow reads (keys as in the reference static-ready operator)."""
    site, names, rid, out = ctx.site, ctx.run['names'], ctx.run['run_id'], ctx.out
    P_, b = site['product'], site['binaries']
    run_secret = Path(expand(site['secret_root'])) / 'provision' / rid
    product_rt = phase_data(out, 'product_runtime')
    release = product_rt['release']
    db = names['db_prefix'] + 'target_v1'
    schema = ctx.run['source']['database']
    receipt_sha = digest_file(out / 'product' / 'product-receipt.json')
    passes = load_pii_state(out)['passes']
    if not passes:
        raise Refused('pii_state_missing')
    final_bundle = passes[-1]['bundle']
    return {
        'api_binary': b['selfbuild_control_api']['path'],
        'api_binary_container': container_binary(site, 'selfbuild_control_api'),
        'api_container': product_rt['api_container'],
        'api_runtime': str(run_secret / 'api' / 'api-runtime.json'),
        'bundle_parent': str(run_secret),
        'cli_binary': b['cli']['path'],
        'coordinator_binary': b['release_coordinator']['path'],
        'coordinator_config_container': '/secrets/provision/%s/release/target-coordinator.json' % rid,
        'fixed_bin_root': P_['fixed_bin_root'],
        'gateway_binary': b['gateway']['path'],
        'gateway_container_prefix': names['container_prefix'] + '-g',
        'gateway_data_ca': expand(P_['standby_ca']),
        'gateway_data_endpoint': {'database': db, 'host': site['standby_container'], 'port': 5432},
        'gateway_image': P_['image'],
        'gateway_login': names['reader_login'],
        'gateway_networks': list(P_['gateway_networks']),
        'mapping_binary': b['provision_mapping_reader']['path'],
        'mapping_template': str(run_secret / 'release' / 'mapping-template.json'),
        'preflight_sql': [preflight_query(ctx, schema)],
        'previous_gateway': str(run_secret / 'gateway-seed'),
        'previous_release_binary_container': release['binary_container'],
        'previous_selfbuild_binary_container': container_binary(site, 'selfbuild_control_api'),
        'product_config': str(out / 'private' / 'product.json'),
        'release_binary': b['release_control_api']['path'],
        'release_binary_container': release['binary_container'],
        'release_container': release['container'],
        'release_origin': release['origin'],
        'release_private': release['private'],
        'release_runtime': release['runtime'],
        'release_suffix': names['logical_project'],
        'reuse_product': str(out / 'product'),
        'reuse_receipt': receipt_sha,
        'seed_bundle': str(run_secret / final_bundle),
        'source_access': 'network',
        'profile_access': profile_access_block(ctx),
        'source_client_file': P_['source_client_file'],
        'source_container': P_['source_container'],
        'token_admin_password_file': str(run_secret / 'token-admin.pass'),
        'token_binary': b['issue_token']['path'],
    }


def token_admin_credential(site):
    """The token store's admin credential is one shared secret (role oo_token_admin, the same file in every gateway
    generation). The run copies it from the site config; a generated value would not authenticate to the store."""
    source = private_file(expand(site['product']['token_admin_source']))
    data = source.read_bytes()
    if not data.strip():
        raise Refused('token_admin_source')
    return data


def build_static_operator(ctx):
    """Static-ready operator: the capture operator with the follow-time product block (no secret values).

    The six-field binding stays the CDC account (follow compares it with the captured binding). Network source SQL
    uses `onboarding.product.profile_access` (P12) for the SELECT-only profile account.
    """
    op = json.loads((ctx.out / 'private' / 'operator-capture.json').read_text(encoding='utf-8'))
    op['onboarding']['after_generation'] = 0
    op['onboarding']['runtime_helper_container'] = to_run_current(ctx.site['binaries']['source_onboarding']['path'])
    op['onboarding']['product'] = operator_product(ctx)
    path = ctx.out / 'private' / 'operator-static-ready.json'
    write_json(path, op)
    return path


FOLLOW_ERROR = re.compile(r'^(?:source-onboarding:\s*)?([a-z][a-z0-9_:.\-]{0,79})(?:;.*)?$')


def follow_error_code(stderr_path):
    """A stable code from follow's last stderr line, e.g. 'source-onboarding: owned_fixture_only; ...'.
    Only the code is returned; the raw text is never copied."""
    try:
        lines = [line.strip() for line in Path(stderr_path).read_text(encoding='utf-8', errors='replace').splitlines()
                 if line.strip()]
    except OSError:
        return 'unknown'
    match = FOLLOW_ERROR.match(lines[-1]) if lines else None
    return match.group(1) if match else 'unknown'


def archive_publish_attempt(shared, log, run_secret=None):
    """Keep a failed follow attempt as evidence (never overwrite it) so the publish phase can be retried.
    The follow also writes rust-bundle / rust-gateway copies under the run secret root, named by the output path, so
    a retry would collide with them (immutable_output_drift). Those move with the attempt."""
    index = len(list(shared.glob('publish-attempt-*.exit'))) + 1
    base = shared / ('publish-attempt-%d' % index)
    for suffix in ('.exit', '.stdout', '.stderr'):
        source = Path(str(log) + suffix)
        if source.exists():
            os.replace(source, str(base) + suffix)
    if (shared / 'follow').exists():
        os.replace(shared / 'follow', shared / ('follow-attempt-%d' % index))
    if run_secret is not None:
        moved = [path for pattern in ('rust-bundle-g*', 'rust-gateway-g*') for path in Path(run_secret).glob(pattern)]
        if moved:
            target = Path(run_secret) / ('publish-attempt-%d' % index)
            target.mkdir(mode=0o700, exist_ok=True)
            for path in moved:
                os.replace(path, target / path.name)


def handler_publish(ctx):
    """Pinned source-onboarding-linux follow inside the runtime container: release plan/apply, mapping, pack, gateway."""
    out, site, names, rid = ctx.out, ctx.site, ctx.run['names'], ctx.run['run_id']
    write_release_helpers(ctx)
    operator = build_static_operator(ctx)
    shared = SHARED_PROVISION / rid
    shared.mkdir(mode=0o700, parents=True, exist_ok=True)
    follow_out = shared / 'follow'
    log = shared / 'publish'
    if follow_out.exists() or Path(str(log) + '.exit').exists():
        archive_publish_attempt(shared, log, Path(expand(site['secret_root'])) / 'provision' / rid)
    binary = site['binaries']['source_onboarding']['path']
    shell = ('"$@" >"' + str(log) + '.stdout" 2>"' + str(log) + '.stderr"; result=$?; '
             'printf "%s\\n" "$result" >"' + str(log) + '.exit"; exit "$result"')
    run_cmd(out, 'follow-launch', ['docker', 'exec', '-d', site['runtime_container'], 'sh', '-c', shell, 'sh',
                                   binary, 'follow', '--input', str(producer_six_path(ctx)),
                                   '--operator', str(operator), '--out', str(follow_out)])
    exit_file = Path(str(log) + '.exit')
    deadline = time.monotonic() + site.get('follow_deadline_seconds', 7200)
    while not exit_file.exists():
        if time.monotonic() > deadline:
            raise Refused('follow_timeout')
        time.sleep(10)
    code = int(exit_file.read_text().strip() or '1')
    if code != 0:
        raise Refused('follow_failed:%s' % follow_error_code(str(log) + '.stderr'))
    ready_path = follow_out / 'follow' / 'gateway-ready.json'
    if not ready_path.is_file():
        raise Refused('gateway_ready_missing')
    ready = json.loads(ready_path.read_text(encoding='utf-8'))
    if ready.get('ready') is not True or ready.get('readonly_replica') is not True or ready.get('role') != names['role']:
        raise Refused('gateway_not_ready')
    return {'follow_out': str(follow_out), 'gateway_ready_sha256': digest_file(ready_path),
            'gateway_container': ready['gateway_container'], 'private_root': ready['private_root'],
            'pack_digest': ready['pack_digest'], 'source_generation': ready['source_generation'],
            'role': ready['role'], 'token_sha256': ready['token_sha256'], 'follow_exit': code}


def handler_replica_check(ctx):
    """Physical standby: in recovery, read-only session, reader role can SET and reads over verified TLS."""
    out, site, names, rid = ctx.out, ctx.site, ctx.run['names'], ctx.run['run_id']
    standby = site['standby_container']
    network = site['target']['network']
    _, inspect_raw = run_cmd(out, 'standby-inspect', ['docker', 'inspect', '-f', '{{json .NetworkSettings.Networks}}',
                                                       standby], check=False)
    if network not in json.loads(inspect_raw or b'{}'):
        raise Refused('standby_network')
    run_secret = Path(expand(site['secret_root'])) / 'provision' / rid
    reader = (run_secret / 'gateway-reader.pass').read_bytes().strip().decode()
    db = names['db_prefix'] + 'target_v1'
    login = names['reader_login']
    pgpass = run_secret / 'standby.pgpass'
    write_private(pgpass, '%s:5432:%s:%s:%s\n' % (standby, db, login, reader))
    del reader
    ca = expand(site['product']['standby_ca'])
    conn = 'host=%s port=5432 dbname=%s user=%s sslmode=verify-full sslrootcert=/ca/ca.crt' % (standby, db, login)
    sql = ("SELECT pg_is_in_recovery(), current_setting('transaction_read_only'), current_user, "
           "pg_has_role(current_user, '%s', 'SET'), (SELECT ssl FROM pg_stat_ssl WHERE pid = pg_backend_pid())"
           % names['role'])
    _, raw = run_cmd(out, 'replica-reader', [
        'docker', 'run', '--rm', '--network', network,
        '--mount', 'type=bind,source=%s,target=/private/pgpass,readonly' % pgpass,
        '--mount', 'type=bind,source=%s,target=/ca/ca.crt,readonly' % ca,
        '-e', 'PGPASSFILE=/private/pgpass', site['product']['pg_client_image'],
        'psql', conn, '-X', '-Atc', sql])
    observed = raw.decode('utf-8').strip()
    expected = 't|on|%s|t|t' % login
    if observed != expected:
        raise Refused('replica_check_failed')
    return {'standby': standby, 'database': db, 'reader': login, 'checks': ['in_recovery', 'read_only_session',
                                                                           'role_set', 'tls'],
            'result': expected}


def gateway_container_facts(out, container):
    _, raw = run_cmd(out, 'gateway-inspect', ['docker', 'inspect', container], check=False)
    doc = json.loads(raw or b'[]')
    if not doc:
        raise Refused('gateway_missing')
    return doc[0]


def handler_gateway(ctx):
    """Gateway container (two mounts only: /fixed read-only, /run-current) on the reader's network, listening."""
    out, site = ctx.out, ctx.site
    pub = phase_data(out, 'publish')
    facts = gateway_container_facts(out, pub['gateway_container'])
    if facts.get('State', {}).get('Running') is not True:
        raise Refused('gateway_not_running')
    mounts = sorted(m['Destination'] for m in facts.get('Mounts', []))
    if mounts != ['/fixed', '/run-current']:
        raise Refused('gateway_mounts')
    networks = sorted(facts.get('NetworkSettings', {}).get('Networks', {}))
    if not set(site['product']['gateway_networks']) <= set(networks):
        raise Refused('gateway_network')
    listen = (Path(pub['private_root']) / 'listen.addr').read_text(encoding='utf-8').strip()
    if not listen:
        raise Refused('gateway_not_listening')
    return {'container': pub['gateway_container'], 'mounts': mounts, 'networks': networks, 'listen_addr': listen,
            'readonly_fixed_mount': True}


def handler_token(ctx):
    """Client token issued by the pinned issue-token inside follow: 0600, hash-bound to the ready record, TTL 3600000."""
    out = ctx.out
    pub = phase_data(out, 'publish')
    root = Path(pub['private_root'])
    token = private_file(root / 'current-client.token')
    token_sha = digest_file(token)
    if token_sha != pub['token_sha256']:
        raise Refused('token_hash_mismatch')
    grant = json.loads((root / 'grant.json').read_text(encoding='utf-8'))
    if grant.get('client_ttl_ms') != 3600000:
        raise Refused('token_ttl')
    return {'token_file_name': 'current-client.token', 'token_sha256': token_sha, 'client_ttl_ms': 3600000,
            'role': grant.get('role')}


def cli_call(out, container, cli_dir, gateway, token_name, label, args, expect_ok=True):
    flags = ['--json', '--gateway', 'http://' + gateway, '--auth', 'external', '--transport', 'local-development',
             '--token-file', '/run-current/' + token_name]
    code, raw = run_cmd(out, label, ['docker', 'exec', container, cli_dir] + list(args) + flags, check=False,
                        timeout=120)
    if expect_ok and code != 0:
        raise Refused('smoke_failed:%s' % label)
    return code, raw


def handler_smoke(ctx):
    """Through the gateway container: oo freshness | tables | query (counts vs the profile), withheld columns refused."""
    out, names = ctx.out, ctx.run['names']
    pub = phase_data(out, 'publish')
    gw = phase_data(out, 'gateway')
    token = phase_data(out, 'token')['token_file_name']
    container = pub['gateway_container']
    cli_path = Path(ctx.site['binaries']['cli']['path'])
    cli_dir = '/fixed/%s/%s' % (cli_path.parent.name, cli_path.name)
    _, raw = cli_call(out, container, cli_dir, gw['listen_addr'], token, 'smoke-freshness', ['freshness'])
    fresh = json.loads(raw)
    if fresh.get('publication') != 'live' or not fresh.get('sources'):
        raise Refused('smoke_freshness')
    _, raw = cli_call(out, container, cli_dir, gw['listen_addr'], token, 'smoke-tables', ['tables'])
    tables = {t['name']: t for t in json.loads(raw).get('tables', [])}
    artifacts = phase_data(out, 'artifacts')
    expected = artifacts['rows_by_table']
    if set(tables) != set(expected):
        raise Refused('smoke_tables')
    counts = {}
    for dataset, rows in sorted(expected.items()):
        _, raw = cli_call(out, container, cli_dir, gw['listen_addr'], token, 'smoke-count-' + dataset.split('.')[1],
                          ['query', 'SELECT COUNT(*) AS n FROM %s' % dataset])
        got = json.loads(raw)['rows']
        if got != [[str(rows)]]:
            raise Refused('smoke_count')
        counts[dataset] = rows
    withheld = sorted(json.loads(withheld_path(out).read_text(encoding='utf-8'))['withheld'])
    # Hidden from the role: PII-withheld columns, plus columns the reviewer rejected (they never enter the role).
    rejected = []
    decisions_path = out / 'review' / 'decisions.json'
    if decisions_path.is_file():
        rejected = [r['name'] for r in json.loads(decisions_path.read_text(encoding='utf-8'))['reviews']
                    if r['decision'] == 'rejected']
    hidden = sorted(set(withheld) | set(rejected))
    refused = []
    for name in hidden:
        dataset, column = name.rsplit('.', 1)
        code, _ = cli_call(out, container, cli_dir, gw['listen_addr'], token,
                           'smoke-withheld-' + column, ['query', 'SELECT `%s` FROM %s LIMIT 1' % (column, dataset)],
                           expect_ok=False)
        if code == 0:
            raise Refused('withheld_column_queryable')
        refused.append(name)
    return {'freshness_publication': fresh.get('publication'), 'tables': sorted(tables), 'counts': counts,
            'withheld_refused': [n for n in refused if n in withheld],
            'rejected_refused': [n for n in refused if n in rejected], 'role': names['role']}


def handler_ready(ctx):
    """Scope record (contract section 2), oo-current style alias, ready pointer. Written only after every check passed."""
    out, site, names, rid = ctx.out, ctx.site, ctx.run['names'], ctx.run['run_id']
    pub = phase_data(out, 'publish')
    artifacts = phase_data(out, 'artifacts')
    s0 = phase_data(out, 's0_s8')
    prepared_root = SHARED_PROVISION / rid / 'prepared'
    ready_record = prepared_root / 'rust-current-gateway.json'
    cli_path = site['binaries']['cli']['path']
    scope = {'source_id': artifacts['source_id'], 'label': ctx.run['label'],
             'input_path': str(out / 'six.json'), 'operator_path': str(out / 'private' / 'operator-static-ready.json'),
             'alias_path': str(out / 'oo-current.json'), 'ready_record': str(ready_record),
             'cli_binary': cli_path, 'container': pub['gateway_container'], 'role': names['role'],
             'build_id': load_pii_state(out)['passes'][-1]['build_id'], 'product_config': str(out / 'private' / 'product.json'),
             'token_file_name': phase_data(out, 'token')['token_file_name']}
    if ctx.run['egress'] == 'eip':
        scope['transport_input_path'] = str(private_file(out / 'private' / 'producer-six.json'))
    write_json(out / 'scope.json', scope)
    write_json(out / 'oo-current.json', {'protocol': 'oo.source-onboarding.oo-current/v1', 'ready_record': str(ready_record),
                                         'cli_binary': cli_path, 'container': pub['gateway_container'],
                                         'role': names['role'], 'source_id': artifacts['source_id']})
    return scope


def handler_review(ctx):
    product = ctx.out / 'product'
    config = ctx.out / 'private' / 'product.json'
    if not config.is_file():
        raise Refused('product_config_missing')
    _, body = run_cmd(ctx.out, 'product-flow-decisions',
                      [sys.executable, str(PRODUCT_FLOW), '--config', str(config), '--out', str(product),
                       '--decisions', str(ctx.out / 'private' / 'product-decisions.json')], timeout=7200)
    return {'product_flow': 'decisions_applied', 'stdout_sha256': hashlib.sha256(body).hexdigest()}


def not_implemented(name):
    def handler(ctx):
        raise Refused('phase_not_implemented:%s' % name)
    return handler


HANDLERS = {name: not_implemented(name) for name in PHASES}
HANDLERS.update({
    'validate': handler_validate,
    'egress': handler_egress,
    'identity': handler_identity,
    'target': handler_target,
    'capture': handler_capture,
    'artifacts': handler_artifacts,
    'product_runtime': handler_product_runtime,
    's0_s8': handler_s0_s8,
    'pii_rebuild': handler_pii_rebuild,
    'oracle_rebuild': handler_oracle_rebuild,
    'review': handler_review,
    'pack': handler_pack,
    'publish': handler_publish,
    'replica_check': handler_replica_check,
    'gateway': handler_gateway,
    'token': handler_token,
    'smoke': handler_smoke,
    'ready': handler_ready,
})


def next_phase(out, summary):
    """The first phase not complete. oracle_rebuild is not in the plan: it runs only when publish failed with a
    typed-oracle code (see oracle_retry_due)."""
    for name in PHASES:
        if name != 'oracle_rebuild' and not phase_complete(out, name, summary):
            return name
    return None


def oracle_round_done(out):
    """The final sealed pass was already built by an oracle round: a refused publish is then retried, not rebuilt."""
    passes = load_pii_state(out)['passes']
    return bool(passes) and passes[-1].get('kind') == 'oracle'


def oracle_retry_due(summary):
    entry = summary.get('publish', {})
    return entry.get('state') == 'failed' and entry.get('error_code') in ORACLE_RETRY_CODES


def runner(out, handlers=None):
    """Run pending phases in order. Returns the final run state."""
    handlers = HANDLERS if handlers is None else handlers
    out = Path(out)
    lock = os.open(out / 'runner.lock', os.O_RDWR | os.O_CREAT, 0o600)
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except OSError:
        return 'runner_active'
    journal_append(out, '_run', 'state', state='running')
    try:
        ctx = Ctx(out, load_site())
    except Refused as exc:
        journal_append(out, '_run', 'state', state='failed', error_code=exc.code)
        return 'failed:%s' % exc.code
    try:
        while True:
            summary = phase_summary(read_journal(out))
            name = next_phase(out, summary)
            if name is None:
                journal_append(out, '_run', 'state', state='ready')
                return 'ready'
            if name == 'awaiting_review':
                journal_append(out, name, 'awaiting')
                journal_append(out, '_run', 'state', state='awaiting_review')
                return 'awaiting_review'
            if name == 'publish' and oracle_retry_due(summary) and not oracle_round_done(out):
                name = 'oracle_rebuild'
            started = now()
            journal_append(out, name, 'started')
            try:
                data = handlers[name](ctx) or {}
            except Refused as exc:
                journal_append(out, name, 'failed', error_code=exc.code)
                journal_append(out, '_run', 'state', state='failed', error_code=exc.code)
                teardown_forward(out)
                return 'failed:%s' % exc.code
            except Exception as exc:  # any unexpected failure must still be journaled, with a type-only code
                code = 'internal_error:%s' % type(exc).__name__
                journal_append(out, name, 'failed', error_code=code)
                journal_append(out, '_run', 'state', state='failed', error_code=code)
                teardown_forward(out)
                return 'failed:%s' % code
            record_phase(out, name, started, data)
            journal_append(out, name, 'succeeded')
    finally:
        fcntl.flock(lock, fcntl.LOCK_UN)
        os.close(lock)


def spawn_runner(out):
    out = Path(out)
    log = os.open(out / 'runner.log', os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o600)
    proc = subprocess.Popen([sys.executable, str(Path(__file__).resolve()), '_runner', '--out', str(out)],
                            stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True,
                            close_fds=True, cwd=str(HERE))
    os.close(log)
    write_private(out / 'runner.pid', str(proc.pid))
    return proc.pid


def runner_alive(out):
    pid_file = Path(out) / 'runner.pid'
    if not pid_file.is_file():
        return False
    try:
        pid = int(pid_file.read_text().strip())
        os.kill(pid, 0)
    except (ValueError, OSError):
        return False
    code, cmd = run_cmd_quiet(['ps', '-o', 'command=', '-p', str(pid)])
    return code == 0 and b'_runner' in cmd


# ---------------------------------------------------------------- review and decisions

PII_REVIEWABLE = 'none'


def overlay_items(out):
    path = Path(out) / 'product' / 'prepared-overlay.json'
    if not path.is_file():
        raise Refused('review_not_ready')
    overlay = json.loads(path.read_text(encoding='utf-8'))
    items = []
    for column in overlay.get('columns', []):
        name = '%s.%s' % (column['dataset'], column['name'])
        pii = column.get('pii', 'none')
        reviewable = pii == PII_REVIEWABLE
        items.append({'name': name, 'dataset': column['dataset'], 'column': column['name'],
                      'data_type': column.get('data_type'),
                      'semantic_type': column.get('semantic_type') or column.get('sealed_data_type'),
                      'label': column.get('label', ''), 'pii': pii,
                      'reviewable': reviewable,
                      'withheld_reason': None if reviewable else 'pii_%s' % pii,
                      'binds_fact': column.get('fact_sha256'),
                      'classification_state': column.get('classification_state', 'pending')})
    if not items:
        raise Refused('review_not_ready')
    return overlay, items


def model_semantics(out, build):
    """The builder's per-column semantic type and pii (native-model-output/decisions.json), keyed by name."""
    if not build:
        return {}
    run_id = load_run(out)['run_id']
    path = (Path(expand(load_site()['secret_root'])) / 'provision' / run_id / 'work' / 'builds' / build
            / 'native-model-output' / 'decisions.json')
    if not path.is_file():
        return {}
    rows = json.loads(path.read_text(encoding='utf-8'))
    return {'%s.%s' % (r['dataset'], r['column']): r for r in rows}


def accepted_reviews(out):
    """Reviews the API recorded (HTTP 200 in the product evidence)."""
    accepted = set()
    for path in sorted((Path(out) / 'product').glob('*-review.response.json')):
        doc = json.loads(path.read_text(encoding='utf-8'))
        if doc.get('status') == 200:
            accepted.add(doc['request']['name'])
    return accepted


def review_items(out):
    """Overlay items with the effective semantic type and PII class (builder output over the overlay), plus the
    declared hidden columns. A declared column is withheld (not reviewable) whatever the overlay says."""
    overlay, items = overlay_items(out)
    proposed = model_semantics(out, overlay.get('build_id'))
    declared = {column['name']: column for column in load_hidden_declaration(out)['columns']}
    for item in items:
        row = proposed.get(item['name'])
        if row is not None:
            item['semantic_type'] = row.get('semantic_type') or item['semantic_type']
            item['pii'] = row.get('pii') or item['pii']
        if item['name'] in declared:
            item['pii'] = declared[item['name']]['pii']
    present = {item['name'] for item in items}
    for name, column in sorted(declared.items()):
        if name not in present:
            items.append({'name': name, 'dataset': column['dataset'], 'column': column['column'],
                          'data_type': column.get('data_type'), 'semantic_type': column.get('semantic_type'),
                          'label': '', 'pii': column['pii'], 'binds_fact': None,
                          'classification_state': 'withheld'})
    for item in items:
        item['reviewable'] = item['pii'] == PII_REVIEWABLE and item['name'] not in declared
        item['withheld_reason'] = None if item['reviewable'] else 'pii_%s' % item['pii']
    return overlay, items


def review_document(out):
    overlay, items = review_items(out)
    decisions = {}
    path = Path(out) / 'review' / 'decisions.json'
    if path.is_file():
        decisions = {r['name']: r['decision'] for r in json.loads(path.read_text(encoding='utf-8'))['reviews']}
    accepted = accepted_reviews(out)
    for item in items:
        item['decision'] = decisions.get(item['name'])
        item['accepted'] = item['name'] in accepted
    stage_states = {'S9': 'awaiting', 'S10': 'awaiting'}
    if path.is_file():
        stage_states = {'S9': 'submitted', 'S10': 'submitted'}
    declaration = load_hidden_declaration(out)
    return {'protocol': PROTOCOL_REVIEW, 'build_id': overlay.get('build_id'), 'stage_states': stage_states,
            'items': items,
            'hidden_declaration': {'declared_by': declaration.get('declared_by'),
                                   'signer_class': declaration.get('signer_class'),
                                   'business_owner_signed': declaration.get('business_owner_signed'),
                                   'basis': declaration.get('basis'),
                                   'columns': [column['name'] for column in declaration['columns']]}}


def validate_decisions(doc, items):
    if not isinstance(doc, dict) or set(doc) != {'reviews', 'confirm_columns'}:
        raise Refused('decisions_shape')
    if not isinstance(doc['reviews'], list) or not isinstance(doc['confirm_columns'], list):
        raise Refused('decisions_shape')
    if not all(isinstance(name, str) for name in doc['confirm_columns']):
        raise Refused('confirm_columns_mismatch')
    if any(item['pii'] == 'unknown' for item in items):
        raise Refused('pii_unknown')
    by_name = {item['name']: item for item in items}
    withheld = {item['name'] for item in items if not item['reviewable']}
    seen = {}
    for review in doc['reviews']:
        if not isinstance(review, dict) or set(review) != {'name', 'binds_fact', 'decision'}:
            raise Refused('review_shape')
        name = review['name']
        if not isinstance(name, str):
            raise Refused('review_shape')
        if name in seen:
            raise Refused('review_duplicate')
        if name in withheld:
            raise Refused('withheld_item_submitted')
        if name not in by_name:
            raise Refused('review_unknown_item')
        if not isinstance(review['binds_fact'], str) or not HEX64.match(review['binds_fact']):
            raise Refused('binds_fact_invalid')
        if review['binds_fact'] != by_name[name]['binds_fact']:
            raise Refused('binds_fact_mismatch')
        if review['decision'] not in DECISION_VALUES:
            raise Refused('decision_invalid')
        if review['decision'] == 'rejected':
            # In this API version a rejected visible column makes S10 fail with classification.incomplete.
            # Feedback is a rebuild, offered by the UI, never a rejection here.
            raise Refused('rejection_requires_rebuild')
        seen[name] = review
    pending = {item['name'] for item in items
               if item['reviewable'] and item['classification_state'] in PENDING_STATES}
    missing = sorted(pending - set(seen))
    if missing:
        raise Refused('decisions_incomplete')
    confirmed = sorted(n for n, r in seen.items() if r['decision'] == 'confirmed')
    if sorted(doc['confirm_columns']) != confirmed:
        raise Refused('confirm_columns_mismatch')
    if not confirmed:
        raise Refused('no_confirmed_columns')
    return [seen[name] for name in sorted(seen)], confirmed


def decide(out, decisions_path):
    ctx = Ctx(out)
    events = read_journal(out)
    summary = phase_summary(events)
    state, _ = run_state(events)
    failed = sorted(name for name, entry in summary.items() if entry['state'] == 'failed')
    retry_review = state == 'failed' and failed == ['review'] and not phase_complete(out, 'review', summary)
    if not phase_complete(out, 's0_s8', summary) or not (state == 'awaiting_review' or retry_review):
        raise Refused('not_awaiting_review')
    _, items = review_items(out)
    assert_withheld_current(out)
    doc = load_private_json(decisions_path, code='decisions_shape')
    reviews, confirmed = validate_decisions(doc, items)
    canonical = {'reviews': reviews, 'confirm_columns': confirmed}
    binding = Path(out) / 'review' / 'decisions.json'
    if binding.is_file():
        if json.loads(binding.read_text(encoding='utf-8')) != canonical:
            raise Refused('decisions_drift')
        return {'state': state, 'decisions': 'already_recorded'}
    write_json(binding, canonical)
    accepted = accepted_reviews(out)
    to_submit = [r for r in reviews if r['name'] not in accepted]
    product_doc = {'reviews': to_submit, 'confirm_columns': confirmed, 'role': ctx.run['names']['role'],
                   'fixture_authorized': True}
    write_json(Path(out) / 'private' / 'product-decisions.json', product_doc)
    journal_append(out, 'awaiting_review', 'succeeded')
    record_phase(out, 'awaiting_review', None, {'reviews': len(reviews), 'confirmed': len(confirmed),
                                                'already_accepted': len(reviews) - len(to_submit)})
    return {'state': 'decisions_recorded', 'reviews': len(reviews), 'confirmed': len(confirmed),
            'to_submit': len(to_submit), 'withheld': sum(1 for i in items if not i['reviewable'])}


def cancel(out):
    load_run(out)
    if runner_alive(out):
        pid = int((Path(out) / 'runner.pid').read_text().strip())
        os.kill(pid, signal.SIGTERM)
    teardown_forward(out)
    journal_append(out, '_run', 'state', state='canceled')
    return {'state': 'canceled'}


# ---------------------------------------------------------------- status

def build_status(out):
    out = Path(out)
    run = load_run(out)
    events = read_journal(out)
    summary = phase_summary(events)
    state, code = run_state(events)
    if state == 'running' and not runner_alive(out):
        state, code = 'failed', 'runner_exited'
    phases = []
    for name in PHASES:
        entry = summary.get(name, {'state': 'pending', 'started_at': None, 'finished_at': None, 'error_code': None})
        wall = None
        data_path = out / 'phases' / (name + '.json')
        if data_path.is_file():
            wall = json.loads(data_path.read_text(encoding='utf-8')).get('wall_seconds')
        elif entry['started_at'] and entry['finished_at']:
            wall = round((parse_ts(entry['finished_at']) - parse_ts(entry['started_at'])).total_seconds(), 3)
        phases.append({'name': name, 'state': entry['state'], 'started_at': entry['started_at'],
                       'finished_at': entry['finished_at'], 'error_code': entry['error_code'],
                       'wall_seconds': wall})
    capture = {'format': run['format'], 'tables': [], 'rows': 0, 'bytes': 0, 'bytes_per_second': 0,
               'reconnects': 0, 'generation': None}
    load_path = Path('/tmp/oo-mysql-connection-whole/provision') / run['run_id'] / 'native' / 'load.json'
    if load_path.is_file():
        load = json.loads(load_path.read_text(encoding='utf-8'))
        capture['tables'] = [{'table': '%s.%s' % (e['schema'], e['table']), 'chunks_done': 1,
                              'rows': e['row_count'], 'bytes': 0} for e in load['entries']]
        capture['rows'] = sum(e['row_count'] for e in load['entries'])
        capture['generation'] = load.get('source_generation')
    build_id = None
    if (out / 'product' / 'build.json').is_file():
        build_id = json.loads((out / 'product' / 'build.json').read_text(encoding='utf-8')).get('build_id')
    s0_s8 = summary.get('s0_s8', {}).get('state') == 'succeeded'
    review_done = summary.get('review', {}).get('state') == 'succeeded'
    stages = []
    for stage in STAGES:
        if stage in ('S9', 'S10'):
            status = 'succeeded' if review_done else 'pending'
        else:
            status = 'succeeded' if s0_s8 else 'pending'
        stages.append({'stage': stage, 'status': status})
    # Accepted = reviews the API recorded (HTTP 200 in the product evidence), not reviews merely submitted.
    accepted = accepted_reviews(out)
    review = {'pending': 0, 'submitted': len(accepted), 'withheld': 0}
    try:
        _, items = review_items(out)
    except Refused:
        items = []
    review['pending'] = sum(1 for item in items if item['reviewable'] and item['classification_state'] in PENDING_STATES
                            and item['name'] not in accepted)
    review['withheld'] = sum(1 for item in items if not item['reviewable'])
    scope = None
    if (out / 'phases' / 'ready.json').is_file():
        scope = json.loads((out / 'phases' / 'ready.json').read_text(encoding='utf-8')).get('data')
    started = run['created_utc']
    last = events[-1]['ts'] if events else started
    return {'protocol': PROTOCOL_STATUS, 'run_id': run['run_id'], 'label': run['label'], 'state': state,
            'error_code': code, 'source': {'type': 'mysql', 'host': run['source']['host'],
                                           'port': run['source']['port'], 'database': run['source']['database'],
                                           'user': run['source']['user'], 'egress': run['egress']},
            'phases': phases, 'capture': capture,
            'build': {'build_id': build_id, 'stages': stages,
                      'passes': [pass_summary(p) for p in load_pii_state(out)['passes']]},
            'review': review, 'scope': scope,
            'timing': {'created_utc': started, 'last_event_utc': last,
                       'wall_seconds_total': round((parse_ts(last) - parse_ts(started)).total_seconds(), 3)}}


# ---------------------------------------------------------------- commands

def create_run(site, six, password, fmt, egress, replica, label, out, ca_bytes=None, discover=None,
               profile=None, profile_password=None):
    """Create RUN with private layout, the secret copy and registry. Never spawns a runner."""
    out = Path(os.path.abspath(str(out)))
    if out.exists() and (not out.is_dir() or any(out.iterdir())):
        raise Refused('run_exists')
    token = secrets.token_hex(8)
    run_id = 'run_' + token
    short = token[:8]
    out.mkdir(mode=0o700, parents=True, exist_ok=True)
    os.chmod(out, 0o700)
    for sub in ('private', 'evidence', 'phases', 'review', 'product'):
        (out / sub).mkdir(mode=0o700, exist_ok=True)
    provision_root = Path(expand(site['secret_root'])) / 'provision'
    provision_root.mkdir(mode=0o700, parents=True, exist_ok=True)
    os.chmod(provision_root, 0o700)
    pdir = provision_root / run_id
    pdir.mkdir(mode=0o700)
    pw_path = pdir / 'password.pass'
    write_private(pw_path, password + b'\n')
    registry = {six['secret_ref']: str(pw_path)}
    if profile is not None:
        profile_path = pdir / 'profile.pass'
        write_private(profile_path, profile_password + b'\n')
        registry[profile['secret_ref']] = str(profile_path)
    write_json(out / 'private' / 'secret-registry.json', registry)
    ca_path = pdir / 'source-ca.pem'
    write_private(ca_path, ca_bytes or b'')
    write_json(out / 'six.json', six)
    run = {'protocol': PROTOCOL_RUN, 'run_id': run_id, 'label': label, 'format': fmt, 'egress': egress,
           'created_utc': now(), 'source': six, 'replica': replica,
           'names': {'container_prefix': 'oo-ob-%s' % short, 'db_prefix': 'ob_%s_' % short,
                     'logical_project': 'ob_%s' % short,
                     'identity_project': 'ob_%s_identity' % short, 'role': 'oo_r_ob_%s_full' % short,
                     'reader_login': 'ob_%s_reader' % short},
           'secret_registry': 'private/secret-registry.json',
           'source_ca': str(ca_path), 'discover_endpoint': discover, 'profile': profile}
    write_json(out / 'run.json', run)
    journal_append(out, '_run', 'state', state='running')
    return run, out


def cmd_start(args):
    site = load_site()
    six = validate_six(load_private_json(args.input))
    fmt = choose_format(args.format)
    site = site_for_format(site, fmt)
    egress = args.egress or 'direct'
    if egress not in EGRESS_MODES:
        raise Refused('egress_invalid')
    if egress == 'eip' and args.discover_endpoint:
        raise Refused('egress_invalid')
    password = read_password(args.password_file)
    replica = None
    if args.read_replica:
        replica = validate_replica(load_private_json(args.read_replica, code='replica_invalid'))
    label = validate_label(args.label, six)
    profile = profile_password = None
    if args.profile_input or args.profile_password_file:
        if not (args.profile_input and args.profile_password_file):
            raise Refused('profile_invalid')
        profile = validate_profile(load_private_json(args.profile_input, code='profile_invalid'), six)
        profile_password = read_password(args.profile_password_file)
    verify_site_binaries(site)
    out = Path(os.path.abspath(args.out))
    roots = [Path(expand(root)) for root in site['shared_roots']]
    if not any(out == root or root in out.parents for root in roots):
        raise Refused('out_not_shared')
    ca_bytes = owned_public_file(args.ca_file).read_bytes() if args.ca_file else None
    if not ca_bytes:
        raise Refused('ca_file_required')
    discover = None
    if args.discover_endpoint:
        host, _, port = args.discover_endpoint.rpartition(':')
        if not host or not port.isdigit():
            raise Refused('discover_endpoint_invalid')
        discover = {'host': host, 'port': int(port)}
    run, out = create_run(site, six, password, fmt, egress, replica, label, args.out, ca_bytes, discover,
                          profile=profile, profile_password=profile_password)
    del password, profile_password
    spawn_runner(out)
    return {'run_id': run['run_id'], 'label': run['label'], 'state': 'running', 'out': str(out)}


def cmd_status(args):
    return build_status(args.out)


def cmd_review(args):
    return review_document(args.out)


def cmd_decide(args):
    result = decide(args.out, args.decisions)
    if result['state'] == 'decisions_recorded':
        spawn_runner(args.out)
    return result


def cmd_resume(args):
    load_run(args.out)
    if runner_alive(args.out):
        raise Refused('runner_active')
    state, _ = run_state(read_journal(args.out))
    if state == 'ready':
        return build_status(args.out)
    if state == 'canceled':
        raise Refused('run_canceled')
    spawn_runner(args.out)
    return {'state': 'resuming'}


def cmd_cancel(args):
    return cancel(args.out)


class Parser(argparse.ArgumentParser):
    def error(self, message):
        # Never echo argv: it may contain values that must not leak.
        sys.stderr.write(json.dumps({'error_code': 'argv_invalid'}) + '\n')
        sys.exit(2)


def build_parser():
    p = Parser(prog='provision.py')
    sub = p.add_subparsers(dest='command', required=True)
    s = sub.add_parser('start')
    s.add_argument('--input', required=True)
    s.add_argument('--password-file', required=True)
    s.add_argument('--out', required=True)
    s.add_argument('--egress', choices=EGRESS_MODES)
    s.add_argument('--format')
    s.add_argument('--read-replica')
    s.add_argument('--label')
    s.add_argument('--ca-file', help='CA certificate (PEM) that verifies the source server certificate')
    s.add_argument('--profile-input', help='JSON {"user","secret_ref"}: SELECT-only profile reader for the source-SQL '
                   'profile step (host/port/database are the six fields)')
    s.add_argument('--profile-password-file', help='0600 file holding the profile reader password')
    s.add_argument('--discover-endpoint', help='HOST:PORT used for host-side schema discovery (default: none; '
                   'the trusted container connects to the six-field host)')
    s.set_defaults(func=cmd_start)
    for name, func in (('status', cmd_status), ('review', cmd_review), ('resume', cmd_resume), ('cancel', cmd_cancel)):
        c = sub.add_parser(name)
        c.add_argument('--out', required=True)
        c.set_defaults(func=func)
    d = sub.add_parser('decide')
    d.add_argument('--out', required=True)
    d.add_argument('--decisions', required=True)
    d.set_defaults(func=cmd_decide)
    r = sub.add_parser('_runner')
    r.add_argument('--out', required=True)
    r.set_defaults(func=lambda a: {'result': runner(a.out)})
    return p


def main(argv=None):
    argv = list(sys.argv[1:] if argv is None else argv)
    if any(a == '--password' or a.startswith('--password=') for a in argv):
        sys.stderr.write(json.dumps({'error_code': 'inline_password_refused'}) + '\n')
        return 2
    args = build_parser().parse_args(argv)
    try:
        result = args.func(args)
    except Refused as exc:
        sys.stderr.write(json.dumps({'error_code': exc.code}) + '\n')
        return 2
    if args.command == '_runner':
        sys.stderr.write(json.dumps({'runner_result': result['result']}) + '\n')
        return 0 if result['result'] in ('ready', 'awaiting_review') else 2
    sys.stdout.write(json.dumps(result, sort_keys=True, indent=2, ensure_ascii=False) + '\n')
    return 0


if __name__ == '__main__':
    sys.exit(main())
