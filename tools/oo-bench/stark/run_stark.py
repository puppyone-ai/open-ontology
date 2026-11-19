#!/usr/bin/env python3
"""STaRK retrieval with the OO engine (`crates/oo-docs/examples/stark_kb.rs`).

Pipeline per question:
  1. optional LLM query plan (answer types, anchor mentions with their
     relations, residual text constraints), cached in `--plan-cache` so a
     rerun never calls the model again;
  2. OO search: anchor resolution through the alias table, one-hop walk over
     the typed KB edges, intersection, BM25 over the node text (IR blocks);
  3. optional LLM rerank of the top `--rerank-k` candidates, cached too.

Writes `predictions.csv` in the leaderboard format (`query_id,pred_rank`,
top 100 node ids) and `metrics.json` (Hit@1, Hit@5, Recall@20, MRR as the
official `stark_qa.Evaluator` computes them over the top 100; ties beyond
the list are ignored) when gold answers are available for the split.

Model endpoints and keys are read only from the files named on the command
line; they are never printed or written.
"""
import argparse
import concurrent.futures as cf
import csv
import hashlib
import json
import os
import random
import re
import subprocess
import sys
import threading
import time
import urllib.request
from pathlib import Path



def load_split(qa_dir, split):
    human = split == "human_generated_eval"
    name = "stark_qa_human_generated_eval.csv" if human else "stark_qa.csv"
    rows = {}
    with open(Path(qa_dir) / "stark_qa" / name, newline="") as f:
        for r in csv.DictReader(f):
            rows[int(r["id"])] = (r["query"], json.loads(r["answer_ids"]))
    if human:
        ids = sorted(rows)
    else:
        ids = [int(x) for x in (Path(qa_dir) / "split" / f"{split}.index").read_text().split()]
    return [(i, rows[i][0], rows[i][1]) for i in ids]


def metrics(pred, gold):
    pred = pred[:100]
    gold = set(gold)
    first = next((r for r, p in enumerate(pred, 1) if p in gold), None)
    return {
        "hit@1": float(bool(pred[:1]) and pred[0] in gold),
        "hit@5": float(any(p in gold for p in pred[:5])),
        "recall@20": len(gold & set(pred[:20])) / max(len(gold), 1),
        "mrr": 1.0 / first if first else 0.0,
    }


class Engine:
    def __init__(self, binary, nodes, edges, extra):
        self.p = subprocess.Popen(
            [binary, "--nodes", nodes, "--edges", edges, *extra],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1,
        )
        self.lock = threading.Lock()

    def call(self, obj):
        with self.lock:
            self.p.stdin.write(json.dumps(obj) + "\n")
            self.p.stdin.flush()
            line = self.p.stdout.readline()
        r = json.loads(line)
        if not r.get("ok"):
            raise RuntimeError(r.get("error"))
        return r

    def close(self):
        self.p.stdin.close()
        self.p.wait(timeout=60)


class Llm:
    def __init__(self, model, endpoint_file, key_file, cache_path):
        self.model = model
        self.base = Path(endpoint_file).expanduser().read_text().strip().rstrip("/")
        self.key = Path(key_file).expanduser().read_text().strip()
        self.cache_path = Path(cache_path)
        self.cache = {}
        if self.cache_path.exists():
            for line in self.cache_path.open():
                r = json.loads(line)
                self.cache[r["k"]] = r["v"]
        self.lock = threading.Lock()
        self.calls = 0
        self.tokens = [0, 0]

    def chat(self, tag, prompt, max_tokens=800):
        k = hashlib.sha256(f"{self.model}\0{tag}\0{prompt}".encode()).hexdigest()
        if k in self.cache:
            return self.cache[k]
        body = {
            "model": self.model,
            "messages": [{"role": "user", "content": prompt}],
            "temperature": 0,
            "max_tokens": max_tokens,
        }
        err = None
        for attempt in range(6):
            try:
                req = urllib.request.Request(
                    self.base + "/chat/completions",
                    data=json.dumps(body).encode(),
                    headers={"Authorization": "Bearer " + self.key, "Content-Type": "application/json"},
                )
                d = json.load(urllib.request.urlopen(req, timeout=180))
                text = d["choices"][0]["message"]["content"] or ""
                u = d.get("usage") or {}
                with self.lock:
                    self.calls += 1
                    self.tokens[0] += u.get("prompt_tokens", 0)
                    self.tokens[1] += u.get("completion_tokens", 0)
                    self.cache[k] = text
                    with self.cache_path.open("a") as f:
                        f.write(json.dumps({"k": k, "v": text}) + "\n")
                return text
            except Exception as e:  # noqa: BLE001 - retried, then surfaced without the endpoint
                err = f"{type(e).__name__}: {str(e)[:120]}".replace(self.base, "<endpoint>")
                time.sleep(2 * (attempt + 1))
        raise RuntimeError(err)


def json_of(text):
    m = re.search(r"\{.*\}|\[.*\]", text, re.S)
    return json.loads(m.group(0)) if m else None


PLAN_PROMPT = """You turn a question about a {kb_desc} knowledge base into a retrieval plan. Do not answer it.
Node types: {types}
Relations that exist (relation: type - type):
{schema}
Return JSON only:
{{"answer_types": [node types the answer can be, most likely first],
 "anchors": [{{"mention": exact name of an entity named in the question (gene symbol, drug, disease, pathway...), "types": [its node type], "relations": [relations that link the answer to this entity, from the list above], "via": [only for an indirect link: the node type in between, e.g. a drug that shares a target gene with X has via ["gene/protein"]]}}],
 "text": "the remaining descriptive constraints on the answer, in a few keywords"}}
Only list anchors that are named entities explicitly mentioned in the question; never invent one. Use [] when there is none. Generic words (symptom names used as descriptions are fine as anchors, but not words like "disease" or "gene").
Question: {q}"""

RERANK_PROMPT = """Question: {q}

Candidates from a {kb_desc} knowledge base:
{cands}

Rank the candidates by how well each one satisfies every constraint of the question (relations to named entities and descriptive details). Return JSON only: {{"ranking": [candidate numbers, best first]}} listing every candidate number exactly once."""


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--kb", default="prime")
    ap.add_argument("--binary", required=True)
    ap.add_argument("--nodes", required=True)
    ap.add_argument("--edges", required=True)
    ap.add_argument("--qa-dir", required=True)
    ap.add_argument("--split", default="val")
    ap.add_argument("--limit", type=int, default=0, help="first N questions after a seeded shuffle (dev subset)")
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--out", required=True)
    ap.add_argument("--plan", choices=["none", "llm"], default="none")
    ap.add_argument("--rerank-k", type=int, default=0)
    ap.add_argument("--rerank-rels", type=int, default=6)
    ap.add_argument("--rerank-chars", type=int, default=1200)
    ap.add_argument("--model", default="cb/gpt-5.4-mini")
    ap.add_argument("--endpoint-file", default="~/.config/open-ontology/secrets/model-endpoint-9r")
    ap.add_argument("--key-file", default="~/.config/open-ontology/secrets/model-key-9r-sol")
    ap.add_argument("--cache", default=str(Path("~/.cache/oo-bench/stark-llm.jsonl").expanduser()))
    ap.add_argument("--workers", type=int, default=8)
    ap.add_argument("--fuzzy", type=int, default=2)
    ap.add_argument("--candidate-type", action="append", default=[],
                    help="node types the benchmark ranks (MAG: paper, Amazon: product); default all")
    ap.add_argument("--engine-arg", action="append", default=[])
    a = ap.parse_args()

    kb_desc = {"prime": "biomedical (PrimeKG)", "mag": "academic papers (MAG)", "amazon": "product (Amazon)"}[a.kb]
    qs = load_split(a.qa_dir, a.split)
    if a.limit:
        random.Random(a.seed).shuffle(qs)
        qs = sorted(qs[: a.limit])
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    t0 = time.time()
    eng = Engine(a.binary, a.nodes, a.edges, a.engine_arg)
    stats = eng.call({"op": "stats"})
    print(f"engine: {stats['nodes']} nodes, {stats['blocks']} IR blocks, {stats['facts']} rule facts, load {time.time()-t0:.0f}s", file=sys.stderr)
    schema = "\n".join(f"- {r}: {x} - {y}" for r, x, y, n in stats["schema"] if n >= 50)
    llm = None
    if a.plan == "llm" or a.rerank_k:
        Path(a.cache).parent.mkdir(parents=True, exist_ok=True)
        llm = Llm(a.model, a.endpoint_file, a.key_file, a.cache)

    def one(item):
        qid, q, gold = item
        t = time.time()
        plan = None
        if a.plan == "llm":
            try:
                plan = json_of(llm.chat("plan/v2", PLAN_PROMPT.format(kb_desc=kb_desc, types=", ".join(stats["types"]), schema=schema, q=q)))
            except Exception as e:  # noqa: BLE001
                print(f"plan failed q{qid}: {e}", file=sys.stderr)
        req = {"op": "search", "text": q, "plan": plan, "k": 100, "fuzzy": a.fuzzy}
        if a.candidate_type:
            req["candidate_types"] = a.candidate_type
        r = eng.call(req)
        ids = r["ids"]
        if a.rerank_k and len(ids) > 1:
            top = ids[: a.rerank_k]
            docs = eng.call({"op": "docs", "ids": top, "max_chars": a.rerank_chars, "rels": a.rerank_rels})["docs"]
            cands = "\n\n".join(f"[{i+1}] {d['doc']}" for i, d in enumerate(docs))
            try:
                raw = llm.chat("rerank/v1", RERANK_PROMPT.format(q=q, kb_desc=kb_desc, cands=cands), max_tokens=400)
                # Read the numbers after "ranking" even when the JSON is
                # malformed (a missing bracket is the common failure).
                tail = raw.split('"ranking"', 1)[-1]
                order = [int(x) - 1 for x in re.findall(r"\d+", tail)]
                seen, new = set(), []
                for o in order:
                    if 0 <= o < len(top) and o not in seen:
                        seen.add(o)
                        new.append(top[o])
                new += [x for i, x in enumerate(top) if i not in seen]
                ids = new + ids[a.rerank_k:]
            except Exception as e:  # noqa: BLE001
                print(f"rerank failed q{qid}: {e}", file=sys.stderr)
        return qid, q, gold, ids, r, plan, time.time() - t

    results = []
    with cf.ThreadPoolExecutor(max_workers=a.workers if llm else 1) as ex:
        for res in ex.map(one, qs):
            results.append(res)
    eng.close()
    results.sort(key=lambda x: x[0])
    with open(out / "predictions.csv", "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["query_id", "pred_rank"])
        for qid, _, _, ids, *_ in results:
            w.writerow([qid, json.dumps(ids[:100])])
    with open(out / "trace.jsonl", "w") as f:
        for qid, q, gold, ids, r, plan, dt in results:
            f.write(json.dumps({"id": qid, "q": q, "gold": gold, "top": ids[:10], "anchors": r["anchors"], "types": r["types"], "plan": plan, "s": round(dt, 3)}) + "\n")
    summary = {"kb": a.kb, "split": a.split, "n": len(results), "limit": a.limit, "seed": a.seed,
               "plan": a.plan, "rerank_k": a.rerank_k, "model": a.model if llm else None,
               "fuzzy": a.fuzzy, "engine_args": a.engine_arg, "candidate_types": a.candidate_type, "wall_s": round(time.time() - t0, 1)}
    if llm:
        summary["llm_calls"] = llm.calls
        summary["llm_tokens_in_out"] = llm.tokens
    lat = sorted(x[6] for x in results)
    if lat:
        summary["latency_p50_s"] = round(lat[len(lat) // 2], 3)
        summary["latency_p95_s"] = round(lat[int(len(lat) * 0.95) - 1], 3)
    if all(x[2] is not None for x in results) and results:
        m = [metrics(ids, gold) for _, _, gold, ids, *_ in results]
        for key in ("hit@1", "hit@5", "recall@20", "mrr"):
            summary[key] = round(100 * sum(x[key] for x in m) / len(m), 2)
    (out / "metrics.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps(summary))


if __name__ == "__main__":
    main()
