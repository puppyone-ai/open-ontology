#!/usr/bin/env python3
"""Live S4-A-04 runner: `oo up|status|down --target ssh` against a clean Linux
target (sshd + systemd + Docker, privileged, labelled oo.lane=a2).

This proves the three lite foundation services, the pinned host key, the
systemd timer, idempotent re-run and complete removal. It is not seven-service
acceptance: worker/scheduler/Gateway wiring is lane M's and Gateway health is
expected to be `unavailable`. All keys and passwords are generated at runtime
under the git-ignored evidence directory and removed at the end.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import subprocess
import time

ROOT = Path(__file__).resolve().parents[2]
LABEL = 'oo.lane=a2'
TARGET_DOCKERFILE = r'''
FROM docker.io/library/ubuntu:22.04@sha256:076e7bf82319619f28dd0aa9c02ab34b055ad37100395b4f49a2ffba04fb5e11
LABEL oo.lane=a2
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    systemd systemd-sysv dbus openssh-server docker.io docker-compose-v2 python3 iptables \
    ca-certificates && rm -rf /var/lib/apt/lists/* && rm -f /etc/ssh/ssh_host_* && \
    systemctl enable ssh docker containerd && \
    systemctl mask getty@tty1.service systemd-logind.service console-getty.service \
      systemd-binfmt.service proc-sys-fs-binfmt_misc.automount proc-sys-fs-binfmt_misc.mount \
      systemd-sysctl.service systemd-modules-load.service systemd-udevd.service \
      systemd-udevd-control.socket systemd-udevd-kernel.socket systemd-udev-trigger.service \
      systemd-remount-fs.service sys-kernel-config.mount sys-kernel-debug.mount \
      sys-kernel-tracing.mount sys-fs-fuse-connections.mount && \
    rm -rf /usr/lib/binfmt.d /usr/lib/sysctl.d /etc/sysctl.d /etc/sysctl.conf && \
    mkdir -p /root/.ssh && chmod 700 /root/.ssh
STOPSIGNAL SIGRTMIN+3
CMD ["/sbin/init"]
'''


# The privileged target shares the Docker VM kernel. Units that rewrite global
# kernel state (binfmt: Rosetta amd64 emulation; sysctl; modules) are masked,
# and the runner checks that state before and after every target.
VM_SYSCTLS = ['kernel/printk', 'kernel/kptr_restrict', 'kernel/sysrq', 'kernel/pid_max',
              'fs/protected_hardlinks', 'fs/protected_symlinks', 'fs/protected_regular',
              'fs/protected_fifos', 'vm/mmap_min_addr', 'vm/max_map_count', 'kernel/dmesg_restrict']


def vm_state(probe_image):
    sysctl = sh(['docker', 'run', '--rm', '--label', LABEL, '--platform', 'linux/arm64', '--entrypoint', 'sh',
                 probe_image, '-c', 'for k in ' + ' '.join(VM_SYSCTLS) + '; do echo "$k=$(cat /proc/sys/$k)"; done'])
    emulation = sh(['docker', 'run', '--rm', '--label', LABEL, '--platform', 'linux/amd64', '--entrypoint', 'uname',
                    VM_AMD64_PROBE[0], '-m'], check=False)
    return {'sysctl': sysctl.stdout.decode(), 'amd64_emulation': emulation.stdout.decode().strip()}


VM_AMD64_PROBE = ['']


def sh(argv, check=True, **kw):
    p = subprocess.run(argv, capture_output=True, **kw)
    if check and p.returncode:
        raise RuntimeError(f'{argv[:3]} exited {p.returncode}: {p.stderr[-400:]!r}')
    return p


def free_gib():
    return shutil.disk_usage('/').free / 1024 ** 3


def fingerprint(pub):
    out = sh(['ssh-keygen', '-l', '-E', 'sha256', '-f', str(pub)]).stdout.decode().split()
    return out[1]


class Target:
    def __init__(self, arch, run_id, out):
        self.arch, self.out = arch, out
        self.name = f'oo-a2-ssh-{run_id}'
        recipe = hashlib.sha256(TARGET_DOCKERFILE.encode()).hexdigest()[:12]
        self.image = f'oo-a2-ssh-target:{arch}-{recipe}'
        self.volume = f'oo-a2-ssh-docker-{run_id}'
        self.containerd = f'oo-a2-ssh-containerd-{run_id}'

    def exec(self, *argv, stdin=None, check=True):
        return sh(['docker', 'exec', *(['-i'] if stdin is not None else []), self.name, *argv],
                  check=check, input=stdin)

    def remote(self, script, check=True):
        return self.exec('sh', '-c', script, check=check).stdout.decode()

    def start(self, private):
        if sh(['docker', 'image', 'inspect', self.image], check=False).returncode:
            with (self.out / f'target-image-{self.arch}.log').open('wb') as log:
                subprocess.run(['docker', 'build', '--platform', f'linux/{self.arch}', '--label', LABEL,
                                '-t', self.image, '-'], input=TARGET_DOCKERFILE.encode(),
                               stdout=log, stderr=subprocess.STDOUT, check=True)
        for volume in (self.volume, self.containerd):
            sh(['docker', 'volume', 'create', '--label', LABEL, volume])
        sh(['docker', 'create', '--name', self.name, '--label', LABEL, '--platform', f'linux/{self.arch}',
            '--privileged', '--cgroupns=private', '--tmpfs', '/run', '--tmpfs', '/run/lock',
            '-e', 'container=docker', '-v', f'{self.volume}:/var/lib/docker',
            '-v', f'{self.containerd}:/var/lib/containerd',
            '-p', '127.0.0.1::22', self.image])
        for name in ('ssh_host_ed25519_key', 'ssh_host_ed25519_key.pub'):
            sh(['docker', 'cp', str(private / name), f'{self.name}:/etc/ssh/{name}'])
        sh(['docker', 'cp', str(private / 'client.pub'), f'{self.name}:/root/.ssh/authorized_keys'])
        sh(['docker', 'cp', str(private / 'sshd.conf'), f'{self.name}:/etc/ssh/sshd_config.d/oo.conf'])
        sh(['docker', 'start', self.name])
        port = sh(['docker', 'port', self.name, '22/tcp']).stdout.decode().split(':')[-1].strip()
        deadline = time.time() + 300
        while time.time() < deadline:
            ready = self.exec('sh', '-c', 'systemctl is-active ssh docker && docker info >/dev/null',
                              check=False)
            if ready.returncode == 0:
                break
            time.sleep(3)
        else:
            raise RuntimeError('target did not become ready')
        self.exec('sh', '-c', 'chown root:root /etc/ssh/ssh_host_* /root/.ssh/authorized_keys && '
                  'chmod 600 /etc/ssh/ssh_host_ed25519_key /root/.ssh/authorized_keys && '
                  'systemctl restart ssh')
        return int(port)

    def snapshot(self):
        return self.remote("docker inspect --format '{{.Id}} {{.Name}} {{.Created}} {{.State.StartedAt}}' "
                           "$(docker ps -aq) | sort")

    def remove(self):
        sh(['docker', 'rm', '-f', self.name], check=False)
        sh(['docker', 'volume', 'rm', self.volume, self.containerd], check=False)


def run_platform(arch, run_id, binary, images, offline, password_case, keep=False):
    out = ROOT / 'evidence' / run_id
    out.mkdir(mode=0o700)
    private = out / '.private'
    private.mkdir(mode=0o700)
    home = out / 'home'
    store = home / '.config/open-ontology/secrets'
    store.mkdir(parents=True, mode=0o700)
    for d in (home / '.config', home / '.config/open-ontology'):
        d.chmod(0o700)
    sh(['ssh-keygen', '-q', '-t', 'ed25519', '-N', '', '-C', 'oo-a2-host', '-f',
        str(private / 'ssh_host_ed25519_key')])
    sh(['ssh-keygen', '-q', '-t', 'ed25519', '-N', '', '-C', 'oo-a2-client', '-f', str(private / 'client')])
    sh(['ssh-keygen', '-q', '-t', 'ed25519', '-N', '', '-C', 'oo-a2-wrong', '-f', str(private / 'wrong')])
    (private / 'sshd.conf').write_text(
        'PermitRootLogin yes\nPasswordAuthentication ' + ('yes' if password_case else 'no') +
        '\nKbdInteractiveAuthentication no\nUsePAM yes\n')
    values = {'model': secrets.token_hex(24), 'source': secrets.token_hex(24),
              'endpoint': 'http://127.0.0.1:9/v1', 'root-password': secrets.token_hex(20)}
    for name, value in [('model', values['model']), ('source', values['source']),
                        ('endpoint', values['endpoint']), ('root-password', values['root-password'])]:
        (store / name).touch(mode=0o600)
        (store / name).write_text(value)
    shutil.copyfile(private / 'client', store / 'deploy-key')
    (store / 'deploy-key').chmod(0o600)
    client_key = (private / 'client').read_bytes()
    pin = fingerprint(private / 'ssh_host_ed25519_key.pub')
    wrong = fingerprint(private / 'wrong.pub')
    target = Target(arch, run_id, out)
    results = {'platform': f'linux/{arch}', 'run_id': run_id, 'pinned_host_key': pin,
               'free_gib_before': round(free_gib(), 1)}
    if free_gib() < 20:
        raise RuntimeError('less than 20 GiB free; refusing to start target')
    env = dict(os.environ, HOME=str(home),
               DOCKER_CONFIG=os.environ.get('DOCKER_CONFIG', str(Path.home() / '.docker')),
               OO_LITE_RUNTIME_IMAGE_ARM64=images['arm64'], OO_LITE_RUNTIME_IMAGE_AMD64=images['amd64'])
    env.pop('OO_LITE_RUNTIME_IMAGE', None)
    outputs = []

    def config(name, **ssh):
        body = {'version': 'oo.stack/v1',
                'sources': [{'id': 'fixture', 'engine': 'postgres', 'host': 'source.invalid', 'port': 5432,
                             'database': 'fixture', 'user': 'reader', 'password': {'secret_ref': 'source'},
                             'scope': {'include': ['public.events']}, 'sync': {'interval': '1h'}}],
                'model': {'provider': 'openai-compatible', 'base_url': {'secret_ref': 'endpoint'},
                          'model': 'fixture-model', 'key': {'secret_ref': 'model'}},
                'deploy': {'target': 'local', 'profile': 'lite',
                           'ssh': {'host': '127.0.0.1', 'port': port, 'user': 'root', **ssh}}}
        path = out / name
        path.write_text(json.dumps(body, indent=2))
        path.chmod(0o600)
        return path

    def oo(case, action, path, expected, extra=()):
        started = time.time()
        proc = subprocess.run([str(binary), action, '--target', 'ssh', '--config', str(path), '--json', *extra],
                              env=env, capture_output=True, timeout=2400)
        raw = proc.stdout + proc.stderr
        outputs.append(raw)
        (out / f'{case}.json').write_bytes(raw)
        (out / f'{case}.exit').write_text(str(proc.returncode))
        results.setdefault('timings_s', {})[case] = round(time.time() - started, 1)
        assert proc.returncode == expected, (case, proc.returncode, raw[-600:])
        try:
            return json.loads(proc.stdout)
        except ValueError:
            return {'stderr': proc.stderr.decode(errors='replace')}

    VM_AMD64_PROBE[0] = images['amd64']
    vm_before = vm_state(images['arm64'])
    assert vm_before['amd64_emulation'] == 'x86_64', vm_before
    try:
        port = target.start(private)
        assert vm_state(images['arm64']) == vm_before, 'target changed Docker VM kernel state'
        if password_case:
            target.exec('chpasswd', stdin=f'root:{values["root-password"]}\n'.encode())
        if offline:
            # Registry unreachable from the target: images must come over SSH.
            target.remote('echo "127.0.0.1 registry-1.docker.io index.docker.io auth.docker.io '
                          'production.cloudflare.docker.com" >> /etc/hosts')
        good = config('stack.json', key={'secret_ref': 'deploy-key'}, host_key_sha256=pin)
        bad = config('stack-wrong-pin.json', key={'secret_ref': 'deploy-key'}, host_key_sha256=wrong)
        refused = oo('mismatch', 'up', bad, 1)
        text = json.dumps(refused)
        assert 'SSH_HOST_KEY_MISMATCH' in text and pin in text, text
        assert target.exec('test', '-e', '/root/.open-ontology', check=False).returncode == 1
        results['host_key_mismatch'] = 'refused before any remote write; offered key reported'

        up = oo('up', 'up', good, 1)
        assert up['status'] == 'foundations_only' and up['healthy'] is False, up
        assert up['target'] == 'ssh' and up['remote']['arch'] == arch
        assert {r['Service'] for r in up['services']} == {'agent-db', 'control-db', 'model-egress'}
        assert all(r['Healthy'] for r in up['services']), up['services']
        timer = up['remote']['timer']
        assert timer['installed'] and timer['enabled'] and timer['active'], timer
        transfers = {t['role']: t['transfer'] for t in up['remote']['images']}
        assert transfers['runtime'] == 'docker_save_load', transfers
        assert transfers['postgres'] == ('docker_save_load' if offline else 'pulled_by_digest'), transfers
        modes = target.remote(
            'stat -c "%a %U %n" /root/.open-ontology /root/.open-ontology/lite /root/.open-ontology/lite/pki '
            '/root/.open-ontology/lite/pki/* /root/.open-ontology/source-home/.config/open-ontology/secrets/* '
            '/root/.open-ontology/lite/home/.config/open-ontology/secrets/*')
        (out / 'remote-modes.txt').write_text(modes)
        for line in modes.splitlines():
            mode, owner, name = line.split(' ', 2)
            assert owner == 'root' and mode in ('700', '600'), line
        token = target.exec('cat', '/root/.open-ontology/lite/pki/operator-token').stdout
        assert token.startswith(b'ooct_')
        units = target.remote('systemctl cat open-ontology-lite-cycle.timer open-ontology-lite-cycle.service')
        (out / 'remote-units.txt').write_text(units)
        assert 'OnUnitActiveSec=3600s' in units
        before = target.snapshot()
        (out / 'containers-before.txt').write_text(before)

        again = oo('up-again', 'up', good, 1)
        assert again['remote']['noop'] is True and again['remote']['changed'] == [], again['remote']
        assert all(t['transfer'] == 'present' for t in again['remote']['images'])
        after = target.snapshot()
        (out / 'containers-after.txt').write_text(after)
        assert before == after
        assert token == target.exec('cat', '/root/.open-ontology/lite/pki/operator-token').stdout
        results['rerun'] = 'noop: no transfer, no file/unit/container change, credentials unchanged'

        target.exec('systemctl', 'start', 'open-ontology-lite-cycle.service')
        status = oo('status', 'status', good, 1)
        assert status['status'] == 'foundations_only'
        assert all(r['Healthy'] for r in status['services'])
        assert status['gateway_health']['status'] == 'unavailable'
        assert status['remote']['timer']['last_cycle']['status'] == 'not_configured'
        assert status['remote']['timer']['next_elapse']
        assert status['bootstrap_credentials']['nearest_credential_expiry']
        results['status'] = {'services': {r['Service']: r['Status'] for r in status['services']},
                             'gateway_health': status['gateway_health']['status'],
                             'schedule_status': status.get('schedule_status'),
                             'timer': {k: status['remote']['timer'][k] for k in
                                       ('installed', 'enabled', 'active', 'next_elapse')},
                             'last_cycle': status['remote']['timer']['last_cycle']['status']}
        if password_case:
            pw = config('stack-password.json', password={'secret_ref': 'root-password'}, host_key_sha256=pin)
            via = oo('status-password', 'status', pw, 1)
            assert via['status'] == 'foundations_only' and all(r['Healthy'] for r in via['services'])
            results['password_auth'] = 'status over password authentication via SSH_ASKPASS file reference'

        journal = target.exec('sh', '-c', 'journalctl --no-pager -o cat > /tmp/j; '
                              'cat /root/.bash_history 2>/dev/null >> /tmp/j; '
                              'grep -c -F -f /dev/stdin /tmp/j; rm -f /tmp/j',
                              stdin='\n'.join([values['model'], values['source'], values['root-password'],
                                               token.decode().strip()]).encode() + b'\n', check=False)
        assert journal.stdout.decode().strip() == '0', journal.stdout
        assert target.exec('test', '-e', '/root/.bash_history', check=False).returncode == 1
        inspect = target.remote('docker inspect $(docker ps -aq)')
        for value in (values['model'], values['source'], token.decode().strip()):
            assert value not in inspect
        results['secret_scan_remote'] = 'journal, shell history and container config contain no secret'

        down = oo('down', 'down', good, 0)
        assert down['status'] == 'removed' and down['remote']['remaining'] == {}, down
        leftover = target.remote('docker ps -aq | wc -l; docker volume ls -q | wc -l; '
                                 'docker network ls -q --filter label=oo.lane=a | wc -l; '
                                 'docker images -q | wc -l; '
                                 'ls /etc/systemd/system | grep -c open-ontology; '
                                 'test -e /root/.open-ontology && echo base || echo nobase', check=False)
        (out / 'remote-after-down.txt').write_text(leftover)
        assert leftover.split() == ['0', '0', '0', '0', '0', 'nobase'], leftover
        assert not (out / '.oo/ssh').exists()
        again_down = oo('down-again', 'down', good, 0)
        results['down'] = {'removed': down['remote']['removed'], 'remaining': {},
                           'second_down': again_down['remote'].get('removed')}
        for raw in outputs:
            for value in [values['model'], values['source'], values['root-password'],
                          token.decode().strip()]:
                assert value.encode() not in raw
            assert client_key not in raw
        results['secret_scan_local'] = 'no secret in any oo stdout/stderr'
        vm_after = vm_state(images['arm64'])
        assert vm_after == vm_before, ('Docker VM kernel state changed', vm_after)
        results['docker_vm_kernel_state'] = 'unchanged (binfmt amd64 emulation and global sysctls)'
        results['result'] = 'passed'
        return results
    except BaseException as error:
        results['result'] = 'failed'
        results['error'] = repr(error)[:1500]
        logs = sh(['docker', 'exec', target.name, 'sh', '-c',
                   'journalctl --no-pager -n 300 -o cat; docker ps -a'], check=False)
        (out / 'target-diagnostics.txt').write_bytes(logs.stdout + logs.stderr)
        raise
    finally:
        if not (keep and results.get('result') == 'failed'):
            target.remove()
        (out / 'result.json').write_text(json.dumps(results, indent=2) + '\n')
        shutil.rmtree(private, ignore_errors=True)
        shutil.rmtree(home, ignore_errors=True)
        shutil.rmtree(out / '.oo', ignore_errors=True)


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--run-id', required=True)
    p.add_argument('--arch', choices=['arm64', 'amd64'], required=True)
    p.add_argument('--binary', default=str(ROOT / 'target/debug/oo'))
    p.add_argument('--image-arm64', required=True)
    p.add_argument('--image-amd64', required=True)
    p.add_argument('--offline', action='store_true')
    p.add_argument('--password-case', action='store_true')
    p.add_argument('--keep-on-failure', action='store_true', help='debug only; remove manually')
    a = p.parse_args()
    if not re.fullmatch('[a-z0-9-]+', a.run_id):
        p.error('invalid run id')
    for image in (a.image_arm64, a.image_amd64):
        if not re.fullmatch('sha256:[0-9a-f]{64}', image):
            p.error('runtime images must be content digests')
    results = run_platform(a.arch, a.run_id, Path(a.binary),
                           {'arm64': a.image_arm64, 'amd64': a.image_amd64}, a.offline, a.password_case, a.keep_on_failure)
    print(json.dumps(results, indent=2))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
