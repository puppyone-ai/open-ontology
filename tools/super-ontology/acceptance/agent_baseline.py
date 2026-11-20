#!/usr/bin/env python3
"""CLI-agent baseline for the acceptance questions: the agent loop of tools/oo-test/super_ontology_agent_qa.py (one tool, `oo`, read-only, token file)
asks every question of gold/questions.jsonl and writes ask-results.json in the shape score_ask.py reads, so the in-app Ask and a plain CLI agent are
scored by the same rules.

    python3 -B agent_baseline.py --oo <oo binary> --endpoint <engine url> --token-file <read token> --out <dir> [--workers 3]
    python3 -B score_ask.py --ask-results <dir>/ask-results.json --questions gold/questions.jsonl

The model endpoint and key are read from the files named by path (defaults: ~/.config/open-ontology/secrets/model-endpoint-9r, model-key-9r-sol); neither is printed.
"""
import argparse, concurrent.futures, json, sys, time
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent.parent
sys.path.insert(0, str(REPO / "tools/oo-test"))
import super_ontology_agent_qa as qa  # noqa: E402

GOLD = REPO / "testing/super-ontology/acceptance/gold/questions.jsonl"


def source_paths(text):
    """File paths in the SOURCES line, as the ontology surface prints them (project/<folder>/<file>)."""
    import re
    return sorted(set(re.findall(r"[\w./-]+\.(?:pdf|xlsx|csv|docx|pptx|wav|m4a|png|md|txt)", text)))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--oo", required=True)
    ap.add_argument("--endpoint", required=True)
    ap.add_argument("--token-file", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--questions", default=str(GOLD))
    ap.add_argument("--model", default="gcli/grok-4.7")
    ap.add_argument("--model-endpoint-file", default=str(qa.SECRETS / "model-endpoint-9r"))
    ap.add_argument("--model-key-file", default=str(qa.SECRETS / "model-key-9r-sol"))
    ap.add_argument("--max-calls", type=int, default=12)
    ap.add_argument("--workers", type=int, default=3)
    ap.add_argument("--agent-home", default="/private/tmp/so-AB/agent-home")
    ap.add_argument("--only", default="")
    a = ap.parse_args()
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    model = qa.Model(a.model_endpoint_file, a.model_key_file, a.model)
    env = qa.agent_env(a)
    system = qa.SYSTEM_PROMPT.replace("{max_calls}", str(a.max_calls))
    qs = [json.loads(l) for l in Path(a.questions).read_text().splitlines() if l.strip()]
    if a.only:
        qs = [q for q in qs if q["id"] in a.only.split(",")]

    def one(q):
        t = time.time()
        try:
            rec = qa.ask(model, a.oo, env, q["question"], a.max_calls, system)
        except Exception as e:  # a model outage is a logged failure, never a silent skip
            rec = {"calls": [], "final": "", "error": str(e)[:200]}
        answer, sources = qa.split_answer(rec.get("final", ""))
        declined = answer.strip().upper().startswith("NOT_IN_DATA") or (not answer and rec.get("error"))
        row = {"id": q["id"], "ms": int((time.time() - t) * 1000), "ok": not rec.get("error"), "status": "unanswerable" if declined else "answered",
               "answer": "" if declined else answer, "reason": answer if declined else "", "steps": len(rec.get("calls", [])),
               "sources": [] if declined else [{"kind": "file", "path": p} for p in source_paths(sources)], "error": rec.get("error")}
        # A declined answer cites nothing: the SOURCES line of a NOT_IN_DATA reply lists what was looked at, not what supports an answer.
        print(f"{q['id']} {row['status']} calls={row['steps']}", flush=True)
        return row

    with concurrent.futures.ThreadPoolExecutor(a.workers) as ex:
        rows = list(ex.map(one, qs))
    (out / "ask-results.json").write_text(json.dumps(sorted(rows, key=lambda r: r["id"]), ensure_ascii=False, indent=1))
    print(f"wrote {out / 'ask-results.json'}")


if __name__ == "__main__":
    main()
