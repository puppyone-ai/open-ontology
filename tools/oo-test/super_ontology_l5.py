#!/usr/bin/env python3
"""Super Ontology L5 Suggest end to end (30 section L5 acceptance 1-4; B-S7-01..05, B-S7-07, B-S8-01, B-S8-02).

  /usr/bin/python3 tools/oo-test/super_ontology_l5.py [--run-id ID] [--evidence-dir DIR] [--skip-build] [--keep]

Runs the real stack like the L2 script (labelled Postgres, the real `oo-docs-engine` under the real `oo serve`, the real
`oo context mount|collect`) over a small synthetic Drive. Needs migration 0143 compiled into the engine.

  A1  the idea document's section 4, steps 2 to 6: a Share for the task (`--role suggester`), the Worker reads BRIEF.md, writes
      one `suggest/` file, the collector sends it, the suggestion panel holds the two revisions, accept takes at most 2 s (API and CLI),
      a later Share's BRIEF has the new owner and status, and the old Share is told (UPDATES.md and `:check`)
  A2  the four impact rules, one case each, through the collector and the share credential
  A3  50 suggestions in a day: at most 5 human operations (counted here) and nothing waits afterwards
  A4  traceability: every accepted fact leads to its suggestion, the credential that wrote it, and the evidence block it cites
  A5  OO never writes the Drive: the Drive tree is byte identical after everything

Evidence (commands, outputs, timings) goes to testing/super-ontology/evidence/L5/. No token or credential is written there.
"""
import argparse
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import super_ontology_l2 as l2  # noqa: E402

REPO = l2.REPO
LANE = "oo.lane=super-ontology-be2"
ACCEPT_SECONDS = 2.0
HUMAN_OPERATION_LIMIT = 5

DRIVE_FILES = {
    "Projects/星火计划.md": "---\ntype: project\nname: 星火计划\nowner: 林川\n---\n# 星火计划\n\n负责人：林川\n\n状态：进行中\n",
    "People/林川.md": "---\ntype: person\nname: 林川\nrole: 工程负责人\n---\n# 林川\n",
    "People/小王.md": "---\ntype: person\nname: 小王\nrole: 工程师\n---\n# 小王\n",
}


def sh(argv, **kw):
    return subprocess.run(argv, capture_output=True, text=True, **kw)


def src_tag(brief, needle):
    """The `[src:...]` tag of the first BRIEF line that has `needle`."""
    for line in brief.splitlines():
        if needle in line and "[src:" in line:
            return line[line.index("[src:"): line.index("]", line.index("[src:")) + 1]
    return None


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--run-id", default=time.strftime("l5-%Y%m%d-%H%M%S"))
    ap.add_argument("--evidence-dir", default=str(REPO / "testing/super-ontology/evidence/L5"))
    ap.add_argument("--skip-build", action="store_true")
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args(argv)
    l2.LANE = LANE
    ev = l2.Evidence(Path(a.evidence_dir), a.run_id)
    ev.log(f"# Super Ontology L5 Suggest end to end, run {a.run_id}")
    target = Path(os.environ.get("CARGO_TARGET_DIR", REPO / "target")) / "debug"
    oo, engine = target / "oo", target / "oo-docs-engine"
    work = Path(tempfile.mkdtemp(prefix=f"oo-l5-{a.run_id}-", dir="/tmp"))
    pg = l2.Postgres(ev, a.run_id)
    serve = None
    try:
        if not a.skip_build:
            l2.run(ev, ["cargo", "build", "--locked", "-p", "oo-cli", "--features", "docs"])
            l2.run(ev, ["cargo", "build", "--locked", "-p", "oo-docs", "--bin", "oo-docs-engine"])
        pg.start()
        db, login = "oo_docs_l5", "oo_docs_l5_runtime"
        pg.psql("postgres", f"CREATE DATABASE {db}")
        admin_url = pg.url("postgres", db)
        state = work / "state"
        (state / "engine").mkdir(parents=True)
        state.chmod(0o700)
        (state / "engine").chmod(0o700)
        (state / "engine/docs.json").write_text(json.dumps({"version": "oo.docs/v1", "sync": {"fs_poll_seconds": 3}}))
        drive = work / "drive"
        old = time.time() - 30 * 86400  # the files are a month old: a new owner starts later than the old one
        for rel, text in DRIVE_FILES.items():
            p = drive / rel
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(text, encoding="utf-8")
            os.utime(p, (old, old))
        drive_before = l2.tree_hashes(drive)
        tokfile = work / "install.token"
        tokfile.write_text("oodi_install_only_0123456789abcdef")
        tokfile.chmod(0o600)
        env_base = {"PATH": os.environ.get("PATH", ""), "TMPDIR": os.environ.get("TMPDIR", "/tmp")}
        inst_env = dict(env_base, OO_DOCS_LISTEN="127.0.0.1:0", OO_DOCS_STATE_DIR=str(work / "install-state"),
                        OO_DOCS_DATABASE_URL=admin_url, OO_DOCS_ADMIN_DATABASE_URL=admin_url, OO_DOCS_ENGINE_TOKEN_FILE=str(tokfile))
        (work / "install-state").mkdir()
        inst = subprocess.Popen([str(engine)], env=inst_env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        first = inst.stdout.readline()
        inst.send_signal(signal.SIGTERM)
        inst.wait(timeout=30)
        ev.check("schema installed (docs-suggest/1 among the migrations)", '"listening"' in first and
                 pg.psql(db, "SELECT count(*) FROM _oo_docs.schema_migrations WHERE version = 'docs-suggest/1'") == "1", first.strip()[:60])
        pg.psql("postgres", f"CREATE ROLE {login} LOGIN IN ROLE oo_docs_runtime")
        serve = l2.Serve(ev, oo, engine, state, dict(env_base, OO_DOCS_DATABASE_URL=pg.url(login, db)))
        api = serve.start()
        st, d = api.request("POST", "/v1/docs/drives", {"name": "l5", "backend": "fs", "config": {"root": str(drive)}})
        ev.check("drive registered", st == 201, f"HTTP {st}")
        did = d["id"]
        done = l2.wait_until(120, lambda: (lambda s: len(s) == 3 and all(v == "published" for v in s.values()))(l2.file_states(api, did)), step=1)
        ev.check("the three files are published", bool(done))
        oo_args = lambda *rest: [str(oo), *rest, "--state-dir", str(state)]
        human_ops = []

        def human(label):
            human_ops.append(label)

        # ------------------------------------------------------------ A1: the six steps
        ev.log("\n## A1 section 4 of the idea document")
        st, r = api.request("POST", "/v1/docs/resolve", {"text": "星火计划 林川 小王", "drive_id": did})
        ev.check("step 1 resolve answers the project and the people", st == 200 and len(r.get("mentions", [])) >= 2, f"HTTP {st}")
        ticket = work / "ticket-123"
        ticket.mkdir()
        p = l2.run(ev, oo_args("context", "mount", "--scope", "Projects", "--scope", "People", "--to", str(ticket), "--role", "suggester", "--for", "星火计划 负责人", "--json"), check=False)
        ev.check("step 2 oo context mount --role suggester", p.returncode == 0, f"exit {p.returncode}")
        ctx = ticket / ".context"
        brief = (ctx / "BRIEF.md").read_text(encoding="utf-8")
        mount = json.loads(p.stdout.strip().splitlines()[-1])
        sid1 = mount["share_id"]
        ev.check("step 3 BRIEF.md names 林川 as the owner, every line sourced", "林川" in brief and src_tag(brief, "林川") is not None)
        sdir = ctx / "suggest"
        ev.check("the suggester mount has a writable suggest/ with README.md", (sdir / "README.md").is_file() and os.access(sdir, os.W_OK) and oct(os.stat(sdir).st_mode & 0o777) == "0o755", oct(os.stat(sdir).st_mode & 0o777))
        ev.check("everything else in the mount is read only", not os.access(ctx / "BRIEF.md", os.W_OK) and not os.access(ctx, os.W_OK))
        owner_tag, status_tag = src_tag(brief, "林川"), src_tag(brief, "进行中")
        today = time.strftime("%Y-%m-%d", time.gmtime())
        records = [
            {"protocol": "oo.suggest/v1", "kind": "supersede", "subject": "小王", "predicate": "owns", "object": "星火计划", "valid_from": today,
             "supersedes": owner_tag, "rationale": "对话里说明小王接手", "evidence": [{"kind": "brief_line", "ref": owner_tag}], "idempotency_key": "l5-owner-0001"},
            {"protocol": "oo.suggest/v1", "kind": "supersede", "subject": "星火计划", "predicate": "attr.status", "value": "跟进中", "valid_from": today,
             "supersedes": status_tag, "rationale": "小王开始跟进", "evidence": [{"kind": "brief_line", "ref": status_tag}], "idempotency_key": "l5-status-0001"},
        ]
        (sdir / "项目A.json").write_text(json.dumps(records, ensure_ascii=False))
        p = l2.run(ev, oo_args("context", "collect", str(ticket), "--json"), check=False)
        col = json.loads(p.stdout.strip().splitlines()[-1]) if p.returncode == 0 else {}
        ev.check("step 4 oo context collect sends the file and renames it", p.returncode == 0 and col.get("collected") == 1 and (sdir / "项目A.json.collected").exists() and not (sdir / "项目A.json").exists(), json.dumps({k: col.get(k) for k in ("collected", "rejected", "skipped")}))
        st, pend = api.request("GET", "/v1/docs/suggestions?status=pending")
        items = pend.get("items", [])
        ev.check("step 5 the panel holds both revisions, both superseding a fact the Share used", len(items) == 2 and all(i["rule"] == "supersede_in_use" for i in items), json.dumps([i["rule"] for i in items]))
        ev.check("each lists the affected Share", all(any(s["share_id"] == sid1 for s in api.request("GET", f"/v1/docs/suggestions/{i['id']}")[1]["impact"]["shares"]) for i in items))
        st, rv = api.request("GET", "/v1/docs/review?kind=suggestion&status=pending")
        ev.check("the same two are suggestion items in the review queue", st == 200 and len(rv.get("items", [])) == 2, f"HTTP {st}")
        st, dr = api.request("GET", f"/v1/docs/suggestions/{items[0]['id']}/diff")
        ev.check("the file-level diff draft names the file and is not writable for a suggester", st == 200 and dr.get("writable") is False and dr["changes"] and dr["changes"][0]["path"] == "Projects/星火计划.md", f"HTTP {st}")
        human("accept the owner revision (API)")
        t0 = time.time()
        st, acc = api.request("POST", f"/v1/docs/suggestions/{items[0]['id']}:decide", {"decision": "accept"})
        t_api = time.time() - t0
        ev.timings["a1_accept_api_seconds"] = round(t_api, 3)
        ev.check(f"step 5 accept takes at most {ACCEPT_SECONDS:.0f} s (API)", st == 200 and acc["status"] == "accepted" and t_api <= ACCEPT_SECONDS, f"{t_api:.3f}s")
        human("accept the status revision (CLI)")
        t0 = time.time()
        p = l2.run(ev, oo_args("docs", "suggestion", "accept", items[1]["id"], "--json"), check=False)
        t_cli = time.time() - t0
        ev.timings["a1_accept_cli_seconds"] = round(t_cli, 3)
        ev.check(f"step 5 accept takes at most {ACCEPT_SECONDS:.0f} s (CLI, process start included)", p.returncode == 0 and t_cli <= ACCEPT_SECONDS, f"{t_cli:.3f}s")
        # the old Share is told, the new BRIEF has the facts
        updates = ctx / "UPDATES.md"
        told = l2.wait_until(30, lambda: updates.exists() and "建议已被接受" in updates.read_text(encoding="utf-8"), step=1)
        ev.check("the old Share's UPDATES.md has the broadcast entry", bool(told))
        chk = l2.run(ev, oo_args("context", "check", "--dir", str(ctx), "--json"), check=False)
        ev.check("oo context check exits 10 (refresh recommended) or 20 for the old Share", chk.returncode in (10, 20), f"exit {chk.returncode}")
        ticket2 = work / "ticket-124"
        ticket2.mkdir()

        def new_brief():
            q = l2.run(ev, oo_args("context", "mount", "--scope", "Projects", "--scope", "People", "--to", str(ticket2), "--for", "星火计划 负责人", "--json"), check=False)
            if q.returncode != 0:
                return None
            b = (ticket2 / ".context/BRIEF.md").read_text(encoding="utf-8")
            if "小王" in b and "跟进中" in b:
                return b
            l2.remove_tree(ticket2 / ".context")
            return None

        b2 = l2.wait_until(180, new_brief, step=5)
        ev.check("step 6 next week's Brief: the owner is 小王 and the status 跟进中", bool(b2))
        ev.write("a1-BRIEF-before.md", brief)
        ev.write("a1-BRIEF-after.md", b2 or "")
        ev.write("a1-UPDATES.md", updates.read_text(encoding="utf-8") if updates.exists() else "")

        # ------------------------------------------------------------ A2: the four rules, through the share credential
        ev.log("\n## A2 the four impact rules")
        sj = json.loads((ctx / "SHARE.json").read_text())
        cred = sj["refs"]["credential"]
        tag = src_tag(b2 or brief, "星火计划") or "[src:Projects/星火计划.md]"
        ticket3 = work / "ticket-125"
        ticket3.mkdir()
        p3 = l2.run(ev, oo_args("context", "mount", "--scope", "Projects", "--scope", "People", "--to", str(ticket3), "--role", "suggester", "--json"), check=False)
        sid3 = json.loads(p3.stdout.strip().splitlines()[-1])["share_id"] if p3.returncode == 0 else None
        ev.check("a second suggester Share for the rule cases", bool(sid3))
        sdir3 = ticket3 / ".context/suggest"

        def put(name, rec):
            (sdir3 / name).write_text(json.dumps(rec, ensure_ascii=False))
            l2.run(ev, oo_args("context", "collect", str(ticket3), "--json"), check=False)

        ev_ref = [{"kind": "ref", "ref": "Projects/星火计划.md"}]
        put("r1.json", {"kind": "new_fact", "subject": "星火计划", "predicate": "attr.deadline", "value": "2027-03-31", "rationale": "排期会", "evidence": ev_ref, "idempotency_key": "l5-rule-new-01"})
        st, lst = api.request("GET", "/v1/docs/suggestions?share_id=" + sid3)
        by_key = {i["payload"].get("idempotency_key") or i["payload"]["predicate"]: i for i in lst["items"]}
        r1 = next((i for i in lst["items"] if i["payload"]["predicate"] == "attr.deadline"), {})
        ev.check("rule 1 a new fact with evidence in scope is auto accepted", r1.get("status") == "auto_accepted" and r1.get("rule") == "new_fact", json.dumps({k: r1.get(k) for k in ("status", "rule")}))
        ev.check("rule 1 the suggestion remembers the credential that wrote it", bool(r1.get("credential_id")), "credential_id set")
        deadline_key = pg.psql(db, f"SELECT fact_key FROM _oo_docs.fact WHERE id = '{r1.get('fact_id')}'") if r1.get("fact_id") else ""
        put("r2.json", {"kind": "supersede", "subject": "星火计划", "predicate": "attr.deadline", "value": "2027-04-30", "valid_from": today,
                        "supersedes": deadline_key, "rationale": "延期", "evidence": ev_ref, "idempotency_key": "l5-rule-unused-01"})
        st, lst = api.request("GET", "/v1/docs/suggestions?share_id=" + sid3)
        r2 = next((i for i in lst["items"] if i["payload"].get("idempotency_key") == "l5-rule-unused-01"), {})
        ev.check("rule 2 superseding a fact no Share used is silent", r2.get("status") == "superseded_silently" and r2.get("rule") == "supersede_unused", json.dumps({k: r2.get(k) for k in ("status", "rule")}))
        ev.check("rule 3 (A1) superseding a used fact waited for a person and listed the Share", all(i["rule"] == "supersede_in_use" for i in items))
        put("r4.json", {"kind": "preference", "subject": "团队", "predicate": "prefers", "value": "简短汇报", "rationale": "负责人偏好", "idempotency_key": "l5-rule-pref-01"})
        st, lst = api.request("GET", "/v1/docs/suggestions?share_id=" + sid3)
        r4 = next((i for i in lst["items"] if i["payload"].get("idempotency_key") == "l5-rule-pref-01"), {})
        ev.check("rule 4 a preference is always a person's", r4.get("status") == "pending" and r4.get("rule") == "manual", json.dumps({k: r4.get(k) for k in ("status", "rule")}))
        st, dg = api.request("GET", "/v1/docs/suggestions:digest")
        ev.check("the day's digest lists what is in effect and what waits", st == 200 and dg["counts"]["auto_accepted"] >= 1 and dg["counts"]["superseded_silently"] >= 1 and dg["counts"]["pending"] >= 1, json.dumps(dg.get("counts")))
        ev.write("a2-digest.json", json.dumps(dg, ensure_ascii=False, indent=2) + "\n")
        # replaying the same file changes nothing
        before_n = len(api.request("GET", "/v1/docs/suggestions?limit=1000")[1]["items"])
        put("r1b.json", {"kind": "new_fact", "subject": "星火计划", "predicate": "attr.deadline", "value": "2027-03-31", "rationale": "排期会", "evidence": ev_ref, "idempotency_key": "l5-rule-new-01"})
        after_n = len(api.request("GET", "/v1/docs/suggestions?limit=1000")[1]["items"])
        ev.check("resending the same idempotency_key adds nothing", before_n == after_n, f"{before_n} -> {after_n}")

        # ------------------------------------------------------------ A3: 50 suggestions, at most 5 human operations
        ev.log("\n## A3 a day of 50 suggestions")
        human_ops.clear()
        day = []
        for i in range(40):
            day.append({"kind": "new_fact", "subject": "星火计划", "predicate": f"attr.note_{i:02d}", "value": f"备注{i}", "rationale": "日常观察",
                        "evidence": ev_ref, "idempotency_key": f"l5-day-new-{i:02d}"})
        for i in range(5):
            day.append({"kind": "preference", "subject": "团队", "predicate": "prefers", "value": f"偏好{i}", "rationale": "说过", "idempotency_key": f"l5-day-pref-{i:02d}"})
        for i in range(5):
            day.append({"kind": "type_change", "subject": f"合同{i}", "predicate": "type", "value": "Document", "rationale": "归类", "idempotency_key": f"l5-day-type-{i:02d}"})
        (sdir3 / "day.json").write_text(json.dumps(day[:50], ensure_ascii=False)[:60000])
        # the file limit is 50 records and 64 KiB: one file carries the day
        l2.run(ev, oo_args("context", "collect", str(ticket3), "--json"), check=False)
        st, dg = api.request("GET", "/v1/docs/suggestions:digest")
        waiting = dg["counts"]["pending"]
        auto = dg["counts"]["auto_accepted"]
        ev.check("50 suggestions came in, the rules took the 40 with evidence", auto >= 41, f"auto accepted {auto}, waiting {waiting}")
        human("read the digest")
        st, pend = api.request("GET", "/v1/docs/review?kind=suggestion&status=pending&limit=500")
        entries = [{"id": i["id"], "revision": i["revision"], "decision": "accept"} for i in pend["items"]]
        human("accept everything that waits in one review:decide batch")
        t0 = time.time()
        st, res = api.request("POST", "/v1/docs/review:decide", {"items": entries})
        ev.timings["a3_batch_accept_seconds"] = round(time.time() - t0, 3)
        ev.check("one batch accepts every waiting suggestion", st == 200 and res.get("accepted") == len(entries) and res.get("rejected") == 0, f"{res.get('accepted')}/{len(entries)}")
        st, dg = api.request("GET", "/v1/docs/suggestions:digest")
        ev.check("nothing waits afterwards", dg["counts"]["pending"] == 0, json.dumps(dg["counts"]))
        ev.check(f"human operations for the day: {len(human_ops)} (limit {HUMAN_OPERATION_LIMIT})", len(human_ops) <= HUMAN_OPERATION_LIMIT, "; ".join(human_ops))
        ev.write("a3-human-operations.json", json.dumps({"operations": human_ops, "limit": HUMAN_OPERATION_LIMIT, "suggestions": 50}, ensure_ascii=False, indent=2) + "\n")

        # ------------------------------------------------------------ A4: traceability
        ev.log("\n## A4 traceability")
        l2.wait_until(90, lambda: int(pg.psql(db, "SELECT count(*) FROM _oo_docs.fact WHERE origin = 'suggest' AND status = 'published'") or 0) >= 40, step=2)
        n_pub = int(pg.psql(db, "SELECT count(*) FROM _oo_docs.fact WHERE origin = 'suggest' AND status = 'published'") or 0)
        ev.check("the accepted suggestions are published facts with origin suggest", n_pub >= 40, f"{n_pub} published")
        trace = pg.psql(db, "SELECT count(*) FROM _oo_docs.suggestion s JOIN _oo_docs.fact f ON f.id = s.fact_id JOIN _oo_docs.refs_credential c ON c.id = s.credential_id "
                            "WHERE s.status IN ('auto_accepted','accepted') AND EXISTS (SELECT 1 FROM _oo_docs.evidence e WHERE e.target_kind = 'fact' AND e.target_id = f.id AND e.origin = 'suggest')")
        total_acc = pg.psql(db, "SELECT count(*) FROM _oo_docs.suggestion WHERE status IN ('auto_accepted','accepted') AND fact_id IS NOT NULL")
        ev.check("every accepted fact leads back to its suggestion, the credential that wrote it and its evidence block", trace == total_acc and int(total_acc) >= 40, f"{trace}/{total_acc}")
        ov = pg.psql(db, "SELECT count(*) FROM _oo_docs.overlay WHERE kind = 'fact_assert' AND payload ? 'suggestion_id'")
        ev.check("each is asserted by an overlay row naming the suggestion", int(ov) >= 40, f"{ov} overlays")
        ev.write("a4-trace-sample.txt", pg.psql(db, "SELECT s.id, s.status, s.rule, s.decided_by, c.id AS credential, f.fact_key FROM _oo_docs.suggestion s JOIN _oo_docs.fact f ON f.id = s.fact_id LEFT JOIN _oo_docs.refs_credential c ON c.id = s.credential_id ORDER BY s.created_at LIMIT 5") + "\n")

        # ------------------------------------------------------------ A5: the Drive is untouched
        ev.check("A5 OO wrote nothing into the Drive", l2.tree_hashes(drive) == drive_before, f"{len(drive_before)} files")
        ev.write("l5-result.json", json.dumps({"run_id": a.run_id, "checks": ev.checks, "timings": ev.timings}, ensure_ascii=False, indent=2) + "\n")
        ev.write("l5-log.txt", "\n".join(ev.log_lines) + "\n")
        return 0 if all(c["ok"] for c in ev.checks) else 1
    finally:
        if serve:
            serve.stop()
        if not a.keep:
            pg.stop()
            l2.remove_tree(work)


if __name__ == "__main__":
    sys.exit(main())
