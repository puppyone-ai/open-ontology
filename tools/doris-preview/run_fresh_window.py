#!/usr/bin/env python3
"""Run the owned Doris DBA window after a seven-table worker baseline."""

import argparse
import json
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "testing/dl01-driver-preflight/doris/scripts"))
import window_9081609e as source  # noqa: E402

STATEMENTS = (
    "INSERT INTO dup_events VALUES (15, 'native_full', CAST('3.25' AS DECIMAL(10,2)))",
    "INSERT INTO dup_events VALUES (15, 'native_full', CAST('3.25' AS DECIMAL(10,2)))",
    "UPDATE uniq_acct SET label = 'native_full' WHERE id = 5",
    "DELETE FROM nz_obs WHERE id = 1",
    "INSERT INTO part_day VALUES ('2026-04-21', 'east', CAST('2.50' AS DECIMAL(12,2)))",
    "DELETE FROM part_day WHERE day = '2026-03-20' AND region = 'east'",
)


def run(args, input_text=None):
    return subprocess.run(args, input=input_text, text=True, capture_output=True, timeout=180)


def target(sql, database):
    result = run(["docker", "exec", "oo-dl01w1-43-native-pg", "psql", "-X", "-U",
                  "oo_admin", "-d", database, "-v", "ON_ERROR_STOP=1", "-Atc", sql])
    if result.returncode:
        raise RuntimeError("target guard refused: " + result.stderr[-300:])
    return result.stdout.strip()


def tick(directory, label, period):
    directory.mkdir()
    reader = ROOT / "evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-product/native-full-20261012/budget-build/doris-reader"
    command = ["docker", "exec", "oo-dl01w1-43-native-worker", str(reader),
               "capture-tick-budget", str(directory), "/tmp/oo-dl01w1-43-ns2-reader",
               "172.16.143.10", "9030", str(period), label, "64"]
    result = run(command)
    (directory / "capture.stdout").write_text(result.stdout)
    (directory / "capture.stderr").write_text(result.stderr)
    (directory / "capture.exit").write_text(str(result.returncode) + "\n")
    if result.returncode:
        raise RuntimeError("source capture refused")
    capture = directory / (label + ".json")
    document = json.loads(capture.read_text())
    if document["product_reader_write_attempts"] != 0:
        raise RuntimeError("source reader wrote")
    return capture, document


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--database", required=True)
    parser.add_argument("--collection", type=Path, required=True)
    parser.add_argument("--legacy-pause-proof", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    args.out = args.out.resolve()
    args.collection = args.collection.resolve()
    owned = ROOT / "evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-product"
    if owned.resolve() not in args.out.resolve().parents or args.out.exists():
        raise ValueError("owned unused evidence directory required")
    proof = args.legacy_pause_proof.read_bytes()
    for token in (b"pause_host=172.16.143.10 pause_port=9030", b"pause_matched=4",
                  b"pause_enabled_remaining=0", b"pause_source_writes=0"):
        if token not in proof:
            raise ValueError("legacy source is not paused")
    status = target("SELECT jsonb_build_object('watermark',(SELECT source_generation FROM _oo.source_watermark WHERE source='dorispreview'),"
                    "'enabled',(SELECT count(*) FROM jw_native.bindings WHERE source_id='dorispreview' AND enabled),"
                    "'active',(SELECT count(*) FROM jw_native.source_progress WHERE source_id='dorispreview' AND active_task_id IS NOT NULL),"
                    "'pending',(SELECT count(*) FROM jobs.tasks WHERE state IN ('queued','running')))::text", args.database)
    guard = json.loads(status)
    if guard != {"watermark": 7, "enabled": 0, "active": 0, "pending": 0}:
        raise ValueError("worker baseline or drain is incomplete")
    args.out.mkdir(parents=True)
    (args.out / "target-guard.json").write_text(json.dumps(guard, indent=2) + "\n")
    before_path, before = tick(args.out / "before", "before", 5000)
    diff = args.out / "before-diff.json"
    compared = run(["python3", str(ROOT / "tools/doris-preview/compare_current.py"),
                    "--database", args.database, "--collection", str(args.collection),
                    "--capture", str(before_path), "--out", str(diff)])
    (args.out / "before-diff.stdout").write_text(compared.stdout)
    (args.out / "before-diff.stderr").write_text(compared.stderr)
    if compared.returncode:
        raise RuntimeError("baseline source-target cells differ")
    rows = {table["name"]: table["rows"] for table in before["tables"]}
    if "2026-03-20\x1feast\x1f1.50" not in rows["part_day"] or rows["nz_obs"] != ["1\x1f\\N"]:
        raise ValueError("DBA preimage changed")
    sql = args.out / "dba-window.sql"
    sql.write_text("USE oo_dl01w1_43;\n" + ";\n".join(STATEMENTS) + ";\n")
    source.EVIDENCE = args.out
    dml = source.mysql("oo_dba", source.DBA_PASSWORD, [sql], "dba-window")
    (args.out / "dba-window.json").write_text(json.dumps({"account": "oo_dba", "statements": STATEMENTS,
                                                             "exit": dml.returncode, "reader_writes": 0}, indent=2) + "\n")
    if dml.returncode:
        raise RuntimeError("DBA mutation failed; preserve partial effects")
    after_path, after = tick(args.out / "after", "after", 11000)
    observed = {table["name"]: table["rows"] for table in after["tables"]}
    expected = {name: list(values) for name, values in rows.items()}
    expected["dup_events"] += ["15\x1fnative_full\x1f3.25"] * 2
    expected["uniq_acct"].remove("5\x1f4.0000\x1fwindow")
    expected["uniq_acct"].append("5\x1f4.0000\x1fnative_full")
    expected["nz_obs"] = []
    expected["part_day"].remove("2026-03-20\x1feast\x1f1.50")
    expected["part_day"].append("2026-04-21\x1feast\x1f2.50")
    mismatches = [name for name in expected if sorted(expected[name]) != sorted(observed[name])]
    version_before = {part["PartitionName"]: part["VisibleVersion"] for part in before["partitions"]}
    version_after = {part["PartitionName"]: part["VisibleVersion"] for part in after["partitions"]}
    changed = [name for name in version_before if version_before[name] != version_after.get(name)]
    result = {"source_after": str(after_path), "mismatches": mismatches,
              "changed_partitions": changed, "duplicate15_multiplicity": observed["dup_events"].count("15\x1fnative_full\x1f3.25"),
              "reader_write_attempts": before["product_reader_write_attempts"] + after["product_reader_write_attempts"]}
    (args.out / "window-result.json").write_text(json.dumps(result, indent=2) + "\n")
    if mismatches or len(changed) != 2:
        raise RuntimeError("actual source mutation differs")
    print(json.dumps(result))


if __name__ == "__main__":
    main()
