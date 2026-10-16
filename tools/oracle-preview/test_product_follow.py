import hashlib
import json
import subprocess
import sys
import tempfile
import unittest
import uuid
from pathlib import Path
from unittest.mock import patch

import product_follow as follow_module


class PublicationContextTests(unittest.TestCase):
    def test_candidate_version_binds_mapping_and_fresh_request_nonce(self):
        run_id = str(uuid.uuid4())
        mapping_sha = hashlib.sha256(b"native mapping").hexdigest()
        first = follow_module.release_candidate_version(11, run_id, mapping_sha, "a" * 12)
        second = follow_module.release_candidate_version(11, run_id, mapping_sha, "b" * 12)
        self.assertNotEqual(first, second)
        self.assertTrue(first.endswith("-" + mapping_sha[:12] + "-" + "a" * 12))
        self.assertLessEqual(len(first), 64)
        self.assertRegex(first, r"^[A-Za-z0-9][A-Za-z0-9._:-]{0,63}$")

    def test_release_entry_uses_explicit_runtime_base_and_replica_host(self):
        with tempfile.TemporaryDirectory() as tmp:
            private_root = Path(tmp) / "release"
            private_root.mkdir(mode=0o700)
            runtime_path = private_root / "release-runtime-base-verified.json"
            runtime_path.write_text(json.dumps({"replica_pg": {"host": "verified-dns-name"}}))
            release = {
                "private_root": str(private_root),
                "runtime_base": str(runtime_path),
                "runtime_container_prefix": "/owned/release/",
            }

            container_path = follow_module.release_runtime_container(release)
            _, runtime = follow_module.release_runtime_base(release)

            release["runtime_base"] = str(Path(tmp) / "outside-runtime.json")
            Path(release["runtime_base"]).write_text(json.dumps({"replica_pg": {"host": "other-name"}}))
            with self.assertRaisesRegex(ValueError, "release_runtime_base_path"):
                follow_module.release_runtime_container(release)

        self.assertEqual(container_path, "/owned/release/release-runtime-base-verified.json")
        self.assertEqual(runtime["replica_pg"]["host"], "verified-dns-name")


    def test_gateway_uses_matching_explicit_replica_host(self):
        gateway = {"standby_container": "docker-container-name", "standby_host": "verified-dns-name"}
        self.assertEqual(follow_module.gateway_replica_host(gateway, "verified-dns-name"), "verified-dns-name")
        with self.assertRaisesRegex(ValueError, "gateway_replica_host_mismatch"):
            follow_module.gateway_replica_host({"standby_container": "docker-container-name"}, "verified-dns-name")
        with self.assertRaisesRegex(ValueError, "gateway_replica_host_mismatch"):
            follow_module.gateway_replica_host({**gateway, "standby_host": "wrong-host"}, "verified-dns-name")

    def test_saved_release_checkpoint_requires_coherent_operation_and_active_pack(self):
        run_id = str(uuid.uuid4())
        version = "oracle-c5-g11-" + run_id.replace("-", "")[:8] + "-" + "a" * 12 + "-" + "b" * 12
        mapping = {"datasets": [{"dataset": "orders", "source": "oracle_oneclick_orders", "generation": 11}]}
        mapping_sha = hashlib.sha256(follow_module.canonical(mapping)).hexdigest()
        pack_digest = "c" * 64
        source_hash = "d" * 64
        manifest_sha = "e" * 64
        current = {"generation": 11, "result": {"run_id": run_id}, "native": {"pack_digest": pack_digest}}
        status = {"operation_id": "op_checkpoint", "candidate_id": version, "version": version,
                  "status": "succeeded", "half_published": False, "pack_digest": pack_digest,
                  "source_hash": source_hash}
        intent = {"candidate_id": version, "version": version, "generation": 11, "run_id": run_id,
                  "native_mapping_sha256": mapping_sha}
        receipt = {"candidate_id": version, "version": version, "operation_id": "op_checkpoint",
                   "native_mapping_sha256": mapping_sha, "idempotency_key": "f" * 32,
                   "request_body_sha256": "1" * 64}
        active = {"project_id": "oracle", "candidate_id": version, "version": version,
                  "pack_digest": pack_digest, "source_hash": source_hash,
                  "manifest_sha256": manifest_sha, "half_published": False}
        pack = {"project_id": "oracle", "candidate_id": version, "version": version,
                "pack_digest": pack_digest, "manifest_sha256": manifest_sha, "half_published": False}

        verified = follow_module.verified_release_checkpoint(
            current, "oracle", status, intent, receipt, active, pack, mapping)
        self.assertEqual(verified["operation_id"], "op_checkpoint")
        self.assertEqual(verified["mapping_sha256"], mapping_sha)
        with self.assertRaisesRegex(ValueError, "saved_release_checkpoint_mismatch"):
            follow_module.verified_release_checkpoint(
                current, "oracle", status, intent, receipt, {**active, "candidate_id": "stale"}, pack, mapping)
        with self.assertRaisesRegex(ValueError, "saved_release_checkpoint_mismatch"):
            follow_module.verified_release_checkpoint(
                current, "oracle", {**status, "status": "failed"}, intent, receipt, active, pack, mapping)

    def test_oracle_c5_single_table_uses_official_per_table_collection_shape(self):
        follow = follow_module.Follow.__new__(follow_module.Follow)
        follow.c = {"source_id": follow_module.w.SOURCE}
        run_id = str(uuid.uuid4())
        current = {
            "generation": 11,
            "root": None,
            "request": {
                "position_codec": "snapshot-zero/v1",
                "position_lineage": "configured.oracle",
                "applied_pos": "0",
                "consistency_grade": "S0",
                "cross_table_consistent": False,
                "capture_method": "c5_diff",
                "delete_detection": "reconcile",
                "config_digest": "b" * 64,
            },
            "result": {"run_id": run_id},
            "table": {
                "logical_schema": "q_4f4f30345f464958",
                "logical_table": "q_4f4e424f4152445f4f5244455253",
                "stored_physical": "pt_" + "a" * 60,
                "accepted_rows": 4,
                "canonical_digest": "a" * 64,
            },
        }
        with tempfile.TemporaryDirectory() as tmp:
            current["root"] = Path(tmp)
            mapping = follow.publication_context(current)

            receipt = mapping["publication"]["receipt"]
            self.assertEqual(receipt["collection_mode"], "per_table")
            self.assertEqual(receipt["run_ids"], [run_id])
            self.assertEqual(receipt["tables"][0]["run_id"], run_id)
            self.assertEqual(receipt["consistency"], "per_table")
            self.assertEqual(receipt["consistency_grade"], "S0")
            self.assertIs(receipt["cross_table_consistent"], False)
            self.assertEqual(receipt["position_codec"], "snapshot-zero/v1")
            self.assertEqual(receipt["applied_pos"], "0")
            self.assertEqual(receipt["tables"][0]["capture_method"], "c5_diff")
            self.assertEqual(receipt["tables"][0]["delete_detection"], "reconcile")
            self.assertEqual(
                mapping["publication"]["receipt_sha256"],
                hashlib.sha256(follow_module.canonical(receipt)).hexdigest(),
            )
            self.assertEqual(json.loads((Path(tmp) / "publication.json").read_text()), receipt)

    def test_completed_cut_checkpoint_binds_gateway_publication_gold_and_maintenance(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "generation-12"
            gold, product = root / "gold", root / "product"
            gold.mkdir(parents=True)
            product.mkdir()
            run_id = str(uuid.uuid4())
            generation, source_id, binding = 12, "oracle_oneclick_orders", str(uuid.uuid4())
            typed = {"source_id": source_id, "source_generation": generation, "differences": 0,
                     "rows": 4, "cells": 20, "source_gold_sha256": "a" * 64,
                     "target_gold_sha256": "a" * 64}
            iud = {"from_generation": 11, "to_generation": generation, "operations": {"inserted": ["24"]}}
            cut = {"binding": binding, "pin": 1981,
                   "load": {"run_id": run_id, "source_id": source_id,
                            "source_generation": generation, "target_generation": generation}}
            status = {"status": "succeeded", "half_published": False, "candidate_id": "candidate",
                      "version": "candidate", "pack_digest": "b" * 64}
            active = {"candidate_id": "candidate", "version": "candidate", "pack_digest": "b" * 64}
            pack = {"pack_digest": "b" * 64}
            publication = {"source_id": source_id, "source_generation": generation,
                           "target_generation": generation, "run_id": run_id,
                           "consistency_grade": "S0", "position_codec": "snapshot-zero/v1",
                           "tables": [{"schema": "q_schema", "table": "q_table"}]}
            freshness = {
                "publication": "live", "source_time": "unmeasured", "replica_lag": "unmeasured",
                "freshness": [], "capture_method": "c5_diff", "deletion_guarantee": "reconcile",
                "warnings": [], "gap": [{"relation": "source_watermark", "state": "read"}],
                "sources": [{"source": source_id, "consistency": "per_table",
                             "cross_table_consistent": False, "applied_pos": "0"}],
                "mappings": [{"source": source_id, "schema": "q_schema", "table": "q_table",
                              "run_generation": generation, "bound": True}],
                "runs": [{"source": source_id, "source_generation": generation,
                          "target_generation": generation, "state": "published", "sealed": True}],
                "imports": [{"schema": "q_schema", "table": "q_table", "run_generation": generation,
                             "applied_pos": "0", "exhausted": True, "applied_rows": 4}],
                "receipts": [{"source": source_id, "schema": "q_schema", "table": "q_table",
                              "run_generation": generation, "codec": "snapshot-zero/v1", "applied_pos": "0",
                              "cross_table_consistent": False, "partial_txn": False,
                              "target_commit_at": "2026-10-16T02:26:04.994893Z"}],
                "quality_counts": [{"accepted_rows": 4, "rejected_rows": 0}],
            }
            ready = {"protocol": "oo.oracle.automatic-product-follow-gateway/v1", "source_id": source_id,
                     "source_generation": generation, "target_generation": generation, "run_id": run_id,
                     "typed_gold_sha256": None, "freshness_sha256": None, "ready": True}

            def save(path, value):
                path.write_text(json.dumps(value, sort_keys=True))

            save(gold / "acknowledged-cut.json", cut)
            save(gold / "typed-gold.json", typed)
            save(gold / "fixture-iud-diff.json", iud)
            save(product / "maintenance-consumption.json", {"generation": generation})
            save(root / "release-status.json", status)
            save(root / "active-release.json", active)
            save(root / "active-pack.json", pack)
            save(root / "publication.json", publication)
            save(root / "gateway-freshness.json", freshness)
            ready["typed_gold_sha256"] = follow_module.sha(gold / "typed-gold.json")
            ready["freshness_sha256"] = follow_module.sha(root / "gateway-freshness.json")
            save(root / "gateway-ready.json", ready)
            completed = {"protocol": "oo.oracle.product-follow-cut/v1", "source_id": source_id,
                         "binding": binding, "source_generation": generation, "target_generation": generation,
                         "run_id": run_id, "pin": 1981, "ready": True, "iud": iud,
                         "zero_difference_cells": 20, "release_active_checkpoint_sha256": None,
                         "maintenance_sha256": follow_module.sha(product / "maintenance-consumption.json"),
                         "release_status_sha256": follow_module.sha(root / "release-status.json"),
                         "publication_sha256": follow_module.sha(root / "publication.json"),
                         "gateway_ready_sha256": follow_module.sha(root / "gateway-ready.json"),
                         "typed_gold_sha256": follow_module.sha(gold / "typed-gold.json")}
            save(root / "cut-complete.json", completed)

            self.assertEqual(
                follow_module.verified_cut_checkpoint(root, generation, source_id, binding), completed)
            previous_gold = gold / "source-gold.json"
            save(previous_gold, [{"key": "1", "amount": "10.00"}])
            original_evidence = {path.relative_to(root): path.read_bytes()
                                 for path in root.rglob("*") if path.is_file()}

            def binary(name, data, content_addressed=False):
                source = Path(tmp) / (name + ".source")
                source.write_bytes(data)
                digest = follow_module.sha(source)
                target = Path(tmp) / (digest if content_addressed else "bins") / name
                target.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
                target.write_bytes(data)
                return target, digest

            worker, worker_sha = binary("worker", b"worker", True)
            release_binary, release_sha = binary("release", b"release", True)
            api_binary, _ = binary("api", b"api")
            gateway_binary, gateway_sha = binary("gateway", b"gateway")
            token_binary, token_sha = binary("token", b"token")
            cli_binary, cli_sha = binary("cli", b"cli")
            release_private = Path(tmp) / "release-private"
            release_private.mkdir(mode=0o700)
            runtime_base = release_private / "runtime-base.json"
            runtime_base.write_text(json.dumps({"replica_pg": {
                "host": "replica.example", "port": 5432, "database": "previewdb",
                "user": "reader", "ca_file": "/owned/ca.crt"}}))
            config = {
                "initial_after_generation": generation,
                "initial_previous_gold": str(previous_gold),
                "source_id": source_id, "binding": binding, "poll_seconds": 0,
                "expected_period_ms": 300000,
                "native": {"container": "native", "worker_container": "/worker",
                           "worker_config": "/worker.json", "worker_host": str(worker),
                           "worker_sha256": worker_sha, "project_id": "project",
                           "worker_name": "oracle-worker", "serve_seconds": 86400},
                "api": {"container": "api", "binary_host": str(api_binary), "binary_sha256": follow_module.sha(api_binary),
                        "binary_container": "/api", "runtime_container": "/api-runtime.json"},
                "release": {"container": "release", "project_id": "oracle",
                            "private_root": str(release_private), "runtime_base": str(runtime_base),
                            "runtime_container_prefix": "/owned/release/",
                            "binary_host": str(release_binary), "binary_sha256": release_sha,
                            "binary_container": "/release"},
                "gateway": {"gateway_binary": str(gateway_binary), "gateway_binary_sha256": gateway_sha,
                            "token_binary": str(token_binary), "token_binary_sha256": token_sha,
                            "cli_binary": str(cli_binary), "cli_binary_sha256": cli_sha,
                            "standby_host": "replica.example", "standby_container": "standby"},
            }
            probe = {"sequence": 0}

            def fake_subprocess_run(argv, input=None, capture_output=False, timeout=None):
                probe["sequence"] += 1
                sequence = probe["sequence"]
                if argv[:2] == ["docker", "inspect"] and argv[-1] == "native":
                    started = (follow_module.datetime.now(follow_module.timezone.utc)
                               - follow_module.timedelta(days=2)).isoformat().replace("+00:00", "Z")
                    output = [{"State": {"Running": True, "StartedAt": started,
                                          "diagnostic_sequence": sequence}}]
                elif argv[:2] == ["docker", "inspect"] and argv[-1] == "release":
                    output = [{"State": {"Running": True, "diagnostic_sequence": sequence}}]
                elif "native-state" in argv:
                    output = {"protocol": "oo.jobs.native-state/v1", "source_id": source_id,
                              "binding": binding, "source_generation": generation,
                              "applied": True, "diagnostic_sequence": sequence}
                elif len(argv) > 2 and argv[:3] == ["docker", "exec", "native"] and argv[-1] == "/worker":
                    output = [{"pid": 101, "argv": ["/worker", "native-serve", "--project", "project",
                                 "--worker", "oracle-worker", "--seconds", "86400"],
                               "sha256": worker_sha, "exe": "/proc/101/exe"}]
                elif len(argv) > 2 and argv[:3] == ["docker", "exec", "api"]:
                    output = [{"pid": 202, "argv": ["/api", "serve", "--config", "/api-runtime.json"],
                               "sha256": follow_module.sha(api_binary), "exe": "/proc/202/exe"}]
                else:
                    raise AssertionError("unexpected subprocess: " + repr(argv))
                raw = json.dumps(output, sort_keys=True).encode() + b"\n"
                return subprocess.CompletedProcess(argv, 0, stdout=raw, stderr=b"")

            def fake_start_release_api(follow, out, runtime_container, label):
                follow_module.write_json(Path(out) / (label + "-release-process-identity.json"), {
                    "container": "release", "runtime_container": runtime_container,
                    "binary_sha256": release_sha, "diagnostic_sequence": probe["sequence"]})

            def fake_release_flow(follow, out):
                class Flow:
                    def call(self, role, path, payload=None, name="request"):
                        response = {"operation": "getActiveRelease", "half_published": False,
                                    "diagnostic_sequence": probe["sequence"]}
                        follow_module.write_json(Path(out) / (name + ".response.json"),
                                                 {"status": 200, "response": response})
                        return response
                return Flow()

            watermark = {"source_id": source_id, "binding": binding,
                         "source_generation": generation, "target_generation": generation,
                         "run_id": run_id}
            schedule = {"enabled": True, "period_ms": 300000, "pin": 1981,
                        "last_source_generation": generation}
            mark = {"source_generation": generation, "target_generation": generation, "run_id": run_id}
            with patch.object(follow_module.subprocess, "run", side_effect=fake_subprocess_run), \
                 patch.object(follow_module.Follow, "start_release_api", fake_start_release_api), \
                 patch.object(follow_module.Follow, "release_flow", fake_release_flow), \
                 patch.object(follow_module.Follow, "verify_native_state_identity", return_value=watermark), \
                 patch.object(follow_module.Follow, "schedule_and_mark", return_value=(schedule, mark)), \
                 patch.object(follow_module.Follow, "prepare_generation", side_effect=AssertionError("prepare called")), \
                 patch.object(follow_module.Follow, "publish", side_effect=AssertionError("publish called")), \
                 patch.object(follow_module.Follow, "follow_generation", side_effect=AssertionError("generation called")):
                first_result = follow_module.Follow(config, tmp).run()
                first = next((Path(tmp) / "attempts").iterdir())
                second_result = follow_module.Follow(config, tmp).run()
                second = next(path for path in (Path(tmp) / "attempts").iterdir() if path != first)
                resumed = follow_module.Follow(config, tmp).resume_after_maintenance(generation)
                official = next(path for path in (Path(tmp) / "attempts").iterdir()
                                if path not in (first, second))

            self.assertTrue(first_result["full_product_follow_verified"])
            self.assertTrue(second_result["full_product_follow_verified"])
            self.assertEqual(resumed, completed)
            self.assertNotEqual(first, second)
            self.assertTrue((first / "fixed-executables.json").is_file())
            self.assertTrue((second / "fixed-executables.json").is_file())
            self.assertFalse((Path(tmp) / "fixed-executables.json").exists())
            self.assertNotEqual((first / "native-service-inspect.stdout").read_bytes(),
                                (second / "native-service-inspect.stdout").read_bytes())
            first_state = next((first / "initial-state").glob("native-state-*.stdout"))
            second_state = next((second / "initial-state").glob("native-state-*.stdout"))
            self.assertNotEqual(first_state.read_bytes(), second_state.read_bytes())
            for attempt in (first, second):
                self.assertTrue((attempt / "selfbuild-api" / "selfbuild-api-process-identity.json").is_file())
                self.assertTrue((attempt / "release-api" / "release-base-release-process-identity.json").is_file())
                self.assertTrue((attempt / "release-bootstrap" / "active-before-follow.response.json").is_file())
                self.assertFalse((attempt / "generation-13").exists())
                self.assertFalse((attempt / "publication.json").exists())
                self.assertFalse((attempt / "fixture.sql").exists())
            self.assertTrue((official / "resume-native-state").is_dir())
            self.assertTrue((official / "resumed-cut-reference.json").is_file())
            self.assertTrue(json.loads((official / "resumed-cut-reference.json").read_text())["read_only"])
            self.assertEqual({path.relative_to(root): path.read_bytes()
                              for path in root.rglob("*") if path.is_file()}, original_evidence)
            self.assertEqual(follow_module.verified_cut_checkpoint(root, generation, source_id, binding), completed)
            (root / "active-pack.json").write_text(json.dumps({"pack_digest": "c" * 64}))
            with self.assertRaisesRegex(ValueError, "follow_cut_checkpoint_identity"):
                follow_module.verified_cut_checkpoint(root, generation, source_id, binding)

    def test_oracle_freshness_gate_uses_formal_s0_receipt_projection(self):
        source_id, schema, table, generation, rows = "oracle_oneclick_orders", "q_schema", "q_table", 12, 4
        snapshot = {
            "publication": "live", "source_time": "unmeasured", "replica_lag": "unmeasured",
            "freshness": [], "capture_method": "c5_diff", "deletion_guarantee": "reconcile",
            "warnings": [], "gap": [{"relation": "source_watermark", "state": "read"}],
            "sources": [{"source": source_id, "consistency": "per_table",
                         "cross_table_consistent": False, "applied_pos": "0"}],
            "mappings": [{"source": source_id, "schema": schema, "table": table,
                          "run_generation": generation, "bound": True}],
            "runs": [{"source": source_id, "source_generation": generation,
                      "target_generation": generation, "state": "published", "sealed": True}],
            "imports": [{"schema": schema, "table": table, "run_generation": generation,
                         "applied_pos": "0", "exhausted": True, "applied_rows": rows}],
            "receipts": [{"source": source_id, "schema": schema, "table": table,
                          "run_generation": generation, "codec": "snapshot-zero/v1", "applied_pos": "0",
                          "cross_table_consistent": False, "partial_txn": False,
                          "target_commit_at": "2026-10-16T02:26:04.994893Z"}],
            "quality_counts": [{"accepted_rows": rows, "rejected_rows": 0}],
        }
        follow_module.verify_oracle_gateway_freshness(snapshot, source_id, schema, table, generation, rows)
        for section, mutation in (
            ("sources", lambda x: x[0].update(applied_pos="1")),
            ("mappings", lambda x: x[0].update(run_generation=generation - 1)),
            ("runs", lambda x: x[0].update(sealed=False)),
            ("imports", lambda x: x[0].update(exhausted=False)),
            ("receipts", lambda x: x[0].update(partial_txn=True)),
        ):
            bad = json.loads(json.dumps(snapshot))
            mutation(bad[section])
            with self.assertRaisesRegex(ValueError, "gateway_freshness_not_current"):
                follow_module.verify_oracle_gateway_freshness(bad, source_id, schema, table, generation, rows)
        bad = json.loads(json.dumps(snapshot))
        bad["warnings"] = [{"code": "STALE_DATA"}]
        with self.assertRaisesRegex(ValueError, "gateway_freshness_not_current"):
            follow_module.verify_oracle_gateway_freshness(bad, source_id, schema, table, generation, rows)


class QuietSamplingTests(unittest.TestCase):
    def test_quiet_evidence_waits_for_exact_schedule_pin(self):
        with tempfile.TemporaryDirectory() as tmp:
            follow = follow_module.Follow.__new__(follow_module.Follow)
            follow.c = {"expected_period_ms": 6000, "binding": "binding",
                        "native": {"container": "native"}}
            outputs = [
                b"complete=nochange binding=binding pin=2335 task=old generation=1 source_generation=12\n",
                b"complete=nochange binding=binding pin=2336 task=current generation=1 source_generation=12\n",
            ]
            with patch.object(follow_module, "command", side_effect=outputs) as run, \
                 patch.object(follow_module.time, "sleep"):
                row = follow.record_quiet(
                    {"enabled": True, "period_ms": 6000, "pin": 2336, "last_source_generation": 12},
                    {"source_generation": 12, "target_generation": 12, "run_id": "run"}, tmp)
            self.assertEqual(run.call_count, 2)
            self.assertEqual(row["schedule_pin"], 2336)
            self.assertIn("pin=2336", row["nochange_line"])

    def test_quiet_sampling_skips_a_non_nochange_scheduled_run(self):
        with tempfile.TemporaryDirectory() as tmp:
            follow = follow_module.Follow.__new__(follow_module.Follow)
            follow.c = {"expected_period_ms": 6000, "binding": "binding",
                        "native": {"container": "native"}}
            with patch.object(follow_module, "command", return_value=b"complete=succeeded binding=binding pin=2336 task=current generation=13 load_state=awaiting_publish applied_pos=0\n"):
                row = follow.record_quiet(
                    {"enabled": True, "period_ms": 6000, "pin": 2336, "last_source_generation": 12},
                    {"source_generation": 12, "target_generation": 12, "run_id": "run"}, tmp)
            self.assertIsNone(row)
            self.assertFalse((Path(tmp) / "quiet-pin-2336.json").exists())


class ResumedCutTests(unittest.TestCase):
    def test_resumed_cut_uses_database_watermark_run_id_not_absent_native_state_field(self):
        source_id, binding, run_id, generation = "oracle_oneclick_orders", str(uuid.uuid4()), str(uuid.uuid4()), 12
        state = {"protocol": "oo.jobs.native-state/v1", "source_id": source_id,
                 "binding": binding, "source_generation": generation, "applied": True}
        watermark = {"source_id": source_id, "binding": binding, "source_generation": generation,
                     "target_generation": generation, "run_id": run_id}
        completed = {"source_id": source_id, "binding": binding, "source_generation": generation,
                     "target_generation": generation, "run_id": run_id}
        self.assertNotIn("run_id", state)
        follow_module.verify_resumed_cut_watermark(state, watermark, completed, generation)

        for field, value in (("run_id", str(uuid.uuid4())), ("target_generation", generation - 1),
                             ("source_generation", generation - 1), ("binding", str(uuid.uuid4()))):
            bad = dict(watermark, **{field: value})
            with self.subTest(field=field), self.assertRaisesRegex(
                    ValueError, "follow_cut_checkpoint_current_state_mismatch"):
                follow_module.verify_resumed_cut_watermark(state, bad, completed, generation)

        behind = dict(state, source_generation=generation - 1)
        with self.assertRaisesRegex(ValueError, "follow_cut_checkpoint_current_state_mismatch"):
            follow_module.verify_resumed_cut_watermark(behind, watermark, completed, generation)


if __name__ == "__main__":
    unittest.main()
