#!/usr/bin/env python3
"""Live evaluation of the whole pipeline on the golden set, model OFF and ON
(30 section 4, L4 targets with the model: precision >= 0.85, recall >= 0.75).

Each mode runs `cargo test -p oo-docs --test eval_live` against a labelled
Postgres container: a real `oo-docs-engine`, the Docling sidecar container and,
for ON, the real model egress. The engine's entities and facts are written in
the labels format and scored by `oo-eval docs` against the golden labels. The
model endpoint and key are only ever passed as file paths (`file:` references):

  --endpoint-file FILE   0600 file holding the model's base URL
  --key-file FILE        0600 file holding the key
  --model-id ID

Usage: tools/oo-test/super_ontology_eval_live.py [--modes off,on] [--workers 4]
       [--endpoint-file F --key-file F --model-id ID] [--evidence-dir DIR]
Evidence: <dir>/model-off/ and <dir>/model-on/ with report.md, report.json (all candidates),
report-published.md, report-published.json (published facts only) and run.json.
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.dont_write_bytecode = True
import super_ontology_l2 as l2  # noqa: E402

REPO = l2.REPO
l2.LANE = "oo.lane=" + os.environ.get("OO_TEST_LANE", "super-ontology-be4")


def latency_summary(path):
    """p50/p95/max seconds of the wire calls (`<millis> <ok|error> <tokens out>` lines of OO_DOCS_MODEL_LATENCY_LOG)."""
    rows = [r for r in (ln.split() for ln in Path(path).read_text().splitlines()) if len(r) >= 3]
    ok = sorted(int(r[0]) / 1000 for r in rows if r[1] == "ok")
    errs = {}
    for r in rows:
        if r[1] != "ok":
            errs[r[1]] = errs.get(r[1], 0) + 1

    def q(p):
        return round(ok[min(len(ok) - 1, int(p * len(ok)))], 1) if ok else None
    return {"calls": len(rows), "ok": len(ok), "errors": errs, "p50_s": q(0.5), "p95_s": q(0.95), "p99_s": q(0.99), "max_s": q(1.0),
            "tokens_out_mean": round(sum(int(r[2]) for r in rows if r[1] == "ok") / max(1, len(ok)))}


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--modes", default="off,on")
    ap.add_argument("--workers", default="4")
    ap.add_argument("--endpoint-file")
    ap.add_argument("--key-file")
    ap.add_argument("--model-id")
    ap.add_argument("--run-id", default=time.strftime("%Y%m%d-%H%M%S"))
    ap.add_argument("--golden", default="testing/super-ontology/golden", help="golden or holdout set (docs/ and labels/)")
    ap.add_argument("--keep-out", help="keep raw predictions under this directory instead of a temp dir")
    ap.add_argument("--evidence-dir", default=str(REPO / "testing/super-ontology/evidence/L4/eval"))
    args = ap.parse_args()
    ev = l2.Evidence(Path(args.evidence_dir), args.run_id)
    pg = l2.Postgres(ev, args.run_id)
    if args.keep_out:
        out_root = Path(args.keep_out)
        out_root.mkdir(parents=True, exist_ok=True)
    else:
        out_root = Path(tempfile.mkdtemp(prefix="oo-eval-live-", dir="/tmp"))
    ok = True
    try:
        pg.start()
        for mode in args.modes.split(","):
            out = out_root / mode
            env = dict(os.environ, OO_DOCS_TEST_PG_PORT=str(pg.port), OO_EVAL_OUT=str(out), OO_EVAL_GOLDEN=str(Path(args.golden).resolve()),
                       CARGO_BUILD_JOBS=os.environ.get("CARGO_BUILD_JOBS", "6"))
            if mode == "on":
                if not (args.endpoint_file and args.key_file and args.model_id):
                    raise SystemExit("--endpoint-file, --key-file and --model-id are required for mode on")
                env.update(OO_EVAL_MODEL="1", OO_EVAL_LLM_WORKERS=args.workers, OO_EVAL_ENDPOINT_FILE=args.endpoint_file,
                           OO_EVAL_KEY_FILE=args.key_file, OO_EVAL_MODEL_ID=args.model_id,
                           OO_DOCS_MODEL_LATENCY_LOG=str(out_root / "latency.log"))
            argv = ["cargo", "test", "-p", "oo-docs", "--locked", "--test", "eval_live", "--", "--ignored", "--nocapture", "--test-threads=1"]
            ev.log(f"$ [{mode}] " + " ".join(argv))
            p = subprocess.run(argv, cwd=REPO, env=env, capture_output=True, text=True, timeout=7200)
            ev.check(f"the {mode} run finished", p.returncode == 0, f"exit {p.returncode}")
            if p.returncode != 0:
                ok = False
                ev.log((p.stdout + p.stderr)[-1500:])
                continue
            rep = out / "report"
            q = subprocess.run(["cargo", "run", "-q", "-p", "oo-eval", "--locked", "--", "docs", "--golden", args.golden,
                                "--predictions", str(out / "predictions"), "--out", str(rep)], cwd=REPO, capture_output=True, text=True, timeout=600)
            ev.log(q.stdout.strip() or q.stderr.strip()[-300:])
            # What the product acts on: published facts only (held candidates left out).
            repp = out / "report-published"
            q = subprocess.run(["cargo", "run", "-q", "-p", "oo-eval", "--locked", "--", "docs", "--golden", args.golden,
                                "--predictions", str(out / "predictions-published"), "--out", str(repp)], cwd=REPO, capture_output=True, text=True, timeout=600)
            ev.log("published only: " + (q.stdout.strip() or q.stderr.strip()[-300:]))
            dest = Path(args.evidence_dir) / f"model-{mode}"
            lat = out_root / "latency.log"
            if mode == "on" and lat.is_file():
                (out / "latency.json").write_text(json.dumps(latency_summary(lat), indent=1))
            dest.mkdir(parents=True, exist_ok=True)
            for src, name in ((rep / "report.md", "report.md"), (rep / "report.json", "report.json"), (repp / "report.md", "report-published.md"),
                              (repp / "report.json", "report-published.json"), (out / "run.json", "run.json"),
                              (out / "latency.json", "latency.json")):
                if src.is_file():
                    shutil.copyfile(src, dest / name)
        ok = ok and all(c["ok"] for c in ev.checks)
    finally:
        pg.stop()
        if not args.keep_out:
            shutil.rmtree(out_root, ignore_errors=True)
    print(f"# {sum(1 for c in ev.checks if c['ok'])}/{len(ev.checks)} checks passed")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
