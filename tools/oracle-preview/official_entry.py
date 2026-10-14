#!/usr/bin/env python3
"""Six-field Oracle step. Reuses build b_739 and the spent S5. No DML and no new model create.

The installed engines.json recipe still pins whole_run.py plus stale product_flow and dialogue
hashes. This file is the replacement step. Its dialogue step is the repo verifier at 80aec7764.
It does not register whole and it does not rewrite the spent S5 receipt.
"""
import hashlib
import json
import os
import shutil
import subprocess
import sys
import time
from decimal import Decimal
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import c5_live  # noqa: E402
import whole_run as w  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
PYTHON = Path("/opt/homebrew/opt/python@3.14/bin/python3.14")
FLOW = ROOT / "tools/source-onboarding/product_flow.py"
DIALOGUE = ROOT / "tools/agent-product-verifier/dialogue.py"
VERIFIER_COMMIT = "80aec776414aaed9064daf3600cf3df21fc21eb5"
PRODUCT_DIR = Path("/tmp/oo-oracle-connection-whole/whole-20261014f/product")
SEALED = Path("/Users/redux/.cache/oo-oracle-connection-whole/bundle/catalog.json")
PACK = Path("/Users/redux/.cache/oo-oracle-connection-whole/gateway/packs/oo_r_oracle_orders.json")
BANK = Path("/tmp/oo-oracle-connection-whole/maint-c44/dialogue-g7/bank.json")
TRUTH = Path("/tmp/oo-oracle-connection-whole/maint-c44/dialogue-g7/gold.json")
ROLE_RESPONSE = Path("/tmp/oo-oracle-connection-whole/maint-c44/g7/role-response.json")
PATHS = Path("/tmp/oo-oracle-connection-whole/maint-c44/dialogue-g7/run-a.json")
PHYSICAL = "pt_f4daa4bff72834557de4213463fb9d26ad42a188d253e84865ddb84fc0e9"
TABLE = "q_4f4f30345f464958.q_4f4e424f4152445f4f5244455253"
BUILD = "b_739b3a14d1167f50b5f48ae22a8317e2"
S5_CATALOG = "5c94877dc45b51d3606c47b4ca4965df6363b6d07809ad1eb453b4bbe9ebaf4a"
SEALED_CATALOG = "67f8d2bf28a8c7126302a1e39e3e868082913422ecc141abc25fb6117777bd1d"
IDENTITY_HASH = "394dc60f76bc01a3115e6e4e894e86824f4b6c3bd809c2c82a67ea0d9c165c35"
PACK_FILE = "a25dfbfc6d1ed292bf0f2547af3f7ace929171fbc408e6c4c496d239c237d5c2"
PACK_DIGEST = "ef174bd20ff756039211d5e0ef999bf8444338dd7f5efa7d87b72d18c643f5a7"
API_PACK = "9bfd65d2ca3a27fa019882c83253d1dccb6f623a611d179589f73d573ccb9a53"
RECEIPT_PACK = "731226cc65be31acfd1920548bf94075f90f8574d9d5916c31bba2ba2ac49e45"
RUN_ID = "18aa01b0-cc91-49b6-91ce-2dfd2122ee0d"
TEMPLATE = Path("/tmp/oo-oracle-connection-whole/oneclick-config.json")


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def write(path, value):
    path = Path(path)
    path.write_text(json.dumps(value, indent=2) + "\n")
    path.chmod(0o600)


def six_fields():
    return {
        "protocol": "oo.source-onboarding/v1",
        "connection": {
            "type": "oracle", "host": "oo-dl01w1-04-p2-oracle", "port": 1521,
            "database": "FREEPDB1", "user": "OO04_READER",
            "secret_ref": "secret://oo04p2/driver/source/reader",
        },
    }


def pack_identities():
    """File bytes, semantic digest, HTTP pack id, and the S5 receipt are different identities."""
    file_sha = sha(PACK)
    digest = json.loads(PACK.read_text()).get("pack_digest")
    response = json.loads(ROLE_RESPONSE.read_text())
    http = response.get("response") if isinstance(response.get("response"), dict) else {}
    if response.get("status") != 200 or http.get("llm_calls") != 0 or http.get("compiled_pack") != "not_published":
        raise SystemExit("current_identity_drift")
    api = http.get("pack_sha256")
    receipt_pack = json.loads((PRODUCT_DIR / "product-receipt.json").read_text()).get("native_role_pack_sha256")
    packs = {
        "role_pack_file_sha256": file_sha,
        "semantic_pack_digest": digest,
        "api_pack_sha256": api,
        "s5_receipt_pack_sha256": receipt_pack,
    }
    if packs != {
        "role_pack_file_sha256": PACK_FILE,
        "semantic_pack_digest": PACK_DIGEST,
        "api_pack_sha256": API_PACK,
        "s5_receipt_pack_sha256": RECEIPT_PACK,
    } or len(set(packs.values())) != 4:
        raise SystemExit("current_identity_drift")
    return packs


def current_identity():
    """S5 evidence catalog stays the build input. The sealed catalog is generation 7."""
    sealed = sha(SEALED)
    if sealed != SEALED_CATALOG:
        raise SystemExit("current_identity_drift")
    return {
        "source_id": w.SOURCE, "source_generation": 7, "target_generation": 7,
        "run_id": RUN_ID, "build_id": BUILD, "role": w.ROLE,
        "s5_catalog_sha256": S5_CATALOG, "sealed_catalog_sha256": sealed,
        "identity_hash": IDENTITY_HASH, "schema_digest": "f002b65dac929bfd96866a1936003e92ed366a186e4dc624fb978ef4af2cc425",
        "packs": pack_identities(), "physical": PHYSICAL, "whole": False,
    }


def write_dialogue_config(run):
    """Private config for the repo verifier. Selection pack_sha256 is the role-pack file bytes."""
    packs = pack_identities()
    paths = json.loads(PATHS.read_text())
    gateway = "http://" + (w.GW / "listen.addr").read_text().strip()
    cfg = {
        "question_bank": str(BANK),
        "independent_truth_file": str(TRUTH),
        "sample_size": 2,
        "history": [],
        "model": "gpt-6.1-sol",
        "model_origin": paths["model_origin"],
        "model_bearer_file": paths["model_bearer_file"],
        "gateway": gateway,
        "transport": paths["transport"],
        "auth": paths["auth"],
        "gateway_token_file": paths["gateway_token_file"],
        "oo_cli": paths["oo_cli"],
        "independent_gateway_audit_file": paths["independent_gateway_audit_file"],
        "max_tool_rounds": 16,
        "identity": {
            "source_id": w.SOURCE, "source_generation": 7,
            "pack_sha256": packs["role_pack_file_sha256"], "role": w.ROLE,
        },
    }
    write(Path(run) / "dialogue-config.json", cfg)
    return cfg


def materials(dest):
    """Recipe for the shared registry writer. Does not register whole=true."""
    me = Path(__file__).resolve()
    recipe = {
        "protocol": "oo.source-onboarding.recipe/v1", "engine": "oracle", "owned_fixture": True,
        "mode": "whole_source", "completion_artifact": "whole.json",
        "config_template": str(TEMPLATE),
        "connection_map": {
            "/host": "host", "/port": "port", "/database": "database",
            "/user": "user", "/secret_ref": "secret_ref", "/password_file": "password_file",
        },
        "verifier_commit": VERIFIER_COMMIT,
        "pins": [
            {"path": str(PYTHON), "sha256": sha(PYTHON)},
            {"path": str(me), "sha256": sha(me)},
            {"path": str(FLOW), "sha256": sha(FLOW)},
            {"path": str(DIALOGUE), "sha256": sha(DIALOGUE)},
        ],
        "steps": [
            {"name": "identity", "deadline_seconds": 60, "argv": [str(PYTHON), str(me), "identity", "{run}"], "artifacts": [{"path": "current-identity.json"}, {"path": "dialogue-config.json"}]},
            {"name": "product", "deadline_seconds": 180, "argv": [str(PYTHON), str(FLOW), "--config", str(w.PRODUCT), "--out", "{run}/product"], "artifacts": [{"path": "product/product-receipt.json"}]},
            {"name": "recover", "deadline_seconds": 180, "argv": [str(PYTHON), str(me), "recover", "{run}"], "artifacts": [{"path": "recovery.json"}]},
            {"name": "gold", "deadline_seconds": 180, "argv": [str(PYTHON), str(me), "gold", "{run}"], "artifacts": [{"path": "typed-gold.json"}]},
            {"name": "cancel", "deadline_seconds": 90, "argv": [str(PYTHON), str(me), "cancel", "{run}"], "artifacts": [{"path": "cancel.json"}]},
            {"name": "dialogue", "deadline_seconds": 600, "argv": [str(PYTHON), str(DIALOGUE), "--config", "{run}/dialogue-config.json", "--out", "{run}/dialogue"], "artifacts": [{"path": "dialogue/selection.json"}, {"path": "dialogue/transcript.json"}, {"path": "dialogue/verdict.json"}]},
            {"name": "close", "deadline_seconds": 60, "argv": [str(PYTHON), str(me), "close", "{run}"], "artifacts": [{"path": "whole.json"}]},
        ],
    }
    write(dest, recipe)
    return recipe


def seed_product(run):
    """Copy the existing receipt so product_flow returns reused and does not create a build."""
    product = Path(run) / "product"
    product.mkdir(mode=0o700, exist_ok=True)
    os.chmod(product, 0o700)
    for name in ("build.json", "product-receipt.json"):
        dest = product / name
        if not dest.exists():
            shutil.copyfile(PRODUCT_DIR / name, dest)
            dest.chmod(0o600)


def reuse_product(run):
    seed_product(run)
    product = Path(run) / "product"
    proc = subprocess.run([str(PYTHON), str(FLOW), "--config", str(w.PRODUCT), "--out", str(product)], capture_output=True, text=True)
    (product / "reuse.stdout").write_text(proc.stdout)
    (product / "reuse.stderr").write_text(proc.stderr)
    if proc.returncode != 0:
        raise SystemExit("product_reuse_failed")
    body = json.loads(proc.stdout)
    if body.get("build_id") != BUILD or body.get("reused_completed") is not True:
        raise SystemExit("product_reuse_failed")
    return body


def columns():
    text = w.psql("previewdb", f"""
SELECT c.logical_column || '|' || c.physical_name
FROM _oo.physical_mappings t
JOIN _oo.physical_mappings c ON c.source = t.source AND c.run_generation = t.run_generation
 AND c.config_digest = t.config_digest AND c.logical_namespace = t.logical_namespace
 AND c.logical_table = t.logical_table AND c.level = 'c'
WHERE t.level = 't' AND t.physical_name = '{PHYSICAL}' AND t.source = '{w.SOURCE}';
""", tuples=True)
    found = {}
    for line in text.splitlines():
        if "|" not in line:
            continue
        logical, physical = line.split("|", 1)
        found[bytes.fromhex(logical[2:]).decode()] = physical
    if set(found) != {"KEY_ID", "AMOUNT", "OCCURRED", "LABEL", "PAYLOAD"}:
        raise SystemExit("physical_columns")
    return found


def norm(row):
    return {
        "key": str(row["key"]),
        "amount": format(Decimal(row["amount"]), "f"),
        "occurred": row["occurred"],
        "label": row["label"],
        "payload": (row.get("payload") or "").lower(),
    }


def gold(run):
    c5_live.require_running(c5_live.ORACLE, c5_live.ORACLE_ID)
    c5_live.require_running(c5_live.PG, c5_live.PG_ID)
    cfg = json.loads(TEMPLATE.read_text())
    password = Path(cfg["password_file"]).read_text().strip("\r\n")
    sql_dir = Path(run) / "sql"
    sql_dir.mkdir(mode=0o700, exist_ok=True)
    sql = c5_live.HEAD + (
        "SELECT 'ROW|' || JSON_OBJECT('key' VALUE KEY_ID, 'amount' VALUE TO_CHAR(AMOUNT, 'FM9999999990.00'), "
        "'occurred' VALUE TO_CHAR(OCCURRED, 'YYYY-MM-DD\"T\"HH24:MI:SS'), 'label' VALUE LABEL, "
        "'payload' VALUE LOWER(RAWTOHEX(PAYLOAD)) ABSENT ON NULL) FROM OO04_FIX.ONBOARD_ORDERS ORDER BY KEY_ID;\nEXIT\n"
    )
    code, text = c5_live.sqlplus("OO04_READER", password, sql, "typed-gold", sql_dir, [password.encode()])
    if code != 0:
        raise SystemExit("reader_gold")
    left = [norm(json.loads(line.strip()[4:])) for line in text.splitlines() if line.strip().startswith("ROW|")]
    mapped = columns()
    raw = w.psql("previewdb", f"""
SELECT json_agg(json_build_object(
  'key', {mapped['KEY_ID']}::text,
  'amount', to_char({mapped['AMOUNT']}, 'FM9999999990.00'),
  'occurred', to_char({mapped['OCCURRED']}, 'YYYY-MM-DD\"T\"HH24:MI:SS'),
  'label', {mapped['LABEL']},
  'payload', encode({mapped['PAYLOAD']}, 'hex')
) ORDER BY {mapped['KEY_ID']}) FROM _oo.{PHYSICAL};
""", tuples=True)
    right = [norm(row) for row in json.loads(raw.strip() or "null") or []]
    body = {
        "protocol": "oo.oracle.typed-source-gold/v1", "generation": 7, "target_physical": PHYSICAL,
        "rows": len(left), "cells": len(left) * 5, "differences": 0 if left == right else 1,
        "key_2_absent": all(row["key"] != "2" for row in left), "reader_exit": code,
        "keys": [row["key"] for row in left],
        "source_sha256": hashlib.sha256(json.dumps(left, separators=(",", ":")).encode()).hexdigest(),
    }
    if body["differences"] or body["keys"] != ["1", "3", "11", "13"] or not body["key_2_absent"]:
        write(Path(run) / "typed-gold.json", body)
        raise SystemExit("typed_gold")
    write(Path(run) / "typed-gold.json", body)
    return body


def recover(run):
    before = w.read_mark()
    evidence = w.refuse_admission(Path(run) / "recovery-attempt")
    after = w.read_mark()
    body = {
        "exit": evidence["exit"], "wandered": evidence["wandered"],
        "before_generation": before.get("source_generation"), "after_generation": after.get("source_generation"),
        "run_id_unchanged": before.get("run_id") == after.get("run_id") == RUN_ID,
    }
    write(Path(run) / "recovery.json", body)
    if body["exit"] == 0 or body["wandered"] or not body["run_id_unchanged"]:
        raise SystemExit("recovery")
    return body


def cancel(run):
    gateway = "http://" + (w.GW / "listen.addr").read_text().strip()
    code, tables = w.cli(gateway, ["tables"])
    if code != 0 or tables["tables"][0]["name"] != TABLE:
        raise SystemExit("cancel_table")
    marker = "oo-oracle-g7-entry-cancel"
    holder = subprocess.Popen([
        "docker", "exec", "-i", "-u", "postgres", w.PG, "psql", "-d", "previewdb", "-v", "ON_ERROR_STOP=1", "-c",
        f"/* {marker} */ BEGIN; LOCK TABLE _oo.{PHYSICAL} IN ACCESS EXCLUSIVE MODE; SELECT pg_sleep(20); ROLLBACK;",
    ], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            found = w.psql("previewdb", f"SELECT count(*) FROM pg_locks WHERE relation = '_oo.{PHYSICAL}'::regclass AND mode = 'AccessExclusiveLock';", tuples=True)
            if found.strip() not in {"", "0"}:
                break
            time.sleep(0.2)
        else:
            raise SystemExit("cancel_lock")
        query = subprocess.Popen([
            str(w.GW / "oo-cli"), "query", f"SELECT COUNT(*) AS row_count FROM {TABLE}",
            "--json", "--gateway", gateway, "--transport", "local-development", "--auth", "external",
            "--token-file", str(w.GW / "client.token"), "--max-rows", "100", "--timeout-ms", "15000",
        ], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        query_id = ""
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline and not query_id:
            for path in (w.GW / "wal").glob("*-admit.wal"):
                if not Path(str(path).replace("-admit.wal", "-complete.wal")).exists():
                    query_id = path.name.split("-admit.wal")[0]
                    break
            time.sleep(0.2)
        if not query_id:
            raise SystemExit("cancel_query")
        code, body = w.cli(gateway, ["cancel", query_id])
        wal = w.GW / "wal" / f"{query_id}-cancel.wal"
        record = {"exit": code, "query_id": query_id, "cancel_wal": wal.is_file()}
        if wal.is_file():
            phase = json.loads(wal.read_text().splitlines()[0])
            record["phase"] = phase.get("phase")
            record["outcome"] = phase.get("outcome")
        write(Path(run) / "cancel.json", record)
        query.wait(timeout=20)
        if code != 0 or not wal.is_file():
            raise SystemExit("cancel")
        return record
    finally:
        if holder.poll() is None:
            holder.kill()
        w.psql("previewdb", f"SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE query LIKE '%{marker}%' AND pid <> pg_backend_pid();")


def dialogue_record(run):
    """Only this run's repo verifier output counts. A private earlier verdict is not copied."""
    folder = Path(run) / "dialogue"
    selection = folder / "selection.json"
    if not selection.is_file():
        return {"outcome": "not_run", "program": str(DIALOGUE), "verifier_commit": VERIFIER_COMMIT}
    body = json.loads(selection.read_text())
    item = {
        "outcome": "unfinished",
        "program": str(DIALOGUE),
        "verifier_commit": VERIFIER_COMMIT,
        "generation": body["identity"]["source_generation"],
        "semantic_ids": body["semantic_ids"],
        "seed": body["seed"],
        "counts_as_random": body["counts_as_random"],
        "pack_sha256": body["identity"]["pack_sha256"],
    }
    verdict = folder / "verdict.json"
    if verdict.is_file():
        got = json.loads(verdict.read_text())
        item["outcome"] = got.get("outcome")
        item["model"] = got.get("model")
        item["query_ids"] = got.get("query_ids")
    return item


def current_pack_callback():
    return {
        "protocol": "oo.oracle.current-pack-callback/v1",
        "to": "shared registry owner d28ca4bd",
        "needed": "official derived currentPack association",
        "packs": pack_identities(),
        "schema_digest": "f002b65dac929bfd96866a1936003e92ed366a186e4dc624fb978ef4af2cc425",
        "identity_hash": IDENTITY_HASH,
        "receipt_rewritten": False,
        "copied_private_verdict": False,
        "full_product_chain_complete": False,
    }


def close(run):
    run = Path(run)
    identity = current_identity()
    receipt = json.loads((run / "product" / "product-receipt.json").read_text())
    body = {
        "protocol": "oo.oracle.official-entry/v1",
        "full_product_chain_complete": False,
        "published": False,
        "whole_registered": False,
        "identity": identity,
        "reused_product_pack_sha256": receipt.get("native_role_pack_sha256"),
        "s5_catalog_sha256": receipt.get("catalog_sha256"),
        "build_id": receipt.get("build_id"),
        "gold": "typed-gold.json",
        "recovery": "recovery.json",
        "cancel": "cancel.json",
        "dialogue": dialogue_record(run),
        "current_pack_callback": current_pack_callback(),
        "customer_writes": 0,
        "c1_logminer": "off",
        "installed_entry": "pin_drift",
    }
    write(run / "whole.json", body)
    write(run / "current-identity.json", identity)
    return body


def main():
    if len(sys.argv) != 3:
        raise SystemExit("usage")
    command, run = sys.argv[1], Path(sys.argv[2])
    if command == "materials":
        print(json.dumps({"wrote": str(run), "whole": False}))
        materials(run)
        return
    if command == "callback":
        write(run, current_pack_callback())
        print(json.dumps({"wrote": str(run), "receipt_rewritten": False}))
        return
    run.mkdir(mode=0o700, exist_ok=True)
    os.chmod(run, 0o700)
    if command == "identity":
        seed_product(run)
        write(run / "current-identity.json", current_identity())
        write_dialogue_config(run)
    elif command == "product":
        reuse_product(run)
    elif command == "recover":
        recover(run)
    elif command == "gold":
        gold(run)
    elif command == "cancel":
        cancel(run)
    elif command == "close":
        close(run)
    else:
        raise SystemExit("usage")


if __name__ == "__main__":
    main()
