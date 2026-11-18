#!/usr/bin/env python3
"""Super Ontology L4 end to end through the `oo` CLI and the control API
(30 section L4 acceptance 2-5, 7-12, phase one of D80).

Runs the real stack like the L2 script: a labelled Postgres container, the real
`oo-docs-engine` under the real `oo serve`, and the real `oo context` / `oo docs`
commands over a Drive of the synthetic golden files (md, txt, csv; the red-team
files are in the Drive too). docx, pptx, pdf and xlsx need the parser sidecar,
which the host-process engine does not route to yet, so they are not in this Drive.

  A2   red team: packs of scopes without the red-team files hold no canary, no
       red-team-only name or alias (acceptance 2, the CI gate also runs in Rust)
  A3   provenance: every BRIEF content line has [src:, each tag names a file of
       the scope and resolves to a block anchor (acceptance 3)
  A4   determinism: two mounts, same parameters, byte identical but SHARE.json
  A5   (the p95 bench is `cargo test --test bench_compile`; its numbers are
       copied into the result when the file `bench.json` is next to the evidence)
  A7   expiry and revocation: credential dead, mount deleted by the daemon,
       `oo context list` shows both states
  A9   drift: a file that changes a decision -> the share is `drifted` within
       5 s of the new version, refresh gives a new active share, the old one is
       `refreshed`
  A10  rule-certain supersession runs by itself and the queue has one prompt for
       the share that used the old fact
  A11  coverage and --require-complete; `check` exit codes 0, 20, 30, 40;
       task group atomic refresh; a person's correction reaches the share
  A12  P21: two shares of one group, a used decision marked wrong, UPDATES.md
       block within 5 s, `oo context check` = 20, group refresh, check = 0,
       the Drive's files unchanged

Usage:  tools/oo-test/super_ontology_l4.py [--run-id ID] [--evidence-dir DIR]
        [--keep] [--skip-build]
"""

import argparse
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.dont_write_bytecode = True
import super_ontology_l2 as l2  # noqa: E402
from super_ontology_l2 import (  # noqa: E402
    Evidence, Postgres, Serve, content_lines, remove_tree, run, sha256, strip_share,
    tree_hashes, wait_until,
)

REPO = l2.REPO
GOLDEN = REPO / "testing/super-ontology/golden"
l2.LANE = "oo.lane=" + os.environ.get("OO_TEST_LANE", "super-ontology-be4")
LOCAL_EXT = {".md", ".txt", ".csv"}
DECISION_OLD = "Decisions/D-2026-05-星图改用开源图表库.md"
SUPERSEDER = "Decisions/D-2026-09-恢复自研图表引擎.md"
SUPERSEDER_TEXT = (
    "---\ntype: decision\nstatus: active\ndate: 2026-09-10\nowner: 林远舟\n---\n\n"
    "# 决定：星图看板恢复自研图表引擎\n\n2026年9月10日评审后，林远舟决定星图看板恢复自研图表引擎。\n\n"
    "本决定取代“星图看板改用开源图表库”，开源方案作废。\n"
)


def local_files(root):
    return sorted(p for p in root.rglob("*") if p.is_file() and p.suffix in LOCAL_EXT)


def build_drive(drive: Path):
    n = 0
    for base in (GOLDEN / "docs", GOLDEN / "redteam/docs"):
        for p in local_files(base):
            dest = drive / p.relative_to(base)
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(p, dest)
            n += 1
    return n


def forbidden_words():
    words = [l.strip() for l in (GOLDEN / "canaries.txt").read_text().splitlines() if l.strip()]
    for line in (GOLDEN / "redteam/redteam-entities.jsonl").read_text().splitlines():
        v = json.loads(line)
        if v.get("redteam_only"):
            words.append(v["name"])
            words += [a for a in v.get("aliases", []) if len(a) >= 2]
    return words


def scopes(drive_paths):
    doc = json.loads((GOLDEN / "scopes.json").read_text())
    top = sorted({p.split("/")[0] for p in drive_paths if p.split("/")[0] != "Confidential"})
    out = []
    for s in doc["scopes"]:
        if s["contains_redteam"]:
            continue
        pref = set()
        for inc in s["include"]:
            inc = inc.removeprefix("docs/")
            if inc == "**":
                pref |= set(top)
            elif inc.endswith("/**"):
                pref.add(inc[:-3])
            else:
                pref.add(inc)
        pref = [p for p in sorted(pref) if any(f == p or f.startswith(p + "/") for f in drive_paths)]
        if pref:
            out.append((s["id"], pref))
    return out


class Oo:
    """The `oo` binary against one state directory."""

    def __init__(self, ev, oo, state, env):
        self.ev, self.oo, self.state, self.env = ev, str(oo), str(state), env

    def run(self, *args, check=False, quiet=False, timeout=120):
        argv = [self.oo, *args, "--state-dir", self.state]
        p = subprocess.run(argv, env=self.env, capture_output=True, text=True, timeout=timeout)
        if not quiet:
            shown = " ".join(a if "/tmp" not in a else "<tmp>/" + os.path.basename(a) for a in args)
            self.ev.log(f"$ oo {shown}  -> exit {p.returncode}")
        if check and p.returncode != 0:
            raise SystemExit(f"oo {' '.join(args)} failed ({p.returncode}): {p.stderr.strip()}")
        return p

    def json(self, *args, **kw):
        p = self.run(*args, "--json", **kw)
        try:
            if not p.stdout.strip():
                return p.returncode, {"err": p.stderr.strip()[:300]}
            return p.returncode, json.loads(p.stdout.strip().splitlines()[-1])
        except ValueError:
            return p.returncode, {"raw": p.stdout, "err": p.stderr}


def frag_of(anchor):
    if isinstance(anchor, dict) and "line_start" in anchor:
        return f"#L{anchor['line_start']}-L{anchor['line_end']}"
    return None


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--run-id", default=time.strftime("%Y%m%d-%H%M%S"))
    ap.add_argument("--evidence-dir", default=str(REPO / "testing/super-ontology/evidence/L4"))
    ap.add_argument("--keep", action="store_true")
    ap.add_argument("--skip-build", action="store_true")
    args = ap.parse_args()

    ev = Evidence(Path(args.evidence_dir), args.run_id)
    target = Path(os.environ.get("CARGO_TARGET_DIR", REPO / "target")) / "debug"
    oo_bin, engine = target / "oo", target / "oo-docs-engine"
    work = Path(tempfile.mkdtemp(prefix=f"oo-l4-{args.run_id}-", dir="/tmp"))
    ev.log(f"# Super Ontology L4 end to end, run {args.run_id}")
    pg = Postgres(ev, args.run_id)
    pg.name = f"oo-l4-pg-{args.run_id}"
    serve = None
    ok = False
    try:
        if not args.skip_build:
            run(ev, ["cargo", "build", "--locked", "-p", "oo-cli", "--features", "docs"])
            run(ev, ["cargo", "build", "--locked", "-p", "oo-docs", "--bin", "oo-docs-engine"])
        pg.start()
        db, login = "oo_docs_l4", "oo_docs_l4_runtime"
        pg.psql("postgres", f"CREATE DATABASE {db}")
        admin_url = pg.url("postgres", db)
        state = work / "state"
        (state / "engine").mkdir(parents=True)
        state.chmod(0o700)
        (state / "engine").chmod(0o700)
        (state / "engine/docs.json").write_text(json.dumps({"version": "oo.docs/v1", "sync": {"fs_poll_seconds": 2}}))
        drive = work / "drive"
        n_files = build_drive(drive)
        n_csv = len([p for p in drive.rglob("*.csv")])
        ev.check("drive has the golden md/txt/csv files and the red-team md/txt/csv files", n_files >= 25 and n_csv >= 3, f"{n_files} files, {n_csv} csv")
        tokfile = work / "install.token"
        tokfile.write_text("oodi_install_only_0123456789abcdef")
        tokfile.chmod(0o600)
        env_base = {"PATH": os.environ.get("PATH", ""), "TMPDIR": os.environ.get("TMPDIR", "/tmp")}
        inst_env = dict(env_base, OO_DOCS_LISTEN="127.0.0.1:0", OO_DOCS_STATE_DIR=str(work / "install-state"),
                        OO_DOCS_DATABASE_URL=admin_url, OO_DOCS_ADMIN_DATABASE_URL=admin_url,
                        OO_DOCS_ENGINE_TOKEN_FILE=str(tokfile))
        (work / "install-state").mkdir()
        inst = subprocess.Popen([str(engine)], env=inst_env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        first = inst.stdout.readline()
        inst.send_signal(signal.SIGTERM)
        inst.wait(timeout=30)
        ev.check("schema installed by the engine installer (incl. 0145)", '"listening"' in first)
        pg.psql(db, f"CREATE ROLE {login} LOGIN IN ROLE oo_docs_runtime")
        ev.check("0145 share tables exist", pg.psql(db, "SELECT count(*) FROM pg_tables WHERE schemaname='_oo_docs' AND tablename IN ('share_group','share_update','share_section_use')") == "3")
        env = dict(env_base, OO_DOCS_DATABASE_URL=pg.url(login, db))
        serve = Serve(ev, oo_bin, engine, state, env)
        api = serve.start()
        cli = Oo(ev, oo_bin, state, env_base)

        def psql(sql):
            return pg.psql(db, sql)

        # ---------------------------------------------------------- the Drive
        ev.log("\n## Drive: `oo docs drive add`, every file published")
        rc, d = cli.json("docs", "drive", "add", "--name", "l4-drive", "--root", str(drive))
        ev.check("oo docs drive add exits 0 and returns the drive", rc == 0 and d.get("id"), f"exit {rc}")
        drive_id = d["id"]
        total = n_files

        def all_published():
            st = l2.file_states(api, drive_id)
            # csv is not routed by the host-process engine's stages yet (lane B): `unsupported`.
            done = all(s == "published" or (p.endswith(".csv") and s in ("unsupported", "published")) for p, s in st.items())
            return len(st) == total and done and st
        t0 = time.time()
        states = wait_until(180, all_published, step=0.5)
        counts = {s: list(l2.file_states(api, drive_id).values()).count(s) for s in set(l2.file_states(api, drive_id).values())}
        ev.check("every md and txt file is published (csv is unsupported until lane B routes it)", bool(states), f"{time.time() - t0:.1f}s {json.dumps(counts, sort_keys=True)}")
        paths = sorted(l2.file_states(api, drive_id))
        rc, files = cli.json("docs", "files", "ls", "--drive", "l4-drive")
        ev.check("oo docs files ls lists the files", rc == 0 and len(files.get("files", [])) == total)
        rc, drv = cli.json("docs", "drive", "show", "l4-drive")
        ev.check("Drive.drive_rev and freshness.covers_drive_rev are in the Drive object", rc == 0 and isinstance(drv.get("drive_rev"), int) and "covers_drive_rev" in drv.get("freshness", {}), json.dumps({"drive_rev": drv.get("drive_rev"), "covers": drv.get("freshness", {}).get("covers_drive_rev")}))
        rc, hlth = cli.json("docs", "health")
        ev.check("oo docs health answers", rc == 0 and hlth.get("status") in ("ok", "degraded", "ready"), f"exit {rc} {json.dumps(hlth, ensure_ascii=False)[:200]}")
        rc, usage = cli.json("docs", "usage")
        ev.check("oo docs usage answers", rc == 0)
        all_scopes = scopes(paths)

        # ------------------------------------------------------------ A2 red team
        ev.log("\n## A2: red team, no canary and no red-team-only name in any pack of another scope")
        words = forbidden_words()
        seen = 0
        hits = []
        for i, (sid, pref) in enumerate(all_scopes):
            to = work / f"rt{i}"
            to.mkdir()
            args_ = ["context", "mount", "--to", str(to), "--for", "星图看板的决定和负责人"]
            for p in pref:
                args_ += ["--scope", p]
            rc, m = cli.json(*args_, quiet=True)
            if rc == 5:
                continue
            ev.check(f"A2 mount of scope {sid} exits 0", rc == 0, f"exit {rc}")
            for f in sorted((to / ".context").rglob("*")):
                if f.is_file():
                    text = f.read_text(encoding="utf-8", errors="replace").lower()
                    seen += 1
                    hits += [(sid, str(f.relative_to(to)), w) for w in words if w.lower() in text]
            cli.run("context", "revoke", str(to), quiet=True)
        ev.check("A2 zero red-team hits in every file of every pack", not hits and seen > 50, f"{seen} files scanned over {len(all_scopes)} scopes, {len(hits)} hits")
        ev.write("a2-redteam.json", json.dumps({"scopes": [s for s, _ in all_scopes], "files_scanned": seen, "hits": len(hits), "forbidden_words": len(words)}, indent=2) + "\n")

        # -------------------------------------------------- A3 provenance, A4 determinism
        ev.log("\n## A3 provenance, A4 determinism")
        main_scope = ["Decisions", "Projects", "People"]
        mounts = {}
        for name in ("m1", "m2"):
            to = work / name
            to.mkdir()
            a = ["context", "mount", "--to", str(to), "--for", "星图看板的决定", "--budget", "1200"]
            for s in main_scope:
                a += ["--scope", s]
            rc, m = cli.json(*a)
            ev.check(f"A4 mount {name} exits 0", rc == 0, f"exit {rc}")
            mounts[name] = (to, m)
        t1, m1 = mounts["m1"]
        t2, m2 = mounts["m2"]
        h1, h2 = tree_hashes(t1 / ".context"), tree_hashes(t2 / ".context")
        diff = [p for p in h1 if p != "SHARE.json" and h1[p] != h2.get(p)]
        ev.check("A4 same file set and every file but SHARE.json byte identical", sorted(h1) == sorted(h2) and not diff, f"{len(h1)} files" + (f"; differ {diff}" if diff else ""))
        ev.check("A4 SHARE.json equal without share id, times and refs", strip_share((t1 / ".context/SHARE.json").read_bytes()) == strip_share((t2 / ".context/SHARE.json").read_bytes()))
        ev.check("A4 pack_digest equal", m1["pack_digest"] == m2["pack_digest"])
        # AIGC: label first on every Markdown file, front matter, not removable.
        md = [p for p in (t1 / ".context").rglob("*.md")]
        label = (t1 / ".context/BRIEF.md").read_text().splitlines()[0]
        bad_aigc = [str(p) for p in md if not (p.read_text().splitlines()[0] == label and "\nAIGC:\n  Label: \"1\"" in p.read_text()[:400])]
        ev.check("D66 every generated .md starts with the AI label and carries AIGC front matter", md and not bad_aigc, f"{len(md)} files")
        brief = (t1 / ".context/BRIEF.md").read_text()
        lines = content_lines(brief)
        ev.check("A3 BRIEF content lines all have [src:", lines and all("[src:" in l for l in lines), f"{len(lines)} lines")
        ev.check("A3 BRIEF header names coverage and the five promises", "> 覆盖：" in brief and "一致性承诺：" in brief)
        # every tag of every checked file resolves
        st, listing = api.request("GET", f"/v1/docs/drives/{drive_id}/files?limit=500")
        file_ids = {f["path"]: f["id"] for f in listing["items"]}
        block_cache = {}

        def blocks_of(path):
            if path not in block_cache:
                rows, cursor = [], None
                while True:
                    q = f"/v1/docs/files/{file_ids[path]}/blocks?limit=1000" + (f"&cursor={cursor}" if cursor else "")
                    st, b = api.request("GET", q)
                    rows += b.get("items", [])
                    cursor = b.get("next_cursor")
                    if not cursor:
                        break
                block_cache[path] = rows
            return block_cache[path]

        from urllib.parse import unquote
        checked = unresolved = outside = 0
        for f in ["BRIEF.md"] + [str(p.relative_to(t1 / ".context")) for p in (t1 / ".context").rglob("*.md") if str(p.relative_to(t1 / ".context")).split("/")[0] in ("people", "projects") or p.name in ("decisions.md", "terms.md")]:
            for line in (t1 / ".context" / f).read_text().splitlines():
                if not line.startswith("- "):
                    continue
                for tag in re.findall(r"\[src:([^\]]*)\]", line):
                    if tag.startswith("+"):
                        continue
                    path, _, fr = tag.partition("#")
                    path = unquote(path)
                    fr = "#" + fr
                    checked += 1
                    if not any(path == s or path.startswith(s + "/") for s in main_scope):
                        outside += 1
                    if path not in file_ids:
                        unresolved += 1
                        continue
                    anchors = [frag_of(b.get("anchor")) for b in blocks_of(path)]
                    if fr.startswith("#L") and fr not in anchors:
                        unresolved += 1
        ev.check("A3 every [src:] names a file of the scope and resolves to a block anchor of that file", checked > 20 and not unresolved and not outside, f"{checked} tags, {unresolved} unresolved, {outside} outside the scope")
        ev.write("a3-BRIEF.md", brief)
        bench = Path(args.evidence_dir) / "bench.json"
        if bench.is_file():
            b = json.loads(bench.read_text())
            ev.check("A5 compile p95 under 2 s on a 1000 file Drive, budget 800 (bench_compile)", b["p95_s"] < 2.0, f"p95 {b['p95_s']:.3f}s ({b['build']})")

        # ---------------------------------------------------------------- A7
        ev.log("\n## A7: expiry and revocation")
        dec_b3 = psql("SELECT fv.b3 FROM _oo_docs.file_version fv WHERE fv.path_at_version LIKE 'Decisions/D-2026-02%' AND fv.superseded_at IS NULL LIMIT 1")

        def share_json(d):
            return json.loads((d / ".context/SHARE.json").read_text())

        def cred_status(sj, b3):
            import http.client
            c = http.client.HTTPConnection("127.0.0.1", api.port, timeout=10)
            c.request("GET", f"/v1/docs/refs/{b3}", headers={"Authorization": f"Bearer {sj['refs']['credential']}"})
            r = c.getresponse()
            r.read()
            c.close()
            return r.status
        te, tr = work / "exp", work / "rev"
        for t in (te, tr):
            t.mkdir()
            rc, _ = cli.json("context", "mount", "--to", str(t), "--scope", "Decisions", quiet=True)
            assert rc == 0
        sje, sjr = share_json(te), share_json(tr)
        keep = work / "keep-exp"
        (keep / ".context").mkdir(parents=True)
        shutil.copy(te / ".context/SHARE.json", keep / ".context/SHARE.json")
        ev.check("A7 both credentials read a cited original while active", cred_status(sje, dec_b3) == 200 and cred_status(sjr, dec_b3) == 200)
        rc, rv = cli.json("context", "revoke", str(tr))
        ev.check("A7 revoke deletes the local mount and says so", rc == 0 and rv.get("local_removed") is True and not (tr / ".context").exists(), json.dumps(rv))
        ev.check("A7 a revoked share's credential is dead (410)", cred_status(sjr, dec_b3) == 410)
        psql(f"UPDATE _oo_docs.share SET expires_at = clock_timestamp() - interval '1 second' WHERE id = '{sje['share_id']}'")
        t_exp = time.time()
        gone = wait_until(15, lambda: not (te / ".context").exists(), step=0.2)
        ev.check("A7 the daemon deleted the expired share's mount", bool(gone), f"{time.time() - t_exp:.1f}s")
        ev.check("A7 an expired share's credential is dead (410)", cred_status(sje, dec_b3) == 410)
        rc, lst = cli.json("context", "list")
        sts = {s["id"]: s["state"] for s in lst.get("shares", [])}
        ev.check("A7 the list shows expired and revoked correctly", sts.get(sje["share_id"]) == "expired" and sts.get(sjr["share_id"]) == "revoked", json.dumps({k[:8]: v for k, v in sts.items() if v in ("expired", "revoked")}))
        p = cli.run("context", "check", "--dir", str(keep / ".context"))
        ev.check("A7/A11 check on an expired share exits 30", p.returncode == 30, f"exit {p.returncode}")

        # ---------------------------------------------------------------- A9, A10
        ev.log("\n## A9 light drift, A10 rule-certain supersession")
        td = work / "drift"
        td.mkdir()
        rc, md_ = cli.json("context", "mount", "--to", str(td), "--scope", "Decisions", "--scope", "Projects", "--for", "星图看板用什么图表方案")
        ev.check("A9 mount exits 0", rc == 0)
        sd = share_json(td)["share_id"]
        pack_before = tree_hashes(td / ".context")
        head0 = int(psql("SELECT coalesce(max(n),0) FROM _oo_docs.ontology_version"))
        (drive / SUPERSEDER).write_text(SUPERSEDER_TEXT, encoding="utf-8")
        t_drop = time.time()
        v_new = wait_until(60, lambda: int(psql("SELECT coalesce(max(n),0) FROM _oo_docs.ontology_version")) > head0 and psql(f"SELECT status FROM _oo_docs.share WHERE id = '{sd}'") == "drifted", step=0.2)
        drift_lag = psql(f"SELECT extract(epoch FROM (s.drifted_at - v.published_at))::numeric(8,3) FROM _oo_docs.share s, _oo_docs.ontology_version v WHERE s.id = '{sd}' AND v.n = (SELECT max(n) FROM _oo_docs.ontology_version)")
        ev.check("A9 the share turned drifted with the new version", bool(v_new), f"{time.time() - t_drop:.1f}s from the drop to drifted")
        ev.check("A9 drifted_at - published_at <= 5 s (same transaction)", drift_lag != "" and abs(float(drift_lag)) <= 5.0, f"{drift_lag} s")
        ev.timings["a9_drift_after_publish_seconds"] = float(drift_lag or 0)
        rc, stx = cli.json("context", "status", str(td))
        ev.check("A9 oo context status shows drifted with updates", rc == 0 and stx.get("state") == "drifted" and stx["updates"]["count"] >= 1, json.dumps({"state": stx.get("state"), "updates": stx.get("updates")}))
        got = wait_until(10, lambda: (td / ".context/UPDATES.md").is_file(), step=0.2)
        ev.check("A9 the daemon appended UPDATES.md", bool(got))
        upd = (td / ".context/UPDATES.md").read_text() if got else ""
        ev.check("A9 UPDATES.md has the AI label first and a warn entry for the superseded decision", upd.startswith(label) and "· warn" in upd and "已被取代" in upd, upd[-200:].replace("\n", " | "))
        pack_after = tree_hashes(td / ".context")
        changed = [p for p in pack_before if p != "UPDATES.md" and pack_before[p] != pack_after.get(p)]
        ev.check("A11 the pack bytes never change except UPDATES.md", not changed, f"changed: {changed}")
        p = cli.run("context", "check", "--dir", str(td / ".context"))
        ev.check("A11 check exits 10 for a warn", p.returncode == 10, f"exit {p.returncode}")
        # A10: the supersession ran by itself, and the queue has the prompt.
        old_valid = psql("SELECT count(*) FROM _oo_docs.fact f JOIN _oo_docs.entity e ON (e.id = f.subject_id OR e.id = f.object_id) WHERE e.canonical_name LIKE '%开源图表库%' AND f.valid_to IS NOT NULL AND f.superseded_by IS NOT NULL AND f.origin = 'rule'")
        ev.check("A10 the old decision's facts were ended by the rule, without a person", int(old_valid) >= 1, f"{old_valid} facts ended")
        in_use = psql("SELECT count(*) FROM _oo_docs.review_item WHERE reason_code = 'supersede_in_use'")
        ev.check("A10 the queue has a supersede_in_use prompt (the old fact was in use by a live share)", int(in_use) >= 1, f"{in_use} items")
        rc, q = cli.json("docs", "review", "ls", "--status", "pending")
        ev.check("A10 oo docs review ls shows the prompt", rc == 0 and any(i.get("reason_code") == "supersede_in_use" for i in q.get("items", [])))
        ev.check("A10 nothing needed a person for the rule-certain supersession itself", int(psql("SELECT count(*) FROM _oo_docs.review_item WHERE reason_code = 'supersede_candidate'")) == 0)
        # refresh
        rc, rf = cli.json("context", "refresh", str(td))
        ev.check("A9 refresh gives a new active share and mounts it", rc == 0 and rf["shares"][0]["mounted"] and psql(f"SELECT status FROM _oo_docs.share WHERE id = '{sd}'") == "refreshed", json.dumps(rf)[:160])
        new_sd = share_json(td)["share_id"]
        ev.check("A9 the directory holds the new share", new_sd != sd and psql(f"SELECT status FROM _oo_docs.share WHERE id = '{new_sd}'") == "active")
        p = cli.run("context", "check", "--dir", str(td / ".context"))
        ev.check("A11 check exits 0 after the refresh", p.returncode == 0, f"exit {p.returncode}")
        ev.write("a9-UPDATES.md", upd)

        # ---------------------------------------------------------------- A11 coverage
        ev.log("\n## A11: coverage, --require-complete, check, groups, corrections")
        (drive / "Notes").mkdir(exist_ok=True)
        (drive / "Notes/刚放进去.md").write_text("# 刚放进去\n\n负责人：孙晓棠\n", encoding="utf-8")
        api.request("POST", f"/v1/docs/drives/{drive_id}:sync", {})
        tc = work / "cov"
        tc.mkdir()
        # Catch the new file while it is still in the pipeline, then ask for a complete pack.
        seen_state = wait_until(20, lambda: l2.file_states(api, drive_id).get("Notes/刚放进去.md") not in (None, "published") and l2.file_states(api, drive_id).get("Notes/刚放进去.md"), step=0.05)
        t_try = time.time()
        rc1 = cli.run("context", "mount", "--to", str(tc), "--scope", "Notes", "--require-complete", "--wait", "0s")
        waited = wait_until(30, lambda: l2.file_states(api, drive_id).get("Notes/刚放进去.md") == "published", step=0.2)
        if seen_state:
            ev.check("A11 --require-complete refuses a scope with a file still in the pipeline (exit 7)", rc1.returncode == 7, f"file was {seen_state}; exit {rc1.returncode}; {rc1.stderr.strip()[:120]}")
            ev.check("A11 nothing was mounted by the refused call", not (tc / ".context").exists())
        else:
            ev.log("  (the new file was published before the refusal could be tried; skipped)")
        rc, _ = cli.json("context", "mount", "--to", str(tc), "--scope", "Notes", "--require-complete", "--wait", "20s")
        ev.check("A11 once processed, --require-complete succeeds", rc == 0)
        covered = (tc / ".context/BRIEF.md").read_text() if rc == 0 else ""
        cov_line = [l for l in covered.splitlines() if l.startswith("> 覆盖")][:1]
        m_cov = re.match(r"> 覆盖：(\d+)/(\d+) 个文件已处理$", cov_line[0]) if cov_line else None
        ev.check("A11 BRIEF names the coverage in its header (all processed)", bool(m_cov) and m_cov.group(1) == m_cov.group(2), cov_line)
        cli.run("context", "revoke", str(tc), quiet=True)

        # check exit codes 0 / 20 / 40 and the correction path
        ev.log("\n## A11/A12: P21 (two shares of one group, a used decision marked wrong)")
        ta, tb = work / "agent-a", work / "agent-b"
        ta.mkdir(), tb.mkdir()
        rc, ma = cli.json("context", "mount", "--to", str(ta), "--scope", "Decisions", "--scope", "Projects", "--group", "TICKET-P21", "--for", "星图看板用什么图表方案", "--refresh-policy", "manual")
        rc2, mb = cli.json("context", "mount", "--to", str(tb), "--scope", "Decisions", "--scope", "People", "--group", "TICKET-P21", "--for", "林远舟的决定")
        ev.check("A12 both group members mount", rc == 0 and rc2 == 0)
        sja, sjb = share_json(ta), share_json(tb)
        ev.check("A11 same group, same ontology version", psql(f"SELECT count(DISTINCT p.version_n) FROM _oo_docs.share s JOIN _oo_docs.context_pack p ON p.id = s.pack_id WHERE s.id IN ('{sja['share_id']}', '{sjb['share_id']}')") == "1" and sja["ontology_version"] == sjb["ontology_version"])
        for t in (ta, tb):
            p = cli.run("context", "check", "--dir", str(t / ".context"))
            ev.check(f"A12 check in {t.name} is 0 before the decision is overturned", p.returncode == 0, f"exit {p.returncode}")
        # a single member must not refresh alone
        rcg = cli.run("context", "refresh", str(ta))
        ev.check("A11 a group member cannot refresh alone (exit 8)", rcg.returncode == 8, f"exit {rcg.returncode}")
        # pick a used decision fact and mark it wrong through the CLI
        row = psql(f"SELECT f.id || '|' || e.key FROM _oo_docs.share_fact_use u JOIN _oo_docs.fact f ON f.id = u.fact_id JOIN _oo_docs.entity e ON e.id = f.subject_id WHERE u.share_id = '{sja['share_id']}' AND f.predicate IN ('decides','attr.status') AND f.origin = 'rule' AND f.valid_to IS NULL AND f.retracted_in IS NULL AND e.type = 'Decision' ORDER BY f.fact_key LIMIT 1")
        fact_id, ent_key = row.split("|")
        drive_hash_before = tree_hashes(drive)
        t_mark = time.time()
        v_before = int(psql("SELECT max(n) FROM _oo_docs.ontology_version"))
        rc, cr = cli.json("docs", "entity", "correct", fact_id, "--mark-wrong", "--reason", "决定被推翻", "--entity", ent_key)
        ev.check("A12 oo docs entity correct --mark-wrong exits 0", rc == 0 and cr.get("status") == "retracted", f"exit {rc} {json.dumps(cr, ensure_ascii=False)[:200]}")
        blocked = wait_until(30, lambda: psql(f"SELECT count(*) FROM _oo_docs.share_update WHERE share_id = '{sja['share_id']}' AND level = 'block'") != "0", step=0.2)
        lag = psql(f"SELECT extract(epoch FROM (s.drifted_at - v.published_at))::numeric(8,3) FROM _oo_docs.share s, _oo_docs.ontology_version v WHERE s.id = '{sja['share_id']}' AND v.n = (SELECT max(n) FROM _oo_docs.ontology_version)")
        ev.check("A11 a block entry reached the share after the correction", bool(blocked), f"{time.time() - t_mark:.1f}s from the correction")
        ev.check("A12 drifted within 5 s of the new version", lag != "" and abs(float(lag)) <= 5.0, f"{lag} s")
        ev.timings["a12_block_after_publish_seconds"] = float(lag or 0)
        appended = wait_until(5, lambda: (ta / ".context/UPDATES.md").is_file() and "· block" in (ta / ".context/UPDATES.md").read_text(), step=0.2)
        ev.check("A12 UPDATES.md of agent-a got the block entry within 5 s of the daemon's next pass", bool(appended))
        p = cli.run("context", "check", "--dir", str(ta / ".context"))
        ev.check("A12 oo context check in agent-a exits 20", p.returncode == 20, f"exit {p.returncode}")
        pj = cli.run("context", "check", "--dir", str(ta / ".context"), "--json", quiet=True)
        cj = json.loads(pj.stdout.strip().splitlines()[-1])
        ev.check("A12 check --json is oo.context-check/v1 with level block", cj.get("protocol") == "oo.context-check/v1" and cj.get("level") == "block" and cj.get("exit_code") == 20)
        pb = cli.run("context", "check", "--dir", str(tb / ".context"))
        ev.check("A12 agent-b (did not use that decision's section) is told what its own pack used", pb.returncode in (0, 10, 20), f"exit {pb.returncode}")
        # group refresh: all or none, same version
        rcr, gr = cli.json("context", "refresh", "--group", "TICKET-P21")
        ev.check("A12 group refresh succeeds", rcr == 0 and len(gr.get("shares", [])) == 2 and all(s["mounted"] for s in gr["shares"]), json.dumps(gr)[:200])
        versions = psql("SELECT string_agg(DISTINCT p.version_n::text, ',') FROM _oo_docs.share s JOIN _oo_docs.context_pack p ON p.id = s.pack_id WHERE s.group_id IS NOT NULL AND s.status = 'active'")
        ev.check("A12 both members are on the same, newer version", "," not in versions and int(versions) > v_before, f"v{versions}")
        for t in (ta, tb):
            p = cli.run("context", "check", "--dir", str(t / ".context"))
            ev.check(f"A12 check in {t.name} is 0 after the group refresh", p.returncode == 0, f"exit {p.returncode}")
        ev.check("A12 the wrong fact is not in the new pack of agent-a", psql(f"SELECT count(*) FROM _oo_docs.share_fact_use WHERE share_id = '{share_json(ta)['share_id']}' AND fact_id = '{fact_id}'") == "0")
        ev.check("A12 the Drive's files are unchanged by the whole exercise", tree_hashes(drive) == drive_hash_before)
        # engine down: fail closed
        engine_pid = int((state / "engine.pid").read_text())
        os.kill(engine_pid, signal.SIGKILL)
        wait_until(15, lambda: api.request("GET", "/v1/docs/health")[1].get("status") == "unavailable", step=0.1)
        p = cli.run("context", "check", "--dir", str(ta / ".context"))
        ev.check("A11 check exits 40 when the engine cannot be reached (fail closed)", p.returncode == 40, f"exit {p.returncode}")
        wait_until(60, lambda: api.request("GET", "/v1/docs/health")[1].get("status") not in ("unavailable", None), step=0.5)

        # ---------------------------------------------------------------- A8 summary
        ev.log("\n## A8: the end-to-end demonstration ran above: Drive -> files published -> mount -> BRIEF with sources -> drift -> refresh -> revoke")
        rc, lst = cli.json("docs", "share", "ls")
        ev.check("A8 oo docs share ls works as an alias of oo context list", rc == 0 and len(lst.get("shares", [])) >= 5)
        for t in (ta, tb, td):
            cli.run("context", "revoke", str(t), quiet=True)
        ev.check("A8 revoke removes every remaining mount", not any((t / ".context").exists() for t in (ta, tb, td)))
        ok = all(c["ok"] for c in ev.checks)
    finally:
        if serve and serve.proc:
            serve.stop()
        if not args.keep:
            pg.stop()
            remove_tree(work)
        failed = [c for c in ev.checks if not c["ok"]]
        ev.log(f"\n# {len(ev.checks) - len(failed)}/{len(ev.checks)} checks passed" + ("" if not failed else "; FAILED: " + "; ".join(c["check"] for c in failed)))
        ev.write("run.log", "\n".join(ev.log_lines) + "\n")
        head = subprocess.run(["git", "-C", str(REPO), "rev-parse", "HEAD"], capture_output=True, text=True).stdout.strip()
        built = {b.name: sha256(b.read_bytes()) for b in (oo_bin, engine) if b.is_file()}
        ev.write("result.json", json.dumps({"run_id": args.run_id, "status": "pass" if ok and not failed else "fail", "git_head": head, "binaries_sha256": built, "postgres_image": l2.PG_IMAGE.split("/", 2)[-1], "timings": ev.timings, "checks": ev.checks}, ensure_ascii=False, indent=2) + "\n")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
