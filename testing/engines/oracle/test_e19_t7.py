"""19c dispatch must preserve shared audit decisions and evidence hashes."""
import argparse
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

import e19_t7 as adapter
from verdicts import merge, write_rows


class EnterpriseAuditTests(unittest.TestCase):
    def test_incremental_requires_both_real_workloads(self):
        result = adapter.coverage("incremental", "oracle", "19",
                                  {"C5": "passed", "C1": "not_run"}, [])
        self.assertTrue(result["c5"][0])
        self.assertFalse(result["c1"][0])
        self.assertEqual(adapter.coverage("incremental", "oracle", "21",
                                         {"C5": "passed"}, []).keys(), {"c5"})

    def test_standard_miner_audit_uses_common_account(self):
        run = Mock(engine="oracle", version="19", name="owned-fixture")
        audit = adapter.EnterpriseAudit(run)
        audit.mode = "oracle-standard"
        audit.enable_miner()
        sql = run.command.call_args.kwargs["data"]
        self.assertIn("connect / as sysdba", sql)
        self.assertIn("AUDIT ALL STATEMENTS BY C##OO_MINER BY ACCESS", sql)
        self.assertIn("EXECUTE PROCEDURE BY C##OO_MINER BY ACCESS", sql)
        self.assertNotIn("CREATE AUDIT POLICY", sql)
        self.assertIn("C##OO_MINER", audit.users)

    def test_common_standard_query_uses_userid_and_restores_command(self):
        run = Mock(engine="oracle", version="19", name="owned-fixture")
        command = run.command
        command.return_value = (0, "")
        audit = adapter.EnterpriseAudit(run)
        audit.mode = "oracle-standard"

        def collect(_):
            run.command(["docker", "exec"], data="SELECT a.sqltext FROM sys.aud$ a "
                        "LEFT JOIN audit_actions m ON m.action=a.action# "
                        "WHERE dbusername='C##OO_MINER' ORDER BY a.ntimestamp#;")
            raise RuntimeError("collection failure")

        with patch.object(adapter.Audit, "raw", collect):
            with self.assertRaisesRegex(RuntimeError, "collection failure"):
                audit.raw()
        self.assertIs(run.command, command)
        text = command.call_args.kwargs["data"]
        self.assertIn("WHERE a.userid='C##OO_MINER'", text)
        self.assertNotIn("dbusername", text)

    def test_sqlplus_wrapping_is_lossless_or_rejected(self):
        sql = "SELECT " + "x" * 35000 + " FROM dual"
        row = json.dumps({"sql": sql, "sql_length": str(len(sql))})
        wrapped = row[:32767] + "\n" + row[32767:] + "\n"
        self.assertEqual(adapter.unwrap_audit_rows(wrapped), row)
        with self.assertRaisesRegex(ValueError, "length mismatch"):
            adapter.unwrap_audit_rows(json.dumps({"sql": sql[:-1],
                                                  "sql_length": str(len(sql))}))
        with self.assertRaisesRegex(ValueError, "incomplete"):
            adapter.unwrap_audit_rows(row[:-1])

    def test_metadata_correction_preserves_failed_verdicts_and_hashes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            out = root / "t7" / "oracle-19"
            out.mkdir(parents=True)
            raw = out / "t7-incremental.json"
            report = {"not_run": {"c1": "legacy XE exclusion"},
                      "verdicts": {"c1": {"result": "fail", "reasons": ["rejected"]},
                                   "c5": {"result": "pass"}}}
            adapter.t7.runner.write_json(raw, report)
            run = argparse.Namespace(engine="oracle", version="19", id="test",
                                     receipt={"version_actual": "19.19.0.0",
                                              "execution": "native"})
            write_rows(run, raw, [("T7", "c1", "fail"), ("T7", "c5", "pass")])
            with patch.object(adapter, "shared_run_fixture", return_value=report):
                result = adapter.run_fixture("oracle", "19", "incremental", root, None)
            self.assertNotIn("not_run", result)
            self.assertEqual(result["verdicts"]["c1"]["reasons"], ["rejected"])
            self.assertEqual({r["variant"]: r["result"] for r in merge(root)},
                             {"c1": "fail", "c5": "pass"})
            self.assertEqual(json.loads(raw.read_text()), result)


if __name__ == "__main__":
    unittest.main()
