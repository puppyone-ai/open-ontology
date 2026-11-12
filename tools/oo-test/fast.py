"""oo-test fast --layer L0|L1|L2: the daily accelerated suite (plan section 1).

L0  in-process contracts: Rust unit tests of the product crates, the Python
    harness/lite suites. No source database.
L1  per engine x version on a small boundary snapshot clone shared by suites:
    T0 doctor, T1/T8 (chunk-shrunk) and T7 statement audit on every version;
    T2 (CDC) and T4 (DDL) on each equivalence class's daily representative.
L2  per engine one rotating representative: T3 faults made deterministic
    with failpoints and toxiproxy (each paired with its real-fault L3 case).
L3  the existing full commands (matrix, t2, t3t4, t7), unchanged.

Every cell writes ordinary verdict rows and timing receipts, so
`oo-test timing evidence/<run-id>` gives the per-stage table.
"""
import argparse
import datetime
import json
import os
from pathlib import Path
import re
import subprocess
import time
import traceback

import layers
import runner

ROOT = runner.ROOT
HERE = Path(__file__).resolve().parent
T4_CASES = ("add_column", "drop_column", "rename_column", "widen_type", "widen_string",
            "add_table", "drop_table", "pk_change")
# L2 fault cases with a deterministic fast variant (failpoint / toxiproxy).
L2_CASES = ("source_restart_mid_chunk", "network_drop_mid_c1", "network_drop_mid_c5",
            "sigkill_mid_cycle", "retention_expiry", "slot_invalidation")
FAILPOINT_BINARY = ROOT / "target/ta-fp/debug/oo-load"

L0_COMMANDS = [
    ("rust-core-pipeline", ["cargo", "test", "--locked", "-p", "oo-core", "-p", "oo-pipeline",
                            "--lib"]),
    ("rust-connectors", ["cargo", "test", "--locked", "-p", "oo-connectors", "--lib",
                         "--features", "mysql,oracle"]),
    ("rust-cli", ["cargo", "test", "--locked", "-p", "oo-cli", "--lib"]),
    ("rust-oo-load", ["cargo", "test", "--locked", "--manifest-path", "tools/oo-load/Cargo.toml",
                      "--bins"]),
    ("rust-failpoints", ["cargo", "test", "--locked", "-p", "oo-pipeline", "--features",
                         "failpoints", "--lib", "failpoint"]),
    ("py-oo-test", ["python3", "-m", "unittest", "discover", "-s", "tools/oo-test"]),
    ("py-lite", ["python3", "-m", "unittest", "discover", "-s", "deploy/lite"]),
]


def env(lane=True):
    # L0 unit suites assert their own default lanes; only live layers own lane ta.
    value = dict(os.environ, PYTHONDONTWRITEBYTECODE="1", CARGO_BUILD_JOBS="4",
                 CARGO_INCREMENTAL="0")
    if lane:
        value["OO_TEST_LANE"] = "ta"
    else:
        value.pop("OO_TEST_LANE", None)
    return value


def timed_command(name, command, root, timeout, lane=True):
    log = root / (name + ".log")
    started = time.monotonic()
    if command[0] == "cargo":
        command = ["nice", "-n", "10", *command]
    with log.open("wb") as out:
        try:
            code = subprocess.run(command, cwd=ROOT, env=env(lane), stdout=out,
                                  stderr=subprocess.STDOUT, timeout=timeout).returncode
        except subprocess.TimeoutExpired:
            code = 124
    return {"name": name, "exit": code, "status": "passed" if code == 0 else "failed",
            "seconds": round(time.monotonic() - started, 3)}


def run_l0(root, args):
    results = []
    for name, command in L0_COMMANDS:
        results.append(timed_command(name, command, root, 1800, lane=False))
        print(json.dumps(results[-1]), flush=True)
    return results


# -- L1 -----------------------------------------------------------------------
def platform_for(engine, version):
    arch = subprocess.check_output(["docker", "info", "--format", "{{.Architecture}}"], text=True)
    native = "linux/arm64" if arch.strip() in ("arm64", "aarch64") else "linux/amd64"
    item = runner.resolve(engine, version)
    return native if native in item["platforms"] else "linux/amd64"


def run_shared_cell(cell, path, args):
    """T0/T1/T7/T8 on one snapshot clone; each suite leases it in turn."""
    import shared
    import snapshots
    import t1
    t1.LANE = "ta"
    os.environ["OO_TEST_LANE"] = "ta"
    os.environ["OO_TEST_CHUNK_ROWS"] = str(args.chunk_rows)
    os.environ["OO_TEST_KEYLESS_ROWS"] = str(args.keyless_rows)
    platform = platform_for(cell["engine"], cell["version"])
    options = argparse.Namespace(scale=args.scale, tables=args.tables, platform=platform,
                                 health_timeout=None, variant=None, suites=["T0", "T1", "T7"])
    key, manifest = snapshots.identity(cell["engine"], cell["version"], args.scale,
                                       args.tables, platform)
    hooks = {"T0": str(HERE / "t0.py"), "T1": str(HERE / "t1.py")}
    verdicts, run = {}, None
    try:
        snapshots.capacity(cell["engine"])
        with snapshots.locked_cache(key) as cache:
            run = snapshots.SnapshotRun.__new__(snapshots.SnapshotRun)
            run.__init__(path, cell["engine"], cell["version"], options, cache, key, manifest)
            run.setup()
            run.seed()
            if cell["engine"] == "oracle":
                # No in-instance pristine copy for Oracle (LOBs, constraints):
                # suites run in sequence on this clone, as in the full matrix.
                run.args.suites = ["T0", "T1", "T7", "T8"]
                suites = ["T0", "T1", "T7", "T8"] if "T1" in cell["full"] else ["T0", "T7"]
                verdicts.update(runner.execute_suites(run, suites, hooks))
                return verdicts
            source = shared.SharedSource(run)
            source.prepare()
            # T0 is the capability probe for every version.
            with source.lease("T0"):
                verdicts.update(runner.execute_suites(run, ["T0"], hooks))
            requested = ["T1", "T7", "T8"] if "T1" in cell["full"] else ["T7"]
            with source.lease("T1" if "T1" in requested else "T7"):
                # T7 audits the statements of what this lease runs.
                if "T1" not in requested:
                    run.args.suites = ["T0", "T7"]
                    verdicts.update({k: v for k, v in runner.execute_suites(
                        run, ["T0", "T7"], hooks).items() if k == "T7"})
                else:
                    verdicts.update(runner.execute_suites(run, requested, hooks))
    except Exception as error:
        verdicts["error"] = str(error) if isinstance(error, RuntimeError) else type(error).__name__
        verdicts["trace"] = [f"{Path(f.filename).name}:{f.lineno}"
                             for f in traceback.extract_tb(error.__traceback__)][-6:]
    finally:
        if run is not None and hasattr(run, "private") and run.cleanup():
            verdicts["cleanup_failed"] = True
    return verdicts


def run_suite_command(suite, cell, root, args):
    rid = f"{args.run_id}-{suite.lower()}-{cell['engine']}-{cell['version'].replace('.', '')}"
    if suite == "T2":
        command = (["python3", "tools/oo-test/t2.py", "--versions", cell["version"], "--run-id", rid]
                   if cell["engine"] == "postgres" else
                   ["python3", "tools/oo-test/t2_p0.py", "--engine", cell["engine"],
                    "--versions", cell["version"], "--run-id", rid])
    else:
        command = ["tools/oo-test/oo-test", "t3t4", "--engine", cell["engine"],
                   "--versions", cell["version"], "--run-id", rid,
                   "--cases", ",".join(args.t4_cases), "--disk-wait-minutes", "5"]
    result = timed_command(rid, command, root, 3600)
    result["evidence"] = "evidence/" + rid
    return result


def oracle_free():
    import snapshots
    try:
        snapshots.capacity("oracle")
        return True
    except RuntimeError as error:
        if str(error) == "oracle_slot_occupied":
            return False
        raise


def run_l1(root, args):
    day = args.day
    engines = args.engines
    cells = layers.l1_cells(engines, ["T0", "T1", "T2", "T4", "T7", "T8"], day)
    budget = layers.docker_budget_gb()
    waves = layers.schedule(cells, budget)
    runner.write_json(root / "l1-plan.json", {"day": str(day), "budget_gb": budget,
                                              "waves": waves})
    results = []
    for wave in waves:
        # Waves hold one cell under this host's caps; run in plan order.
        for cell in wave["cells"]:
            name = f"{cell['engine']}-{cell['version']}"
            path = root / name
            path.mkdir(parents=True, mode=0o700)
            started = time.monotonic()
            entry = dict(cell, suites={}, commands=[])
            if cell["engine"] == "oracle" and not oracle_free():
                entry["status"] = "pending"
                entry["reason"] = "oracle_slot_occupied"
            else:
                entry["suites"] = run_shared_cell(cell, path, args)
                for suite in ("T2", "T4"):
                    if suite in cell["full"] and not args.skip_cdc:
                        command = run_suite_command(suite, cell, root, args)
                        entry["commands"].append(command)
                        entry["suites"][suite] = command["status"]
                verdicts = [v for k, v in entry["suites"].items() if re.fullmatch(r"T\d", k)]
                entry["status"] = ("passed" if verdicts and "error" not in entry["suites"]
                                   and all(v == "passed" for v in verdicts) else "failed")
            entry["seconds"] = round(time.monotonic() - started, 3)
            results.append(entry)
            runner.write_json(root / "l1-results.json", results)
            print(json.dumps({k: entry[k] for k in ("engine", "version", "status", "seconds")}
                             | {"suites": entry["suites"]}), flush=True)
    return results


def run_l2(root, args):
    results = []
    for engine in args.engines:
        version = layers.representative(layers.P0[engine], args.day)
        cell = {"engine": engine, "version": version}
        if engine == "oracle" and not oracle_free():
            results.append(dict(cell, status="pending", reason="oracle_slot_occupied"))
            continue
        rid = f"{args.run_id}-l2-{engine}-{version.replace('.', '')}"
        command = ["tools/oo-test/oo-test", "t3t4", "--engine", engine, "--versions", version,
                   "--run-id", rid, "--cases", ",".join(args.l2_cases),
                   "--fast-faults", "--binary", str(FAILPOINT_BINARY),
                   "--disk-wait-minutes", "5"]
        result = timed_command(rid, command, root, 3600)
        results.append(dict(cell, **result, evidence="evidence/" + rid))
        runner.write_json(root / "l2-results.json", results)
        print(json.dumps(results[-1]), flush=True)
    return results


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--layer", action="append", choices=("L0", "L1", "L2"), required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--engines", default="postgres,mysql,oracle")
    parser.add_argument("--base", help="select L1/L2 engines by impact of BASE...HEAD")
    parser.add_argument("--coverage-export", action="append", default=[], metavar="ENGINE=PATH")
    parser.add_argument("--day", help="rotation day, YYYY-MM-DD (default today)")
    # The boundary fixture whose product coverage equals the 2M-row run's
    # (evidence ta-01-cov-diff4): 23000 rows keep a partial last chunk and a
    # >64 KiB COPY buffer at the default 10000-row chunks.
    parser.add_argument("--scale", type=int, default=23000)
    parser.add_argument("--tables", type=int, default=3)
    parser.add_argument("--chunk-rows", type=int, default=10000)
    parser.add_argument("--keyless-rows", type=int, default=4000)
    parser.add_argument("--skip-cdc", action="store_true", help="omit T2/T4 commands")
    parser.add_argument("--plan-only", action="store_true")
    args = parser.parse_args(argv)
    if not re.fullmatch(r"[a-z0-9][a-z0-9-]{0,80}", args.run_id):
        parser.error("invalid run id")
    args.day = (datetime.date.fromisoformat(args.day) if args.day else layers.today())
    args.engines = [e for e in args.engines.split(",") if e]
    if set(args.engines) - set(layers.P0):
        parser.error("unknown engine")
    args.t4_cases, args.l2_cases = T4_CASES, L2_CASES
    selection = None
    if args.base:
        maps = {}
        for item in args.coverage_export:
            engine, _, path = item.partition("=")
            maps[engine] = layers.coverage_map(path)
        engines, reasons = layers.impacted(layers.changed_files(args.base), maps)
        args.engines = [e for e in args.engines if e in engines]
        selection = {"base": args.base, "engines": args.engines, "reasons": reasons}
    root = ROOT / "evidence" / args.run_id
    if args.plan_only:
        plan = {"selection": selection, "day": str(args.day),
                "l1": layers.l1_cells(args.engines, ["T0", "T1", "T2", "T4", "T7", "T8"], args.day),
                "l2": {e: layers.representative(layers.P0[e], args.day) for e in args.engines}}
        print(json.dumps(plan, indent=2))
        return 0
    root.mkdir(parents=True, mode=0o700)
    report = {"format": "oo-fast/v1", "run_id": args.run_id, "day": str(args.day),
              "selection": selection, "layers": {}}
    sha = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    report["sha"] = sha
    started = time.monotonic()
    for layer in args.layer:
        layer_root = root / layer
        layer_root.mkdir(exist_ok=True, mode=0o700)
        layer_started = time.monotonic()
        rows = {"L0": run_l0, "L1": run_l1, "L2": run_l2}[layer](layer_root, args)
        report["layers"][layer] = {"seconds": round(time.monotonic() - layer_started, 3),
                                   "results": rows}
        runner.write_json(root / "fast-report.json", report)
    report["wall_seconds"] = round(time.monotonic() - started, 3)
    statuses = [r.get("status") for layer in report["layers"].values() for r in layer["results"]]
    report["status"] = ("passed" if statuses and all(s == "passed" for s in statuses) else
                        "pending" if statuses and all(s in ("passed", "pending") for s in statuses)
                        else "failed")
    runner.write_json(root / "fast-report.json", report)
    print(json.dumps({k: report[k] for k in ("run_id", "status", "wall_seconds")}))
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
