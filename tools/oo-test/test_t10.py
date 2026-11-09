"""Offline T10 checks. No Docker, native driver, or customer connection is used."""
import argparse
from contextlib import redirect_stderr
import io
import json
from pathlib import Path
import tempfile
import threading
import time
import unittest
from unittest.mock import Mock, patch

import t10


class MathTests(unittest.TestCase):
    def test_percentiles_interpolate(self):
        self.assertEqual(t10.percentile([40, 10, 30, 20], 50), 25)
        self.assertAlmostEqual(t10.percentile([10, 20, 30, 40], 95), 38.5)
        self.assertAlmostEqual(t10.percentile([10, 20, 30, 40], 99), 39.7)

    def test_percentile_empty_singleton(self):
        self.assertIsNone(t10.percentile([], 95))
        self.assertEqual(t10.percentile([7], 99), 7)

    def test_queue_delay_is_latency(self):
        result = t10.summarize([dict(intended=0, actual=2, end=3, error=None)], 4)
        self.assertEqual(result['p95_ms'], 3000)
        self.assertEqual(result['max_start_delay_ms'], 2000)

    def test_error_classes_remain_distinct(self):
        rows = [dict(intended=0, actual=0, end=1, error=error)
                for error in (None, 'scheduler_queue_full', 'DatabaseError', 'scheduler_queue_full')]
        result = t10.summarize(rows, 2)
        self.assertEqual(result['errors'], 3)
        self.assertEqual(result['error_counts'], {'scheduler_queue_full': 2, 'DatabaseError': 1})

    def test_late_completion_not_counted_as_on_time_throughput(self):
        result = t10.summarize([dict(intended=0, actual=2, end=3, error=None)], 2)
        self.assertEqual(result['achieved_rate'], 0)
        self.assertEqual(result['requests'], 1)

    def test_partial_window_and_completion_window(self):
        rows = [dict(intended=9, actual=9, end=11, error=None),
                dict(intended=10, actual=10, end=12, error=None),
                dict(intended=11, actual=None, end=11, error='queue_full')]
        windows = t10.windows(rows, 15)
        self.assertEqual([w['requests'] for w in windows], [1, 2])
        self.assertEqual([w['duration_seconds'] for w in windows], [10, 5])
        self.assertEqual([w['achieved_rate'] for w in windows], [0, .4])
        self.assertEqual(windows[1]['errors'], 1)

    def test_zero_duration(self):
        self.assertEqual(t10.windows([], 0), [])
        self.assertIsNone(t10.summarize([], 0)['p99_ms'])


class VerdictTests(unittest.TestCase):
    def setUp(self):
        self.base = dict(requests=100, errors=0, achieved_rate=100, p95_ms=10, p99_ms=20)

    def test_inclusive_boundaries(self):
        phase = dict(self.base, p95_ms=12, p99_ms=30, achieved_rate=95)
        self.assertEqual(t10.verdict(self.base, phase)['result'], 'pass')

    def test_each_budget_failure(self):
        for field, value in [('p95_ms', 12.01), ('p99_ms', 30.01), ('achieved_rate', 94.99), ('errors', 1)]:
            with self.subTest(field=field):
                self.assertEqual(t10.verdict(self.base, dict(self.base, **{field: value}))['result'], 'fail')

    def test_invalid_baselines_cannot_pass(self):
        for base in (None, dict(self.base, errors=1), dict(self.base, achieved_rate=0),
                     dict(self.base, p95_ms=None), dict(self.base, requests=0)):
            self.assertEqual(t10.verdict(base, self.base)['result'], 'fail')

    def test_empty_phase_cannot_pass(self):
        self.assertEqual(t10.verdict(self.base, dict(self.base, requests=0))['result'], 'fail')


class ParserTests(unittest.TestCase):
    # Fixed stdout-format samples, not claims of a T10 live measurement.
    def test_docker_sample(self):
        sample = '{"CPUPerc":"125.50%","BlockIO":"1.5MB / 2.0GiB"}'
        self.assertEqual(t10.docker_metrics(sample), dict(cpu_percent=125.5,
                         block_read_bytes=1500000, block_write_bytes=2 * 1024**3))

    def test_postgres_sample(self):
        self.assertEqual(t10.sql_metrics('postgres', ' 120|43|2|1048576\n'),
                         dict(backend_xmin_age=120, n_dead_tup=43, autovacuum_count=2,
                              slot_retained_wal_bytes=1048576))

    def test_mysql_sample(self):
        self.assertEqual(t10.sql_metrics('mysql', 'Threads_running\t4\nInnodb_history_list_length\t83\n'),
                         dict(Threads_running=4, Innodb_history_list_length=83, purge_backlog_transactions=83))

    def test_oracle_sample(self):
        self.assertEqual(t10.sql_metrics('oracle', '\n 2048|900|3\n\n'),
                         dict(undo_blocks=2048, tuned_undoretention_seconds=900, ora_01555_count=3))

    def test_missing_invalid_metrics_fail_closed(self):
        for engine, sample in [('postgres', ''), ('postgres', '1|2|3'),
                               ('postgres', '1|2|3|NaN'), ('oracle', 'ORA-00942'),
                               ('mysql', 'Threads_running\t2')]:
            with self.subTest(engine=engine, sample=sample), self.assertRaises((ValueError, KeyError)):
                t10.sql_metrics(engine, sample)

    def test_bad_block_io(self):
        with self.assertRaises(ValueError):
            t10.byte_size('unknown')
        with self.assertRaises(ValueError):
            t10.docker_metrics('{"CPUPerc":"NaN%","BlockIO":"0B / 0B"}')


class SchedulerTests(unittest.TestCase):
    def test_arrivals_do_not_wait_for_previous_completion(self):
        def slow(_):
            time.sleep(.025)
        rows, duration = t10.open_loop(slow, 200, .05, workers=1)
        rows.sort(key=lambda r: r['intended'])
        self.assertEqual(len(rows), 10)
        self.assertAlmostEqual(rows[-1]['intended'], .045)
        self.assertGreater(rows[-1]['actual'] - rows[-1]['intended'], .1)
        self.assertGreater(rows[-1]['end'], duration)

    def test_fixed_schedule_fake_clock(self):
        now = [0.0]
        def sleep(delta):
            now[0] += delta
        rows, duration = t10.open_loop(lambda _: None, 2, 2, clock=lambda: now[0], sleep=sleep, workers=1)
        self.assertEqual(sorted(r['intended'] for r in rows), [0, .5, 1, 1.5])
        self.assertEqual(duration, 2)

    def test_errors_retained_without_driver_message(self):
        def bad(_):
            raise ValueError('sensitive driver details')
        rows, _ = t10.open_loop(bad, 100, .02, workers=1)
        self.assertEqual(len(rows), 2)
        self.assertTrue(all(r['error'] == 'ValueError' for r in rows))
        self.assertNotIn('sensitive', json.dumps(rows))

    def test_only_allowlisted_oracle_codes_survive_error_sanitizing(self):
        class DriverError:
            full_code = 'ORA-00020'
            def __str__(self):
                return 'sensitive driver details'
        def bad(_):
            raise RuntimeError(DriverError())
        rows, _ = t10.open_loop(bad, 100, .02, workers=1)
        self.assertTrue(all(r['error'] == 'RuntimeError:ORA-00020' for r in rows))
        self.assertNotIn('sensitive', json.dumps(rows))
        DriverError.full_code = 'ORA-00020 sensitive details'
        rows, _ = t10.open_loop(bad, 100, .02, workers=1)
        self.assertTrue(all(r['error'] == 'RuntimeError' for r in rows))

    def test_completion_driven_phase(self):
        done = threading.Event()
        rows, _ = t10.open_loop(lambda _: done.set(), 100, None, finished=done, workers=1)
        self.assertGreaterEqual(len(rows), 1)
        self.assertLess(len(rows), 10)

    def test_overload_is_explicit_not_dropped(self):
        now = [0.0]
        gate = threading.Event()
        def sleep(delta):
            now[0] += delta
        def slow(_):
            gate.wait(.05)
        with patch.object(t10, 'BACKLOG_SECONDS', 0.01):  # backlog beyond the bound
            rows, _ = t10.open_loop(slow, 1000, .1, clock=lambda: now[0], sleep=sleep, workers=1)
        self.assertEqual(len(rows), 100)
        self.assertTrue(any(r['error'] == 'scheduler_queue_full' for r in rows))

    def test_calibration_refuses_unproven_capacity(self):
        with self.assertRaisesRegex(RuntimeError, 'capacity_not_found'):
            t10.calibrate(lambda _: None, seconds=.001, levels=(1,))

    def test_calibration_errors_fail(self):
        with self.assertRaisesRegex(RuntimeError, 'calibration_errors'):
            t10.calibrate(lambda _: 1 / 0, seconds=.01, levels=(1,))

    def test_calibration_uses_one_sequence_despite_unequal_worker_speeds(self):
        seen = []
        def work(index):
            seen.append(index)
            time.sleep(.001 if index % 2 else .0001)
        with self.assertRaisesRegex(RuntimeError, 'capacity_not_found'):
            t10.calibrate(work, seconds=.03, levels=(4,))
        self.assertGreater(len(seen), 10)
        self.assertEqual(sorted(seen), list(range(len(seen))))

    def test_calibration_plateau_and_55_percent(self):
        with patch.object(t10, 'ThreadPoolExecutor') as executor, \
                patch.object(t10.time, 'monotonic', side_effect=[0, 0, 1, 2, 2, 3, 4, 4, 5]):
            executor.return_value.__enter__.return_value.map.side_effect = [
                [(100, 0)], [(200, 0)], [(205, 0)]]
            result = t10.calibrate(lambda _: None)
        self.assertEqual(result['capacity'], 205)
        self.assertAlmostEqual(result['rate'], 112.75)
        self.assertEqual(len(result['samples']), 3)


class CLITests(unittest.TestCase):
    def argv(self):
        return ['--engine', 'postgres', '--version', '17', '--evidence',
                str(t10.runner.ROOT / 'evidence' / 'unit-not-created')]

    def test_defaults(self):
        args = t10.parse_args(self.argv())
        self.assertEqual(args.phases, list(t10.PHASES))
        self.assertEqual(args.minutes, 10)
        self.assertEqual(args.rate, 'auto')

    def test_explicit_rate_and_engine_versions(self):
        for engine, version in [('mysql', '8.4'), ('oracle', '23'), ('postgres', '17')]:
            argv = self.argv() + ['--engine', engine, '--version', version, '--rate', '42']
            self.assertEqual(t10.parse_args(argv).rate, 42)

    def test_reject_invalid(self):
        for extra in [['--minutes', '0'], ['--minutes', 'nan'], ['--cpus', '-1'],
                      ['--rate', 'inf'], ['--rate', 'bad'], ['--memory', '0g'],
                      ['--phases', 'initial'], ['--phases', 'baseline,c1,c1'],
                      ['--phases', 'baseline,typo'], ['--version', 'bad'],
                      ['--engine', 'oracle', '--version', '11'], ['--evidence', '/tmp/outside']]:
            with self.subTest(extra=extra), redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                t10.parse_args(self.argv() + extra)

    def test_unpatched_loader_refused_before_docker(self):
        with patch.object(t10.subprocess, 'run', return_value=argparse.Namespace(stdout='oo-load --help')):
            with patch.object(t10, 'ImpactSource') as source:
                with self.assertRaisesRegex(SystemExit, 'duty-cycle'):
                    t10.main(self.argv())
                source.assert_not_called()

    def test_busy_host_refused_without_creating_resources(self):
        busy = argparse.Namespace(stdout='unrelated-container\t42.0%\n')
        with patch.object(t10.subprocess, 'run', side_effect=[
                argparse.Namespace(stdout='--duty-cycle-percent'), busy, busy, busy]) as run, \
                patch.object(t10.importlib, 'import_module'), \
                patch.object(t10, 'ImpactSource') as source:
            with self.assertRaisesRegex(SystemExit, 'not idle; nothing stopped'):
                t10.main(self.argv())
            source.assert_not_called()
            self.assertEqual(run.call_args.args[0][:3], ['docker', 'stats', '--no-stream'])

    def test_idle_preexisting_containers_are_recorded_not_stopped(self):
        idle = argparse.Namespace(stdout='customer-a\t0.00%\ncustomer-b\t0.01%\n')
        with patch.object(t10.subprocess, 'run', side_effect=[idle, idle, idle]) as run:
            seen = t10.quiet_host()
        self.assertEqual(seen, {'count': 2, 'peak_cpu_percent': 0.01})
        self.assertNotIn('customer', json.dumps(seen))
        self.assertTrue(all(call.args[0][:2] == ['docker', 'stats'] for call in run.call_args_list))


class BacklogTests(unittest.TestCase):
    def test_short_stall_is_latency_not_overload(self):
        clock = {'now': 0.0}
        def work(index):
            if index == 0:
                clock['now'] += 2.0  # one two-second stall
        requests, _ = t10.open_loop(work, 5000, 1, clock=lambda: clock['now'],
                                    sleep=lambda d: clock.__setitem__('now', clock['now'] + d), workers=1)
        self.assertFalse([r for r in requests if r['error'] == 'scheduler_queue_full'])
        self.assertGreaterEqual(max(r['latency_ms'] for r in requests), 1000)


class SettleTests(unittest.TestCase):
    def test_closes_connections_between_trials_and_after_success(self):
        active = []
        def loop(work, rate, seconds):
            self.assertEqual(active, [])
            active.extend(range(64))
            return [dict(error='scheduler_queue_full' if rate > 70 else None)], seconds
        result = t10.settle(None, 100, loop=loop, close=active.clear)
        self.assertEqual(result['rate'], 70)
        self.assertEqual(active, [])

    def test_failed_settle_preserves_sanitized_trials_and_closes_connections(self):
        close = Mock()
        loop = lambda work, rate, seconds: ([dict(error='OperationalError')], seconds)
        with self.assertRaises(t10.SettleFailure) as raised:
            t10.settle(None, 100, loop=loop, tries=2, close=close)
        self.assertEqual(close.call_count, 2)
        self.assertEqual([t['rate'] for t in raised.exception.trials], [100, 70])
        self.assertEqual(raised.exception.trials[0]['error_counts'], {'OperationalError': 1})

    def test_closes_connections_when_trial_raises(self):
        close = Mock()
        with self.assertRaises(ValueError):
            t10.settle(None, 100, loop=Mock(side_effect=ValueError), close=close)
        close.assert_called_once_with()

    def test_steps_down_until_a_sustained_trial_is_error_free(self):
        def loop(work, rate, seconds):
            error = 'scheduler_queue_full' if rate > 50 else None
            return [dict(error=error), dict(error=None)], seconds
        result = t10.settle(None, 100, loop=loop)
        self.assertAlmostEqual(result['rate'], 49.0)
        self.assertEqual([t['errors'] for t in result['trials']], [1, 1, 0])

    def test_refuses_when_no_rate_is_sustainable(self):
        loop = lambda work, rate, seconds: ([dict(error='x')], seconds)
        with self.assertRaisesRegex(RuntimeError, 'no_sustainable_rate'):
            t10.settle(None, 100, loop=loop, tries=2)


class IntegrationContractTests(unittest.TestCase):
    def test_invalid_baseline_cannot_start_product_comparison(self):
        result = dict(requests=100, errors=0, achieved_rate=100, p95_ms=10, p99_ms=20,
                      product_errors=[], metrics={'errors': [], 'samples': [{}]})
        self.assertTrue(t10.valid_baseline(result))
        for change in ({'errors': 1}, {'requests': 0}, {'product_errors': ['RuntimeError']},
                       {'metrics': {'errors': ['missing'], 'samples': [{}]}},
                       {'metrics': {'errors': [], 'samples': []}}):
            self.assertFalse(t10.valid_baseline(dict(result, **change)))

    def test_insert_keys_survive_calibration_and_phase_boundaries(self):
        from unittest.mock import MagicMock
        with tempfile.TemporaryDirectory(dir=t10.runner.ROOT) as tmp:
            run = t10.ImpactSource(Path(tmp), 'postgres', '17', argparse.Namespace())
            statements = []
            for _ in range(3):
                work = t10.Workload(run)
                work.local.conn = MagicMock()
                cursor = work.local.conn.cursor.return_value.__enter__.return_value
                for index in (9, 19):
                    work(index)
                    statements.append(cursor.execute.call_args.args[0])
            self.assertEqual(len(set(statements)), 6)
            for key, statement in enumerate(statements, t10.SCALE + 1):
                self.assertIn(f'VALUES({key},', statement)

    def test_c5_compares_both_policies_with_independent_baselines(self):
        from unittest.mock import Mock
        h = Mock()
        policies = []
        def measure(h, name, work, rate, seconds, action, sequence):
            action()
            return {'phase': name}
        with patch.object(t10, 'phase', side_effect=measure), \
                patch.object(t10, 'loader', side_effect=lambda *args: policies.append(args[-1])):
            results = t10.c5_phases(h, 42, 24, 10_000_000)
        self.assertEqual(tuple(results), t10.C5_PHASES)
        self.assertEqual(policies, [{'cycle': 1, 'full_read_every': p} for p in (24, 24, 1, 1)])
        self.assertEqual([r['c5_policy'] for r in results.values()], policies)
        self.assertEqual([call.args for call in h.snapshot.call_args_list], [
            ('t10_c5_idle', ['bulk']),
            ('t10_c5_changed', ['bulk', 'items', 'other']),
            ('t10_c5_full_idle', ['bulk']),
            ('t10_c5_full_changed', ['bulk', 'items', 'other'])])
        self.assertEqual(h.src.call_count, 2)

    def test_phase_retains_product_failure_and_closes_writer(self):
        from unittest.mock import Mock
        with tempfile.TemporaryDirectory(dir=t10.runner.ROOT) as tmp, patch.object(t10, 'Meter') as meter:
            h = Mock()
            h.run.path = Path(tmp)
            work = Mock()
            meter.return_value.finish.return_value = {'samples': [], 'errors': ['missing'], 'peaks': {}}
            def fail():
                raise RuntimeError('not logged')
            result = t10.phase(h, 'baseline', work, 100, .02, fail, 100000)
            self.assertEqual(result['product_errors'], ['RuntimeError'])
            self.assertEqual(result['metrics']['errors'], ['missing'])
            self.assertTrue((Path(tmp) / 'baseline-requests.json').exists())
            work.close.assert_called_once()

    def test_source_limits_and_lane_labels(self):
        with tempfile.TemporaryDirectory(dir=t10.runner.ROOT) as tmp:
            args = argparse.Namespace(cpus=2.5, memory='6g')
            run = t10.ImpactSource(Path(tmp), 'postgres', '17', args)
            with patch.object(t10.t1, 'LANE', 't10'), patch.object(t10.runner.Run, 'command', return_value=(0, '')) as call:
                run.command(['docker', 'create', '--name', run.name, '--label', 'oo.lane=h', 'image'])
                command = call.call_args.args[-1]
                self.assertIn('oo.lane=t10', command)
                self.assertEqual(command[command.index('--cpus')+1], '2.5')
                self.assertEqual(command[command.index('--memory')+1], '6g')
                self.assertIn('wal_level=logical', command)

    def test_loader_duty100_requires_receipt(self):
        from unittest.mock import Mock
        h = Mock()
        h.argv.return_value = ['oo-load']
        h.run.command.return_value = (0, '{"read_budget":{"duty_cycle_percent":25}}')
        with self.assertRaisesRegex(RuntimeError, 'duty100_not_confirmed'):
            t10.loader(h, 'source', 'run', duty100=True)
        self.assertEqual(h.run.command.call_args.args[0][-2:], ['--duty-cycle-percent', '100'])

    def test_loader_rejects_structured_errors_and_empty_output(self):
        from unittest.mock import Mock
        h = Mock()
        h.argv.return_value = ['oo-load']
        for output in ('', '{"error":"failed"}'):
            h.run.command.return_value = (0, output)
            with self.assertRaisesRegex(RuntimeError, 'missing_or_failed'):
                t10.loader(h, 'source', 'run')

    def test_product_defaults_not_overridden(self):
        from unittest.mock import Mock
        h = Mock()
        h.argv.return_value = ['oo-load', '--source', 'fixture']
        h.run.command.return_value = (0, '{"read_budget":{"duty_cycle_percent":25}}')
        t10.loader(h, 'source', 'run')
        self.assertEqual(h.run.command.call_args.args[0], h.argv.return_value)


if __name__ == '__main__':
    unittest.main()
