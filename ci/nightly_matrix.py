#!/usr/bin/env python3
"""Nightly P0 engine matrix: the fast L1 equivalence classes plus the L2 fault layer.

Runs `oo-test fast` (L1: T0/T1/T7/T8 on every P0 engine version, T2/T4 on each
equivalence class's daily representative; L2: deterministic T3 fault pairs on
each engine's representative) for PostgreSQL, MySQL and Oracle. Real product
binaries, real pinned engine images; no hook stubs.

A cell this host cannot run is an explicit skip with a fixed reason in the
receipt, never a pass: only the rules in `unavailable()` can skip, each checks
the actual condition, and every other cell must execute and pass. No
scheduler installation or shared-file writes.
"""
import argparse
from datetime import datetime, timezone
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import uuid

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools/oo-test"))
import layers  # noqa: E402
import runner  # noqa: E402

ENGINES = ("postgres", "mysql", "oracle")
SUITES = ["T0", "T1", "T2", "T4", "T7", "T8"]


def summarize(runs):
    signatures = [
        sorted((r["target"], r["version"], r["suite"], r.get("variant") or "", r["result"])
               for r in run["rows"])
        for run in runs
    ]
    return {
        "schema": "oo-nightly-stability/v1",
        "attempts": len(runs),
        "stable": bool(runs) and all(run["exit"] == 0 and run["rows"] and
                                    all(row["result"] == "pass" for row in run["rows"])
                                    for run in runs),
        "repeatable_outcomes": bool(signatures) and all(s == signatures[0] for s in signatures),
        "consecutive_nights": False,
        "runs": [{k: v for k, v in run.items() if k != "rows"} for run in runs],
    }


def native_platform():
    arch = subprocess.check_output(["docker", "info", "--format", "{{.Architecture}}"],
                                   text=True, timeout=60).strip()
    return "linux/arm64" if arch in ("arm64", "aarch64") else "linux/amd64"


def image_present(item, native):
    platform = native if native in item["platforms"] else "linux/amd64"
    digest = item["platforms"].get(platform, {}).get("digest")
    if not digest:
        return False
    image = item["ref"] + "@" + digest
    return subprocess.run(["docker", "image", "inspect", image], capture_output=True,
                          timeout=60).returncode == 0


def unavailable(engine, version, native, env):
    """Reasons this host cannot run the cell; empty means it must run and pass."""
    item = runner.resolve(engine, version)
    reasons = []
    if native not in item["platforms"] and "linux/amd64" not in item["platforms"]:
        reasons.append(f"locked image {item['ref']} has no {native} build "
                       f"(locked: {', '.join(sorted(item['platforms']))})")
    if item["ref"].startswith("container-registry.oracle.com/") and not image_present(item, native):
        reasons.append("image requires an Oracle container registry sign-in and is not present "
                       "locally; this workflow holds no repository credentials")
    if engine == "oracle" and version == "11" and not env.get("OO_TEST_ORACLE_WORKER_IMAGE"):
        reasons.append("11g suites need the digest-pinned thick worker image "
                       "(OO_TEST_ORACLE_WORKER_IMAGE); it contains Oracle Instant Client, "
                       "is built only locally by testing/engines/oracle/worker/build.py and "
                       "is never published")
    return reasons


def plan(day, native, env):
    cells, skipped = [], []
    for cell in layers.l1_cells(list(ENGINES), SUITES, day):
        reasons = unavailable(cell["engine"], cell["version"], native, env)
        (skipped if reasons else cells).append(dict(cell, layer="L1", reasons=reasons))
    l2 = []
    for engine in ENGINES:
        version = layers.representative(layers.P0[engine], day)
        reasons = unavailable(engine, version, native, env)
        entry = {"engine": engine, "version": version, "layer": "L2", "reasons": reasons}
        (skipped if reasons else l2).append(entry)
    for entry in skipped:
        entry["result"] = "skip"
    return cells, l2, skipped


def fast(arguments, run_id, output, name):
    command = [str(ROOT / "tools/oo-test/oo-test"), "fast", *arguments, "--run-id", run_id]
    result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True)
    (output / f"{name}.stdout").write_text(result.stdout)
    (output / f"{name}.stderr").write_text(result.stderr)
    report = ROOT / "evidence" / run_id / "fast-report.json"
    data = json.loads(report.read_text()) if report.is_file() else None
    return {"command": command, "exit": result.returncode, "run_id": run_id,
            "report_sha256": hashlib.sha256(report.read_bytes()).hexdigest() if data else None,
            "status": data.get("status") if data else None}, data


def rows_from(report, layer, run_id, sha):
    rows = []
    for entry in (report or {}).get("layers", {}).get(layer, {}).get("results", []):
        base = {"target": entry["engine"], "version": entry["version"], "run_id": run_id,
                "sha256": sha, "layer": layer}
        if layer == "L1":
            suites = entry.get("suites", {})
            verdicts = {k: v for k, v in suites.items() if re.fullmatch(r"T\d", k)}
            for suite, verdict in sorted(verdicts.items()):
                rows.append(dict(base, suite=suite, result="pass" if verdict == "passed" else "fail"))
            if not verdicts or entry.get("status") != "passed":
                rows.append(dict(base, suite="cell", result="fail",
                                 error=suites.get("error") or entry.get("reason")))
        else:
            rows.append(dict(base, suite="T3-fast", result="pass" if entry.get("status") == "passed" else "fail"))
    return rows


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repeats", type=int, default=1)
    parser.add_argument("--day", help="rotation day, YYYY-MM-DD (default today)")
    args = parser.parse_args(argv)
    if not 1 <= args.repeats <= 3:
        parser.error("--repeats must be 1..3")
    day = datetime.strptime(args.day, "%Y-%m-%d").date() if args.day else layers.today()
    evidence = ROOT / "evidence"
    evidence.mkdir(exist_ok=True)
    # One launcher per worktree; no daemon, LaunchAgent, or unrelated processes.
    with (evidence / ".nightly-matrix.lock").open("w") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            print("another nightly launcher owns this worktree", file=sys.stderr)
            return 2
        run_id = ("nightly-" + datetime.now(timezone.utc).strftime("%Y%m%d%H%M%S-")
                  + uuid.uuid4().hex[:8])
        output = evidence / run_id
        output.mkdir()
        print(output.relative_to(ROOT), flush=True)
        cells, l2, skipped = plan(day, native_platform(), os.environ)
        runs, commands = [], []
        for attempt in range(1, args.repeats + 1):
            rows, exits = [], []
            if cells:
                rid = f"{run_id}-a{attempt}-l1"
                ran, report = fast(["--layer", "L1", "--day", str(day),
                                    "--engines", ",".join(sorted({c["engine"] for c in cells})),
                                    "--cells", ",".join(f"{c['engine']}-{c['version']}" for c in cells)],
                                   rid, output, f"attempt-{attempt}-l1")
                commands.append(ran)
                exits.append(ran["exit"])
                rows += rows_from(report, "L1", rid, ran["report_sha256"])
            if l2:
                rid = f"{run_id}-a{attempt}-l2"
                ran, report = fast(["--layer", "L2", "--day", str(day),
                                    "--engines", ",".join(e["engine"] for e in l2)],
                                   rid, output, f"attempt-{attempt}-l2")
                commands.append(ran)
                exits.append(ran["exit"])
                rows += rows_from(report, "L2", rid, ran["report_sha256"])
            # Every planned runnable cell must appear; a missing row is a failure.
            seen = {(r["target"], r["version"], r["layer"]) for r in rows}
            for cell in cells + l2:
                key = (cell["engine"], cell["version"], cell["layer"])
                if key not in seen:
                    rows.append({"target": key[0], "version": key[1], "layer": key[2],
                                 "suite": "cell", "result": "fail", "error": "no_result"})
            runs.append({"exit": max(exits) if exits else 1, "attempt": attempt, "rows": rows})
        summary = summarize(runs)
        receipt = {
            "schema": "oo-nightly-matrix/v2", "run_id": run_id, "day": str(day),
            "source_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT,
                                                     text=True).strip(),
            "scope": "fast L1 equivalence classes and L2 fault pairs on P0 engines; "
                     "not the full T0-T8 matrix, not product sign-off",
            "executed": [{k: c[k] for k in ("engine", "version", "layer")} for c in cells + l2],
            "skipped": skipped, "commands": commands, "stability": summary,
            "rows": [row for run in runs for row in run["rows"]],
            "support_matrix_candidate": None,
            "support_matrix_note": "fast-layer rows are not full-matrix evidence; "
                                   "the support matrix is not regenerated from them",
        }
        receipt["status"] = "passed" if summary["stable"] and cells else "failed"
        (output / "nightly-receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
        (output / "stability.json").write_text(json.dumps(summary, indent=2) + "\n")
        print(json.dumps({"run_id": run_id, "status": receipt["status"],
                          "executed": len(cells) + len(l2), "skipped": [
                              f"{s['layer']} {s['engine']}-{s['version']}: {'; '.join(s['reasons'])}"
                              for s in skipped],
                          "stability": {k: summary[k] for k in ("stable", "attempts")}}), flush=True)
        return 0 if receipt["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
