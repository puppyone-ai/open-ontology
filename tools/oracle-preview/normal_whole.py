#!/usr/bin/env python3
"""Source-owned normal Oracle whole-run steps for the six-field entry.

The generation-15 source, recovery, publication, and Gateway proofs are immutable
inputs.  This runner performs a fresh official product-flow maintenance call and
two new random, tool-grounded Agent dialogues without writing to Oracle.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import stat
import subprocess
import sys
import time
import uuid
from decimal import Decimal


PLAN = Path("/private/tmp/oo-oracle-c5-luna-max-20261016/normal-whole-g15/plan-final.json")
PROTOCOL = "oo.oracle.normal-whole/v1"
SOURCE_PROTOCOL = "oo.oracle.normal-whole-source-proof/v1"
REQUIRED_TOOLS = {"query", "freshness", "describe", "knowledge_cat"}


class Refused(ValueError):
    pass


def need(condition, reason):
    if not condition:
        raise Refused(reason)


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def private(path):
    path = Path(path)
    st = path.lstat()
    need(stat.S_ISREG(st.st_mode) and not stat.S_ISLNK(st.st_mode), "regular_file")
    need(st.st_uid == os.getuid() and not st.st_mode & 0o077, "private_file")
    return path


def immutable_regular(path):
    path = Path(path)
    st = path.lstat()
    need(stat.S_ISREG(st.st_mode) and not stat.S_ISLNK(st.st_mode), "immutable_file")
    need(st.st_uid == os.getuid() and not st.st_mode & 0o022, "immutable_file")
    return path


def read_json(path):
    return json.loads(Path(path).read_text())


def write_json(path, value):
    path = Path(path)
    need(not path.exists() and not path.is_symlink(), "immutable_output_drift")
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    with path.open("x") as out:
        os.chmod(path, 0o600)
        json.dump(value, out, indent=2, sort_keys=True)
        out.write("\n")
        out.flush()
        os.fsync(out.fileno())


def load_plan():
    plan_path = private(PLAN)
    plan = read_json(plan_path)
    need(plan.get("protocol") == SOURCE_PROTOCOL, "source_plan_protocol")
    for item in plan.get("immutable_files", []):
        path = immutable_regular(item["path"])
        need(sha(path) == item["sha256"], "source_proof_drift")
    runtime = plan.get("runtime")
    need(isinstance(runtime, dict), "source_plan_runtime")
    for key in ("python", "product_flow", "dialogue", "product_config", "onboard", "operator", "exporter",
                "gateway_cli", "gateway_token", "model_bearer"):
        need(isinstance(runtime.get(key), str) and Path(runtime[key]).is_absolute(), "source_plan_runtime")
    for key in ("python_sha256", "product_flow_sha256", "dialogue_sha256", "product_config_sha256", "onboard_sha256",
                "operator_sha256", "exporter_sha256", "gateway_cli_sha256"):
        need(re.fullmatch(r"[a-f0-9]{64}", runtime.get(key, "")) is not None, "source_plan_runtime")
    return plan


def check_sha(path, expected, reason):
    path = Path(path).resolve(strict=True)
    need(path.is_file() and not path.is_symlink() and sha(path) == expected, reason)


def verify_recovery(ack, recovery_wait, recovery_kill, restart, expected_process_sha256):
    try:
        killed_task = json.loads(recovery_kill["task_row"])["id"]
    except (KeyError, TypeError, json.JSONDecodeError) as exc:
        raise Refused("recovery_task_identity") from exc
    task_id = ack.get("task")
    need(task_id == recovery_wait.get("task", {}).get("id") == killed_task, "recovery_task_identity")
    need(recovery_wait.get("task", {}).get("attempt") == 2, "recovery_wait_attempt")
    need(recovery_kill.get("ungranted_full_import_locks") == "0" and recovery_kill.get("active_native_runtime_queries") == "0", "recovery_kill_release")
    need(restart.get("same_container_id") is True and restart.get("configured_seconds") == 86400, "worker_restart")
    need(restart.get("process_sha256") == expected_process_sha256, "worker_restart")
    need(ack.get("task_state") == "succeeded" and ack.get("lease_generation") == 4, "acknowledged_cut")
    need(ack.get("load", {}).get("loader_invoked") is True and ack["load"].get("mirror_committed") is True, "acknowledged_cut")
    need(ack.get("load", {}).get("jobs_lease_receipt", {}).get("generation") == ack["lease_generation"], "acknowledged_cut")
    return task_id


def verify_active_release(active_release, active_pack, ready):
    need(active_release.get("contract") == "oo.release-control-api/v1" and
         active_release.get("operation") == "getActiveRelease" and
         active_release.get("project_id") == "oracle", "active_release")
    need(active_pack.get("contract") == active_release.get("contract") and
         active_pack.get("operation") == "getActiveRolePack" and
         active_pack.get("project_id") == active_release.get("project_id"), "active_release")
    candidate = active_release.get("candidate_id")
    version = active_release.get("version")
    manifest = active_release.get("manifest_sha256")
    pack_digest = active_release.get("pack_digest")
    need(isinstance(candidate, str) and candidate and
         candidate == active_pack.get("candidate_id") == ready.get("release_version"), "active_release")
    need(isinstance(version, str) and version == active_pack.get("version") == ready.get("release_version"), "active_release")
    need(isinstance(manifest, str) and re.fullmatch(r"[a-f0-9]{64}", manifest) and
         manifest == active_pack.get("manifest_sha256"), "active_release")
    need(isinstance(pack_digest, str) and re.fullmatch(r"[a-f0-9]{64}", pack_digest) and
         pack_digest == active_pack.get("pack_digest") == ready.get("pack_digest"), "active_release")
    need(active_release.get("certificate_class") == "test_scope_not_production" and
         active_release.get("production_ready") is False and
         active_release.get("customer_gate_verified") is False, "active_release_scope")
    return candidate


def verify_source_identity(plan):
    source = plan["source"]
    refs = {item["name"]: item for item in plan["immutable_files"]}
    cut = read_json(refs["cut_complete"]["path"])
    typed = read_json(refs["typed_gold"]["path"])
    source_gold = read_json(refs["source_gold"]["path"])
    ready = read_json(refs["gateway_ready"]["path"])
    ack = read_json(refs["acknowledged_cut"]["path"])
    recovery_wait = read_json(refs["loader_wait"]["path"])
    recovery_kill = read_json(refs["post_kill_unlock"]["path"])
    restart = read_json(refs["worker_restart"]["path"])
    timestamp = read_json(refs["timestamp_equivalence"]["path"])
    preflight = read_json(refs["current_cli_preflight"]["path"])
    maintenance = read_json(refs["maintenance_proof"]["path"])
    publication = read_json(refs["publication"]["path"])
    target_gold = read_json(refs["target_gold"]["path"])
    active_release = read_json(refs["active_release"]["path"])
    active_pack = read_json(refs["active_pack"]["path"])

    need(source.get("source_id") == "oracle_oneclick_orders" and source.get("generation") == 15, "source_identity")
    need(cut.get("protocol") == "oo.oracle.product-follow-cut/v1" and cut.get("ready") is True, "source_cut")
    need(cut.get("source_id") == source["source_id"] and cut.get("source_generation") == source["generation"] == cut.get("target_generation"), "source_cut")
    need(cut.get("run_id") == source.get("run_id") and cut.get("zero_difference_cells") == 20, "source_cut")
    need(cut.get("iud", {}).get("from_generation") == 14 and cut.get("iud", {}).get("to_generation") == 15 and
         cut.get("iud", {}).get("operations") == source.get("iud_operations") and
         cut.get("iud", {}).get("typed_cell_differences") == 0, "source_iud")
    need(typed.get("source_id") == source["source_id"] and typed.get("source_generation") == source["generation"], "typed_gold")
    need(typed.get("rows") == 4 and typed.get("cells") == 20 and typed.get("differences") == 0 and typed.get("reader_exit") == 0, "typed_gold")
    source_gold_sha = sha(refs["source_gold"]["path"])
    target_gold_sha = sha(refs["target_gold"]["path"])
    need(len(source_gold) == typed.get("rows") and source_gold_sha == source["source_gold"]["sha256"], "source_gold")
    need(source_gold == target_gold and typed.get("source_gold_sha256") == source_gold_sha and
         typed.get("target_gold_sha256") == target_gold_sha, "typed_gold")
    need(cut.get("typed_gold_sha256") == sha(refs["typed_gold"]["path"]), "source_cut")
    need(ready.get("ready") is True and ready.get("source_generation") == 15 and ready.get("target_generation") == 15, "gateway_ready")
    need(ready.get("run_id") == source["run_id"] and ready.get("source_id") == source["source_id"], "gateway_ready")
    need(ack.get("source_generation") == 15, "acknowledged_cut")
    need(verify_recovery(ack, recovery_wait, recovery_kill, restart, plan["service"]["worker_sha256"]) == source["task_id"], "recovery_task_identity")
    need(timestamp.get("all_values_semantically_equal") is True and timestamp.get("source_time") == "unmeasured" and timestamp.get("replica_lag") == "unmeasured", "timestamp_projection")
    need(preflight.get("identity", {}).get("source_generation") == 15 and preflight.get("identity", {}).get("gateway_process_sha256") == ready.get("gateway_process_sha256"), "current_cli_preflight")
    need(preflight.get("identity", {}).get("gateway_token_sha256") == plan["service"].get("gateway_token_sha256"), "current_cli_preflight")
    need(preflight.get("identity", {}).get("source_time") == "unmeasured" and preflight.get("identity", {}).get("replica_lag") == "unmeasured", "current_cli_preflight")
    need(preflight.get("public_cli", {}).get("warnings") == [] and preflight.get("public_cli", {}).get("full_projection_cells") == 20, "current_cli_preflight")
    need(maintenance.get("source_generation") == 15 and maintenance.get("zero_model_calls") is True, "maintenance_proof")
    need(maintenance.get("pack_digest") == ready.get("pack_digest"), "maintenance_proof")
    need(publication.get("source_generation") == 15 and publication.get("target_generation") == 15, "publication")
    need(publication.get("consistency_grade") == "S0" and publication.get("collection_mode") == "per_table", "publication")
    need(publication.get("run_id") == source["run_id"] and publication.get("source_id") == source["source_id"], "publication")
    published_tables = publication.get("tables")
    need(isinstance(published_tables, list) and len(published_tables) == 1 and
         published_tables[0].get("capture_method") == "c5_diff" and
         published_tables[0].get("consistency_grade") == "S0" and
         published_tables[0].get("accepted_rows") == 4, "publication")
    need(cut.get("publication_sha256") == sha(refs["publication"]["path"]) and
         cut.get("maintenance_sha256") == sha(refs["product_maintenance_consumption"]["path"]) and
         cut.get("gateway_ready_sha256") == sha(refs["gateway_ready"]["path"]), "source_cut")
    need(maintenance.get("source_id") == source["source_id"] and maintenance.get("run_id") == source["run_id"] and
         maintenance.get("source_capture_method") == "C5" and maintenance.get("pack_capture_method") == "c5_diff", "maintenance_proof")
    need(maintenance.get("iud", {}).get("operations") == source.get("iud_operations") and
         maintenance.get("iud", {}).get("typed_cell_differences") == 0, "maintenance_proof")
    verify_active_release(active_release, active_pack, ready)
    return {"cut": cut, "typed_gold": typed, "ready": ready, "ack": ack, "timestamp": timestamp,
            "preflight": preflight, "maintenance": maintenance, "publication": publication,
            "active_release": active_release, "active_pack": active_pack}


def make_dialogue_materials(rows):
    by_key = {str(row["key"]): row for row in rows}
    need(set(by_key) == {"1", "25", "26", "27"}, "source_gold_rows")
    amount = lambda key: format(Decimal(by_key[key]["amount"]), "f")
    label27 = by_key["27"]["label"]
    need(isinstance(label27, str) and label27, "source_gold_label")
    nonnull_labels = str(sum(row.get("label") is not None for row in rows))
    negative_rows = [row for row in rows if Decimal(row["amount"]) < 0]
    amount_sum = format(sum((Decimal(row["amount"]) for row in rows), Decimal(0)), "f")
    label_values = {"order_27_label": label27, "order_26_amount": amount("26"),
                    "row_count": str(len(rows)), "labeled_count": nonnull_labels}
    label_bank = [
        {"semantic_id": "g15a_order_27_label", "query_class": "row_fact", "prompt": "What is the exact label stored on order 27 in the public orders table?", "truth_key": "order_27_label"},
        {"semantic_id": "g15a_order_26_amount", "query_class": "row_fact", "prompt": "What is the exact amount stored on order 26 in the public orders table?", "truth_key": "order_26_amount"},
        {"semantic_id": "g15a_public_order_count", "query_class": "aggregate", "prompt": "How many orders are currently present in the public orders table?", "truth_key": "row_count"},
        {"semantic_id": "g15a_orders_with_labels", "query_class": "aggregate", "prompt": "How many public orders have a populated label?", "truth_key": "labeled_count"},
    ]
    amount_values = {"order_25_amount": amount("25"), "order_27_amount": amount("27"),
                     "negative_amount_count": str(len(negative_rows)), "total_amount": amount_sum}
    amount_bank = [
        {"semantic_id": "g15b_order_25_amount", "query_class": "row_fact", "prompt": "What is the exact amount stored on order 25 in the public orders table?", "truth_key": "order_25_amount"},
        {"semantic_id": "g15b_order_27_amount", "query_class": "row_fact", "prompt": "What is the exact amount stored on order 27 in the public orders table?", "truth_key": "order_27_amount"},
        {"semantic_id": "g15b_negative_order_count", "query_class": "aggregate", "prompt": "How many public orders have an amount below zero?", "truth_key": "negative_amount_count"},
        {"semantic_id": "g15b_total_amount", "query_class": "aggregate", "prompt": "What is the exact total of AMOUNT across all public orders?", "truth_key": "total_amount"},
    ]
    gold_a = {key: {"expected_named_facts": {key: value}} for key, value in label_values.items()}
    gold_b = {key: {"expected_named_facts": {key: value}} for key, value in amount_values.items()}
    for bank, gold in ((label_bank, gold_a), (amount_bank, gold_b)):
        need(len({json.dumps(gold[item["truth_key"]], sort_keys=True) for item in bank}) == 4, "dialogue_truth_diversity")
        need(len({item["query_class"] for item in bank}) == 2, "dialogue_query_classes")
    return ((label_bank, gold_a), (amount_bank, gold_b))


def run_cli(plan, args, timeout=60):
    runtime = plan["runtime"]
    service = plan["service"]
    docker = runtime["docker"]["path"]
    common = ["--json", "--gateway", runtime["gateway_endpoint"], "--transport", "local-development",
              "--auth", "external", "--token-file", runtime["gateway_token"]]
    command = [docker, "run", "--rm", "--network", f"container:{service['gateway_container']}",
               "-v", f"{runtime['gateway_cli']}:/oo-cli:ro",
               "-v", f"{service['gateway_private_root']}:/gw:ro",
               "-v", f"{service['gateway_private_root']}:{service['gateway_private_root']}:ro",
               "rust:1.94.0", "/oo-cli", *args, *common]
    proc = subprocess.run(command, capture_output=True, text=True, timeout=timeout)
    need(proc.returncode == 0, "gateway_preflight")
    try:
        body = json.loads(proc.stdout)
    except json.JSONDecodeError as exc:
        raise Refused("gateway_preflight_json") from exc
    return body, proc.stderr


def write_cli_wrapper(plan, run):
    runtime, service = plan["runtime"], plan["service"]
    docker = runtime["docker"]["path"]
    argv = [docker, "run", "--rm", "--network", f"container:{service['gateway_container']}",
            "-v", f"{runtime['gateway_cli']}:/oo-cli:ro",
            "-v", f"{service['gateway_private_root']}:/gw:ro",
            "-v", f"{service['gateway_private_root']}:{service['gateway_private_root']}:ro",
            "rust:1.94.0", "/oo-cli", '"$@"']
    # Keep caller arguments as one shell expansion; the container receives the same token path.
    rendered = " ".join(shlex.quote(arg) if arg != '"$@"' else '"$@"' for arg in argv)
    path = Path(run) / "oo-cli"
    need(not path.exists() and not path.is_symlink(), "immutable_output_drift")
    path.write_text("#!/bin/sh\nset -eu\nexec " + rendered + "\n")
    path.chmod(0o700)
    return path


def check_runtime(plan):
    runtime = plan["runtime"]
    check_sha(runtime["python"], runtime["python_sha256"], "python_pin")
    check_sha(runtime["product_flow"], runtime["product_flow_sha256"], "product_flow_pin")
    check_sha(runtime["dialogue"], runtime["dialogue_sha256"], "dialogue_pin")
    check_sha(runtime["product_config"], runtime["product_config_sha256"], "product_config_pin")
    check_sha(runtime["onboard"], runtime["onboard_sha256"], "onboard_pin")
    check_sha(runtime["operator"], runtime["operator_sha256"], "operator_pin")
    check_sha(runtime["exporter"], runtime["exporter_sha256"], "exporter_pin")
    check_sha(runtime["gateway_cli"], runtime["gateway_cli_sha256"], "gateway_cli_pin")
    private(runtime["gateway_token"])
    private(runtime["model_bearer"])
    product_config = read_json(runtime["product_config"])
    need(product_config.get("operator_sha256") == runtime["operator_sha256"] and product_config.get("export_oos_sha256") == runtime["exporter_sha256"], "product_dependency_pin")
    need(sha(runtime["gateway_token"]) == plan["service"]["gateway_token_sha256"], "gateway_token_identity")
    listen = Path(plan["service"]["gateway_private_root"]) / "listen.addr"
    immutable_regular(listen)
    need("http://" + listen.read_text().strip() == runtime["gateway_endpoint"], "gateway_endpoint")
    need(Path(sys.executable).resolve() == Path(runtime["python"]).resolve(), "python_runtime")
    docker = runtime["docker"]
    check_sha(docker["path"], docker["sha256"], "docker_pin")
    image = subprocess.run([docker["path"], "image", "inspect", plan["service"]["cli_image"], "--format", "{{.Id}}"], capture_output=True, text=True, timeout=30)
    need(image.returncode == 0 and image.stdout.strip() == plan["service"]["cli_image_id"], "cli_image")
    inspect = subprocess.run([docker["path"], "inspect", "--format", "{{.Id}}|{{.State.Running}}", plan["service"]["gateway_container"]], capture_output=True, text=True, timeout=30)
    need(inspect.returncode == 0, "gateway_process")
    parts = inspect.stdout.strip().split("|")
    need(parts == [plan["service"]["gateway_container_id"], "true"], "gateway_process")
    proc = subprocess.run([docker["path"], "exec", plan["service"]["gateway_container"], "sha256sum", "/proc/1/exe"], capture_output=True, text=True, timeout=30)
    need(proc.returncode == 0 and proc.stdout.split()[0] == plan["service"]["gateway_process_sha256"], "gateway_elf")


def prepare(config_path, run):
    run = Path(run).resolve()
    need(run.is_dir() and not run.is_symlink() and run.stat().st_uid == os.getuid() and not run.stat().st_mode & 0o077, "run_permissions")
    config = read_json(private(config_path))
    plan = load_plan()
    check_runtime(plan)
    identity = verify_source_identity(plan)
    for field, expected in plan["connection"].items():
        need(config.get(field) == expected, "owned_connection_scope")
    private(config["password_file"])
    wrapper = write_cli_wrapper(plan, run)

    runtime = plan["runtime"]
    live_freshness, freshness_stderr = run_cli(plan, ["freshness"])
    live_tables, tables_stderr = run_cli(plan, ["tables"])
    need(live_freshness.get("warnings") == [] and live_freshness.get("source_time") == "unmeasured" and live_freshness.get("replica_lag") == "unmeasured", "live_freshness")
    need(live_freshness.get("publication") == "live", "live_freshness")
    need(any(row.get("source") == plan["source"]["source_id"] and row.get("run_generation") == 15 and row.get("bound") is True for row in live_freshness.get("mappings", [])), "live_generation")
    need(isinstance(live_tables, (list, dict)), "live_tables")
    expected_pack = identity["ready"].get("role_pack_sha256")
    gateway_pack = Path(plan["service"]["gateway_private_root"]) / "packs" / "oo_r_oracle_orders.json"
    need(sha(gateway_pack) == expected_pack, "live_pack")
    preflight = {"protocol": "oo.oracle.normal-whole-preflight/v1", "source_id": plan["source"]["source_id"],
                 "source_generation": 15, "gateway_container_id": plan["service"]["gateway_container_id"],
                 "gateway_process_sha256": plan["service"]["gateway_process_sha256"],
                 "cli_sha256": runtime["gateway_cli_sha256"], "role_pack_sha256": expected_pack,
                 "cli_wrapper_sha256": sha(wrapper),
                 "pack_digest": identity["ready"]["pack_digest"], "freshness": live_freshness,
                 "tables_sha256": hashlib.sha256(json.dumps(live_tables, sort_keys=True).encode()).hexdigest(),
                 "freshness_stderr_sha256": hashlib.sha256(freshness_stderr.encode()).hexdigest(),
                 "tables_stderr_sha256": hashlib.sha256(tables_stderr.encode()).hexdigest(),
                 "source_time": "unmeasured", "replica_lag": "unmeasured"}
    write_json(run / "preflight.json", preflight)
    rows = read_json(plan["source"]["source_gold"]["path"])
    ((bank_a, gold_a), (bank_b, gold_b)) = make_dialogue_materials(rows)
    for name, value in (("dialogue-a-bank.json", bank_a), ("dialogue-a-gold.json", gold_a),
                        ("dialogue-b-bank.json", bank_b), ("dialogue-b-gold.json", gold_b)):
        write_json(run / name, value)
    return {"preflight": preflight, "dialogs_prepared": 2, "source_generation": 15}


def prepare_dialogues(run):
    run = Path(run).resolve()
    plan = load_plan()
    pack = Path(plan["service"]["gateway_private_root"]) / "packs" / "oo_r_oracle_orders.json"
    pack_sha = sha(pack)
    need(pack_sha == plan["service"]["role_pack_sha256"], "live_pack")
    product = read_json(run / "product" / "product-receipt.json")
    consumption = read_json(run / "product" / "maintenance-consumption.json")
    current_pack = run / "product" / "current-role-pack.json"
    need(product.get("source_fresh") is True and product.get("fixture_review") is True, "product_receipt")
    need({item.get("stage") for item in product.get("stages", []) if item.get("status") == "succeeded"} >= {f"S{i}" for i in range(11)}, "product_stages")
    need(consumption.get("source_id") == plan["source"]["source_id"] and consumption.get("source_generation") == 15, "product_generation")
    need(sha(current_pack) == pack_sha and consumption.get("role_pack_file_sha256") == pack_sha, "current_pack")
    need(consumption.get("pack_digest") == plan["service"]["pack_digest"], "current_pack")
    for suffix in ("a", "b"):
        config = {"question_bank": str(run / f"dialogue-{suffix}-bank.json"),
                  "independent_truth_file": str(run / f"dialogue-{suffix}-gold.json"),
                  "sample_size": 2, "history": [], "model": "grok-4.7",
                  "model_origin": plan["runtime"]["model_origin"],
                  "model_bearer_file": plan["runtime"]["model_bearer"],
                  "gateway": plan["runtime"]["gateway_endpoint"], "transport": "local-development",
                  "auth": "external", "gateway_token_file": plan["runtime"]["gateway_token"],
                  "oo_cli": str(run / "oo-cli"),
                  "independent_gateway_audit_file": str(Path(plan["service"]["gateway_private_root"]) / "wal"),
                  "max_tool_rounds": 16,
                  "identity": {"source_id": plan["source"]["source_id"], "source_generation": 15,
                               "pack_sha256": pack_sha, "role": "oo_r_oracle_orders"}}
        write_json(run / f"dialogue-{suffix}-config.json", config)
    write_json(run / "product-identity.json", {"build_id": product["build_id"], "product_receipt_sha256": sha(run / "product" / "product-receipt.json"),
                                                "maintenance_consumption_sha256": sha(run / "product" / "maintenance-consumption.json"),
                                                "role_pack_sha256": pack_sha, "pack_digest": consumption["pack_digest"],
                                                "source_id": consumption["source_id"], "source_generation": consumption["source_generation"]})
    return {"dialogues": 2, "source_generation": 15, "role_pack_sha256": pack_sha}


def verify_dialogue_folder(folder, source, pack_sha, truth_path):
    folder = Path(folder)
    selection = read_json(folder / "selection.json")
    verdict = read_json(folder / "verdict.json")
    transcript = read_json(folder / "transcript.json")
    truth_sha = sha(truth_path)
    need(selection.get("mode") == "random" and selection.get("counts_as_random") is True, "dialogue_random")
    need(selection.get("gold_sha256") == verdict.get("gold_sha256") == verdict.get("truth_sha256") == truth_sha and
         selection.get("seed") == verdict.get("seed"), "dialogue_truth")
    ids = selection.get("semantic_ids")
    need(isinstance(ids, list) and len(ids) == 2 and len(set(ids)) == 2, "dialogue_random")
    identity = selection.get("identity", {})
    need(identity.get("source_id") == source["source_id"] and identity.get("source_generation") == 15, "dialogue_identity")
    need(identity.get("pack_sha256") == pack_sha and identity.get("role") == "oo_r_oracle_orders", "dialogue_identity")
    need(verdict.get("outcome") == "verified" and verdict.get("model") == "grok-4.7" and verdict.get("reasoning_effort") == "low" and verdict.get("turns") == 2, "dialogue_verdict")
    need(verdict.get("selection") == "random" and verdict.get("counts_as_random") is True and verdict.get("semantic_ids") == ids, "dialogue_verdict")
    need(REQUIRED_TOOLS <= set(verdict.get("tools", [])), "dialogue_tools")
    qids = verdict.get("query_ids")
    need(isinstance(qids, list) and len(qids) >= 2 and len(qids) == len(set(qids)), "dialogue_qids")
    need(all(isinstance(event, dict) and isinstance(event.get("turn"), int) for event in transcript), "dialogue_transcript")
    transcript_text = json.dumps(transcript, sort_keys=True)
    need(all(qid in transcript_text for qid in qids), "dialogue_transcript")
    return {"selection_sha256": sha(folder / "selection.json"),
            "verdict_sha256": sha(folder / "verdict.json"),
            "transcript_sha256": sha(folder / "transcript.json"),
            "semantic_ids": ids, "seed": selection["seed"], "query_ids": qids,
            "audit_sha256": verdict["audit_sha256"], "truth_sha256": verdict["truth_sha256"], "turns": verdict["turns"]}


def record_dialogue_attempt_failure(attempt, run, suffix, source, pack_sha, *, kind, exit_code,
                                   timed_out, stdout, stderr, reason=None):
    output = {}
    for name, data in (("runner.stdout", stdout), ("runner.stderr", stderr)):
        if isinstance(data, bytes):
            data = data.decode("utf-8", "replace")
        path = Path(attempt) / name
        path.write_text(data or "")
        path.chmod(0o600)
        output[name.replace(".", "_") + "_sha256"] = sha(path)
    truth = Path(run) / f"dialogue-{suffix}-gold.json"
    fields = {"selection_sha256": "selection.json", "transcript_sha256": "transcript.json",
              "verdict_sha256": "verdict.json"}
    outputs = {key: sha(Path(attempt) / rel) if (Path(attempt) / rel).is_file() else None
               for key, rel in fields.items()}
    write_json(Path(attempt) / "failed-attempt.json", {
        "protocol": "oo.oracle.dialogue-failed-attempt/v1", "failure_kind": kind,
        "exit_code": exit_code, "timed_out": timed_out, "reason": reason,
        "source_id": source["source_id"], "source_generation": 15, "pack_sha256": pack_sha,
        "truth_sha256": sha(truth), **outputs, **output})


def run_dialogue(run, suffix):
    run = Path(run).resolve()
    plan = load_plan()
    source = plan["source"]
    pack_sha = plan["service"]["role_pack_sha256"]
    assert_fresh_dialogue_slot(run, suffix)
    attempts_root = run / "attempts"
    prefix = f"dialogue-{suffix}-"
    attempt_rel = Path("attempts") / f"dialogue-{suffix}-{time.time_ns()}-{uuid.uuid4().hex[:8]}"
    attempt = run / attempt_rel
    attempt.mkdir(mode=0o700, parents=True, exist_ok=False)
    command = [plan["runtime"]["python"], plan["runtime"]["dialogue"], "--config",
               str(run / f"dialogue-{suffix}-config.json"), "--out", str(attempt)]
    try:
        proc = subprocess.run(command, capture_output=True, text=True, timeout=1800)
    except subprocess.TimeoutExpired as exc:
        record_dialogue_attempt_failure(attempt, run, suffix, source, pack_sha, kind="timeout",
                                        exit_code=None, timed_out=True, stdout=exc.stdout,
                                        stderr=exc.stderr, reason="dialogue_timeout")
        raise Refused("dialogue_timeout") from exc
    if proc.returncode != 0:
        record_dialogue_attempt_failure(attempt, run, suffix, source, pack_sha, kind="exit_nonzero",
                                        exit_code=proc.returncode, timed_out=False,
                                        stdout=proc.stdout, stderr=proc.stderr)
        need(False, "dialogue_exit")
    for name, data in (("runner.stdout", proc.stdout), ("runner.stderr", proc.stderr)):
        path = attempt / name
        path.write_text(data)
        path.chmod(0o600)
    need(proc.returncode == 0, "dialogue_exit")
    try:
        actual = verify_dialogue_folder(attempt, source, pack_sha, run / f"dialogue-{suffix}-gold.json")
    except (Refused, KeyError, TypeError, ValueError) as exc:
        write_json(attempt / "invalid-attempt.json", {
            "protocol": "oo.oracle.dialogue-invalid-attempt/v1", "reason": str(exc),
            "error_class": type(exc).__name__, "exit_code": proc.returncode, "timed_out": False,
            "source_id": source["source_id"], "source_generation": 15, "pack_sha256": pack_sha,
            "truth_sha256": sha(run / f"dialogue-{suffix}-gold.json"),
            "selection_sha256": sha(attempt / "selection.json") if (attempt / "selection.json").is_file() else None,
            "transcript_sha256": sha(attempt / "transcript.json") if (attempt / "transcript.json").is_file() else None,
            "verdict_sha256": sha(attempt / "verdict.json") if (attempt / "verdict.json").is_file() else None,
            "runner_stdout_sha256": sha(attempt / "runner.stdout"),
            "runner_stderr_sha256": sha(attempt / "runner.stderr")})
        raise
    canonical = run / f"dialogue-{suffix}"
    staging = run / f".{canonical.name}-{uuid.uuid4().hex}.tmp"
    staging.mkdir(mode=0o700)
    try:
        for name in ("selection.json", "transcript.json", "verdict.json"):
            src, dst = attempt / name, staging / name
            need(src.is_file() and not src.is_symlink(), "dialogue_artifact")
            shutil.copyfile(src, dst)
            dst.chmod(0o600)
        os.rename(staging, canonical)
    except BaseException:
        shutil.rmtree(staging, ignore_errors=True)
        raise
    write_json(run / f"dialogue-{suffix}-attempt.json", {"attempt": str(attempt_rel), "artifacts": actual})
    return {"dialogue": suffix, "attempt": str(attempt_rel), "query_count": len(actual["query_ids"])}


def assert_fresh_dialogue_slot(run, suffix):
    need(suffix in {"a", "b"}, "dialogue_suffix")
    run = Path(run)
    attempts_root = run / "attempts"
    prior = sorted(path for path in attempts_root.glob(f"dialogue-{suffix}-*") if path.is_dir()) if attempts_root.exists() else []
    need(not prior and not (run / f"dialogue-{suffix}").exists(), "prior_dialogue_attempt_requires_review")


def verify_dialogue(run, suffix, source, pack_sha):
    folder = Path(run) / f"dialogue-{suffix}"
    result = verify_dialogue_folder(folder, source, pack_sha, Path(run) / f"dialogue-{suffix}-gold.json")
    pointer = read_json(Path(run) / f"dialogue-{suffix}-attempt.json")
    attempt = Path(run) / pointer["attempt"]
    need(attempt.is_dir() and not attempt.is_symlink(), "dialogue_attempt")
    for name in ("selection.json", "transcript.json", "verdict.json"):
        need(sha(attempt / name) == pointer["artifacts"][name.replace(".json", "_sha256")], "dialogue_attempt")
        need(sha(folder / name) == pointer["artifacts"][name.replace(".json", "_sha256")], "dialogue_copy")
    need(result["query_ids"], "dialogue_qids")
    result.update(actual_output_path=str(pointer["attempt"]),
                  actual_output_artifact_sha256s=pointer["artifacts"],
                  actual_output_sha256=pointer["artifacts"]["transcript_sha256"])
    return result


def close(run):
    run = Path(run).resolve()
    plan = load_plan()
    identity = verify_source_identity(plan)
    preflight = read_json(run / "preflight.json")
    product = read_json(run / "product" / "product-receipt.json")
    consumption = read_json(run / "product" / "maintenance-consumption.json")
    product_identity = read_json(run / "product-identity.json")
    pack_sha = sha(run / "product" / "current-role-pack.json")
    need(product.get("build_id") == plan["source"]["build_id"], "product_build")
    need(product.get("source_fresh") is True and product.get("fixture_review") is True, "product_receipt")
    need({item.get("stage") for item in product.get("stages", []) if item.get("status") == "succeeded"} >= {f"S{i}" for i in range(11)}, "product_stages")
    need(consumption.get("source_id") == plan["source"]["source_id"] and consumption.get("source_generation") == 15, "product_generation")
    need(consumption.get("role_pack_file_sha256") == pack_sha == identity["ready"].get("role_pack_sha256"), "product_pack")
    need(consumption.get("pack_digest") == identity["ready"].get("pack_digest"), "product_pack")
    need(product_identity.get("source_id") == plan["source"]["source_id"] and
         product_identity.get("source_generation") == 15 and product_identity.get("role_pack_sha256") == pack_sha and
         product_identity.get("product_receipt_sha256") == sha(run / "product" / "product-receipt.json") and
         product_identity.get("maintenance_consumption_sha256") == sha(run / "product" / "maintenance-consumption.json"), "product_identity")
    a = verify_dialogue(run, "a", plan["source"], pack_sha)
    b = verify_dialogue(run, "b", plan["source"], pack_sha)
    need(a["seed"] != b["seed"] and not (set(a["semantic_ids"]) & set(b["semantic_ids"])), "dialogue_independence")
    need(not (set(a["query_ids"]) & set(b["query_ids"])), "dialogue_qid_reuse")
    refs = [{"name": item["name"], "path": item["path"], "sha256": item["sha256"]} for item in plan["immutable_files"]]
    doc = {"protocol": PROTOCOL, "full_product_chain_complete": True,
           "periodic_insert_update_delete_verified": all(identity["cut"].get("iud", {}).get("operations", {}).get(key) for key in ("inserted", "updated", "deleted")),
           "recovery_verified": identity["ack"]["task_state"] == "succeeded" and identity["ack"]["lease_generation"] == 4,
           "published": True, "gateway_ready": True, "whole_registered": False,
           "release_candidate_id": identity["active_release"]["candidate_id"],
           "release_version": identity["active_release"]["version"],
           "release_manifest_sha256": identity["active_release"]["manifest_sha256"],
           "release_pack_digest": identity["active_release"]["pack_digest"],
           "release_certificate_class": identity["active_release"].get("certificate_class"),
           "customer_gate_verified": identity["active_release"].get("customer_gate_verified"),
           "formal_signoff": False,
           "product_receipt": "product/product-receipt.json", "dialogue_verdict": "dialogue-a/verdict.json",
           "dialogues": [a, b], "source_id": plan["source"]["source_id"], "source_generation": 15,
           "target_generation": 15, "source_run_id": plan["source"]["run_id"],
           "source_proof": refs, "source_plan_sha256": sha(PLAN),
           "source_gold_sha256": plan["source"]["source_gold"]["sha256"],
           "typed_cells": 20, "typed_differences": 0,
           "task_id": identity["ack"]["task"], "lease_generation": identity["ack"]["lease_generation"],
           "recovery_outcome": identity["ack"]["task_state"],
           "previous_failed_recovery_attempt": plan["recovery_history"],
           "model": "grok-4.7", "reasoning_effort": "low", "agent_turns": a["turns"] + b["turns"],
           "query_ids": sorted(set(a["query_ids"] + b["query_ids"])),
           "gateway_container_id": plan["service"]["gateway_container_id"],
           "gateway_process_sha256": plan["service"]["gateway_process_sha256"],
           "gateway_cli_sha256": plan["runtime"]["gateway_cli_sha256"],
           "native_worker_sha256": plan["service"]["worker_sha256"],
           "native_worker_configured_seconds": 86400,
           "native_worker_deadline_utc": read_json({item["name"]: item for item in plan["immutable_files"]}["worker_restart"]["path"]).get("deadline_utc"),
           "pack_sha256": pack_sha, "pack_digest": consumption["pack_digest"],
           "preflight_sha256": sha(run / "preflight.json"), "source_time": "unmeasured", "replica_lag": "unmeasured",
           "customer_oracle_writes": 0, "source_plan_path": str(PLAN)}
    need(doc["periodic_insert_update_delete_verified"] is True, "source_lifecycle")
    need(preflight.get("source_generation") == 15 and preflight.get("role_pack_sha256") == pack_sha, "preflight_drift")
    write_json(run / "whole.json", doc)
    return doc


def main(argv=None):
    parser = argparse.ArgumentParser()
    parser.add_argument("command", choices=("prepare", "prepare-dialogues", "run-dialogue", "close"))
    parser.add_argument("paths", nargs="+")
    args = parser.parse_args(argv)
    try:
        if args.command == "prepare" and len(args.paths) == 2:
            result = prepare(args.paths[0], args.paths[1])
        elif args.command == "prepare-dialogues" and len(args.paths) == 1:
            result = prepare_dialogues(args.paths[0])
        elif args.command == "run-dialogue" and len(args.paths) == 2 and args.paths[1] in {"a", "b"}:
            result = run_dialogue(args.paths[0], args.paths[1])
        elif args.command == "close" and len(args.paths) == 1:
            result = close(args.paths[0])
        else:
            raise Refused("usage")
    except (OSError, KeyError, TypeError, ValueError, subprocess.TimeoutExpired, IndexError):
        print("oracle-normal-whole: refused; private evidence retained", file=sys.stderr)
        return 2
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
