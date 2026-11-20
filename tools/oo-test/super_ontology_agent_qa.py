#!/usr/bin/env python3
"""Agent QA over the `oo` CLI: a tool-calling LLM agent answers a fixed question set.

The agent has one tool, `oo`, which runs the `oo` CLI with an Agent token file (read-only subcommands only,
max N calls per question). The system prompt explains the tool and the answer format and holds no data.
Scoring is deterministic (see `score`). Every call and every final answer is logged.

The model endpoint and key are read from files by path (`--model-endpoint-file`, `--model-key-file`); neither is
ever printed or logged.

  super_ontology_agent_qa.py run    --oo BIN --endpoint URL --token-file F --out DIR [--split dev|held|all]
  super_ontology_agent_qa.py score  --out DIR [--split ...]        (re-score the logs in DIR)
  super_ontology_agent_qa.py safety --oo BIN --endpoint URL --state-dir D --out FILE   (token safety checks)
"""

import argparse
import concurrent.futures
import json
import os
import pwd
import re
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
QA = REPO / "testing/super-ontology/agent-qa"
CORPUS = REPO / "testing/super-ontology/golden/docs"
SECRETS = Path(pwd.getpwuid(os.getuid()).pw_dir) / ".config/open-ontology/secrets"  # the account home, not $HOME

# `oo <group> <sub>` pairs the agent may run: read-only, no flag that names a file, a server or a credential.
ALLOWED = {
    ("ontology", "search"), ("ontology", "entity"), ("ontology", "query"), ("ontology", "schema"),
    ("ontology", "sources"),
    ("help", None), ("agent-skill", None),
}
DOCS_SUB_OK = {}  # `oo docs ...` talks to the owner's local engine, not the Agent endpoint: not offered
DENY_FLAGS = ("--output", "--state-dir", "--endpoint", "--token-file", "--profile", "--out", "--to", "--config",
              "--deploy-dir", "--runtime", "--key", "--grant-file")
OUT_LIMIT = 12000

SYSTEM_PROMPT = """You answer questions about a user's documents and database. You have one tool, `oo`, which runs the `oo` command-line client with your read-only access already configured. Pass the arguments as a list, for example ["--help"] or ["ontology", "--help"]. Use `--help` to find out which commands exist. You can make at most {max_calls} tool calls per question.

Start with `oo ontology --help`. Prefer the plain output over `--json`: it is much shorter. Answer only from what the tool returns. Cite where the answer came from.

Finish with exactly these lines:
ANSWER: <the short answer, with the key names and numbers>
SOURCES: <the source references you used, as printed by the tool, such as a file path or a table name>

If the data does not contain the answer, or the question assumes something the data contradicts, write `ANSWER: NOT_IN_DATA` and one sentence why."""

MCP_SYSTEM = """You answer questions about a user's documents and database with the tools of an MCP server (read-only access is already configured). You can make at most {max_calls} tool calls per question.

Answer only from what the tools return. Cite where the answer came from.

Finish with exactly these lines:
ANSWER: <the short answer, with the key names and numbers>
SOURCES: <the source references you used, as printed by the tools, such as a file path or a table name>

If the data does not contain the answer, or the question assumes something the data contradicts, write `ANSWER: NOT_IN_DATA` and one sentence why."""

EN_NAMES = {"evan lin": "林远舟", "ruby zhou": "周若溪", "ethan chen": "陈子墨", "tina sun": "孙晓棠", "kevin wu": "吴启明",
            "yutong zheng": "郑雨桐", "leo han": "韩立", "jiayi xu": "许嘉怡", "ben he": "贺文博"}
DECLINE_RE = re.compile(r"NOT_IN_DATA|not in the data|no (such )?(data|information|record)|does not contain|"
                        r"cannot (be )?(found|determined|answer)|couldn't find|could not find|没有(相关)?(记录|信息|数据)|"
                        r"未(找到|记录|提供)|无法(回答|确定|找到)|不存在|没有找到", re.I)


def read_file(p):
    return Path(p).read_text().strip()


def load_questions(split="all"):
    qs = [json.loads(l) for l in (QA / "questions.jsonl").read_text().splitlines() if l.strip()]
    return [q for q in qs if split == "all" or q["split"] == split]


# ---------------------------------------------------------------- tool

def check_args(args):
    """None when the call is allowed, else the refusal text."""
    if not isinstance(args, list) or not all(isinstance(a, str) for a in args) or not args:
        return "args must be a non-empty list of strings"
    if args[0] in ("--help", "-h", "--version"):
        return None
    if any(a.split("=")[0] in DENY_FLAGS for a in args):
        return "that option is not available to this tool"
    g = args[0]
    sub = args[1] if len(args) > 1 and not args[1].startswith("-") else None
    if g == "help" or g == "agent-skill":
        return None
    if (g, sub) in ALLOWED:
        ok = DOCS_SUB_OK.get((g, sub))
        if ok is not None:
            third = args[2] if len(args) > 2 and not args[2].startswith("-") else None
            if third not in ok:
                return f"allowed: oo {g} {sub} {'|'.join(sorted(ok))}"
        return None
    if g == "ontology" and sub is None and any(a in ("--help", "-h") for a in args):
        return None
    return "only these commands are available here: oo ontology search|entity|query|schema|sources, oo --help"


def run_oo(oo, args, env, timeout=120):
    refused = check_args(args)
    if refused:
        return 126, f"refused: {refused}"
    p = subprocess.run([oo, *args], env=env, capture_output=True, text=True, timeout=timeout, stdin=subprocess.DEVNULL)
    out = (p.stdout + (("\n" + p.stderr) if p.stderr.strip() else "")).strip()
    if len(out) > OUT_LIMIT:
        out = out[:OUT_LIMIT] + f"\n[output cut at {OUT_LIMIT} characters; narrow the request]"
    return p.returncode, out


# ---------------------------------------------------------------- model

class Model:
    def __init__(self, endpoint_file, key_file, model):
        base = read_file(endpoint_file).rstrip("/")
        self.url = base if base.endswith("/chat/completions") else base + "/chat/completions"
        self.key = read_file(key_file)
        self.model = model

    def chat(self, messages, tools=None, tool_choice=None):
        body = {"model": self.model, "messages": messages, "temperature": 0}
        if tools:
            body["tools"] = tools
            if tool_choice:
                body["tool_choice"] = tool_choice
        data = json.dumps(body).encode()
        last = None
        for attempt in range(4):
            req = urllib.request.Request(self.url, data=data, headers={"Authorization": "Bearer " + self.key,
                                                                         "Content-Type": "application/json"})
            try:
                with urllib.request.urlopen(req, timeout=240) as r:
                    return json.loads(r.read())
            except urllib.error.HTTPError as e:
                last = f"http {e.code}"
                if e.code not in (429, 500, 502, 503, 504):
                    break
            except (urllib.error.URLError, TimeoutError, ConnectionError) as e:
                last = type(e).__name__
            time.sleep(2 ** attempt)
        raise RuntimeError("model call failed: " + str(last))


TOOL = [{"type": "function", "function": {
    "name": "oo", "description": "Run the oo command-line client (read-only). Example args: [\"ontology\", \"--help\"].",
    "parameters": {"type": "object", "properties": {"args": {"type": "array", "items": {"type": "string"},
                                                           "description": "command-line arguments after `oo`"}},
                   "required": ["args"]}}}]


class McpSession:
    """`oo mcp --stdio` spoken over JSON-RPC lines; its tools are offered to the model as they are."""

    def __init__(self, oo, env, endpoint, token_file):
        self.p = subprocess.Popen([oo, "mcp", "--stdio", "--endpoint", endpoint, "--token-file", token_file], env=env,
                                  stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
        self.n = 0
        self.instructions = self.rpc("initialize", {"protocolVersion": "2025-11-25", "capabilities": {},
                                                    "clientInfo": {"name": "agent-qa", "version": "0"}})["result"].get("instructions", "")
        self.p.stdin.write(json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n")
        self.p.stdin.flush()
        self.tools = self.rpc("tools/list", {})["result"]["tools"]

    def rpc(self, method, params):
        self.n += 1
        self.p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": self.n, "method": method, "params": params}) + "\n")
        self.p.stdin.flush()
        while True:
            line = self.p.stdout.readline()
            if not line:
                raise RuntimeError("mcp server closed")
            try:
                m = json.loads(line)
            except ValueError:
                continue
            if m.get("id") == self.n:
                return m

    def openai_tools(self):
        return [{"type": "function", "function": {"name": t["name"], "description": t.get("description", "")[:1000],
                                                  "parameters": t.get("inputSchema") or {"type": "object", "properties": {}}}}
                for t in self.tools]

    def call(self, name, args):
        r = self.rpc("tools/call", {"name": name, "arguments": args or {}})
        if "error" in r:
            return 1, json.dumps(r["error"])[:OUT_LIMIT]
        res = r["result"]
        text = "\n".join(c.get("text", "") for c in res.get("content", []) if c.get("type") == "text")
        if not text and "structuredContent" in res:
            text = json.dumps(res["structuredContent"], ensure_ascii=False)
        return (1 if res.get("isError") else 0), text[:OUT_LIMIT]

    def close(self):
        try:
            self.p.stdin.close()
            self.p.wait(timeout=5)
        except Exception:
            self.p.kill()


def ask(model, oo, env, question, max_calls, system, mcp=None):
    tools = mcp.openai_tools() if mcp else TOOL
    msgs = [{"role": "system", "content": system}, {"role": "user", "content": question}]
    calls, usage, t0, final = [], 0, time.time(), ""
    llm_calls = 0
    while True:
        forced = len(calls) >= max_calls
        r = model.chat(msgs, tools, "none" if forced else None)
        llm_calls += 1
        usage += (r.get("usage") or {}).get("total_tokens", 0)
        m = r["choices"][0]["message"]
        tcs = m.get("tool_calls") or []
        if forced and "ANSWER:" not in (m.get("content") or ""):
            msgs.append({"role": "user", "content": "Call limit reached. Give the final ANSWER and SOURCES lines now, from what the tool has returned so far."})
            r2 = model.chat(msgs, None)
            llm_calls += 1
            usage += (r2.get("usage") or {}).get("total_tokens", 0)
            final = r2["choices"][0]["message"].get("content") or ""
            break
        if not tcs or forced:
            final = m.get("content") or ""
            break
        msgs.append({"role": "assistant", "content": m.get("content"), "tool_calls": tcs})
        for tc in tcs:
            if len(calls) >= max_calls:
                msgs.append({"role": "tool", "tool_call_id": tc["id"], "content": "call limit reached"})
                continue
            ts = time.time()
            if mcp:
                try:
                    args = json.loads(tc["function"]["arguments"] or "{}")
                except ValueError:
                    args = None
                try:
                    rc, out = mcp.call(tc["function"]["name"], args) if args is not None else (126, "refused: arguments must be JSON")
                except Exception as e:
                    rc, out = 124, f"mcp error: {e}"
                args = [tc["function"]["name"], json.dumps(args, ensure_ascii=False)]
            else:
                try:
                    args = json.loads(tc["function"]["arguments"] or "{}").get("args")
                except ValueError:
                    args = None
                if args is None:
                    rc, out = 126, "refused: arguments must be JSON {\"args\": [...]}"
                else:
                    try:
                        rc, out = run_oo(oo, args, env)
                    except subprocess.TimeoutExpired:
                        rc, out = 124, "timed out"
            calls.append({"args": args, "rc": rc, "seconds": round(time.time() - ts, 2), "output": out})
            msgs.append({"role": "tool", "tool_call_id": tc["id"], "content": f"exit {rc}\n{out}"})
    return {"calls": calls, "final": final, "seconds": round(time.time() - t0, 2), "llm_calls": llm_calls,
            "tokens": usage, "hit_call_limit": len(calls) >= max_calls}


# ---------------------------------------------------------------- scoring

def split_answer(text):
    m = list(re.finditer(r"ANSWER:", text))
    if not m:
        return "", text
    rest = text[m[-1].end():]
    s = re.search(r"SOURCES?:", rest)
    return (rest[:s.start()] if s else rest).strip(), (rest[s.end():] if s else "").strip()


def norm(t):
    t = t.lower()
    for en, zh in EN_NAMES.items():
        t = t.replace(en, zh)
    return re.sub(r"(?<=\d),(?=\d{3})", "", t)


def numbers(t):
    return [float(x) for x in re.findall(r"-?\d+(?:\.\d+)?", norm(t))]


def match(gold, answer):
    t = gold["type"]
    if t == "all":
        return all(match(p, answer) for p in gold["parts"])
    a = norm(answer)
    if t == "entities":
        return all(re.search(m.lower(), a) for m in gold["must"]) and not any(re.search(f.lower(), a) for f in gold["forbid"])
    if t == "number":
        vals = [gold["value"], *gold.get("alts", [])]
        return any(abs(n - v) <= max(gold.get("tol", 0), 1e-9) for n in numbers(answer) for v in vals)
    return False


def corpus_paths():
    return [str(p.relative_to(CORPUS)) for p in CORPUS.rglob("*") if p.is_file()]


def refs_in(text, paths):
    found = {p for p in paths if p in text or Path(p).name in text}
    found |= set(re.findall(r"public\.(?:customers|products|orders|order_items)", text))
    found |= set(re.findall(r"\bq_[A-Za-z0-9]{16,}", text))  # a Gateway query id, as the CLI prints it
    return found


def score(q, rec, paths=None):
    paths = paths or corpus_paths()
    answer, sources = split_answer(rec["final"])
    declined = bool(DECLINE_RE.search(answer or rec["final"]))
    out = {"declined": declined, "answer": answer}
    if q["gold"]["type"] == "decline":
        out["correct"] = declined
        out["cite_valid"] = out["cite_gold"] = None
        return out
    out["correct"] = (not declined) and match(q["gold"], answer)
    cited = refs_in(sources or rec["final"], paths)
    seen = "\n".join(c["output"] for c in rec["calls"])
    # the SQL behind each query id the tool printed: a query id stands for the tables that SQL reads
    sql_of = {}
    for c in rec["calls"]:
        for qid in re.findall(r"\bq_[A-Za-z0-9]{16,}", c["output"]):
            sql_of[qid] = " ".join(c["args"] or [])
    grounded = {c for c in cited if c in seen or Path(c).name in seen}
    tables = {t for c in cited if c in sql_of for t in re.findall(r"public\.(?:customers|products|orders|order_items)", sql_of[c])}
    out["cite_valid"] = bool(grounded)
    out["cite_grounded"] = bool(cited) and grounded == cited
    out["cite_gold"] = any(s in cited | tables or Path(s).name in {Path(c).name for c in cited} for s in q["src"])
    return out


def report(rows):
    def agg(rs):
        n = len(rs)
        if not n:
            return None
        cit = [r for r in rs if r["score"]["cite_valid"] is not None]
        return {"n": n, "correct": sum(r["score"]["correct"] for r in rs), "accuracy": round(sum(r["score"]["correct"] for r in rs) / n, 3),
                "cite_valid": f"{sum(bool(r['score']['cite_valid']) for r in cit)}/{len(cit)}",
                "cite_gold": f"{sum(bool(r['score']['cite_gold']) for r in cit)}/{len(cit)}",
                "avg_calls": round(sum(len(r["calls"]) for r in rs) / n, 2),
                "avg_latency_s": round(sum(r["seconds"] for r in rs) / n, 1),
                "hit_call_limit": sum(r["hit_call_limit"] for r in rs)}
    res = {}
    for split in ("dev", "held", "all"):
        rs = [r for r in rows if split == "all" or r["split"] == split]
        res[split] = {"total": agg(rs), **{c: agg([r for r in rs if r["category"] == c])
                                           for c in ("file", "db", "cross", "unanswerable")}}
    return res


# ---------------------------------------------------------------- commands

def agent_env(args):
    env = {"PATH": "/usr/bin:/bin", "HOME": args.agent_home, "LC_ALL": "en_US.UTF-8",
           "OO_ENDPOINT": args.endpoint, "OO_AGENT_TOKEN_FILE": args.token_file}
    os.makedirs(args.agent_home, mode=0o700, exist_ok=True)
    return env


def cmd_run(args):
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    model = Model(args.model_endpoint_file, args.model_key_file, args.model)
    system = Path(args.system_prompt_file).read_text() if args.system_prompt_file else SYSTEM_PROMPT
    system = system.replace("{max_calls}", str(args.max_calls))
    env = agent_env(args)
    qs = load_questions(args.split)
    if args.only:
        qs = [q for q in qs if q["id"] in args.only.split(",")]
    paths = corpus_paths()

    def one(q):
        mcp = None
        try:
            mcp = McpSession(args.oo, env, args.endpoint, args.token_file) if args.mode == "mcp" else None
            rec = ask(model, args.oo, env, q["question"], args.max_calls, MCP_SYSTEM.replace("{max_calls}", str(args.max_calls)) if mcp else system, mcp)
        except Exception as e:  # a model outage is a logged failure, never a silent skip
            rec = {"calls": [], "final": "", "seconds": 0, "llm_calls": 0, "tokens": 0, "hit_call_limit": False,
                   "error": str(e)}
        if mcp:
            mcp.close()
        rec.update(id=q["id"], split=q["split"], category=q["category"], question=q["question"])
        rec["score"] = score(q, rec, paths)
        (out / f"{q['id']}.json").write_text(json.dumps(rec, ensure_ascii=False, indent=1))
        print(f"{q['id']} {'PASS' if rec['score']['correct'] else 'FAIL'} calls={len(rec['calls'])} {rec['seconds']}s", flush=True)
        return rec

    with concurrent.futures.ThreadPoolExecutor(args.workers) as ex:
        rows = list(ex.map(one, qs))
    summary = report(rows)
    (out / "summary.json").write_text(json.dumps(summary, ensure_ascii=False, indent=1))
    print(json.dumps(summary, ensure_ascii=False, indent=1))


def cmd_score(args):
    out = Path(args.out)
    qs = {q["id"]: q for q in load_questions("all")}
    rows = []
    for f in sorted(out.glob("[DH][0-9][0-9].json")):
        rec = json.loads(f.read_text())
        rec["score"] = score(qs[rec["id"]], rec)
        f.write_text(json.dumps(rec, ensure_ascii=False, indent=1))
        if args.split in ("all", rec["split"]):
            rows.append(rec)
    summary = report(rows)
    (out / "summary.json").write_text(json.dumps(summary, ensure_ascii=False, indent=1))
    print(json.dumps(summary, ensure_ascii=False, indent=1))


def cmd_safety(args):
    """Token checks through the CLI: a write is denied, a docs-only token cannot read rows, a revoked or expired token is 401."""
    import http.client
    import urllib.parse
    results = []

    def check(name, ok, detail=""):
        results.append({"check": name, "ok": bool(ok), "detail": str(detail)[:300]})
        print(("PASS " if ok else "FAIL ") + name + ("" if ok else "  " + str(detail)[:200]), flush=True)

    def raw(token, method, path, body=None):
        u = urllib.parse.urlparse(args.endpoint)
        c = http.client.HTTPConnection(u.hostname, u.port, timeout=30)
        h = {"Authorization": "Bearer " + token}
        data = None
        if body is not None:
            data, h["Content-Type"] = json.dumps(body).encode(), "application/json"
        c.request(method, path, body=data, headers=h)
        r = c.getresponse()
        raw_body = r.read()
        c.close()
        return r.status, raw_body.decode(errors="replace")

    base = {"PATH": "/usr/bin:/bin", "HOME": args.agent_home, "OO_ENDPOINT": args.endpoint}
    os.makedirs(args.agent_home, mode=0o700, exist_ok=True)

    def cli(token_file, *a):
        env = dict(base, OO_AGENT_TOKEN_FILE=token_file)
        p = subprocess.run([args.oo, *a], env=env, capture_output=True, text=True, timeout=60)
        return p.returncode, p.stdout + p.stderr

    def mint(name, scopes, ttl="1d"):
        f = Path(args.work) / f"{name}.tok"
        if f.exists():
            f.unlink()
        subprocess.run([args.oo, "agent", "token", "revoke", name, "--state-dir", args.state_dir], env=dict(base),
                       capture_output=True, text=True, timeout=60)  # a run before this one may have left a live token
        cmd = [args.oo, "agent", "token", "create", "--name", name, "--ttl", ttl, "--output-file", str(f), "--state-dir", args.state_dir,
               "--drive", args.drive, "--json"]
        for s in scopes:
            cmd += ["--scope", s]
        if "db:read" in scopes:
            cmd += ["--gw-profile", args.gw_profile]
        p = subprocess.run(cmd, env=dict(base), capture_output=True, text=True, timeout=60)
        return f if p.returncode == 0 else None, p.stderr[-200:]

    Path(args.work).mkdir(parents=True, exist_ok=True)
    rw, e1 = mint("safety-rw", ["docs:read", "db:read"])
    ro_docs, e2 = mint("safety-docs", ["docs:read"])
    check("both tokens minted", rw and ro_docs, (e1, e2))
    if not (rw and ro_docs):
        return
    # a read token can read through the CLI
    rc, o = cli(str(rw), "ontology", "query", "select count(*) as n from public.customers")
    check("read token: SQL select works", rc == 0 and "200" in o, o[-200:])
    # a write attempt with the read token is denied (CLI, and the raw route)
    rc, o = cli(str(rw), "ontology", "query", "delete from public.customers where id = 1")
    check("read token: DELETE through the CLI is denied", rc != 0, o[-200:])
    rc, o = cli(str(rw), "ontology", "query", "insert into public.customers values (9999,'x','east','gold')")
    check("read token: INSERT through the CLI is denied", rc != 0, o[-200:])
    st, body = raw(rw.read_text().strip(), "POST", "/v1/ontology/query", {"sql": "update public.orders set status='paid'"})
    check("read token: UPDATE on the raw route is refused", st in (400, 403, 422), (st, body[:120]))
    probe = str(Path(args.work).resolve())
    st, body = raw(rw.read_text().strip(), "POST", "/v1/docs/drives", {"name": "evil", "backend": "fs", "config": {"root": probe}})
    check("read token: creating a drive is refused", st in (401, 403, 404), (st, body[:120]))
    mounts = Path(args.state_dir) / "drive-mounts.json"
    check("read token: the refused drive request mounted nothing", not mounts.exists() or probe not in mounts.read_text(), "mounted")
    st, body = raw(rw.read_text().strip(), "POST", "/v1/docs/drives",
                   {"name": "evil", "backend": "fs", "config": {"root": "/definitely/not/a/directory"}})
    check("read token: a bad root gets the same 403, not a path probe", st == 403, (st, body[:120]))
    rc, o = cli(str(rw), "ontology", "query", "select count(*) from public.customers")
    check("the table is unchanged after the write attempts", rc == 0 and "200" in o, o[-120:])
    # a docs-only token cannot read DB rows
    rc, o = cli(str(ro_docs), "ontology", "query", "select * from public.customers limit 1")
    check("docs-only token: row query is denied", rc != 0 and "Acme" not in o, o[-200:])
    rc, o = cli(str(ro_docs), "ontology", "search", "澄海物流")
    sides = set(re.findall(r"^(doc|db) ", o, re.M))
    check("docs-only token: search returns no database rows", rc == 0 and "db" not in sides, (sides, o[-120:]))
    st, body = raw(ro_docs.read_text().strip(), "POST", "/v1/ontology/query", {"sql": "select * from public.customers limit 1"})
    check("docs-only token: raw query route is 403", st == 403, (st, body[:120]))
    # revoked token is 401
    gone, e3 = mint("safety-gone", ["docs:read", "db:read"])
    rc, _ = cli(str(gone), "ontology", "sources")
    check("to-be-revoked token works first", rc == 0)
    subprocess.run([args.oo, "agent", "token", "revoke", "safety-gone", "--state-dir", args.state_dir], env=dict(base),
                   capture_output=True, text=True, timeout=60)
    st, _ = raw(gone.read_text().strip(), "GET", "/v1/ontology/sources")
    rc, o = cli(str(gone), "ontology", "sources")
    check("revoked token: 401 on the raw route", st == 401, st)
    check("revoked token: CLI fails", rc != 0, o[-160:])
    # expired token: a 1-second TTL
    short, e4 = mint("safety-short", ["docs:read"], ttl="60s")
    if short:
        st, _ = raw(short.read_text().strip(), "GET", "/v1/ontology/sources")
        check("a 60 s token works before it expires", st == 200, st)
        time.sleep(65)
        st, _ = raw(short.read_text().strip(), "GET", "/v1/ontology/sources")
        check("expired token: 401", st == 401, st)
    else:
        check("expired token: could mint a 60s token", False, e4)
    st, _ = raw("ooa_" + "x" * 43, "GET", "/v1/ontology/sources")
    check("garbage token: 401", st == 401, st)
    Path(args.out).write_text(json.dumps(results, indent=1))
    print(f"{sum(r['ok'] for r in results)}/{len(results)} checks passed")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sp = ap.add_subparsers(dest="cmd", required=True)
    r = sp.add_parser("run")
    r.add_argument("--oo", required=True)
    r.add_argument("--endpoint", required=True)
    r.add_argument("--token-file", required=True)
    r.add_argument("--out", required=True)
    r.add_argument("--split", default="all", choices=["dev", "held", "all"])
    r.add_argument("--only", default="")
    r.add_argument("--model", default="gcli/grok-4.7")
    r.add_argument("--model-endpoint-file", default=str(SECRETS / "model-endpoint-9r"))
    r.add_argument("--model-key-file", default=str(SECRETS / "model-key-9r-sol"))
    r.add_argument("--max-calls", type=int, default=12)
    r.add_argument("--workers", type=int, default=3)
    r.add_argument("--mode", default="cli", choices=["cli", "mcp"])
    r.add_argument("--agent-home", default="/private/tmp/soV/agent-home")
    r.add_argument("--system-prompt-file", default="")
    s = sp.add_parser("score")
    s.add_argument("--out", required=True)
    s.add_argument("--split", default="all", choices=["dev", "held", "all"])
    f = sp.add_parser("safety")
    f.add_argument("--oo", required=True)
    f.add_argument("--endpoint", required=True)
    f.add_argument("--state-dir", required=True)
    f.add_argument("--gw-profile", default="shop")
    f.add_argument("--drive", default="golden")
    f.add_argument("--work", required=True)
    f.add_argument("--out", required=True)
    f.add_argument("--agent-home", default="/private/tmp/soV/agent-home")
    a = ap.parse_args()
    {"run": cmd_run, "score": cmd_score, "safety": cmd_safety}[a.cmd](a)


if __name__ == "__main__":
    sys.dont_write_bytecode = True
    main()
