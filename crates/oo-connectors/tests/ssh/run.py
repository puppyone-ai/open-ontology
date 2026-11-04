#!/usr/bin/env python3
"""Run the unchanged shared T1 oracle through owned SSH-only source networks.

At most two databases: one source and T1's Agent DB. All source administration
uses the shared fixture-admin path. Product reads use russh, not an exec relay.
"""
import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import secrets
import select
import shutil
import socket
import subprocess
import sys
import threading
import time

ROOT = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(ROOT / 'tools/oo-test'))
import t1
import runner

t1.LANE = 'b'

class NoRelay:
    # T1 uses this only to choose a config port. Never open a host relay.
    def __init__(self, container, port):
        self.port = port
    def close(self):
        pass

t1.Relay = NoRelay

def tty_confirmation(run, stack_path):
    master, slave = os.openpty()
    child = subprocess.Popen(
        [str(ROOT / 'target/debug/ssh-transport-test'), '--ignored', '--nocapture',
         'first_use_tty_confirmation'],
        stdin=slave, stdout=slave, stderr=slave,
        env=dict(os.environ, OO_SSH_STACK=str(stack_path)))
    os.close(slave)
    output, replies = b'', 0
    try:
        deadline = time.monotonic() + 30
        while child.poll() is None and time.monotonic() < deadline:
            if select.select([master], [], [], .2)[0]:
                try:
                    data = os.read(master, 65536)
                except OSError:
                    break
                if not data:
                    break
                output += data
                if output.count(b'Type yes: ') > replies:
                    os.write(master, b'no\n' if replies == 0 else b'yes\n')
                    replies += 1
        code = child.wait(timeout=5)
        while select.select([master], [], [], 0)[0]:
            try:
                data = os.read(master, 65536)
            except OSError:
                break
            if not data:
                break
            output += data
    finally:
        if child.poll() is None:
            child.kill()
            child.wait()
        os.close(master)
        (run.path / 'ssh-tty.stdout').write_bytes(output)
    assert code == 0 and replies == 2 and b'1 passed' in output, 'TTY confirmation failed'
    runner.write_json(run.path / 'ssh-tty.json', {'exit': code, 'explicit_replies': replies})

def doctor_probe(run, stack, store):
    home = store / 'doctor-home'
    secret_dir = home / '.config/open-ontology/secrets'
    secret_dir.mkdir(parents=True, mode=0o700)
    for name in ('source-password', 'source-ca', 'client-key', 'jump-password'):
        if (store / name).exists():
            t1.private(secret_dir / name, (store / name).read_text())
    config = copy.deepcopy(stack)
    config['sources'][0]['scope']['include'] = [
        'public.regions' if run.engine == 'postgres' else 'fixture.regions']
    path = store / 'doctor-stack.json'
    runner.write_json(path, config)
    code, raw = runner.Run.command(run, [
        'env', 'HOME=' + str(home), str(ROOT / 'target/debug/oo'),
        'doctor', '--config', str(path), '--json',
    ], required=False)
    report = json.loads(raw)
    assert code == 1 and report['model']['status'] == 'blocked', 'no model secret is supplied'
    source = report['sources'][0]
    assert source['status'] == 'ok' and source['tunnel']['status'] == 'ok', source
    runner.write_json(run.path / 'ssh-doctor.json', report)

class SshRun(t1.LaneRun):
    def command(self, command, **kwargs):
        command = [str(x) for x in command]
        command = ['oo.lane=b' if x == 'oo.lane=h' else x for x in command]
        if command[:2] == ['docker', 'create'] and self.name in command:
            alias = 'db.origin' if self.engine == 'mysql' else 'db.internal'
            command[2:2] = ['--network', self.network, '--network-alias', alias]
        if command[:3] == ['openssl', 'x509', '-req']:
            ext = Path(command[command.index('-extfile') + 1])
            ext.write_text(ext.read_text().replace('DNS:localhost', 'DNS:db.internal'))
        if command[0] == str(self.binary):
            stack_path = Path(command[command.index('--stack') + 1])
            store = Path(command[command.index('--secrets-dir') + 1])
            stack = json.loads(stack_path.read_text())
            source = stack['sources'][0]
            source.update(host='db.internal', port=5432 if self.engine == 'postgres' else 3306)
            method = 'key' if self.engine == 'postgres' else 'password'
            name = 'client-key' if method == 'key' else 'jump-password'
            t1.private(store / name, (self.jump_private / name).read_text())
            source['ssh'] = dict(host='127.0.0.1', port=self.jump_port, user='root',
                                 host_key_sha256=self.pin, **{method: {'secret_ref': name}})
            runner.write_json(stack_path, stack)
            if not self.security_checked:
                self.security_checked = True
                t1.private(store / 'wrong-password', secrets.token_hex(24))
                runner.Run.command(self, [
                    'env', 'OO_SSH_STACK=' + str(stack_path),
                    'OO_SSH_SECRETS=' + str(store), 'OO_SSH_JUMP_CONTAINER=' + self.jump,
                    'OO_SSH_RUN_ID=' + self.id, str(ROOT / 'target/debug/ssh-transport-test'),
                    '--ignored', '--nocapture', 'pin_auth_reconnect_and_shutdown',
                ])
                tty_confirmation(self, stack_path)
                doctor_probe(self, stack, store)
                source['ssh']['host_key_sha256'] = 'SHA256:' + 'A' * 43
                runner.write_json(stack_path, stack)
                code, _ = runner.Run.command(self, command, **dict(kwargs, required=False))
                error = (self.path / f'{self.seq:04d}.stderr').read_text()
                if not code or 'SSH_HOST_KEY_MISMATCH' not in error:
                    raise RuntimeError('fingerprint mismatch not refused')
                runner.write_json(self.path / 'ssh-mismatch.json', {'refused': True, 'command': self.seq})
                source['ssh']['host_key_sha256'] = self.pin
                runner.write_json(stack_path, stack)
            target_path = Path(command[command.index('--target') + 1])
            if not self.fault_checked and json.loads(target_path.read_text())['run_id'] == 'corrected':
                self.fault_checked = True
                return self.interrupt_and_resume(command, kwargs)
        # Do not call LaneRun.command: it publishes a source port.
        return runner.Run.command(self, command, **kwargs)

    def interrupt_and_resume(self, command, kwargs):
        stop = threading.Event()
        caught = {}
        watcher = copy.copy(self)
        watcher.seq = 4_000_000
        if self.engine == 'mysql':
            watcher.command(['docker', 'exec', self.jump, 'touch', '/tmp/fault-arm'])
        def interrupt():
            query = ("SELECT query FROM pg_stat_activity WHERE usename='fixture_reader' "
                     "AND query LIKE 'FETCH FORWARD%oo_chunk' LIMIT 1;" if self.engine == 'postgres' else
                     "SELECT INFO FROM information_schema.PROCESSLIST WHERE USER='fixture_reader' "
                     "AND COMMAND='Query' AND INFO LIKE '%FROM%events%' AND INFO LIKE '%event_no%';")
            while not stop.is_set():
                if self.engine == 'mysql':
                    code, output = watcher.command(
                        ['docker', 'exec', self.jump, 'cat', '/tmp/fault-triggered'],
                        required=False)
                    if code == 0:
                        caught.update(json.loads(output))
                        _, sql = watcher.sql(
                            "SELECT COALESCE(s.SQL_TEXT,'') FROM performance_schema.events_statements_current s "
                            "JOIN performance_schema.threads t ON t.THREAD_ID=s.THREAD_ID "
                            "WHERE t.PROCESSLIST_USER='fixture_reader';", admin=True, required=False)
                        caught['query'] = sql.strip()
                        caught['observed_at'] = time.time()
                        watcher.command(['docker', 'kill', '--signal', 'KILL', self.jump])
                        watcher.command(['docker', 'start', self.jump])
                        caught['restarted'] = True
                        return
                    stop.wait(.02)
                    continue
                code, output = watcher.sql(query, admin=True, required=False)
                if code == 0 and output.strip():
                    caught['query'] = output.strip()
                    caught['observed_at'] = time.time()
                    watcher.command(['docker', 'restart', self.jump])
                    caught['restarted'] = True
                    return
                stop.wait(.02)
        thread = threading.Thread(target=interrupt)
        thread.start()
        try:
            code, out = runner.Run.command(self, command, **dict(kwargs, required=False))
        finally:
            stop.set()
            thread.join(timeout=60)
        caught.update(failed_exit=code, failed_command=self.seq)
        caught['completed_tables_before_fault'] = [
            json.loads(line)['table']['name'] for line in out.splitlines() if line.startswith('{')]
        runner.write_json(self.path / 'ssh-interruption.json', caught)
        if not caught.get('restarted') or code == 0:
            raise RuntimeError('mid-read disconnect was not observed and rejected')
        self.wait_jump()
        code, out = runner.Run.command(self, command, **kwargs)
        caught['resume_exit'] = code
        runner.write_json(self.path / 'ssh-interruption.json', caught)
        return code, out

    def wait_jump(self):
        for _ in range(60):
            try:
                with socket.create_connection(('127.0.0.1', self.jump_port), timeout=1) as s:
                    if s.recv(100).startswith(b'SSH-'):
                        return
            except OSError:
                pass
            time.sleep(.2)
        raise RuntimeError('sshd startup')

    def start_jump(self):
        self.jump_private = self.path / '.ssh-private'
        self.jump_private.mkdir(mode=0o700)
        for name in ['host-key', 'client-key']:
            self.command(['ssh-keygen', '-q', '-t', 'ed25519', '-N', '', '-f', str(self.jump_private / name)])
        t1.private(self.jump_private / 'jump-password', secrets.token_hex(24))
        t1.private(self.jump_private / 'sshd_config',
                   'Port 22\nHostKey /etc/ssh/ssh_host_ed25519_key\nPermitRootLogin yes\n'
                   'PasswordAuthentication yes\nPubkeyAuthentication yes\nAllowTcpForwarding local\n'
                   'PermitOpen db.internal:5432 db.internal:3306\nLogLevel ERROR\n')
        _, output = self.command(['ssh-keygen', '-lf', str(self.jump_private / 'host-key.pub'), '-E', 'sha256'])
        self.pin = output.split()[1]
        self.jump = 'oo-b-jump-' + self.id
        with socket.socket() as reservation:
            reservation.bind(('127.0.0.1', 0))
            self.jump_port = reservation.getsockname()[1]
        self.command(['docker', 'create', '--name', self.jump, '--label', 'oo.lane=b',
                      '--label', 'oo.run_id=' + self.id, '-p', f'127.0.0.1:{self.jump_port}:22',
                      '-e', 'OO_MYSQL_PROXY=' + ('1' if self.engine == 'mysql' else '0'),
                      '-v', str(self.jump_private) + ':/run/private:ro', self.image])
        self.jump_created = True
        aliases = ['--alias', 'db.internal'] if self.engine == 'mysql' else []
        self.command(['docker', 'network', 'connect', *aliases, self.network, self.jump])
        self.command(['docker', 'start', self.jump])
        _, output = self.command(['docker', 'port', self.jump, '22/tcp'])
        self.jump_port = int(output.strip().rsplit(':', 1)[1])
        self.wait_jump()
        self.command(['docker', 'exec', self.jump, '/usr/sbin/sshd', '-V'])
        _, inspection = self.command(['docker', 'inspect', self.name])
        info = json.loads(inspection)[0]
        assert not any(info['NetworkSettings']['Ports'].values()), 'source port published'
        assert set(info['NetworkSettings']['Networks']) == {self.network}
        source_ip = info['NetworkSettings']['Networks'][self.network]['IPAddress']
        try:
            with socket.create_connection((source_ip, 5432 if self.engine == 'postgres' else 3306), timeout=1):
                raise RuntimeError('source is reachable directly from host')
        except OSError as error:
            host_probe = type(error).__name__
        _, info = self.command(['docker', 'network', 'inspect', self.network])
        assert json.loads(info)[0]['Internal'] is True
        runner.write_json(self.path / 'ssh-topology.json', {'internal': True, 'source_published_ports': [],
            'jump_port': self.jump_port, 'host_key_sha256': self.pin, 'source_host': 'db.internal',
            'host_direct_connect': host_probe, 'mysql_fault_relay': self.engine == 'mysql'})

    def cleanup_jump(self):
        for kind, name, created in [('container', getattr(self, 'jump', ''), self.jump_created),
                                    ('network', self.network, self.network_created),
                                    ('image', self.image, self.image_created)]:
            if not created:
                continue
            fmt = '{{json .Labels}}' if kind == 'network' else '{{json .Config.Labels}}'
            _, raw = self.command(['docker', kind, 'inspect', '--format', fmt, name])
            labels = json.loads(raw)
            assert labels['oo.lane'] == 'b' and labels['oo.run_id'] == self.id
            if kind == 'container':
                self.command(['docker', 'logs', name], required=False)
            self.command(['docker', kind, 'rm'] + (['-f','-v'] if kind == 'container' else []) + [name])
        if hasattr(self, 'jump_private'):
            shutil.rmtree(self.jump_private)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--run-id', required=True)
    ap.add_argument('--engine', choices=['postgres', 'mysql'], required=True)
    ap.add_argument('--scale', type=int, default=2_000_000)
    ap.add_argument('--tables', type=int, default=200)
    ap.add_argument('--subnet', default='10.238.141.0/24')
    args = ap.parse_args()
    if not args.run_id.replace('-', '').isalnum():
        ap.error('invalid run ID')
    active = subprocess.check_output(
        ['docker', 'ps', '-q', '--filter', 'label=oo.lane=b'], text=True).strip()
    if active:
        raise RuntimeError('another lane B fixture is active; refusing parallel database fixtures')
    path = ROOT / 'evidence' / args.run_id
    path.mkdir(mode=0o700)
    options = argparse.Namespace(scale=args.scale, tables=args.tables, platform=None, health_timeout=None)
    run = SshRun(path, args.engine, '17' if args.engine == 'postgres' else '8.4', options)
    run.binary = ROOT / 'target/debug/oo-load'
    run.network = 'oo-b-internal-' + run.id
    run.network_created = run.jump_created = False
    run.image_created = False
    run.security_checked = run.fault_checked = False
    run.image = 'oo-b-ssh:' + run.id
    runner.write_json(path / 'ssh-suite.json', {
        'runner_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        'dockerfile_sha256': hashlib.sha256(Path(__file__).with_name('Dockerfile').read_bytes()).hexdigest(),
        'fault_proxy_sha256': hashlib.sha256(Path(__file__).with_name('mysql_fault_proxy.py').read_bytes()).hexdigest(),
        'transport_test_sha256': hashlib.sha256((ROOT / 'target/debug/ssh-transport-test').read_bytes()).hexdigest(),
        'scale': args.scale, 'small_tables': args.tables,
        'full_scale': args.scale == 2_000_000 and args.tables == 200,
        'fault_phase': 'quiescent_corrected_run_then_same_run_resume',
    })
    try:
        run.command(['docker', 'build', '--label', 'oo.lane=b', '--label', 'oo.run_id=' + run.id,
                     '-t', run.image, str(Path(__file__).parent)], timeout=600)
        run.image_created = True
        run.command(['docker', 'network', 'create', '--internal', '--subnet', args.subnet, '--label', 'oo.lane=b',
                     '--label', 'oo.run_id=' + run.id, run.network])
        run.network_created = True
        run.setup()
        run.seed()
        run.start_jump()
        report = t1.run_t1(run, run.binary)
    finally:
        failures = run.cleanup()
        run.cleanup_jump()
        runner.checksums(path)
    print(json.dumps(report))
    return 0 if report['T1'] == 'passed' and report['T8'] == 'passed' and not failures else 1

if __name__ == '__main__':
    sys.exit(main())
