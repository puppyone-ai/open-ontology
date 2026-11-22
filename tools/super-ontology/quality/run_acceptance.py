#!/usr/bin/env python3
"""Run the acceptance folder through the real engine (no app, no GUI) and score the cross-file relations with the lane AB scorer.

  run_acceptance.py --project DIR --out DIR [--label NAME] [--model-endpoint-file F --model-key-file F --model-id ID] [--repo DIR]

Starts one labelled Postgres container and the `eval_live` test of REPO with OO_EVAL_HOLD (the engine stays up after the folder is
settled), reads the ontology over HTTP with tools/super-ontology/acceptance/score.py, then releases the engine. Scorers and gold are
only read. Writes <out>/<label>/{relations-score.json,entities.json,review-items.jsonl,entities-all.jsonl,files.json}.
Model files are passed as paths only.
"""
import argparse
import json
import os
import shutil
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent.parent / "oo-test"))
sys.dont_write_bytecode = True
import super_ontology_l2 as l2  # noqa: E402

l2.LANE = "oo.lane=" + os.environ.get("OO_TEST_LANE", "super-ontology-aj")


def get(base, token, path):
    req = urllib.request.Request(base + path, headers={"Authorization": f"Bearer {token}", "OO-Docs-Api-Min": "1.0", "Accept": "application/json"})
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.loads(r.read())


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--project", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--label", default="run")
    ap.add_argument("--repo", default=str(l2.REPO))
    ap.add_argument("--model-endpoint-file")
    ap.add_argument("--model-key-file")
    ap.add_argument("--model-id")
    ap.add_argument("--workers", default="4")
    a = ap.parse_args()
    repo = Path(a.repo)
    out = Path(a.out) / a.label
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)
    corpus = Path(a.out) / "acc-corpus"
    shutil.rmtree(corpus, ignore_errors=True)
    shutil.copytree(a.project, corpus / "docs")
    run_id = time.strftime("%Y%m%d-%H%M%S")
    ev = l2.Evidence(Path(a.out) / "evidence", run_id)
    pg = l2.Postgres(ev, run_id)
    flag = out / "release"
    proc = None
    try:
        pg.start()
        env = dict(os.environ, OO_DOCS_TEST_PG_PORT=str(pg.port), OO_EVAL_OUT=str(out), OO_EVAL_GOLDEN=str(corpus), OO_EVAL_HOLD=str(flag),
                   CARGO_BUILD_JOBS=os.environ.get("CARGO_BUILD_JOBS", "6"))
        if a.model_id:
            env.update(OO_EVAL_MODEL="1", OO_EVAL_LLM_WORKERS=a.workers, OO_EVAL_ENDPOINT_FILE=a.model_endpoint_file,
                       OO_EVAL_KEY_FILE=a.model_key_file, OO_EVAL_MODEL_ID=a.model_id)
        argv = ["cargo", "test", "-p", "oo-docs", "--locked", "--test", "eval_live", "--", "--ignored", "--nocapture", "--test-threads=1"]
        log = open(out / "cargo.log", "w")
        proc = subprocess.Popen(argv, cwd=repo, env=env, stdout=log, stderr=subprocess.STDOUT)
        deadline = time.time() + 5400
        while not (out / "engine.json").exists():
            if proc.poll() is not None or time.time() > deadline:
                raise SystemExit("engine did not come up: see " + str(out / "cargo.log"))
            time.sleep(2)
        time.sleep(1)
        addr = json.loads((out / "engine.json").read_text())["addr"]
        base = f"http://{addr}"
        token = (out / "token").read_text().strip()
        files, cursor = [], None
        while True:
            page = get(base, token, "/v1/docs/files?limit=200&recursive=true" + (f"&cursor={cursor}" if cursor else ""))
            files += page.get("items", [])
            cursor = page.get("next_cursor")
            if not cursor:
                break
        run = {"files": {f"project/{f['path']}": {"state": f.get("state"), "detail": {"format": f.get("format"), "state_code": f.get("state_code")}, "timeline": []} for f in files}}
        (out / "files.json").write_text(json.dumps(run, ensure_ascii=False, indent=1))
        r = subprocess.run([sys.executable, "-B", str(repo / "tools/super-ontology/acceptance/score.py"), "--endpoint", base, "--token-file", str(out / "token"),
                            "--run", str(out / "files.json"), "--gold", str(repo / "testing/super-ontology/acceptance/gold"), "--out", str(out)],
                           capture_output=True, text=True)
        (out / "score.log").write_text(r.stdout + r.stderr)
        print(r.stdout[-3000:] or r.stderr[-1500:])
    finally:
        flag.write_text("release")
        if proc:
            try:
                proc.wait(timeout=120)
            except subprocess.TimeoutExpired:
                proc.kill()
        pg.stop()
        for f in ("token",):
            (out / f).unlink(missing_ok=True)


if __name__ == "__main__":
    main()
