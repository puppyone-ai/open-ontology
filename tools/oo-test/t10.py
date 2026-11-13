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
    starts = range(0, math.ceil(duration), WINDOW)
    cohorts = [[] for _ in starts]
    completions = [0 for _ in starts]
    # Bucket once: long loads must not rescan millions of rows per window.
    for row in requests:
        if 0 <= row['intended'] < duration:
            cohorts[int(row['intended'] // WINDOW)].append(row)
        if row['error'] is None and 0 <= row['end'] < duration:
            completions[int(row['end'] // WINDOW)] += 1
    result = []
    for index, start in enumerate(starts):
        end = min(start + WINDOW, duration)
        selected = [dict(r, intended=r['intended'] - start,
                         actual=None if r['actual'] is None else r['actual'] - start,
                         end=r['end'] - start)
                    for r in cohorts[index]]
        summary = summarize(selected, end - start)
        # Completions, unlike latency cohorts, belong to their completion window.
        summary['achieved_rate'] = completions[index] / (end - start)
        result.append(dict(summary, start_seconds=start))
    return result

SLICE_SECONDS = 50


def baseline_noise(requests, duration, whole, seconds=SLICE_SECONDS, step=WINDOW):
    """D24 applied to sliding slices of the baseline against the whole baseline.

    With no product running, a fixture whose slices fail D24 against
    themselves cannot support a relative verdict for phases of that length.
    """
    buckets = [[] for _ in range(math.ceil(duration / step))]
    for row in requests:
        if 0 <= row['intended'] < duration:
            buckets[int(row['intended'] // step)].append(row)
    width = seconds // step
    slices = []
    for first in range(0, len(buckets) - width + 1):
        start = first * step
        if start + seconds > duration + 1e-6:
            break
        selected = [dict(r, intended=r['intended'] - start,
                         actual=None if r['actual'] is None else r['actual'] - start,
                         end=r['end'] - start)
                    for bucket in buckets[first:first + width] for r in bucket]
        result = verdict(whole, summarize(selected, seconds))
        slices.append({'start_seconds': start, 'result': result['result'],
                       'delta_percent': result.get('delta_percent', {})})
    deltas = lambda key: [s['delta_percent'].get(key, 0) for s in slices]
    return {'slice_seconds': seconds, 'step_seconds': step, 'slices': len(slices),
            'fail_p95': sum(d > 20 + 1e-9 for d in deltas('p95_ms')),
            'fail_p99': sum(d > 50 + 1e-9 for d in deltas('p99_ms')),
            'fail_any': sum(s['result'] != 'pass' for s in slices),
            'max_delta_p95_percent': max(deltas('p95_ms'), default=None),
            'max_delta_p99_percent': max(deltas('p99_ms'), default=None)}


def same_length_noise(base_requests, baseline, seconds):
    """Baseline slices as long as a phase, against the whole baseline: the
    deltas the fixture produces with no product for a phase of that length."""
    length = int(seconds // WINDOW) * WINDOW
    if length < WINDOW or length >= baseline['duration_seconds']:
        return None
    return baseline_noise(base_requests, baseline['duration_seconds'], baseline, seconds=length)


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
        # Bounded and large: the product refuses unlimited retention as
        # unproven WAL headroom, and 256MB invalidated the slot under load.
        self.slot_wal_keep_size = SLOT_WAL_KEEP_SIZE

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
            if self.engine == 'postgres' and getattr(self.args, 'diagnose', False):
                command += DIAGNOSE_POSTGRES
            if self.engine == 'mysql':
                command += [f'--{name.replace("_", "-")}={value}'
                            for name, value in MYSQL_SOURCE_SETTINGS.items()]
        elif (command[:2] == ['docker', 'create'] and getattr(self.args, 'target_tmpfs', False)
              and TARGET_DATA_MOUNT in ' '.join(command)):
            command = target_on_tmpfs(command)
        return super().command(command, **kwargs)


SLOT_WAL_KEEP_SIZE = '16GB'
# T10 MySQL source only, sized for the fixture's write rate on a 4-CPU/8 GiB
# container: with the 8.4 defaults (100 MB redo, 128 MB buffer pool, 200 IOPS)
# the foreground stalled 3-6 s on its own, so no relative verdict held.
# Durability stays at the defaults (sync_binlog=1, flush_log_at_trx_commit=1).
MYSQL_SOURCE_SETTINGS = {
    'innodb_redo_log_capacity': 4 * 1024 ** 3,
    'innodb_buffer_pool_size': 4 * 1024 ** 3,
    'innodb_log_buffer_size': 64 * 1024 ** 2,
    'innodb_io_capacity': 2000,
    'innodb_io_capacity_max': 4000,
}
# Diagnostic runs only (--diagnose): never part of a D24 measurement.
DIAGNOSE_POSTGRES = ['-c', 'shared_preload_libraries=pg_stat_statements',
                     '-c', 'pg_stat_statements.track=all', '-c', 'track_io_timing=on',
                     '-c', 'log_checkpoints=on', '-c', 'log_autovacuum_min_duration=0',
                     '-c', 'log_lock_waits=on']
ACTIVITY_SQL = ("SELECT extract(epoch FROM clock_timestamp())::numeric(16,3),pid,"
                "COALESCE(usename,''),backend_type,COALESCE(state,''),"
                "COALESCE(wait_event_type,''),COALESCE(wait_event,''),"
                "COALESCE(extract(epoch FROM clock_timestamp()-query_start)::numeric(10,3),0),"
                "regexp_replace(left(query,100),'[\\s|]+',' ','g') FROM pg_stat_activity "
                "WHERE pid<>pg_backend_pid() AND (backend_type IN ('client backend','autovacuum worker',"
                "'walsender') AND state<>'idle' AND COALESCE(usename,'')<>'t10_writer' "
                "OR wait_event_type IN ('Lock','LWLock','IO'));")
STATEMENTS_SQL = ("SELECT r.rolname,s.calls,round(s.total_exec_time::numeric,1),"
                  "round(s.max_exec_time::numeric,1),s.rows,s.shared_blks_hit,s.shared_blks_read,"
                  "s.shared_blks_dirtied,s.temp_blks_written,s.wal_bytes,"
                  "regexp_replace(left(s.query,140),'[\\s|]+',' ','g') FROM pg_stat_statements s "
                  "JOIN pg_roles r ON r.oid=s.userid ORDER BY s.total_exec_time DESC LIMIT 40;")
COUNTERS_SQL = ("SELECT (SELECT row_to_json(c)::text FROM pg_stat_checkpointer c),"
                "(SELECT row_to_json(b)::text FROM pg_stat_bgwriter b),"
                "(SELECT json_agg(json_build_array(backend_type,object,context,reads,writes,"
                "extends,fsyncs))::text FROM pg_stat_io WHERE reads>0 OR writes>0 OR fsyncs>0);")


class Activity:
    """Diagnostic sampler: one long-lived psql session inside the source.

    Samples non-idle product/maintenance sessions and every wait on locks or
    IO every 250 ms. It runs in every phase, baseline included.
    """
    def __init__(self, run):
        self.run = run
        self.path = run.path / 'diagnose-activity.tsv'
        self.process = None

    def start(self):
        script = 'SET application_name=t10_diagnose;\n' + ACTIVITY_SQL + '\n\\watch 0.25\n'
        self.out = self.path.open('w')
        self.process = subprocess.Popen(
            ['docker', 'exec', '-i', '-u', 'postgres', self.run.name, 'psql', '-X', '-q', '-A', '-t',
             '-F', '|', '-U', 'postgres', '-d', 'fixture'],
            stdin=subprocess.PIPE, stdout=self.out, stderr=subprocess.DEVNULL, text=True)
        self.process.stdin.write(script)
        self.process.stdin.flush()

    def stop(self):
        if self.process:
            self.process.terminate()
            try:
                self.process.wait(10)
            except subprocess.TimeoutExpired:
                self.process.kill()
            self.out.close()


# Host IO canary: an 8 KiB O_DSYNC write in the source container (its
# writable layer shares the Docker disk with the data volume) every 100 ms.
CANARY_SCRIPT = ('while :; do s=$(date +%s%N); '
                 'dd if=/dev/zero of=/tmp/oo-t10-canary bs=8192 count=1 oflag=dsync conv=notrunc '
                 '2>/dev/null; e=$(date +%s%N); echo "$s $e"; sleep 0.1; done')
CANARY_STALL_MS = 100


class IoCanary:
    """Measures the fsync latency the source container's disk sees.

    It writes no database object and runs in every phase, baseline included,
    so a host IO stall is identified independently of the product.
    """
    def __init__(self, run):
        self.run = run
        self.path = run.path / 'io-canary.tsv'
        self.process = None

    def start(self):
        self.out = self.path.open('w')
        self.process = subprocess.Popen(['docker', 'exec', self.run.name, 'sh', '-c', CANARY_SCRIPT],
                                        stdout=self.out, stderr=subprocess.DEVNULL)

    def stop(self):
        if self.process:
            self.process.terminate()
            try:
                self.process.wait(10)
            except subprocess.TimeoutExpired:
                self.process.kill()
            self.out.close()

    def samples(self):
        rows = []
        for line in self.path.read_text().splitlines():
            parts = line.split()
            if len(parts) == 2 and all(p.isdigit() for p in parts):
                start, end = (int(p) / 1e9 for p in parts)
                rows.append((start, (end - start) * 1000))
        return rows


def stall_windows(samples, started, duration, limit=CANARY_STALL_MS):
    """Indexes of the phase's 10 s windows that saw a canary write over `limit` ms."""
    windows = set()
    for start, latency in samples:
        if latency > limit:
            # A write that began before the window can still stall into it.
            for edge in (start, start + latency / 1000):
                offset = edge - started
                if 0 <= offset < duration:
                    windows.add(int(offset // WINDOW))
    return windows


def canary_summary(samples, started, duration):
    inside = [lat for start, lat in samples if started <= start < started + duration]
    stalls = stall_windows(samples, started, duration)
    return {'samples': len(inside), 'stall_ms': CANARY_STALL_MS,
            'over_stall': sum(lat > CANARY_STALL_MS for lat in inside),
            'max_ms': max(inside, default=None), 'stall_windows': sorted(stalls)}


def without_windows(requests, duration, excluded):
    """Requests whose intended start falls outside the excluded windows, and
    the observed time that remains."""
    kept = [r for r in requests if int(r['intended'] // WINDOW) not in excluded]
    removed = sum(min(WINDOW, duration - i * WINDOW) for i in excluded if i * WINDOW < duration)
    return kept, max(duration - removed, 1e-9)


def stall_adjusted(baseline, phase, base_requests, phase_requests, samples):
    """D24 with the windows of host IO stalls removed from both sides.

    Informational: the recorded verdict stays the unadjusted one.
    """
    base_ex = stall_windows(samples, baseline['started_at_unix'], baseline['duration_seconds'])
    phase_ex = stall_windows(samples, phase['started_at_unix'], phase['duration_seconds'])
    base_kept, base_seconds = without_windows(base_requests, baseline['duration_seconds'], base_ex)
    phase_kept, phase_seconds = without_windows(phase_requests, phase['duration_seconds'], phase_ex)
    base_summary = summarize(base_kept, base_seconds)
    phase_summary = summarize(phase_kept, phase_seconds)
    # Excluded windows also remove their completions; rates compare per kept second.
    for summary, kept, seconds, total in ((base_summary, base_kept, base_seconds, baseline['duration_seconds']),
                                          (phase_summary, phase_kept, phase_seconds, phase['duration_seconds'])):
        summary['achieved_rate'] = sum(r['error'] is None and r['end'] < total for r in kept) / seconds
    result = verdict(base_summary, phase_summary)
    result.update(excluded_baseline_windows=len(base_ex), excluded_phase_windows=len(phase_ex))
    return result


def diagnose_phase(harness, name, edge):
    """Reset statement statistics before a phase and keep them after it."""
    run = harness.run
    if not getattr(run.args, 'diagnose', False) or run.engine != 'postgres':
        return
    if edge == 'start':
        run.sql('SELECT pg_stat_statements_reset();', admin=True)
    _, counters = run.sql(COUNTERS_SQL, admin=True)
    record = {'phase': name, 'edge': edge, 'unix': time.time(), 'counters': counters.strip()}
    if edge == 'end':
        _, statements = run.sql(STATEMENTS_SQL, admin=True)
        record['statements'] = [line.split('|') for line in statements.splitlines() if line.strip()]
    with (run.path / 'diagnose-phases.jsonl').open('a') as handle:
        handle.write(json.dumps(record) + '\n')
TARGET_DATA_MOUNT = 'dst=/var/lib/postgresql/data'


def target_on_tmpfs(command):
    """Give the agent DB its own storage instead of the source's shared disk.

    On one Docker host the source and the target share a virtual disk, so the
    target's write bursts (load staging, WAL, checkpoints) stall the source's
    commit fsyncs; no real deployment shares the source database's storage.
    """
    index = next(i for i, arg in enumerate(command)
                 if arg == '--mount' and TARGET_DATA_MOUNT in command[i + 1])
    return command[:index] + ['--tmpfs', '/var/lib/postgresql/data:rw,size=24g'] + command[index + 2:]


class ImpactHarness(t3t4.Harness):
    def c1_binding(self, sid, tables):
        if self.run.engine != 'mysql':
            return super().c1_binding(sid, tables)
        # Quiescent fixture: foreground writes start only after preparation.
        self.snapshot(sid, tables)
        _, epoch = self.src('SELECT @@GLOBAL.server_uuid;')
        _, position = self.src('SHOW MASTER STATUS;' if self.run.version in ('5.7', '8.0')
                               else 'SHOW BINARY LOG STATUS;')
        fields = position.split()
        self.sync[sid] = {
            'initial_position': {'kind': 'mysql_file_pos', 'file': fields[0],
                                 'position': int(fields[1])},
            'source_epoch': epoch.strip(), 'baseline_aligned': True,
            'max_batches': 100, 'server_id': next(self.server_ids),
            'longest_cycle_seconds': 1, 'recovery_seconds': 1,
        }


def oracle_timeout(rows, rows_per_second):
    if rows <= 0 or not math.isfinite(rows_per_second) or rows_per_second <= 0:
        raise ValueError('unmeasured_oracle_load_rate')
    return max(900, math.ceil(3 * rows / rows_per_second))


class ImpactOracleHarness(OracleHarness):
    load_timeout = 900

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.load_rate = None
        self.timeout_records = []

    def call(self, sid, run_id, mode=None, policy=None, required=True, timeout=None):
        if timeout is None and mode is None and self.load_rate is not None:
            self.size_timeout(sid)
        return super().call(sid, run_id, mode, policy, required,
                            self.load_timeout if timeout is None else timeout)

    def size_timeout(self, sid):
        tables = self.tables(sid)
        _, count = self.src("SELECT " + "+".join(
            f"(SELECT COUNT(*) FROM {table})" for table in tables) + " FROM dual;")
        rows = int(count.strip())
        self.load_timeout = oracle_timeout(rows, self.load_rate)
        self.timeout_records.append({
            'source': sid, 'tables': tables, 'rows': rows, 'rows_per_second': self.load_rate,
            'factor': 3, 'minimum_seconds': 900, 'timeout_seconds': self.load_timeout})

    def measure_load_rate(self):
        self.src('CREATE TABLE t10_probe AS SELECT * FROM bulk WHERE id<=20000;'
                 'ALTER TABLE t10_probe ADD PRIMARY KEY(id);'
                 'GRANT SELECT,FLASHBACK ON t10_probe TO fixture_reader;')
        _, count = self.src('SELECT COUNT(*) FROM t10_probe;')
        rows = int(count.strip())
        started = time.monotonic()
        self.snapshot('t10_rate_probe', ['t10_probe'])
        elapsed = time.monotonic() - started
        rate = rows / elapsed
        self.load_rate = rate
        _, total = self.src('SELECT (SELECT COUNT(*) FROM bulk)+'
                            '(SELECT COUNT(*) FROM items)+(SELECT COUNT(*) FROM other) FROM dual;')
        total = int(total.strip())
        self.load_timeout = oracle_timeout(total, rate)
        return {'probe_rows': rows, 'probe_seconds': elapsed, 'rows_per_second': rate,
                'rows': total, 'factor': 3, 'minimum_seconds': 900,
                'timeout_seconds': self.load_timeout}


class Workload:
    """Persistent fixture-writer connections; separate from product read-only roles."""
    def __init__(self, run):
        self.run = run
        self.local = threading.local()
        self.connections = []
        self.lock = threading.Lock()
        self.ready = queue.SimpleQueue()

    def warm(self, count):
        """Open the workers' connections before the phase clock starts.

        Lazily connecting inside the first measured second (64 SCRAM/TLS
        handshakes) inflated every phase's tail by its first second; a short
        phase weighted that warm-up far more than the 10-minute baseline.
        """
        for _ in range(count):
            self.ready.put(self.connect())

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
            try:
                self.local.conn = self.ready.get_nowait()
            except queue.Empty:
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


# Foreground client connections. Oracle Free's listener refuses new sessions
# (ORA-12516) well below 64 under load, so Oracle runs with 32.
WORKERS = {'oracle': 32}
DEFAULT_WORKERS = 64
# Latency is measured from the intended start, so queueing already shows in
# p95/p99. Only a backlog beyond this many seconds counts as overload.
BACKLOG_SECONDS = 30


def open_loop(work, rate, seconds, finished=None, clock=time.monotonic,
              sleep=time.sleep, workers=None, on_start=lambda: None):
    """Bounded queue: overloaded arrivals are errors, never silently dropped.

    A completion-driven phase uses finished instead of seconds. The scheduler
    never waits for an earlier request; end-to-end latency starts at intended.
    """
    workers = workers or DEFAULT_WORKERS
    jobs = queue.Queue(maxsize=max(workers * 32, int(rate * BACKLOG_SECONDS)))
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
                code = getattr(exc.args[0], 'full_code', None) if exc.args else None
                if isinstance(code, str) and re.fullmatch(r'(ORA|DPY|DPI)-[0-9]{4,5}', code):
                    error += ':' + code
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
    for concurrency in [c for c in levels if c <= DEFAULT_WORKERS * 2]:
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

class SettleFailure(RuntimeError):
    def __init__(self, trials):
        super().__init__('calibration_no_sustainable_rate')
        self.trials = trials

def settle(work, rate, seconds=60, step=.7, tries=6, loop=None, close=lambda: None):
    """Burst capacity overstates a sustainable open-loop rate (checkpoints,
    purge and tail stalls). Step down until a sustained trial is error-free."""
    loop = loop or open_loop
    trials = []
    for _ in range(tries):
        try:
            requests, _ = loop(work, rate, seconds)
        finally:
            # Each open-loop trial owns new worker threads and connections.
            close()
        errors = sum(r['error'] is not None for r in requests)
        trials.append({'rate': rate, 'seconds': seconds, 'requests': len(requests), 'errors': errors,
                       'error_counts': dict(Counter(r['error'] for r in requests if r['error']))})
        if errors == 0:
            return {'rate': rate, 'trials': trials}
        rate *= step
    raise SettleFailure(trials)

def foreground_rate(calibration):
    """Keep the settled trial intact; offer half that rate in every phase."""
    settled = calibration['settle']['rate']
    calibration['settled_rate'] = settled
    calibration['load_fraction'] = .5
    calibration['used_rate'] = settled * calibration['load_fraction']
    return calibration['used_rate']


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
    work.warm(DEFAULT_WORKERS)
    diagnose_phase(harness, name, 'start')
    started_at = time.time()
    meter.thread.start()
    try:
        requests, duration = open_loop(work, rate, seconds,
                                       finished=done if seconds is None else None,
                                       on_start=thread.start)
    finally:
        thread.join()
        metrics = meter.finish()
        work.close()
    diagnose_phase(harness, name, 'end')
    result = dict(summarize(requests, duration), phase=name, configured_rate=rate,
                  started_at_unix=started_at,
                  observation_seconds=time.monotonic() - started,
                  windows=windows(requests, duration), metrics=metrics,
                  product_errors=product_errors)
    if name == 'baseline' and requests:
        result['self_noise'] = baseline_noise(requests, duration, result)
    runner.write_json(harness.run.path / f'{name}-requests.json', requests)
    runner.write_json(harness.run.path / f'{name}.json', result)
    return result


def loader(h, sid, label, mode=None, policy=None, duty100=False):
    argv = h.argv(sid, label, mode, policy)
    if duty100:
        argv += ['--duty-cycle-percent', '100']
    _, out = h.run.command(argv, timeout=getattr(h, 'load_timeout', 86400))
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

def prepare_phase(h, name, rate, action):
    started = time.monotonic()
    try:
        action()
    except Exception as exc:
        result = dict(summarize([], 0), phase=name, configured_rate=rate,
                      observation_seconds=time.monotonic() - started, windows=[],
                      metrics={'samples': [], 'errors': [], 'peaks': {}, 'changes': {}},
                      product_errors=[type(exc).__name__], preparation_failed=True)
        runner.write_json(h.run.path / f'{name}.json', result)
        return result
    return None

def c5_cycles(run_cycle, period, seconds, clock=time.monotonic):
    """Consecutive cycles of one binding until `seconds` have passed.

    A single 20-60 s cycle against a 10-minute baseline let one host IO
    stall decide the verdict; whole cycles keep the cycle mix intact.
    """
    started = clock()
    policies = []
    while True:
        policy = {'cycle': len(policies) + 1, 'full_read_every': period}
        run_cycle(policy)
        policies.append(policy)
        if clock() - started >= seconds:
            return policies


def c5_phases(h, rate, every, sequence, seconds=0):
    results = {}
    for variant, period in (('c5', every), ('c5_full', 1)):
        for suffix, tables in (('idle', ['bulk']), ('changed', ['bulk', 'items', 'other'])):
            name = f'{variant}_{suffix}'
            binding = 't10_' + name
            # Independent baselines prevent the first policy priming the second.
            def prepare_binding():
                h.snapshot(binding, tables)
                if suffix == 'changed':
                    h.src('UPDATE items SET n=n+1 WHERE id=1;')
            policies = []

            def cycles():
                policies.extend(c5_cycles(
                    lambda policy: loader(h, binding, f"{binding}-cycle-{policy['cycle']}",
                                          '--c5', policy), period, seconds))
            result = prepare_phase(h, name, rate, prepare_binding)
            if result is None:
                result = phase(h, name, Workload(h.run), rate, None, cycles,
                               sequence + len(results) * 1_000_000)
            result['c5_policy'] = {'cycle': 1, 'full_read_every': period}
            result['c5_cycles'] = len(policies)
            results[name] = result
    return results


def parse_args(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--engine', required=True, choices=('postgres', 'mysql', 'oracle'))
    parser.add_argument('--version', required=True)
    parser.add_argument('--phases', default=','.join(PHASES))
    parser.add_argument('--minutes', type=float, default=10)
    parser.add_argument('--rate', default='auto')
    parser.add_argument('--c5-minutes', type=float, default=None,
                        help='repeat whole C5 cycles of each variant for at least this long '
                             '(default: half of --minutes; 0 runs one cycle)')
    parser.add_argument('--cpus', type=float, default=2)
    parser.add_argument('--memory', default='4g')
    parser.add_argument('--evidence', type=Path, required=True)
    parser.add_argument('--binary', type=Path, default=runner.ROOT / 'tools/oo-load/target/release/oo-load')
    parser.add_argument('--target-tmpfs', action='store_true',
                        help='keep the agent DB on tmpfs, off the disk the source shares')
    parser.add_argument('--diagnose', action='store_true',
                        help='PostgreSQL only: pg_stat_statements, IO timing, checkpoint/autovacuum '
                             'logs and a 250 ms activity sampler; never a D24 measurement')
    parser.add_argument('--quiet-cpu-percent', type=float, default=QUIET_CPU_PERCENT,
                        help='refuse while pre-existing containers use more CPU in total '
                             '(percent of one CPU); the limit and the observed load are recorded')
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
    if args.c5_minutes is None:
        args.c5_minutes = args.minutes / 2
    if not math.isfinite(args.c5_minutes) or args.c5_minutes < 0:
        parser.error('c5-minutes must be finite and not negative')
    if not math.isfinite(args.quiet_cpu_percent) or not 0 <= args.quiet_cpu_percent <= 100:
        parser.error('quiet-cpu-percent must be between 0 and 100')
    if args.diagnose and args.engine != 'postgres':
        parser.error('--diagnose is implemented for postgres only')
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


def quiet_host(samples=3, limit=QUIET_CPU_PERCENT):
    """Pre-existing containers are never stopped; they must be measured idle.

    Returns their count, peak CPU % and the applied limit for the evidence
    report, never names.
    """
    peaks = {}
    for _ in range(samples):
        text = subprocess.run(['docker', 'stats', '--no-stream', '--format', '{{.Name}}\t{{.CPUPerc}}'],
                              capture_output=True, text=True, check=True).stdout
        for line in text.splitlines():
            name, _, cpu = line.partition('\t')
            value = float(cpu.strip().rstrip('%') or 0)
            peaks[name] = max(peaks.get(name, 0.0), value)
    total = sum(peaks.values())
    if total > limit:
        raise SystemExit('blocked: pre-existing containers are not idle; nothing stopped')
    # Names of unrelated containers can be private; record only count and load.
    result = {'count': len(peaks), 'peak_cpu_percent': max(peaks.values(), default=0.0)}
    if limit != QUIET_CPU_PERCENT:
        # A raised limit is a recorded deviation, never a silent bypass.
        result.update(total_peak_cpu_percent=total, limit_cpu_percent=limit)
    return result


def main(argv=None):
    args = parse_args(argv)
    # Preflight before creating resources. The unpatched loader must not silently
    # label its default 25% duty run as the requested 100% tradeoff.
    binary = args.binary.resolve()
    help_text = subprocess.run([str(binary), '--help'], capture_output=True, text=True, check=True).stdout
    if 'initial_duty100' in args.phases and '--duty-cycle-percent' not in help_text:
        raise SystemExit('blocked: oo-load needs the coordinator duty-cycle patch in T10-VALIDATION.md')
    importlib.import_module({'postgres': 'psycopg', 'mysql': 'pymysql', 'oracle': 'oracledb'}[args.engine])
    preexisting = quiet_host(limit=args.quiet_cpu_percent)
    global DEFAULT_WORKERS
    DEFAULT_WORKERS = WORKERS.get(args.engine, DEFAULT_WORKERS)
    t1.LANE = 't10'
    args.evidence.mkdir(parents=True, mode=0o700, exist_ok=False)
    args.platform, args.health_timeout, args.scale, args.tables, args.worker_image = None, 600, SCALE, 0, None
    run = ImpactSource(args.evidence, args.engine, args.version, args)
    run.receipt = {}
    h = (ImpactOracleHarness if args.engine == 'oracle' else ImpactHarness)(run, binary)
    report = {'engine': args.engine, 'version': args.version, 'phases': {}, 'errors': [],
              'preexisting_containers': preexisting,
              'cpus': args.cpus, 'memory': args.memory, 'scale': SCALE,
              'target_storage': 'tmpfs' if args.target_tmpfs else 'volume_on_shared_docker_disk',
              'product_sha': product_sha(), 'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
              'measurement_notes': ['client-observed latency includes queueing',
                                    'foreground connections open before each phase clock; '
                                    'connection setup is not a measured request',
                                    'Oracle undo counters cover the V$UNDOSTAT retention horizon',
                                    'LogMiner CPU is a sampled live-session counter, not total exited-session CPU',
                                    'C5 idle scans bulk only; foreground writes items/other outside that scope']}
    cleanup = []
    activity = None
    canary = None
    if args.diagnose:
        report['diagnose'] = {'postgres_settings': DIAGNOSE_POSTGRES, 'activity_interval_seconds': .25,
                              'note': 'diagnostic run; not a D24 measurement'}
    try:
        prepare(h)
        canary = IoCanary(run)
        canary.start()
        if args.diagnose and args.engine == 'postgres':
            run.sql('CREATE EXTENSION IF NOT EXISTS pg_stat_statements;', admin=True)
            activity = Activity(run)
            activity.start()
        if args.engine == 'oracle':
            report['load_timeout_calibration'] = h.measure_load_rate()
            report['load_timeout_sizing'] = h.timeout_records
        elif args.engine == 'postgres':
            _, setting = h.src('SHOW max_slot_wal_keep_size;')
            report['source_settings'] = {'max_slot_wal_keep_size': setting.strip()}
            if setting.strip() != SLOT_WAL_KEEP_SIZE:
                raise RuntimeError('slot_retention_setting_not_applied')
        elif args.engine == 'mysql':
            names = list(MYSQL_SOURCE_SETTINGS) + ['innodb_flush_log_at_trx_commit', 'sync_binlog']
            _, text = h.src('SELECT ' + ','.join(f'@@GLOBAL.{n}' for n in names) + ';')
            applied = dict(zip(names, (int(v) for v in text.split())))
            report['source_settings'] = applied
            if any(applied[k] != v for k, v in MYSQL_SOURCE_SETTINGS.items()):
                raise RuntimeError('mysql_source_settings_not_applied')
        run.record_version()
        report['resource_receipt'] = run.receipt
        _, limits = run.command(['docker', 'inspect', '--format', '{{json .HostConfig}}', run.name])
        limits = json.loads(limits)
        report['source_limits'] = {k: limits[k] for k in ('NanoCpus', 'Memory')}
        work = Workload(run)
        try:
            if args.rate == 'auto':
                report['calibration'] = calibrate(work, close=work.close)
                try:
                    report['calibration']['settle'] = settle(
                        work, report['calibration']['rate'], close=work.close)
                except SettleFailure as exc:
                    report['calibration']['settle'] = {
                        'rate': None, 'trials': exc.trials, 'error': str(exc)}
                    raise
                rate = foreground_rate(report['calibration'])
            else:
                rate = args.rate
                report['calibration'] = {'skipped': 'explicit rate', 'rate': rate,
                                         'capacity': None, 'settled_rate': None,
                                         'load_fraction': None, 'used_rate': rate}
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
                if args.engine == 'oracle':
                    h.size_timeout(sid)
                action = lambda: loader(h, sid, sid, duty100=name == 'initial_duty100')
            elif name == 'c1':
                # Preparation failure must not hide independent C5 measurements.
                failed = prepare_phase(h, name, rate, lambda: h.c1_binding(sid, tables))
                if failed is not None:
                    report['phases'][name] = failed
                    continue

                def action():
                    deadline = time.monotonic() + seconds
                    while time.monotonic() < deadline:
                        loader(h, sid, f'{sid}-{h.calls}', '--sync', h.sync[sid])
            else:
                # Separate scope makes the idle cycle genuinely unchanged while
                # preserving the identical foreground workload in both subphases.
                report['phases'].update(c5_phases(h, rate, every, 10_000_000 * (number + 1),
                                                  args.c5_minutes * 60))
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
        if activity:
            activity.stop()
        if canary:
            canary.stop()
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
        samples = canary.samples() if canary else []
        base_requests = None
        for name, result in report['phases'].items():
            if samples and result.get('started_at_unix') is not None:
                result['host_io'] = canary_summary(samples, result['started_at_unix'],
                                                   result['duration_seconds'])
                if (baseline and name != 'baseline' and baseline.get('started_at_unix')
                        and result['requests']):
                    try:
                        if base_requests is None:
                            base_requests = json.loads((run.path / 'baseline-requests.json').read_text())
                        phase_requests = json.loads((run.path / f'{name}-requests.json').read_text())
                        result['budget_without_host_io_stalls'] = stall_adjusted(
                            baseline, result, base_requests, phase_requests, samples)
                        result['baseline_noise_same_length'] = same_length_noise(
                            base_requests, baseline, result['duration_seconds'])
                    except (OSError, ValueError) as exc:
                        result['budget_without_host_io_stalls'] = {'error': type(exc).__name__}
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
