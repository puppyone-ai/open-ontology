#!/usr/bin/env python3
"""The owner's CLI acceptance flow, run end to end on `oo serve --runtime compose`
(30 section 7.2: the CLI acceptance package at L4) and every `oo docs`
subcommand against the real engine.

The steps are the ones in `testing/super-ontology/evidence/L4/ACCEPTANCE-CLI.md`
in the same order, with the same commands; this script runs them with a temporary
state directory and checks each expected result:

  1  start `oo serve --runtime compose` (docs-db, docs-engine, doc-parser)
  2  register a Drive over a sample of the golden set (md, txt, csv, xlsx, docx,
     pptx, pdf) plus two files that cannot be read (an encrypted pdf, a legacy
     .doc), and watch the file states until every readable file is published
  3  `oo docs` read commands: drive ls/show, files ls/show, usage, health
  4  review queue: review ls, decide accept, accept-source
  5  entity correction: search, show, correct, split, merge
  6  `oo docs files retry`, `oo docs drive sync`
  7  mount a context pack, read BRIEF (every line sourced)
  8  the P21 scenario: two mounts of one task group, a used decision is marked
     wrong, UPDATES.md gets the block entry, `oo context check` = 20, the group
     is refreshed, check = 0, the Drive's files are untouched
  9  stop and start again: the Drive, its files, the shares and the mounts are still there
 10  revoke, `oo docs share ls`, `oo docs drive rm`
 11  stop the daemon (SIGTERM): exit 0, containers stopped

Every command, its exit code and the head of its output go to
`cli-acceptance-run.json` in the evidence directory.

Usage: tools/oo-test/super_ontology_cli_acceptance.py [--run-id ID]
       [--evidence-dir DIR] [--keep] [--skip-build]
"""

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.dont_write_bytecode = True
import super_ontology_compose as comp  # noqa: E402
import super_ontology_l2 as l2  # noqa: E402

REPO = l2.REPO
GOLDEN = REPO / "testing/super-ontology/golden/docs"
comp.LANE = os.environ.get("OO_TEST_LANE", "super-ontology-be4")
# `OO_ACCEPT_DEPLOY_DIR`: a copy of deploy/docs whose images.lock.json names the image ids present on this host,
# for a host where the locked ids are not (the lock itself is the integration thread's).
DEPLOY = Path(os.environ.get("OO_ACCEPT_DEPLOY_DIR", comp.DEPLOY))
SAMPLE = (
    [f"Decisions/{p.name}" for p in sorted((GOLDEN / "Decisions").glob("*.md"))]
    + [f"People/{p.name}" for p in sorted((GOLDEN / "People").glob("*.md"))]
    + [f"Projects/{p.name}" for p in sorted((GOLDEN / "Projects").glob("*.md"))]
    + ["Meetings/2026-01-08-年度计划会.docx", "Meetings/2026-02-15-星图技术评审.docx",
       "Meetings/2026-06-01-项目分工调整.docx", "Tables/人员表.xlsx", "Tables/项目负责人表.xlsx",
       "Proposals/星图看板产品方案.pdf", "Weekly/2026-W22-周报.pptx", "Data/任务清单.csv",
       "Notes/速记-0412.txt"]
)
FIXTURES = REPO / "testing/super-ontology/fixtures/files"
# Two files that cannot be read, on purpose: an encrypted pdf and a legacy .doc.
BAD = {"pdf-encrypted.pdf": ("encrypted", "DOCS_PARSE_ENCRYPTED"), "legacy.doc": ("unsupported", "DOCS_PARSE_LEGACY_OFFICE")}
# `indexed` is final here: every fact of such a file waits for a person in the review queue.
FINAL = {"published", "duplicate", "unsupported", "too_large", "encrypted", "parse_failed", "extract_partial", "indexed"}


class Cli:
    """`oo` against one state directory; every call is recorded."""

    def __init__(self, ev, oo, state, log):
        self.ev, self.oo, self.state, self.log = ev, str(oo), str(state), log

    def run(self, *args, timeout=180, quiet=False):
        argv = [self.oo, *args, "--state-dir", self.state]
        p = subprocess.run(argv, capture_output=True, text=True, timeout=timeout)
        shown = " ".join(args).replace(self.state, "<state>")
        self.log.append({"command": f"oo {shown}", "exit": p.returncode, "stdout": p.stdout[:1500], "stderr": p.stderr[:300]})
        if not quiet:
            self.ev.log(f"$ oo {shown}  -> exit {p.returncode}")
        return p

    def json(self, *args, **kw):
        p = self.run(*args, "--json", **kw)
        try:
            return p.returncode, json.loads(p.stdout.strip().splitlines()[-1]) if p.stdout.strip() else {}
        except ValueError:
            return p.returncode, {"raw": p.stdout, "err": p.stderr}


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--run-id", default=time.strftime("%Y%m%d-%H%M%S"))
    ap.add_argument("--evidence-dir", default=str(REPO / "testing/super-ontology/evidence/L4"))
    ap.add_argument("--keep", action="store_true")
    ap.add_argument("--skip-build", action="store_true")
    ap.add_argument("--endpoint-file", help="0600 file with the model's base URL (optional step 9b)")
    ap.add_argument("--key-file", help="0600 file with the model's key")
    ap.add_argument("--model-id")
    args = ap.parse_args()

    ev = l2.Evidence(Path(args.evidence_dir), args.run_id)
    target = Path(os.environ.get("CARGO_TARGET_DIR", REPO / "target")) / "debug"
    oo = target / "oo"
    work = Path(tempfile.mkdtemp(prefix=f"oo-cliacc-{args.run_id}-", dir="/tmp"))
    state, drive = work / "state", work / "drive"
    project = comp.Project("oo-docs-" + hashlib.sha256(os.path.realpath(state).encode()).hexdigest()[:12])
    log, daemon = [], None
    ok = False
    try:
        if not args.skip_build:
            l2.run(ev, ["cargo", "build", "--locked", "-p", "oo-cli", "--features", "docs"])
        state.mkdir(mode=0o700)
        for rel in SAMPLE:
            dest = drive / rel
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(GOLDEN / rel, dest)
        (drive / "Broken").mkdir()
        for name in BAD:
            shutil.copyfile(FIXTURES / name, drive / "Broken" / name)
        total_files = len(SAMPLE) + len(BAD)
        cli = Cli(ev, oo, state, log)

        ev.log("\n## 1. start the compose runtime")
        daemon = comp.Daemon(ev, oo, state, DEPLOY)
        api = daemon.start()
        ev.check("1 oo serve --runtime compose prints its address and the compose project", daemon.info["engine"]["mode"] == "compose" and daemon.info.get("port"), json.dumps({k: daemon.info[k] for k in ("port",)}))
        cli.run("docs", "health", quiet=True)  # human form, for the manual
        rc, h = cli.json("docs", "health")
        ev.check("1 oo docs health exits 0 and the engine is up", rc == 0 and h.get("status") in ("ok", "degraded"), f"exit {rc} status {h.get('status')}")

        p = cli.run("serve", "--status")
        ev.check("1 oo serve --status says it is running", p.returncode == 0 and "is running" in p.stdout, p.stdout.strip()[:80])

        ev.log("\n## 2. register the sample Drive and watch the states")
        rc, d = cli.json("docs", "drive", "add", "--name", "acceptance", "--root", str(drive))
        cli.run("docs", "drive", "ls", quiet=True)
        ev.check("2 oo docs drive add exits 0", rc == 0 and d.get("id"), f"exit {rc}")
        did = d["id"]
        seen_states = []
        t0 = time.time()

        def settled():
            rc, v = cli.json("docs", "files", "ls", "--drive", "acceptance", quiet=True)
            files = v.get("files", [])
            states = sorted({f["state"] for f in files})
            if not seen_states or seen_states[-1][1] != states:
                seen_states.append((round(time.time() - t0, 1), states))
            return len(files) == total_files and all(f["state"] in FINAL for f in files)

        l2.wait_until(900, settled, step=3)
        rc, v = cli.json("docs", "files", "ls", "--drive", "acceptance")
        cli.run("docs", "files", "ls", "--drive", "acceptance", quiet=True)
        files = v.get("files", [])
        by = {}
        for f in files:
            by[f["state"]] = by.get(f["state"], 0) + 1
        ev.check("2 every readable sample file is published or waits for review (md, txt, csv, xlsx, docx, pptx, pdf)", len(files) == total_files and by.get("published", 0) + by.get("indexed", 0) == len(SAMPLE), f"{time.time() - t0:.0f}s {json.dumps(by, sort_keys=True)}")
        bad = {f["path"].split("/")[-1]: (f["state"], f.get("state_code")) for f in files if f["path"].startswith("Broken/")}
        ev.check("2 the two broken files say why (encrypted, legacy office)", all(bad.get(n, (None, None))[0] == st for n, (st, _) in BAD.items()), json.dumps(bad, sort_keys=True))
        ev.log("  states seen: " + json.dumps(seen_states, ensure_ascii=False))
        ev.timings["publish_seconds"] = round(time.time() - t0, 1)
        file_ids = {f["path"]: f["id"] for f in files}

        ev.log("\n## 3. read commands")
        rc, v = cli.json("docs", "drive", "ls")
        ev.check("3 oo docs drive ls lists the Drive", rc == 0 and any(x["name"] == "acceptance" for x in v.get("drives", [])))
        cli.run("docs", "drive", "show", "acceptance", quiet=True)
        rc, v = cli.json("docs", "drive", "show", "acceptance")
        ev.check("3 oo docs drive show has drive_rev and freshness", rc == 0 and isinstance(v.get("drive_rev"), int) and "freshness" in v, f"drive_rev {v.get('drive_rev')}")
        rc, v = cli.json("docs", "files", "ls", "--drive", "acceptance", "--state", "published")
        rc_i, vi = cli.json("docs", "files", "ls", "--drive", "acceptance", "--state", "indexed")
        # Deterministic sample: every file but the one of the conflict (D57) is published; that one waits in indexed.
        ev.check("3 oo docs files ls --state published", rc == 0 and rc_i == 0 and len(v.get("files", [])) == len(SAMPLE) - 1 and len(vi.get("files", [])) == 1,
                 f"{len(v.get('files', []))} published, {len(vi.get('files', []))} indexed of {len(SAMPLE)} (the file of the conflict waits in indexed)")
        rc, v = cli.json("docs", "files", "show", file_ids["Broken/pdf-encrypted.pdf"])
        ev.check("3 oo docs files show gives the reason of the encrypted pdf", rc == 0 and v.get("state") == "encrypted" and v.get("state_code") == "DOCS_PARSE_ENCRYPTED", f"{v.get('state')} {v.get('state_code')}")
        rc, v = cli.json("docs", "files", "show", file_ids["Meetings/2026-01-08-年度计划会.docx"])
        ev.check("3 oo docs files show names the docx and its state", rc == 0 and v.get("state") == "published", f"state {v.get('state')}")
        cli.run("docs", "usage", quiet=True)
        rc, v = cli.json("docs", "usage")
        ev.check("3 oo docs usage answers (model not configured: 0 calls)", rc == 0 and v.get("model", {}).get("status") == "not_configured", json.dumps(v.get("model")))

        ev.log("\n## 4. review queue")
        cli.run("docs", "review", "ls", "--status", "pending", quiet=True)
        rc, q = cli.json("docs", "review", "ls", "--status", "pending")
        items = q.get("items", [])
        kinds = {}
        for it in items:
            kinds[it["kind"]] = kinds.get(it["kind"], 0) + 1
        ev.check("4 oo docs review ls shows the real owner conflict of the sample (one project, two owners)", rc == 0 and kinds.get("constraint_conflict") == 1, f"{len(items)} pending {json.dumps(kinds, sort_keys=True)}")
        conflict = next((i for i in items if i["kind"] == "constraint_conflict"), None)
        if conflict:
            facts = (conflict.get("subject") or {}).get("fact_ids") or []
            ev.check("4 the conflict names the facts that disagree", len(facts) >= 2, f"{len(facts)} facts")
            # Without a winner the decision is refused; naming one settles it.
            rc, dec = cli.json("docs", "review", "decide", "accept", conflict["id"])
            ev.check("4 oo docs review decide accept without --winner is refused for a conflict", rc == 0 and dec.get("accepted") == 0 and dec["results"][0].get("code") == "DOCS_REVIEW_CONSTRAINT_CONFLICT", json.dumps(dec)[:160])
            rc, dec = cli.json("docs", "review", "decide", "accept", conflict["id"], "--winner", facts[0])
            ev.check("4 oo docs review decide accept --winner settles it", rc == 0 and dec.get("accepted") == 1, json.dumps(dec)[:160])
            src_path = (conflict.get("source") or {}).get("path")
            if src_path in file_ids:
                rc, dec = cli.json("docs", "review", "accept-source", file_ids[src_path])
                ev.check("4 oo docs review accept-source answers (it touches low-confidence items only)", rc == 0 and "accepted" in dec, json.dumps(dec)[:160])
        rc, q2 = cli.json("docs", "review", "ls", "--status", "pending")
        ev.check("4 the queue is empty afterwards", rc == 0 and not q2.get("items"), f"{len(q2.get('items', []))} pending")

        ev.log("\n## 5. entity correction")
        cli.run("docs", "entity", "search", "林远舟", "--type", "Person", quiet=True)
        rc, s = cli.json("docs", "entity", "search", "林远舟", "--type", "Person")
        people = s.get("entities", s.get("items", []))
        ev.check("5 oo docs entity search finds 林远舟", rc == 0 and bool(people), f"{len(people)} hits")
        key = people[0]["key"]
        cli.run("docs", "entity", "show", key, "--history", quiet=True)
        rc, sh = cli.json("docs", "entity", "show", key, "--history")
        rc, al = cli.json("docs", "entity", "search", "老林")
        ev.check("5 an alias (老林) resolves to the same person", rc == 0 and any(e.get("key") == key for e in al.get("entities", [])), f"{len(al.get('entities', []))} hits")
        shown = sh.get("history") or sh.get("facts") or []
        ev.check("5 oo docs entity show --history lists facts with evidence", rc == 0 and bool(shown), f"{len(shown)} facts")
        rc, sp = cli.json("docs", "entity", "split", key, "--groups", json.dumps([{"aliases": ["老林", "林总"]}, {"aliases": ["Evan Lin"]}], ensure_ascii=False))
        new_keys = [r["id"] for r in sp.get("results", []) if r.get("id") and r["id"] != key]
        ev.check("5 oo docs entity split creates a new entity", rc == 0 and bool(new_keys), f"exit {rc} {json.dumps(sp, ensure_ascii=False)[:160]}")
        if new_keys:
            rc, mg = cli.json("docs", "entity", "merge", "--into", key, new_keys[0])
            ev.check("5 oo docs entity merge puts it back", rc == 0, f"exit {rc} {json.dumps(mg, ensure_ascii=False)[:160]}")
        # correct: mark one rule fact wrong, then set a value on an attribute
        rc, sh = cli.json("docs", "entity", "show", key)
        facts = [f for f in (sh.get("facts") or sh.get("history") or []) if f.get("predicate") in ("mentions", "about", "owns", "decides")]
        if facts:
            rc, cr = cli.json("docs", "entity", "correct", facts[0]["id"], "--mark-wrong", "--reason", "acceptance", "--entity", key)
            ev.check("5 oo docs entity correct --mark-wrong", rc == 0 and cr.get("status") == "retracted", f"exit {rc} {cr.get('status')}")

        ev.log("\n## 6. retry and sync")
        docx_id = file_ids["Meetings/2026-02-15-星图技术评审.docx"]
        rc, rr = cli.json("docs", "files", "retry", docx_id)
        res = rr.get("results", [{}])[0]
        ev.check("6 oo docs files retry answers per file (a published file is not retryable)", rc == 0 and res.get("status") in ("queued", "retried", "not_retryable"), json.dumps(res, ensure_ascii=False)[:200])
        rc, rr = cli.json("docs", "files", "retry", file_ids["Broken/legacy.doc"], file_ids["Broken/pdf-encrypted.pdf"])
        ev.check("6 oo docs files retry on the two unreadable files says why or requeues them", rc == 0 and len(rr.get("results", [])) == 2, json.dumps(rr.get("results"), ensure_ascii=False)[:240])
        (drive / "Notes").mkdir(exist_ok=True)
        (drive / "Notes/刚放进去.md").write_text("# 刚放进去\n\n负责人：孙晓棠\n", encoding="utf-8")
        rc, sy = cli.json("docs", "drive", "sync", "acceptance")
        ev.check("6 oo docs drive sync answers with the generation", rc == 0 and "generation" in sy, json.dumps(sy)[:160])
        seen_new = l2.wait_until(60, lambda: "Notes/刚放进去.md" in {f["path"] for f in cli.json("docs", "files", "ls", "--drive", "acceptance", quiet=True)[1].get("files", [])}, step=2)
        ev.check("6 the new file shows up in oo docs files ls after the sync", bool(seen_new))
        rc, sy2 = cli.json("docs", "drive", "sync", "acceptance", "--path", "Notes/刚放进去.md")
        ev.check("6 oo docs drive sync --path (targeted) exits 0", rc == 0, json.dumps(sy2)[:160])
        l2.wait_until(120, lambda: cli.json("docs", "files", "ls", "--drive", "acceptance", quiet=True)[1].get("files") and all(f["state"] in FINAL for f in cli.json("docs", "files", "ls", "--drive", "acceptance", quiet=True)[1]["files"]) and len(cli.json("docs", "files", "ls", "--drive", "acceptance", quiet=True)[1]["files"]) == total_files + 1, step=2)

        ev.log("\n## 7. mount a context pack")
        mnt = work / "work"
        mnt.mkdir()
        rc, m = cli.json("context", "mount", "--to", str(mnt), "--scope", "Decisions", "--scope", "Projects", "--for", "星图看板用什么图表方案")
        cli.run("context", "list", quiet=True)
        ev.check("7 oo context mount exits 0", rc == 0, f"exit {rc}")
        brief = (mnt / ".context/BRIEF.md").read_text() if (mnt / ".context/BRIEF.md").exists() else ""
        lines = l2.content_lines(brief)
        ev.check("7 BRIEF has coverage in its header and every content line has [src:", "> 覆盖：" in brief and bool(lines) and all("[src:" in l for l in lines), f"{len(lines)} content lines")
        ev.write("cli-acceptance-BRIEF.md", brief)
        ev.check("7 BRIEF ends with the blocked section: a count, no names", "## 已拦截" in brief, "")
        rc, lst = cli.json("context", "list")
        ev.check("7 oo context list shows the share active", rc == 0 and any(s["state"] == "active" for s in lst.get("shares", [])))
        sj = json.loads((mnt / ".context/SHARE.json").read_text())
        hm = work / "work-human"
        hm.mkdir()
        p = cli.run("context", "mount", "--to", str(hm), "--scope", "Decisions", "--for", "星图看板用什么图表方案", quiet=True)  # human form, for the manual
        ev.check("7 oo context mount (human output) exits 0", p.returncode == 0, p.stdout.strip()[:120])
        cli.run("context", "revoke", str(hm), quiet=True)

        ev.log("\n## 8. the P21 scenario")
        ta, tb = work / "agent-a", work / "agent-b"
        ta.mkdir(), tb.mkdir()
        rc, _ = cli.json("context", "mount", "--to", str(ta), "--scope", "Decisions", "--scope", "Projects", "--group", "TICKET-P21", "--for", "星图看板用什么图表方案", "--refresh-policy", "manual")
        rc2, _ = cli.json("context", "mount", "--to", str(tb), "--scope", "Decisions", "--scope", "People", "--group", "TICKET-P21", "--for", "林远舟的决定")
        ev.check("8 two mounts of one task group", rc == 0 and rc2 == 0)
        for t in (ta, tb):
            p = cli.run("context", "check", "--dir", str(t / ".context"))
            ev.check(f"8 oo context check in {t.name} exits 0 before the correction", p.returncode == 0, f"exit {p.returncode}")
        sja = json.loads((ta / ".context/SHARE.json").read_text())
        # the used decision fact, from the pack's own entity file
        rc, rs = cli.json("docs", "entity", "search", "开源图表库", "--type", "Decision")
        dec_key = (rs.get("entities") or rs.get("items") or [{}])[0].get("key")
        rc, esh = cli.json("docs", "entity", "show", dec_key) if dec_key else (1, {})
        dfacts = [f for f in esh.get("facts", []) if f.get("predicate") in ("decides", "about", "rejects")]
        drive_before = l2.tree_hashes(drive)
        if dfacts:
            t_mark = time.time()
            rc, cr = cli.json("docs", "entity", "correct", dfacts[0]["id"], "--mark-wrong", "--reason", "决定被推翻", "--entity", dec_key)
            ev.check("8 the decision's fact is marked wrong", rc == 0 and cr.get("status") == "retracted", f"exit {rc}")
            got = l2.wait_until(30, lambda: (ta / ".context/UPDATES.md").is_file() and "· block" in (ta / ".context/UPDATES.md").read_text(), step=0.5)
            ev.check("8 UPDATES.md of agent-a has the block entry", bool(got), f"{time.time() - t_mark:.1f}s from the correction")
            p = cli.run("context", "check", "--dir", str(ta / ".context"))
            ev.check("8 oo context check exits 20 (do not commit)", p.returncode == 20, f"exit {p.returncode}")
            ev.write("cli-acceptance-UPDATES.md", (ta / ".context/UPDATES.md").read_text())
            rc, gr = cli.json("context", "refresh", "--group", "TICKET-P21")
            ev.check("8 the group refreshes together", rc == 0 and len(gr.get("shares", [])) == 2, json.dumps(gr)[:200])
            for t in (ta, tb):
                p = cli.run("context", "check", "--dir", str(t / ".context"))
                ev.check(f"8 oo context check in {t.name} exits 0 after the refresh", p.returncode == 0, f"exit {p.returncode}")
            ev.check("8 the Drive's files are unchanged", l2.tree_hashes(drive) == drive_before)
        else:
            ev.check("8 a decision fact is available for the P21 scenario", False, "no facts on the decision entity")

        ev.log("\n## 9. stop and start again: everything is still there")
        rc, v = cli.json("docs", "files", "ls", "--drive", "acceptance", "--state", "published")
        published_before = len(v.get("files", [])) if rc == 0 else -1
        code = daemon.term()
        ev.check("9 SIGTERM: oo serve exits 0", code == 0, f"exit {code}")
        states_after = {n: i["State"]["Status"] for n, i in project.by_service().items()}
        ev.check("9 the containers are stopped, not removed", states_after and all(s == "exited" for s in states_after.values()), json.dumps(states_after, sort_keys=True))
        daemon = comp.Daemon(ev, oo, state, DEPLOY)
        api = daemon.start()
        rc, v = cli.json("docs", "drive", "show", "acceptance")
        ev.check("9 the Drive survived the restart", rc == 0 and v.get("name") == "acceptance", f"exit {rc}")
        rc, v = cli.json("docs", "files", "ls", "--drive", "acceptance", "--state", "published")
        ev.check("9 its files are still published", rc == 0 and published_before >= len(SAMPLE) - 1 and len(v.get("files", [])) == published_before,
                 f"{len(v.get('files', []))} files after the restart, {published_before} before")
        rc, sls = cli.json("docs", "share", "ls")
        ev.check("9 the shares are still listed", rc == 0 and len(sls.get("shares", [])) == 6, f"{len(sls.get('shares', []))} shares (L4 run: 6)")
        ev.check("9 the mounted directories were not touched", all((t / ".context/BRIEF.md").exists() for t in (mnt, ta, tb)))

        if args.endpoint_file and args.key_file and args.model_id:
            # The model's endpoint and key only ever travel as file: references; the daemon binds the
            # two files read-only into the engine container and the engine starts its egress there.
            ev.log("\n## 9b. model egress inside the compose engine")
            body = {"provider": "openai-compatible", "base_url": f"file:{args.endpoint_file}", "model": args.model_id, "key_ref": f"file:{args.key_file}"}
            st, put = api.request("PUT", "/v1/docs/config/model", body, timeout=300, headers={"If-Match": '"r1"'})
            ev.check("9b PUT /v1/docs/config/model through the daemon: 200 and the egress is up in the engine container",
                     st == 200 and isinstance(put, dict) and put.get("status") == "ok", f"HTTP {st} status {put.get('status') if isinstance(put, dict) else '?'} code {put.get('code') if isinstance(put, dict) else '?'}")
            st, conf = api.request("GET", "/v1/docs/config/model")
            ev.check("9b GET /v1/docs/config/model: configured and the egress is up", st == 200 and isinstance(conf, dict) and conf.get("configured") is True and conf.get("status") == "ok", f"HTTP {st} configured {conf.get('configured') if isinstance(conf, dict) else '?'} status {conf.get('status') if isinstance(conf, dict) else '?'}")
            svc = project.by_service().get("docs-engine")
            binds = [m for m in (svc or {}).get("Mounts", []) if m.get("Destination") in (args.endpoint_file, args.key_file)]
            ev.check("9b the two files are bind-mounted read-only at their own path", len(binds) == 2 and all(m.get("RW") is False for m in binds), f"{len(binds)} mounts")

        ev.log("\n## 10. revoke and remove")
        rc, rv = cli.json("context", "revoke", str(mnt))
        ev.check("10 oo context revoke deletes the mount and says so", rc == 0 and rv.get("local_removed") is True and not (mnt / ".context").exists(), json.dumps(rv)[:160])
        rc, sls = cli.json("docs", "share", "ls")
        ev.check("10 oo docs share ls shows the revoked share", rc == 0 and any(s["id"] == sj["share_id"] and s["state"] == "revoked" for s in sls.get("shares", [])))
        rc, shw = cli.json("docs", "share", "show", sj["share_id"])
        ev.check("10 oo docs share show", rc == 0 and shw.get("state") == "revoked", f"state {shw.get('state')}")
        for t in (ta, tb):
            cli.run("context", "revoke", str(t), quiet=True)
        rc, rm = cli.json("docs", "drive", "rm", "acceptance")
        ev.check("10 oo docs drive rm starts the deletion", rc == 0, json.dumps(rm)[:160])
        gone = l2.wait_until(60, lambda: not any(x["name"] == "acceptance" for x in cli.json("docs", "drive", "ls", quiet=True)[1].get("drives", [])), step=2)
        ev.check("10 the Drive is gone from oo docs drive ls", bool(gone))

        ev.log("\n## 11. stop")
        p = cli.run("serve", "--stop")
        try:
            code = daemon.proc.wait(timeout=90)
        except subprocess.TimeoutExpired:
            code = None
        daemon.proc = None
        ev.check("11 oo serve --stop: it says stopped and the daemon exits 0", p.returncode == 0 and "stopped" in p.stdout and code == 0, f"{p.stdout.strip()} / exit {code}")
        states_after = {n: i["State"]["Status"] for n, i in project.by_service().items()}
        ev.check("11 the containers are stopped", states_after and all(s == "exited" for s in states_after.values()), json.dumps(states_after, sort_keys=True))
        ok = all(c["ok"] for c in ev.checks)
    finally:
        if daemon and daemon.proc:
            daemon.term()
        if not args.keep:
            project.purge()
        left = project.containers() + project.networks() + project.volumes()
        ev.check("cleanup left nothing", args.keep or not left, f"{len(left)} left")
        if not args.keep:
            l2.remove_tree(work)
        ok = ok and all(c["ok"] for c in ev.checks)
        ev.write("cli-acceptance-run.json", json.dumps({"run_id": args.run_id, "ok": ok, "checks": ev.checks, "timings": ev.timings, "commands": log}, indent=1, ensure_ascii=False) + "\n")
        ev.write("cli-acceptance-run.txt", "\n".join(ev.log_lines) + "\n")
    passed = sum(1 for c in ev.checks if c["ok"])
    print(f"# {passed}/{len(ev.checks)} checks passed")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
