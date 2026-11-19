#!/usr/bin/env python3
"""GraphRAG-Bench (Xiang et al., Novel / Medical) with the OO engine
(`crates/oo-docs/examples/grb_corpus.rs`).

  prepare  corpus JSON -> docs.jsonl (markdown, one paragraph per ~80 words)
  subset   write the fixed dev subset (seeded, 10 % of each question type)
  run      retrieve with OO, answer with the reader model, write the file the
           official `Evaluation/generation_eval.py` reads (id, question,
           question_type, ground_truth, generated_answer, context)
  score    dev diagnostic only: statement-level factual F1 with the official
           prompts (read from the official source as data, never executed)
           and ROUGE-L without stemming. Not the board metric: the board uses
           0.75 x F1 + 0.25 x bge-large cosine, judged by GPT-4o-mini.

Model endpoints and keys are read only from the files named on the command
line; they are never printed or written.
"""
import argparse
import ast
import concurrent.futures as cf
import hashlib
import json
import random
import re
import subprocess
import sys
import threading
import time
import urllib.request
from pathlib import Path

TYPES = ["Fact Retrieval", "Complex Reasoning", "Contextual Summarize", "Creative Generation"]


def corpus_docs(path):
    data = json.load(open(path))
    items = data if isinstance(data, list) else [data]
    for it in items:
        yield it["corpus_name"], it["context"]


def to_markdown(name, text, words=80):
    sents = re.split(r"(?<=[.!?])\s+", " ".join(text.split()))
    paras, cur, n = [], [], 0
    for s in sents:
        cur.append(s)
        n += len(s.split())
        if n >= words:
            paras.append(" ".join(cur))
            cur, n = [], 0
    if cur:
        paras.append(" ".join(cur))
    # Escape characters markdown would read as structure at line start.
    paras = [re.sub(r"^([#>*+\-]|\d+\.)", r"\\\1", p) for p in paras]
    return f"# {name}\n\n" + "\n\n".join(paras) + "\n"


class Llm:
    def __init__(self, model, endpoint_file, key_file, cache_path, temperature):
        self.model = model
        self.temperature = temperature
        self.base = Path(endpoint_file).expanduser().read_text().strip().rstrip("/")
        self.key = Path(key_file).expanduser().read_text().strip()
        self.cache_path = Path(cache_path)
        self.cache_path.parent.mkdir(parents=True, exist_ok=True)
        self.cache = {}
        if self.cache_path.exists():
            for line in self.cache_path.open():
                r = json.loads(line)
                self.cache[r["k"]] = r["v"]
        self.lock = threading.Lock()
        self.calls = 0
        self.tokens = [0, 0]

    def chat(self, tag, prompt, max_tokens=1200):
        k = hashlib.sha256(f"{self.model}\0{self.temperature}\0{tag}\0{prompt}".encode()).hexdigest()
        if k in self.cache:
            return self.cache[k]
        body = {"model": self.model, "messages": [{"role": "user", "content": prompt}],
                "temperature": self.temperature, "max_tokens": max_tokens}
        err = None
        for attempt in range(6):
            try:
                req = urllib.request.Request(
                    self.base + "/chat/completions", data=json.dumps(body).encode(),
                    headers={"Authorization": "Bearer " + self.key, "Content-Type": "application/json"})
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


READER = """You are answering a question about a document collection. Use only the context passages below; each starts with its source tag.

{context}

Question: {question}

Answer directly. For a factual question give the fact in one or two sentences; for a summary or a creative task write a complete answer of a few sentences grounded in the context."""


def cmd_prepare(a):
    out = Path(a.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    n = 0
    with open(out, "w") as w:
        for name, text in corpus_docs(a.corpus):
            w.write(json.dumps({"path": f"{a.domain}/{name}.md", "text": to_markdown(name, text)}) + "\n")
            n += 1
    print(json.dumps({"docs": n}), file=sys.stderr)


def cmd_subset(a):
    res = {"seed": a.seed, "fraction": a.fraction, "note": "dev subset; tuning looks only at these ids"}
    for domain, path in (("novel", a.novel), ("medical", a.medical)):
        qs = json.load(open(path))
        rng = random.Random(f"{a.seed}/{domain}")
        ids = []
        for t in TYPES:
            group = sorted(q["id"] for q in qs if q["question_type"] == t)
            ids += sorted(rng.sample(group, max(1, round(len(group) * a.fraction))))
        res[domain] = ids
    Path(a.out).write_text(json.dumps(res, indent=1) + "\n")
    print({k: len(v) for k, v in res.items() if isinstance(v, list)})


def cmd_run(a):
    qs = json.load(open(a.questions))
    if a.subset:
        keep = set(json.load(open(a.subset))[a.domain])
        qs = [q for q in qs if q["id"] in keep]
    if a.limit:
        qs = qs[: a.limit]
    eng = subprocess.Popen([a.binary, "--docs", a.docs, "--words", str(a.words)],
                           stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
    lock = threading.Lock()

    def call(obj):
        with lock:
            eng.stdin.write(json.dumps(obj) + "\n")
            eng.stdin.flush()
            return json.loads(eng.stdout.readline())

    stats = call({"op": "stats"})
    print(f"engine: {stats}", file=sys.stderr)
    llm = Llm(a.model, a.endpoint_file, a.key_file, a.cache, a.temperature)

    def one(q):
        t = time.time()
        r = call({"op": "query", "text": q["question"], "k": a.k, "spread": a.spread})
        ctx = [f"{p['tag']} {p['text']}" for p in r["passages"]]
        t_ret = time.time() - t
        ans = llm.chat("reader/v1", READER.format(context="\n\n".join(ctx), question=q["question"]))
        return {"id": q["id"], "question": q["question"], "source": q["source"], "question_type": q["question_type"],
                "context": ctx, "evidence": q.get("evidence"), "generated_answer": ans.strip(),
                "ground_truth": q["answer"], "retrieval_s": round(t_ret, 4), "anchors": r.get("anchors")}

    t0 = time.time()
    with cf.ThreadPoolExecutor(max_workers=a.workers) as ex:
        rows = list(ex.map(one, qs))
    eng.stdin.close()
    eng.wait(timeout=60)
    out = Path(a.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(rows, indent=1, ensure_ascii=False) + "\n")
    lat = sorted(r["retrieval_s"] for r in rows)
    meta = {"domain": a.domain, "n": len(rows), "reader": a.model, "temperature": a.temperature, "k": a.k,
            "spread": a.spread, "words": a.words, "engine": stats, "llm_calls": llm.calls,
            "llm_tokens_in_out": llm.tokens, "wall_s": round(time.time() - t0, 1),
            "retrieval_p50_s": lat[len(lat) // 2] if lat else None,
            "context_words_mean": round(sum(len(" ".join(r["context"]).split()) for r in rows) / max(len(rows), 1))}
    out.with_suffix(".meta.json").write_text(json.dumps(meta, indent=2) + "\n")
    print(json.dumps(meta))


def official_prompts(src):
    """String constants and the examples list from the official
    `Evaluation/metrics/answer_accuracy.py`, read with `ast` (not imported)."""
    tree = ast.parse(Path(src).read_text())
    out = {}
    for node in tree.body:
        if isinstance(node, ast.Assign) and len(node.targets) == 1 and isinstance(node.targets[0], ast.Name):
            name = node.targets[0].id
            if name in ("STATEMENT_GENERATOR_PROMPT", "CORRECTNESS_PROMPT_TEMPLATE", "CORRECTNESS_EXAMPLES"):
                out[name] = ast.literal_eval(node.value)
    return out


def lcs(a, b):
    prev = [0] * (len(b) + 1)
    for x in a:
        cur = [0]
        for j, y in enumerate(b):
            cur.append(prev[j] + 1 if x == y else max(prev[j + 1], cur[j]))
        prev = cur
    return prev[-1]


def rouge_l(pred, ref):
    tok = lambda s: re.sub(r"[^a-z0-9]+", " ", s.lower()).split()  # noqa: E731
    p, r = tok(pred), tok(ref)
    if not p or not r:
        return 0.0
    m = lcs(p, r)
    if m == 0:
        return 0.0
    prec, rec = m / len(p), m / len(r)
    return 2 * prec * rec / (prec + rec)


def json_any(text):
    m = re.search(r"\{.*\}|\[.*\]", text, re.S)
    return json.loads(m.group(0)) if m else None


def cmd_score(a):
    P = official_prompts(a.official_metrics)
    rows = json.load(open(a.answers))
    llm = Llm(a.model, a.endpoint_file, a.key_file, a.cache, 0.0)
    examples = "\n".join(f"Input: {json.dumps(ex['input'])}\nOutput: {json.dumps(ex['output'])}"
                         for ex in P["CORRECTNESS_EXAMPLES"])

    def statements(q, text):
        try:
            v = json_any(llm.chat("grb-stmt", P["STATEMENT_GENERATOR_PROMPT"].format(question=q, answer=text)))
        except Exception:  # noqa: BLE001
            v = None
        if isinstance(v, dict):
            v = next((x for x in v.values() if isinstance(x, list)), list(v.values()))
        return [str(x) for x in v] if isinstance(v, list) else [text]

    def one(r):
        a_st = statements(r["question"], r["generated_answer"])
        g_st = statements(r["question"], r["ground_truth"])
        prompt = P["CORRECTNESS_PROMPT_TEMPLATE"].format(examples=examples, question=r["question"],
                                                        answer=json.dumps(a_st), ground_truth=json.dumps(g_st))
        try:
            c = json_any(llm.chat("grb-correct", prompt, max_tokens=2000)) or {}
            tp, fp, fn = (len(c.get(k, [])) for k in ("TP", "FP", "FN"))
            f1 = 2 * tp / (2 * tp + fp + fn) if tp + fp + fn else 1.0
        except Exception:  # noqa: BLE001
            f1 = 0.0
        return r["question_type"], f1, rouge_l(r["generated_answer"], r["ground_truth"])

    with cf.ThreadPoolExecutor(max_workers=a.workers) as ex:
        res = list(ex.map(one, rows))
    by = {}
    for t, f1, rl in res:
        by.setdefault(t, []).append((f1, rl))
    summary = {"judge": a.model, "n": len(res), "metric": "factual F1 (official prompts) and ROUGE-L (no stemmer); dev diagnostic, not the board ACC"}
    for t in TYPES:
        v = by.get(t, [])
        if v:
            summary[t] = {"n": len(v), "factual_f1": round(100 * sum(x[0] for x in v) / len(v), 2),
                          "rouge_l": round(100 * sum(x[1] for x in v) / len(v), 2)}
    f1s = [summary[t]["factual_f1"] for t in TYPES if t in summary]
    summary["mean_factual_f1_over_types"] = round(sum(f1s) / len(f1s), 2) if f1s else None
    Path(a.out).write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps(summary))


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("prepare")
    p.add_argument("--corpus", required=True)
    p.add_argument("--domain", required=True)
    p.add_argument("--out", required=True)
    p = sub.add_parser("subset")
    p.add_argument("--novel", required=True)
    p.add_argument("--medical", required=True)
    p.add_argument("--seed", type=int, default=0)
    p.add_argument("--fraction", type=float, default=0.1)
    p.add_argument("--out", required=True)
    for name in ("run", "score"):
        p = sub.add_parser(name)
        p.add_argument("--model", default="cx/gpt-5.6-luna")
        p.add_argument("--endpoint-file", default="~/.config/open-ontology/secrets/model-endpoint-9r")
        p.add_argument("--key-file", default="~/.config/open-ontology/secrets/model-key-9r-sol")
        p.add_argument("--cache", default=str(Path("~/.cache/oo-bench/grb-llm.jsonl").expanduser()))
        p.add_argument("--workers", type=int, default=6)
        p.add_argument("--out", required=True)
    p = sub.choices["run"]
    p.add_argument("--binary", required=True)
    p.add_argument("--docs", required=True)
    p.add_argument("--questions", required=True)
    p.add_argument("--domain", required=True)
    p.add_argument("--subset")
    p.add_argument("--limit", type=int, default=0)
    p.add_argument("--k", type=int, default=5)
    p.add_argument("--spread", type=int, default=1)
    p.add_argument("--words", type=int, default=200)
    p.add_argument("--temperature", type=float, default=0.7, help="the paper's generation temperature")
    p = sub.choices["score"]
    p.add_argument("--answers", required=True)
    p.add_argument("--official-metrics", required=True, help="path to the official Evaluation/metrics/answer_accuracy.py")
    a = ap.parse_args()
    {"prepare": cmd_prepare, "subset": cmd_subset, "run": cmd_run, "score": cmd_score}[a.cmd](a)


if __name__ == "__main__":
    main()
