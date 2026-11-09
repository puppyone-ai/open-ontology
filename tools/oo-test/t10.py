#!/usr/bin/env python3
"""T10 production impact. Run only on a quiet host; never against customer sources.

Native Python drivers are loaded only for live runs. Latencies include arrival
queueing, not merely database service time. Raw evidence is private/ignored.
"""
import argparse
from collections import Counter
import copy
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
import hashlib
import importlib
import importlib.util
import json
import math
from pathlib import Path
import queue
import re
import shutil
import signal
import subprocess
import threading
import time

import runner
import t1
import t3t4
from oracle_t3t4 import OracleHarness
from verdicts import product_sha

PHASES = ('baseline', 'initial', 'initial_duty100', 'c1', 'c5')
C5_PHASES = ('c5_idle', 'c5_changed', 'c5_full_idle', 'c5_full_changed')
SCALE = 2_000_000
WINDOW = 10


def percentile(values, percent):
    if not values:
        return None
    values = sorted(values)
    index = (len(values) - 1) * percent / 100
    lo = math.floor(index)
    return values[lo] + (values[math.ceil(index)] - values[lo]) * (index - lo)


def summarize(requests, duration):
    latencies = [r['end'] - r['intended'] for r in requests]
    return {**{f'p{p}_ms': None if not latencies else percentile(latencies, p) * 1000
               for p in (50, 95, 99)},
            'duration_seconds': duration, 'requests': len(requests),
            'errors': sum(r['error'] is not None for r in requests),
            'error_counts': dict(Counter(r['error'] for r in requests if r['error'] is not None)),
            'achieved_rate': sum(r['error'] is None and r['end'] < duration
                                 for r in requests) / duration if duration else 0,
            'max_start_delay_ms': max((r['actual'] - r['intended'] for r in requests
                                      if r['actual'] is not None), default=0) * 1000}


def windows(requests, duration):
    result = []
    for start in range(0, math.ceil(duration), WINDOW):
        end = min(start + WINDOW, duration)
        selected = [dict(r, intended=r['intended'] - start,
                         actual=None if r['actual'] is None else r['actual'] - start,
                         end=r['end'] - start)
                    for r in requests if start <= r['intended'] < end]
        summary = summarize(selected, end - start)
        # Completions, unlike latency cohorts, belong to their completion window.
        summary['achieved_rate'] = sum(r['error'] is None and start <= r['end'] < end
                                       for r in requests) / (end - start)
        result.append(dict(summary, start_seconds=start))
    return result


def verdict(baseline, phase):
    valid = (baseline and baseline['requests'] > 0 and phase['requests'] > 0
             and baseline['errors'] == 0 and baseline['achieved_rate'] > 0
             and all(baseline[k] is not None and baseline[k] > 0
                     and phase[k] is not None for k in ('p95_ms', 'p99_ms')))
    if not valid:
        return {'result': 'fail', 'reason': 'invalid_or_missing_baseline_or_samples'}
    deltas = {k: (phase[k] / baseline[k] - 1) * 100
              for k in ('p95_ms', 'p99_ms', 'achieved_rate')}
    checks = {'p95': deltas['p95_ms'] <= 20 + 1e-9,
              'p99': deltas['p99_ms'] <= 50 + 1e-9,
              'throughput': deltas['achieved_rate'] >= -5 - 1e-9,
              'zero_errors': phase['errors'] == 0}
    return {'result': 'pass' if all(checks.values()) else 'fail',
            'delta_percent': deltas, 'checks': checks}


def byte_size(text):
    match = re.fullmatch(r'\s*([\d.]+)\s*([kMGTPE]?i?B)\s*', text)
    if not match:
        raise ValueError('invalid docker byte count')
    unit = match[2]
    power = 'BKMGTPE'.index(unit[0].upper()) if unit != 'B' else 0
    return float(match[1]) * (1024 if 'i' in unit else 1000) ** power


def docker_metrics(text):
    row = json.loads(text)
    read, write = row['BlockIO'].split('/')
    result = {'cpu_percent': float(row['CPUPerc'].rstrip('%')),
              'block_read_bytes': byte_size(read), 'block_write_bytes': byte_size(write)}
    if any(not math.isfinite(v) or v < 0 for v in result.values()):
        raise ValueError('invalid docker metric')
    return result


METRICS = {
    'postgres': ('backend_xmin_age', 'n_dead_tup', 'autovacuum_count', 'slot_retained_wal_bytes'),
    'mysql': ('Innodb_history_list_length', 'Threads_running'),
    'oracle': ('undo_blocks', 'tuned_undoretention_seconds', 'ora_01555_count'),
}
QUERIES = {
    'postgres': "SELECT COALESCE((SELECT max(age(backend_xmin)) FROM pg_stat_activity "
                "WHERE usename='fixture_reader'),0),n_dead_tup,autovacuum_count,"
                "COALESCE((SELECT sum(pg_wal_lsn_diff(pg_current_wal_lsn(),restart_lsn)) "
                "FROM pg_replication_slots WHERE slot_name LIKE 's_t10%'),0) "
                "FROM pg_stat_user_tables WHERE relname='items';",
    'mysql': "SELECT 'Innodb_history_list_length',COUNT FROM information_schema.INNODB_METRICS "
             "WHERE NAME='trx_rseg_history_len' UNION ALL "
             "SELECT 'Threads_running',VARIABLE_VALUE FROM performance_schema.global_status "
             "WHERE VARIABLE_NAME='Threads_running';",
    'oracle': "SELECT NVL(SUM(undoblks),0)||'|'||NVL(MAX(tuned_undoretention),0)||'|'||"
              "NVL(SUM(ssolderrcnt),0) FROM v$undostat;",
}
MINER_SQL = ("set heading off feedback off pagesize 0\n"
             "SELECT NVL(SUM(st.value),0) FROM v$sesstat st JOIN v$statname n "
             "ON st.statistic#=n.statistic# JOIN v$session s ON s.sid=st.sid "
             "WHERE s.username='C##OO_MINER' AND n.name='CPU used by this session';")


def sql_metrics(engine, text):
    if engine == 'mysql':
        rows = dict(line.split() for line in text.strip().splitlines() if line.strip())
        result = {k: float(rows[k]) for k in METRICS[engine]}
        # History-list length is the purge backlog, not a time-lag measurement.
        result['purge_backlog_transactions'] = result['Innodb_history_list_length']
    else:
        lines = [line.strip() for line in text.splitlines() if line.strip()]
        if len(lines) != 1:
            raise ValueError('missing or ambiguous metric row')
        values = lines[0].split('|')
        if len(values) != len(METRICS[engine]):
            raise ValueError('metric column count')
        result = dict(zip(METRICS[engine], map(float, values)))
    if any(not math.isfinite(v) or v < 0 for v in result.values()):
        raise ValueError('invalid metric value')
    return result


class ImpactSource(t3t4.Source):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.foreground_key = SCALE + 1
        self.foreground_lock = threading.Lock()

    def next_foreground_key(self):
        # Calibration and every phase write the same source table.
        with self.foreground_lock:
            key = self.foreground_key
            self.foreground_key += 1
            return key

    def command(self, command, **kwargs):
        command = list(command)
        if command[:2] == ['docker', 'create'] and self.name in command:
            command[2:2] = ['--cpus', str(self.args.cpus), '--memory', self.args.memory]
        return super().command(command, **kwargs)


class Workload:
    """Persistent fixture-writer connections; separate from product read-only roles."""
    def __init__(self, run):
        self.run = run
        self.local = threading.local()
        self.connections = []
        self.lock = threading.Lock()

    def connect(self):
        run = self.run
        if run.engine == 'postgres':
            conn = importlib.import_module('psycopg').connect(
                host='127.0.0.1', port=run.host_port, dbname='fixture',
                user='t10_writer', password=run.secret, autocommit=True,
                connect_timeout=10, options='-c statement_timeout=30000')
        elif run.engine == 'mysql':
            conn = importlib.import_module('pymysql').connect(
                host='127.0.0.1', port=run.host_port, database='fixture',
                user='t10_writer', password=run.secret, autocommit=True,
                connect_timeout=10, read_timeout=30, write_timeout=30)
        else:
            conn = importlib.import_module('oracledb').connect(
                user='fixture', password=run.secret,
                dsn=f'127.0.0.1:{run.host_port}/FREEPDB1')
            conn.autocommit = True
            conn.call_timeout = 30000
        with self.lock:
            self.connections.append(conn)
        return conn

    def __call__(self, index):
        if not getattr(self.local, 'conn', None):
            self.local.conn = self.connect()
        # Fixed 60/30/10 mix and deterministic hot keys in every phase.
        hot = index % 20 + 1
        if index % 10 < 6:
            sql = f'SELECT payload FROM items WHERE id={hot}'
        elif index % 10 < 9:
            sql = f'UPDATE items SET n=n+1 WHERE id={hot}'
        else:
            key = self.run.next_foreground_key()
            sql = f"INSERT INTO other VALUES({key},'foreground',{index % 1000000})"
        with self.local.conn.cursor() as cursor:
            cursor.execute(sql)
            if index % 10 < 6:
                if cursor.fetchone() is None:
                    raise RuntimeError('point lookup missing')

    def close(self):
        for conn in self.connections:
            conn.close()
        self.connections.clear()


def open_loop(work, rate, seconds, finished=None, clock=time.monotonic,
              sleep=time.sleep, workers=32, on_start=lambda: None):
    """Bounded queue: overloaded arrivals are errors, never silently dropped.

    A completion-driven phase uses finished instead of seconds. The scheduler
    never waits for an earlier request; end-to-end latency starts at intended.
    """
    jobs = queue.Queue(maxsize=workers * 32)
    results = []
    lock = threading.Lock()
    start = clock()

    def worker():
        while True:
            job = jobs.get()
            if job is None:
                jobs.task_done()
                return
            index, intended = job
            actual = clock() - start
            error = None
            try:
                work(index)
            except Exception as exc:
                error = type(exc).__name__  # Driver messages can contain credentials.
            record = dict(intended=intended, actual=actual, end=clock() - start, error=error)
            with lock:
                results.append(record)
            jobs.task_done()

    threads = [threading.Thread(target=worker) for _ in range(workers)]
    for thread in threads:
        thread.start()
    index = 0
    try:
        on_start()
        while True:
            intended = index / rate
            if (seconds is not None and intended >= seconds) or (finished and finished.is_set()):
                break
            delay = start + intended - clock()
            if delay > 0:
                sleep(delay)
            if finished and finished.is_set():
                break
            try:
                jobs.put_nowait((index, intended))
            except queue.Full:
                results.append(dict(intended=intended, actual=None, end=clock() - start,
                                    error='scheduler_queue_full'))
            index += 1
        if seconds is not None:
            sleep(max(0, start + seconds - clock()))
        duration = max(clock() - start, 1e-9)
    finally:
        for _ in threads:
            jobs.put(None)
        for thread in threads:
            thread.join()
    results.sort(key=lambda row: row['intended'])
    for row in results:
        row['latency_ms'] = (row['end'] - row['intended']) * 1000
        row['service_ms'] = None if row['actual'] is None else (row['end'] - row['actual']) * 1000
    return results, duration


def calibrate(work, seconds=10, levels=(1, 2, 4, 8, 16, 32, 64, 128), close=lambda: None):
    samples = []
    for concurrency in levels:
        stop = time.monotonic() + seconds
        next_index = 0
        index_lock = threading.Lock()

        def loop(worker):
            nonlocal next_index
            count = errors = 0
            while time.monotonic() < stop:
                try:
                    # Worker-local strides bias the mix toward faster operations.
                    with index_lock:
                        index = next_index
                        next_index += 1
                    work(index)
                    count += 1
                except Exception:
                    errors += 1
                    break
            return count, errors

        started = time.monotonic()
        try:
            with ThreadPoolExecutor(max_workers=concurrency) as pool:
                counts = list(pool.map(loop, range(concurrency)))
        finally:
            close()
        elapsed = time.monotonic() - started
        row = {'concurrency': concurrency, 'rate': sum(c for c, _ in counts) / elapsed,
               'errors': sum(e for _, e in counts), 'seconds': elapsed}
        samples.append(row)
        if row['errors']:
            raise RuntimeError('calibration_errors')
        if len(samples) >= 3 and row['rate'] <= samples[-2]['rate'] * 1.05:
            capacity = max(s['rate'] for s in samples)
            return {'capacity': capacity, 'rate': capacity * .55, 'samples': samples}
    raise RuntimeError('calibration_capacity_not_found_at_concurrency_cap')

def valid_baseline(result):
    return (verdict(result, result)['result'] == 'pass'
            and not result['product_errors'] and not result['metrics']['errors']
            and bool(result['metrics']['samples']))


class Meter:
    def __init__(self, harness, sequence):
        self.h = copy.copy(harness)
        self.h.run = copy.copy(harness.run)
        self.h.run.seq = sequence
        self.samples, self.errors = [], []
        self.stop = threading.Event()
        self.thread = threading.Thread(target=self.collect)

    def collect(self):
        started = time.monotonic()
        while not self.stop.is_set():
            before = time.monotonic()
            try:
                run = self.h.run
                _, raw = run.command(['docker', 'stats', '--no-stream', '--format', '{{json .}}', run.name])
                values = docker_metrics(raw)
                _, raw = run.sql(QUERIES[run.engine], admin=True)
                values.update(sql_metrics(run.engine, raw))
                if run.engine == 'oracle':
                    _, raw = self.h.root_sql(MINER_SQL)
                    values['logminer_session_cpu_centiseconds'] = float(raw.strip())
                    if not math.isfinite(values['logminer_session_cpu_centiseconds']):
                        raise ValueError('invalid miner CPU')
                self.samples.append(dict(seconds=before - started, **values))
            except Exception as exc:
                self.errors.append(type(exc).__name__)
            self.stop.wait(max(0, WINDOW - (time.monotonic() - before)))

    def finish(self):
        self.stop.set()
        self.thread.join()
        keys = set().union(*(s.keys() for s in self.samples)) - {'seconds'}
        return {'samples': self.samples, 'errors': self.errors,
                'peaks': {k: max(s[k] for s in self.samples) for k in keys},
                'changes': {k: self.samples[-1][k] - self.samples[0][k] for k in keys}}


def phase(harness, name, work, rate, seconds, action, sequence):
    started = time.monotonic()
    done = threading.Event()
    product_errors = []
    meter = Meter(harness, sequence)

    def product():
        try:
            action()
        except Exception as exc:
            product_errors.append(type(exc).__name__)
        finally:
            done.set()

    thread = threading.Thread(target=product)
    meter.thread.start()
    try:
        requests, duration = open_loop(work, rate, seconds,
                                       finished=done if seconds is None else None,
                                       on_start=thread.start)
    finally:
        thread.join()
        metrics = meter.finish()
        work.close()
    result = dict(summarize(requests, duration), phase=name, configured_rate=rate,
                  observation_seconds=time.monotonic() - started,
                  windows=windows(requests, duration), metrics=metrics,
                  product_errors=product_errors)
    runner.write_json(harness.run.path / f'{name}-requests.json', requests)
    runner.write_json(harness.run.path / f'{name}.json', result)
    return result


def loader(h, sid, label, mode=None, policy=None, duty100=False):
    argv = h.argv(sid, label, mode, policy)
    if duty100:
        argv += ['--duty-cycle-percent', '100']
    _, out = h.run.command(argv, timeout=86400)
    receipts = t3t4.rows(out)
    if not receipts or any('error' in r for r in receipts):
        raise RuntimeError('missing_or_failed_loader_receipt')
    if duty100 and any(r.get('read_budget', {}).get('duty_cycle_percent') != 100 for r in receipts):
        raise RuntimeError('duty100_not_confirmed')
    return receipts


def prepare(h):
    h.prepare()
    # Same full T1 row scale, with the T3/T4 narrow synthetic schema.
    if h.run.engine == 'postgres':
        h.src(f"INSERT INTO bulk SELECT i,'bulk-'||i,i FROM generate_series({t3t4.BULK_ROWS+1},{SCALE}) i;")
        h.src(f"CREATE ROLE t10_writer LOGIN PASSWORD '{h.run.secret}';"
              "GRANT USAGE ON SCHEMA public TO t10_writer;"
              "GRANT SELECT,UPDATE ON items TO t10_writer; GRANT INSERT ON other TO t10_writer;")
    elif h.run.engine == 'oracle':
        h.src(f"INSERT INTO bulk SELECT level+{t3t4.BULK_ROWS},'bulk',level FROM dual "
              f"CONNECT BY level <= {SCALE-t3t4.BULK_ROWS};")
    else:
        for start in range(t3t4.BULK_ROWS + 1, SCALE + 1, 1000):
            h.src('INSERT INTO bulk VALUES' + ','.join(
                f"({i},'bulk',{i})" for i in range(start, min(start+1000, SCALE+1))) + ';')
        h.src(f"CREATE USER 't10_writer'@'%' IDENTIFIED BY '{h.run.secret}';"
              "GRANT SELECT,UPDATE ON fixture.items TO 't10_writer'@'%';"
              "GRANT INSERT ON fixture.other TO 't10_writer'@'%';")

def c5_phases(h, rate, every, sequence):
    results = {}
    for variant, period in (('c5', every), ('c5_full', 1)):
        for suffix, tables in (('idle', ['bulk']), ('changed', ['bulk', 'items', 'other'])):
            name = f'{variant}_{suffix}'
            binding = 't10_' + name
            # Independent baselines prevent the first policy priming the second.
            h.snapshot(binding, tables)
            if suffix == 'changed':
                h.src('UPDATE items SET n=n+1 WHERE id=1;')
            policy = {'cycle': 1, 'full_read_every': period}
            result = phase(h, name, Workload(h.run), rate, None,
                           lambda: loader(h, binding, f'{binding}-cycle', '--c5', policy),
                           sequence + len(results) * 1_000_000)
            result['c5_policy'] = policy
            results[name] = result
    return results


def parse_args(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--engine', required=True, choices=('postgres', 'mysql', 'oracle'))
    parser.add_argument('--version', required=True)
    parser.add_argument('--phases', default=','.join(PHASES))
    parser.add_argument('--minutes', type=float, default=10)
    parser.add_argument('--rate', default='auto')
    parser.add_argument('--cpus', type=float, default=2)
    parser.add_argument('--memory', default='4g')
    parser.add_argument('--evidence', type=Path, required=True)
    parser.add_argument('--binary', type=Path, default=runner.ROOT / 'tools/oo-load/target/release/oo-load')
    args = parser.parse_args(argv)
    versions = {'postgres': t3t4.POSTGRES, 'mysql': t3t4.MYSQL, 'oracle': ('19', '23')}
    if args.version not in versions[args.engine]:
        parser.error('unsupported version (Oracle live harness requires 19 or 23)')
    args.phases = args.phases.split(',')
    if (not args.phases or args.phases[0] != 'baseline' or len(set(args.phases)) != len(args.phases)
            or any(p not in PHASES for p in args.phases)):
        parser.error('unique known phases required, with baseline first')
    for value in (args.minutes, args.cpus):
        if not math.isfinite(value) or value <= 0:
            parser.error('minutes and cpus must be finite and positive')
    if not re.fullmatch(r'[1-9][0-9]*[kmgKMG]?', args.memory):
        parser.error('memory must be a positive Docker memory size')
    if args.rate != 'auto':
        try:
            args.rate = float(args.rate)
            if not math.isfinite(args.rate) or args.rate <= 0:
                raise ValueError()
        except ValueError:
            parser.error('rate must be auto or finite and positive')
    args.evidence = args.evidence.resolve()
    if not args.evidence.is_relative_to(runner.ROOT / 'evidence') or args.evidence == runner.ROOT / 'evidence':
        parser.error('evidence must be a new directory under worktree evidence/')
    return args


QUIET_CPU_PERCENT = 5.0


def quiet_host(samples=3):
    """Pre-existing containers are never stopped; they must be measured idle.

    Returns the containers seen (name and peak CPU %) for the evidence report.
    """
    peaks = {}
    for _ in range(samples):
        text = subprocess.run(['docker', 'stats', '--no-stream', '--format', '{{.Name}}\t{{.CPUPerc}}'],
                              capture_output=True, text=True, check=True).stdout
        for line in text.splitlines():
            name, _, cpu = line.partition('\t')
            value = float(cpu.strip().rstrip('%') or 0)
            peaks[name] = max(peaks.get(name, 0.0), value)
    if sum(peaks.values()) > QUIET_CPU_PERCENT:
        raise SystemExit('blocked: pre-existing containers are not idle; nothing stopped')
    return [{'name': name, 'peak_cpu_percent': cpu} for name, cpu in sorted(peaks.items())]


def main(argv=None):
    args = parse_args(argv)
    # Preflight before creating resources. The unpatched loader must not silently
    # label its default 25% duty run as the requested 100% tradeoff.
    binary = args.binary.resolve()
    help_text = subprocess.run([str(binary), '--help'], capture_output=True, text=True, check=True).stdout
    if 'initial_duty100' in args.phases and '--duty-cycle-percent' not in help_text:
        raise SystemExit('blocked: oo-load needs the coordinator duty-cycle patch in T10-VALIDATION.md')
    importlib.import_module({'postgres': 'psycopg', 'mysql': 'pymysql', 'oracle': 'oracledb'}[args.engine])
    preexisting = quiet_host()
    t1.LANE = 't10'
    args.evidence.mkdir(parents=True, mode=0o700, exist_ok=False)
    args.platform, args.health_timeout, args.scale, args.tables, args.worker_image = None, 600, SCALE, 0, None
    run = ImpactSource(args.evidence, args.engine, args.version, args)
    run.receipt = {}
    h = (OracleHarness if args.engine == 'oracle' else t3t4.Harness)(run, binary)
    report = {'engine': args.engine, 'version': args.version, 'phases': {}, 'errors': [],
              'preexisting_containers': preexisting,
              'cpus': args.cpus, 'memory': args.memory, 'scale': SCALE,
              'product_sha': product_sha(), 'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
              'measurement_notes': ['client-observed latency includes queueing',
                                    'Oracle undo counters cover the V$UNDOSTAT retention horizon',
                                    'LogMiner CPU is a sampled live-session counter, not total exited-session CPU',
                                    'C5 idle scans bulk only; foreground writes items/other outside that scope']}
    cleanup = []
    try:
        prepare(h)
        run.record_version()
        report['resource_receipt'] = run.receipt
        _, limits = run.command(['docker', 'inspect', '--format', '{{json .HostConfig}}', run.name])
        limits = json.loads(limits)
        report['source_limits'] = {k: limits[k] for k in ('NanoCpus', 'Memory')}
        work = Workload(run)
        try:
            if args.rate == 'auto':
                report['calibration'] = calibrate(work, close=work.close)
                rate = report['calibration']['rate']
            else:
                rate = args.rate
                report['calibration'] = {'skipped': 'explicit rate', 'rate': rate}
        finally:
            work.close()
        # Read the actual checked-out lite default, rather than duplicating it.
        spec = importlib.util.spec_from_file_location('t10_lite_scheduler', runner.ROOT / 'deploy/lite/scheduler.py')
        scheduler = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(scheduler)
        every = scheduler.c5_full_read_every({})
        report['c5_full_read_every'] = every
        tables = ['bulk', 'items', 'other']
        for number, name in enumerate(args.phases):
            seconds = args.minutes * 60 if name in ('baseline', 'c1') else None
            sid = 't10_' + name
            h.add_source(sid, tables)
            if name == 'baseline':
                action = lambda: None
            elif name.startswith('initial'):
                h.target.close()
                h.target = t3t4.DurableAgentDB(run)
                h.target.start()
                t1.private(h.store / 'target-password', h.target.password)
                action = lambda: loader(h, sid, sid, duty100=name == 'initial_duty100')
            elif name == 'c1':
                h.c1_binding(sid, tables)  # Quiescent aligned baseline, outside measurement.

                def action():
                    deadline = time.monotonic() + seconds
                    while time.monotonic() < deadline:
                        loader(h, sid, f'{sid}-{h.calls}', '--sync', h.sync[sid])
            else:
                # Separate scope makes the idle cycle genuinely unchanged while
                # preserving the identical foreground workload in both subphases.
                report['phases'].update(c5_phases(h, rate, every, 10_000_000 * (number + 1)))
                continue
            report['phases'][name] = phase(h, name, Workload(run), rate, seconds, action,
                                           10_000_000 * (number + 1))
            if name == 'baseline' and not valid_baseline(report['phases'][name]):
                report['errors'].append('invalid_baseline')
                break
            if name == 'c1':
                h.retire(sid)
    except Exception as exc:
        report['errors'].append(type(exc).__name__)
    finally:
        for close in (h.target.close, run.cleanup):
            try:
                failures = close()
                if failures:
                    cleanup.extend(failures)
            except Exception as exc:
                cleanup.append(type(exc).__name__)
        shutil.rmtree(h.store)
        report['cleanup_failures'] = cleanup
        baseline = report['phases'].get('baseline')
        for name, result in report['phases'].items():
            result['budget'] = verdict(baseline, result)
            if (result['product_errors'] or result['metrics']['errors'] or not result['metrics']['samples']
                    or not baseline or baseline['metrics']['errors'] or baseline['product_errors']
                    or not baseline['metrics']['samples']):
                result['budget'].update(result='fail', reason='product_or_metrics_incomplete')
            runner.write_json(run.path / f'{name}.json', result)
        expected = [p for p in args.phases if p != 'c5'] + (list(C5_PHASES) if 'c5' in args.phases else [])
        report['result'] = 'pass' if (not report['errors'] and not cleanup
            and all(report['phases'].get(p, {}).get('budget', {}).get('result') == 'pass' for p in expected)) else 'fail'
        runner.write_json(run.path / 'summary.json', report)
        digest = hashlib.sha256((run.path / 'summary.json').read_bytes()).hexdigest()
        # Existing verdicts.py intentionally only accepts T0..T8. Export explicit
        # T10 rows separately for coordinator ingestion, without weakening it.
        runner.write_json(run.path / 't10-index-rows.json', [
            {'suite': 'T10', 'variant': p, 'target': f'{args.engine}-{args.version}',
             'version': run.receipt.get('version_actual'), 'run_id': run.path.name,
             'evidence': 'summary.json', 'sha256': digest, 'product_sha': report['product_sha'],
             'version_measured': bool(run.receipt.get('version_actual')),
             'observed_at': datetime.now(timezone.utc).isoformat(),
             'emulated': run.receipt.get('execution') == 'emulated',
             'result': 'fail' if cleanup or report['errors'] else
                 report['phases'].get(p, {}).get('budget', {}).get('result', 'fail')}
            for p in expected])
        runner.checksums(run.path)
    print(json.dumps({'result': report['result'], 'run_id': run.path.name}))
    return 0 if report['result'] == 'pass' else 1


if __name__ == '__main__':
    def interrupted(signum, frame):
        raise KeyboardInterrupt(f'signal {signum}')
    signal.signal(signal.SIGTERM, interrupted)
    raise SystemExit(main())
