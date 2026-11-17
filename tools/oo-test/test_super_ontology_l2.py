import unittest

import super_ontology_l2 as l2

SAMPLE = """COMMAND   PID USER   FD   TYPE DEVICE SIZE/OFF NODE NAME
oo      100 u   10u  IPv4 0x1      0t0  TCP 127.0.0.1:5000 (LISTEN)
oo      100 u   11u  IPv6 0x2      0t0  TCP [::1]:5001 (LISTEN)
engine  101 u   12u  IPv4 0x3      0t0  TCP 127.0.0.1:5002->127.0.0.1:5432 (ESTABLISHED)
"""


class SocketAudit(unittest.TestCase):
    def test_loopback_only_passes(self):
        rows = l2.parse_lsof_names(SAMPLE)
        self.assertEqual(len(rows), 3)
        self.assertEqual(l2.non_loopback(rows), [])

    def test_wildcard_bind_fails(self):
        rows = l2.parse_lsof_names(SAMPLE + "engine 101 u 13u IPv4 0x4 0t0 TCP *:8080 (LISTEN)\n")
        self.assertEqual(l2.non_loopback(rows), ["*:8080"])

    def test_remote_peer_fails(self):
        rows = l2.parse_lsof_names(SAMPLE + "engine 101 u 13u IPv4 0x4 0t0 TCP 127.0.0.1:5003->93.184.216.34:443 (ESTABLISHED)\n")
        self.assertEqual(len(l2.non_loopback(rows)), 1)

    def test_ipv6_remote_fails(self):
        rows = l2.parse_lsof_names(SAMPLE + "oo 100 u 14u IPv6 0x5 0t0 TCP [::1]:5004->[2001:db8::1]:443 (ESTABLISHED)\n")
        self.assertEqual(len(l2.non_loopback(rows)), 1)


if __name__ == "__main__":
    unittest.main()


class FastSuite(unittest.TestCase):
    def test_suite_runs_the_l2_script_with_the_run_id(self):
        import tempfile
        from pathlib import Path
        from types import SimpleNamespace
        from unittest.mock import patch

        import fast

        seen = {}

        def fake(name, command, root, timeout, lane=True):
            seen.update(name=name, command=command, lane=lane)
            return {"name": name, "exit": 0, "status": "passed", "seconds": 0.0}

        with tempfile.TemporaryDirectory() as tmp, patch.object(fast, "timed_command", fake):
            rows = fast.run_super_ontology_l2(Path(tmp), SimpleNamespace(run_id="x1"))
        self.assertEqual(rows[0]["suite"], "super-ontology")
        self.assertEqual(rows[0]["status"], "passed")
        self.assertIn("super_ontology_l2.py", seen["command"][2])
        self.assertEqual(seen["command"][seen["command"].index("--run-id") + 1], "x1")

    def test_suite_needs_layer_l2(self):
        import fast
        with self.assertRaises(SystemExit):
            fast.main(["--layer", "L1", "--suite", "super-ontology", "--run-id", "x"])
