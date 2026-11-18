#!/usr/bin/env python3
"""Super Ontology L6 end to end on the compose stack (30 section L6 acceptance 1, 3, 4, 5, 7, 8, 10; B-L6-2).

  /usr/bin/python3 tools/oo-test/super_ontology_l6.py --suite e2e [--run-id ID] [--evidence-dir DIR] [--keep]

Runs the real `oo serve --runtime compose` (docs-db, docs-engine, doc-parser) with the `ocr` and `media` sidecars
(OO_DOCS_PROFILES=ocr,media) and drives the control API and `oo context mount`. Needs the local images from
deploy/docs/images.lock.json plus `oo-docs-ocr-parser:dev` and `oo-docs-media-parser:dev`, Pillow on the host
python (the macOS system python has it) and, for the synthetic speech, macOS `say` and an ffmpeg with libopus,
libvpx-vp9 and libx264 (host tools that make the *inputs*; the containers decode with their own LGPL FFmpeg).
Waits while another Super Ontology compose stack is up. No credential is written to the evidence.

  A1  scanned pdf and a standalone image published; the OCR text carries the core facts and the planted words
  A3  3 speaker meeting: one speaker_identity item per label, nothing from an unconfirmed speaker published, after
      confirming S1 the fact's subject is the named Person (the 2 hour kill/resume run is the L1 cell docs-media-long)
  A4  2 slide screen recording (VP9/Opus webm): one key frame per slide, frame text, `#t=` provenance
      (jumping in Desktop is phase two, F-S10-07)
  A5  H.264/AAC mp4 and AAC m4a: unsupported with DOCS_PARSE_CODEC_UNAVAILABLE (no host decoder on Linux);
      macOS host decode is a Desktop helper, phase two (F-S10-05); ffmpeg -buildconf is the L1 cell docs-media
  A7  change a fact a Share used, refresh: BRIEF lists the change after the head
  A8  parser version bump (container run): outdated, old facts stay published meanwhile, no retraction churn, DOC-G10 judges
  A10 a media job is running while a Markdown edit publishes: its latency stays within the D49 bound
Acceptance 2 (bake-off) and 6 (SBOM) are in evidence/L6/asr-bakeoff.md and sbom-*.json; 9 (`puppyone share`) is
the Puppyone CLI and phase two.
"""
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
import super_ontology_l2 as l2  # noqa: E402  (Evidence, Api, run, wait_until, remove_tree, tree_hashes)

REPO = l2.REPO
LANE = "super-ontology-be2"
D49_MD_SECONDS = 7.0  # D49 as calibrated at L3 (2026-11-17): md/txt detected to published, p95
D49_EDIT_SECONDS = 30.0  # md edit to new BRIEF while a media job runs (the L2 bound, 30 section L2 acceptance 5)

SLIDES = [("季度复盘", "营收增长百分之十二"), ("下季计划", "星火计划九月上线")]
SCAN_LINES = ["2026年度计划会纪要", "负责人：林川", "项目：星火计划"]
MEETING = [  # (voice, text); S1 introduces themselves and says what they own
    ("Tingting", "大家好，我是小王。我负责星火计划。"),
    ("Sandy (中文（中国大陆）)", "好的，我们开始今天的例会。"),
    ("Tingting", "小李你说一下进度。"),
    ("Rocko (中文（中国大陆）)", "我是小李，进度正常。"),
]


def sh(argv, **kw):
    return subprocess.run(argv, capture_output=True, text=True, **kw)


def wait_for_free(ev):
    while True:
        out = sh(["docker", "ps", "--filter", "label=oo.lane", "--format", "{{.Names}} {{.Label \"oo.lane\"}}"]).stdout
        busy = [l for l in out.splitlines() if "super-ontology" in l and LANE not in l and "-pg" not in l.split()[0]]
        if not busy:
            return
        ev.log(f"  another stack is up ({len(busy)} containers), waiting")
        time.sleep(30)


def render_page(path, lines, size=(1240, 1754)):
    from PIL import Image, ImageDraw, ImageFont
    img = Image.new("RGB", size, "white")
    d = ImageDraw.Draw(img)
    font = None
    for f in ("/System/Library/Fonts/STHeiti Medium.ttc", "/System/Library/Fonts/Songti.ttc"):
        try:
            font = ImageFont.truetype(f, 52)
            break
        except OSError:
            pass
    for i, t in enumerate(lines):
        d.text((120, 200 + i * 110), t, font=font, fill="black")
    img.save(path)


def make_inputs(root: Path):
    """Synthetic inputs: scanned pdf + png, 4 turn 3 speaker meeting wav, 2 slide webm, mp4 (H.264/AAC), m4a (AAC)."""
    (root / "Scans").mkdir(parents=True)
    (root / "Meetings").mkdir()
    page = root / "Scans/纪要.png"
    render_page(page, SCAN_LINES)
    # An image-only pdf (no text layer): sips converts the png.
    sh(["sips", "-s", "format", "pdf", str(page), "--out", str(root / "Scans/扫描件.pdf")])
    ff = ["ffmpeg", "-nostdin", "-v", "error", "-y"]
    tmp = Path(tempfile.mkdtemp(prefix="l6e2e-"))
    wavs = []
    for i, (voice, text) in enumerate(MEETING):
        aiff, wav = tmp / f"t{i}.aiff", tmp / f"t{i}.wav"
        sh(["say", "-v", voice, "-o", str(aiff), text], check=True)
        sh(ff + ["-i", str(aiff), "-ac", "1", "-ar", "16000", str(wav)], check=True)
        wavs.append(wav)
    lst = tmp / "list.txt"
    lst.write_text("".join(f"file '{w}'\n" for w in wavs))
    sh(ff + ["-f", "concat", "-safe", "0", "-i", str(lst), "-c:a", "pcm_s16le", str(root / "Meetings/周会 参会=小王、小李.wav")], check=True)
    # Two slide screen recording: 6 s per slide, narration is the second meeting turn.
    frames = []
    for i, (title, body) in enumerate(SLIDES):
        png = tmp / f"slide{i}.png"
        render_page(png, [title, body], size=(1280, 720))
        frames.append(png)
    vl = tmp / "v.txt"
    vl.write_text("".join(f"file '{p}'\nduration 6\n" for p in frames) + f"file '{frames[-1]}'\n")
    sh(ff + ["-f", "concat", "-safe", "0", "-i", str(vl), "-i", str(wavs[1]), "-c:v", "libvpx-vp9", "-r", "1", "-c:a", "libopus",
             "-af", "apad", "-t", "12", str(root / "Meetings/录屏.webm")], check=True)
    sh(ff + ["-f", "concat", "-safe", "0", "-i", str(vl), "-i", str(wavs[1]), "-c:v", "libx264", "-pix_fmt", "yuv420p",
             "-c:a", "aac", "-af", "apad", "-t", "12", str(root / "Meetings/录屏h264.mp4")], check=True)
    sh(ff + ["-i", str(wavs[0]), "-c:a", "aac", str(root / "Meetings/讲话.m4a")], check=True)
    shutil.rmtree(tmp, ignore_errors=True)


def files(api, drive):
    st, b = api.request("GET", f"/v1/docs/drives/{drive}/files?limit=200")
    assert st == 200, b
    return {f["path"]: f for f in b["items"]}


def main(argv=None):
    import argparse
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--run-id", default=time.strftime("l6e2e-%Y%m%d-%H%M%S"))
    ap.add_argument("--evidence-dir", default=str(REPO / "testing/super-ontology/evidence/L6"))
    ap.add_argument("--keep", action="store_true")
    ap.add_argument("--skip-build", action="store_true")
    a = ap.parse_args(argv)
    ev = l2.Evidence(Path(a.evidence_dir), a.run_id)
    ev.log(f"# Super Ontology L6 end to end (compose), run {a.run_id}")
    wait_for_free(ev)
    target = Path(os.environ.get("CARGO_TARGET_DIR", REPO / "target")) / "debug"
    oo = target / "oo"
    work = Path(tempfile.mkdtemp(prefix=f"oo-l6e2e-{a.run_id}-", dir="/tmp"))
    state, drive_root = work / "state", work / "drive"
    state.mkdir(mode=0o700)
    # The fs poll is 30 s by default; the edit-to-BRIEF bound of A10 (30 s) is a bound on the work after the
    # edit is seen, as in L2 (which also polls every 5 s). B-L6rem: 38 s of the first 40.8 s were the poll.
    (state / "engine").mkdir(mode=0o700)
    (state / "engine/docs.json").write_text(json.dumps({"version": "oo.docs/v1", "sync": {"fs_poll_seconds": 5}}))
    serve = None
    try:
        if not a.skip_build:
            l2.run(ev, ["cargo", "build", "--locked", "-p", "oo-cli", "--features", "docs"])
        make_inputs(drive_root)
        md = {
            "Projects/星火计划.md": "---\ntype: project\nname: 星火计划\nowner: 林川\n---\n# 星火计划\n\n负责人：林川\n\n状态：进行中\n",
            "People/林川.md": "---\ntype: person\nname: 林川\nrole: 工程负责人\n---\n# 林川\n",
        }
        for rel, text in md.items():
            p = drive_root / rel
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(text, encoding="utf-8")
        # ---- oo serve --runtime compose with the ocr and media sidecars
        lock = json.loads((REPO / "deploy/docs/images.lock.json").read_text())
        env = dict(os.environ, OO_DOCS_PROFILES="ocr,media", OO_DOCS_LANE=LANE, OO_DOCS_DEPLOY_DIR=str(REPO / "deploy/docs"),
                   OO_DOCS_MEDIA_IMAGE=lock["media_parser"]["tag"], OO_DOCS_OCR_IMAGE=lock["ocr_parser"]["tag"])
        argv = [str(oo), "serve", "--runtime", "compose", "--json", "--port", "0", "--state-dir", str(state)]
        ev.command(argv)
        serve = subprocess.Popen(argv, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        info = json.loads(serve.stdout.readline())
        project = (info.get("engine") or {}).get("project") or info.get("project") or "oo-docs"
        api = l2.Api(info["port"], Path(info["token_file"]).read_text().strip())
        st, h = api.request("GET", "/v1/docs/health")
        ev.check("compose stack is up and healthy", st == 200 and h.get("status") in ("ok", "degraded"), f"HTTP {st}")
        names = sh(["docker", "ps", "--filter", f"label=oo.lane={LANE}", "--format", "{{.Names}}"]).stdout
        ev.check("ocr-parser and media-parser run beside the engine", "ocr-parser" in names and "media-parser" in names, names.replace("\n", " "))
        st, d = api.request("POST", "/v1/docs/drives", {"name": "l6", "backend": "fs", "config": {"root": str(drive_root)}})
        assert st == 201, d
        did = d["id"]
        # ---- A1, A4, A5: wait for every file to leave the working states
        working = {"stored", "queued", "parsing", "parsed", "extracting", "indexed", "outdated"}
        t0 = time.time()
        fs = l2.wait_until(1500, lambda: (lambda f: f if f and not any(x["state"] in working for x in f.values()) else None)(files(api, did)), step=5)
        ev.timings["all_files_settled_seconds"] = round(time.time() - t0, 1)
        ev.check("all files left the working states", bool(fs), json.dumps({k: v["state"] for k, v in (fs or {}).items()}, ensure_ascii=False))
        fs = fs or files(api, did)
        state_of = lambda p: (fs.get(p) or {}).get("state")
        code_of = lambda p: (fs.get(p) or {}).get("state_code")
        # A1
        for p in ("Scans/纪要.png", "Scans/扫描件.pdf"):
            ev.check(f"A1 {p} published", state_of(p) == "published", f"{state_of(p)} {code_of(p)}")
        def blocks(path):
            st, b = api.request("GET", f"/v1/docs/files/{fs[path]['id']}/blocks?limit=500")
            return b.get("items", []) if st == 200 else []
        text = "".join(b.get("text", "") for p in ("Scans/纪要.png", "Scans/扫描件.pdf") for b in blocks(p))
        hit = sum(1 for w in ("星火计划", "林川", "年度计划会纪要") if w in text)
        ev.check("A1 the OCR text carries the planted words", hit == 3, f"{hit}/3 words in {len(text)} chars")
        # A3: the meeting file and its speaker_identity items
        mp = "Meetings/周会 参会=小王、小李.wav"
        ev.check("A3 the meeting is published", state_of(mp) == "published", f"{state_of(mp)} {code_of(mp)}")
        st, rv = api.request("GET", "/v1/docs/review?kind=speaker_identity&status=pending&limit=50")
        items = rv.get("items", []) if st == 200 else []
        ev.check("A3 one speaker_identity item per speaker label", len(items) >= 2, f"{len(items)} items")
        st, ents = api.request("GET", "/v1/docs/entities?q=小王")
        persons = lambda e: [i for i in e.get("items", []) if i.get("type") == "Person" and "小王" in (i.get("name") or "")]
        ev.check("A3 nothing is published under a named Person before confirmation", st == 200 and not persons(ents), json.dumps(ents, ensure_ascii=False)[:120])
        s1 = next((i for i in items if i.get("subject", {}).get("speaker") == "S1"), None)
        if s1:
            st, r = api.request("POST", "/v1/docs/review:decide", {"items": [{"id": s1["id"], "revision": s1["revision"], "decision": "accept", "edit": {"name": "小王"}}]})
            ev.check("A3 confirming S1 as 小王 succeeds", st == 200 and r["results"][0]["status"] == "ok", json.dumps(r, ensure_ascii=False)[:160])
            ok = l2.wait_until(60, lambda: persons(api.request("GET", "/v1/docs/entities?q=小王")[1]), step=2)
            ev.check("A3 after confirming, the fact's subject is the Person 小王", bool(ok), json.dumps(ok, ensure_ascii=False)[:160] if ok else "")
        # A4
        vp = "Meetings/录屏.webm"
        ev.check("A4 the screen recording is published", state_of(vp) == "published", f"{state_of(vp)} {code_of(vp)}")
        vb = blocks(vp) if state_of(vp) == "published" else []
        frames = [b for b in vb if b.get("kind") == "frame_text"]
        ev.check("A4 one key frame per slide", len(frames) == len(SLIDES), f"{len(frames)} frame_text blocks")
        ftext = "".join(b.get("text", "") for b in frames)
        rec = sum(len(set(t + b) & set(ftext)) / max(1, len(set(t + b))) for t, b in SLIDES) / len(SLIDES)
        ev.check("A4 frame text recall >= 0.9 (character level)", rec >= 0.9, f"{rec:.2f}")
        ev.check("A4 blocks carry media anchors (t_start_ms) for #t= provenance", any("t_start_ms" in json.dumps(b.get("anchor", {})) for b in vb))
        # A5
        for p in ("Meetings/录屏h264.mp4", "Meetings/讲话.m4a"):
            ev.check(f"A5 {p} is unsupported with DOCS_PARSE_CODEC_UNAVAILABLE", state_of(p) == "unsupported" and code_of(p) == "DOCS_PARSE_CODEC_UNAVAILABLE", f"{state_of(p)} {code_of(p)}")
        # A7: Share, change a fact it used, refresh, BRIEF head lists the change
        body = {"drive_id": did, "scope": ["Projects"], "task": "星火计划"}
        t1 = work / "m1"
        t1.mkdir()
        p1 = l2.run(ev, [str(oo), "context", "mount", "--scope", "Projects", "--to", str(t1), "--json", "--state-dir", str(state)], check=False)
        ev.check("A7 mount before the change", p1.returncode == 0)
        st, shares = api.request("GET", f"/v1/docs/shares?drive_id={did}")
        sid = (shares.get("items") or [{}])[0].get("id")
        (drive_root / "Projects/星火计划.md").write_text(md["Projects/星火计划.md"].replace("进行中", "已暂停"), encoding="utf-8")
        l2.wait_until(90, lambda: state_of_now(api, did, "Projects/星火计划.md") == "published" and api.request("GET", f"/v1/docs/shares/{sid}")[1].get("status") == "drifted", step=1)
        st, new = api.request("POST", f"/v1/docs/shares/{sid}:refresh", {})
        ev.check("A7 the share drifted and refresh answers", st in (200, 201), f"HTTP {st}")
        brief = ""
        if st in (200, 201):
            st2, pk = api.request("GET", f"/v1/docs/context-packs/{new['pack_id']}/files/BRIEF.md") if new.get("pack_id") else (0, {})
            brief = pk if isinstance(pk, str) else (pk.decode() if isinstance(pk, bytes) else "")
        ev.check("A7 BRIEF lists the change after the head", "自上次以来的变化" in brief and "已暂停" in brief, f"{len(brief)} bytes")
        ev.write("a7-BRIEF-after-refresh.md", brief)
        # A8: a sidecar parser version bump. The stored media result is relabelled as an older version and the engine
        # restarted (it compares at start): the file turns outdated, the old facts stay published during the rerun,
        # nothing is retracted and DOC-G10 has nothing to hold.
        db = f"{project}-docs-db-1"
        psql = lambda q: sh(["docker", "exec", db, "psql", "-U", "postgres", "-d", "oo_docs", "-tAc", q]).stdout.strip()
        sidecar_paths = ("Scans/纪要.png", "Scans/扫描件.pdf", mp, "Meetings/录屏.webm")
        in_list = ", ".join("'" + p.replace("'", "''") + "'" for p in sidecar_paths)
        keys_sql = "SELECT fact_key FROM _oo_docs.fact WHERE status = 'published' ORDER BY 1"
        b3s_sql = ("SELECT fv.b3 FROM _oo_docs.file fl JOIN _oo_docs.file_version fv ON fv.id = fl.current_version_id "
                   f"WHERE fl.path IN ({in_list})")
        before = psql(keys_sql).splitlines()
        retracted_before = int(psql("SELECT count(*) FROM _oo_docs.fact WHERE status = 'retracted'") or 0)
        b3s = psql(b3s_sql).splitlines()
        ev.check("A8 the drive has published facts before the bump", len(before) >= 1 and len(b3s) == len(sidecar_paths), f"{len(before)} facts, {len(b3s)} sidecar files")
        psql(f"UPDATE _oo_docs.parse_result SET parser_version = parser_version || '-old' WHERE status = 'complete' AND b3 IN ({b3s_sql})")
        sh(["docker", "restart", f"{project}-docs-engine-1"])
        low, seen_not_published = len(before), False
        def rerun_done():
            nonlocal low, seen_not_published
            n_text = psql(f"SELECT count(*) FROM ({keys_sql}) k")
            if not n_text.isdigit():
                return False  # the query itself failed (docker exec hiccup): not a reading
            low = min(low, int(n_text))
            working = int(psql(f"SELECT count(*) FROM _oo_docs.file WHERE path IN ({in_list}) AND state <> 'published'") or 0)
            seen_not_published = seen_not_published or working > 0
            fresh = int(psql(f"SELECT count(DISTINCT b3) FROM _oo_docs.parse_result WHERE status = 'complete' AND parser_version NOT LIKE '%-old' AND b3 IN ({b3s_sql})") or 0)
            return working == 0 and fresh >= len(set(b3s))
        done = l2.wait_until(600, rerun_done, step=1)
        after = psql(keys_sql).splitlines()
        retracted = str(int(psql("SELECT count(*) FROM _oo_docs.fact WHERE status = 'retracted'") or 0) - retracted_before)
        if not done:
            ev.write("a8-failure-diagnostics.txt", "engine log (outdated, parser, reprocess lines):\n" + "\n".join(
                [l[:300] for l in sh(["docker", "logs", "--tail", "400", f"{project}-docs-engine-1"]).stdout.splitlines() + sh(["docker", "logs", "--tail", "400", f"{project}-docs-engine-1"]).stderr.splitlines()
                 if any(k in l for k in ("outdated", "parser", "reprocess", "set_current"))][-60:]) + "\n\nfiles:\n" +
                psql("SELECT path || ' ' || state || ' ' || coalesce(state_code,'') FROM _oo_docs.file ORDER BY path") + "\n\nparse_result:\n" +
                psql("SELECT parser_id || ' ' || parser_version || ' ' || status || ' ' || count(*) FROM _oo_docs.parse_result GROUP BY 1") + "\n")
        st, diffs = api.request("GET", "/v1/docs/review?kind=reprocess_diff&status=pending&limit=50")
        ev.check("A8 the sidecar files were reprocessed with the new parser version", bool(done), f"{'seen outdated' if seen_not_published else 'state not observed'}")
        ev.check("A8 the old facts stayed published during the rerun", low >= len(before), f"min {low} of {len(before)}")
        ev.check("A8 no churn: same fact keys after, nothing retracted", after == before and retracted in ("0", ""), f"{len(after)} facts, retracted {retracted or 0}")
        held = diffs.get("items", []) if st == 200 else []
        ev.check("A8 DOC-G10 judged the reruns: whatever it held is a reprocess_diff item for a person and nothing was retracted behind it", st == 200 and retracted in ("0", ""),
                 f"{len(held)} held: " + ", ".join(str((h.get("source") or {}).get("path")) + " " + str(h.get("reason_code")) for h in held))
        ev.write("a8-held-items.json", json.dumps(held, ensure_ascii=False, indent=2) + "\n")
        # A10: a media job runs while a Markdown edit publishes. D49 (calibrated, 2026-11-17): a md file goes from detected
        # to published within 7 s (p95); measured here from the file's own state history, one sample, media job running.
        sh(["ffmpeg", "-nostdin", "-v", "error", "-y", "-stream_loop", "40", "-i", str(drive_root / "Meetings/周会 参会=小王、小李.wav"),
            "-c", "copy", str(drive_root / "Meetings/周会2 参会=小王、小李.wav")])
        media_busy = lambda: int(psql("SELECT count(*) FROM _oo_docs.job WHERE kind = 'parse_media' AND state IN ('queued','running')") or 0)
        started = l2.wait_until(120, lambda: media_busy() > 0, step=0.5)
        t_edit = time.time()
        media_at_edit = media_busy()
        seen = {"media": media_at_edit}
        (drive_root / "Projects/星火计划.md").write_text(md["Projects/星火计划.md"].replace("进行中", "收尾中"), encoding="utf-8")
        def edit_done():
            seen["media"] = max(seen["media"], media_busy())
            return state_of_now(api, did, "Projects/星火计划.md") == "published" and "收尾中" in json.dumps(api.request("POST", "/v1/docs/context-packs:compile", {"drive_id": did, "scope": ["Projects"]})[1], ensure_ascii=False)
        ok = l2.wait_until(D49_EDIT_SECONDS + 30, edit_done, step=0.5)
        el = time.time() - t_edit
        lat = psql("SELECT round(extract(epoch FROM (max(at) FILTER (WHERE to_state = 'published') - min(at)))::numeric, 2) FROM _oo_docs.file_state_history "
                   "WHERE file_id = (SELECT id FROM _oo_docs.file WHERE path = 'Projects/星火计划.md') AND at > to_timestamp(%f) - interval '1 second'" % t_edit)
        # Where the seconds went: the jobs and the file's states after the edit, relative to the edit (seconds).
        tl = psql("SELECT round(extract(epoch FROM (created_at - to_timestamp(%f)))::numeric, 2) || 's ' || kind || ' ' || state || ' run_at +' || "
                  "round(extract(epoch FROM (run_at - to_timestamp(%f)))::numeric, 2) || 's done +' || round(extract(epoch FROM (updated_at - to_timestamp(%f)))::numeric, 2) || 's' "
                  "FROM _oo_docs.job WHERE created_at > to_timestamp(%f) - interval '1 second' AND kind <> 'parse_media' ORDER BY created_at LIMIT 30" % (t_edit, t_edit, t_edit, t_edit))
        ev.write("a10-timeline.txt", f"edit at t0; job rows created after it:\n{tl}\n\nfile states of the edited file:\n" + psql(
            "SELECT round(extract(epoch FROM (at - to_timestamp(%f)))::numeric, 2) || 's ' || to_state FROM _oo_docs.file_state_history "
            "WHERE file_id = (SELECT id FROM _oo_docs.file WHERE path = 'Projects/星火计划.md') AND at > to_timestamp(%f) - interval '1 second' ORDER BY at" % (t_edit, t_edit)) + "\n")
        ev.timings["a10_edit_to_brief_seconds"] = round(el, 2)
        ev.timings["a10_detected_to_published_seconds"] = float(lat) if lat else None
        ev.check("A10 a media job was queued or running when the edit was made", started is not None and seen["media"] >= 1, f"{media_at_edit} at the edit, max {seen['media']}")
        ev.check("A10 the md file went from detected to published within the calibrated D49 bound (7 s) while media work ran", bool(ok) and bool(lat) and float(lat) <= D49_MD_SECONDS, f"{lat} s (edit to BRIEF {el:.1f}s)")
        ev.check("A10 the edit reached a new BRIEF within the L2 bound (30 s)", bool(ok) and el <= D49_EDIT_SECONDS, f"{el:.1f}s")
        ev.write("l6-e2e-result.json", json.dumps({"run_id": a.run_id, "checks": ev.checks, "timings": ev.timings}, ensure_ascii=False, indent=2) + "\n")
        ev.write("l6-e2e-log.txt", "\n".join(ev.log_lines) + "\n")
        return 0 if all(c["ok"] for c in ev.checks) else 1
    finally:
        if serve:
            serve.send_signal(signal.SIGTERM)
            try:
                serve.wait(timeout=60)
            except subprocess.TimeoutExpired:
                serve.kill()
        if not a.keep:
            sh(["docker", "compose", "-p", f"oo-docs-{__import__('hashlib').sha256(str(state.resolve()).encode()).hexdigest()[:12]}", "down", "-v"])
            l2.remove_tree(work)


def state_of_now(api, did, path):
    return (files(api, did).get(path) or {}).get("state")


if __name__ == "__main__":
    sys.exit(main())
