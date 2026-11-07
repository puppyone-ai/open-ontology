#!/usr/bin/env python3
"""Local/CI nightly launcher. No scheduler installation or shared-file writes."""
import argparse
from datetime import datetime, timezone
import fcntl
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import uuid

ROOT = Path(__file__).resolve().parents[1]


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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repeats", type=int, default=1)
    parser.add_argument("matrix_args", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if not 1 <= args.repeats <= 3:
        parser.error("--repeats must be 1..3")
    extra = args.matrix_args
    if extra[:1] == ["--"]:
        extra = extra[1:]
    if any(arg.split("=")[0] in ("--all", "--engine", "--versions", "--suites") for arg in extra):
        parser.error("nightly target and suite coverage cannot be overridden")
    evidence = ROOT / "evidence"
    evidence.mkdir(exist_ok=True)
    # One launcher per worktree; no daemon, LaunchAgent, or unrelated processes.
    with (evidence / ".nightly-matrix.lock").open("w") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            print("another nightly launcher owns this worktree", file=sys.stderr)
            return 2
        run_id = "h-s5-nightly-" + datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ-") + str(uuid.uuid4())
        output = evidence / run_id
        output.mkdir()
        print(output.relative_to(ROOT), flush=True)
        runs, rows = [], []
        command = [str(ROOT / "tools/oo-test/oo-test"), "matrix", "--all",
                   "--suites", "T0-T4,T7,T8", *extra]
        for attempt in range(1, args.repeats + 1):
            result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True)
            (output / f"attempt-{attempt}.stdout").write_text(result.stdout)
            (output / f"attempt-{attempt}.stderr").write_text(result.stderr)
            lines = result.stdout.splitlines()
            path = (ROOT / lines[0]).resolve() if lines else None
            valid = path is not None and path.parent == evidence and path.name.startswith("h-s5-")
            index = path / "evidence-index.json" if valid else None
            current = json.loads(index.read_text()) if index and index.is_file() else []
            runs.append({"exit": result.returncode, "run_id": path.name if valid else None,
                         "index_sha256": hashlib.sha256(index.read_bytes()).hexdigest() if current else None,
                         "rows": current})
            rows.extend(current)
        (output / "evidence-index.json").write_text(json.dumps(rows, indent=2) + "\n")
        summary = summarize(runs)
        (output / "stability.json").write_text(json.dumps(summary, indent=2) + "\n")
        spec = importlib.util.spec_from_file_location(
            "support_generator", ROOT / "specs/scripts/gen_support_matrix.py")
        generator = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(generator)
        errors = generator.validate(rows)
        if errors:
            (output / "validation-errors.json").write_text(json.dumps(errors, indent=2) + "\n")
        else:
            (output / "support-matrix.candidate.json").write_text(generator.render(rows))
        print(json.dumps(summary), flush=True)
        return 0 if summary["stable"] and not errors else 1


if __name__ == "__main__":
    raise SystemExit(main())
