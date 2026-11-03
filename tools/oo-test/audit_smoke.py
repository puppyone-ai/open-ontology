#!/usr/bin/env python3
"""Exercise audit capture and denied-write detection, NOT product T1/T2/T7."""
import argparse
import json
import uuid

from audit import Audit
from runner import ROOT, Run, checksums, redact, write_json


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--targets", default="mysql:8.4,postgres:17,oracle:23ai")
    args = parser.parse_args()
    root = ROOT / "evidence" / ("h-s5-audit-smoke-" + str(uuid.uuid4()))
    root.mkdir(parents=True)
    print(root.relative_to(ROOT), flush=True)
    results = []
    for target in args.targets.split(","):
        engine, version = target.split(":")
        path = root / (engine + "-" + version)
        path.mkdir()
        options = argparse.Namespace(scale=100, tables=1, suites=["T7"],
                                     platform=None, health_timeout=None)
        run = Run(path, engine, version, options)
        result = {"engine": engine, "version": version, "passed": False,
                  "scope": "audit plumbing smoke, not product suite evidence"}
        try:
            run.setup()
            run.seed()
            run.verify_shapes()
            run.gold()
            result["version_actual"] = run.receipt["version_actual"]
            audit = Audit(run)
            audit.enable()
            audit.marker("start")
            run.sql("SELECT COUNT(*) FROM orders;")
            audit.marker("end")
            positive = audit.collect()
            write_json(path / "positive.json", positive)
            if not positive["passed"]:
                if engine == "oracle" and version != "11g":
                    run.sql("SELECT policy_name,enabled_option,entity_name FROM audit_unified_enabled_policies;\n"
                            "SELECT dbusername,action_name,COUNT(*) FROM unified_audit_trail "
                            "GROUP BY dbusername,action_name;", admin=True)
                raise RuntimeError("read capture failed")
            code, _ = run.sql("DELETE FROM orders WHERE 1=0;", required=False)
            audit.marker("negative_end")
            negative = audit.collect()
            write_json(path / "negative.json", negative)
            if code == 0 or negative["passed"] or not negative["violations"]:
                raise RuntimeError("denied-write capture failed")
            result["passed"] = True
        except Exception as error:
            result["error"] = redact(str(error), run.secret, run.reader_secret)
        finally:
            if run.cleanup():
                result["passed"] = False
            write_json(path / "smoke.json", result)
            checksums(path)
        results.append(result)
        write_json(root / "smoke.json", results)
        print(json.dumps(result), flush=True)
    checksums(root)
    return 0 if all(row["passed"] for row in results) else 1


if __name__ == "__main__":
    raise SystemExit(main())
