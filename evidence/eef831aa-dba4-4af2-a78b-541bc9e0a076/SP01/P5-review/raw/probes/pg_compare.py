#!/usr/bin/env python3
"""Compare parser decisions with a private PG 17.10 fixture. Does not send writes."""
import json
import subprocess
from pathlib import Path

NAME = "oo-a076-pg1710-81b3f22f"
DECISIONS = Path("/tmp/oo-a076-81b3f22f-b143-4cf8-b2c8-19110890d30a/out/decisions.jsonl")
OUT = Path("/tmp/oo-a076-81b3f22f-b143-4cf8-b2c8-19110890d30a/out/pg-compare.jsonl")

# Explicit visible-column SQL that should match a correct rewrite.
ORACLE = {
    "legal-filter": "SELECT o.id, o.region, o.amount FROM analytics.orders o WHERE o.region = 'eu' ORDER BY o.id",
    "star-orders": "SELECT o.id, o.region, o.amount FROM analytics.orders o WHERE o.region = 'eu' ORDER BY o.id",
    "qualified-star": "SELECT o.id, o.region, o.amount FROM analytics.orders o WHERE o.amount > 0 ORDER BY o.id",
    "star-then-extra": "SELECT o.id, o.region, o.amount, o.region FROM analytics.orders o WHERE o.region = 'eu' ORDER BY o.id",
    "extra-then-star": "SELECT o.region, o.id, o.region, o.amount FROM analytics.orders o WHERE o.region = 'eu' ORDER BY o.id",
    "count-star": "SELECT count(*) FROM analytics.orders o WHERE o.region = 'eu'",
    "count-star-group": "SELECT o.region, count(*) FROM analytics.orders o GROUP BY o.region ORDER BY o.region",
    "count-filter": "SELECT count(*) FILTER (WHERE o.region = 'eu') FROM analytics.orders o",
    "left-join-count": "SELECT count(*), count(c.name) FROM analytics.orders o LEFT JOIN analytics.customer c ON o.id = c.id",
    "self-join-star": "SELECT a.id, a.region, a.amount, b.id, b.region, b.amount FROM analytics.orders a JOIN analytics.orders b ON a.id = b.id WHERE a.region = 'eu' ORDER BY a.id",
    "join-using-star": "SELECT orders.id, orders.region, orders.amount, customer.name FROM analytics.orders JOIN analytics.customer USING (id) WHERE orders.region = 'eu' ORDER BY orders.id",
    "join-on-star": "SELECT o.id, o.region, o.amount, c.id, c.name FROM analytics.orders o JOIN analytics.customer c ON o.id = c.id WHERE o.region = 'eu' ORDER BY o.id",
    "left-join-star": "SELECT o.id, o.region, o.amount, c.id, c.name FROM analytics.orders o LEFT JOIN analytics.customer c ON o.id = c.id ORDER BY o.id",
    "cte-star": "WITH s AS (SELECT o.id, o.region, o.amount FROM analytics.orders o) SELECT s.id, s.region, s.amount FROM s ORDER BY s.id",
    "cte-shadow": "WITH pg_catalog AS (SELECT 1 AS id) SELECT pg_catalog.id FROM pg_catalog",
    "cte-values-not-table": "WITH orders AS (SELECT 9 AS id, 'zz' AS region, 0 AS amount) SELECT orders.id, orders.region, orders.amount FROM orders",
    "subquery-star": "SELECT s.id, s.region FROM (SELECT o.id, o.region FROM analytics.orders o WHERE o.region = 'eu') s ORDER BY s.id",
    "scalar-subquery": "SELECT o.id, (SELECT c.name FROM analytics.customer c WHERE c.id = o.id) AS name FROM analytics.orders o ORDER BY o.id",
    "union-all": "SELECT o.id, o.region FROM analytics.orders o WHERE o.region = 'eu' UNION ALL SELECT o.id, o.region FROM analytics.orders o WHERE o.region = 'us' ORDER BY id",
    "union-distinct": "SELECT o.id FROM analytics.orders o UNION SELECT c.id FROM analytics.customer c ORDER BY id",
    "except": "SELECT o.id FROM analytics.orders o EXCEPT SELECT c.id FROM analytics.customer c ORDER BY id",
    "star-union": "SELECT o.id, o.region, o.amount FROM analytics.orders o WHERE o.region = 'eu' UNION ALL SELECT o.id, o.region, o.amount FROM analytics.orders o WHERE o.region = 'us' ORDER BY id",
    "quoted-alias": "SELECT \"O\".id, \"O\".region FROM analytics.orders \"O\" WHERE \"O\".region = 'eu' ORDER BY \"O\".id",
    "quoted-star": "SELECT \"O\".id, \"O\".region, \"O\".amount FROM analytics.orders \"O\" WHERE \"O\".region = 'eu' ORDER BY \"O\".id",
    "json-table": "SELECT jt.id FROM json_table('[{\"id\": 1}]', '$[*]' COLUMNS (id int PATH '$.id')) AS jt",
    "json-table-star": "SELECT jt.id FROM json_table('[{\"id\": 1}]', '$[*]' COLUMNS (id int PATH '$.id')) AS jt",
    "window-count": "SELECT o.id, count(*) OVER () FROM analytics.orders o ORDER BY o.id",
    "unicode-eu": "SELECT o.id FROM analytics.orders o WHERE o.region = U&'\\0065\\0075' ORDER BY o.id",
    "where-true-large": "SELECT b.id, b.ts FROM analytics.big_fact b WHERE true",
    "where-one-eq-one-large": "SELECT b.id, b.ts FROM analytics.big_fact b WHERE 1 = 1",
    "r9-wrap": "SELECT s.id, s.ts FROM (SELECT b.id, b.ts FROM analytics.big_fact b) s",
    "select-const": "SELECT 1",
    "order-by-alias": "SELECT o.region AS id FROM analytics.orders o ORDER BY id",
    "current-setting": "SELECT current_setting('server_version')",
}

# Original SQL is not sent when it can write, lock, or call a session-changing function.
DO_NOT_SEND = {
    "set", "set-local-claims", "select-into", "for-update", "for-key-share",
    "writable-cte", "nested-write", "multi", "multi-delete", "set-config", "pg-read-file",
}


def psql(role: str, sql: str):
    proc = subprocess.run(
        ["docker", "exec", "-e", "PGOPTIONS=-c statement_timeout=5000", "-i", NAME,
         "psql", "-U", role, "-d", "postgres", "-v", "ON_ERROR_STOP=1", "-At", "-F", "|"],
        input=sql + "\n",
        text=True,
        capture_output=True,
    )
    text = ((proc.stdout or "") + (proc.stderr or "")).strip()
    return proc.returncode, text


def main():
    before_rc, before = psql("postgres", "SELECT 'orders', count(*) FROM analytics.orders UNION ALL SELECT 'customer', count(*) FROM analytics.customer UNION ALL SELECT 'big_fact', count(*) FROM analytics.big_fact UNION ALL SELECT 'empty_vis', count(*) FROM analytics.empty_vis ORDER BY 1")
    rows = []
    for line in DECISIONS.read_text().splitlines():
        if not line.strip():
            continue
        item = json.loads(line)
        case_id = item["id"]
        native = item["native"]
        record = {
            "id": case_id,
            "code": native["code"],
            "syntax_allow": native["syntax_allow"],
            "oid_acl_rls_proven": native["oid_acl_rls_proven"],
            "star": native["original_projection_star"],
            "executed_sql": native["executed_sql"],
            "same_code": item["same_code"],
            "same_sql": item["same_sql"],
            "remote_reaped": item["remote_reaped"],
            "remote_workdir_removed": item["remote_workdir_removed"],
        }
        if case_id in DO_NOT_SEND:
            record["sent"] = False
        elif case_id == "catalog":
            rc, out = psql("agent_reader", "SELECT count(*) FROM pg_catalog.pg_class")
            record["sent"] = "count_only"
            record["server_rc"] = rc
            record["server_out"] = out
        elif case_id == "information-schema":
            rc, out = psql("agent_reader", "SELECT count(*) FROM information_schema.tables")
            record["sent"] = "count_only"
            record["server_rc"] = rc
            record["server_out"] = out
        else:
            # original query, then rewritten query, then the visible oracle
            src = None
            # recover original from executed when the rewrite is identity; otherwise the
            # decision file does not store the source SQL. The oracle and executed are compared.
            record["sent"] = True
        if native["syntax_allow"] and native["executed_sql"] and case_id not in DO_NOT_SEND:
            rc, out = psql("agent_reader", native["executed_sql"])
            record["executed_rc"] = rc
            record["executed_out"] = out
            oracle = ORACLE.get(case_id)
            if oracle:
                orc, oout = psql("agent_reader", oracle)
                record["oracle_rc"] = orc
                record["oracle_out"] = oout
                record["matches_oracle"] = rc == 0 and orc == 0 and out == oout
        rows.append(record)
        print(f"{case_id} code={native['code']} match={record.get('matches_oracle')}")
    after_rc, after = psql("postgres", "SELECT 'orders', count(*) FROM analytics.orders UNION ALL SELECT 'customer', count(*) FROM analytics.customer UNION ALL SELECT 'big_fact', count(*) FROM analytics.big_fact UNION ALL SELECT 'empty_vis', count(*) FROM analytics.empty_vis ORDER BY 1")
    summary = {"before_rc": before_rc, "before": before, "after_rc": after_rc, "after": after, "rows_unchanged": before == after}
    OUT.write_text("".join(json.dumps(row, ensure_ascii=False) + "\n" for row in rows))
    Path("/tmp/oo-a076-81b3f22f-b143-4cf8-b2c8-19110890d30a/out/pg-compare-summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps(summary, indent=2))

if __name__ == "__main__":
    main()
