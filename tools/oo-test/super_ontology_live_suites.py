#!/usr/bin/env python3
"""One gate runner for every `#[ignore]`d live / conformance / bench target of `oo-docs`.

The `#[ignore]` tests of `crates/oo-docs/tests` need a Postgres (and some need docker, an S3 store or an
output directory), so a plain `cargo test` skips them. This script finds every test target that has an
`#[ignore]` attribute (so a new target cannot be forgotten), starts ONE labelled Postgres with
`track_commit_timestamp=on`, runs each target with `-- --ignored`, and reports per target
(passed / failed / skipped with the reason). A target with no entry is impossible: unknown targets run as
the default PG kind, and `--list` shows the kind of every one.

Kinds:
  pg        needs OO_DOCS_TEST_PG_PORT only (the default)
  docker    pg + the `oo-docs-parser:dev` image (sidecar tests start their own container)
  eval      eval_live: OO_EVAL_OUT=<tmp dir>, model off (the model-on and held-out scoring run is
            super_ontology_eval_live.py, which needs the owner's model files)
  bench     bench_compile / bench_scale: release build, OO_BENCH_OUT=<tmp file>, OO_SCALE_FILES small
            (the 10,000 file run with its asserts is `oo-test l3 run --suite super-ontology --bench scale10k`)
  faults    the failpoints feature and OO_DOCS_TEST_PG_CONTAINER (the Postgres restart test)
  s3        download: needs a RustFS / MinIO; runs through super_ontology_s3.py with --with-s3, else skipped

Usage: tools/oo-test/super_ontology_live_suites.py [--only a,b] [--skip a,b] [--with-s3] [--list]
       [--run-id ID] [--evidence-dir DIR] [--timeout-minutes N]
Lane label: oo.lane=$OO_TEST_LANE (default super-ontology-be1). Evidence: <dir>/live-suites.json and .md.
Exit 0 only when no target failed and (without --only/--skip) none was skipped.
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.dont_write_bytecode = True
import super_ontology_l2 as l2  # noqa: E402

REPO = l2.REPO
LANE_NAME = os.environ.get("OO_TEST_LANE", "super-ontology-be1")
l2.LANE = "oo.lane=" + LANE_NAME
TESTS = REPO / "crates/oo-docs/tests"
PG_OPTS = ["-c", "max_connections=300", "-c", "fsync=off"]

KINDS = {
    "eval_live": "eval",
    "bench_compile": "bench",
    "bench_scale": "bench",
    "faults": "faults",
    "download": "s3",
    "sidecar_formats": "docker",
    "sidecar_supervision": "docker",
    "l3_routing": "docker",
}


def ignored_targets():
    """Every integration-test file with a real `#[ignore` attribute (not a doc comment)."""
    return sorted(p.stem for p in TESTS.glob("*.rs") if re.search(r"^\s*#\[ignore", p.read_text(encoding="utf-8"), re.M))


def free_port():
    import socket
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def run_target(name, kind, pg, env_base, timeout):
    env = dict(env_base)
    tmp = None
    argv = ["cargo", "test", "-p", "oo-docs", "--locked"]
    fixed = None
    if kind == "faults":
        # The restart test restarts the container: it needs its own Postgres on a fixed host port (a published
        # random port changes on restart), and it must not take the shared one down for the targets after it.
        fixed = l2.Postgres(l2.Evidence(Path(tempfile.mkdtemp(prefix="oo-live-faults-", dir="/tmp")), "faults"), "faults-" + pg.name.rsplit("-", 1)[-1])
        fixed.start(host_port=free_port())
        env["OO_DOCS_TEST_PG_PORT"] = str(fixed.port)
        env["OO_DOCS_TEST_PG_CONTAINER"] = fixed.name
        argv += ["--features", "failpoints"]
    if kind == "bench":
        argv.append("--release")
        env["OO_SCALE_FILES"] = "1000"
        tmp = Path(tempfile.mkdtemp(prefix="oo-live-bench-", dir="/tmp"))
        env["OO_BENCH_OUT"] = str(tmp / "bench.json")
    if kind == "eval":
        tmp = Path(tempfile.mkdtemp(prefix="oo-live-eval-", dir="/tmp"))
        env["OO_EVAL_OUT"] = str(tmp / "out")  # model off: no OO_EVAL_MODEL
    argv += ["--test", name, "--", "--ignored", "--nocapture", "--test-threads=1"]
    t0 = time.time()
    try:
        p = subprocess.run(argv, cwd=REPO, env=env, capture_output=True, text=True, timeout=timeout)
        out, code = p.stdout + p.stderr, p.returncode
    except subprocess.TimeoutExpired as e:
        out, code = (e.stdout or b"").decode(errors="replace") if isinstance(e.stdout, bytes) else (e.stdout or ""), -1
        out += "\nTIMEOUT"
    finally:
        if tmp:
            shutil.rmtree(tmp, ignore_errors=True)
        if fixed:
            fixed.stop()
    results = re.findall(r"test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored", out)
    passed = sum(int(r[1]) for r in results)
    failed = sum(int(r[2]) for r in results)
    ran_ignored = passed + failed
    row = {"target": name, "kind": kind, "seconds": round(time.time() - t0), "tests_passed": passed, "tests_failed": failed}
    if code == 0 and ran_ignored > 0:
        row["status"] = "passed"
    else:
        row["status"] = "failed"
        row["tail"] = "\n".join(out.strip().splitlines()[-12:])[-1800:]
        if code == 0:
            row["tail"] = "exit 0 but no ignored test ran\n" + row["tail"]
    return row


def run_s3(run_id, timeout):
    t0 = time.time()
    argv = [sys.executable, "-B", str(HERE / "super_ontology_s3.py"), "--cells", "docs-rustfs", "--run-id", run_id]
    p = subprocess.run(argv, cwd=REPO, capture_output=True, text=True, timeout=timeout)
    row = {"target": "download", "kind": "s3", "seconds": round(time.time() - t0), "via": "super_ontology_s3.py --cells docs-rustfs"}
    row["status"] = "passed" if p.returncode == 0 else "failed"
    if p.returncode:
        row["tail"] = (p.stdout + p.stderr).strip()[-1500:]
    return row


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--only")
    ap.add_argument("--skip")
    ap.add_argument("--with-s3", action="store_true")
    ap.add_argument("--list", action="store_true")
    ap.add_argument("--run-id", default=time.strftime("live-%Y%m%d-%H%M%S"))
    ap.add_argument("--evidence-dir", default=str(REPO / "testing/super-ontology/evidence/V2"))
    ap.add_argument("--timeout-minutes", type=int, default=45)
    args = ap.parse_args(argv)
    targets = ignored_targets()
    if args.list:
        for t in targets:
            print(f"{t}\t{KINDS.get(t, 'pg')}")
        return 0
    only = set(filter(None, (args.only or "").split(",")))
    skip = set(filter(None, (args.skip or "").split(",")))
    unknown = (only | skip) - set(targets)
    if unknown:
        raise SystemExit(f"not an ignored target: {sorted(unknown)}")
    ev = l2.Evidence(Path(args.evidence_dir), args.run_id)
    pg = l2.Postgres(ev, args.run_id)
    rows = []
    timeout = args.timeout_minutes * 60
    try:
        pg.start()
        ev.log(f"# live suites, run {args.run_id}, lane {l2.LANE}, {len(targets)} ignored targets")
        env = dict(os.environ, OO_DOCS_TEST_PG_PORT=str(pg.port), CARGO_BUILD_JOBS=os.environ.get("CARGO_BUILD_JOBS", "6"))
        for name in targets:
            kind = KINDS.get(name, "pg")
            if (only and name not in only) or name in skip:
                rows.append({"target": name, "kind": kind, "status": "skipped", "reason": "not selected"})
            elif kind == "s3":
                rows.append(run_s3(args.run_id, timeout) if args.with_s3 else
                            {"target": name, "kind": kind, "status": "skipped", "reason": "needs an S3 store; rerun with --with-s3"})
            else:
                rows.append(run_target(name, kind, pg, env, timeout))
            r = rows[-1]
            ev.log(f"[{r['status'].upper():7}] {name} ({kind}) {r.get('seconds', '')}s {r.get('reason', '')}")
            if r["status"] == "failed":
                ev.log(r.get("tail", ""))
    finally:
        pg.stop()
        left = subprocess.run(["docker", "ps", "-aq", "--filter", f"name={pg.name}"], capture_output=True, text=True).stdout.split()
        ev.log(f"cleanup: {len(left)} postgres container left")
    summary = {"run_id": args.run_id, "lane": LANE_NAME, "targets": len(targets), "passed": sum(r["status"] == "passed" for r in rows),
               "failed": sum(r["status"] == "failed" for r in rows), "skipped": sum(r["status"] == "skipped" for r in rows), "rows": rows}
    ev.write("live-suites.json", json.dumps(summary, indent=2, ensure_ascii=False) + "\n")
    md = [f"# Live suites, run {args.run_id}", "", f"{summary['passed']} passed, {summary['failed']} failed, {summary['skipped']} skipped of {len(targets)} ignored targets.", "",
          "| target | kind | status | seconds | note |", "|---|---|---|---|---|"]
    for r in rows:
        md.append(f"| {r['target']} | {r['kind']} | {r['status']} | {r.get('seconds', '')} | {r.get('reason') or r.get('via') or ''} |")
    ev.write("live-suites.md", "\n".join(md) + "\n")
    print(f"# {summary['passed']}/{len(targets)} targets passed, {summary['failed']} failed, {summary['skipped']} skipped")
    incomplete = summary["skipped"] and not (only or skip)  # a full gate run leaves no target skipped
    return 0 if summary["failed"] == 0 and not incomplete else 1


if __name__ == "__main__":
    sys.exit(main())
