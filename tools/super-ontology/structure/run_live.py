#!/usr/bin/env python3
"""Runs the structure-layer test through the real pipeline and scores it (D93).

    OO_TEST_LANE=super-ontology-am python3 -B tools/super-ontology/structure/run_live.py --out DIR
        [--project DIR] [--acceptance] [--model --endpoint-file F --key-file F --model-id ID]

A labelled Postgres container, the real engine and Docling sidecar (the `structure_live` test). On the structure corpus the result is scored by
`score.py` against the frozen gold, before a person confirms (induced) and after (confirmed, read by the extraction). With --acceptance the project
is the acceptance folder and its cross-file relations are scored by `acceptance/score.py` before and after (the same harness). The endpoint and key
are only ever passed as paths.
"""
import argparse, json, os, subprocess, sys, time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parents[1] / "oo-test"))
sys.dont_write_bytecode = True
import super_ontology_l2 as l2  # noqa: E402

REPO = l2.REPO
l2.LANE = "oo.lane=" + os.environ.get("OO_TEST_LANE", "super-ontology-am")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    ap.add_argument("--project")
    ap.add_argument("--acceptance", action="store_true")
    ap.add_argument("--shop", action="store_true", help="the acceptance shop database as the Agent DB: one merged ontology YAML")
    ap.add_argument("--model", action="store_true")
    ap.add_argument("--ask", action="store_true", help="with --model: answer the frozen questions from the BRIEF before and after (secondary)")
    ap.add_argument("--endpoint-file")
    ap.add_argument("--key-file")
    ap.add_argument("--model-id")
    a = ap.parse_args()
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    ev = l2.Evidence(out / "log", time.strftime("%Y%m%d-%H%M%S"))
    pg = l2.Postgres(ev, "am-" + time.strftime("%H%M%S"))
    ok = True
    try:
        pg.start()
        env = dict(os.environ, OO_DOCS_TEST_PG_PORT=str(pg.port), OO_STRUCTURE_OUT=str(out), CARGO_BUILD_JOBS=os.environ.get("CARGO_BUILD_JOBS", "6"), CARGO_INCREMENTAL="0")
        if a.acceptance:
            env["OO_STRUCTURE_PROJECT"] = str(REPO / "testing/super-ontology/acceptance/project")
            env["OO_STRUCTURE_ACCEPTANCE"] = str(REPO / "tools/super-ontology/acceptance/score.py")
        elif a.project:
            env["OO_STRUCTURE_PROJECT"] = a.project
        if a.shop:
            env["OO_STRUCTURE_SHOP"] = str(REPO / "testing/super-ontology/agent-qa")
        if a.model and a.ask:
            env["OO_STRUCTURE_ASK"] = "1"
        if a.model:
            if not (a.endpoint_file and a.key_file and a.model_id):
                raise SystemExit("--endpoint-file, --key-file and --model-id are required with --model")
            env.update(OO_STRUCTURE_MODEL="1", OO_EVAL_ENDPOINT_FILE=a.endpoint_file, OO_EVAL_KEY_FILE=a.key_file, OO_EVAL_MODEL_ID=a.model_id, OO_DOCS_MODEL_TIMEOUT_SECS="180")
        argv = ["cargo", "test", "-p", "oo-docs", "--locked", "--test", "structure_live", "--", "--ignored", "--nocapture", "--test-threads=1"]
        ev.log("$ " + " ".join(argv))
        p = subprocess.run(argv, cwd=REPO, env=env, capture_output=True, text=True, timeout=7200)
        (out / "cargo.log").write_text(p.stdout + p.stderr)
        ev.check("the live run finished", p.returncode == 0, f"exit {p.returncode}")
        if p.returncode != 0:
            ev.log((p.stdout + p.stderr)[-3000:])
            ok = False
        elif not a.acceptance and not a.project:
            for tag in ("before", "after"):
                d = out / tag
                d.mkdir(exist_ok=True)
                (d / "structure.json").write_text((out / f"structure-{tag}.json").read_text())
                q = subprocess.run([sys.executable, "-B", str(HERE / "score.py"), "--run", str(d), "--out", str(out / f"score-{tag}.json")], capture_output=True, text=True)
                ev.log(f"== {tag} ==\n" + q.stdout[-1500:])
                if a.model and a.ask:
                    r = subprocess.run([sys.executable, "-B", str(REPO / "tools/super-ontology/acceptance/score_ask.py"), "--ask-results", str(out / f"ask-{tag}" / "ask-results.json"),
                                        "--questions", str(REPO / "testing/super-ontology/structure/gold/questions.jsonl"), "--out", str(out / f"score-ask-{tag}.json")], capture_output=True, text=True)
                    ev.log(f"== ask {tag} ==\n" + r.stdout[-800:])
        # D93 (lane AT): the cells the confirmed table classes filled, scored against the frozen cell gold of the corpus.
        project = Path(a.project) if a.project else REPO / "testing/super-ontology/structure/project"
        gold = project.parent / "gold" / "cells.json"
        if not a.acceptance and ok and gold.exists() and (out / "instances-after.json").exists():
            q = subprocess.run([sys.executable, "-B", str(HERE / "score_cells.py"), "--instances", str(out / "instances-after.json"), "--gold", str(gold), "--out", str(out / "score-cells.json")],
                               capture_output=True, text=True)
            ev.log("== cells ==\n" + q.stdout[-3000:] + q.stderr[-500:])
    finally:
        pg.stop()
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
