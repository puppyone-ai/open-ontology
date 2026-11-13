#!/usr/bin/env python3
"""C5 over durable ranges that outgrew the chunk budget (owned fixture only).

T10 found the defect: durable C5 ranges never move, so an append tail
accumulated every row inserted after the baseline until reading it exceeded the
chunk budget and the table stayed stale (`table_budget`) on every later cycle.
This drives the real loader: baseline, tail growth to several budgets, then
three cycles (grown split, no-op, mixed changes inside the split children),
comparing every source row with the published target after each cycle.
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

CHUNK = 500
GROWTH = 6 * CHUNK


def insert_rows(h, start, count):
    if h.run.engine == "oracle":
        h.src(f"INSERT INTO bulk SELECT level+{start - 1},'grown-'||TO_CHAR(level),level "
              f"FROM dual CONNECT BY level <= {count};")
    elif h.run.engine == "postgres":
        h.src(f"INSERT INTO bulk SELECT i,'grown-'||i,i FROM generate_series({start},{start + count - 1}) i;")
    else:
        for first in range(start, start + count, 1000):
            last = min(first + 1000, start + count)
            h.src("INSERT INTO bulk VALUES" + ",".join(
                f"({i},'grown-{i}',{i})" for i in range(first, last)) + ";")


def top(h):
    _, value = h.src("SELECT TO_CHAR(MAX(id)) FROM bulk;" if h.run.engine == "oracle"
                     else "SELECT MAX(id) FROM bulk;")
    return int(value.strip())


def cycle(h, sid, label):
    code, receipts, error = h.c5(sid, required=False)
    table = next((r for r in receipts if r.get("mode") == "c5"), {})
    comparison = h.compare(sid)
    return {"step": label, "exit": code, "error": error, "status": table.get("status"),
            "stale_reason": table.get("stale_reason"), "ranges_replaced": table.get("ranges_replaced"),
            "new_generations": table.get("new_generations"), "equal": h.equal(comparison),
            "rows": {k: v.get("rows") for k, v in comparison.items()}}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--engine", required=True, choices=("postgres", "mysql", "oracle"))
    parser.add_argument("--version", required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    parser.add_argument("--binary", type=Path, required=True)
    args = parser.parse_args(argv)
    args.evidence = args.evidence.resolve()
    if not args.evidence.is_relative_to(runner.ROOT / "evidence") or args.evidence.exists():
        parser.error("evidence must be a new directory under worktree evidence/")
    t1.LANE = "t10"
    args.evidence.mkdir(parents=True, mode=0o700)
    args.platform, args.health_timeout, args.scale, args.tables, args.worker_image = None, 600, 0, 0, None
    run = t3t4.Source(args.evidence, args.engine, args.version, args)
    run.receipt = {}
    h = (OracleHarness if args.engine == "oracle" else t3t4.Harness)(run, args.binary.resolve())
    report = {"engine": args.engine, "version": args.version, "chunk_rows": CHUNK, "steps": [],
              "errors": []}
    sid = "growth"
    try:
        h.prepare()
        run.record_version()
        h.c5_binding(sid, ["bulk"], chunk_rows=CHUNK)
        report["steps"].append(cycle(h, sid, "baseline_noop"))
        start = top(h) + 1
        insert_rows(h, start, GROWTH)
        report["steps"].append(cycle(h, sid, "tail_grew_past_budget"))
        report["steps"].append(cycle(h, sid, "after_split_noop"))
        # Changes inside the split children, plus a further budget of growth.
        h.src(f"UPDATE bulk SET payload='changed' WHERE id={start + CHUNK // 2};"
              f"DELETE FROM bulk WHERE id={start + GROWTH - 1};")
        insert_rows(h, start + GROWTH, CHUNK + 1)
        report["steps"].append(cycle(h, sid, "children_changed_and_grew"))
    except Exception as exc:
        report["errors"].append(type(exc).__name__)
    finally:
        report["cleanup_failures"] = []
        for close in (h.target.close, run.cleanup):
            try:
                report["cleanup_failures"].extend(close() or [])
            except Exception as exc:
                report["cleanup_failures"].append(type(exc).__name__)
        shutil.rmtree(h.store, ignore_errors=True)
        expected = {"baseline_noop": "noop", "tail_grew_past_budget": "fresh",
                    "after_split_noop": "noop", "children_changed_and_grew": "fresh"}
        steps = {s["step"]: s for s in report["steps"]}
        report["result"] = "pass" if (
            not report["errors"] and not report["cleanup_failures"]
            and set(steps) == set(expected)
            and all(steps[k]["exit"] == 0 and steps[k]["equal"] and steps[k]["status"] == v
                    for k, v in expected.items())) else "fail"
        runner.write_json(run.path / "summary.json", report)
        runner.checksums(run.path)
    print(json.dumps({"result": report["result"], "run_id": run.path.name}))
    return 0 if report["result"] == "pass" else 1


if __name__ == "__main__":
    def interrupted(signum, frame):
        raise KeyboardInterrupt(f"signal {signum}")
    signal.signal(signal.SIGTERM, interrupted)
    raise SystemExit(main())
