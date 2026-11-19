#!/usr/bin/env python3
"""The owner's L6 CLI acceptance flow (30 section 7.2: the CLI acceptance package at L6), run end to end on
`oo serve --runtime compose` with the `ocr` and `media` sidecars. It extends the L4 package
(`evidence/L4/ACCEPTANCE-CLI.md`, `super_ontology_cli_acceptance.py`) with what L6 adds, and every step is an
`oo` command (the inputs are made by `super_ontology_l6_samples.py`).

The steps are those of `testing/super-ontology/evidence/L6/ACCEPTANCE-CLI.md`, with the same commands:

  1  start `oo serve --runtime compose` with OO_DOCS_PROFILES=ocr,media; `oo docs health` names the sidecars
  2  register the sample Drive (md, scanned pdf and png, a 3 speaker meeting wav, a 2 slide recording webm,
     an H.264 mp4 and an AAC m4a) and watch the file states
  3  OCR: a pack of the scans carries the page text as evidence (`refs/`, `#p1`, `#img`)
  4  speaker identity: one review item per speaker label, nothing published under a name before it is
     confirmed, `oo docs review decide accept --name` confirms one and its facts get the Person as subject
  5  the screen recording: a pack carries the slide text with `#t=` evidence
  6  the files no Linux container can decode say `DOCS_PARSE_CODEC_UNAVAILABLE`, also in the BRIEF's coverage line
  7  drift: a fact a Share used changes, `oo docs share refresh` and the BRIEF lists the change
  8  a media job is running while a Markdown file is edited: the edit reaches a BRIEF within 30 s (D49)
  9  revoke, stop

Not in a CLI package, and where they are: the parser version bump and the 2 hour meeting (A8, 30 section L6
acceptance 3 and 8: `super_ontology_l6_e2e.py`, `super_ontology_l6_long.py`), the OCR and ASR recall numbers
(`ocr-recall.md`, `asr-bakeoff.md`).

Every command, its exit code and the head of its output go to `cli-acceptance-run.json` in the evidence
directory (names `cli-acceptance-l6-*`).

Usage: tools/oo-test/super_ontology_cli_acceptance_l6.py [--run-id ID] [--evidence-dir DIR] [--keep] [--skip-build]
"""

import argparse
import hashlib
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
import super_ontology_compose as comp  # noqa: E402
import super_ontology_l2 as l2  # noqa: E402

REPO = l2.REPO
comp.LANE = os.environ.get("OO_TEST_LANE", "super-ontology-a7")
# `OO_ACCEPT_DEPLOY_DIR`: a copy of deploy/docs whose images.lock.json names the image ids present on this host,
# for a host where the locked ids are not (the lock itself is the integration thread's).
DEPLOY = Path(os.environ.get("OO_ACCEPT_DEPLOY_DIR", REPO / "deploy/docs"))
WORKING = {"stored", "queued", "parsing", "parsed", "extracting", "indexed", "outdated"}
# the sample Drive of super_ontology_l6_samples.py: 8 files and the degraded OCR picture
N_FILES = 9
MEETING = "Meetings/周会 参会=小王、小李.wav"
D49_EDIT_SECONDS = 30.0
D49_MD_SECONDS = 7.0  # D49 as calibrated at L3 (2026-11-17): md/txt detected to published, p95


class Cli:
    def __init__(self, ev, oo, state, log):
        self.ev, self.oo, self.state, self.log = ev, str(oo), str(state), log

    def run(self, *args, timeout=240, quiet=False):
        argv = [self.oo, *args, "--state-dir", self.state]
        t0 = time.time()
        p = subprocess.run(argv, capture_output=True, text=True, timeout=timeout)
        shown = " ".join(args).replace(self.state, "<state>")
        self.log.append({"command": f"oo {shown}", "exit": p.returncode, "seconds": round(time.time() - t0, 1),
                         "stdout": p.stdout[:1500], "stderr": p.stderr[:300]})
        if not quiet:
            self.ev.log(f"$ oo {shown}  -> exit {p.returncode}")
        return p

    def json(self, *args, **kw):
        p = self.run(*args, "--json", **kw)
        try:
            return p.returncode, json.loads(p.stdout.strip().splitlines()[-1]) if p.stdout.strip() else {}
        except ValueError:
            return p.returncode, {"raw": p.stdout, "err": p.stderr}


def wait_for_slot(ev):
    """At most two Super Ontology stacks run at once on the host (30 section 7, resource rules): wait for a slot."""
    while True:
        out = comp.sh("docker", "ps", "--filter", "label=oo.lane", "--format", "{{.Label \"oo.lane\"}}").stdout.split()
        others = {l for l in out if l.startswith("super-ontology") and l != comp.LANE}
        if len(others) < 2:
            return
        ev.log(f"  {len(others)} other stacks are up, waiting for a slot")
        time.sleep(30)


def files_of(cli):
    rc, v = cli.json("docs", "files", "ls", "--drive", "l6", quiet=True)
    return {f["path"]: f for f in v.get("files", [])}


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--run-id", default=time.strftime("l6cli-%Y%m%d-%H%M%S"))
    ap.add_argument("--evidence-dir", default=str(REPO / "testing/super-ontology/evidence/L6"))
    ap.add_argument("--keep", action="store_true")
    ap.add_argument("--skip-build", action="store_true")
    args = ap.parse_args()

    ev = l2.Evidence(Path(args.evidence_dir), args.run_id)
    ev.log(f"# Super Ontology L6 CLI acceptance, run {args.run_id}")
    wait_for_slot(ev)
    target = Path(os.environ.get("CARGO_TARGET_DIR", REPO / "target")) / "debug"
    oo = target / "oo"
    work = Path(tempfile.mkdtemp(prefix=f"oo-cliacc6-{args.run_id}-", dir="/tmp"))
    state, drive = work / "state", work / "drive"
    project = comp.Project("oo-docs-" + hashlib.sha256(os.path.realpath(state).encode()).hexdigest()[:12])
    log, serve, ok = [], None, False
    try:
        if not args.skip_build:
            l2.run(ev, ["cargo", "build", "--locked", "-p", "oo-cli", "--features", "docs"])
        state.mkdir(mode=0o700)
        # The fs poll is 30 s by default; the 30 s bound of step 8 starts when the edit is seen (as in L2 and
        # the L6 end to end run), so the poll is 5 s here.
        (state / "engine").mkdir(mode=0o700)
        (state / "engine/docs.json").write_text(json.dumps({"version": "oo.docs/v1", "sync": {"fs_poll_seconds": 5}}))
        cli = Cli(ev, oo, state, log)

        ev.log("\n## 0. the sample Drive")
        p = subprocess.run(["/usr/bin/python3", str(REPO / "tools/oo-test/super_ontology_l6_samples.py"), str(drive)],
                           capture_output=True, text=True)
        ev.command(["python3", "tools/oo-test/super_ontology_l6_samples.py", "<drive>"])
        listing = p.stdout.strip().splitlines()
        ev.check("0 the sample script writes 9 synthetic files", p.returncode == 0 and listing and listing[-1] == f"{N_FILES} files", listing[-1] if listing else p.stderr[-200:])

        ev.log("\n## 1. start the compose runtime with the ocr and media sidecars")
        lock = json.loads((DEPLOY / "images.lock.json").read_text())
        env = dict(os.environ, OO_DOCS_PROFILES="ocr,media", OO_DOCS_LANE=comp.LANE, OO_DOCS_DEPLOY_DIR=str(DEPLOY),
                   OO_DOCS_MEDIA_IMAGE=lock["media_parser"]["tag"], OO_DOCS_OCR_IMAGE=lock["ocr_parser"]["tag"])
        argv = [str(oo), "serve", "--runtime", "compose", "--json", "--port", "0", "--state-dir", str(state), "--deploy-dir", str(DEPLOY)]
        ev.command(argv)
        serve = subprocess.Popen(argv, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        info = json.loads(serve.stdout.readline())
        ev.check("1 oo serve --runtime compose prints its address", info.get("engine", {}).get("mode") == "compose" and info.get("port"), json.dumps({"port": info.get("port")}))
        names = comp.sh("docker", "ps", "--filter", f"label=oo.lane={comp.LANE}", "--format", "{{.Names}}").stdout
        ev.check("1 ocr-parser and media-parser run beside the engine", "ocr-parser" in names and "media-parser" in names, names.replace("\n", " "))
        cli.run("docs", "health", quiet=True)
        rc, h = cli.json("docs", "health")
        parser = (h.get("components") or {}).get("parser", {})
        ev.write("cli-acceptance-l6-health.json", json.dumps(h, ensure_ascii=False, indent=1) + "\n")
        # No model is configured and every sidecar was started by `oo serve`: nothing is degraded here (spec 10 4.12: degraded
        # only for an unavailable parser/db/cas). The parsers may need a moment after the engine answers, so wait for ok.
        def health_ok():
            nonlocal rc, h
            rc, h = cli.json("docs", "health", quiet=True)
            return rc == 0 and h.get("status") == "ok"
        l2.wait_until(60, health_ok, step=3)
        ev.check("1 oo docs health is ok", rc == 0 and h.get("status") == "ok", f"exit {rc} status {h.get('status')}")

        ev.log("\n## 2. register the Drive and watch the states")
        rc, d = cli.json("docs", "drive", "add", "--name", "l6", "--root", str(drive))
        ev.check("2 oo docs drive add exits 0", rc == 0 and d.get("id"), f"exit {rc}")
        t0 = time.time()
        seen = []

        def settled():
            fs = files_of(cli)
            states = sorted({f["state"] for f in fs.values()})
            if not seen or seen[-1][1] != states:
                seen.append((round(time.time() - t0, 1), states))
            return len(fs) == N_FILES and not any(f["state"] in WORKING and f["state"] != "indexed" for f in fs.values())

        l2.wait_until(1500, settled, step=5)
        ev.log("  states seen: " + json.dumps(seen, ensure_ascii=False))
        cli.run("docs", "files", "ls", "--drive", "l6", quiet=True)
        fs = files_of(cli)
        ev.timings["all_files_settled_seconds"] = round(time.time() - t0, 1)
        st = lambda p: (fs.get(p) or {}).get("state")
        code = lambda p: (fs.get(p) or {}).get("state_code")
        ev.write("cli-acceptance-l6-files.json", json.dumps({k: [v["state"], v.get("state_code")] for k, v in fs.items()}, ensure_ascii=False, indent=1) + "\n")
        ev.check("2 the two Markdown files, the scan (pdf, png), the meeting and the recording are published",
                 all(st(p) == "published" for p in ("Projects/星火计划.md", "People/林川.md", "Scans/纪要.png", "Scans/扫描件.pdf", MEETING, "Meetings/录屏.webm")),
                 json.dumps({k: v["state"] for k, v in fs.items()}, ensure_ascii=False))

        ev.log("\n## 3. OCR")
        mnt_s = work / "scan-mount"
        mnt_s.mkdir()
        rc, m = cli.json("context", "mount", "--to", str(mnt_s), "--scope", "Scans", "--for", "纪要")
        ev.check("3 oo context mount of Scans exits 0", rc == 0, f"exit {rc}")
        refs = {p.name: p.read_text(encoding="utf-8") for p in sorted((mnt_s / ".context/refs").glob("*.md"))} if (mnt_s / ".context/refs").is_dir() else {}
        scan_refs = {k: v for k, v in refs.items() if "文件：Scans/" in v}
        ev.write("cli-acceptance-l6-scan-refs.txt", "\n\n".join(f"== refs/{k}\n" + v.split("\n# ", 1)[-1] for k, v in scan_refs.items()) + "\n")
        # A page can appear in several refs (one per quoted span); the text of a file is what its refs say together.
        pdf_ref = "\n".join(v for v in scan_refs.values() if "文件：Scans/扫描件.pdf" in v and "位置：#p1" in v)
        png_ref = "\n".join(v for v in scan_refs.values() if "文件：Scans/纪要.png" in v and "位置：#img" in v)
        words = ("2026年度计划会纪要", "负责人：林川", "项目：星火计划")
        ev.check("3 refs/ carries the OCR text of the scanned pdf (#p1) and of the png (#img), every planted line",
                 all(w in pdf_ref for w in words) and all(w in png_ref for w in words), f"{len(scan_refs)} refs of Scans")
        brief = (mnt_s / ".context/BRIEF.md").read_text(encoding="utf-8") if (mnt_s / ".context/BRIEF.md").exists() else ""
        ev.check("3 the BRIEF names the project from the scan with a [src:Scans/..#p1] or #img source", bool(re.search(r"\[src:Scans/[^\]]*#(p1|img)\]", brief)), "")

        ev.log("\n## 4. speaker identity")
        rc, q = cli.json("docs", "review", "ls", "--kind", "speaker_identity", "--status", "pending")
        cli.run("docs", "review", "ls", "--kind", "speaker_identity", "--status", "pending", quiet=True)
        items = q.get("items", [])
        ev.write("cli-acceptance-l6-speaker-items.json", json.dumps(items, ensure_ascii=False, indent=1) + "\n")
        ev.check("4 oo docs review ls shows one speaker_identity item per speaker label", rc == 0 and len(items) >= 2, f"{len(items)} items")
        rc, ps = cli.json("docs", "entity", "search", "小王", "--type", "Person")
        ev.check("4 no Person 小王 exists before the confirmation", rc == 0 and not ps.get("entities"), f"{len(ps.get('entities', []))} hits")
        s1 = next((i for i in items if (i.get("subject") or {}).get("speaker") == "S1" and (i.get("source") or {}).get("path") == MEETING), None)
        ev.check("4 the item of speaker S1 of the meeting is there", s1 is not None, "")
        if s1:
            rc, dec = cli.json("docs", "review", "decide", "accept", s1["id"], "--name", "小王")
            ev.check("4 oo docs review decide accept --name confirms S1 as 小王", rc == 0 and dec.get("accepted") == 1, json.dumps(dec, ensure_ascii=False)[:160])
            got = l2.wait_until(90, lambda: cli.json("docs", "entity", "search", "小王", "--type", "Person", quiet=True)[1].get("entities"), step=3)
            ev.check("4 after the confirmation the Person 小王 exists", bool(got), json.dumps(got, ensure_ascii=False)[:160] if got else "")
            if got:
                cli.run("docs", "entity", "show", got[0]["key"], quiet=True)
                def owns_of():
                    rc, shw = cli.json("docs", "entity", "show", got[0]["key"], quiet=True)
                    return [f for f in shw.get("facts", []) if f.get("predicate") == "owns"], shw
                l2.wait_until(90, lambda: owns_of()[0], step=3)
                owns, shw = owns_of()
                ev.check("4 the Person 小王 owns the project the speaker named (the fact's subject is the Person)", bool(owns), json.dumps([(f.get("predicate"), f.get("status")) for f in shw.get("facts", [])], ensure_ascii=False))
        rc, q2 = cli.json("docs", "review", "ls", "--kind", "speaker_identity", "--status", "pending")
        ev.check("4 the other speaker labels still wait for a person", rc == 0 and len(q2.get("items", [])) == len(items) - 1, f"{len(q2.get('items', []))} pending")

        ev.log("\n## 5. the screen recording")
        rc, v = cli.json("docs", "files", "show", fs["Meetings/录屏.webm"]["id"])
        ev.check("5 oo docs files show: the recording is published", rc == 0 and v.get("state") == "published", f"{v.get('state')} {v.get('state_code')}")
        mnt_m = work / "media-mount"
        mnt_m.mkdir()
        rc, m = cli.json("context", "mount", "--to", str(mnt_m), "--scope", "Meetings", "--for", "星火计划九月上线")
        ev.check("5 oo context mount of Meetings exits 0", rc == 0, f"exit {rc}")
        pack = {str(p.relative_to(mnt_m / ".context")): p.read_text(encoding="utf-8", errors="ignore") for p in (mnt_m / ".context").rglob("*") if p.is_file() and p.suffix in (".md", ".json")}
        ev.write("cli-acceptance-l6-media-mount-files.txt", "\n".join(sorted(pack)) + "\n")
        webm_ref = next((v for k, v in pack.items() if k.startswith("refs/") and "文件：Meetings/录屏.webm" in v), "")
        wav_ref = next((v for k, v in pack.items() if k.startswith("refs/") and "文件：Meetings/周会" in v), "")
        ev.write("cli-acceptance-l6-media-refs.txt", webm_ref.split("\n# ", 1)[-1] + "\n" + wav_ref.split("\n# ", 1)[-1] + "\n")
        ev.check("5 refs/ carries the slide text of the recording with a #t= time range", "星火计划九月上线" in webm_ref and re.search(r"位置：#t=\d\d:\d\d:\d\d-\d\d:\d\d:\d\d", webm_ref) is not None, "")
        ev.check("5 the meeting's text has a #t= range with the speaker label (@S1)", re.search(r"位置：#t=[0-9:\-]+@S1", wav_ref) is not None, "")
        allt = "\n".join(pack.values())
        ev.check("5 BRIEF and project files cite the recording as [src:Meetings/录屏.webm#t=..]", "[src:Meetings/录屏.webm#t=" in allt, "")

        ev.log("\n## 6. what no Linux container can decode")
        for p_ in ("Meetings/录屏h264.mp4", "Meetings/讲话.m4a"):
            rc, v = cli.json("docs", "files", "show", fs[p_]["id"])
            ev.check(f"6 {p_}: unsupported, DOCS_PARSE_CODEC_UNAVAILABLE", rc == 0 and v.get("state") == "unsupported" and v.get("state_code") == "DOCS_PARSE_CODEC_UNAVAILABLE", f"{v.get('state')} {v.get('state_code')}")
        mbrief = (mnt_m / ".context/BRIEF.md").read_text(encoding="utf-8")
        cov = next((l for l in mbrief.splitlines() if l.startswith("> 覆盖：")), "")
        ev.write("cli-acceptance-l6-BRIEF-media.md", mbrief)
        ev.check("6 the BRIEF's coverage line names both files with DOCS_PARSE_CODEC_UNAVAILABLE", cov.count("DOCS_PARSE_CODEC_UNAVAILABLE") == 2, cov[:160])

        ev.log("\n## 7. drift: a fact a Share used changes")
        t1 = work / "m1"
        t1.mkdir()
        rc, m1 = cli.json("context", "mount", "--to", str(t1), "--scope", "Projects", "--for", "星火计划")
        ev.check("7 oo context mount before the change", rc == 0, f"exit {rc}")
        sid = json.loads((t1 / ".context/SHARE.json").read_text())["share_id"]
        md = drive / "Projects/星火计划.md"
        md.write_text(md.read_text(encoding="utf-8").replace("进行中", "已暂停"), encoding="utf-8")
        t_edit = time.time()
        got = l2.wait_until(90, lambda: (cli.run("context", "check", "--dir", str(t1 / ".context"), quiet=True).returncode in (10, 20)), step=2)
        p = cli.run("context", "check", "--dir", str(t1 / ".context"))
        ev.check("7 oo context check tells the share drifted (exit 10 or 20)", bool(got) and p.returncode in (10, 20), f"exit {p.returncode} after {time.time() - t_edit:.0f}s: {p.stdout.strip()[:100]}")
        rc, r = cli.json("docs", "share", "refresh", sid)
        ev.check("7 oo docs share refresh makes a new share and mounts it in the same directory", rc == 0 and bool(r.get("shares")) and r["shares"][0].get("mounted") is True and r["shares"][0].get("old") == sid, json.dumps(r, ensure_ascii=False)[:200])
        pcheck = cli.run("context", "check", "--dir", str(t1 / ".context"))
        ev.check("7 oo context check on the refreshed mount exits 0", pcheck.returncode == 0, f"exit {pcheck.returncode}")
        brief = (t1 / ".context/BRIEF.md").read_text(encoding="utf-8") if (t1 / ".context/BRIEF.md").exists() else ""
        ev.write("cli-acceptance-l6-BRIEF-after-refresh.md", brief)
        ev.check("7 the BRIEF lists the change after its head (自上次以来的变化, 已暂停)", "自上次以来的变化" in brief and "已暂停" in brief, f"{len(brief)} bytes")

        ev.log("\n## 8. a media job is running while a Markdown file is edited")
        wav = drive / MEETING
        longer = drive / "Meetings/周会2 参会=小王、小李.wav"
        subprocess.run(["ffmpeg", "-nostdin", "-v", "error", "-y", "-stream_loop", "40", "-i", str(wav), "-c", "copy", str(longer)], check=True)
        cli.run("docs", "drive", "sync", "l6", quiet=True)
        busy = l2.wait_until(180, lambda: any(f["state"] in ("queued", "parsing", "parsed", "extracting") and f["path"].endswith("周会2 参会=小王、小李.wav") for f in files_of(cli).values()), step=1)
        ev.check("8 the long recording is being processed", bool(busy), "")
        md.write_text(md.read_text(encoding="utf-8").replace("已暂停", "收尾中"), encoding="utf-8")
        t_edit = time.time()
        t3 = work / "m3"
        t3.mkdir()
        def edit_visible():
            rc, _ = cli.json("context", "mount", "--to", str(t3), "--scope", "Projects", "--for", "星火计划", quiet=True)
            b = (t3 / ".context/BRIEF.md").read_text(encoding="utf-8") if (t3 / ".context/BRIEF.md").exists() else ""
            if "收尾中" in b:
                return True
            cli.run("context", "revoke", str(t3), quiet=True)  # a stale pack: drop it and ask again
            return False
        done = l2.wait_until(D49_EDIT_SECONDS + 30, edit_visible, step=1)
        el = time.time() - t_edit
        still = [f["state"] for f in files_of(cli).values() if f["path"].endswith("周会2 参会=小王、小李.wav")]
        ev.timings["edit_to_brief_seconds"] = round(el, 1)
        ev.check("8 edit to BRIEF round trip (edit, detect, publish, new mount; not the per-file latency) within 30 s while the media job runs", bool(done) and el <= D49_EDIT_SECONDS, f"{el:.1f}s; long recording state {still}")
        # D49 (calibrated at L3): md/txt detected to published, p95 <= 7 s. Per-file `latency_ms.total_ms` of each md/txt
        # file's latest cycle (the edited file, measured while the long recording is processed, and the other file).
        lat = {k: (f.get("latency_ms") or {}).get("total_ms") for k, f in files_of(cli).items() if k.endswith((".md", ".txt")) and f["state"] == "published"}
        worst = max((v for v in lat.values() if v is not None), default=None)
        ev.timings["d49_md_txt_total_ms"] = lat
        ev.check("8 D49: every md/txt file went from detected to published within 7 s (p95)", len(lat) >= 2 and None not in lat.values() and worst <= D49_MD_SECONDS * 1000, json.dumps(lat, ensure_ascii=False))

        ev.log("\n## 9. revoke and stop")
        for t in (t1, t3, mnt_m, mnt_s):
            cli.run("context", "revoke", str(t), quiet=True)
        rc, sls = cli.json("docs", "share", "ls")
        ev.check("9 every share is revoked or refreshed away", rc == 0 and not any(s["state"] == "active" for s in sls.get("shares", [])), json.dumps([s["state"] for s in sls.get("shares", [])]))
        rc, rm = cli.json("docs", "drive", "rm", "l6")
        ev.check("9 oo docs drive rm starts the deletion", rc == 0, json.dumps(rm)[:120])
        p = cli.run("serve", "--stop")
        try:
            code_ = serve.wait(timeout=120)
        except subprocess.TimeoutExpired:
            code_ = None
        serve = None
        ev.check("9 oo serve --stop: stopped and the daemon exits 0", p.returncode == 0 and "stopped" in p.stdout and code_ == 0, f"{p.stdout.strip()} / exit {code_}")
        ok = all(c["ok"] for c in ev.checks)
    finally:
        if serve:
            serve.send_signal(signal.SIGTERM)
            try:
                serve.wait(timeout=90)
            except subprocess.TimeoutExpired:
                serve.kill()
        if not args.keep:
            project.purge()
        left = project.containers() + project.networks() + project.volumes()
        ev.check("cleanup left nothing", args.keep or not left, f"{len(left)} left")
        if not args.keep:
            l2.remove_tree(work)
        ok = ok and all(c["ok"] for c in ev.checks)
        ev.write("cli-acceptance-l6-run.json", json.dumps({"run_id": args.run_id, "ok": ok, "checks": ev.checks, "timings": ev.timings, "commands": log}, indent=1, ensure_ascii=False) + "\n")
        ev.write("cli-acceptance-l6-run.txt", "\n".join(ev.log_lines) + "\n")
    passed = sum(1 for c in ev.checks if c["ok"])
    print(f"# {passed}/{len(ev.checks)} checks passed")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
