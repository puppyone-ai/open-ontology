"""Prove failpoints are absent from a release build and present in the test build.

python3 tools/oo-test/failpoints_release.py --release target/ta-rel/release/oo-load \\
    --failpoints target/ta-fp/debug/oo-load

The release binary must contain neither the trigger variable nor any point
name; the failpoints build must contain all of them (so the check is not
vacuous). Point names are read from the product sources, not hard-coded.
"""
import argparse
import json
from pathlib import Path
import re
import sys

import runner

SOURCES = runner.ROOT / "crates/oo-pipeline/src"


def point_names():
    names = set()
    for path in SOURCES.rglob("*.rs"):
        names.update(re.findall(r'failpoint::hit\("([a-z0-9_]+)"\)', path.read_text()))
    return sorted(names)


def markers(binary, names):
    data = Path(binary).read_bytes()
    return {name: name.encode() in data for name in ["OO_FAILPOINTS", *names]}


def check(release, failpoints):
    names = point_names()
    if not names:
        raise SystemExit("no failpoints found in product sources")
    shipped, test = markers(release, names), markers(failpoints, names)
    report = {"points": names, "release": shipped, "failpoints_build": test,
              "release_clean": not any(shipped.values()),
              "test_build_complete": all(test.values())}
    report["status"] = ("passed" if report["release_clean"] and report["test_build_complete"]
                        else "failed")
    return report


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--release", type=Path, required=True)
    parser.add_argument("--failpoints", type=Path, required=True)
    parser.add_argument("--out", type=Path)
    args = parser.parse_args(argv)
    report = check(args.release, args.failpoints)
    if args.out:
        runner.write_json(args.out, report)
    print(json.dumps(report))
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    sys.exit(main())
