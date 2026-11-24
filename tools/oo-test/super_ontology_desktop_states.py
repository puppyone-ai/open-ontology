#!/usr/bin/env python3
"""Two review states the Desktop T1 needs from a real engine (lane I, reports/I-desktop-findings.md):
the `type_promotion` review item and the `reprocess_diff` review item, produced by the engine's own code paths.

A real `oo serve` (host engine) over an fs Drive and a Postgres container. The only stand-in is the *model*: the
engine's real extraction stage sends its real request through the real model egress to `tools/super-ontology/
fixture_model.py` (loopback), which answers deterministically from markers in the documents. Everything after the
answer (quote check, entity resolution, confidence, publish, DOC-G10, review queue) is the engine.

  T  `type_promotion`  three documents hold six `Thing/合同` markers: after the publish the engine proposes the open type
                       (10 4.8, D16; thresholds 5 instances in 3 files). Nothing promotes by itself.
  R  `reprocess_diff`  one document owns five facts. Switch the model id (a new cache key), leave four of its
                       entities out of the fixture's answers (control file) and run the one manual rebuild
                       (`POST /drives/{id}:rebuild`): the new extraction differs by more than 30% (DOC-G10), the old
                       extraction stays published and one `reprocess_diff` item waits for a person.

Usage: tools/oo-test/super_ontology_desktop_states.py [--run-id ID] [--evidence-dir DIR] [--skip-build] [--keep]
Docker: one postgres container labelled oo.lane=$OO_TEST_LANE (default super-ontology-i1).
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
LANE = os.environ.get("OO_TEST_LANE", "super-ontology-i1")

CONTRACTS = {
    f"Contracts/c{i}.md": f"# 合同{i}\n\n[[Thing/合同: 合同{i}A]]\n\n[[Thing/合同: 合同{i}B]]\n" for i in (1, 2, 3)
}
PROJECTS = ["星图", "灯塔", "知了", "看板", "引擎"]
PLAN = "# 项目分工\n\n[[Person: 林川]]\n\n" + "".join(f"[[Project: {p}]]\n\n" for p in PROJECTS) + "".join(
    f"[[owns: 林川 > {p}]]\n\n" for p in PROJECTS
)


def put_model(api, key_file, port, model):
    st, cur = api.request("GET", "/v1/docs/config/model")
    rev = cur.get("revision", 1) if isinstance(cur, dict) else 1
    return api.request("PUT", "/v1/docs/config/model", {
        "provider": "openai-compatible", "base_url": f"http://127.0.0.1:{port}", "model": model, "key_ref": f"file:{key_file}",
    }, headers={"If-Match": f'"r{rev}"'})


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--run-id", default=time.strftime("ds-%Y%m%d-%H%M%S"))
    ap.add_argument("--evidence-dir", default=str(REPO / "testing/super-ontology/evidence/L5"))
    ap.add_argument("--skip-build", action="store_true")
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()
    l2.LANE = f"oo.lane={LANE}"
    ev = l2.Evidence(Path(a.evidence_dir), a.run_id)
    target = Path(os.environ.get("CARGO_TARGET_DIR", REPO / "target")) / "debug"
    oo, engine = target / "oo", target / "oo-docs-engine"
    work = Path(tempfile.mkdtemp(prefix=f"oo-ds-{a.run_id}-", dir="/tmp"))
    pg = l2.Postgres(ev, a.run_id)
    pg.name = f"oo-i1-pg-{a.run_id}"
    serve, stub = None, None
    ok = False
    ev.log(f"# Desktop T1 states (type_promotion, reprocess_diff), run {a.run_id}")
    try:
        if not a.skip_build:
            l2.run(ev, ["cargo", "build", "--locked", "-p", "oo-cli", "--features", "docs"])
            l2.run(ev, ["cargo", "build", "--locked", "-p", "oo-docs", "--bin", "oo-docs-engine"])
        pg.start()
        db, login = "oo_docs_ds", "oo_docs_ds_runtime"
        pg.psql("postgres", f"CREATE DATABASE {db}")
        admin_url = pg.url("postgres", db)
        state, drive = work / "state", work / "drive"
        (state / "engine").mkdir(parents=True)
        state.chmod(0o700)
        (state / "engine").chmod(0o700)
        (state / "engine/docs.json").write_text(json.dumps({"version": "oo.docs/v1", "sync": {"fs_poll_seconds": 3, "settle_seconds": 0}}))
        drive.mkdir()
        tok = work / "install.token"
        tok.write_text("oodi_install_only_0123456789abcdef")
        tok.chmod(0o600)
        base = {"PATH": os.environ.get("PATH", ""), "TMPDIR": os.environ.get("TMPDIR", "/tmp")}
        (work / "install-state").mkdir()
        inst = subprocess.Popen([str(engine)], env=dict(base, OO_DOCS_LISTEN="127.0.0.1:0", OO_DOCS_STATE_DIR=str(work / "install-state"),
                                                       OO_DOCS_DATABASE_URL=admin_url, OO_DOCS_ADMIN_DATABASE_URL=admin_url,
                                                       OO_DOCS_ENGINE_TOKEN_FILE=str(tok)),
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        first = inst.stdout.readline()
        inst.send_signal(signal.SIGTERM)
        inst.wait(timeout=30)
        ev.check("schema installed", '"listening"' in first)
        pg.psql(db, f"CREATE ROLE {login} LOGIN IN ROLE oo_docs_runtime")
        # the fixture model (loopback) and its key reference
        control = work / "control.json"
        control.write_text("{}")
        stub = subprocess.Popen([sys.executable, str(REPO / "tools/super-ontology/fixture_model.py"), "--control", str(control)],
                                stdout=subprocess.PIPE, text=True)
        mport = json.loads(stub.stdout.readline())["port"]
        key_file = work / "model.key"
        key_file.write_text("fixture-key-not-a-secret")
        key_file.chmod(0o600)
        env = dict(base, OO_DOCS_DATABASE_URL=pg.url(login, db), OO_DOCS_MODEL_EGRESS=str(REPO / "tools/model-egress/model_egress.py"))
        serve = l2.Serve(ev, oo, engine, state, env)
        api = serve.start()
        st, r = put_model(api, key_file, mport, "fixture-model-1")
        ev.check("the fixture model is configured (references only)", st == 200 and r.get("status") == "ok", f"HTTP {st} {r.get('status') if isinstance(r, dict) else r}")
        st, d = api.request("POST", "/v1/docs/drives", {"name": "states", "backend": "fs", "config": {"root": str(drive)}})
        assert st == 201, d
        did = d["id"]
        st, d = api.request("PATCH", f"/v1/docs/drives/{did}", {"model_enabled": True}, headers={"If-Match": f'"r{d["revision"]}"'})
        ev.check("the Drive uses the model", st == 200 and d.get("model_enabled") is True, f"HTTP {st}")
        for rel, text in {**CONTRACTS, "Plans/分工.md": PLAN}.items():
            p = drive / rel
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(text, encoding="utf-8")
        want = len(CONTRACTS) + 1
        ready = l2.wait_until(240, lambda: (lambda s: s if len(s) == want and all(v == "published" for v in s.values()) else None)(l2.file_states(api, did)), step=2)
        ev.check("every document is published", bool(ready), json.dumps(l2.file_states(api, did), ensure_ascii=False))
        # ---- T: type_promotion
        def pending(kind):
            st, b = api.request("GET", f"/v1/docs/review?kind={kind}&status=pending&limit=50")
            return b.get("items", []) if st == 200 else []
        tp = l2.wait_until(90, lambda: pending("type_promotion"), step=2) or []
        ev.check("T one type_promotion item waits for a person", len(tp) == 1, json.dumps([i.get("subject") for i in tp], ensure_ascii=False)[:200])
        ev.check("T it names the label 合同 and nothing promoted by itself",
                 bool(tp) and "合同" in json.dumps(tp[0].get("subject", {}), ensure_ascii=False)
                 and pg.psql(db, "SELECT count(*) FROM _oo_docs.overlay WHERE kind = 'type_promote'") == "0")
        # ---- R: reprocess_diff
        plan_facts = lambda: int(pg.psql(db, "SELECT count(*) FROM _oo_docs.evidence e JOIN _oo_docs.file f ON f.current_version_id = e.version_id "
                                             "JOIN _oo_docs.fact x ON x.id = e.target_id WHERE e.target_kind = 'fact' AND e.withdrawn_at IS NULL "
                                             "AND f.path = 'Plans/分工.md' AND x.status IN ('draft','published')") or 0)
        before = plan_facts()
        ev.check("R the plan document has facts before the rebuild", before >= 5, f"{before} fact rows")
        control.write_text(json.dumps({"skip": PROJECTS[1:]}, ensure_ascii=False))
        st, cur = api.request("GET", "/v1/docs/config/model")
        st, r = put_model(api, key_file, mport, "fixture-model-2")
        ev.check("R the model id changes (a new cache key)", st == 200, f"HTTP {st}")
        st, rb = api.request("POST", f"/v1/docs/drives/{did}:rebuild", {"confirm": did})
        ev.check("R the one manual rebuild is accepted", st == 202, f"HTTP {st} {rb if st != 202 else ''}")
        rd = l2.wait_until(240, lambda: pending("reprocess_diff"), step=2) or []
        ev.check("R one reprocess_diff item waits for a person", len(rd) == 1, f"{len(rd)} items")
        st_plan = l2.file_states(api, did).get("Plans/分工.md")
        ev.check("R the old extraction stays: the file is published and its facts live", st_plan == "published" and plan_facts() >= before, f"{st_plan}, {plan_facts()} fact rows")
        if rd:
            ev.log("  reprocess_diff subject: " + json.dumps(rd[0].get("subject"), ensure_ascii=False)[:300])
        ok = all(c["ok"] for c in ev.checks)
    finally:
        if serve:
            serve.stop()
        if stub:
            stub.terminate()
        if not a.keep:
            pg.stop()
            l2.remove_tree(work)
            shutil.rmtree(work, ignore_errors=True)
    ev.log(f"\n# {sum(1 for c in ev.checks if c['ok'])}/{len(ev.checks)} checks passed")
    ev.write(f"desktop-states-{a.run_id}.md", "\n".join(ev.log_lines) + "\n")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
