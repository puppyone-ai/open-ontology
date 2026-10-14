#!/usr/bin/env python3
"""Continue the owned Oracle fixture after the recorded generation-2 load.

Does not create containers, does not rebuild ELFs, and does not fire a loader
generation whose watermark already says awaiting_publish. Customer endpoints
are refused before any secret or SQL. C1 LogMiner stays off.
"""
import argparse
import json
from pathlib import Path
import re
import subprocess
import sys

import c5_capture
import c5_live
import configured_native
import configured_worker

OWNED = {"host": "oo-dl01w1-04-p2-oracle", "port": 1521, "database": "FREEPDB1", "user": "OO04_READER"}
UUID = re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\Z")


def owned(cfg):
    if cfg.get("host") == "47.104.150.9" or cfg.get("database") == "scm5":
        raise ValueError("customer endpoint is metadata-only")
    for key, value in OWNED.items():
        if cfg.get(key) != value:
            raise ValueError("owned fixture connection required")
    if cfg.get("secret_ref") != "secret://oo04p2/driver/source/reader":
        raise ValueError("owned secret ref required")


def recorded(step_text, watermark):
    if "pin=2" not in step_text or "complete=succeeded" not in step_text or "load_state=awaiting_publish" not in step_text:
        raise ValueError("generation 2 load is not a recorded success")
    if watermark.get("source_generation") != 2 or watermark.get("load_state") != "awaiting_publish" or watermark.get("source_id") != "oracle_oneclick_orders":
        raise ValueError("watermark is not the recorded generation")


def next_action(present_keys, generation3_exists):
    if generation3_exists:
        return "already_captured"
    if "2" not in {str(key) for key in present_keys}:
        return "capture_after_committed_delete"
    return "delete_key_2_then_capture_generation_3"


def read_watermark(binding):
    if UUID.fullmatch(binding) is None:
        raise ValueError("binding")
    query = "SELECT result::text FROM jw_native.watermarks WHERE binding_id='" + binding + "'::uuid"
    proc = c5_live.run_cmd(["docker", "exec", "-u", "postgres", c5_live.PG, "psql", "-d", "previewdb", "-At", "-c", query], timeout=20)
    if proc.returncode != 0 or not proc.stdout.strip():
        raise ValueError("watermark read failed")
    return json.loads(proc.stdout.decode())


def present_keys(reader_password, sql_dir, secrets):
    code, text = c5_live.sqlplus(
        "OO04_READER",
        reader_password,
        "SET PAGESIZE 0 FEEDBACK OFF HEADING OFF\nSELECT 'KEY|' || KEY_ID FROM OO04_FIX.ONBOARD_ORDERS ORDER BY KEY_ID;\nEXIT\n",
        "keys-before-delete",
        sql_dir,
        secrets,
    )
    if code != 0:
        raise ValueError("reader key probe failed")
    return c5_live.lines(text, "KEY|")


def delete_key(password, sql_dir, secrets):
    code, text = c5_live.sqlplus(
        "system",
        password,
        "DELETE FROM OO04_FIX.ONBOARD_ORDERS WHERE KEY_ID=2;\nCOMMIT;\nSELECT 'LEFT|' || KEY_ID FROM OO04_FIX.ONBOARD_ORDERS ORDER BY KEY_ID;\nEXIT\n",
        "delete-key-2",
        sql_dir,
        secrets,
    )
    left = c5_live.lines(text, "LEFT|")
    if code != 0 or "2" in set(left):
        raise ValueError("owned delete did not remove key 2")
    return left


def run(cfg, previous, out, collect_only=False):
    owned(cfg)
    previous = previous.resolve()
    if (out / "cycle-receipt.json").is_file():
        return json.loads((out / "cycle-receipt.json").read_text())
    out.mkdir(mode=0o700, exist_ok=True)
    identity = json.loads((previous / "worker-identity.json").read_text())
    step_text = (previous / "step.stdout").read_text()
    resources = {
        "oracle": c5_live.require_running(c5_live.ORACLE, c5_live.ORACLE_ID),
        "postgres": c5_live.require_running(c5_live.PG, c5_live.PG_ID),
    }
    actual = read_watermark(identity["binding"])
    recorded(step_text, actual)
    receipt = {
        "protocol": "oo.oracle.whole-continue/v1",
        "binding": identity["binding"],
        "resources": resources,
        "watermark": {k: actual.get(k) for k in ("source_id", "source_generation", "load_state", "run_id", "row_count")},
        "repeat_load": False,
        "c1_logminer": "off",
        "customer_writes": 0,
        "action": "collect",
    }
    (out / "collect.json").write_text(json.dumps(receipt, indent=2) + "\n")
    if collect_only:
        return receipt
    native = out / "native-g3"
    shadow_path = native / "capture/run/shadow/ONBOARD_ORDERS/gen-3.json"
    if shadow_path.is_file():
        configured_native.resume_capture(cfg, native, 3, previous)
        return finish_receipt(receipt, previous, native, out)
    if not (previous / "disable.exit").is_file():
        if configured_worker.run(previous, "disable", ["native-enable", "--binding", identity["binding"], "--enabled", "false"]):
            raise ValueError("schedule disable failed; raw exit retained")
    reader = Path(cfg["password_file"]).read_text().strip("\r\n")
    dba = c5_live.DBA_PASSWORD.read_text().strip("\r\n")
    secrets = [reader.encode(), dba.encode()]
    sql_dir = out / "sql"
    sql_dir.mkdir(mode=0o700, exist_ok=True)
    action = next_action(present_keys(reader, sql_dir, secrets), (native / "loader-result.json").is_file())
    receipt["action"] = action
    (out / "collect.json").write_text(json.dumps(receipt, indent=2) + "\n")
    if action == "already_captured":
        return receipt
    left = ["1", "3"]
    if action == "delete_key_2_then_capture_generation_3":
        left = delete_key(dba, sql_dir, secrets)
    wait = c5_capture.wait_realtime(c5_capture.resolve_periods(cfg["periods"]))
    scn, log_mode, supplemental = c5_live.publish_scn(dba, sql_dir, secrets, "scn-after-delete")
    Path(cfg["scn_file"]).write_text(scn + "\n")
    configured_native.run(cfg, native, 3, previous)
    receipt.update(keys_left=left, wait=wait, scn=scn, log_mode=log_mode, supplemental_log=supplemental)
    return finish_receipt(receipt, previous, native, out)


def finish_receipt(receipt, previous, native, out):
    shadow = json.loads((native / "capture/run/shadow/ONBOARD_ORDERS/gen-3.json").read_text())
    events = c5_capture.diff_hashes(
        json.loads((previous / "capture/run/shadow/ONBOARD_ORDERS/gen-2.json").read_text())["hashes"],
        shadow["hashes"],
    )
    if not any(event["op"] == "delete" and event["key"] == "2" for event in events):
        raise ValueError("generation 3 did not reconcile the physical delete")
    loaded_path = native / "loader-result.json"
    applied = {}
    if loaded_path.is_file():
        loaded = json.loads(loaded_path.read_text())
        applied = {k: loaded.get(k) for k in ("source_id", "source_generation", "load_state", "run_id", "row_count", "physical_name")}
    receipt.update(events=events, row_count=len(shadow["cells"]), tombstones=shadow["tombstones"], repeat_load=False, applied=applied, action="delete_key_2_then_capture_generation_3")
    (out / "cycle-receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    return receipt


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--previous", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--collect-only", action="store_true")
    args = parser.parse_args()
    try:
        run(json.loads(args.config.read_text()), args.previous, args.out, args.collect_only)
    except (ValueError, KeyError, OSError, json.JSONDecodeError, subprocess.TimeoutExpired) as error:
        print("oracle whole continue refused: " + str(error), file=sys.stderr)
        sys.exit(2)
