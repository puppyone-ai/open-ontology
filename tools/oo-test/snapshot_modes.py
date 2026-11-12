#!/usr/bin/env python3
"""Owned-fixture single-snapshot and immutable replay provenance regression."""
import argparse
import copy
import shutil
import threading
from pathlib import Path

import runner
import t3t4


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--engine', choices=('postgres', 'mysql'), required=True)
    parser.add_argument('--version', required=True)
    parser.add_argument('--run-id', required=True)
    parser.add_argument('--binary', type=Path, required=True)
    args = parser.parse_args()
    if not args.run_id or not all(c.isalnum() or c in '-_' for c in args.run_id):
        parser.error('invalid run ID')
    path = runner.ROOT / 'evidence' / args.run_id
    path.mkdir(mode=0o700, exist_ok=False)
    run = t3t4.Source(path, args.engine, args.version, argparse.Namespace(
        platform=None, health_timeout=600, scale=1, tables=0, worker_image=None))
    h = t3t4.Harness(run, args.binary.resolve())
    stop, writer = threading.Event(), None
    errors, writes = [], []
    report = {'status': 'failed', 'engine': args.engine, 'version': args.version}
    try:
        h.prepare()
        h.snapshot('chunk', ['items', 'other'])
        source = next(s for s in h.stack['sources'] if s['id'] == 'chunk')
        source['snapshot'] = 'single'
        _, replay, _ = h.call('chunk', 'chunk-base')
        assert len(replay) == 2 and all(r['replay'] and r['consistency'] == 'chunk_consistent' for r in replay)
        h.add_source('single', ['items', 'other'])
        source = next(s for s in h.stack['sources'] if s['id'] == 'single')
        source['snapshot'] = 'single'
        writer_run = copy.copy(run)
        writer_run.seq = 8000000
        def mutate():
            while not stop.is_set():
                try:
                    writer_run.sql('BEGIN; UPDATE items SET n=n+1; UPDATE other SET n=n+1; COMMIT;', admin=True)
                    writes.append(1)
                except Exception as error:
                    errors.append(type(error).__name__)
                    return
                stop.wait(.05)
        writer = threading.Thread(target=mutate)
        writer.start()
        _, fresh, _ = h.call('single', 'single-base')
        stop.set()
        writer.join(timeout=60)
        assert not writer.is_alive() and not errors and writes
        assert len(fresh) == 2 and all(not r['replay'] and r['consistency'] == 'single' for r in fresh)
        h.remember('single', fresh)
        left, right = (h.relations[('single', table)] for table in ('items', 'other'))
        _, counts = h.tgt(f'SELECT count(*),count(*) FILTER(WHERE a.n<>b.n) FROM {left} a JOIN {right} b USING(id);')
        assert counts.strip().split() == ['20', '0']
        source['snapshot'] = 'chunk_consistent'
        _, replay, _ = h.call('single', 'single-base')
        assert len(replay) == 2 and all(r['replay'] and r['consistency'] == 'single' for r in replay)
        report.update(status='passed', writer_commits=len(writes), cross_table_equal_rows=20,
                      both_replay_mode_flips_preserved=True)
    finally:
        stop.set()
        if writer:
            writer.join(timeout=60)
        try:
            h.target.close()
        finally:
            report['cleanup_failures'] = run.cleanup()
            shutil.rmtree(h.store, ignore_errors=True)
        runner.write_json(path / 'result.json', report)
        runner.checksums(path)
    if report['status'] != 'passed' or report['cleanup_failures']:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
