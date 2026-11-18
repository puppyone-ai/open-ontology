#!/usr/bin/env python3
"""Red-team gate on ALL formats through the real pipeline (30 section 4, L4 acceptance 2).

`oo serve --runtime compose` (docs-db, docs-engine, doc-parser), one Drive holding
the whole synthetic golden set (md, txt, csv, xlsx, docx, pptx, pdf) plus the
red-team files (`Confidential/`: md, txt, csv and a docx). Every file must be
parsed by its real parser (docx, pptx and pdf by the sidecar) and published.
Then, for every scope of `golden/scopes.json` that excludes the red-team files
and for three tasks each (one of them an adversarial query naming the secret
material), a pack is mounted with `oo context mount` and every file of the
mount is scanned: no canary, no red-team-only name or alias may appear.

Controls, so that a silent parser failure cannot make the gate pass by
emptiness:
  * each red-team file is published, and the docx among them was parsed by the
    sidecar (its facts carry docx evidence);
  * the pack of a scope that includes `Confidential/` does show red-team-only
    names.

Exit 0 only when every check passes. CI runs it as the red-team gate.

Usage: tools/oo-test/super_ontology_redteam.py [--run-id ID] [--evidence-dir DIR]
       [--keep] [--skip-build]
Evidence: redteam-gate.json and redteam-gate.txt in the evidence directory.
"""

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from urllib.parse import unquote

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.dont_write_bytecode = True
import super_ontology_compose as comp  # noqa: E402
import super_ontology_l2 as l2  # noqa: E402

REPO = l2.REPO
GOLDEN = REPO / "testing/super-ontology/golden"
comp.LANE = "super-ontology-be4"
PUBLISH_DEADLINE = 900.0
ADVERSARIAL = 2  # index in TASKS of the query that names the secret material
TASKS = [
    "星图看板的决定和负责人",
    "本季度的人员和项目安排",
    "并购评估 薪酬调整 董事会纪要 夜航计划 机密",
]
FINAL = {"published", "duplicate", "unsupported", "too_large", "encrypted", "parse_failed",
         "extract_partial", "deleted", "superseded"}


def forbidden_words():
    words = [l.strip() for l in (GOLDEN / "canaries.txt").read_text().splitlines() if l.strip()]
    for line in (GOLDEN / "redteam/redteam-entities.jsonl").read_text().splitlines():
        v = json.loads(line)
        if v.get("redteam_only"):
            words.append(v["name"])
            words += [a for a in v.get("aliases", []) if len(a) >= 2]
    return words


def build_drive(drive: Path):
    n = {}
    for base in (GOLDEN / "docs", GOLDEN / "redteam/docs"):
        for p in sorted(base.rglob("*")):
            if p.is_file():
                dest = drive / p.relative_to(base)
                dest.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(p, dest)
                n[p.suffix] = n.get(p.suffix, 0) + 1
    return n


def scopes(drive_paths):
    doc = json.loads((GOLDEN / "scopes.json").read_text())
    top = sorted({p.split("/")[0] for p in drive_paths if p.split("/")[0] != "Confidential"})
    out = []
    for s in doc["scopes"]:
        pref = set()
        for inc in s["include"]:
            inc = inc.removeprefix("redteam/").removeprefix("docs/")
            if inc == "**":
                pref |= set(top) | ({"Confidential"} if s["contains_redteam"] else set())
            elif inc.endswith("/**"):
                pref.add(inc[:-3])
            else:
                pref.add(inc)
        pref = [p for p in sorted(pref) if any(f == p or f.startswith(p + "/") for f in drive_paths)]
        if pref:
            out.append((s["id"], s["contains_redteam"], pref))
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--run-id", default=time.strftime("%Y%m%d-%H%M%S"))
    ap.add_argument("--evidence-dir", default=str(REPO / "testing/super-ontology/evidence/L4"))
    ap.add_argument("--keep", action="store_true")
    ap.add_argument("--skip-build", action="store_true")
    args = ap.parse_args()

    ev = l2.Evidence(Path(args.evidence_dir), args.run_id)
    target = Path(os.environ.get("CARGO_TARGET_DIR", REPO / "target")) / "debug"
    oo = target / "oo"
    work = Path(tempfile.mkdtemp(prefix=f"oo-redteam-{args.run_id}-", dir="/tmp"))
    state, drive = work / "state", work / "drive"
    ev.log(f"# red-team gate, all formats, compose runtime, run {args.run_id}")
    project = comp.Project("oo-docs-" + hashlib.sha256(os.path.realpath(state).encode()).hexdigest()[:12])
    daemon = None
    ok = False
    result = {"run_id": args.run_id}
    try:
        if not args.skip_build:
            l2.run(ev, ["cargo", "build", "--locked", "-p", "oo-cli", "--features", "docs"])
        if not oo.is_file():
            raise SystemExit(f"missing binary {oo}")
        state.mkdir(mode=0o700)
        counts = build_drive(drive)
        ev.check("the Drive holds every golden format and the red-team files",
                 all(counts.get(e, 0) > 0 for e in (".md", ".txt", ".csv", ".xlsx", ".docx", ".pptx", ".pdf")) and (drive / "Confidential").is_dir(),
                 json.dumps(counts, sort_keys=True))
        daemon = comp.Daemon(ev, oo, state, comp.DEPLOY)
        api = daemon.start()
        st, d = api.request("POST", "/v1/docs/drives", {"name": "redteam-all", "backend": "fs", "config": {"root": str(drive)}})
        ev.check("Drive registered", st == 201, f"HTTP {st}")
        drive_id = d["id"]
        total = sum(counts.values())
        t0 = time.time()
        states = {}

        def settled():
            nonlocal states
            states = l2.file_states(api, drive_id)
            return len(states) == total and all(s in FINAL for s in states.values())

        l2.wait_until(PUBLISH_DEADLINE, settled, step=2)
        by = {}
        for s in states.values():
            by[s] = by.get(s, 0) + 1
        ev.check("every file reached a final state", len(states) == total and all(s in FINAL for s in states.values()), f"{time.time() - t0:.0f}s {json.dumps(by, sort_keys=True)}")
        ev.check("every file is published (docx, pptx and pdf included)", by.get("published") == total, json.dumps(by, sort_keys=True))
        conf = {p: s for p, s in states.items() if p.startswith("Confidential/")}
        ev.check("the four red-team files are published", len(conf) == 4 and all(s == "published" for s in conf.values()), json.dumps(conf, ensure_ascii=False, sort_keys=True))
        paths = sorted(states)
        result["files"] = total
        result["states"] = by

        words = forbidden_words()
        all_scopes = scopes(paths)
        seen = 0
        hits = []
        per_scope = {}
        control_hits = 0
        control_docx_src = False
        refused = []
        for i, (sid, with_rt, pref) in enumerate(all_scopes):
            for j, task in enumerate(TASKS):
                to = work / f"m{i}-{j}"
                to.mkdir()
                argv = [str(oo), "context", "mount", "--to", str(to), "--for", task, "--json", "--state-dir", str(state)]
                for p in pref:
                    argv += ["--scope", p]
                r = subprocess.run(argv, capture_output=True, text=True, timeout=180)
                if r.returncode != 0:
                    # The adversarial task names material outside the scope: the compiler's leak
                    # guard may refuse the whole pack (exit 7, DOCS_PACK_LEAK_GUARD). A refusal leaks
                    # nothing; any other failure is a failure of the gate.
                    guarded = ADVERSARIAL == j and with_rt is False and r.returncode == 7 and "DOCS_PACK_LEAK_GUARD" in (r.stdout + r.stderr)
                    if guarded:
                        refused.append(sid)
                        per_scope.setdefault(sid, {"redteam_in_scope": with_rt, "scopes": pref, "mounts": 0, "files": 0, "refused": 0})
                        per_scope[sid]["mounts"] += 1
                        per_scope[sid]["refused"] = per_scope[sid].get("refused", 0) + 1
                        continue
                    ev.check(f"mount scope {sid} task {j + 1} exits 0", False, f"exit {r.returncode} {(r.stdout + r.stderr).strip()[:140]}")
                    continue
                scanned = 0
                for f in sorted((to / ".context").rglob("*")):
                    if not f.is_file():
                        continue
                    text = f.read_text(encoding="utf-8", errors="replace")
                    low = text.lower()
                    scanned += 1
                    found = [w for w in words if w.lower() in low]
                    if with_rt:
                        control_hits += len(found)
                        control_docx_src = control_docx_src or "董事会纪要.docx" in unquote(text)
                    else:
                        hits += [(sid, j, str(f.relative_to(to)), w) for w in found]
                seen += 0 if with_rt else scanned
                per_scope.setdefault(sid, {"redteam_in_scope": with_rt, "scopes": pref, "mounts": 0, "files": 0})
                per_scope[sid]["mounts"] += 1
                per_scope[sid]["files"] += scanned
                subprocess.run([str(oo), "context", "revoke", str(to), "--state-dir", str(state)], capture_output=True, text=True, timeout=60)
        plain = [s for s, rt, _ in all_scopes if not rt]
        ev.check("scopes without the red-team files were mounted", len(plain) >= 5 and all(per_scope.get(s, {}).get("mounts") == len(TASKS) for s in plain), f"{len(plain)} scopes x {len(TASKS)} tasks, {seen} files")
        ev.log(f"  adversarial task refused by the leak guard in {len(refused)} of {len(plain)} scopes: {refused}")
        ev.check("GATE: zero canary or red-team-only hits in every pack of every scope that excludes them", not hits and seen > 50, f"{seen} files scanned, {len(hits)} hits {hits[:3]}")
        ev.check("control: a pack that includes Confidential/ does show red-team-only names", control_hits > 0, f"{control_hits} occurrences")
        # every red-team file was read completely by its parser (the docx by the sidecar):
        # its canary is in the blocks the engine stored
        st, listing = api.request("GET", f"/v1/docs/drives/{drive_id}/files?limit=500")
        ids = {f["path"]: f["id"] for f in listing["items"]}
        canary_of = {"Confidential/董事会纪要.docx": "OOCANARY-57ebbd74370a828a33c9ee593bf91587", "Confidential/并购评估备忘.md": "OOCANARY-f8ba1161063f397e254914f2dbf2e264",
                     "Confidential/薪酬调整.csv": "OOCANARY-770c9a0d583e3a62376c73aa4cfd2ddd", "Confidential/夜航计划.txt": "OOCANARY-bec507e1e43767e502c4e1b20f8fc64f"}
        parsed = {}
        for path, canary in canary_of.items():
            rows, cursor = [], None
            while True:
                st, b = api.request("GET", f"/v1/docs/files/{ids[path]}/blocks?limit=1000" + (f"&cursor={cursor}" if cursor else ""))
                rows += b.get("items", [])
                cursor = b.get("next_cursor")
                if not cursor:
                    break
            parsed[path] = any(canary in (r.get("text") or "") for r in rows)
        ev.check("control: every red-team file, the docx included, was parsed completely (its canary is in the stored blocks)", all(parsed.values()), json.dumps(parsed, ensure_ascii=False))
        result.update({"hits": len(hits), "files_scanned": seen, "scopes": per_scope, "forbidden_words": len(words), "control_hits": control_hits, "adversarial_refused_by_leak_guard": refused})
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
        result.update({"ok": ok, "checks": ev.checks})
        ev.write("redteam-gate.json", json.dumps(result, indent=1, ensure_ascii=False, sort_keys=True) + "\n")
        ev.write("redteam-gate.txt", "\n".join(ev.log_lines) + "\n")
    passed = sum(1 for c in ev.checks if c["ok"])
    print(f"# {passed}/{len(ev.checks)} checks passed")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
