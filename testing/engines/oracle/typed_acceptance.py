#!/usr/bin/env python3
"""Lane E adapters for the unchanged shared oo-load T1/T2 harnesses."""
import json
import os
from decimal import Decimal
from pathlib import Path
import sys
import types

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "tools/oo-test"))
os.environ["OO_TEST_LANE"] = "e"
import t1


def compare_typed(run, target, receipt, table):
    _, raw = run.sql(
        "SELECT column_name||'|'||data_type||'|'||NVL(TO_CHAR(data_precision),'-')"
        "||'|'||NVL(TO_CHAR(data_scale),'-') FROM all_tab_columns "
        f"WHERE owner='FIXTURE' AND table_name='{table}' ORDER BY column_id;"
    )
    columns = [line.strip().split("|") for line in raw.splitlines() if "|" in line]
    actual = target.sql(
        "SELECT attname||'|'||format_type(atttypid,atttypmod) FROM pg_attribute "
        f"WHERE attrelid='{receipt['target']}'::regclass AND attnum>0 AND NOT attisdropped ORDER BY attnum;"
    )
    actual = dict(line.split("|", 1) for line in actual.splitlines() if "|" in line)
    expected = {}
    binary = set()
    for name, kind, p, s in columns:
        ty = "text"
        if kind in ("NUMBER", "INTEGER") and p != "-" and s != "-":
            p, s = int(p), int(s)
            ty = "bigint" if s == 0 and p <= 18 else f"numeric({p},{s})"
        elif kind in ("RAW", "BLOB"):
            ty = "bytea"
            binary.add(name)
        elif kind in ("BINARY_FLOAT", "BINARY_DOUBLE"):
            ty = "double precision"
        elif kind == "DATE" or kind.startswith("TIMESTAMP"):
            ty = "timestamp with time zone" if "TIME ZONE" in kind else "timestamp without time zone"
        expected[name] = ty
    if actual != expected:
        raise RuntimeError(f"typed target schema mismatch: {table}")

    class Target:
        def sql(self, query):
            # Preserve JSON numeric precision; bytea JSON uses PostgreSQL's \x prefix.
            lines = []
            for line in target.sql(query).splitlines():
                if line.startswith("{"):
                    row = json.loads(line, parse_float=Decimal)
                    for name in binary:
                        if row[name] is not None:
                            if not row[name].startswith("\\x"):
                                raise RuntimeError("typed bytea JSON framing")
                            row[name] = row[name][2:]
                    line = json.dumps(row, default=str)
                lines.append(line)
            return "\n".join(lines)

    class Source:
        def sql(self, query):
            # UTC Arrow preserves the instant, not the source's offset spelling.
            for name, kind, _, _ in columns:
                if "TIME ZONE" in kind:
                    query = query.replace(f'TO_CHAR("{name}",',
                                          f'TO_CHAR("{name}" AT TIME ZONE \'UTC\',')
            return run.sql(query)

    result = original_compare(Source(), Target(), receipt, table)
    result["target_types"] = actual
    result["typed_schema_equal"] = True
    result["timezone_comparison"] = "utc_instant; original offset not retained by Arrow"
    if table == "ORDERS":
        _, left = run.sql(
            "SELECT TO_CHAR(SUM(amount),'TM9')||'|'||TO_CHAR(MIN(amount),'TM9')"
            "||'|'||TO_CHAR(MAX(amount),'TM9') FROM FIXTURE.ORDERS;"
        )
        right = target.sql(f'SELECT SUM("AMOUNT"),MIN("AMOUNT"),MAX("AMOUNT") FROM {receipt["target"]};')
        if [Decimal(v) for v in left.strip().split("|")] != [Decimal(v) for v in right.strip().split("|")]:
            raise RuntimeError("typed numeric aggregates differ")
        _, left = run.sql("SELECT TO_CHAR(amount,'TM9') FROM FIXTURE.ORDERS ORDER BY amount,id;")
        right = target.sql(f'SELECT "AMOUNT" FROM {receipt["target"]} ORDER BY "AMOUNT","ID";')
        if [Decimal(v) for v in left.split()] != [Decimal(v) for v in right.split()]:
            raise RuntimeError("typed numeric ordering differs")
        result["numeric_sql"] = "SUM/MIN/MAX/ORDER BY equal; direct Agent DB, not Gateway"
    return result


original_compare = t1.compare_oracle
t1.compare_oracle = compare_typed


def main():
    suite = sys.argv.pop(1)
    if suite == "t1":
        t1.main()
    elif suite == "t2":
        # The shared T2 worker's creation AND ownership check are lane-M literals.
        # Keep its tests unchanged, substituting only our own resource label.
        path = ROOT / "tools/oo-test/t2_p0.py"
        source = path.read_text()
        for old, new in [
            ('"oo.lane=m"', '"oo.lane=e"'),
            ('labels.get("oo.lane") != "m"', 'labels.get("oo.lane") != "e"'),
        ]:
            if source.count(old) != 1:
                raise RuntimeError("shared T2 lane adapter drift")
            source = source.replace(old, new)
        module = types.ModuleType("oracle_lane_e_t2")
        module.__file__ = str(path)
        exec(compile(source, str(path), "exec"), module.__dict__)
        module.main()
    else:
        raise SystemExit("expected t1 or t2")


if __name__ == "__main__":
    main()
