#!/usr/bin/env python3
"""Export the actual acknowledged Oracle cut, compare every cell, and seal its bundle."""
import argparse
from decimal import Decimal
import hashlib
import json
from pathlib import Path

import c5_live
import configured_native
import sealed_bundle
import whole_run as w


def save(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")
    path.chmod(0o600)


def export(out):
    out = Path(out).resolve()
    out.mkdir(mode=0o700, parents=True, exist_ok=False)
    actual = json.loads(w.psql("previewdb", """
SELECT json_build_object('binding',b.id,'pin',m.pin,'task',t.id,'task_state',t.state,
 'lease_generation',t.generation,'source_generation',m.source_generation,
 'request',o.request,'load',m.result)
FROM jw_native.bindings b JOIN jw_native.watermarks m ON m.binding_id=b.id
JOIN jw_native.occurrences o ON o.task_id=m.task_id JOIN jobs.tasks t ON t.id=m.task_id
WHERE b.id='e7325034-f8f4-4b72-91bd-4f3f9a48128a'::uuid;
""", tuples=True))
    request, loaded = actual["request"], actual["load"]
    if actual["task_state"] != "succeeded" or loaded["source_generation"] != actual["source_generation"]:
        raise ValueError("current cut is not acknowledged")
    save(out / "acknowledged-cut.json", actual)
    target = {k: v for k, v in request["target"].items() if k not in ("owner", "shard")}
    query = {k: request[k] for k in ("source_id", "source_generation", "config_digest", "schema_digest", "type_digest", "input_digest", "snapshot")}
    query.update(protocol="oo.pipeline.native-sealed-catalog/v1", run_id=loaded["run_id"],
                 owner=request["target"]["owner"], target_generation=loaded["target_generation"], target=target)
    registry = Path(request["registry_path"])
    # The current producer's private directory is the only mutable mount.
    root = registry.parents[6]
    raw = configured_native.saved_run(out, "catalog", configured_native.docker(root, registry,
        ["/opt/loader", "catalog-import-native"]), json.dumps(query).encode())
    (out / "catalog.json").write_bytes(raw)
    catalog = json.loads(raw)
    if catalog["run_id"] != loaded["run_id"] or catalog["source_generation"] != actual["source_generation"]:
        raise ValueError("catalog identity")
    table = catalog["tables"][0]
    mapped = {sealed_bundle.source_name(c["logical_name"]): c["stored_physical"] for c in table["columns"]}
    if set(mapped) != set(sealed_bundle.RULES):
        raise ValueError("approved columns differ")
    sql_dir = out / "source-sql"
    sql_dir.mkdir(mode=0o700)
    password = Path("/Users/redux/.oo-dl01w1-04-p2/reader-window").read_text().strip()
    sql = c5_live.HEAD + """
SELECT 'ROW|' || JSON_OBJECT('key' VALUE TO_CHAR(KEY_ID,'TM9'),
 'amount' VALUE TO_CHAR(AMOUNT,'FM9999999990.00','NLS_NUMERIC_CHARACTERS=''.,'''),
 'occurred' VALUE TO_CHAR(OCCURRED,'YYYY-MM-DD"T"HH24:MI:SS'),
 'label' VALUE LABEL,'payload' VALUE LOWER(RAWTOHEX(PAYLOAD)) NULL ON NULL)
FROM OO04_FIX.ONBOARD_ORDERS ORDER BY KEY_ID;
EXIT
"""
    code, text = c5_live.sqlplus("OO04_READER", password, sql, "independent-gold", sql_dir, [password.encode()])
    if code or "ORA-" in text or "SP2-" in text:
        raise ValueError("independent source read")
    source = [json.loads(line[4:]) for line in c5_live.lines(text, "") if line.startswith("ROW|")]
    target = json.loads(w.psql("previewdb", f"""
SELECT json_agg(json_build_object('key',{mapped['KEY_ID']}::text,
 'amount',to_char({mapped['AMOUNT']},'FM9999999990.00'),
 'occurred',to_char({mapped['OCCURRED']},'YYYY-MM-DD"T"HH24:MI:SS'),
 'label',{mapped['LABEL']},'payload',encode({mapped['PAYLOAD']},'hex'))
ORDER BY {mapped['KEY_ID']}) FROM _oo.{table['stored_physical']};
""", tuples=True))
    def norm(rows):
        return [{**r, "key": str(r["key"]), "amount": None if r["amount"] is None else str(Decimal(r["amount"]))} for r in rows]
    source, target = norm(source), norm(target)
    save(out / "source-gold.json", source)
    save(out / "target-gold.json", target)
    result = {"source_id": request["source_id"], "source_generation": actual["source_generation"],
              "rows": len(source), "cells": len(source) * len(mapped), "differences": int(source != target),
              "keys": [r["key"] for r in source], "key_2_absent": all(r["key"] != "2" for r in source),
              "all_approved_columns": sorted(mapped),
              "reader": "OO04_READER", "reader_exit": code,
              "source_gold_sha256": hashlib.sha256((out / "source-gold.json").read_bytes()).hexdigest()}
    save(out / "typed-gold.json", result)
    if (result["differences"] or not result["key_2_absent"] or len(result["keys"]) > 8
            or result["keys"] != sorted(result["keys"], key=int)
            or len(result["keys"]) != len(set(result["keys"]))):
        raise ValueError("source-target oracle")
    occurrence = Path(request["arrow_path"]).parents[3]
    shadow = occurrence / "capture/run/shadow/ONBOARD_ORDERS" / ("gen-" + str(actual["source_generation"]) + ".json")
    return sealed_bundle.build(out / "catalog.json", out / "bundle", shadow)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("out", type=Path)
    args = parser.parse_args()
    print(json.dumps(export(args.out)))
