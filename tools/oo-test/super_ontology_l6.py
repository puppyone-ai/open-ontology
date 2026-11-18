#!/usr/bin/env python3
"""Super Ontology L1 cells `docs-ocr` and `docs-media` (B-S9-01, B-S10-05, 10 section 12.3, 17).

  tools/oo-test/super_ontology_l6.py --cells docs-ocr,docs-media [--run-id ID] [--evidence-dir DIR] [--keep]

Each cell starts one labelled container (`oo.lane=super-ontology-be6`) with `--network none --read-only` from the local
image tag, drives its HTTP API from inside the container with the standard library, checks the answers and removes the
container. Before it starts it waits while another Super Ontology container stack is up (`docker ps --filter label=oo.lane`).
Evidence: <evidence-dir>/l6-<cell>.txt and l6-cells.json.

docs-ocr: a scanned pdf page and a standalone image come back with OCR blocks; the sidecar cannot reach any host (no egress).
docs-media: `ffmpeg -buildconf` has no gpl, nonfree or forbidden decoder (ffmpeg_gate); an AAC file ends as
PARSER_CODEC_UNAVAILABLE with the codec and how to configure; a Opus/FLAC speech file is parsed to utterances with
speaker labels S1..; no egress; job scratch (chunks, features) is gone after the file.
"""
import argparse
import json
import re
import secrets
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
LANE = "super-ontology-be6"
TOKEN = "l6-" + secrets.token_hex(12)

CLIENT = r'''
import http.client, json, hashlib, sys, time, socket
TOK = sys.argv[1]; PORT = int(sys.argv[2])
def call(m, p, body=None, timeout=900):
    c = http.client.HTTPConnection("127.0.0.1", PORT, timeout=timeout)
    c.request(m, p, body=body, headers={"Authorization": "Bearer " + TOK, "Content-Type": "application/json"})
    r = c.getresponse(); return r.status, r.read()
def put(path):
    d = open(path, "rb").read(); sha = hashlib.sha256(d).hexdigest()
    assert call("PUT", f"/v1/blobs/{sha}", d)[0] in (200, 201); return sha
def egress():
    for host in ("1.1.1.1", "8.8.8.8"):
        s = socket.socket(); s.settimeout(3)
        try:
            s.connect((host, 53)); return False
        except OSError: pass
        finally: s.close()
    try: socket.getaddrinfo("example.com", 80); return False
    except OSError: return True
def wait_task(t):
    while True:
        st, b = call("GET", "/v1/tasks/" + t); j = json.loads(b)
        if j["state"] != "running": return j
        time.sleep(0.5)
'''

OCR_CASE = r'''
exec(open("/tmp/client.py").read())
st, b = call("GET", "/v1/info"); info = json.loads(b); assert info["parser_id"] == "paddleocr" and info["profile"] == "cpu", info
assert {x["spdx"] for x in info["licenses"]} == {"Apache-2.0"}
out = {"egress_blocked": egress()}
for path, fmt in (("/tmp/scan.pdf", "pdf"), ("/tmp/scan.png", "image")):
    sha = put(path); t = time.time()
    st, b = call("POST", "/v1/parse", json.dumps({"blob": sha, "format": fmt, "options": {"ocr": True}}).encode())
    j = json.loads(b); assert st == 200, (st, b[:300])
    texts = "".join(blk["text"] for p in j["page_list"] for blk in p["blocks"])
    out[fmt] = {"seconds": round(time.time() - t, 1), "blocks": sum(len(p["blocks"]) for p in j["page_list"]), "has_text": "计划会" in texts or "纪要" in texts}
    assert out[fmt]["has_text"], texts[:200]
    if fmt == "pdf": assert j["page_list"][0]["pt_w"] > 0
print("RESULT " + json.dumps(out))
assert out["egress_blocked"]
'''

MEDIA_CASE = r'''
exec(open("/tmp/client.py").read())
import subprocess, os
st, b = call("GET", "/v1/info"); info = json.loads(b); assert info["parser_id"] == "media", info
names = {x["name"] for x in info["licenses"] if "spdx" in x}
assert "qwen-asr" not in names and "pyannote.audio" not in names
g = subprocess.run(["python", "-m", "media_parser.ffmpeg_gate", "/opt/ffmpeg/bin/ffmpeg"], capture_output=True, text=True, cwd="/opt/app")
out = {"ffmpeg_gate": g.stdout.strip().splitlines()[-1], "egress_blocked": egress()}
assert g.returncode == 0, g.stdout
# AAC in m4a: no decoder path
sha = put("/tmp/aac.m4a")
st, b = call("POST", "/v1/probe", json.dumps({"blob": sha, "format": "audio"}).encode()); p = json.loads(b)
assert p["decode"] == "restricted" and p["restricted_codecs"] == ["aac"], p
st, b = call("POST", "/v1/parse", json.dumps({"blob": sha, "format": "audio"}).encode()); assert st == 202, (st, b)
j = wait_task(json.loads(b)["task"]); assert j["state"] == "failed" and j["error"]["code"] == "PARSER_CODEC_UNAVAILABLE", j
assert j["error"]["details"]["codecs"] == ["aac"] and "media.external_ffmpeg" in j["error"]["details"]["how_to"]
out["codec_unavailable"] = j["error"]["details"]["codecs"]
# speech in opus: parsed to utterances
sha = put("/tmp/speech.opus"); t = time.time()
st, b = call("POST", "/v1/parse", json.dumps({"blob": sha, "format": "audio"}).encode()); assert st == 202
j = wait_task(json.loads(b)["task"]); assert j["state"] == "done", j
r = j["result"]; items = r["items"]
out["speech"] = {"seconds": round(time.time() - t, 1), "utterances": len(items), "speakers": r["speakers"], "decode": r["decode"], "text": "".join(i["text"] for i in items)[:60]}
assert items and all(i["kind"] == "utterance" and i["speaker"].startswith("S") for i in items)
assert not os.path.exists("/scratch/media-jobs/" + sha), "job directory left behind"
print("RESULT " + json.dumps(out, ensure_ascii=False))
assert out["egress_blocked"]
'''


def sh(cmd, **kw):
    return subprocess.run(cmd, capture_output=True, text=True, **kw)


def wait_for_free():
    while True:
        names = sh(["docker", "ps", "--filter", "label=oo.lane", "--format", "{{.Names}} {{.Label \"oo.lane\"}}"]).stdout
        if not [l for l in names.splitlines() if "super-ontology" in l.split(" ")[-1] and "be6" not in l]:
            return
        time.sleep(20)


def make_inputs(d):
    """Synthetic inputs made on the host: AAC m4a, opus speech (macOS `say`, else a tone), scanned page images."""
    ff = ["ffmpeg", "-nostdin", "-v", "error", "-y"]
    sh(ff + ["-f", "lavfi", "-i", "sine=frequency=440:duration=2", "-c:a", "aac", str(d / "aac.m4a")], check=True)
    say = d / "s.aiff"
    if sh(["which", "say"]).returncode == 0:
        sh(["say", "-v", "Tingting", "-o", str(say), "各位好，现在开始本周的项目例会，今天主要讨论星图看板的上线安排。"], check=True)
        sh(ff + ["-i", str(say), "-ac", "1", "-ar", "16000", "-c:a", "libopus", str(d / "speech.opus")], check=True)
    else:
        sh(ff + ["-f", "lavfi", "-i", "sine=frequency=300:duration=3", "-c:a", "libopus", str(d / "speech.opus")], check=True)


def run_cell(name, run_id, evidence, keep):
    image = {"docs-ocr": "oo-docs-ocr-parser:dev", "docs-media": "oo-docs-media-parser:dev"}[name]
    port = 7482 if name == "docs-ocr" else 7483
    cname = f"oo-{LANE}-{name}-{run_id}"
    log, status, started = [f"# {name} image {image}\n"], "failed", time.time()
    wait_for_free()
    tmp = Path(tempfile.mkdtemp(prefix="l6-"))
    try:
        mem = "10g" if name == "docs-ocr" else "4g"
        sh(["docker", "rm", "-f", cname])
        run = sh(["docker", "run", "-d", "--name", cname, "--label", f"oo.lane={LANE}", "--network", "none", "--read-only",
                 "--cap-drop", "ALL", "--security-opt", "no-new-privileges:true", "--tmpfs", "/tmp:size=1g,mode=1777",
                 "--tmpfs", "/scratch:uid=10001,size=2g", "--memory", mem, "--cpus", "4", "-e", f"OO_PARSER_TOKEN={TOKEN}", image])
        if run.returncode:
            raise RuntimeError(run.stderr.strip())
        time.sleep(4)
        files = {"/tmp/client.py": CLIENT}
        if name == "docs-ocr":
            sys.path.insert(0, str(REPO / "tools/ocr-parser"))
            from PIL import Image

            page = Image.open(_page(tmp)).convert("RGB")
            page.save(tmp / "scan.png")
            page.save(tmp / "scan.pdf", resolution=150)
            (tmp / "page.png").unlink()
        else:
            make_inputs(tmp)
        for path, text in files.items():
            sh(["docker", "exec", "-i", cname, "sh", "-c", f"cat > {path}"], input=text)
        for f in tmp.iterdir():
            with open(f, "rb") as fh:
                subprocess.run(["docker", "exec", "-i", cname, "sh", "-c", f"cat > /tmp/{f.name}"], stdin=fh, check=True)
        case = OCR_CASE if name == "docs-ocr" else MEDIA_CASE
        res = sh(["docker", "exec", "-i", "-w", "/opt/app", "-e", "PYTHONPATH=/opt/app", cname, "python", "-", TOKEN, str(port)], input=f"import sys; sys.argv = ['x', '{TOKEN}', '{port}']\n" + case)
        log.append(res.stdout + res.stderr[-2000:])
        mem_line = sh(["docker", "stats", "--no-stream", cname, "--format", "mem {{.MemUsage}}"]).stdout.strip()
        log.append(mem_line + "\n")
        status = "passed" if res.returncode == 0 and "RESULT" in res.stdout else "failed"
    except Exception as e:  # noqa: BLE001
        log.append(f"ERROR {e}\n")
    finally:
        if not keep:
            sh(["docker", "rm", "-f", cname])
    log = [re.sub(TOKEN, "<token>", l) for l in log]
    (evidence / f"l6-{name}.txt").write_text("".join(log) + f"# status {status} in {time.time() - started:.0f}s\n")
    return {"cell": name, "status": status, "seconds": round(time.time() - started), "image": image}


def _page(tmp):
    from PIL import Image, ImageDraw, ImageFont

    img = Image.new("RGB", (1240, 400), "white")
    d = ImageDraw.Draw(img)
    for f in ("/System/Library/Fonts/Songti.ttc", "/usr/share/fonts/opentype/noto/NotoSerifCJK-Regular.ttc"):
        if Path(f).exists():
            d.text((60, 120), "2026年度计划会纪要", font=ImageFont.truetype(f, 60), fill="black")
            break
    p = tmp / "page.png"
    img.save(p)
    return p


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--cells", default="docs-ocr,docs-media")
    ap.add_argument("--run-id", default=time.strftime("l6-%Y%m%d-%H%M%S"))
    ap.add_argument("--evidence-dir", default=str(REPO / "testing/super-ontology/evidence/L6"))
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args(argv)
    ev = Path(a.evidence_dir)
    ev.mkdir(parents=True, exist_ok=True)
    rows = [run_cell(n, a.run_id, ev, a.keep) for n in a.cells.split(",") if n]
    (ev / "l6-cells.json").write_text(json.dumps({"run_id": a.run_id, "cells": rows}, indent=2) + "\n")
    for r in rows:
        print(json.dumps(r), flush=True)
    return 0 if all(r["status"] == "passed" for r in rows) else 1


if __name__ == "__main__":
    sys.exit(main())
