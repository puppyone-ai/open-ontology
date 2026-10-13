#!/usr/bin/env python3
"""Check native catalog facts. This does not accept MariaDB as a source."""

import json
import pathlib
import sys


def main() -> int:
    catalog = json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
    ready = json.loads(pathlib.Path(sys.argv[2]).read_text(encoding="utf-8"))
    failed = 0

    def need(condition: bool, fact: str) -> None:
        nonlocal failed
        if not condition:
            failed += 1
            print(fact)

    need(catalog["protocol"] == "oo.mariadb-driver-preflight.catalog/v1", "catalog protocol")
    need(catalog["engine"] == "MariaDB", "engine")
    need(str(catalog["service_version"]).startswith("11.4.13-MariaDB"), catalog["service_version"])
    need("mariadb" in str(catalog["version_comment"]).lower(), "comment")
    need(catalog["driver"] == "mysql_async 0.36.1", "driver")
    need(catalog["deployment"] == "docker-official-image", "deployment")
    need(catalog["time_zone"] == "+00:00", "time_zone")
    need(catalog["log_bin"] in (0, "0"), f"log_bin {catalog['log_bin']}")
    need("NO_ZERO_DATE" in catalog["sql_mode"], "sql_mode")
    need(catalog["tls"] == "absent_fixture" and catalog["tls_cipher"] == "", "tls")
    need(catalog["business_connections"] == 1, f"connections {catalog['business_connections']}")
    need(catalog["handshake_connection_id"] == catalog["session_connection_id"], "connection id")
    need(catalog["source_acceptance"] is False, "source claim")
    need(ready["state"] == "validated" and ready["source_acceptance"] is False, "ready state")
    grants = "\n".join(catalog["grants_redacted"]).upper()
    need("SELECT" in grants and "OO_SYNTH" in grants, "select grant")
    for denied in ("PROCESS", "CONNECTION ADMIN", "INSERT", "ALL PRIVILEGES"):
        need(denied not in grants, f"grant contains {denied}")
    tables = {item["name"]: item for item in catalog["tables"]}
    refused = {name: item["error"] for name, item in tables.items() if item["plan"] == "refused"}
    need(refused.get("k_float_key") == "unsafe_float_key", f"float key {refused}")
    need(refused.get("k_float") == "unsafe_float", f"float {refused}")
    need(refused.get("k_bit") == "unsupported_bit", f"bit {refused}")
    json_columns = [column for column in tables["k_json"]["columns"] if column["name"] == "doc"]
    need(bool(json_columns), "json doc")
    if json_columns:
        column = json_columns[0]
        need(column["data_type"] == "longtext", f"json data_type {column['data_type']}")
        need(column.get("provenance") == "longtext_json_valid", f"provenance {column.get('provenance')}")
        need("json_valid" in (column.get("check_clause") or "").lower(), "check clause")
    unsigned = [column for column in tables["k_unsigned"]["columns"] if column["name"] == "id"][0]
    need(unsigned["arrow"] == "uint64", f"unsigned arrow {unsigned['arrow']}")
    need("unsigned" in unsigned["column_type"], unsigned["column_type"])
    decimal = {column["name"]: column for column in tables["k_decimal"]["columns"]}
    need(decimal["id"]["arrow"] == "decimal128(38,10)", decimal["id"]["arrow"])
    need(decimal["payload"]["arrow"] == "decimal256(65,30)", decimal["payload"]["arrow"])
    temporal = {column["name"]: column for column in tables["k_temporal"]["columns"]}
    need(temporal["t"]["arrow"] == "int64_interval_us", temporal["t"]["arrow"])
    need("time_is_interval_not_clock" in (temporal["t"].get("caveat") or ""), "time caveat")
    binary = {column["name"]: column for column in tables["k_binary"]["columns"]}
    need("binary_order_is_raw_bytes" in (binary["id"].get("caveat") or ""), "binary caveat")
    uca = {column["name"]: column for column in tables["k_collation"]["columns"]}
    need("utf8mb4_uca1400_ai_ci" in (uca["uca_text"].get("caveat") or ""), "uca caveat")
    print(f"catalog_failures={failed}")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
