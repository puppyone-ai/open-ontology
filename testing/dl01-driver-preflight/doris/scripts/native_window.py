#!/usr/bin/env python3
"""One synthetic DBA window, guarded by paused real target jobs.

This command writes only the owned fixture as oo_dba. It refuses until the
seven registered target bindings are disabled and have no queued/running work.
The legacy source schedule pause is supplied as the coordinator's fixed proof.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

import window_9081609e as source
from native_full import compare

STATEMENTS = (
    "INSERT INTO dup_events VALUES (15, 'native_full', CAST('3.25' AS DECIMAL(10,2)))",
    "INSERT INTO dup_events VALUES (15, 'native_full', CAST('3.25' AS DECIMAL(10,2)))",
    "UPDATE uniq_acct SET label = 'native_full' WHERE id = 5",
    "DELETE FROM nz_obs WHERE id = 1",
    "INSERT INTO part_day VALUES ('2026-04-21', 'east', CAST('2.50' AS DECIMAL(12,2)))",
    "DELETE FROM part_day WHERE day = '2026-03-20' AND region = 'east'",
)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--evidence", required=True)
    parser.add_argument("--legacy-pause-proof", required=True)
    args = parser.parse_args()
    evidence = Path(args.evidence).resolve()
    owned = source.REPO / "evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-product"
    if owned not in evidence.parents:
        raise SystemExit("evidence outside owned source")
    evidence.mkdir(parents=True, exist_ok=False)
    source.EVIDENCE = evidence
    proof = Path(args.legacy_pause_proof).read_bytes()
    for token in (b"pause_host=172.16.143.10 pause_port=9030", b"pause_matched=4",
                  b"pause_enabled_remaining=0", b"pause_source_writes=0", b"pause_resume=not_called"):
        if token not in proof:
            raise SystemExit("legacy source pause proof missing required fact")
    source.write(evidence / "legacy-pause-proof.sha256", hashlib.sha256(proof).hexdigest())
    # This target is owned; this SQL is an administrative read, not a lease
    # insertion or a write to the source. Source DML remains impossible until
    # the actual worker pause is verified.
    sql = """SELECT count(*),count(*) FILTER (WHERE enabled),
      (SELECT count(*) FROM jw_native.occurrences o JOIN jobs.tasks t ON t.id=o.task_id
       JOIN jw_native.bindings b ON b.id=o.binding_id
       WHERE b.source_id='dorispreview' AND t.state IN ('queued','running')),
      (SELECT count(*) FROM jw_native.watermarks w JOIN jw_native.bindings b ON b.id=w.binding_id
       JOIN jobs.tasks t ON t.id=w.task_id WHERE b.source_id='dorispreview'
       AND w.source_generation=3 AND t.state='succeeded'),
      (SELECT count(*) FROM _oo.table_freshness WHERE source='dorispreview' AND source_generation=3)
      FROM jw_native.bindings WHERE source_id='dorispreview';"""
    checked = subprocess.run(["docker", "exec", "oo-dl01w1-43-native-pg", "psql",
                              "-X", "-U", "oo_admin", "-d", "dorisnative", "-Atc", sql],
                             capture_output=True)
    (evidence / "target-pause.stdout").write_bytes(checked.stdout)
    (evidence / "target-pause.stderr").write_bytes(checked.stderr)
    source.write(evidence / "target-pause.exit", str(checked.returncode))
    if checked.returncode or checked.stdout.strip() != b"7|0|0|7|7":
        raise SystemExit("seven target jobs have not applied generation3 and paused/drained")
    if source.sha256_file(source.ELF) != source.ELF_SHA:
        raise SystemExit("source reader changed")
    if source.run_tick("windowbefore", 6000, "windowbefore", 180) or compare("windowbefore"):
        raise SystemExit("independent before source cells differ")
    baseline = source.load_tick("windowbefore")
    rows = {table["name"]: table["rows"] for table in baseline["tables"]}
    if any(row.startswith("15\x1f") for row in rows["dup_events"]):
        raise SystemExit("new duplicate id already exists")
    if rows["nz_obs"] != ["1\x1f\\N"] or "5\x1f4.0000\x1fwindow" not in rows["uniq_acct"]:
        raise SystemExit("fixture changed before authorized window")
    if "2026-03-20\x1feast\x1f1.50" not in rows["part_day"]:
        raise SystemExit("partition deletion preimage absent")
    start = source.audit_size()
    sql_path = evidence / "dba-window.sql"
    source.write(sql_path, ";\n".join(STATEMENTS) + ";\n")
    result = source.mysql("oo_dba", source.DBA_PASSWORD, [sql_path], "dba-window")
    source.write(evidence / "dba-window.json", json.dumps(
        {"account": "oo_dba", "statements": STATEMENTS, "exit": result.returncode,
         "product_reader_write_attempts": 0, "audit_start_offset": start}, indent=2))
    if result.returncode:
        raise SystemExit("DBA window failed; preserve partial mutation and investigate")
    # A different period permits an immediate independent post-window tick.
    if source.run_tick("windowafter", 13000, "windowafter", 180) or compare("windowafter"):
        raise SystemExit("independent post-window source cells differ")
    after = source.load_tick("windowafter")
    post = {table["name"]: table["rows"] for table in after["tables"]}
    expected = {name: list(values) for name, values in rows.items()}
    expected["dup_events"] += ["15\x1fnative_full\x1f3.25"] * 2
    expected["uniq_acct"].remove("5\x1f4.0000\x1fwindow")
    expected["uniq_acct"].append("5\x1f4.0000\x1fnative_full")
    expected["nz_obs"] = []
    expected["part_day"].remove("2026-03-20\x1feast\x1f1.50")
    expected["part_day"].append("2026-04-21\x1feast\x1f2.50")
    mismatches = [name for name in rows if sorted(expected[name]) != sorted(post[name])]
    before_versions = {p["PartitionName"]: p["VisibleVersion"] for p in baseline["partitions"]}
    after_versions = {p["PartitionName"]: p["VisibleVersion"] for p in after["partitions"]}
    changed = [name for name in before_versions if before_versions[name] != after_versions.get(name)]
    source.write(evidence / "window-result.json", json.dumps(
        {"mismatch_tables": mismatches, "duplicate15_multiplicity": post["dup_events"].count("15\x1fnative_full\x1f3.25"),
         "nz_rows": len(post["nz_obs"]), "before_partition_versions": before_versions,
         "after_partition_versions": after_versions, "changed_partitions": changed,
         "target_import": "not_this_command", "scheduler_resume": "not_this_command"}, indent=2))
    source.sha_tree()
    if mismatches or len(changed) != 2:
        raise SystemExit("actual window differs or partition versions did not move")
    print("Doris DBA window and independent full typed source cells verified")


if __name__ == "__main__":
    main()
