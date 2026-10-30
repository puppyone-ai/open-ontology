"""Managed capture guards; all unit data are synthetic, no Docker or source DML."""
import contextlib
import copy
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest
from unittest.mock import patch
from types import SimpleNamespace

import continuous_capture as m


class ManagedCaptureTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.budgets = {"deadline_seconds": 120, "window_seconds": 5, "quiet_seconds": 2,
                        "poll_seconds": 0.1, "max_new_events": 100, "max_transactions": 100,
                        "max_spool_bytes": 1048576, "max_transport_messages": 1000}
        self.cfg = {"protocol": m.PROTOCOL, "source_id": "test", "table": "OWNER.DATA",
                    "budgets": self.budgets,
                    "resources": {"sink_name": "unit-sink", "server_name": "unit-server",
                                  "source": {"name": "unit-db"}, "sink_image": "sha256:" + "a" * 64,
                                  "server_image": "sha256:" + "b" * 64,
                                  "owner_label": {"key": "oo.owner", "value": "unit-only"}}}

    def tearDown(self):
        self.tmp.cleanup()

    def spool(self):
        path = self.root / "capture.sqlite"
        with contextlib.closing(sqlite3.connect(path)) as db, db:
            db.executescript("""
CREATE TABLE transport(seq INTEGER PRIMARY KEY,body BLOB);
CREATE TABLE snapshots(seq INTEGER PRIMARY KEY,body BLOB);
CREATE TABLE events(tx TEXT,ord INTEGER,body BLOB,PRIMARY KEY(tx,ord));
CREATE TABLE boundaries(tx TEXT PRIMARY KEY,expected INTEGER,body BLOB);
""")
            db.execute("INSERT INTO events VALUES ('tx',1,?)",
                       (json.dumps({"op": "u", "source": {"commit_scn": "123"}}),))
            db.execute("INSERT INTO boundaries VALUES ('tx',1,?)",
                       (json.dumps({"status": "END", "id": "tx", "event_count": 1}),))
        return path

    def test_configurable_resource_and_budget(self):
        m.validate_config(self.cfg)
        for name, value in (("max_new_events", 0), ("deadline_seconds", 4),
                            ("poll_seconds", False), ("poll_seconds", float("nan")),
                            ("max_transactions", 1.5)):
            bad = copy.deepcopy(self.cfg)
            bad["budgets"][name] = value
            with self.subTest(name=name), self.assertRaises(m.Refusal):
                m.validate_config(bad)
        bad = copy.deepcopy(self.cfg)
        bad["resources"]["sink_name"] = bad["resources"]["server_name"]
        with self.assertRaisesRegex(m.Refusal, "resource_collision"):
            m.validate_config(bad)

    def test_no_mutable_image_tags_or_multitable_regex(self):
        for field in ("sink_image", "server_image"):
            bad = copy.deepcopy(self.cfg)
            bad["resources"][field] = "debezium:latest"
            with self.assertRaisesRegex(m.Refusal, "image_digest"):
                m.validate_config(bad)
        for table in ("OWNER.*", "OWNER.ONE,OWNER.TWO", "OWNER.ONE;DROP"):
            with self.assertRaisesRegex(m.Refusal, "single_table_scope"):
                m.validate_config({**self.cfg, "table": table})

    def test_multitable_scope_is_explicit_not_a_regex_or_silent_membership_change(self):
        config = {k: v for k, v in self.cfg.items() if k != "table"}
        config["tables"] = ["OWNER.B", "OWNER.A"]
        m.validate_config(config)
        for tables in ([], ["OWNER.A", "OWNER.A"], ["OWNER.*"], ["OWNER.A", "OTHER.B"]):
            with self.assertRaises(m.Refusal):
                m.validate_config(dict(config, tables=tables))
        with self.assertRaisesRegex(m.Refusal, "ambiguous_table_scope"):
            m.validate_config(dict(config, table="OWNER.A"))

    def test_large_scope_keeps_all_members_and_existing_resource_budgets(self):
        config = {k: v for k, v in self.cfg.items() if k != "table"}
        config["tables"] = [f"OWNER.T{i:04d}" for i in range(257)]
        m.validate_config(config)
        self.assertEqual(len(m.p.capture_tables(config)), 257)
        bad = copy.deepcopy(config)
        bad["budgets"]["max_new_events"] = 100001
        with self.assertRaisesRegex(m.Refusal, "budget_max_new_events"):
            m.validate_config(bad)

    def test_capture_config_bytes_checked_before_secret_or_resource_access(self):
        path = self.root / "oversized.json"
        path.write_bytes(b" " * 65)
        with patch.object(m.p, "MAX_CONFIG_BYTES", 64), \
                patch.object(m.operator, "private") as private, \
                self.assertRaisesRegex(ValueError, "config_input_size"):
            m.Cycle(path, self.root / "out")
        private.assert_not_called()

    def test_whole_end_accepted(self):
        value = m.spool_cut(self.spool(), self.budgets)
        self.assertEqual(len(value["transactions"]), 1)
        self.assertEqual(value["committed_counts"], {"c": 0, "u": 1, "d": 0})

    def test_event_budget_yields_only_after_the_complete_oversized_transaction(self):
        baseline = {"captured_log_rows": 9000000, "transactions": ["retained"],
                    "transport_messages": 2000000}
        current = {"captured_log_rows": 9101000, "transactions": ["retained"],
                   "transport_messages": 2000001, "incomplete_transactions": 1}
        for elapsed in (0, 100):
            self.assertFalse(m.cut_ready(current, baseline, self.budgets, elapsed, 100))
        current.update(transactions=["retained", "complete_101k"], incomplete_transactions=0)
        self.assertEqual(m.capture_progress(current, baseline, self.budgets), 101000)
        self.assertTrue(m.cut_ready(current, baseline, self.budgets, 0, 100))
        self.assertFalse(m.cut_ready(current, baseline, self.budgets, 0, 0))
        current["transport_messages"] = baseline["transport_messages"]+self.budgets["max_transport_messages"]+1
        with self.assertRaisesRegex(m.Refusal, "capture_budget"):
            m.cut_ready(current, baseline, self.budgets, 100, 100)

    def test_deadline_remains_hard_while_an_oversized_transaction_is_incomplete(self):
        cycle = object.__new__(m.Cycle)
        cycle.budgets = self.budgets
        cycle.deadline = 0
        with patch.object(m.time, "monotonic", return_value=1), \
                patch.object(m.time, "sleep") as sleep, self.assertRaisesRegex(m.Refusal, "deadline"):
            cycle.sleep()
        sleep.assert_not_called()

    def test_resume_cannot_extend_an_expired_or_missing_deadline(self):
        state = {"started_unix": 100, "deadline_unix": 220}
        with patch.object(m.time, "time", return_value=200), \
                patch.object(m.time, "monotonic", return_value=900):
            self.assertEqual(m.retained_deadline(state, 120), 920)
            with self.assertRaisesRegex(m.Refusal, "resume_deadline_binding"):
                m.retained_deadline({}, 120)
            with self.assertRaisesRegex(m.Refusal, "resume_deadline_binding"):
                m.retained_deadline(state, 240)
        with patch.object(m.time, "time", return_value=221), self.assertRaisesRegex(m.Refusal, "deadline"):
            m.retained_deadline(state, 120)
        self.assertNotIn("deadline", m.RECOVERABLE)
        self.assertEqual(state, {"started_unix": 100, "deadline_unix": 220})

    def test_large_disk_quota_requires_real_capacity_without_changing_memory_budgets(self):
        config = copy.deepcopy(self.cfg)
        config["budgets"]["max_spool_bytes"] = 1024**3
        m.validate_config(config)
        with patch.object(m.shutil, "disk_usage", return_value=SimpleNamespace(free=4*1024**3)):
            proof = m.storage_preflight(self.root, config["budgets"])
            self.assertEqual(proof["required_free_bytes"], 4*1024**3)
        with patch.object(m.shutil, "disk_usage", return_value=SimpleNamespace(free=4*1024**3-1)), \
                self.assertRaisesRegex(m.Refusal, "spool_storage_capacity"):
            m.storage_preflight(self.root, config["budgets"])
    def test_events_without_end_refused(self):
        path = self.spool()
        with contextlib.closing(sqlite3.connect(path)) as db, db:
            db.execute("DELETE FROM boundaries")
        with self.assertRaisesRegex(m.Refusal, "incomplete_end"):
            m.spool_cut(path, self.budgets)

    def test_end_without_events_also_refused(self):
        path = self.spool()
        with contextlib.closing(sqlite3.connect(path)) as db, db:
            db.execute("DELETE FROM events")
        with self.assertRaisesRegex(m.Refusal, "incomplete_end"):
            m.spool_cut(path, self.budgets)

    def test_wrong_end_count_and_event_gap_refused(self):
        path = self.spool()
        with contextlib.closing(sqlite3.connect(path)) as db, db:
            db.execute("UPDATE boundaries SET expected=2")
        with self.assertRaisesRegex(m.Refusal, "incomplete_end"):
            m.spool_cut(path, self.budgets)
        with contextlib.closing(sqlite3.connect(path)) as db, db:
            db.execute("UPDATE boundaries SET expected=1")
            db.execute("UPDATE events SET ord=2")
        with self.assertRaisesRegex(m.Refusal, "incomplete_end"):
            m.spool_cut(path, self.budgets)

    def test_live_wal_not_immutable_input(self):
        path = self.spool()
        Path(str(path) + "-wal").touch()
        with self.assertRaisesRegex(m.Refusal, "immutable_spool_required"):
            m.spool_cut(path, self.budgets)

    def test_missing_logs_fail_closed_not_connection_retry(self):
        for text in ("ORA-01291: missing logfile",
                     "None of log files contains offset SCN 123",
                     "SCN 123 is no longer available"):
            self.assertEqual(m.classify_log(text), "missing_log_fail_closed")
        self.assertNotIn("missing_log_fail_closed", m.RECOVERABLE)
        self.assertEqual(m.classify_log("ORA-03113: end-of-file on communication channel"),
                         "capture_connection_error")
        self.assertIn("capture_connection_error", m.RECOVERABLE)

    def test_unknown_error_and_schema_fail_closed(self):
        self.assertEqual(m.classify_log('{"level":"ERROR","message":"arbitrary"}'),
                         "connector_fail_closed")
        self.assertEqual(m.classify_log("schema_drift_rebaseline_required"), "schema_fail_closed")
        self.assertIsNone(m.classify_log('{"level":"INFO","message":"Starting streaming"}'))

    def test_requested_completed_shutdown_only_ignores_exact_interrupt(self):
        start = {"loggerName": "io.debezium.server.DebeziumServer",
                 "message": "Received request to stop the engine"}
        error = {"loggerName": "io.debezium.pipeline.ErrorHandler", "level": "ERROR",
                 "message": "Producer failure [Error Occurred After Shutdown]",
                 "exception": {"exceptionType": "java.lang.InterruptedException",
                               "message": "sleep interrupted"}}
        end = {"loggerName": "io.debezium.server.ConnectorLifecycle",
               "message": "Connector completed: success = 'true', message = "
               "'Connector 'io.debezium.connector.oracle.OracleConnector' completed normally.', error = 'null'"}
        render = lambda rows: "\n".join(json.dumps(row) for row in rows)
        self.assertIsNone(m.classify_log(render([start, error, end]), stopping=True))
        self.assertEqual(m.classify_log(render([start, error, end])), "connector_fail_closed")
        for rows in ([error, start, end], [start, error], [error, end],
                     [start, error, end, {"level": "ERROR", "message": "unknown"}],
                     [start, {**error, "loggerName": "unknown", "message": ""}, end],
                     [start, {**error, "loggerName": None, "message": ""}, end],
                     [start, {**error, "message": ""}, end],
                     [start, {**error, "exception": {"exceptionType": "java.sql.SQLException"}}, end],
                     [start, {**error, "message": "ORA-01291"}, end]):
            with self.subTest(rows=rows):
                self.assertIsNotNone(m.classify_log(render(rows), stopping=True))

    def test_metadata_checks_exact_identity_object_keys_columns(self):
        identity = "1|2|" + "A" * 32 + "|PDB|DB|READER"
        table = {"owner": "OWNER", "table": "DATA", "keys": ["ID"], "object": ["42|date"],
                 "issues": [], "columns": [{"name": "ID", "type": "NUMBER"}]}
        path = self.root / "schema.json"
        m.write(path, {"binding": {"identity": identity, "inventory": [table]}})
        cfg = {**self.cfg, "source_identity": m.c.identity(identity)}
        seed = {"schema_manifest": m.c.reference(path)}
        inv = {"source_id": "test", "identity": identity, "tables": [table]}
        m.verify_metadata(cfg, seed, inv)
        for key, changed in (("keys", []), ("object", ["43|new"]), ("columns", [{"name": "ID", "type": "RAW"}])):
            bad = copy.deepcopy(inv)
            bad["tables"][0][key] = changed
            with self.subTest(key=key), self.assertRaises(m.Refusal):
                m.verify_metadata(cfg, seed, bad)
        with self.assertRaisesRegex(m.Refusal, "source_identity_drift"):
            m.verify_metadata(cfg, seed, {**inv, "identity": identity.replace("1|2|", "9|2|")})

    def test_named_capture_drift_prevents_stop(self):
        cycle = object.__new__(m.Cycle)
        cycle.cfg = self.cfg
        cycle.state = {"resources": {"server": {"name": "unit-server", "id": "original",
                       "image": "sha256:" + "b" * 64, "mounts": {"/data": ["/owned", True]}}}}
        good = {"Id": "original", "Image": "sha256:" + "b" * 64,
                "Config": {"Labels": {"oo.owner": "unit-only"}},
                "Mounts": [{"Destination": "/data", "Source": "/owned", "RW": True}]}
        with patch.object(cycle, "inspect", return_value=good):
            self.assertEqual(cycle.owned_capture("server")["Id"], "original")
        for key, changed in (("Id", "replacement"), ("Image", "sha256:" + "c" * 64),
                             ("Mounts", []), ("Config", {"Labels": {"oo.owner": "other"}})):
            with patch.object(cycle, "inspect", return_value={**good, key: changed}), \
                    patch.object(cycle, "command") as command, \
                    self.assertRaisesRegex(m.Refusal, "capture_resource_drift"):
                cycle.stop()
            command.assert_not_called()

    def test_every_selected_table_metadata_is_checked_before_capture(self):
        identity = "1|2|" + "A" * 32 + "|PDB|DB|READER"
        tables = [{"owner": "OWNER", "table": name, "keys": ["ID"], "object": [f"{i}|date"],
                   "issues": [], "columns": [{"name": "ID", "type": "NUMBER"}]}
                  for i, name in enumerate(("A", "B"), 1)]
        path = self.root / "multi-schema.json"
        m.write(path, {"binding": {"identity": identity, "inventory": tables}})
        config = {k: v for k, v in self.cfg.items() if k != "table"}
        config.update(tables=["OWNER.A", "OWNER.B"], source_identity=m.c.identity(identity))
        seed = {"schema_manifest": m.c.reference(path)}
        inventory = {"source_id": "test", "identity": identity, "tables": tables}
        m.verify_metadata(config, seed, inventory)
        for replacement in (tables[:1], tables + [tables[0]]):
            with self.assertRaisesRegex(m.Refusal, "table_missing"):
                m.verify_metadata(config, seed, dict(inventory, tables=replacement))
        changed = copy.deepcopy(inventory)
        changed["tables"][1]["columns"][0]["type"] = "RAW"
        with self.assertRaisesRegex(m.Refusal, "schema_fail_closed"):
            m.verify_metadata(config, seed, changed)

    def test_33rd_member_cannot_be_missing_duplicated_or_changed(self):
        identity = "1|2|" + "A" * 32 + "|PDB|DB|READER"
        tables = [{"owner": "OWNER", "table": f"T{i:02d}", "keys": ["ID"],
                   "object": [f"{i}|date"], "issues": [],
                   "columns": [{"name": "ID", "type": "NUMBER"}]} for i in range(1, 34)]
        path = self.root / "33-schema.json"
        m.write(path, {"binding": {"identity": identity, "inventory": tables}})
        config = {k: v for k, v in self.cfg.items() if k != "table"}
        config.update(tables=[f"OWNER.T{i:02d}" for i in range(1, 34)],
                      source_identity=m.c.identity(identity))
        seed = {"schema_manifest": m.c.reference(path)}
        inventory = {"source_id": "test", "identity": identity, "tables": tables}
        m.verify_metadata(config, seed, inventory)
        for replacement in (tables[:-1], tables + [tables[-1]]):
            with self.assertRaisesRegex(m.Refusal, "table_missing"):
                m.verify_metadata(config, seed, dict(inventory, tables=replacement))
        changed = copy.deepcopy(inventory)
        changed["tables"][-1]["columns"][0]["type"] = "RAW"
        with self.assertRaisesRegex(m.Refusal, "schema_fail_closed"):
            m.verify_metadata(config, seed, changed)

    def test_unsupported_interface_explicit(self):
        self.assertIn("No source DML", m.__doc__)
        self.assertIn("explicit set of keyed tables", m.__doc__)
        self.assertIn("incomplete_end", m.RECOVERABLE)
        self.assertNotIn("history_fail_closed", m.RECOVERABLE)

    def test_successful_resume_does_not_report_prior_error(self):
        cycle = object.__new__(m.Cycle)
        cycle.root = self.root
        cycle.state = {"status": "failed", "error": "capture_process_stopped",
                       "cleanup_error": None, "resources": {"server": {"id": "owned"}}}
        m.write(self.root / "prior-failure.json", cycle.state)
        old = (self.root / "prior-failure.json").read_bytes()
        cycle.persist("starting")
        cycle.persist("sealed", result={"sha256": "unit"})
        state = json.loads((self.root / "state.json").read_text())
        self.assertNotIn("error", state)
        self.assertNotIn("cleanup_error", state)
        self.assertEqual(state["resources"]["server"]["id"], "owned")
        self.assertEqual((self.root / "prior-failure.json").read_bytes(), old)

    def test_working_configuration_drift_refused(self):
        cycle = object.__new__(m.Cycle)
        cycle.root, cycle.cfg = self.root, self.cfg
        path = self.root / "working/config/application.properties"
        path.parent.mkdir(parents=True)
        path.write_text("expected")
        with patch.object(m, "properties", return_value="expected"):
            cycle.check_working_config()
            path.write_text("changed-to-resnapshot")
            with self.assertRaisesRegex(m.Refusal, "capture_config_drift"):
                cycle.check_working_config()


if __name__ == "__main__":
    unittest.main()
