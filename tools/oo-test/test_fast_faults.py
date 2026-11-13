import json
import signal
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import t3t4


class Proc:
    def __init__(self, code):
        self.code, self.started, self.killed = code, 0.0, False

    def poll(self):
        return self.code


class FastFaultTests(unittest.TestCase):
    def tearDown(self):
        t3t4.FAST = False

    def harness(self):
        h = SimpleNamespace(failpoint=None, proxy=None, healed=0)
        h.finish = lambda proc, timeout: (proc.code, [], None)
        h.heal = lambda: setattr(h, "healed", h.healed + 1)
        return h

    def test_failpoints_are_armed_only_for_fast_sigkill(self):
        h = self.harness()
        t3t4.arm(h, "sigkill", "c5")
        self.assertIsNone(h.failpoint)
        t3t4.FAST = True
        t3t4.arm(h, "network_drop", "c5")
        self.assertIsNone(h.failpoint)
        for path in ("snapshot", "c5", "c1"):
            t3t4.arm(h, "sigkill", path)
            self.assertEqual(h.failpoint, t3t4.FAILPOINT[path])

    def test_failpoint_abort_counts_as_mid_run_kill_only_on_sigabrt(self):
        t3t4.FAST = True
        h = self.harness()
        h.failpoint = t3t4.FAILPOINT["snapshot"]
        step = t3t4.interrupted(h, "sigkill", Proc(-signal.SIGABRT), lambda: False)
        self.assertTrue(step["injected_mid_run"])
        self.assertEqual(step["mechanism"], "failpoint")
        self.assertIsNone(h.failpoint)
        self.assertTrue(t3t4.first_ok(step, "sigkill"))
        # The loader finished before the armed point: not an injected fault.
        h.failpoint = t3t4.FAILPOINT["c1"]
        step = t3t4.interrupted(h, "sigkill", Proc(0), lambda: False)
        self.assertFalse(step["injected_mid_run"])

    def test_toxiproxy_outage_reenables_the_proxy(self):
        t3t4.FAST = True
        h = self.harness()
        calls = []
        h.proxy = SimpleNamespace(set=calls.append)
        proc = Proc(None)
        proc.send_signal = lambda s: None
        h.run = SimpleNamespace(command=lambda *a, **k: (0, ""))
        with patch.object(t3t4, "wait_until", return_value=True), \
             patch.object(t3t4.time, "sleep"):
            step = t3t4.interrupted(h, "network_drop", proc, lambda: True)
        self.assertEqual(calls, [False, True])
        self.assertEqual(step["mechanism"], "toxiproxy")

    def test_proxy_cleanup_refuses_foreign_containers(self):
        proxy = object.__new__(t3t4.Toxiproxy)
        proxy.created, proxy.name = True, "oo-ta-toxi-x"
        proxy.run = SimpleNamespace(id="ours", command=lambda *a, **k: (
            0, json.dumps({"oo.lane": "other", "oo.run_id": "ours"})))
        with self.assertRaisesRegex(RuntimeError, "ownership"):
            proxy.close()

    def test_fast_and_subset_runs_write_no_support_rows(self):
        result = {"cases": {c: {"status": "passed"} for c in t3t4.T4}}
        self.assertEqual(len(t3t4.suite_verdicts(result, "postgres", "17", list(t3t4.T4))), 2)
        self.assertEqual(t3t4.suite_verdicts(result, "postgres", "17", ["add_column"]), [])
        t3t4.FAST = True
        self.assertEqual(t3t4.suite_verdicts(result, "postgres", "17", list(t3t4.T4)), [])


if __name__ == "__main__":
    unittest.main()
