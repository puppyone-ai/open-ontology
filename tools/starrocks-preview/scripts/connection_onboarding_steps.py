#!/usr/bin/env python3
"""Prepare and finalize one StarRocks six-field onboarding run."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import traceback

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "tools/starrocks-preview/scripts"))
import connection_whole as source


def save(path, body):
    path = Path(path)
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    path.write_text(json.dumps(body, indent=2) + "\n")
    os.chmod(path, 0o600)


def prepare(config_path, run_path):
    cfg = json.loads(Path(config_path).read_text())
    run_dir = source.open_run_dir(run_path, config_path)
    source.require_fixture(cfg)
    if source.worker_sha(source.WORKER) != source.WORKER_SHA:
        raise RuntimeError("fixed C5 worker binary changed")
    proc = subprocess.run(["docker", "exec", source.W, "ps", "-eo", "pid=,args="], capture_output=True, text=True, timeout=20)
    exact = [line.strip() for line in proc.stdout.splitlines() if "native-serve" in line and "--worker star-conn-c5" in line and source.WORKER in line]
    if proc.returncode or len(exact) != 1:
        raise RuntimeError("exactly one fixed C5 native-serve must be running")
    serve_pid = int(exact[0].split(None, 1)[0])
    source_text, target_text = source.psql("SELECT source_generation::text || ' ' || target_generation::text FROM _oo.source_watermark WHERE source='sr_w45_native'").split()
    generation, target_generation = int(source_text), int(target_text)
    if generation != 19 or target_generation <= generation:
        raise RuntimeError("source/target watermarks are not the retained source19/target31 state")
    live = {}
    for line in source.psql("SELECT logical_table || '=' || max(run_generation)::text FROM _oo.physical_mappings WHERE source='sr_w45_native' AND level='t' GROUP BY logical_table ORDER BY 1").splitlines():
        table, gen = line.split("=")
        live[table] = int(gen)
    if source.sha(source.RETAINED_GOLD) != source.RETAINED_GOLD_SHA:
        raise RuntimeError("retained typed gold mismatch")
    gold = json.loads(source.RETAINED_GOLD.read_text())
    tables = {name: int(entry["source_generation"]) for name, entry in gold["tables"].items()}
    if tables != live or gold.get("all_equal") is not True or gold.get("sampled") is not False:
        raise RuntimeError("retained typed gold is not the live mixed vector")
    catalog = source.REUSED_BUILD / "catalog.json"
    if not catalog.is_file() or catalog.is_symlink():
        raise RuntimeError("confirmed build catalog is absent")
    binding = json.loads((source.GW_DIR / "reviewed/w45_native/binding.json").read_text())
    if int(source.psql("SELECT period_ms::text FROM jw_native.bindings WHERE id='%s'" % source.BINDING)) != source.PERIOD:
        raise RuntimeError("native binding period changed")
    source.product(run_dir, None, source.sha(catalog))
    source.refresh_gateway(run_dir)
    cli = source.gateway_cli_wrapper(run_dir)
    token = source.GW_DIR / ("client-conn-" + run_dir.name + ".token")
    dialog = {
        "oo_cli": str(cli),
        "gateway": "http://" + (source.GW_DIR / "listen.addr").read_text().strip(),
        "transport": "local-development",
        "auth": "external",
        "gateway_token_file": str(token),
        "token_sha256": source.sha(token),
        "pack_file": str(run_dir / "product/current-role-pack.json"),
        "model_origin": "http://127.0.0.1:8317",
        "model": "grok-4.7",
        "model_bearer_file": "/tmp/oo-product-model-20261012/bearer",
        "independent_truth_file": str(source.SECOND_GOLD),
        "independent_gateway_audit_file": str(source.GW_DIR / "wal"),
        "max_tool_rounds": 12,
        "question_bank": str(source.SECOND_BANK),
        "sample_size": 2,
        "history_file": str(source.SECOND_HISTORY),
        "prior_manifest": str(source.SECOND_PRIOR),
        "identity": {"source_id": "sr_w45_native", "source_generation": generation,
                     "pack_sha256": source.sha(run_dir / "product/current-role-pack.json"),
                     "role": "oo_r_w45_native"},
    }
    save(run_dir / "dialogue-config.json", dialog)
    save(run_dir / "prepare.json", {
        "protocol": "oo.starrocks.onboarding-prepare/v1",
        "source_id": "sr_w45_native", "source_generation": generation,
        "target_generation": target_generation, "target_tables": tables,
        "binding": source.BINDING, "period_ms": source.PERIOD,
        "native_serve_pid": serve_pid, "worker_sha256": source.sha(source.WORKER),
        "worker_config_sha256": source.sha("/tmp/oo-star-runtime-finish/worker-c5.conf"),
        "pack_sha256": dialog["identity"]["pack_sha256"], "token_sha256": dialog["token_sha256"],
    })


def finalize(config_path, run_path):
    cfg = json.loads(Path(config_path).read_text())
    run_dir = Path(run_path)
    source.open_run_dir(run_dir, config_path)
    preflight = json.loads((run_dir / "preflight/identity.json").read_text())
    dialog = json.loads((run_dir / "dialogue-config.json").read_text())
    if preflight.get("passed") is not True or preflight.get("model_calls") != 0 or preflight.get("results_injected") is not False:
        raise RuntimeError("same-token read-only preflight is absent or failed")
    identity = dialog["identity"]
    if any(preflight.get(k) != identity.get(k) for k in ("source_id", "source_generation", "role", "pack_sha256")) or preflight.get("token_sha256") != dialog["token_sha256"]:
        raise RuntimeError("preflight and dialogue do not share current source/role/pack/token")
    binding = json.loads((source.GW_DIR / "reviewed/w45_native/binding.json").read_text())
    source_text, target_text = source.psql("SELECT source_generation::text || ' ' || target_generation::text FROM _oo.source_watermark WHERE source='sr_w45_native'").split()
    generation, target_generation = int(source_text), int(target_text)
    live = {}
    for line in source.psql("SELECT logical_table || '=' || max(run_generation)::text FROM _oo.physical_mappings WHERE source='sr_w45_native' AND level='t' GROUP BY logical_table ORDER BY 1").splitlines():
        table, gen = line.split("=")
        live[table] = int(gen)
    checked = source.exit_checks(binding, live, generation, target_generation, run_dir)
    checked["gaps"] = [g for g in checked["gaps"] if g != "this run has no second fresh two-turn on the current pack"]
    dialogue = json.loads((run_dir / "dialogue/verdict.json").read_text())
    selection = json.loads((run_dir / "dialogue/selection.json").read_text())
    if dialogue.get("outcome") != "verified" or selection.get("mode") != "random":
        checked["gaps"].append("current run dialogue is not a verified random selection")
    checked["whole"] = not checked["gaps"]
    result = {
        "protocol": "oo.starrocks.entry-callback/v1", "whole": checked["whole"],
        "published": False, "formal_signoff": False,
        "source_generation": generation, "target_generation": target_generation,
        "tables": live, "native_serve_pid": json.loads((run_dir / "prepare.json").read_text())["native_serve_pid"],
        "period_ms": source.PERIOD, "product_receipt": "product/product-receipt.json" if checked.get("product_receipt") else None,
        "dialogue_verdict": "dialogue/verdict.json" if dialogue.get("outcome") == "verified" else None,
        "cli_preflight": "preflight/identity.json", "dialogue_config": "dialogue-config.json",
        "periodic_insert_update_delete_verified": checked.get("periodic_insert_update_delete_verified"),
        "recovery_verified": checked.get("recovery_verified"),
        "gaps": checked["gaps"], "identities": checked.get("identities"),
    }
    save(run_dir / "entry-callback.json", result)
    if checked["whole"]:
        save(run_dir / "whole.json", {
            "full_product_chain_complete": True,
            "periodic_insert_update_delete_verified": True,
            "recovery_verified": True,
            "product_receipt": "product/product-receipt.json",
            "dialogue_verdict": "dialogue/verdict.json",
            "cli_preflight": "preflight/identity.json",
            "dialogue_config": "dialogue-config.json",
        })
        return
    raise RuntimeError("; ".join(checked["gaps"]))


if __name__ == "__main__":
    try:
        phase, config, run = sys.argv[1:4]
        (prepare if phase == "prepare" else finalize)(config, run)
    except Exception as exc:
        traceback.print_exc(file=sys.stderr)
        print("starrocks-onboarding-steps: " + str(exc), file=sys.stderr)
        raise SystemExit(1)
