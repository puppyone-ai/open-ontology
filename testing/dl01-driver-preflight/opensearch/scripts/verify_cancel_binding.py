#!/usr/bin/env python3
"""Verify actual server-log bytes against the native owned-task observations."""
import argparse
import datetime as dt
import json
from pathlib import Path


def verify(doc, raw):
    failures = []
    b = doc.get('server_log_binding', {})
    before, after = doc.get('task_before', {}), doc.get('task_after', {})
    task = before.get('task_id', '')
    line = b.get('line', '')
    trigger = doc.get('user_triggered_unix_ms', 0)
    cancelled = after.get('cancellation_time_millis', 0)
    if line not in raw.splitlines():
        failures.append('binding line absent from captured raw server log')
    try:
        stamp = dt.datetime.fromisoformat(line.split()[0].replace('Z', '+00:00'))
        stamp_ms = int(stamp.timestamp() * 1000)
    except (ValueError, IndexError):
        stamp_ms = 0
    # The cancel flag must follow the trigger within 5s. OpenSearch may write the
    # QueryPhase line later; 15s is the observed bound, and expired lines are rejected below.
    if not trigger <= cancelled <= trigger + 5000 or not cancelled <= stamp_ms <= cancelled + 15000:
        failures.append('server reason outside observed cancellation window')
    node = task.split(':')[0]
    index, marker = doc.get('workload_index', ''), doc.get('marker', '')
    if not task or not index or f'[{node}][{index}][' not in line:
        failures.append('server reason has wrong node/index')
    if not marker or marker not in before.get('description', '') or marker not in after.get('description', ''):
        failures.append('marker missing from observed same task')
    if task != after.get('task_id') or task != b.get('task_id') or b.get('index') != index or b.get('marker') != marker:
        failures.append('task identity changed')
    if before.get('cancelled') is not False or after.get('cancelled') is not True:
        failures.append('native cancellation transition missing')
    if doc.get('task_id_present_after') is not False or doc.get('task_list_after_status') != 200:
        failures.append('remote task not observed gone')
    if doc.get('other_search_task_ids_before') != []:
        failures.append('ambiguous concurrent business search')
    if 'TaskCancelledException[cancelled task with reason: channel closed]' not in line or 'expired' in line.lower():
        failures.append('native reason is not explicit channel cancellation')
    if doc.get('natural_completion_before_trigger') is not False or doc.get('cancel_after_time_interval_sent') is not False or doc.get('dba_cancel_used') is not False:
        failures.append('late/timer/admin cancellation substituted')
    if b.get('trigger_unix_ms') != trigger or b.get('cancelled_unix_ms') != cancelled:
        failures.append('binding timestamps differ from native observations')
    return failures

if __name__ == '__main__':
    ap = argparse.ArgumentParser()
    ap.add_argument('--cancel', required=True)
    ap.add_argument('--log', required=True)
    ap.add_argument('--out', required=True)
    a = ap.parse_args()
    errors = verify(json.loads(Path(a.cancel).read_text()), Path(a.log).read_text())
    Path(a.out).write_text(json.dumps({'passed': not errors, 'failures': errors, 'independent_approval': False}, indent=2) + '\n')
    raise SystemExit(bool(errors))
