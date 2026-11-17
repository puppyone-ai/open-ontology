#!/usr/bin/env python3
"""Super Ontology L3 "read the files" acceptance against the real stack
(30 section L3 acceptance 1, 3, 4, 5 (fs half), 8, 9, 10).

The stack is `oo serve --runtime compose` (docs-db, docs-engine and the Docling
doc-parser in containers, the daemon on the host), over fs Drives made from
the synthetic golden set and the hostile fixtures.

  acceptance 1   golden set, per format parse success >= 98 %, every failure has
                 an error code from errors.v1.json; the hostile fixtures get the
                 state and code of 10 4.4 (encrypted, legacy office, zip bomb ...)
  acceptance 3   poison: a PDF that kills the sidecar worker every time is
                 quarantined after at most 3 attempts, the other files publish
  acceptance 4   10 copies of one content: parsed once, 1 file published and 9
                 `duplicate`, each path with its own evidence
  acceptance 5   (fs) add, modify, rename, delete with the right states and evidence
  acceptance 8   files written straight into the Drive directory (no API call):
                 states follow; single file latency vs D49 (md/txt p95 <= 5 s,
                 pdf/docx <= 30 s, xlsx <= 20 s), measured with a 1 s poll and,
                 with the default 30 s poll, with the targeted sync hint the
                 Desktop watcher sends (D46)
  acceptance 9   a targeted sync does not sweep (generation and missing files
                 untouched); the seven freshness states are observed
  acceptance 10  a format-only DOCX change does not run the extraction again and
                 the evidence moves to the new version

Usage: tools/oo-test/super_ontology_l3.py [--run-id ID] [--evidence-dir DIR]
       [--only golden,dups,poison,life,reuse,latency,hint,targeted,fresh]
       [--skip-build] [--keep]

Needs docker compose, the images of deploy/docs/images.lock.json and a built
`oo` (cargo build -p oo-cli --features docs). Removes every container, network
and volume it made. Evidence: testing/super-ontology/evidence/L3/ (no tokens).
"""

import argparse
import hashlib
import io
import json
import os
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import threading
import time
import zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import super_ontology_compose as comp  # noqa: E402
import super_ontology_l2 as l2  # noqa: E402

REPO = l2.REPO
sys.path.insert(0, str(REPO / "testing/super-ontology/fixtures"))
import variants  # noqa: E402

GOLDEN = REPO / "testing/super-ontology/golden/docs"
FIXTURES = REPO / "testing/super-ontology/fixtures/files"
DEPLOY = REPO / "deploy/docs"
LANE = comp.LANE
FINAL = {"published", "duplicate", "unsupported", "encrypted", "parse_failed", "too_large", "extract_partial"}
FORMATS = ["md", "txt", "csv", "xlsx", "docx", "pptx", "pdf"]
ERROR_CODES = set(re.findall(r'"(DOCS_[A-Z_]+)"', (REPO / "contracts/docs-api/errors.v1.json").read_text()))
D49 = {"md/txt": 5.0, "pdf": 30.0, "docx": 30.0, "xlsx": 20.0}

HOSTILE = {  # fixture -> (state, code)
    "docx-encrypted.docx": ("encrypted", "DOCS_PARSE_ENCRYPTED"),
    "pdf-encrypted.pdf": ("encrypted", "DOCS_PARSE_ENCRYPTED"),
    "legacy.doc": ("unsupported", "DOCS_PARSE_LEGACY_OFFICE"),
    "zip-bomb-ratio.docx": ("unsupported", "DOCS_PARSE_ZIP_BOMB"),
    "zip-bomb-size.xlsx": ("unsupported", "DOCS_PARSE_ZIP_BOMB"),
    "zip-plain.zip": ("unsupported", "DOCS_PARSE_UNSUPPORTED_FORMAT"),
    "zip-traversal.docx": ("unsupported", "DOCS_PARSE_CORRUPT"),
    "gb18030.txt": ("unsupported", "DOCS_PARSE_ENCODING"),
    "pdf-table.pdf": ("published", None),
    "xlsx-merged-regions.xlsx": ("published", None),
    "xlsx-hidden-sheet.xlsx": ("published", None),
    "comma.csv": ("published", None),
    "semicolon.csv": ("published", None),
    "empty.csv": None,  # reported, not asserted
}

KILL_WORKERS = r"""
import os, signal
procs = {}
for d in os.listdir('/proc'):
    if d.isdigit():
        try:
            stat = open(f'/proc/{d}/stat').read()
            ppid = int(stat.rsplit(')', 1)[1].split()[1])
            cmd = open(f'/proc/{d}/cmdline').read().replace('\0', ' ')
            procs[int(d)] = (ppid, cmd)
        except Exception:
            pass
fs = [p for p, (pp, c) in procs.items() if pp == 1 and 'forkserver' in c]
killed = 0
for p, (pp, c) in procs.items():
    if pp in fs and 'resource_tracker' not in c:
        os.kill(p, signal.SIGKILL); killed += 1
print(killed)
"""


def p95(values):
    """Nearest rank 95th percentile."""
    v = sorted(values)
    return v[max(0, min(len(v) - 1, -(-95 * len(v) // 100) - 1))] if v else None


class Lab:
    """One `oo serve --runtime compose` stack and the helpers around it."""

    def __init__(self, ev, oo, work, deploy):
        self.ev, self.oo, self.work, self.deploy = ev, oo, work, deploy
        self.state = work / "state"
        self.root = work / "drive"
        self.held_root = work / "held"
        self.daemon = None
        self.api = None
        self.project = None
        self.drive_id = None
        self.held_id = None
        self.db = None

    # ---- lifecycle
    def project_name(self):
        return "oo-docs-" + hashlib.sha256(os.path.realpath(self.state).encode()).hexdigest()[:12]

    def start(self, poll_seconds=None):
        self.state.mkdir(mode=0o700, exist_ok=True)
        engine_dir = self.state / "engine"
        engine_dir.mkdir(mode=0o700, exist_ok=True)
        cfg = engine_dir / "docs.json"
        if poll_seconds is None:
            cfg.unlink(missing_ok=True)
        else:
            cfg.write_text(json.dumps({"version": "oo.docs/v1", "sync": {"fs_poll_seconds": poll_seconds}}))
        self.project = comp.Project(self.project_name())
        self.daemon = comp.Daemon(self.ev, self.oo, self.state, self.deploy)
        self.api = self.daemon.start()
        self.db = None

    def stop(self):
        if self.daemon and self.daemon.proc:
            return self.daemon.term()

    def db_id(self):
        if not self.db:
            self.db = self.project.by_service()["docs-db"]["Id"]
        return self.db

    def psql(self, sql):
        r = comp.sh("docker", "exec", self.db_id(), "psql", "-U", "postgres", "-d", "oo_docs", "-tAc", sql)
        if r.returncode != 0:
            raise RuntimeError(f"psql: {r.stderr.strip()[:200]}")
        return r.stdout.strip()

    def sidecar_id(self):
        return self.project.by_service()["doc-parser"]["Id"]

    # ---- api with the retries an engine recreation needs
    def req(self, method, path, body=None, ok=(200, 201, 202, 204), tries=120):
        last = None
        for _ in range(tries):
            try:
                st, data = self.api.request(method, path, body, timeout=60)
            except Exception as e:  # engine restarting
                last = repr(e)
                time.sleep(0.5)
                continue
            if st in (502, 503, 504):
                last = f"HTTP {st}"
                time.sleep(0.5)
                continue
            if st not in ok:
                raise RuntimeError(f"{method} {path}: HTTP {st} {str(data)[:200]}")
            return data
        raise RuntimeError(f"{method} {path}: no answer ({last})")

    def register(self, name, root):
        d = self.req("POST", "/v1/docs/drives", {"name": name, "backend": "fs", "config": {"root": str(root)}})
        return d["id"]

    def files(self, drive_id=None, prefix=""):
        drive_id = drive_id or self.drive_id
        out, cursor = [], None
        while True:
            q = f"/v1/docs/drives/{drive_id}/files?recursive=true&limit=1000"
            if prefix:
                q += f"&path={prefix}"
            if cursor:
                q += f"&cursor={cursor}"
            page = self.req("GET", q)
            out += page["items"]
            cursor = page.get("next_cursor")
            if not cursor:
                return out

    def states(self, prefix="", drive_id=None):
        return {f["path"]: f for f in self.files(drive_id, prefix)}

    def wait_final(self, prefix, expect, deadline, drive_id=None, final=FINAL):
        end = time.time() + deadline
        last = {}
        while time.time() < end:
            last = self.states(prefix, drive_id)
            if len(last) >= expect and all(f["state"] in final for f in last.values()):
                return last
            time.sleep(1.0)
        return last

    def drive(self, drive_id=None):
        return self.req("GET", f"/v1/docs/drives/{drive_id or self.drive_id}")

    def sync(self, paths=None, drive_id=None):
        body = {"paths": paths} if paths else {}
        return self.req("POST", f"/v1/docs/drives/{drive_id or self.drive_id}:sync", body)

    def kill_sidecar_workers(self):
        r = comp.exec_py(self.sidecar_id(), KILL_WORKERS)
        try:
            return int(r.stdout.strip().splitlines()[-1])
        except (ValueError, IndexError):
            return 0


def put(root, rel, data):
    dest = root / rel
    dest.parent.mkdir(parents=True, exist_ok=True)
    if isinstance(data, str):
        data = data.encode("utf-8")
    dest.write_bytes(data)
    return dest


def ext_of(path):
    return path.rsplit(".", 1)[-1].lower() if "." in path else ""


def golden_files():
    return sorted(p for p in GOLDEN.rglob("*") if p.is_file())


def find_golden(ext, nth=0):
    return sorted(p for p in GOLDEN.rglob(f"*.{ext}"))[nth]


# ======================================================================= phases


def phase_golden(ev, lab):
    ev.log("\n## acceptance 1: golden set and hostile fixtures")
    n = 0
    for p in golden_files():
        put(lab.root, f"golden/{p.relative_to(GOLDEN)}", p.read_bytes())
        n += 1
    for name in HOSTILE:
        put(lab.root, f"hostile/{name}", (FIXTURES / name).read_bytes())
    t0 = time.time()
    states = lab.wait_final("", n + len(HOSTILE), 900)
    took = time.time() - t0
    ev.timings["golden_settle_seconds"] = round(took, 1)
    golden = {p: f for p, f in states.items() if p.startswith("golden/")}
    ev.check("all golden files were found", len(golden) == n, f"{len(golden)}/{n}")
    per = {}
    for p, f in golden.items():
        e = ext_of(p)
        d = per.setdefault(e, {"total": 0, "ok": 0, "failed": []})
        d["total"] += 1
        if f["state"] in ("published", "duplicate"):
            d["ok"] += 1
        else:
            d["failed"].append({"path": p, "state": f["state"], "code": f.get("state_code")})
    for e in FORMATS:
        d = per.get(e, {"total": 0, "ok": 0, "failed": []})
        rate = 100.0 * d["ok"] / d["total"] if d["total"] else 0.0
        ev.check(f"golden {e}: parse success >= 98%", d["total"] > 0 and rate >= 98.0, f"{d['ok']}/{d['total']} = {rate:.1f}%")
        ev.timings[f"golden_success_{e}"] = f"{d['ok']}/{d['total']}"
    bad = [(p, f["state"], f.get("state_code")) for p, f in states.items() if f["state"] not in ("published", "duplicate") and p.startswith("golden/")]
    ev.check("no golden file is left in a failure state", not bad, json.dumps(bad))
    lat = {}
    for p, f in golden.items():
        t = (f.get("latency_ms") or {}).get("total")
        if t is not None:
            lat.setdefault(ext_of(p), []).append(t / 1000.0)
    ev.timings["golden_latency_stored_to_published_seconds"] = {e: {"median": round(statistics.median(v), 1), "max": round(max(v), 1)} for e, v in lat.items()}
    # hostile
    hostile = {p.split("/", 1)[1]: f for p, f in states.items() if p.startswith("hostile/")}
    for name, want in HOSTILE.items():
        f = hostile.get(name)
        if want is None:
            ev.log(f"  info: {name} -> {f and (f['state'], f.get('state_code'))}")
            continue
        got = (f["state"], f.get("state_code")) if f else None
        ev.check(f"hostile {name}: {want[0]} {want[1] or ''}".strip(), got == want, str(got))
    failing = {p: f for p, f in states.items() if f["state"] not in ("published", "duplicate")}
    no_code = [p for p, f in failing.items() if not f.get("state_code")]
    unknown = sorted({f["state_code"] for f in failing.values() if f.get("state_code") and f["state_code"] not in ERROR_CODES})
    ev.check("every file that did not publish carries an error code", not no_code, json.dumps(no_code))
    ev.check("every such code is in errors.v1.json", not unknown, json.dumps(unknown))
    ev.write("l3-golden-states.json", json.dumps({"per_format": per, "hostile": {k: [v["state"], v.get("state_code")] for k, v in hostile.items()}, "settle_seconds": round(took, 1)}, indent=1, sort_keys=True, ensure_ascii=False) + "\n")


def phase_dups(ev, lab):
    ev.log("\n## acceptance 4: ten copies of one content parse once")
    picks = {"md": find_golden("md"), "pdf": find_golden("pdf"), "docx": find_golden("docx"), "xlsx": find_golden("xlsx")}
    base = {}
    for i, (e, p) in enumerate(picks.items()):
        base[e] = variants.vary(e, p.read_bytes(), 9000 + i)
        for c in range(10):
            put(lab.root, f"dups/{e}/copy-{c}.{e}", base[e])
    states = lab.wait_final("dups", 40, 600)
    for e in picks:
        mine = [f for p, f in states.items() if p.startswith(f"dups/{e}/")]
        counts = {}
        for f in mine:
            counts[f["state"]] = counts.get(f["state"], 0) + 1
        ev.check(f"{e}: 10 copies -> 1 published and 9 duplicate", counts == {"published": 1, "duplicate": 9}, json.dumps(counts, sort_keys=True))
        b3 = mine[0]["b3"] if mine else ""
        jobs = lab.psql(f"SELECT count(*) FROM _oo_docs.job j JOIN _oo_docs.file_version v ON v.id = j.version_id WHERE j.kind = 'parse' AND j.state = 'succeeded' AND v.b3 = '{b3}'")
        results = lab.psql(f"SELECT count(*) FROM _oo_docs.parse_result WHERE b3 = '{b3}' AND status = 'complete'")
        ev.check(f"{e}: one parse job and one parse result for the content", jobs == "1" and results == "1", f"parse jobs {jobs}, parse results {results}")
        rows = lab.psql(f"SELECT f.path || '|' || count(e.id) FROM _oo_docs.file f JOIN _oo_docs.evidence e ON e.version_id = f.current_version_id AND e.withdrawn_at IS NULL WHERE f.path LIKE 'dups/{e}/%' GROUP BY f.path ORDER BY f.path")
        per = [int(r.split("|")[1]) for r in rows.splitlines() if r]
        ev.check(f"{e}: every one of the 10 paths has its own evidence, equal in number", len(per) == 10 and len(set(per)) == 1 and per[0] > 0, f"{len(per)} paths, evidence per path {sorted(set(per))}")


def phase_poison(ev, lab):
    ev.log("\n## acceptance 3: poison file")
    pdf = FIXTURES / "pdf-60pages.pdf"
    put(lab.root, "poison/poison.pdf", variants.pdf(pdf.read_bytes(), 9500))
    for i in range(8):
        put(lab.root, f"poison/n{i}.md", f"---\ntype: project\nname: 普通项目{i}\nowner: 林川\n---\n# 普通项目{i}\n\n负责人：林川\n")
        put(lab.root, f"poison/t{i}.csv", f"名称,数量\n甲{i},{i}\n乙{i},{i + 1}\n")
    stop = threading.Event()
    kills = []

    def killer():
        while not stop.is_set():
            kills.append(lab.kill_sidecar_workers())
            stop.wait(0.5)

    th = threading.Thread(target=killer, daemon=True)
    th.start()
    t0 = time.time()
    quarantined = None
    while time.time() - t0 < 360:
        f = lab.states("poison").get("poison/poison.pdf")
        if f and f["state"] == "parse_failed" and f.get("state_code") == "DOCS_PARSE_QUARANTINED":
            quarantined = time.time() - t0
            break
        time.sleep(1)
    stop.set()
    th.join(timeout=10)
    ev.check("the poison pdf ends parse_failed with DOCS_PARSE_QUARANTINED", quarantined is not None, f"after {quarantined and round(quarantined, 1)} s, {sum(kills)} worker kills")
    others = lab.wait_final("poison", 17, 240)
    ok = [p for p, f in others.items() if p != "poison/poison.pdf" and f["state"] == "published"]
    ev.check("the 16 other files in the same drive publish", len(ok) == 16, f"{len(ok)}/16")
    row = lab.psql("SELECT state || '|' || attempt || '|' || crash_count FROM _oo_docs.job WHERE kind = 'parse' AND file_id = (SELECT id FROM _oo_docs.file WHERE path = 'poison/poison.pdf') ORDER BY created_at DESC LIMIT 1")
    state, attempt, crashes = row.split("|")
    ev.check("the job is quarantined after at most 3 attempts", state == "quarantined" and int(attempt) <= 3 and int(crashes) >= 2, f"state {state}, attempt {attempt}, crashes {crashes}")
    retried = lab.psql("SELECT count(*) FROM (SELECT file_id FROM _oo_docs.job WHERE kind = 'parse' AND file_id IN (SELECT id FROM _oo_docs.file WHERE path LIKE 'poison/%' AND path <> 'poison/poison.pdf') GROUP BY file_id HAVING count(*) > 1) x")
    ev.check("no other file needed a second parse job", retried == "0", f"{retried} retried")
    stuck = lab.psql("SELECT count(*) FROM _oo_docs.job WHERE state = 'running' AND lease_until < clock_timestamp()")
    ev.check("no job is stuck", stuck == "0")
    # the sidecar is healthy afterwards: a fresh pdf parses
    put(lab.root, "poison/after.pdf", variants.pdf(find_golden("pdf").read_bytes(), 9501))
    got = lab.wait_final("poison/after", 1, 180)
    ev.check("the sidecar still parses after the poison", got.get("poison/after.pdf", {}).get("state") == "published", str(got.get("poison/after.pdf", {}).get("state")))


def phase_life(ev, lab):
    ev.log("\n## acceptance 5 (fs): add, modify, rename, delete")

    def doc(v):
        return f"---\ntype: project\nname: 生命周期项目\nowner: 林川\n---\n# 生命周期项目\n\n负责人：林川\n\n状态：{v}\n"

    def state_of(path):
        f = lab.states("life").get(path)
        return f["state"] if f else None

    def wait_state(path, want, deadline=90):
        return l2.wait_until(deadline, lambda: state_of(path) == want, step=0.5)

    def evidence_live(path, version="current"):
        return int(lab.psql(f"SELECT count(*) FROM _oo_docs.evidence e JOIN _oo_docs.file f ON f.current_version_id = e.version_id WHERE f.path = '{path}' AND f.deleted_at IS NULL AND e.withdrawn_at IS NULL"))

    def history(path):
        return lab.psql(f"SELECT string_agg(h.to_state, '>' ORDER BY h.seq) FROM _oo_docs.file_state_history h JOIN _oo_docs.file f ON f.id = h.file_id WHERE f.path = '{path}'")

    retracted0 = int(lab.psql("SELECT count(*) FROM _oo_docs.fact WHERE status = 'retracted'"))
    # add
    put(lab.root, "life/a.md", doc("进行中"))
    ev.check("add: published", wait_state("life/a.md", "published"), history("life/a.md"))
    e1 = evidence_live("life/a.md")
    ev.check("add: evidence rows written", e1 > 0, f"{e1} live evidence rows")
    v1 = lab.states("life")["life/a.md"]["b3"]
    # modify
    put(lab.root, "life/a.md", doc("已完成"))
    changed = l2.wait_until(90, lambda: lab.states("life").get("life/a.md", {}).get("b3") not in (None, v1) and state_of("life/a.md") == "published", step=0.5)
    ev.check("modify: the new content is published", bool(changed), history("life/a.md"))
    h = history("life/a.md") or ""
    ev.check("modify: the chain went through superseded and stored", "superseded>stored" in h, h)
    old_live = lab.psql(f"SELECT count(*) FROM _oo_docs.evidence e JOIN _oo_docs.file_version v ON v.id = e.version_id JOIN _oo_docs.file f ON f.id = v.file_id WHERE f.path = 'life/a.md' AND v.b3 = '{v1}' AND e.withdrawn_at IS NULL")
    ev.check("modify: the old version's evidence is withdrawn, the new version's is live", old_live == "0" and evidence_live("life/a.md") > 0, f"old live {old_live}, new live {evidence_live('life/a.md')}")
    # rename
    b3_before = lab.states("life")["life/a.md"]["b3"]
    os.rename(lab.root / "life/a.md", lab.root / "life/b.md")
    ok = l2.wait_until(90, lambda: state_of("life/b.md") == "published" and "life/a.md" not in lab.states("life"), step=0.5)
    ev.check("rename: the new path is published, the old path is gone", bool(ok), history("life/b.md"))
    ev.check("rename: same content, evidence is live on the new path", lab.states("life").get("life/b.md", {}).get("b3") == b3_before and evidence_live("life/b.md") > 0, f"{evidence_live('life/b.md')} live evidence rows")
    renamed = lab.psql("SELECT count(*) FROM _oo_docs.event WHERE type = 'file.renamed'")
    ev.check("rename: a file.renamed event exists", int(renamed) >= 1, f"{renamed} events")
    retracted1 = int(lab.psql("SELECT count(*) FROM _oo_docs.fact WHERE status = 'retracted'"))
    ev.check("rename: no fact was retracted", retracted1 == retracted0 + int(lab.psql("SELECT count(*) FROM (SELECT 1) x WHERE false")) or retracted1 == retracted0, f"retracted {retracted0} -> {retracted1}")
    # delete
    (lab.root / "life/b.md").unlink()
    gone = l2.wait_until(90, lambda: "life/b.md" not in lab.states("life"), step=0.5)
    ev.check("delete: the file disappears from the listing", bool(gone))
    row = lab.psql("SELECT (deleted_at IS NOT NULL)::text || '|' || state FROM _oo_docs.file WHERE path = 'life/b.md' ORDER BY created_at DESC LIMIT 1")
    ev.check("delete: the file row is marked deleted", row.startswith("true|"), row)
    live = lab.psql("SELECT count(*) FROM _oo_docs.evidence e JOIN _oo_docs.file f ON f.current_version_id = e.version_id WHERE f.path = 'life/b.md' AND e.withdrawn_at IS NULL")
    ev.check("delete: its evidence is withdrawn", live == "0", f"{live} live rows")
    retracted2 = int(lab.psql("SELECT count(*) FROM _oo_docs.fact WHERE status = 'retracted'"))
    ev.check("delete: facts left without evidence are retracted", retracted2 > retracted1, f"retracted {retracted1} -> {retracted2}")


def docx_format_only(data):
    """Same text, different formatting: bold the first run."""
    src = zipfile.ZipFile(io.BytesIO(data))
    out = io.BytesIO()
    done = False
    with zipfile.ZipFile(out, "w") as dst:
        for info in src.infolist():
            blob = src.read(info.filename)
            if info.filename == "word/document.xml" and not done:
                xml = blob.decode("utf-8")
                m = re.search(r"<w:r>(?!<w:rPr>)", xml) or re.search(r"<w:r(?: [^>]*)?>(?!<w:rPr>)", xml)
                if m:
                    xml = xml[: m.end()] + "<w:rPr><w:b/><w:color w:val=\"FF0000\"/></w:rPr>" + xml[m.end():]
                    blob = xml.encode("utf-8")
                    done = True
            zi = zipfile.ZipInfo(info.filename, info.date_time)
            zi.compress_type = info.compress_type
            zi.external_attr = info.external_attr
            dst.writestr(zi, blob)
    if not done:
        raise RuntimeError("no run to format")
    return out.getvalue()


def phase_reuse(ev, lab):
    ev.log("\n## acceptance 10: format-only DOCX change")
    base = variants.vary("docx", find_golden("docx", 1).read_bytes(), 9700)
    put(lab.root, "reuse/doc.docx", base)

    def cur():
        return lab.states("reuse").get("reuse/doc.docx", {})

    ok = l2.wait_until(240, lambda: cur().get("state") == "published", step=1)
    ev.check("the docx is published", bool(ok), str(cur().get("state")))
    b3a = cur().get("b3")
    facts_a = lab.psql("SELECT count(DISTINCT e.target_id) FROM _oo_docs.evidence e JOIN _oo_docs.file f ON f.current_version_id = e.version_id WHERE f.path = 'reuse/doc.docx' AND e.target_kind = 'fact' AND e.withdrawn_at IS NULL")
    ev_a = lab.psql("SELECT count(*) FROM _oo_docs.evidence e JOIN _oo_docs.file f ON f.current_version_id = e.version_id WHERE f.path = 'reuse/doc.docx' AND e.withdrawn_at IS NULL")
    rules_a = lab.psql("SELECT count(*) FROM _oo_docs.job j JOIN _oo_docs.file f ON f.id = j.file_id WHERE f.path = 'reuse/doc.docx' AND j.kind = 'extract_rules' AND j.state = 'succeeded'")
    retracted_a = lab.psql("SELECT count(*) FROM _oo_docs.fact WHERE status = 'retracted'")
    ev.check("the first version has facts with evidence", int(facts_a) > 0 and int(ev_a) > 0, f"{facts_a} facts, {ev_a} evidence rows")
    formatted = docx_format_only(base)
    ev.check("the new file differs in bytes", hashlib.sha256(formatted).digest() != hashlib.sha256(base).digest())
    put(lab.root, "reuse/doc.docx", formatted)
    ok = l2.wait_until(240, lambda: cur().get("b3") not in (None, b3a) and cur().get("state") == "published", step=1)
    ev.check("the formatted docx is published as a new version", bool(ok), f"{cur().get('state')} b3 changed: {cur().get('b3') != b3a}")
    detail = lab.psql("SELECT h.detail::text FROM _oo_docs.file_state_history h JOIN _oo_docs.file f ON f.id = h.file_id WHERE f.path = 'reuse/doc.docx' AND h.to_state = 'indexed' ORDER BY h.seq DESC LIMIT 1")
    ev.check("the extraction was reused: the history says reused_from", "reused_from" in detail, detail[:120])
    ev_b = lab.psql("SELECT count(*) FROM _oo_docs.evidence e JOIN _oo_docs.file f ON f.current_version_id = e.version_id WHERE f.path = 'reuse/doc.docx' AND e.withdrawn_at IS NULL")
    old_live = lab.psql(f"SELECT count(*) FROM _oo_docs.evidence e JOIN _oo_docs.file_version v ON v.id = e.version_id WHERE v.b3 = '{b3a}' AND v.file_id = (SELECT id FROM _oo_docs.file WHERE path = 'reuse/doc.docx') AND e.withdrawn_at IS NULL")
    ev.check("the evidence moved to the new version: same count live, none left on the old one", ev_b == ev_a and old_live == "0", f"new {ev_b} (was {ev_a}), old live {old_live}")
    retracted_b = lab.psql("SELECT count(*) FROM _oo_docs.fact WHERE status = 'retracted'")
    ev.check("no fact was retracted", retracted_b == retracted_a, f"{retracted_a} -> {retracted_b}")
    # the same file with a changed word is extracted again (control)
    changed = variants.vary("docx", formatted, 9701)
    put(lab.root, "reuse/doc.docx", changed)
    ok = l2.wait_until(240, lambda: cur().get("state") == "published" and cur().get("b3") not in (None, b3a), step=1)
    b3c = cur().get("b3")
    ok = l2.wait_until(240, lambda: cur().get("b3") != b3c or True, step=1)
    time.sleep(3)
    detail2 = lab.psql("SELECT h.detail::text FROM _oo_docs.file_state_history h JOIN _oo_docs.file f ON f.id = h.file_id WHERE f.path = 'reuse/doc.docx' AND h.to_state = 'indexed' ORDER BY h.seq DESC LIMIT 1")
    ev.check("control: a changed word is extracted again (no reused_from)", "reused_from" not in detail2, detail2[:100])


def measure(lab, rel, data, hint, deadline=120):
    """Write a file straight into the Drive directory (like a Finder drop) and
    watch the API until it is published. Returns (seconds, server side stored
    to published seconds)."""
    dest = put(lab.root, rel, data)
    t0 = time.time()
    if hint:
        lab.sync([rel])
    name = rel.rsplit("/", 1)[-1]
    state = None
    while time.time() - t0 < deadline:
        page = lab.req("GET", f"/v1/docs/drives/{lab.drive_id}/files?recursive=true&limit=5&q={name}")
        for f in page["items"]:
            if f["path"] == rel:
                state = f["state"]
                if state == "published":
                    t = time.time() - t0
                    return t, (f.get("latency_ms") or {}).get("total", 0) / 1000.0
                if state in FINAL:
                    return None, state
        time.sleep(0.1)
    return None, state


def phase_latency(ev, lab, hint, samples):
    mode = "hint" if hint else "poll"
    ev.log(f"\n## acceptance 8: single file latency, {'targeted sync hint, 30 s poll' if hint else 'files dropped without any API call, 1 s poll'}")
    sources = {"md/txt": [("md", find_golden("md", 2)), ("txt", find_golden("txt", 0))], "pdf": [("pdf", find_golden("pdf", 1))], "docx": [("docx", find_golden("docx", 2))], "xlsx": [("xlsx", find_golden("xlsx", 0))]}
    result = {}
    counter = 20000 + (500 if hint else 0)
    for group, srcs in sources.items():
        times, server = [], []
        for i in range(samples[group]):
            ext, src = srcs[i % len(srcs)]
            counter += 1
            data = variants.vary(ext, src.read_bytes(), counter)
            t, s = measure(lab, f"lat-{mode}/{group.replace('/', '_')}-{counter}.{ext}", data, hint)
            if t is None:
                ev.log(f"  sample failed: {group} {s}")
                continue
            times.append(t)
            server.append(s)
        q = p95(times)
        result[group] = {"n": len(times), "p50": round(statistics.median(times), 2) if times else None, "p95": round(q, 2) if q is not None else None, "max": round(max(times), 2) if times else None, "server_stored_to_published_p95": round(p95(server), 2) if server else None, "target_p95": D49[group]}
        ok = q is not None and q <= D49[group]
        ev.check(f"D49 {group} ({mode}): p95 {result[group]['p95']} s vs target {D49[group]} s", ok, json.dumps(result[group]))
    ev.timings[f"latency_{mode}"] = result
    ev.write(f"l3-latency-{mode}.json", json.dumps(result, indent=1, sort_keys=True) + "\n")


def phase_targeted(ev, lab):
    ev.log("\n## acceptance 9: a targeted sync does not sweep")
    for attempt in range(1, 5):
        gen0 = lab.psql(f"SELECT sync_generation FROM _oo_docs.drive WHERE id = '{lab.drive_id}'")
        polls0 = lab.psql(f"SELECT count(*) FROM _oo_docs.drive_sync WHERE drive_id = '{lab.drive_id}'")
        victim = f"golden/{sorted(p.name for p in (GOLDEN / 'Notes').iterdir())[0]}" if (GOLDEN / "Notes").is_dir() else None
        if not victim or not (lab.root / victim).exists():
            ev.check("a golden file exists to delete", False, str(victim))
            return
        saved = (lab.root / victim).read_bytes()
        (lab.root / victim).unlink()
        new = f"targeted/new-{attempt}.md"
        put(lab.root, new, f"---\ntype: project\nname: 定向同步{attempt}\nowner: 林川\n---\n# 定向同步{attempt}\n\n负责人：林川\n")
        lab.sync([new])
        ok = l2.wait_until(60, lambda: lab.states("targeted").get(new, {}).get("state") == "published", step=0.5)
        gen1 = lab.psql(f"SELECT sync_generation FROM _oo_docs.drive WHERE id = '{lab.drive_id}'")
        polls1 = lab.psql(f"SELECT count(*) FROM _oo_docs.drive_sync WHERE drive_id = '{lab.drive_id}' AND trigger <> 'manual'")
        still = lab.states("golden").get(victim, {}).get("state")
        if gen1 != gen0 or still is None:
            # a poll sweep ran in between: put the file back and try again
            ev.log(f"  attempt {attempt}: a poll sweep ran in the window (generation {gen0} -> {gen1}); retrying")
            (lab.root / victim).write_bytes(saved)
            time.sleep(2)
            continue
        ev.check("the targeted sync published the new file", bool(ok), f"{new}")
        ev.check("the generation did not advance", gen1 == gen0, f"{gen0} -> {gen1}")
        ev.check("the file deleted on disk is still listed (no sweep)", still is not None, f"{victim}: {still}")
        lab.sync()
        gone = l2.wait_until(90, lambda: victim not in lab.states("golden") or lab.states("golden")[victim]["state"] == "deleted", step=0.5)
        gen2 = lab.psql(f"SELECT sync_generation FROM _oo_docs.drive WHERE id = '{lab.drive_id}'")
        ev.check("the full sync sweeps it and advances the generation", bool(gone) and int(gen2) > int(gen1), f"{gen1} -> {gen2}")
        return
    ev.check("a window without a poll sweep was found in 4 attempts", False)


def freshness_seen(lab):
    seen = set()
    for e in lab.api.events(0, listen=4.0):
        blob = json.dumps(e)
        if "freshness" not in blob:
            continue
        fr = e.get("freshness") or (e.get("payload") or {}).get("freshness") or (e.get("data") or {}).get("freshness")
        if isinstance(fr, dict):
            fr = fr.get("state")
        if isinstance(fr, str):
            seen.add(fr)
    return seen


def phase_fresh(ev, lab):
    ev.log("\n## acceptance 9: the seven freshness states")
    held_id = lab.held_id
    drive = lab.drive(held_id)
    ev.log("  freshness now: " + json.dumps(drive.get("freshness")))
    # held: delete every file of the small drive at once (mass delete), then confirm
    n0 = len(lab.states("", held_id))
    for p in sorted(lab.held_root.glob("*.md")):
        p.unlink()
    lab.sync(drive_id=held_id)
    held = l2.wait_until(90, lambda: (lab.drive(held_id).get("freshness") or {}).get("state") == "held", step=0.5)
    ev.check("held: deleting every file of a Drive at once is held", bool(held), json.dumps(lab.drive(held_id).get("freshness")))
    # degraded: the root goes away
    for i in range(n0):
        put(lab.held_root, f"h{i:03d}.md", f"# 文件{i}\n\n内容{i}\n")
    moved = lab.held_root.with_name("held.off")
    lab.held_root.rename(moved)
    lab.sync(drive_id=held_id)
    deg = l2.wait_until(90, lambda: (lab.drive(held_id).get("freshness") or {}).get("state") in ("degraded", "held"), step=0.5)
    fr = lab.drive(held_id).get("freshness")
    ev.log("  after the root was removed: " + json.dumps(fr))
    moved.rename(lab.held_root)
    lab.sync(drive_id=held_id)
    back = l2.wait_until(180, lambda: (lab.drive(held_id).get("freshness") or {}).get("state") == "up_to_date", step=1)
    ev.check("the Drive recovers to up_to_date once the root is back and files exist", bool(back), json.dumps(lab.drive(held_id).get("freshness")))
    seen = freshness_seen(lab)
    ev.log("  freshness states seen in events: " + ",".join(sorted(seen)))
    for s in ("up_to_date", "syncing", "processing", "publishing", "waiting_review", "degraded", "held"):
        ev.check(f"freshness {s} was observed", s in seen, "")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--run-id", default=time.strftime("%Y%m%d-%H%M%S"))
    ap.add_argument("--evidence-dir", default=str(REPO / "testing/super-ontology/evidence/L3"))
    ap.add_argument("--only", default="golden,dups,poison,life,reuse,latency,hint,targeted,fresh")
    ap.add_argument("--skip-build", action="store_true")
    ap.add_argument("--keep", action="store_true")
    ap.add_argument("--samples", default="md/txt=20,pdf=10,docx=10,xlsx=10")
    args = ap.parse_args()
    only = set(args.only.split(","))
    samples = {k: int(v) for k, v in (x.split("=") for x in args.samples.split(","))}

    ev = l2.Evidence(Path(args.evidence_dir), args.run_id)
    ev.log(f"# L3 acceptance, run {args.run_id}")
    target = Path(os.environ.get("CARGO_TARGET_DIR", REPO / "target")) / "debug"
    oo = target / "oo"
    work = Path(tempfile.mkdtemp(prefix=f"oo-l3-{args.run_id}-", dir="/tmp"))
    lab = Lab(ev, oo, work, DEPLOY)
    lab.root.mkdir()
    lab.held_root.mkdir()
    ok = False
    try:
        free = shutil.disk_usage("/").free / 2**30
        ev.check("disk: at least 20 GiB free", free >= 20, f"{free:.0f} GiB")
        if not args.skip_build:
            l2.run(ev, ["cargo", "build", "--locked", "-p", "oo-cli", "--features", "docs"])
        if not oo.is_file():
            raise SystemExit(f"missing binary {oo}")
        for i in range(120):
            put(lab.held_root, f"h{i:03d}.md", f"# 文件{i}\n\n内容{i}\n")

        ev.log("\n## stack: oo serve --runtime compose (1 s poll)")
        t0 = time.time()
        lab.start(poll_seconds=1)
        ev.timings["start_seconds"] = round(time.time() - t0, 1)
        lab.drive_id = lab.register("l3", lab.root)
        lab.held_id = lab.register("held", lab.held_root)
        ev.check("two Drives registered", bool(lab.drive_id and lab.held_id))
        ev.check("the doc-parser is up and the engine can parse (health ok)", lab.req("GET", "/v1/docs/health").get("status") in ("ok", "degraded"))
        lab.wait_final("", 120, 300, lab.held_id)

        if "golden" in only:
            phase_golden(ev, lab)
        if "dups" in only:
            phase_dups(ev, lab)
        if "poison" in only:
            phase_poison(ev, lab)
        if "life" in only:
            phase_life(ev, lab)
        if "reuse" in only:
            phase_reuse(ev, lab)
        if "latency" in only:
            phase_latency(ev, lab, hint=False, samples=samples)
        if "hint" in only or "targeted" in only or "fresh" in only:
            ev.log("\n## restart with the default configuration (30 s poll)")
            code = lab.stop()
            ev.check("oo serve exits 0 on SIGTERM", code == 0, f"exit {code}")
            lab.start(poll_seconds=None)
            ev.check("both Drives survived the restart", len(lab.req("GET", "/v1/docs/drives")["items"]) == 2)
            time.sleep(8)
            if "hint" in only:
                phase_latency(ev, lab, hint=True, samples={k: max(5, v // 2) for k, v in samples.items()})
            if "targeted" in only:
                phase_targeted(ev, lab)
        if "fresh" in only:
            phase_fresh(ev, lab)
        ok = all(c["ok"] for c in ev.checks)
    finally:
        if lab.daemon and lab.daemon.proc:
            lab.stop()
        if lab.project and not args.keep:
            lab.project.purge()
        left = (lab.project.containers() + lab.project.networks() + lab.project.volumes()) if lab.project else []
        ev.log(f"\ncleanup: containers/networks/volumes left = {len(left)}")
        if not args.keep:
            l2.remove_tree(work)
        ev.check("cleanup left nothing", args.keep or not left)
        ok = ok and all(c["ok"] for c in ev.checks)
        ev.write("l3-run.txt", "\n".join(ev.log_lines) + "\n")
        ev.write("l3-result.json", json.dumps({"run_id": args.run_id, "ok": ok, "checks": ev.checks, "timings": ev.timings}, indent=1, ensure_ascii=False) + "\n")
    passed = sum(1 for c in ev.checks if c["ok"])
    print(f"# {passed}/{len(ev.checks)} checks passed")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
