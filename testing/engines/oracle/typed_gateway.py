#!/usr/bin/env python3
"""Typed numeric SQL on an owned fixture-published Gateway, without model calls."""
import argparse
from decimal import Decimal
import json
import os
from pathlib import Path
import sys
import types
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[3]
os.environ["OO_TEST_LANE"] = "e"
os.environ["CARGO_PROFILE_TEST_DEBUG"] = "0"
os.environ["CARGO_PROFILE_DEV_DEBUG"] = "0"
sys.path.insert(0, str(ROOT / "tools/oo-test"))


def fixture_module():
    path = ROOT / "tools/oo-load/test-gateway-cycles.py"
    source = path.read_text()
    # Reuse publication/mTLS setup, not its old one-row/text assertions.
    replacements = [
        ('"amount" NUMBER(18,0)', '"amount" NUMBER(18,3)'),
        ('INSERT INTO "app"."items" VALUES(10);',
         'INSERT INTO "app"."items" SELECT LEVEL+0.125 FROM DUAL CONNECT BY LEVEL<=120;'),
        ("['data_type'] = 'text'", "['data_type'] = 'numeric(18,3)'"),
        ("        private(self.root / 'client-token', self.token)",
         "        # The fixture token stays in memory; no token file is accessed."),
    ]
    for old, new in replacements:
        if source.count(old) != 1:
            raise RuntimeError("shared Gateway fixture adapter drift")
        source = source.replace(old, new)
    module = types.ModuleType("oracle_typed_gateway_fixture")
    module.__file__ = str(path)
    exec(compile(source, str(path), "exec"), module.__dict__)
    return module


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-id", required=True)
    args = parser.parse_args()
    if not args.run_id or any(not (c.isalnum() or c == "-") for c in args.run_id):
        parser.error("invalid run ID")
    os.umask(0o077)
    out = ROOT / "evidence" / args.run_id
    out.mkdir(exist_ok=False)
    module = fixture_module()
    live = module.Live(out, "oracle")
    report = {"status": "failed", "publisher": "owned fixture fence; not reviewed Self-build",
              "model_calls": 0, "checks": []}
    try:
        live.start()
        for label, sql in [
            ("aggregates", "SELECT SUM(amount),MIN(amount),MAX(amount) FROM app.items"),
            ("ordering", "SELECT amount FROM app.items ORDER BY amount"),
        ]:
            source_sql = (
                "SELECT TO_CHAR(SUM(\"amount\"),'TM9')||'|'||TO_CHAR(MIN(\"amount\"),'TM9')"
                "||'|'||TO_CHAR(MAX(\"amount\"),'TM9') FROM \"app\".\"items\";"
                if label == "aggregates" else
                'SELECT TO_CHAR("amount",\'TM9\') FROM "app"."items" ORDER BY "amount";'
            )
            gold = [[Decimal(v) for v in line.strip().split("|")]
                    for line in live.source_sql(source_sql).splitlines() if line.strip()]
            request = urllib.request.Request(live.origin + "/v1/query",
                data=live.fixture.encode({"sql": sql, "format": "json"}),
                headers={"Authorization": "Bearer " + live.token,
                         "Content-Type": "application/json",
                         "x-oo-expected-publication-id": "current"})
            with urllib.request.urlopen(request, context=live.context, timeout=30) as response:
                body = json.loads(response.read(), parse_float=Decimal)
                rows = [[Decimal(v) for v in row] for row in body["rows"]]
                check = {"query": sql, "status": response.status,
                         "publication": response.headers.get("x-oo-publication-id"),
                         "equal": rows == gold,
                         "gold": [[str(v) for v in row] for row in gold],
                         "rows": [[str(v) for v in row] for row in rows]}
                report["checks"].append(check)
                if not check["equal"] or not check["publication"]:
                    raise RuntimeError("published numeric result differs")
        report["status"] = "passed"
    except urllib.error.HTTPError as error:
        report["error"] = {"status": error.code, "body": json.loads(error.read())}
    except Exception as error:
        report["error"] = type(error).__name__
        raise
    finally:
        live.close()
        module.write_json(out / "result.json", report)
        module.checksums(out)
    raise SystemExit(0 if report["status"] == "passed" else 1)


if __name__ == "__main__":
    main()
