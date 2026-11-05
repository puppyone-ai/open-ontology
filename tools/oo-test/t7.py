#!/usr/bin/env python3
"""T7 read-only proof (S5-H-01, AT-ENG-03): audited product T1 and T2 workloads.

For each P0 version, two owned fixtures run with source auditing active from
before the reader's first connection until after its last disconnect:

* snapshot: the T1 product load (concurrent, corrected and replayed loads);
* incremental: the T2 product cycles (C1 and C5 where implemented).

Every audited reader record is classified by audit.classify. Records inside a
product invocation window are attributed to that invocation's variant; the
remaining reader records are harness reads and are held to snapshot rules.
Any rejected record anywhere in a fixture fails every variant of the fixture.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import signal
import sys
import traceback

if os.environ.get("OO_TEST_LANE") not in ("h", "m", "e", "p", "g"):
    os.environ["OO_TEST_LANE"] = "m"  # t1 import guard; the lane label is set below.
import runner  # noqa: E402
import t1  # noqa: E402
import t2  # noqa: E402
import t2_p0  # noqa: E402
from audit import Audit  # noqa: E402
from verdicts import write_rows  # noqa: E402

LANE = "t"
ROOT = runner.ROOT
VERSIONS = {"mysql": ["5.7", "8.0", "8.4", "9"],
            "postgres": ["13", "14", "15", "16", "17", "18"],
            "oracle": ["23", "21", "11"]}
INCREMENTAL_VARIANTS = {"mysql": ("c1", "c5"), "postgres": ("c1", "c5"), "oracle": ("c5",)}


def variant_of(command, kind):
    if "--c5" in command:
        return "c5"
    if "--sync" in command or "--handoff" in command:
        return "c1"
    if kind == "snapshot":
        return "snapshot"
    source = command[command.index("--source") + 1] if "--source" in command else ""
    return "c5" if "c5" in source else "c1"


def audited(base):
    class AuditedRun(base):
        def __init__(self, path, engine, version, args, binary, kind):
            super().__init__(path, engine, version, args)
            self.name = "oo-t-" + self.id
            self.volume = self.name + "-data"
            self.binary, self.kind = str(binary), kind
            self.audit = Audit(self)
            self.windows = []
            self.audit_enabled = False

        def command(self, command, **kwargs):
            if (command[:2] == ["docker", "create"] and self.name in command
                    and self.engine == "mysql"):
                # Server options, so a fixture restart cannot reset logging.
                command = [*command, "--general-log=1", "--log-output=TABLE"]
            if not command or command[0] != self.binary:
                result = super().command(command, **kwargs)
                data = kwargs.get("data") or ""
                if ("sqlplus" in command and "CREATE USER C##OO_MINER" in data
                        and self.audit_enabled):
                    self.audit.enable_miner()
                return result
            window = {"seq": len(self.windows) + 1, "variant": variant_of(command, self.kind),
                      "args": [a for a in command[1:] if a.startswith("--")],
                      "source": command[command.index("--source") + 1],
                      "start": self.audit.clock(), "exit": None}
            self.windows.append(window)
            try:
                code, out = super().command(command, **kwargs)
                window["exit"] = code
                return code, out
            except RuntimeError:
                window["exit"] = "raised"
                raise
            finally:
                window["end"] = self.audit.clock()
                window["command_seq"] = self.seq

        def setup(self):
            super().setup()
            if self.engine != "oracle":
                self.audit.enable()
                self.audit_enabled = True

        def seed(self):
            super().seed()
            if self.engine == "oracle" and not self.audit_enabled:
                # The reader exists only after seed; standard audit needs its restart.
                self.audit.enable()
                self.audit_enabled = True
    return AuditedRun


def coverage(kind, engine, version, workload, windows):
    """Return {variant: (covered, reason)} from the workload's own verdicts."""
    if kind == "snapshot":
        loads = [w for w in windows if w["variant"] == "snapshot"]
        complete = len(loads) >= 3 and all(w["exit"] == 0 for w in loads)
        if workload.get("T1") == "passed" and complete:
            return {"snapshot": (True, "T1 passed")}
        if complete and workload.get("failure") == "source target difference":
            # Product reads completed; the comparison contract (D17) is T1's verdict.
            return {"snapshot": (True, "all product loads completed; T1 comparison failed")}
        return {"snapshot": (False, "product snapshot loads incomplete: " + str(workload.get("failure")))}
    result = {}
    for variant in INCREMENTAL_VARIANTS[engine] + (("c1",) if engine == "oracle" and version == "23" else ()):
        status = workload.get(variant.upper())
        result[variant] = ((True, variant.upper() + " passed") if status == "passed"
                           else (False, f"{variant.upper()} workload {status}"))
    return result


def run_fixture(engine, version, kind, root, args):
    t1.LANE = LANE  # owned resources carry oo.lane=t
    work = root / "workloads" / f"{engine}-{version}-{kind}"
    out = root / "t7" / f"{engine}-{version}"
    work.mkdir(parents=True, mode=0o700)
    out.mkdir(parents=True, exist_ok=True)
    if kind == "snapshot":
        base = t1.LaneRun
        options = argparse.Namespace(scale=args.scale, tables=args.tables, platform=None,
                                     health_timeout=None, suites=["T7"])
    else:
        base = t2.LaneRun if engine == "postgres" else t2_p0.SyncRun
        options = argparse.Namespace(scale=1, tables=0, platform=None,
                                     health_timeout=300 if engine == "postgres" else 1200,
                                     suites=["T7"])
    run = audited(base)(work, engine, version, options, args.binary, kind)
    worker = args.worker_image if engine == "oracle" and version == "11" else None
    report = {"engine": engine, "version": version, "kind": kind, "run_id": run.id,
              "scale": options.scale, "tables": options.tables, "lane": LANE}
    workload = {}
    try:
        runner.disk_gate(work, 20, args.disk_wait_minutes * 60)
        if kind == "snapshot":
            run.setup()
            run.seed()
            workload = t1.run_t1(run, args.binary, worker)
        elif engine == "postgres":
            workload = t2.run_postgres(run, args.binary)
        else:
            workload = t2_p0.run_cycles(run, args.binary, worker)
    except Exception as error:
        report["workload_error"] = f"{type(error).__name__}: {runner.redact(str(error), run.secret, run.reader_secret)}"
        partial = work / ("t1-t8.json" if kind == "snapshot" else "t2.json")
        if not workload and partial.exists():
            workload = json.loads(partial.read_text())
    report["workload"] = {k: v for k, v in workload.items()
                          if k in ("T1", "T2", "T8", "C1", "C5", "failure", "handoff")}
    report["windows"] = run.windows
    audit = None
    try:
        if not getattr(run, "created_container", False) or not run.audit_enabled:
            raise RuntimeError("audit never enabled")
        audit = run.audit.collect(run.windows)
        report["audit_error"] = None
    except Exception as error:
        report["audit_error"] = f"{type(error).__name__}: {runner.redact(str(error), run.secret, run.reader_secret)}"
        (work / "audit-traceback.txt").write_text(traceback.format_exc())
    try:
        run.record_version()
    except Exception:
        report["version_error"] = "version_unmeasured"
    failures = run.cleanup()
    report["cleanup_failed"] = failures
    covered = coverage(kind, engine, version, workload, run.windows)
    verdicts = []
    if audit:
        report.update(mode=audit["mode"], users=audit["users"], privileges=audit["privileges"],
                      statements=audit["statements"], summary=audit["summary"])
        records = audit["records"]
        all_rejected = [r for r in records if r["category"] == "rejected"]
        report["rejected"] = [{k: r.get(k) for k in ("time", "variant", "window", "user", "action",
                                                     "object", "rule", "sql")} for r in all_rejected]
        for variant, (ok, reason) in covered.items():
            scoped = audit["summary"].get(variant, {"statements": 0, "rejected": 0})
            reasons = []
            if not ok:
                reasons.append(reason)
            if scoped["statements"] == 0:
                reasons.append("no audited product statements in " + variant + " windows")
            if all_rejected:
                reasons.append(f"{len(all_rejected)} rejected record(s) in fixture audit")
            if not audit["privileges"]["read_only"]:
                reasons.append("reader privileges are not read-only")
            if failures or report.get("version_error"):
                reasons.append("cleanup or version measurement failed")
            verdicts.append(("T7", variant, "fail" if reasons else "pass"))
            report.setdefault("verdicts", {})[variant] = {"result": "fail" if reasons else "pass",
                                                          "coverage": reason, "reasons": reasons}
        raw = out / f"t7-{kind}-records.json"
        runner.write_json(raw, records)
    else:
        for variant, (_, reason) in covered.items():
            verdicts.append(("T7", variant, "fail"))
            report.setdefault("verdicts", {})[variant] = {"result": "fail", "coverage": reason,
                                                          "reasons": ["audit collection failed"]}
    if kind == "incremental" and engine == "oracle" and version != "23":
        verdicts.append(("T7", "c1", "skip"))
        report.setdefault("verdicts", {})["c1"] = {
            "result": "skip", "reasons": ["Oracle XE C1 (LogMiner) is not implemented by the T2 runner"]}
    path = out / f"t7-{kind}.json"
    runner.write_json(path, report)
    write_rows(run, path, verdicts, cleanup_failed=bool(failures))
    runner.checksums(out)
    runner.checksums(work)
    return report


def main():
    def interrupted(signum, frame):
        raise KeyboardInterrupt(f"signal {signum}")
    signal.signal(signal.SIGINT, interrupted)
    signal.signal(signal.SIGTERM, interrupted)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--engine", choices=sorted(VERSIONS), required=True)
    parser.add_argument("--versions", required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--kinds", default="snapshot,incremental")
    parser.add_argument("--binary", type=Path, default=ROOT / "target/debug/oo-load")
    parser.add_argument("--worker-image", default=os.environ.get("OO_TEST_ORACLE_WORKER_IMAGE"))
    parser.add_argument("--scale", type=int, default=50_000)
    parser.add_argument("--tables", type=int, default=20)
    parser.add_argument("--disk-wait-minutes", type=int, default=30)
    args = parser.parse_args()
    versions = args.versions.split(",")
    kinds = args.kinds.split(",")
    if not args.run_id or not all(c.isalnum() or c in "-_" for c in args.run_id):
        parser.error("invalid run ID")
    if len(set(versions)) != len(versions) or any(v not in VERSIONS[args.engine] for v in versions):
        parser.error("versions must be unique members of " + ",".join(VERSIONS[args.engine]))
    if any(k not in ("snapshot", "incremental") for k in kinds):
        parser.error("kinds must be snapshot and/or incremental")
    if not 1 <= args.scale <= 10_000_000 or not 0 <= args.tables <= 5000:
        parser.error("scale must be 1..10000000; tables must be 0..5000")
    if args.engine == "oracle" and "11" in versions and not args.worker_image:
        parser.error("Oracle 11g requires --worker-image (thick worker)")
    args.binary = args.binary.resolve()
    root = ROOT / "evidence" / args.run_id
    root.mkdir(mode=0o700, parents=True, exist_ok=True)
    results = []
    for version in versions:
        for kind in kinds:
            try:
                result = run_fixture(args.engine, version, kind, root, args)
            except KeyboardInterrupt:
                raise
            except Exception as error:
                result = {"engine": args.engine, "version": version, "kind": kind,
                          "error": type(error).__name__}
            summary = {"engine": args.engine, "version": version, "kind": kind,
                       "verdicts": {v: d["result"] for v, d in result.get("verdicts", {}).items()},
                       "statements": result.get("statements"),
                       "rejected": len(result.get("rejected", [])),
                       "workload_error": result.get("workload_error"),
                       "audit_error": result.get("audit_error"), "error": result.get("error")}
            results.append(summary)
            print(json.dumps(summary), flush=True)
            runner.write_json(root / f"results-{args.engine}.json", results)
    t7_root = root / "t7"
    if t7_root.exists():
        runner.checksums(t7_root)
    shutil.rmtree(root / "workloads" / ".tmp", ignore_errors=True)
    return 0 if all(all(v == "pass" for v in r["verdicts"].values() if v != "skip") and r["verdicts"]
                    for r in results) else 1


if __name__ == "__main__":
    sys.exit(main())
