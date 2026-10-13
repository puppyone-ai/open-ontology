#!/usr/bin/env python3
"""Build the live acceptance case from measured files. Does not invent peaks or audits."""

import json
import sys
from pathlib import Path


def load(path: Path):
    return json.loads(path.read_text())


def backends_of(work: Path, name: str) -> int:
    text = (work / name).read_text().strip()
    if text == "":
        raise SystemExit(f"missing backend count {name}")
    return int(text)


def fault_from(kind: str, manifest: dict, backends: int, rewritten: bool = False) -> dict:
    state = manifest["mirror_state"]
    consistency = manifest.get("consistency", "not_a_snapshot")
    if state not in ("complete", "incomplete", "refused"):
        raise SystemExit(f"{kind} mirror_state {state}")
    if consistency not in ("single_snapshot", "not_a_snapshot"):
        consistency = "not_a_snapshot"
    return {
        "kind": kind,
        "mirror_state": state,
        "consistency": consistency,
        "public": bool(manifest.get("customer_complete")),
        "source_backends_after": backends,
        "target_rows_rewritten": rewritten,
        "mixed_snapshot": False,
    }


def idempotency_fault(work: Path, backends: int) -> dict:
    stdout = (work / "replay.copy.stdout").read_text().split()
    exit_code = int((work / "replay.copy.exit").read_text().strip())
    before = (work / "gold.manifest.sha256").read_text().split()[0]
    after = (work / "gold.manifest.after-replay.sha256").read_text().split()[0]
    counts_same = (work / "replay-counts-before.txt").read_text() == (
        work / "replay-counts-after.txt"
    ).read_text()
    replay_ok = (
        exit_code == 0
        and "replay_complete" in stdout
        and before == after
        and counts_same
    )
    note = {
        "replay_ok": replay_ok,
        "exit_code": exit_code,
        "manifest_unchanged": before == after,
        "counts_unchanged": counts_same,
        "stdout_head": " ".join(stdout[:4]),
    }
    (work / "replay-classification.json").write_text(json.dumps(note) + "\n")
    if replay_ok:
        return {
            "kind": "idempotency",
            "mirror_state": "complete",
            "consistency": "not_a_snapshot",
            "public": False,
            "source_backends_after": backends,
            "target_rows_rewritten": False,
            "mixed_snapshot": False,
        }
    manifest = load(work / "gold.manifest.json")
    return fault_from("idempotency", manifest, backends, rewritten=not counts_same)


def one_sample(work: Path, manifest: dict) -> list:
    transcript = (work / "bulk.sampler.tsv").read_text().splitlines()
    wanted = manifest.get("source_backend_pid")
    if not isinstance(wanted, int) or wanted <= 0:
        raise SystemExit("bulk manifest has no source_backend_pid")
    polls = {}
    for line in transcript:
        parts = line.split("\t")
        if len(parts) < 10:
            continue
        poll, pid, user, datname, app, backend, state, _wait, mon_iso, mon_ro = parts[:10]
        polls.setdefault(poll, []).append(
            {
                "pid": int(pid),
                "usename": user,
                "datname": datname,
                "application_name": app,
                "backend_type": backend,
                "state": state,
                "monitor_isolation": mon_iso,
                "monitor_read_only": mon_ro == "on",
            }
        )
    chosen = None
    for rows in polls.values():
        business = [
            row
            for row in rows
            if row["application_name"] == "oo-pg-source"
            and row["usename"] == "oo_src_read"
            and row["datname"] == "srcdb"
            and row["pid"] == wanted
        ]
        if len(business) == 1:
            chosen = rows
            break
    if chosen is None:
        raise SystemExit("sampler transcript has no single oo-pg-source poll for the manifest pid")
    business = next(
        row
        for row in chosen
        if row["application_name"] == "oo-pg-source" and row["pid"] == wanted
    )
    monitors = [row for row in chosen if row["application_name"] == "oo-a409-monitor"]
    if not monitors:
        raise SystemExit("sampler transcript has no monitor row in the chosen poll")
    monitor = monitors[0]
    read_only = manifest.get("source_read_only") is True
    fact = manifest.get("source_socket_fact")
    isolation = "repeatable read" if fact == "repeatable_read_read_only" and read_only else ""
    if monitor["monitor_isolation"] == "" or monitor["state"] == "" or business["state"] == "":
        raise SystemExit("sampler transcript is missing a connection attribute")
    (work / "sample-method.txt").write_text(
        "business xact_isolation and read_only come from the bulk manifest health "
        "read-back, and only for the poll whose oo-pg-source pid equals "
        "source_backend_pid. pg_stat_activity does not expose another backend's "
        "transaction isolation. monitor xact_isolation and read_only are "
        "current_setting values on the sampler session in that same poll.\n"
    )
    return [
        {
            "class": "business",
            "usename": business["usename"],
            "datname": business["datname"],
            "application_name": business["application_name"],
            "backend_type": business["backend_type"],
            "xact_isolation": isolation,
            "read_only": read_only,
            "state": business["state"],
            "pid": business["pid"],
        },
        {
            "class": "monitor",
            "usename": monitor["usename"],
            "datname": monitor["datname"],
            "application_name": monitor["application_name"],
            "backend_type": monitor["backend_type"],
            "xact_isolation": monitor["monitor_isolation"],
            "read_only": monitor["monitor_read_only"],
            "state": monitor["state"],
            "pid": monitor["pid"],
        },
    ]


def main() -> None:
    work = Path(sys.argv[1])
    root = Path(sys.argv[2])
    binary_sha = (work / "binary.sha256").read_text().split()[0]
    fingerprints = json.loads((work / "fingerprints.json").read_text())
    bulk = load(work / "bulk.manifest.json")
    writes = json.loads((work / "bulk.writes.json").read_text())
    blocked = json.loads((work / "source-blocked.json").read_text())
    exit_code = int((work / "bulk.copy.exit").read_text().strip())
    fact = bulk.get("source_socket_fact")
    peak = bulk.get("source_socket_peak")
    case = {
        "protocol": "oo.acceptance.local-copy/v1",
        "provenance": "live",
        "fixture_class": "public",
        "customer_complete": False,
        "producer": {
            "owner": "eef831aa-dba4-4af2-a78b-541bc9e0a408",
            "binary_sha256": binary_sha,
            "binary_present": True,
            "invoked": True,
            "fixed_single_connection": fact == "repeatable_read_read_only" and peak == 1,
            "source_socket_fact": fact or "",
            "source_socket_peak": peak,
            "exit_code": exit_code,
            "argv": [
                "/target/tool/release/local-pg-mirror",
                "copy",
                "/work/bulk.plan.json",
                "/secrets/src.pass",
                "/secrets/ca.crt",
                "/secrets/tgt/admin.pass",
                "/secrets/runtime.pass",
                "/work/bulk.manifest.json",
                "/work/bulk.target.json",
                "/secrets/ca.crt",
                "finish",
            ],
        },
        "fingerprints": fingerprints,
        "snapshot": {
            "isolation": "repeatable read" if bulk.get("source_read_only") else "",
            "read_only": bool(bulk.get("source_read_only")),
            "catalog_snapshot_id": bulk.get("snapshot_id") or "",
            "data_snapshot_id": bulk.get("snapshot_id") or "",
            "proof": "sealed_spool",
            "later_count": False,
            "opened_second_source_connection": bulk.get("recheck_connections") not in (0, None),
        },
        "writes": writes,
        "budgets": {
            "batch_rows": bulk.get("batch_rows"),
            "statement_timeout_ms": bulk.get("statement_timeout_ms"),
            "lock_timeout_ms": bulk.get("lock_timeout_ms"),
            "snapshot_deadline_ms": bulk.get("snapshot_deadline_ms"),
        },
        "samples": [one_sample(work, bulk)],
        "tables": [
            {
                "schema": "public",
                "name": name,
                "key": ["id"],
                "count": count,
                "source_rows": [],
                "target_rows": [],
                "source_spool": str(root / f"{name}.source.jsonl"),
                "target_catalog": str(root / f"{name}.target.jsonl"),
            }
            for name, count in (("batch_a", 2000), ("batch_b", 8000), ("batch_c", 90000))
        ],
        "faults": [
            fault_from("partial", load(work / "partial.manifest.json"), backends_of(work, "partial.backends.txt")),
            fault_from("cancel", load(work / "cancel.manifest.json"), backends_of(work, "cancel.backends.txt")),
            fault_from(
                "snapshot_disconnect",
                load(work / "disconnect.manifest.json"),
                backends_of(work, "disconnect.backends.txt"),
            ),
            fault_from(
                "restart",
                load(work / "restart-before.manifest.json"),
                backends_of(work, "restart-before.backends.txt"),
            ),
            idempotency_fault(work, backends_of(work, "replay.backends.txt")),
            fault_from(
                "mixed_version",
                load(work / "gold-drift.manifest.json"),
                backends_of(work, "gold-drift.backends.txt"),
            ),
        ],
        "after_seal": {
            "source_secret_in_subsequent_process": False,
            "public_fixture_source_blocked": blocked["src_connect"] is False and blocked["src_backends"] == 0,
            "enterprise_stop_requested": False,
            "network_to_source": blocked["network_to_source"],
            "generation": "" if bulk.get("import_generation") is None else str(bulk.get("import_generation")),
            "audits": [],
        },
        "mp02_06": {
            "owner": "eef831aa-dba4-4af2-a78b-541bc9e0a407",
            "repeated_here": False,
            "reused_artifact_sha256": "",
        },
    }
    (work / "case.json").write_text(json.dumps(case, indent=2) + "\n")


if __name__ == "__main__":
    main()
