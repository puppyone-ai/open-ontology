"""Coverage differential: does the small boundary fixture reach every product
region the 2M-row fixture reaches (plan section 2.3)?

oo-test coverage --engine postgres --version 17 --run-id ID --binary target/ta-cov/debug/oo-load

Runs product T1 (load, replay, C5 correction, keyless idle) twice with an
instrument-coverage build: once on the small fixture with shrunken chunks,
once on the large fixture. Regions are compared by (file, span); the run
passes only when no product region executed by the large run is missed by
the small one. `--profiles` re-diffs existing profdata without a database.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import time

import runner
import snapshots
import t1

LLVM = Path(os.environ.get("OO_LLVM_BIN", "/opt/homebrew/opt/llvm@22/bin"))
PRODUCT = ("crates/", "tools/oo-load/src/")


def product_file(name):
    try:
        relative = str(Path(name).resolve().relative_to(runner.ROOT.resolve()))
    except ValueError:
        return None
    if "/vendor/" in relative or not relative.startswith(PRODUCT):
        return None
    return relative


def regions(export):
    """Covered and instrumented code regions keyed by (file, span)."""
    seen, covered = set(), set()
    for data in export["data"]:
        for function in data["functions"]:
            names = function["filenames"]
            for region in function["regions"]:
                line, column, end_line, end_column, count, file_id, _, kind = region[:8]
                if kind != 0:
                    continue
                name = product_file(names[file_id])
                if name is None:
                    continue
                key = (name, line, column, end_line, end_column)
                seen.add(key)
                if count:
                    covered.add(key)
    return seen, covered


def export(binary, profdata, out):
    raw = subprocess.check_output([str(LLVM / "llvm-cov"), "export", "--format=text",
                                   "--skip-expansions", str(binary),
                                   "-instr-profile", str(profdata)])
    out.write_bytes(raw)
    return json.loads(raw)


def merge(directory):
    files = sorted(directory.glob("*.profraw"))
    if not files:
        raise RuntimeError("no_profiles_written")
    profdata = directory.with_suffix(".profdata")
    subprocess.run([str(LLVM / "llvm-profdata"), "merge", "-sparse", "-o", str(profdata),
                    *map(str, files)], check=True)
    return profdata


def summarize(seen, covered):
    files = {}
    for key in seen:
        entry = files.setdefault(key[0], [0, 0])
        entry[0] += 1
        entry[1] += key in covered
    return {"regions": len(seen), "covered": len(covered),
            "percent": round(100 * len(covered) / len(seen), 2) if seen else None,
            "files": {f: {"regions": n, "covered": c} for f, (n, c) in sorted(files.items())}}


def diff(binary, small, large, root):
    seen, small_covered = regions(export(binary, small, root / "small-export.json"))
    _, large_covered = regions(export(binary, large, root / "large-export.json"))
    missing = sorted(large_covered - small_covered)
    report = {
        "format": "oo-coverage-diff/v1",
        "small": summarize(seen, small_covered), "large": summarize(seen, large_covered),
        "missing_in_small": [{"file": f, "span": [a, b, c, d]} for f, a, b, c, d in missing],
        "extra_in_small": len(small_covered - large_covered),
        "status": "passed" if not missing else "failed",
    }
    report["large"].pop("files")
    runner.write_json(root / "coverage-diff.json", report)
    return report


def run_side(root, label, engine, version, scale, tables, binary, chunk_rows, keyless_rows=0):
    path = root / label
    path.mkdir(mode=0o700)
    profiles = root / (label + "-profiles")
    profiles.mkdir(mode=0o700)
    arch = subprocess.check_output(["docker", "info", "--format", "{{.Architecture}}"], text=True)
    native = "linux/arm64" if arch.strip() in ("arm64", "aarch64") else "linux/amd64"
    item = runner.resolve(engine, version)
    platform = native if native in item["platforms"] else "linux/amd64"
    options = argparse.Namespace(scale=scale, tables=tables, platform=platform, health_timeout=None)
    env = {"LLVM_PROFILE_FILE": str(profiles / "%p-%m.profraw"), "OO_TEST_CHUNK_ROWS": str(chunk_rows),
           "OO_TEST_KEYLESS_ROWS": str(keyless_rows)}
    saved = {k: os.environ.get(k) for k in env}
    os.environ.update(env)
    started = time.monotonic()
    result = {"label": label, "scale": scale, "tables": tables, "chunk_rows": chunk_rows,
              "keyless_rows": keyless_rows}
    run = None
    try:
        snapshots.capacity(engine)
        key, manifest = snapshots.identity(engine, version, scale, tables, platform)
        with snapshots.locked_cache(key) as cache:
            run = snapshots.SnapshotRun.__new__(snapshots.SnapshotRun)
            run.__init__(path, engine, version, options, cache, key, manifest)
            run.setup()
            run.seed()
            report = t1.run_t1(run, binary)
        result.update(T1=report["T1"], T8=report["T8"], failure=report.get("failure"))
    finally:
        for name, value in saved.items():
            if value is None:
                os.environ.pop(name, None)
            else:
                os.environ[name] = value
        if run is not None and hasattr(run, "private"):
            result["cleanup_failed"] = run.cleanup()
        result["seconds"] = round(time.monotonic() - started, 3)
        runner.write_json(path / "coverage-side.json", result)
    return result, merge(profiles)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--engine", choices=("postgres", "mysql"), default="postgres")
    parser.add_argument("--version", default="17")
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--small-scale", type=int, default=23000)
    parser.add_argument("--keyless-rows", type=int, default=4000,
                        help="small side only: pre-grown keyless rows (boundary shape)")
    parser.add_argument("--small-tables", type=int, default=3)
    parser.add_argument("--chunk-rows", type=int, default=10000)
    parser.add_argument("--large-scale", type=int, default=2_000_000)
    parser.add_argument("--large-tables", type=int, default=200)
    parser.add_argument("--profiles", nargs=2, type=Path, metavar=("SMALL", "LARGE"),
                        help="diff existing profdata files only")
    parser.add_argument("--sides", default="small,large")
    args = parser.parse_args(argv)
    t1.LANE = "ta"
    root = runner.ROOT / "evidence" / args.run_id
    root.mkdir(mode=0o700, parents=True, exist_ok=bool(args.profiles))
    binary = args.binary.resolve()
    if args.profiles:
        report = diff(binary, *args.profiles, root)
    else:
        sides, profiles = {}, {}
        for label in args.sides.split(","):
            scale, tables, chunk, keyless = (
                (args.small_scale, args.small_tables, args.chunk_rows, args.keyless_rows)
                if label == "small" else (args.large_scale, args.large_tables, 10000, 0))
            sides[label], profiles[label] = run_side(
                root, label, args.engine, args.version, scale, tables, binary, chunk, keyless)
            runner.write_json(root / "sides.json", sides)
        if set(profiles) != {"small", "large"}:
            print(json.dumps(sides))
            return 0
        report = diff(binary, profiles["small"], profiles["large"], root)
        report["sides"] = sides
        runner.write_json(root / "coverage-diff.json", report)
    print(json.dumps({k: v for k, v in report.items() if k not in ("missing_in_small",)}
                     | {"missing_count": len(report["missing_in_small"])}, default=str)[:4000])
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
