#!/usr/bin/env python3
"""Super Ontology cold parse bench (card B-S4-06, 10 section 12.9, 30 L3 acceptance 6).

1000 synthetic files, generated from the golden set by content variation in
the golden mix of formats (about 35 % md, 6 % txt, 6 % csv, 8 % xlsx, 20 %
docx, 12 % pptx, 14 % pdf), are written into a directory and the Drive is
registered on a fresh `oo serve --runtime compose` stack (no LLM). The clock
runs from the registration to the moment every file is in a final state. Peak
memory of every container is sampled once a second with `docker stats`.

Targets: cold parse <= 30 min; engine <= 1.5 GiB, sidecar <= 3 GiB, PG <= 1 GiB.

`--bench compile` runs the other bench of the board instead (B-S6-07, 30 L4
acceptance 5): the pack compile p95 on a 1000 file Drive with budget 800 in the
release build against a labelled Postgres container, p95 < 2 s (alarm +20 %,
block +50 %). It wraps `cargo test -p oo-docs --release --test bench_compile`
and writes `bench-compile.json` / `bench-compile.md`.

Usage: tools/oo-test/super_ontology_bench.py [--bench cold|compile] [--files 1000]
       [--run-id ID] [--evidence-dir DIR] [--skip-build] [--keep]
Evidence: bench-cold1000.md and bench-cold1000.json (cold), bench-compile.json
and bench-compile.md (compile) in the evidence directory.
"""

import argparse
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
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import super_ontology_l2 as l2  # noqa: E402
import super_ontology_l3 as l3  # noqa: E402

REPO = l2.REPO
sys.path.insert(0, str(REPO / "testing/super-ontology/fixtures"))
import variants  # noqa: E402

GiB = 1024 ** 3
LIMITS = {"docs-engine": 1.5 * GiB, "doc-parser": 3 * GiB, "docs-db": 1 * GiB}
LIMIT_TEXT = {"docs-engine": "1.5 GiB", "doc-parser": "3 GiB", "docs-db": "1 GiB"}
MIX = [("md", 353), ("txt", 59), ("csv", 59), ("xlsx", 78), ("docx", 196), ("pptx", 118), ("pdf", 137)]


def to_bytes(text):
    m = re.match(r"([\d.]+)\s*([KMGT]?i?B)", text.strip())
    if not m:
        return 0
    unit = {"B": 1, "KiB": 1024, "MiB": 1024 ** 2, "GiB": GiB, "TiB": 1024 ** 4, "kB": 1000, "MB": 1000 ** 2, "GB": 1000 ** 3}
    return float(m.group(1)) * unit.get(m.group(2), 1)


class Sampler(threading.Thread):
    """`docker stats` stream for a set of container ids: peak memory each."""

    def __init__(self, ids):
        super().__init__(daemon=True)
        self.ids = ids
        self.peak = {}
        self.samples = 0
        self.proc = None

    def run(self):
        self.proc = subprocess.Popen(["docker", "stats", "--format", "{{json .}}", *self.ids], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
        for line in self.proc.stdout:
            line = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", line).strip()
            if not line.startswith("{"):
                continue
            try:
                row = json.loads(line)
            except ValueError:
                continue
            used = to_bytes(row.get("MemUsage", "0B").split("/")[0])
            name = row.get("Name", "")
            self.peak[name] = max(self.peak.get(name, 0), used)
            self.samples += 1

    def stop(self):
        if self.proc:
            self.proc.terminate()


def build_files(root, total):
    scale = total / sum(n for _, n in MIX)
    plan = [(e, round(n * scale)) for e, n in MIX]
    plan[0] = (plan[0][0], plan[0][1] + total - sum(n for _, n in plan))
    counter = 0
    made = {}
    for ext, n in plan:
        templates = sorted(l3.GOLDEN.rglob(f"*.{ext}"))
        for i in range(n):
            counter += 1
            data = variants.vary(ext, templates[i % len(templates)].read_bytes(), 100000 + counter)
            l3.put(root, f"b{counter % 20:02d}/f{counter:04d}.{ext}", data)
        made[ext] = n
    return made


def compile_bench(args):
    """The pack compile bench: a labelled PG container and the release Rust bench."""
    l2.LANE = "oo.lane=super-ontology-be4"
    ev = l2.Evidence(Path(args.evidence_dir), args.run_id)
    ev.log(f"# compile bench, run {args.run_id}")
    pg = l2.Postgres(ev, args.run_id)
    out = Path(tempfile.mkdtemp(prefix="oo-bench-compile-", dir="/tmp")) / "bench.json"
    ok = False
    try:
        pg.start()
        env = dict(os.environ, OO_DOCS_TEST_PG_PORT=str(pg.port), OO_BENCH_OUT=str(out),
                   CARGO_BUILD_JOBS=os.environ.get("CARGO_BUILD_JOBS", "6"))
        argv = ["cargo", "test", "-p", "oo-docs", "--locked", "--release", "--test", "bench_compile",
                "--", "--ignored", "--nocapture", "--test-threads=1"]
        ev.log("$ " + " ".join(argv))
        p = subprocess.run(argv, cwd=REPO, env=env, capture_output=True, text=True, timeout=3600)
        tail = (p.stdout + p.stderr).strip().splitlines()[-6:]
        ev.log("\n".join("  " + t for t in tail))
        ev.check("the Rust bench passed", p.returncode == 0, f"exit {p.returncode}")
        doc = json.loads(out.read_text()) if out.is_file() else {}
        ev.check("release build", doc.get("build") == "release", str(doc.get("build")))
        p95 = doc.get("p95_s")
        ev.check("compile p95 under 2 s on 1000 files, budget 800", p95 is not None and p95 < doc.get("limit_s", 2.0), f"p95 {p95}")
        ev.write("bench-compile.json", json.dumps(doc, indent=2, sort_keys=True) + "\n")
        ev.write("bench-compile.md", "# Compile bench\n\n" + "\n".join(f"- `{k}`: {v}" for k, v in sorted(doc.items())) + "\n")
        ok = all(c["ok"] for c in ev.checks)
    finally:
        pg.stop()
        shutil.rmtree(out.parent, ignore_errors=True)
        left = subprocess.run(["docker", "ps", "-aq", "--filter", f"name={pg.name}"], capture_output=True, text=True).stdout.split()
        ev.check("cleanup left nothing", not left, f"{len(left)} left")
        ok = ok and all(c["ok"] for c in ev.checks)
    passed = sum(1 for c in ev.checks if c["ok"])
    print(f"# {passed}/{len(ev.checks)} checks passed")
    sys.exit(0 if ok else 1)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--bench", choices=["cold", "compile"], default="cold")
    ap.add_argument("--files", type=int, default=1000)
    ap.add_argument("--run-id", default=time.strftime("%Y%m%d-%H%M%S"))
    ap.add_argument("--evidence-dir", default=str(REPO / "testing/super-ontology/evidence/L3"))
    ap.add_argument("--deadline-minutes", type=int, default=45)
    ap.add_argument("--skip-build", action="store_true")
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    if args.bench == "compile":
        if args.evidence_dir.endswith("evidence/L3"):
            args.evidence_dir = str(REPO / "testing/super-ontology/evidence/L4")
        return compile_bench(args)

    ev = l2.Evidence(Path(args.evidence_dir), args.run_id)
    ev.log(f"# cold parse bench, {args.files} files, run {args.run_id}")
    target = Path(os.environ.get("CARGO_TARGET_DIR", REPO / "target")) / "debug"
    oo = target / "oo"
    work = Path(tempfile.mkdtemp(prefix=f"oo-bench-{args.run_id}-", dir="/tmp"))
    lab = l3.Lab(ev, oo, work, l3.DEPLOY)
    lab.root.mkdir()
    sampler = None
    result = {"run_id": args.run_id, "files": args.files}
    ok = False
    try:
        free = shutil.disk_usage("/").free / 2 ** 30
        ev.check("disk: at least 20 GiB free", free >= 20, f"{free:.0f} GiB")
        if not args.skip_build:
            l2.run(ev, ["cargo", "build", "--locked", "-p", "oo-cli", "--features", "docs"])
        t0 = time.time()
        made = build_files(lab.root, args.files)
        size = sum(p.stat().st_size for p in lab.root.rglob("*") if p.is_file())
        ev.log(f"  generated {sum(made.values())} files {json.dumps(made)} {size / 2 ** 20:.1f} MiB in {time.time() - t0:.1f} s")
        result["mix"] = made
        result["bytes"] = size
        lab.start(poll_seconds=None)
        svc = lab.project.by_service()
        ids = [i["Id"] for i in svc.values()]
        names = {i["Id"]: svc_name for svc_name, i in svc.items()}
        sampler = Sampler(ids)
        sampler.start()
        info = {n: comp_inspect(i) for n, i in svc.items()}
        result["containers"] = info
        t_reg = time.time()
        lab.drive_id = lab.register("bench", lab.root)
        # the engine is recreated by the registration: new container ids
        svc = lab.project.by_service()
        sampler.stop()
        sampler = Sampler([i["Id"] for i in svc.values()])
        sampler.start()
        end = t_reg + args.deadline_minutes * 60
        last = None
        progress = []
        while time.time() < end:
            rows = lab.states("")
            counts = {}
            for f in rows.values():
                counts[f["state"]] = counts.get(f["state"], 0) + 1
            if counts != last:
                progress.append((round(time.time() - t_reg, 1), dict(counts)))
                last = counts
            done = len(rows) >= args.files and all(f["state"] in l3.FINAL for f in rows.values())
            if done:
                break
            time.sleep(5)
        took = time.time() - t_reg
        rows = lab.states("")
        counts = {}
        by_fmt = {}
        for p, f in rows.items():
            counts[f["state"]] = counts.get(f["state"], 0) + 1
            d = by_fmt.setdefault(l3.ext_of(p), {})
            d[f["state"]] = d.get(f["state"], 0) + 1
        time.sleep(2)
        sampler.stop()
        peaks = {}
        for name, used in sampler.peak.items():
            peaks[name] = used
        svc_peaks = {}
        for svc_name, i in lab.project.by_service().items():
            n = i["Name"].lstrip("/")
            if n in peaks:
                svc_peaks[svc_name] = peaks[n]
        result.update({"seconds": round(took, 1), "states": counts, "by_format": by_fmt, "peak_memory_bytes": svc_peaks, "stats_samples": sampler.samples})
        ev.log(f"  all files settled in {took:.1f} s: {json.dumps(counts, sort_keys=True)}")
        ev.check("all files reached a final state", len(rows) >= args.files and all(f["state"] in l3.FINAL for f in rows.values()), f"{len(rows)} files, {json.dumps(counts, sort_keys=True)}")
        ev.check("cold parse of the whole Drive within 30 minutes", took <= 1800, f"{took / 60:.1f} min")
        published = counts.get("published", 0)
        ev.check("every file published (synthetic variants of golden files all parse)", published == args.files, f"{published}/{args.files}")
        for svc_name, limit in LIMITS.items():
            peak = svc_peaks.get(svc_name)
            ev.check(f"peak memory of {svc_name} within {LIMIT_TEXT[svc_name]}", peak is not None and peak <= limit, f"{peak / 2 ** 20:.0f} MiB" if peak is not None else "not sampled")
        lat = [f["latency_ms"]["total"] / 1000 for f in rows.values() if f.get("latency_ms", {}).get("total") is not None]
        if lat:
            result["file_stored_to_published_seconds"] = {"median": round(statistics.median(lat), 1), "p95": round(l3.p95(lat), 1), "max": round(max(lat), 1)}
        result["throughput_files_per_minute"] = round(len(rows) / (took / 60), 1)
        ev.log(f"  throughput {result['throughput_files_per_minute']} files/min")
        result["progress"] = progress[:: max(1, len(progress) // 40)]
        ok = all(c["ok"] for c in ev.checks)
    finally:
        if sampler:
            sampler.stop()
        if lab.daemon and lab.daemon.proc:
            lab.stop()
        if lab.project and not args.keep:
            lab.project.purge()
        left = (lab.project.containers() + lab.project.networks() + lab.project.volumes()) if lab.project else []
        ev.check("cleanup left nothing", args.keep or not left, f"{len(left)} left")
        if not args.keep:
            l2.remove_tree(work)
        ok = ok and all(c["ok"] for c in ev.checks)
        result["ok"] = ok
        result["checks"] = ev.checks
        ev.write("bench-cold1000.json", json.dumps(result, indent=1, sort_keys=True, ensure_ascii=False) + "\n")
        ev.write("bench-cold1000.md", report(result))
    passed = sum(1 for c in ev.checks if c["ok"])
    print(f"# {passed}/{len(ev.checks)} checks passed")
    sys.exit(0 if ok else 1)


def comp_inspect(info):
    hc = info["HostConfig"]
    return {"image": info["Image"][:19], "memory_limit": hc.get("Memory"), "cpus": (hc.get("NanoCpus") or 0) / 1e9}


def report(r):
    lines = [f"# Cold parse bench, {r['files']} files", "", f"Run `{r['run_id']}`, no LLM, `oo serve --runtime compose` (docs-db, docs-engine, doc-parser), local arm64 Docker Desktop.", ""]
    if "seconds" in r:
        lines += [f"- Wall time from Drive registration to every file final: **{r['seconds']:.0f} s ({r['seconds'] / 60:.1f} min)**, target <= 30 min.",
                  f"- Throughput {r.get('throughput_files_per_minute')} files/min over {r.get('bytes', 0) / 2 ** 20:.1f} MiB.",
                  f"- Final states: `{json.dumps(r['states'], sort_keys=True)}`.",
                  f"- Mix generated: `{json.dumps(r.get('mix'), sort_keys=True)}`.", ""]
        lines += ["| container | peak memory | limit |", "|---|---|---|"]
        for name, limit in LIMIT_TEXT.items():
            peak = r["peak_memory_bytes"].get(name)
            lines.append(f"| {name} | {peak / 2 ** 20:.0f} MiB | {limit} |" if peak is not None else f"| {name} | not sampled | {limit} |")
        lines += ["", f"`docker stats` samples: {r.get('stats_samples')} (about one per second).", ""]
        if "file_stored_to_published_seconds" in r:
            lines.append(f"Per file stored to published: `{json.dumps(r['file_stored_to_published_seconds'])}` seconds (queueing included).")
        lines += ["", "## By format", "", "| format | states |", "|---|---|"]
        for e, d in sorted(r["by_format"].items()):
            lines.append(f"| {e} | `{json.dumps(d, sort_keys=True)}` |")
    lines += ["", "## Checks", ""]
    for c in r["checks"]:
        lines.append(f"- [{'x' if c['ok'] else ' '}] {c['check']}" + (f" ({c['detail']})" if c["detail"] else ""))
    return "\n".join(lines) + "\n"


if __name__ == "__main__":
    main()
