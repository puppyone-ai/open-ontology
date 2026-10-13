#!/usr/bin/env python3
"""One owned-fixture DBA window; product reader does not issue writes."""
import argparse
import json
from pathlib import Path
import subprocess

CH = "oo-dl01w1-40-gpt-fresh-ch"
BASELINE_UUID = "3cad2d69-4353-4ab2-88b0-dd2ef92ee466"


def query(user, password, sql, query_id):
    shell = ("export CLICKHOUSE_PASSWORD=\"$(tr -d '[:space:]' < \"$1\")\"; "
             'clickhouse-client --user "$2" --query_id "$4" --query "$3"')
    return subprocess.run(["docker", "exec", CH, "bash", "-lc", shell, "bash", password,
                           user, sql, query_id], capture_output=True)


def save(out, label, result):
    for suffix, content in (("stdout", result.stdout), ("stderr", result.stderr),
                            ("exit", (str(result.returncode) + "\n").encode())):
        (out / (label + "." + suffix)).write_bytes(content)
    if result.returncode:
        raise RuntimeError(label + " failed; raw retained")
    return result.stdout.decode().strip()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    owner = subprocess.run(["docker", "inspect", CH, "--format", "{{json .Config.Labels}}"],
                           capture_output=True, check=True)
    (out / "resource-labels.json").write_bytes(owner.stdout)
    labels = json.loads(owner.stdout)
    if labels.get("oo.owner") != "oo-dl01w1-40" or labels.get("oo.kind") != "owned-synthetic-source":
        raise RuntimeError("source ownership mismatch")
    reader = ("oo_dl01w1_40_reader", "/etc/oo/reader.password")
    dba = ("oo_dl01w1_40_dba", "/etc/oo/dba.password")
    def run(label, actor, sql):
        return save(out, label, query(*actor, sql, "oo-dl01w1-40-c4-update-delete-" + label))
    epoch = run("before-epoch", reader, "SELECT toString(uuid) FROM system.tables WHERE database='oo_dl01w40' AND name='readings' FORMAT TSV")
    if epoch != BASELINE_UUID:
        raise RuntimeError("source table epoch changed")
    before = run("before-rows", reader, "SELECT reading_id,qty,_partition_id FROM oo_dl01w40.readings FINAL ORDER BY reading_id FORMAT JSONEachRow")
    if [(int(x["reading_id"]),int(x["qty"]),x["_partition_id"]) for x in map(json.loads,before.splitlines())] != [(101,7,"202403"),(102,0,"202401"),(103,0,"202401")]:
        raise RuntimeError("baseline rows differ; no DBA mutation")
    run("dba-update-102", dba, "ALTER TABLE oo_dl01w40.readings UPDATE qty = 9 WHERE reading_id = 102 SETTINGS mutations_sync = 2")
    run("dba-delete-101", dba, "ALTER TABLE oo_dl01w40.readings DELETE WHERE reading_id = 101 SETTINGS mutations_sync = 2")
    pending = run("after-pending", reader, "SELECT count() FROM system.mutations WHERE database='oo_dl01w40' AND table='readings' AND is_done=0 FORMAT TSV")
    if pending != "0":
        raise RuntimeError("mutation pending; no capture")
    after = run("after-rows", reader, "SELECT reading_id,qty,_partition_id FROM oo_dl01w40.readings FINAL ORDER BY reading_id FORMAT JSONEachRow")
    if [(int(x["reading_id"]),int(x["qty"]),x["_partition_id"]) for x in map(json.loads,after.splitlines())] != [(102,9,"202401"),(103,0,"202401")]:
        raise RuntimeError("post mutation rows differ")
    if run("after-epoch", reader, "SELECT toString(uuid) FROM system.tables WHERE database='oo_dl01w40' AND name='readings' FORMAT TSV") != epoch:
        raise RuntimeError("source table epoch changed during DBA window")
    print(json.dumps({"owned_fixture": CH, "table_epoch": epoch,
                      "updated": 102, "physically_deleted": 101, "mutation_pending": False}))


if __name__ == "__main__":
    main()
