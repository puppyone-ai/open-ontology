#!/usr/bin/env python3
"""Runs the profile test set through the real pipeline and scores it (D89).

    OO_TEST_LANE=super-ontology-ak python3 -B tools/super-ontology/profiles/run_live.py --out DIR [--model --endpoint-file F --key-file F --model-id ID] [--ask]

A labelled Postgres container, the real engine and Docling sidecar (the `profiles_live` test), then `score.py` against the frozen gold and, with --ask, the
app's Ask loop on the frozen questions (needs --model) scored by `acceptance/score_ask.py`. The endpoint and key are only ever passed as paths.
"""
import argparse, json, os, subprocess, sys, time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parents[1] / "oo-test"))
sys.dont_write_bytecode = True
import super_ontology_l2 as l2  # noqa: E402

REPO = l2.REPO
l2.LANE = "oo.lane=" + os.environ.get("OO_TEST_LANE", "super-ontology-ak")
GOLD = REPO / "testing/super-ontology/profiles/gold"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    ap.add_argument("--model", action="store_true")
    ap.add_argument("--endpoint-file")
    ap.add_argument("--key-file")
    ap.add_argument("--model-id")
    ap.add_argument("--ask", action="store_true")
    a = ap.parse_args()
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    ev = l2.Evidence(out / "log", time.strftime("%Y%m%d-%H%M%S"))
    pg = l2.Postgres(ev, "ak-" + time.strftime("%H%M%S"))
    ok = True
    try:
        pg.start()
        env = dict(os.environ, OO_DOCS_TEST_PG_PORT=str(pg.port), OO_PROFILES_OUT=str(out), CARGO_BUILD_JOBS=os.environ.get("CARGO_BUILD_JOBS", "6"))
        if a.model:
            if not (a.endpoint_file and a.key_file and a.model_id):
                raise SystemExit("--endpoint-file, --key-file and --model-id are required with --model")
            env.update(OO_PROFILES_MODEL="1", OO_EVAL_ENDPOINT_FILE=a.endpoint_file, OO_EVAL_KEY_FILE=a.key_file, OO_EVAL_MODEL_ID=a.model_id,
                       OO_DOCS_MODEL_TIMEOUT_SECS="180")
        if a.ask:
            env["OO_PROFILES_ASK"] = str(out)
        argv = ["cargo", "test", "-p", "oo-docs", "--locked", "--test", "profiles_live", "--", "--ignored", "--nocapture", "--test-threads=1"]
        ev.log("$ " + " ".join(argv))
        p = subprocess.run(argv, cwd=REPO, env=env, capture_output=True, text=True, timeout=7200)
        (out / "cargo.log").write_text(p.stdout + p.stderr)
        ev.check("the live run finished", p.returncode == 0, f"exit {p.returncode}")
        if p.returncode != 0:
            ev.log((p.stdout + p.stderr)[-2500:])
            ok = False
        else:
            q = subprocess.run([sys.executable, "-B", str(HERE / "score.py"), "--run", str(out), "--gold", str(GOLD), "--out", str(out / "score.json")],
                               capture_output=True, text=True)
            ev.log(q.stdout)
            if a.ask:
                r = subprocess.run([sys.executable, "-B", str(REPO / "tools/super-ontology/acceptance/score_ask.py"), "--ask-results", str(out / "ask-results.json"),
                                    "--questions", str(GOLD / "questions.jsonl"), "--out", str(out / "score-ask.json")], capture_output=True, text=True)
                ev.log(r.stdout)
    finally:
        pg.stop()
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
