#!/usr/bin/env python3
"""`oo-test l3 run --suite super-ontology` (30 L3 acceptance, 10 section 13 B-S3-08, B-S4-06).

  oo-test l3 run --suite super-ontology --run-id ID                  acceptance 1, 3-5, 8-10 (super_ontology_l3.py)
  oo-test l3 run --suite super-ontology --only faults --run-id ID    the fault injection suite (10 12.5)
  oo-test l3 run --suite super-ontology --bench cold1000 --run-id ID the 1000 file cold bench (L3-6)

`--only` for the acceptance run takes the phase names of super_ontology_l3.py.
The faults run starts one labelled Postgres container, runs the `--ignored`
tests of `crates/oo-docs/tests/faults.rs` (failpoints feature) and writes the
result to testing/super-ontology/evidence/L3/faults-run.txt.
"""

import argparse
import os
import re
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
LANE = "oo.lane=super-ontology-be1"
PG_IMAGE = "postgres:17.11-bookworm"
EVIDENCE = REPO / "testing/super-ontology/evidence/L3"


def faults(run_id):
    name = f"oo-be1-faults-{run_id}"[:60]
    out = EVIDENCE / "faults-run.txt"
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    subprocess.run(["docker", "run", "-d", "--name", name, "--label", LANE, "-e", "POSTGRES_HOST_AUTH_METHOD=trust",
                    "-p", "127.0.0.1::5432", PG_IMAGE, "-c", "max_connections=300", "-c", "fsync=off",
                    "-c", "track_commit_timestamp=on"], check=True, capture_output=True)
    try:
        port = subprocess.check_output(["docker", "port", name, "5432/tcp"], text=True).strip().splitlines()[0].rsplit(":", 1)[1]
        for _ in range(60):
            if subprocess.run(["docker", "exec", name, "pg_isready", "-U", "postgres"], capture_output=True).returncode == 0:
                break
            time.sleep(1)
        time.sleep(1)
        env = dict(os.environ, OO_DOCS_TEST_PG_PORT=port, OO_DOCS_TEST_PG_CONTAINER=name,
                   CARGO_BUILD_JOBS=os.environ.get("CARGO_BUILD_JOBS", "6"))
        argv = ["cargo", "test", "-p", "oo-docs", "--locked", "--features", "failpoints", "--test", "faults",
                "--", "--ignored", "--test-threads=1", "--nocapture"]
        r = subprocess.run(argv, cwd=REPO, env=env, capture_output=True, text=True)
        lines = [l for l in (r.stdout + r.stderr).splitlines() if re.search(r"^test |test result|poison:|attempts|panicked|error", l)]
        out.write_text(f"$ {' '.join(argv)}\n" + "\n".join(lines) + f"\n(exit {r.returncode})\n")
        print("\n".join(lines[-14:]))
        return r.returncode
    finally:
        subprocess.run(["docker", "rm", "-f", "-v", name], capture_output=True)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("command", choices=["run"])
    ap.add_argument("--suite", required=True, choices=["super-ontology"])
    ap.add_argument("--run-id", required=True)
    ap.add_argument("--only")
    ap.add_argument("--bench", choices=["cold1000"])
    ap.add_argument("--skip-build", action="store_true")
    args = ap.parse_args(argv)
    if not re.fullmatch(r"[a-z0-9][a-z0-9-]{0,60}", args.run_id):
        ap.error("invalid run id")
    if args.bench:
        cmd = [sys.executable, "-B", str(HERE / "super_ontology_bench.py"), "--files", "1000", "--run-id", args.run_id]
    elif args.only == "faults":
        return faults(args.run_id)
    else:
        cmd = [sys.executable, "-B", str(HERE / "super_ontology_l3.py"), "--run-id", args.run_id]
        if args.only:
            cmd += ["--only", args.only]
    if args.skip_build:
        cmd.append("--skip-build")
    return subprocess.run(cmd, cwd=REPO).returncode


if __name__ == "__main__":
    sys.exit(main())
