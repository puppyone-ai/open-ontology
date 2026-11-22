#!/usr/bin/env python3
"""Run the real pipeline on a corpus folder (docs/ + truth.json) and score entity quality (D90) and the review queue (D91).

  run_quality.py --corpus DIR --out DIR [--model-endpoint-file F --model-key-file F --model-id ID] [--label before|after]

Starts one labelled Postgres container, runs `eval_live` (the same test as the L4 harness) with OO_EVAL_GOLDEN=DIR and
writes <out>/<label>/{entities-all.jsonl,review-items.jsonl,run.json,quality.json}. Model files are passed as paths only.
"""
import argparse
import json
import os
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent.parent / "oo-test"))
sys.dont_write_bytecode = True
import super_ontology_l2 as l2  # noqa: E402

l2.LANE = "oo.lane=" + os.environ.get("OO_TEST_LANE", "super-ontology-aj")
REPO = l2.REPO


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--corpus", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--label", default="run")
    ap.add_argument("--model-endpoint-file")
    ap.add_argument("--model-key-file")
    ap.add_argument("--model-id")
    ap.add_argument("--workers", default="4")
    a = ap.parse_args()
    out = Path(a.out) / a.label
    out.mkdir(parents=True, exist_ok=True)
    run_id = time.strftime("%Y%m%d-%H%M%S")
    ev = l2.Evidence(Path(a.out) / "evidence", run_id)
    pg = l2.Postgres(ev, run_id)
    try:
        pg.start()
        env = dict(os.environ, OO_DOCS_TEST_PG_PORT=str(pg.port), OO_EVAL_OUT=str(out), OO_EVAL_GOLDEN=str(Path(a.corpus).resolve()),
                   CARGO_BUILD_JOBS=os.environ.get("CARGO_BUILD_JOBS", "6"))
        if a.model_id:
            env.update(OO_EVAL_MODEL="1", OO_EVAL_LLM_WORKERS=a.workers, OO_EVAL_ENDPOINT_FILE=a.model_endpoint_file,
                       OO_EVAL_KEY_FILE=a.model_key_file, OO_EVAL_MODEL_ID=a.model_id)
        argv = ["cargo", "test", "-p", "oo-docs", "--locked", "--test", "eval_live", "--", "--ignored", "--nocapture", "--test-threads=1"]
        p = subprocess.run(argv, cwd=REPO, env=env, capture_output=True, text=True, timeout=7200)
        (out / "cargo.log").write_text(p.stdout[-6000:] + p.stderr[-6000:])
        if p.returncode != 0:
            print((p.stdout + p.stderr)[-2000:])
            raise SystemExit(f"eval_live failed ({p.returncode})")
    finally:
        pg.stop()
    subprocess.run([sys.executable, "-B", str(HERE / "score_quality.py"), "--corpus", a.corpus, "--run", str(out)], check=True)


if __name__ == "__main__":
    main()
