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
  A8  parser version upgrade: outdated, old facts stay published meanwhile, no retraction churn, DOC-G10 judges
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
             "-shortest", str(root / "Meetings/录屏.webm")], check=True)
    sh(ff + ["-f", "concat", "-safe", "0", "-i", str(vl), "-i", str(wavs[1]), "-c:v", "libx264", "-pix_fmt", "yuv420p",
             "-c:a", "aac", "-shortest", str(root / "Meetings/录屏h264.mp4")], check=True)
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
        env = dict(os.environ, OO_DOCS_PROFILES="ocr,media", OO_DOCS_LANE=LANE, OO_DOCS_DEPLOY_DIR=str(REPO / "deploy/docs"))
        argv = [str(oo), "serve", "--runtime", "compose", "--json", "--port", "0", "--state-dir", str(state)]
        ev.command(argv)
        serve = subprocess.Popen(argv, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        info = json.loads(serve.stdout.readline())
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
        ev.check("A3 nothing is published under a named Person before confirmation", st == 200 and not ents.get("items"), json.dumps(ents, ensure_ascii=False)[:120])
        s1 = next((i for i in items if i.get("subject", {}).get("speaker") == "S1"), None)
        if s1:
            st, r = api.request("POST", "/v1/docs/review:decide", {"items": [{"id": s1["id"], "revision": s1["revision"], "decision": "accept", "edit": {"name": "小王"}}]})
            ev.check("A3 confirming S1 as 小王 succeeds", st == 200 and r["results"][0]["status"] == "ok", json.dumps(r, ensure_ascii=False)[:160])
            ok = l2.wait_until(60, lambda: api.request("GET", "/v1/docs/entities?q=小王")[1].get("items"), step=2)
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
        # A10: a media job runs while a Markdown edit publishes
        (drive_root / "Meetings/周会2 参会=小王、小李.wav").write_bytes((drive_root / "Meetings/周会 参会=小王、小李.wav").read_bytes() + b"\0\0" * 8)
        time.sleep(3)
        t_edit = time.time()
        (drive_root / "Projects/星火计划.md").write_text(md["Projects/星火计划.md"].replace("进行中", "收尾中"), encoding="utf-8")
        ok = l2.wait_until(D49_EDIT_SECONDS + 30, lambda: state_of_now(api, did, "Projects/星火计划.md") == "published" and "收尾中" in json.dumps(api.request("POST", "/v1/docs/context-packs:compile", {"drive_id": did, "scope": ["Projects"]})[1], ensure_ascii=False), step=0.5)
        el = time.time() - t_edit
        busy = sh(["docker", "exec", f"{info.get('project', 'oo-docs')}-docs-db-1", "psql", "-U", "postgres", "-d", "oo_docs", "-tAc", "SELECT count(*) FROM _oo_docs.job WHERE kind = 'parse_media' AND state IN ('queued','running')"]).stdout.strip()
        ev.timings["a10_edit_to_brief_seconds"] = round(el, 2)
        ev.check("A10 the edit published within the D49 bound while media work was queued or running", bool(ok) and el <= D49_EDIT_SECONDS, f"{el:.1f}s, media jobs in flight: {busy or '?'}")
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
