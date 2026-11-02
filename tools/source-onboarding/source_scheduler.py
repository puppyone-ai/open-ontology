#!/usr/bin/env python3
"""Bounded multi-source runner around existing provisioning and onboarding commands."""
import argparse
import json
import math
import os
from pathlib import Path
import signal
import subprocess
import time


def number(value):
    if type(value) not in (int, float) or not math.isfinite(value) or value < 0:
        raise ValueError('numeric_budget')
    return value


def guard(metrics, limits, now):
    observed, egress, balance = (number(metrics.get(k)) for k in ('observed_at', 'egress_bytes', 'balance'))
    maximum, minimum = (number(limits.get(k)) for k in ('max_egress_bytes', 'min_balance'))
    age = number(limits.get('max_age_seconds', 60))
    if observed > now + 5 or now - observed > age:
        return 'metrics_stale'
    if egress >= maximum:
        return 'egress_limit'
    if balance <= minimum:
        return 'balance_limit'
    return None


def argv_check(argv):
    if not isinstance(argv, list) or not argv or any(
            not isinstance(a, str) or '\0' in a for a in argv):
        raise ValueError('argv')


def stop(process):
    if process.poll() is not None:
        return
    os.killpg(process.pid, signal.SIGTERM)
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGKILL)
        process.wait(timeout=5)


def run(spec, out, previous=None):
    concurrency = spec.get('concurrency', 2)
    if type(concurrency) is not int or not 1 <= concurrency <= 32:
        raise ValueError('concurrency')
    stagger = number(spec.get('stagger_seconds', 10))
    deadlines = {phase: number(spec.get(phase + '_timeout', 86400 if phase == 'table' else 600))
                 for phase in ('provision', 'table')}
    if not all(deadlines.values()):
        raise ValueError('deadline')
    wanted = None if previous is None else {(f['source'], f['table']) for f in previous['failed']}
    pending, identities = [], set()
    for source in spec['sources']:
        argv_check(source['provision'])
        tables = []
        for table in source['tables']:
            argv_check(table['argv'])
            identity = (source['key'], table['table'])
            if identity in identities:
                raise ValueError('duplicate_table')
            identities.add(identity)
            if wanted is None or identity in wanted:
                tables.append(table)
        if tables:
            pending.append(dict(source=source, tables=tables, phase='provision', index=0))
    if wanted is not None and not wanted <= identities:
        raise ValueError('retry_identity_changed')
    out = Path(out)
    out.mkdir(mode=0o700, parents=True, exist_ok=False)
    active, results, paused, next_start = [], [], None, 0
    tunnel = None
    tunnel_spec = spec.get('tunnel')
    tunnel_attempts, tunnel_next, tunnel_started = 0, 0, 0
    if tunnel_spec:
        argv_check(tunnel_spec['argv'])
        argv_check(tunnel_spec['probe'])
    limits = spec.get('cost_guard')

    def checkpoint():
        result = dict(protocol='oo.source-schedule-result/v1', paused=paused, results=results,
                      failed=[dict(source=r['source'], table=r['table'], reason=r['reason'])
                              for r in results if r['reason'] != 'ok'])
        tmp = out/'summary.tmp'
        tmp.write_text(json.dumps(result, indent=2) + '\n')
        tmp.replace(out/'summary.json')
        return result

    def launch(state):
        phase = state['phase']
        argv = state['source']['provision'] if phase == 'provision' else state['tables'][state['index']]['argv']
        log = out/f'{len(list(out.glob("*.log"))):06d}.log'
        with log.open('wb') as stream:
            state['process'] = subprocess.Popen(argv, stdout=stream, stderr=stream, start_new_session=True)
        state['started'] = time.monotonic()

    def finish(state, reason):
        for table in state['tables'][state['index']:]:
            results.append(dict(source=state['source']['key'], table=table['table'], reason=reason))

    try:
        while pending or active:
            if limits:
                try:
                    paused = guard(json.loads(Path(limits['metrics_file']).read_text()), limits, time.time())
                except (OSError, ValueError, KeyError, TypeError):
                    paused = 'metrics_unavailable'
                if paused:
                    break
            ready = True
            if tunnel_spec:
                if tunnel is not None and tunnel.poll() is not None:
                    tunnel = None
                    tunnel_next = time.monotonic() + number(tunnel_spec.get('backoff_seconds', 5))
                if tunnel is None and time.monotonic() >= tunnel_next:
                    if tunnel_attempts >= tunnel_spec.get('max_attempts', 5):
                        paused = 'tunnel_reconnect_exhausted'
                        break
                    with (out/'tunnel.log').open('ab') as stream:
                        tunnel = subprocess.Popen(tunnel_spec['argv'], stdout=stream, stderr=stream,
                                                  start_new_session=True)
                    tunnel_attempts += 1
                    tunnel_started = time.monotonic()
                ready = False
                if tunnel is not None:
                    try:
                        ready = subprocess.run(tunnel_spec['probe'], stdout=subprocess.DEVNULL,
                                               stderr=subprocess.DEVNULL, timeout=2).returncode == 0
                    except subprocess.TimeoutExpired:
                        pass
                if not ready and tunnel is not None and time.monotonic() - tunnel_started > number(tunnel_spec.get('ready_timeout', 30)):
                    stop(tunnel)
                    tunnel = None
                    tunnel_next = time.monotonic() + number(tunnel_spec.get('backoff_seconds', 5))
            now = time.monotonic()
            if ready and pending and len(active) < concurrency and now >= next_start:
                state = pending.pop(0)
                launch(state)
                active.append(state)
                next_start = now + stagger
            for state in list(active):
                if 'process' not in state:
                    continue
                process, phase = state['process'], state['phase']
                exit_code = process.poll()
                expired = now - state['started'] > deadlines[phase]
                if exit_code is None and not expired:
                    continue
                if expired:
                    stop(process)
                reason = 'timeout' if expired else f'exit_{exit_code}'
                if phase == 'provision':
                    if exit_code != 0 or expired:
                        finish(state, 'provision_' + reason)
                        active.remove(state)
                    else:
                        state['phase'] = 'table'
                        state.pop('process')
                else:
                    table = state['tables'][state['index']]
                    results.append(dict(source=state['source']['key'], table=table['table'],
                                        reason='ok' if exit_code == 0 and not expired else reason))
                    state['index'] += 1
                    if state['index'] == len(state['tables']):
                        active.remove(state)
                    else:
                        state.pop('process')
                checkpoint()
            for state in active:
                if 'process' not in state and ready:
                    launch(state)
            time.sleep(.05)
    except BaseException:
        paused = 'runner_interrupted'
        raise
    finally:
        for state in active:
            if 'process' in state:
                stop(state['process'])
            finish(state, paused or 'runner_aborted')
        for state in pending:
            finish(state, paused or 'runner_aborted')
        if tunnel is not None:
            stop(tunnel)
        result = checkpoint()
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--config', required=True, type=Path)
    parser.add_argument('--out', required=True, type=Path)
    parser.add_argument('--retry-failed', type=Path)
    args = parser.parse_args()
    os.umask(0o077)
    result = run(json.loads(args.config.read_text()), args.out,
                 json.loads(args.retry_failed.read_text()) if args.retry_failed else None)
    return 75 if result['paused'] else (1 if result['failed'] else 0)


if __name__ == '__main__':
    raise SystemExit(main())
