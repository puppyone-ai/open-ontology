"""L3 nightly rotation, fault-pair check and fast/full differential.

oo-test l3 plan [--day D]           today's L3 slice (one engine, all its versions)
oo-test l3 run --run-id ID [--day D] run that slice with the existing full commands
oo-test l3 pairs FAST REAL          L2 fast-fault vs L3 real-fault verdicts per case
oo-test l3 diff FAST_REPORT FULL... fast vs full verdicts per (engine, version, suite)

L3 commands are the existing full ones, unchanged: matrix T0/T1/T8 at 2M rows,
T2, T3/T4 all cases, T7. A three-day rotation covers every P0 engine; the
release run still executes all of them once.
"""
import argparse
import datetime
import json
from pathlib import Path
import subprocess
import sys

import layers
import runner

ROOT = runner.ROOT
ROTATION = ("postgres", "mysql", "oracle")
VERSION_ARG = {"postgres": "13,14,15,16,17,18", "mysql": "5.7,8.0,8.4,9",
               "oracle": "23,21,11,19"}


def slice_for(day):
    engine = ROTATION[day.toordinal() % len(ROTATION)]
    versions = VERSION_ARG[engine]
    rid = f"l3-{engine}-{day.isoformat()}"
    commands = [
        ["tools/oo-test/oo-test", "matrix", "--engine", engine, "--versions",
         versions.replace("23,21,11", "23ai,21c,11g"), "--suites", "T0,T1,T8",
         "--hook", "T0=tools/oo-test/t0.py", "--hook", "T1=tools/oo-test/t1.py",
         "--min-free-gb", "30"],
        (["python3", "tools/oo-test/t2.py", "--versions", versions, "--run-id", rid + "-t2"]
         if engine == "postgres" else
         ["python3", "tools/oo-test/t2_p0.py", "--engine", engine, "--versions", versions,
          "--run-id", rid + "-t2"]),
        ["tools/oo-test/oo-test", "t3t4", "--engine", engine, "--versions", versions,
         "--run-id", rid + "-t3t4", "--min-free-gb", "30"],
        ["python3", "tools/oo-test/t7.py", "--engine", engine, "--versions", versions,
         "--run-id", rid + "-t7"],
    ]
    return {"day": day.isoformat(), "engine": engine, "commands": commands}


def pairs(fast_path, real_path):
    """Each fast fault (failpoint/toxiproxy) must reach the real fault's conclusion."""
    import t3t4
    fast = json.loads(Path(fast_path).read_text())
    real = json.loads(Path(real_path).read_text())
    rows = []
    for case, mechanism in t3t4.FAST_PAIRS.items():
        a = fast.get("cases", {}).get(case, {}).get("status")
        b = real.get("cases", {}).get(case, {}).get("status")
        rows.append({"case": case, "mechanism": mechanism, "fast": a, "real": b,
                     "agree": a is not None and a == b})
    return {"pairs": rows, "status": "passed" if all(r["agree"] for r in rows) else "failed"}


def full_verdicts(paths):
    """(engine, version, suite) -> the latest full-suite verdict; reruns supersede."""
    latest = {}
    for path in paths:
        for rows_file in sorted(Path(path).rglob("index-rows.json")):
            for row in json.loads(rows_file.read_text()):
                engine = next(e for e in ("postgres", "mysql", "oracle", "sqlserver")
                              if row["target"].startswith(e))
                parts = row["version"].split(".")
                version = ".".join(parts[:2] if engine == "mysql" else parts[:1])
                key = (engine, version, row["suite"])
                stamp = (row.get("observed_at", ""), row.get("run_id", ""))
                results = latest.setdefault(key, {})
                results.setdefault(stamp, set()).add(row["result"])
    out = {}
    for key, runs in latest.items():
        values = runs[max(runs)]
        out[key] = "fail" if "fail" in values else "pass" if "pass" in values else "skip"
    return out


def fast_verdicts(report):
    out = {}
    for entry in report.get("layers", {}).get("L1", {}).get("results", []):
        version = entry["version"]
        if entry["engine"] == "mysql" and version == "9":
            version = "9.5"
        for suite, value in entry.get("suites", {}).items():
            if suite.startswith("T") and len(suite) == 2:
                out[(entry["engine"], version, suite)] = "pass" if value == "passed" else "fail"
    for entry in report.get("layers", {}).get("L2", {}).get("results", []):
        version = entry["version"]
        out[(entry["engine"], version, "T3")] = ("pass" if entry.get("status") == "passed"
                                                 else entry.get("status", "fail"))
    return out


def diff(fast_report, full_paths):
    fast = fast_verdicts(json.loads(Path(fast_report).read_text()))
    full = full_verdicts(full_paths)
    rows = []
    for key in sorted(set(fast) | set(full)):
        a, b = fast.get(key), full.get(key)
        rows.append({"engine": key[0], "version": key[1], "suite": key[2], "fast": a, "full": b,
                     "comparable": a is not None and b is not None,
                     "agree": a == b if a is not None and b is not None else None})
    comparable = [r for r in rows if r["comparable"]]
    return {"rows": rows, "comparable": len(comparable),
            "agree": sum(r["agree"] for r in comparable),
            "disagree": [r for r in comparable if not r["agree"]]}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    for name in ("plan", "run"):
        p = sub.add_parser(name)
        p.add_argument("--day")
        if name == "run":
            p.add_argument("--run-id", required=True)
    p = sub.add_parser("pairs")
    p.add_argument("fast")
    p.add_argument("real")
    p = sub.add_parser("diff")
    p.add_argument("fast_report")
    p.add_argument("full", nargs="+")
    p.add_argument("--out", type=Path)
    args = parser.parse_args(argv)
    if args.command in ("plan", "run"):
        day = datetime.date.fromisoformat(args.day) if args.day else layers.today()
        plan = slice_for(day)
        if args.command == "plan":
            print(json.dumps(plan, indent=2))
            return 0
        root = ROOT / "evidence" / args.run_id
        root.mkdir(parents=True, mode=0o700)
        results = []
        for command in plan["commands"]:
            code = subprocess.run(command, cwd=ROOT).returncode
            results.append({"command": command, "exit": code})
            runner.write_json(root / "l3-results.json", dict(plan, results=results))
        if plan["engine"] != "oracle":
            # Nightly pair check: the L2 fast faults must reach the real faults' verdicts.
            import t3t4
            version = layers.representative(layers.P0[plan["engine"]], day)
            fast_rid = f"l3-pairs-{plan['engine']}-{day.isoformat()}"
            code = subprocess.run(
                ["tools/oo-test/oo-test", "t3t4", "--engine", plan["engine"], "--versions", version,
                 "--run-id", fast_rid, "--fast-faults", "--cases", ",".join(t3t4.FAST_PAIRS),
                 "--binary", "target/ta-fp/debug/oo-load"], cwd=ROOT).returncode
            name = f"{plan['engine']}-{version}/t3t4.json"
            report = pairs(ROOT / "evidence" / fast_rid / name,
                           ROOT / "evidence" / (plan["commands"][2][7]) / name)
            results.append({"pairs": report, "exit": 0 if report["status"] == "passed" else 1,
                            "fast_exit": code})
            runner.write_json(root / "l3-results.json", dict(plan, results=results))
        return 0 if all(r["exit"] == 0 for r in results) else 1
    if args.command == "pairs":
        report = pairs(args.fast, args.real)
        print(json.dumps(report, indent=2))
        return 0 if report["status"] == "passed" else 1
    report = diff(args.fast_report, args.full)
    if args.out:
        runner.write_json(args.out, report)
    print(json.dumps({k: v for k, v in report.items() if k != "rows"}, indent=2))
    return 0 if not report["disagree"] else 1


if __name__ == "__main__":
    sys.exit(main())
