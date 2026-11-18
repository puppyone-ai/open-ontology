#!/usr/bin/env python3
"""Super Ontology L6 long runs on the compose stack (30 section L6 acceptance 1 and 3; B-L6rem).

  /usr/bin/python3 tools/oo-test/super_ontology_l6_long.py --suite meeting2h|ocr44 [--run-id ID] [--evidence-dir DIR]

Both suites start the real `oo serve --runtime compose` (profiles `ocr` and `media`) and register one Drive.

meeting2h  A 2 hour synthetic meeting (macOS `say` voices: the golden 3 person meeting repeated, with 12 planted
           statements "我是X。我负责Y。" between the repeats) is written as Opus in an .ogg (a 2 h 16 kHz wav is 230 MB, over the
           200 MiB file limit) and goes through the real pipeline: media sidecar (VAD, ASR, diarization), rule
           extraction, publish. Reports core type recall (names the golden meeting says that the engine's transcript carries;
           threshold 0.6), informationally how many planted introductions became `owns` facts, wall time, and the
           peak memory of every container (`docker stats`, 2 s).
           Run it alone: it holds the media sidecar for about a quarter of an hour (2 h of audio, Apple silicon).
ocr44      The 44 file scanned subset of the golden set (every source file as an image only pdf of degraded page
           bitmaps, rendered by tools/ocr-parser/scanned_subset.py inside the ocr-parser image) through the same
           stack. Core recall is the sidecar definition (an entity counts when its name or an alias is in the text
           blocks the engine stored for one of its anchor files); threshold 0.65; every file must be `published`.

Waits while another Super Ontology compose stack is up. Inputs and outputs of the containers stay under /tmp and are
removed at the end. No credential is written to the evidence.
"""
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent / "ocr-parser"))
import super_ontology_l2 as l2  # noqa: E402
import super_ontology_l6_e2e as e2e  # noqa: E402

REPO = l2.REPO
LANE = e2e.LANE
sh = e2e.sh

PLANTED = [  # (speaker name, project); every project name is a rare word the rule must carry through ASR
    ("小王", "星火计划"), ("小李", "北辰项目"), ("小张", "青云计划"), ("小陈", "蓝海项目"),
    ("小赵", "银河计划"), ("小周", "远航项目"), ("小吴", "磐石计划"), ("小郑", "启明项目"),
    ("小孙", "晨曦计划"), ("小钱", "飞鸿项目"), ("小冯", "凌云计划"), ("小褚", "天枢项目"),
]
VOICES = ["Tingting", "Sandy (中文（中国大陆）)", "Rocko (中文（中国大陆）)", "Flo (中文（中国大陆）)"]
FILLER = ["好的，我们继续讨论下一个议题。", "目前进度正常，暂时没有阻塞。", "这件事我们会后再对一下时间。",
          "大家有没有其他需要补充的地方。", "我记一下，下周同步结果。"]
THRESHOLD_MEETING = 0.6
THRESHOLD_OCR = 0.65
D4 = {"hours": 2.0, "gap_seconds": 600}


def golden_names():
    """Core-type names the 3 person golden meeting says: golden entities whose name or alias is in the script."""
    g = REPO / "testing/super-ontology/golden"
    script = json.loads((g / "media/script.json").read_text())
    said = "".join(t[1] for t in script["turns"])
    names = {}
    for line in (g / "labels/entities.jsonl").read_text().splitlines():
        if not line.strip():
            continue
        e = json.loads(line)
        for n in [e["name"]] + e.get("aliases", []):
            if len(n) >= 2 and n in said:
                names[e["name"]] = e["type"]
    return names, script


def build_meeting(out: Path, work: Path, hours: float):
    """The golden 3 person meeting (96 s, turns spoken with pauses) repeated, with the 12 planted statements between
    the repeats; the same material as tools/media-parser/long_audio_check.py plus statements that carry facts."""
    ff = ["ffmpeg", "-nostdin", "-v", "error", "-y"]
    names, script = golden_names()
    truth = json.loads((REPO / "testing/super-ontology/golden/media/meeting-3spk.truth.json").read_text())

    def speak(voice, text, name, pad_ms):
        a, w = work / f"{name}.aiff", work / f"{name}.wav"
        sh(["say", "-v", voice, "-o", str(a), text], check=True)
        sh(ff + ["-i", str(a), "-af", f"adelay={pad_ms}:all=1,apad=pad_dur={pad_ms / 1000}", "-ac", "1", "-ar", "16000", str(w)], check=True)
        return w

    turns = []
    for i, (spk, text) in enumerate(script["turns"]):
        turns.append(speak(truth["voices"][spk], text, f"g{i}", 350))
    lst = work / "golden.txt"
    lst.write_text("".join(f"file '{p}'\n" for p in turns))
    golden = work / "golden.wav"
    sh(ff + ["-f", "concat", "-safe", "0", "-i", str(lst), "-c:a", "pcm_s16le", str(golden)], check=True)
    glen = float(sh(["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", str(golden)]).stdout.strip())
    planted = []
    for k, (name, proj) in enumerate(PLANTED):
        planted.append(speak(VOICES[k % len(VOICES)], f"大家好，我是{name}。我负责{proj}。", f"p{k}", 1200))
    total = int(hours * 3600)
    copies_total = max(13, int((total - 12 * 8) / glen))
    per_gap = max(1, copies_total // 13)
    items = []
    for k in range(13):
        items += [golden] * per_gap
        if k < 12:
            items.append(planted[k])
    full = work / "meeting.txt"
    full.write_text("".join(f"file '{p}'\n" for p in items))
    sh(ff + ["-f", "concat", "-safe", "0", "-i", str(full), "-c:a", "libopus", "-b:a", "24k", "-ac", "1", "-ar", "16000",
             "-t", str(total), str(out)], check=True)
    return names, glen, per_gap


class MemSampler(threading.Thread):
    """Peak memory per container of the project, from `docker stats` every 2 s."""

    def __init__(self, project):
        super().__init__(daemon=True)
        self.project, self.peak, self.stop = project, {}, threading.Event()

    @staticmethod
    def mib(text):
        m = re.match(r"([\d.]+)\s*([KMG]i?B)", text.strip())
        if not m:
            return 0.0
        return float(m.group(1)) * {"KiB": 1 / 1024, "KB": 1 / 1024, "MiB": 1, "MB": 1, "GiB": 1024, "GB": 1024}[m.group(2)]

    def run(self):
        while not self.stop.is_set():
            out = sh(["docker", "stats", "--no-stream", "--format", "{{.Name}}\t{{.MemUsage}}"]).stdout
            for line in out.splitlines():
                name, _, usage = line.partition("\t")
                if name.startswith(self.project):
                    v = self.mib(usage.split("/")[0])
                    short = name[len(self.project) + 1:]
                    self.peak[short] = max(self.peak.get(short, 0.0), v)
            self.stop.wait(2)


def boot(ev, work, drive_root, tag):
    state = work / "state"
    state.mkdir(mode=0o700)
    (state / "engine").mkdir(mode=0o700)
    (state / "engine/docs.json").write_text(json.dumps({"version": "oo.docs/v1", "sync": {"fs_poll_seconds": 5}}))
    target = Path(os.environ.get("CARGO_TARGET_DIR", REPO / "target")) / "debug"
    oo = target / "oo"
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
    ev.check(f"{tag} compose stack is up and healthy", st == 200 and h.get("status") in ("ok", "degraded"), f"HTTP {st}")
    st, d = api.request("POST", "/v1/docs/drives", {"name": tag, "backend": "fs", "config": {"root": str(drive_root)}})
    assert st == 201, d
    return serve, state, project, api, d["id"]


def stop(serve, state, keep=False):
    serve.send_signal(signal.SIGTERM)
    try:
        serve.wait(timeout=90)
    except subprocess.TimeoutExpired:
        serve.kill()
    if not keep:
        import hashlib
        sh(["docker", "compose", "-p", f"oo-docs-{hashlib.sha256(str(state.resolve()).encode()).hexdigest()[:12]}", "down", "-v"])


WORKING = {"stored", "queued", "parsing", "parsed", "extracting", "indexed", "outdated"}


def settle(api, did, deadline):
    return l2.wait_until(deadline, lambda: (lambda f: f if f and not any(x["state"] in WORKING for x in f.values()) else None)(e2e.files(api, did)), step=10)


def suite_meeting(ev, a):
    work = Path(tempfile.mkdtemp(prefix=f"oo-l6long-{a.run_id}-", dir="/tmp"))
    drive_root = work / "drive"
    (drive_root / "Meetings").mkdir(parents=True)
    serve = sampler = None
    try:
        t0 = time.time()
        names, glen, per_gap = build_meeting(drive_root / "Meetings/季度例会.ogg", work, a.hours)
        size = (drive_root / "Meetings/季度例会.ogg").stat().st_size
        ev.log(f"meeting built: {a.hours} h, {size / 1e6:.1f} MB opus ({time.time() - t0:.0f} s)")
        e2e.wait_for_free(ev)
        serve, state, project, api, did = boot(ev, work, drive_root, "meeting2h")
        sampler = MemSampler(project)
        sampler.start()
        t1 = time.time()
        fs = settle(api, did, a.timeout)
        wall = time.time() - t1
        fs = fs or e2e.files(api, did)
        f = fs.get("Meetings/季度例会.ogg") or {}
        ev.check("meeting2h the recording reached a final state", f.get("state") not in WORKING and bool(f), f"{f.get('state')} {f.get('state_code') or ''}")
        ev.check("meeting2h the recording is published", f.get("state") == "published", f"{f.get('state')} {f.get('state_code') or ''}")
        db = f"{project}-docs-db-1"
        psql = lambda q: sh(["docker", "exec", db, "psql", "-U", "postgres", "-d", "oo_docs", "-tAc", q]).stdout.strip()
        rows = psql("SELECT DISTINCT o.canonical_name FROM _oo_docs.fact ft JOIN _oo_docs.entity o ON o.id = ft.object_id "
                    "JOIN _oo_docs.evidence e ON e.target_kind = 'fact' AND e.target_id = ft.id AND e.withdrawn_at IS NULL "
                    "JOIN _oo_docs.file fl ON fl.current_version_id = e.version_id "
                    "WHERE ft.predicate = 'owns' AND fl.path = 'Meetings/季度例会.ogg' AND ft.status IN ('draft','published')").splitlines()
        found = [p for _, p in PLANTED if p in rows]
        recall_rule = len(found) / len(PLANTED)
        # The transcript the ASR produced, for the cases where recall is low (what the rules saw).
        lines, after = [], None
        for _ in range(60):
            st, b = api.request("GET", f"/v1/docs/files/{f['id']}/blocks?limit=500" + (f"&cursor={after}" if after else "")) if f.get("id") else (0, {})
            if st != 200:
                break
            lines += [(x.get("anchor", {}), x.get("text", "")) for x in b.get("items", [])]
            after = b.get("next_cursor")
            if not after:
                break
        ev.write("l6-long-meeting-transcript.txt", "\n".join(f"{json.dumps(a, ensure_ascii=False)}\t{t}" for a, t in lines) + "\n")
        ev.log(f"transcript blocks: {len(lines)}")
        sys.path.insert(0, str(HERE.parent / "ocr-parser"))
        import scanned_subset as ss
        text_all = ss.norm("".join(t for _, t in lines))
        seen_names = {n: ty for n, ty in names.items() if ss.norm(n) in text_all}
        recall = len(seen_names) / len(names) if names else 0.0
        ev.check(f"meeting2h core type recall >= {THRESHOLD_MEETING} through the real pipeline (names the golden meeting says that the engine's transcript carries)",
                 recall >= THRESHOLD_MEETING, f"{len(seen_names)}/{len(names)} = {recall:.3f}; missed {sorted(set(names) - set(seen_names))}")
        heard = [p for _, p in PLANTED if p in text_all]
        # Informational (always passes): the rule path needs sentence boundaries that the ASR text does not have, see README.
        ev.check("meeting2h INFO planted self introductions: how many the ASR heard correctly and how many of those became `owns` facts (rules only, no model)",
                 True,
                 f"facts {len(found)}/{len(PLANTED)} = {recall_rule:.3f}; heard correctly {len(heard)}/{len(PLANTED)}, of those with a fact {len([p for p in heard if p in found])}; "
                 f"heard but no fact {[p for p in heard if p not in found]}")
        sampler.stop.set()
        peak = {k: round(v) for k, v in sorted(sampler.peak.items())}
        ev.timings.update({"meeting_hours": a.hours, "opus_bytes": size, "pipeline_wall_seconds": round(wall, 1), "peak_memory_mib": peak})
        ev.log(f"peak memory (MiB) per container: {json.dumps(peak)}")
        ev.check("meeting2h peak memory recorded", bool(peak), json.dumps(peak))
        ev.write("l6-long-meeting.json", json.dumps({"run_id": a.run_id, "core_type_recall": round(recall, 4), "names_expected": names, "names_found": sorted(seen_names),
                                                       "rule_fact_recall_of_planted_statements": round(recall_rule, 4), "planted": len(PLANTED), "planted_found": found,
                                                       "golden_meeting_seconds": round(glen, 1), "golden_copies_per_gap": per_gap,
                                                       "state": f.get("state"), "timings": ev.timings}, ensure_ascii=False, indent=2) + "\n")
    finally:
        if sampler:
            sampler.stop.set()
        if serve:
            stop(serve, work / "state", a.keep)
        l2.remove_tree(work)


def suite_ocr(ev, a):
    import scanned_subset as ss  # noqa: E402  (stdlib only at import time)
    work = Path(tempfile.mkdtemp(prefix=f"oo-l6ocr-{a.run_id}-", dir="/tmp"))
    drive_root = work / "drive"
    drive_root.mkdir()
    serve = sampler = None
    try:
        lock = json.loads((REPO / "deploy/docs/images.lock.json").read_text())
        golden = REPO / "testing/super-ontology/golden"
        pages = work / "pages"
        pages.mkdir()
        # The page renderer draws with a system CJK font; the image has none, so the host's is made visible to it
        # (Docker cannot share /System, a copy in the work directory is).
        fonts = work / "fonts"
        fonts.mkdir()
        for f in ("STHeiti Medium.ttc", "Songti.ttc"):
            if (Path("/System/Library/Fonts") / f).exists():
                shutil.copy(Path("/System/Library/Fonts") / f, fonts / f)
        code = (
            "import sys; sys.path.insert(0, '/tools'); import scanned_subset as s; from pathlib import Path\n"
            "s.render(Path('/golden'), Path('/pages'))\n"
            "from PIL import Image\n"
            "by = {}\n"
            "for p in sorted(Path('/pages').glob('*.png')):\n"
            "    by.setdefault(p.name.rsplit('__p', 1)[0], []).append(p)\n"
            "for stem, ps in by.items():\n"
            "    ims = [Image.open(p).convert('RGB') for p in sorted(ps, key=lambda q: int(q.stem.rsplit('__p', 1)[1]))]\n"
            "    ims[0].save(Path('/out') / (stem + '.pdf'), save_all=True, append_images=ims[1:], resolution=200)\n"
            "print(len(by), 'pdfs')\n"
        )
        p = sh(["docker", "run", "--rm", "--network", "none", "--label", f"oo.lane={LANE}", "--entrypoint", "python3",
                "-v", f"{fonts}:/System/Library/Fonts:ro", "-v", f"{golden}:/golden:ro", "-v", f"{REPO / 'tools/ocr-parser'}:/tools:ro", "-v", f"{pages}:/pages", "-v", f"{drive_root}:/out",
                lock["ocr_parser"]["tag"], "-c", code])
        ev.log((p.stdout + p.stderr).strip()[-300:])
        n_files = len(list(drive_root.glob("*.pdf")))
        ev.check("ocr44 the subset has 44 files", n_files == 44, f"{n_files} image only pdfs")
        e2e.wait_for_free(ev)
        serve, state, project, api, did = boot(ev, work, drive_root, "ocr44")
        sampler = MemSampler(project)
        sampler.start()
        t1 = time.time()
        fs = settle(api, did, a.timeout)
        wall = time.time() - t1
        fs = fs or e2e.files(api, did)
        by_state = {}
        for x in fs.values():
            by_state[x["state"]] = by_state.get(x["state"], 0) + 1
        ev.check("ocr44 every file reached a final state", bool(fs) and not any(s in WORKING for s in by_state), json.dumps(by_state))
        ev.check("ocr44 every file is published", by_state.get("published") == n_files, json.dumps(by_state))
        texts = {}
        for path, x in fs.items():
            if x["state"] != "published":
                continue
            acc, after = "", None
            for _ in range(20):
                st, b = api.request("GET", f"/v1/docs/files/{x['id']}/blocks?limit=500" + (f"&cursor={after}" if after else ""))
                if st != 200:
                    break
                acc += "".join(i.get("text", "") for i in b.get("items", []))
                after = b.get("next_cursor") or b.get("next")
                if not after:
                    break
            texts[path[:-4]] = ss.norm(acc)
        ents = [json.loads(l) for l in (golden / "labels/entities.jsonl").read_text().splitlines() if l.strip()]
        scored = hits = 0
        by_type, missed = {}, []
        for e in ents:
            files_ = {x["file"].replace("/", "__") for x in e["anchors"]} & set(texts)
            if not files_:
                continue
            scored += 1
            names = [ss.norm(e["name"])] + [ss.norm(x) for x in e.get("aliases", [])]
            ok = any(nm and nm in texts[f] for nm in names for f in files_)
            t = by_type.setdefault(e["type"], [0, 0])
            t[1] += 1
            if ok:
                hits += 1
                t[0] += 1
            else:
                missed.append(e["name"])
        recall = hits / scored if scored else 0.0
        sampler.stop.set()
        peak = {k: round(v) for k, v in sorted(sampler.peak.items())}
        ev.check(f"ocr44 core recall >= {THRESHOLD_OCR} end to end", recall >= THRESHOLD_OCR,
                 f"{hits}/{scored} = {recall:.4f}; missed {missed[:8]}")
        ev.timings.update({"pipeline_wall_seconds": round(wall, 1), "peak_memory_mib": peak})
        ev.write("l6-long-ocr44.json", json.dumps({"run_id": a.run_id, "files": n_files, "by_state": by_state, "entities_scored": scored,
                                                     "entities_found": hits, "core_type_recall": round(recall, 4),
                                                     "by_type": {k: f"{v[0]}/{v[1]}" for k, v in sorted(by_type.items())}, "missed": missed,
                                                     "timings": ev.timings}, ensure_ascii=False, indent=2) + "\n")
    finally:
        if sampler:
            sampler.stop.set()
        if serve:
            stop(serve, work / "state", a.keep)
        l2.remove_tree(work)


def main(argv=None):
    import argparse
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--suite", required=True, choices=["meeting2h", "ocr44"])
    ap.add_argument("--run-id", default=time.strftime("l6long-%Y%m%d-%H%M%S"))
    ap.add_argument("--evidence-dir", default=str(REPO / "testing/super-ontology/evidence/L6"))
    ap.add_argument("--hours", type=float, default=D4["hours"])
    ap.add_argument("--timeout", type=int, default=6 * 3600)
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args(argv)
    ev = l2.Evidence(Path(a.evidence_dir), a.run_id)
    ev.log(f"# Super Ontology L6 long run {a.suite}, run {a.run_id}")
    (suite_meeting if a.suite == "meeting2h" else suite_ocr)(ev, a)
    ev.write(f"l6-long-{a.suite}-log.txt", "\n".join(ev.log_lines) + "\n")
    return 0 if all(c["ok"] for c in ev.checks) else 1


if __name__ == "__main__":
    sys.exit(main())
