#!/usr/bin/env python3
"""Oracle LogMiner C1 over a backlog larger than one batch (owned fixture only).

T10 found the defect: the first C1 batch mined every commit from the cursor to
the current SCN and refused with `logminer_batch_over_budget` once a busy
foreground had committed more changes than one batch may carry; no later call
could make progress. This binds C1 on the real loader, commits many small
transactions past a deliberately small batch budget, drains C1 and compares
every source row with the target.
"""
import argparse
import json
from pathlib import Path
import shutil
import signal

import runner
import t1
import t3t4
from oracle_t3t4 import OracleHarness

BATCH_ROWS = 50
COMMITS = 30
ROWS_PER_COMMIT = 20


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True, choices=("19", "23"))
    parser.add_argument("--evidence", type=Path, required=True)
    parser.add_argument("--binary", type=Path, required=True)
    args = parser.parse_args(argv)
    args.evidence = args.evidence.resolve()
    if not args.evidence.is_relative_to(runner.ROOT / "evidence") or args.evidence.exists():
        parser.error("evidence must be a new directory under worktree evidence/")
    t1.LANE = "t10"
    args.evidence.mkdir(parents=True, mode=0o700)
    args.platform, args.health_timeout, args.scale, args.tables, args.worker_image = None, 600, 0, 0, None
    run = t3t4.Source(args.evidence, "oracle", args.version, args)
    run.receipt = {}
    h = OracleHarness(run, args.binary.resolve())
    report = {"engine": "oracle", "version": args.version, "batch_rows": BATCH_ROWS,
              "backlog_rows": COMMITS * ROWS_PER_COMMIT, "steps": [], "errors": []}
    sid = "backlog"
    try:
        h.prepare()
        run.record_version()
        h.c1_binding(sid, ["other"])
        h.c1(sid)  # registers the binding at the aligned baseline
        # The batch budget follows --chunk-rows; make it smaller than the backlog.
        next(s for s in h.stack["sources"] if s["id"] == sid)["_chunk_rows"] = BATCH_ROWS
        statements = []
        for commit in range(COMMITS):
            first = 1000 + commit * ROWS_PER_COMMIT
            statements.append(f"INSERT INTO other SELECT level+{first},'backlog',{commit} "
                              f"FROM dual CONNECT BY level <= {ROWS_PER_COMMIT};COMMIT;")
        statements.append("UPDATE other SET payload='late' WHERE id=1001;COMMIT;"
                          "DELETE FROM other WHERE id=1002;")
        h.src("".join(statements))
        code, results, error = h.c1(sid, required=False)
        comparison = h.compare(sid)
        report["steps"].append({
            "step": "drain_backlog", "exit": code, "error": error, "calls": len(results),
            "batches": sum(r.get("batches", 0) for result in results for r in result["receipts"]),
            "equal": h.equal(comparison), "rows": {k: v.get("rows") for k, v in comparison.items()}})
    except Exception as exc:
        report["errors"].append(type(exc).__name__)
    finally:
        report["cleanup_failures"] = []
        for close in (h.close_worker, h.target.close, run.cleanup):
            try:
                report["cleanup_failures"].extend(close() or [])
            except Exception as exc:
                report["cleanup_failures"].append(type(exc).__name__)
        shutil.rmtree(h.store, ignore_errors=True)
        step = report["steps"][0] if report["steps"] else {}
        report["result"] = "pass" if (
            not report["errors"] and not report["cleanup_failures"] and step.get("exit") == 0
            and step.get("equal") and step.get("batches", 0) > 1) else "fail"
        runner.write_json(run.path / "summary.json", report)
        runner.checksums(run.path)
    print(json.dumps({"result": report["result"], "run_id": run.path.name}))
    return 0 if report["result"] == "pass" else 1


if __name__ == "__main__":
    def interrupted(signum, frame):
        raise KeyboardInterrupt(f"signal {signum}")
    signal.signal(signal.SIGTERM, interrupted)
    raise SystemExit(main())
