import json
from pathlib import Path
import sys
import tempfile
import unittest
import source_scheduler as m


class SchedulerTests(unittest.TestCase):
    def test_cost_guard_and_stale_metrics(self):
        self.assertEqual(m.guard({'observed_at': 100, 'egress_bytes': 20, 'balance': 5},
                                 {'max_egress_bytes': 20, 'min_balance': 1}, 100), 'egress_limit')
        self.assertEqual(m.guard({'observed_at': 0, 'egress_bytes': 0, 'balance': 5},
                                 {'max_egress_bytes': 20, 'min_balance': 1}, 100), 'metrics_stale')
        self.assertEqual(m.guard({'observed_at': 100, 'egress_bytes': 0, 'balance': 0},
                                 {'max_egress_bytes': 20, 'min_balance': 1}, 100), 'balance_limit')
        self.assertRaises(ValueError, m.guard, {}, {}, 100)

    def spec(self, root, fail=False):
        script = root/'child.py'
        script.write_text('import sys\nfrom pathlib import Path\np=Path(sys.argv[1])\n'
                          'if sys.argv[2]=="provision": p.write_text("ready")\n'
                          'else:\n assert p.read_text()=="ready"\n'
                          ' sys.exit(1 if sys.argv[2]=="bad" else 0)\n')
        return {'concurrency': 2, 'stagger_seconds': 0, 'provision_timeout': 2,
                'table_timeout': 2, 'sources': [
                    {'key': 's001', 'provision': [sys.executable, str(script), str(root/'ready'), 'provision'],
                     'tables': [{'table': name, 'argv': [sys.executable, str(script), str(root/'ready'), name]}
                                for name in (['good', 'bad'] if fail else ['good'])]}]}

    def test_provision_then_capture_and_failed_only(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d); spec = self.spec(root, True)
            result = m.run(spec, root/'run')
            self.assertEqual(result['failed'], [{'source': 's001', 'table': 'bad', 'reason': 'exit_1'}])
            retry = m.run(spec, root/'retry', previous=result)
            self.assertEqual([t['table'] for t in retry['results']], ['bad'])

    def test_guard_pause_has_retryable_unfinished_work(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d); spec = self.spec(root)
            path = root/'metrics.json'; path.write_text('{}')
            spec['cost_guard'] = {'metrics_file': str(path), 'max_egress_bytes': 10, 'min_balance': 1}
            result = m.run(spec, root/'run')
            self.assertEqual(result['paused'], 'metrics_unavailable')
            self.assertEqual(result['failed'][0]['table'], 'good')
            self.assertFalse((root/'ready').exists())

    def test_provision_failure_blocks_capture(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d); spec = self.spec(root)
            spec['sources'][0]['provision'] = [sys.executable, '-c', 'raise SystemExit(2)']
            result = m.run(spec, root/'run')
            self.assertEqual(result['failed'][0]['reason'], 'provision_exit_2')
            self.assertFalse((root/'ready').exists())

    def test_tunnel_reconnect_is_bounded_and_never_launches_without_probe(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d); spec = self.spec(root)
            spec['tunnel'] = {'argv': [sys.executable, '-c', 'raise SystemExit(1)'],
                              'probe': [sys.executable, '-c', 'raise SystemExit(1)'],
                              'max_attempts': 2, 'backoff_seconds': 0, 'ready_timeout': .2}
            result = m.run(spec, root/'run')
            self.assertEqual(result['paused'], 'tunnel_reconnect_exhausted')
            self.assertFalse((root/'ready').exists())

    def test_concurrency_cap_and_stagger(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            script = root/'events.py'
            script.write_text('import sys,time\nfrom pathlib import Path\n'
                              'p=Path(sys.argv[1])\n'
                              'with p.open("a") as f:f.write(str(time.monotonic())+"\\n")\n'
                              'time.sleep(.15)\n')
            spec = {'concurrency': 1, 'stagger_seconds': .1, 'sources': [
                {'key': f's{i}', 'provision': [sys.executable, str(script), str(root/'events')],
                 'tables': [{'table': 't_items', 'argv': [sys.executable, str(script), str(root/'events')]}]}
                for i in range(3)]}
            result = m.run(spec, root/'run')
            self.assertEqual(result['failed'], [])
            events = [float(t) for t in (root/'events').read_text().splitlines()]
            self.assertEqual(len(events), 6)
            self.assertTrue(all(b - a >= .14 for a, b in zip(events, events[1:])))

    def test_cost_limit_stops_inflight_owned_process(self):
        with tempfile.TemporaryDirectory() as d:
            import time
            root = Path(d); spec = self.spec(root)
            metrics = root/'metrics.json'
            metrics.write_text(json.dumps({'observed_at': time.time(), 'egress_bytes': 0, 'balance': 10}))
            script = root/'consume.py'
            script.write_text('import json,sys,time\nfrom pathlib import Path\n'
                              'Path(sys.argv[1]).write_text(json.dumps(dict(observed_at=time.time(),egress_bytes=100,balance=10)))\n'
                              'time.sleep(10)\nPath(sys.argv[2]).write_text("unexpected")\n')
            spec['sources'][0]['tables'][0]['argv'] = [sys.executable, str(script), str(metrics), str(root/'finished')]
            spec['cost_guard'] = {'metrics_file': str(metrics), 'max_egress_bytes': 10, 'min_balance': 1}
            started = time.monotonic()
            result = m.run(spec, root/'run')
            self.assertLess(time.monotonic() - started, 5)
            self.assertEqual(result['paused'], 'egress_limit')
            self.assertEqual(result['failed'][0]['reason'], 'egress_limit')
            self.assertFalse((root/'finished').exists())

    def test_invalid_concurrency(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d); spec = self.spec(root); spec['concurrency'] = 0
            self.assertRaises(ValueError, m.run, spec, root/'run')


if __name__ == '__main__':
    unittest.main()
