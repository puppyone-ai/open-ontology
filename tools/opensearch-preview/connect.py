#!/usr/bin/env python3
"""Capture a configured OpenSearch connection with the real immutable C5 reader.

No fixture setup or source write. Downstream acceptance requires the separate
full-chain runner to consume each new capture; capture alone claims no S5.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import time


def write(path, value):
    path.write_text(json.dumps(value, indent=2) + '\n')


def run(config, output, cycles):
    cfg = json.loads(Path(config).read_text())
    if cfg['type'] != 'opensearch' or not 1 <= cycles <= 100:
        raise ValueError('type or bounded cycle count')
    index = cfg.get('indexes', cfg.get('database'))
    if isinstance(index, list):
        if len(index) != 1:
            raise ValueError('one exact index required')
        index = index[0]
    if not isinstance(index, str) or not re.fullmatch('[a-z0-9][a-z0-9_-]{0,254}', index) or index == '_all':
        raise ValueError('one exact index required')
    host = cfg['host']
    if not re.fullmatch('[a-zA-Z0-9.-]+', host) or not 1 <= cfg['port'] <= 65535:
        raise ValueError('host/port')
    if not cfg['secret_ref'].startswith('secret://'):
        raise ValueError('secret reference required')
    # Local resolver contains file references only; credentials never enter config.
    private = Path(cfg['password_file']).resolve().parent
    if Path(cfg['password_file']).name != 'identities.env':
        raise ValueError('OpenSearch SecretRef must resolve identities.env')
    if (private / 'identities.env').stat().st_mode & 0o077:
        raise ValueError('private credentials must have mode 0600')
    identity = dict(line.split('=', 1) for line in (private / 'identities.env').read_text().splitlines() if '=' in line)
    if identity['reader_user'] != cfg['user']:
        raise ValueError('user differs from secret identity')
    endpoint = f'https://{host}:{cfg["port"]}'
    if (private / 'endpoint.url').read_text().strip().rstrip('/') != endpoint:
        raise ValueError('host/port differs from secret endpoint')
    reader = Path(cfg['reader_bin']).resolve()
    digest = hashlib.sha256(reader.read_bytes()).hexdigest()
    if digest != cfg['reader_sha256'] or reader.read_bytes()[:4] != b'\x7fELF':
        raise ValueError('immutable Linux reader identity')
    image_id = subprocess.run(['docker', 'image', 'inspect', cfg['image'], '--format', '{{.Id}}'], capture_output=True, text=True, check=True).stdout.strip()
    if image_id != cfg['image_sha256']:
        raise ValueError('immutable container image identity')
    schedule = json.loads(Path(cfg['schedule_path']).read_text())
    if schedule['approved_index'] != index or schedule['secret_ref'] != cfg['secret_ref']:
        raise ValueError('approved index/secret differs from connection')
    period = cfg['period_ms']
    if not isinstance(period, int) or not 1000 <= period <= 86400000:
        raise ValueError('period_ms')
    schedule['schedules'] = [{'name': 'connection', 'every_ms': period, 'reconcile_every': cfg.get('reconcile_every', 1)}]
    out = Path(output).resolve()
    out.mkdir(parents=True, exist_ok=False)
    write(out / 'schedule.json', schedule)
    write(out / 'connection.json', {k: cfg[k] for k in ('type', 'host', 'port', 'user', 'secret_ref', 'period_ms') } | {'indexes': [index]})
    results = []
    for n in range(1, cycles + 1):
        if n > 1:
            shadow = json.loads((out / 'shadow.json').read_text())
            remaining = shadow['fires_ms']['connection'] + period - int(time.time() * 1000)
            if remaining > 0:
                time.sleep(remaining / 1000 + .05)
        dest = out / f'cycle-{n}'
        argv = ['docker', 'run', '--rm', '--pull', 'never', '--network', cfg['network'],
                '--user', '0:0', '--security-opt', 'no-new-privileges', '--cap-drop', 'ALL',
                '--label', 'owner=oo-opensearch-connection-whole', '--entrypoint', '/bins/os-reader',
                '-v', f'{reader}:/bins/os-reader:ro', '-v', f'{out}:/output',
                '-v', f'{private}:/private:ro', cfg['image'], 'c5-cycle',
                '--schedule', '/output/schedule.json', '--private', '/private', '--index', index,
                '--shadow', '/output/shadow.json', '--schedule-name', 'connection',
                '--page-size', str(cfg.get('page_size', 2)), '--out', f'/output/cycle-{n}']
        proc = subprocess.run(argv, capture_output=True)
        dest.mkdir(exist_ok=True)
        (dest / 'stdout').write_bytes(proc.stdout)
        (dest / 'stderr').write_bytes(proc.stderr)
        (dest / 'exit').write_text(f'{proc.returncode}\n')
        if proc.returncode:
            raise RuntimeError(f'cycle-{n}: exit {proc.returncode}; raw evidence retained')
        result = json.loads((dest / 'result.json').read_text())
        if result['reader_mutations'] != 0 or not result['report']['applied'] or not result['arrow_sha256']:
            raise RuntimeError('incomplete/read-write capture')
        actual = hashlib.sha256((dest / 'batch.arrow').read_bytes()).hexdigest()
        if actual != result['arrow_sha256']:
            raise RuntimeError('Arrow digest mismatch')
        results.append({'cycle': n, 'capture': str(dest), 'arrow_sha256': actual, 'raw_exit': proc.returncode})
        write(out / 'captures.json', {'protocol': 'oo.opensearch.connection-captures/v1', 'reader_sha256': digest,
              'capture_method': 'C5', 'consistency': 'B', 'source_mutations': 0, 'captures': results,
              'fullchain_executed': False})
    return out


if __name__ == '__main__':
    p = argparse.ArgumentParser()
    p.add_argument('--config', required=True)
    p.add_argument('--out', required=True)
    p.add_argument('--cycles', type=int, default=2)
    a = p.parse_args()
    print(run(a.config, a.out, a.cycles))
