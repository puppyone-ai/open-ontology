#!/usr/bin/env python3
"""Typed oracle for the MongoDB preparation corpus.

Compares a live canonical extended-JSON export to handwritten decisions.
Relaxed JSON is a negative sample, not an accepted preservation format.
This module does not open a database connection and does not import a MongoDB driver.
"""

from __future__ import annotations

import argparse
import base64
import json
import math
import pathlib
import sys
import uuid

PRESENCE_NUM = 9
PRESENCE_DEN = 10
CONSISTENCY_NUM = 99
CONSISTENCY_DEN = 100
PROJECTABLE = {
    "objectId",
    "string",
    "int32",
    "int64",
    "decimal128",
    "double",
    "date",
    "bool",
}
WRITE_VERBS = {
    "insert",
    "update",
    "delete",
    "findAndModify",
    "drop",
    "create",
    "createIndexes",
    "dropIndexes",
    "collMod",
    "renameCollection",
    "dropDatabase",
    "applyOps",
    "compact",
    "convertToCapped",
    "createUser",
    "createRole",
    "dropUser",
    "dropRole",
    "updateUser",
    "grantRolesToUser",
}
CATALOG_VERBS = {
    "connectionStatus",
    "hello",
    "ismaster",
    "listCollections",
    "listIndexes",
    "collStats",
    "dbStats",
}
BATCH_VERBS = {"find", "getMore"}
CANCEL_VERBS = {"find", "killCursors", "getMore"}
NOISE_VERBS = {"saslStart", "saslContinue", "ping", "hello", "ismaster", "endSessions"}
READER_PREFIX = "oo-dl01w1-14:reader:"
DBA_PREFIX = "oo-dl01w1-14:dba:"


def presence_ok(present: int, n: int) -> bool:
    return present * PRESENCE_DEN >= n * PRESENCE_NUM


def consistency_ok(dominant: int, present: int) -> bool:
    return present > 0 and dominant * CONSISTENCY_DEN >= present * CONSISTENCY_NUM


def bson_type(value):
    if value is None:
        return "null"
    if isinstance(value, bool):
        return "bool"
    if isinstance(value, str):
        return "string"
    if isinstance(value, list):
        return "array"
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        return "json_number"
    if isinstance(value, dict):
        keys = set(value)
        if keys == {"$oid"}:
            return "objectId"
        if keys == {"$numberLong"}:
            return "int64"
        if keys == {"$numberInt"}:
            return "int32"
        if keys == {"$numberDecimal"}:
            return "decimal128"
        if keys == {"$numberDouble"}:
            return "double"
        if keys == {"$date"}:
            return "date"
        if keys == {"$binary"}:
            subtype = value["$binary"].get("subType", "")
            return "binary:" + subtype
        return "object"
    return "unknown"


def projectable(type_name: str) -> bool:
    if type_name in PROJECTABLE:
        return True
    return type_name.startswith("binary:")


def classify(docs: list) -> dict:
    n = len(docs)
    keys = []
    seen = set()
    for doc in docs:
        for key in doc:
            if key not in seen:
                seen.add(key)
                keys.append(key)
    fields = {}
    for key in keys:
        present_values = [doc[key] for doc in docs if key in doc]
        counts = {}
        for value in present_values:
            name = bson_type(value)
            counts[name] = counts.get(name, 0) + 1
        if counts:
            ranking = sorted(counts.items(), key=lambda item: (-item[1], item[0]))
            dominant, dominant_count = ranking[0]
        else:
            dominant, dominant_count = "absent", 0
        present = len(present_values)
        stable = (
            presence_ok(present, n)
            and consistency_ok(dominant_count, present)
            and projectable(dominant)
        )
        if stable:
            bucket = "stable"
        elif not presence_ok(present, n):
            bucket = "unclassified"
        elif projectable(dominant):
            bucket = "type_drift"
        else:
            bucket = "retained_in_doc"
        fields[key] = {
            "bucket": bucket,
            "dominant": dominant,
            "present": present,
            "dominant_count": dominant_count,
            "types": counts,
        }
    return {"n": n, "fields": fields}


def number_text(value, type_name: str) -> str:
    if bson_type(value) != type_name:
        raise AssertionError(type_name)
    key = {
        "int64": "$numberLong",
        "int32": "$numberInt",
        "decimal128": "$numberDecimal",
    }[type_name]
    return value[key]


def date_ms(value) -> str:
    if bson_type(value) != "date":
        raise AssertionError("date")
    inner = value["$date"]
    if not isinstance(inner, dict) or "$numberLong" not in inner:
        raise AssertionError("date form")
    return inner["$numberLong"]


def oid_hex(value) -> str:
    if bson_type(value) != "objectId":
        raise AssertionError("oid")
    return value["$oid"]


def binary_b64(value, subtype: str) -> str:
    expect = "binary:" + subtype
    if bson_type(value) != expect:
        raise AssertionError(expect)
    return value["$binary"]["base64"]


def uuid_text(value) -> str:
    raw = base64.b64decode(binary_b64(value, "04"))
    return str(uuid.UUID(bytes=raw))


def expected_id(i: int) -> str:
    return f"64b0b1c0e1d2f3a4{i:08x}"


def expected_price(i: int) -> str:
    if i == 0:
        return "NaN"
    if i == 1:
        return "Infinity"
    if i == 2:
        return "-Infinity"
    return f"{10**20 + i}.{i:04d}0001"


def row_index(doc) -> int:
    hex_id = oid_hex(doc["_id"])
    if not hex_id.startswith("64b0b1c0e1d2f3a4"):
        raise AssertionError("id prefix")
    return int(hex_id[-8:], 16)


def check_doc(doc, errors: list) -> None:
    i = row_index(doc)
    want_keys_absent = []
    if i >= 89:
        want_keys_absent.append("rare_code")
    if i < 95:
        want_keys_absent.append("new_attr")
    for key in want_keys_absent:
        if key in doc:
            errors.append(f"i={i} unexpected {key}")
    if "explicit_null" not in doc or doc["explicit_null"] is not None:
        errors.append(f"i={i} explicit_null")
    if doc.get("sku") != f"SKU-{i:03d}":
        errors.append(f"i={i} sku")
    if doc.get("warehouse") != "wh-east":
        errors.append(f"i={i} warehouse")
    try:
        if number_text(doc["units"], "int64") != str(9007199254740993 + i):
            errors.append(f"i={i} units")
        if number_text(doc["small_units"], "int64") != str(i):
            errors.append(f"i={i} small_units")
        if number_text(doc["flag"], "int32") != str(i % 5):
            errors.append(f"i={i} flag")
        if number_text(doc["price"], "decimal128") != expected_price(i):
            errors.append(f"i={i} price {number_text(doc['price'], 'decimal128')}")
        if date_ms(doc["created"]) != str(1700000000000 + i * 1000):
            errors.append(f"i={i} created")
        if uuid_text(doc["uid"]) != f"00000000-0000-4000-8000-{i:012d}":
            errors.append(f"i={i} uid")
        blob = base64.b64decode(binary_b64(doc["blob"], "00"))
        if blob != bytes([i & 255, 0x0A, 0x00, i & 255]):
            errors.append(f"i={i} blob")
        if bson_type(doc["weight"]) != "double":
            errors.append(f"i={i} weight type")
        else:
            got = float(doc["weight"]["$numberDouble"])
            if got != i + 0.5:
                errors.append(f"i={i} weight")
    except (AssertionError, KeyError, ValueError) as exc:
        errors.append(f"i={i} typed {exc}")
    if i == 99:
        if doc.get("note_null_boundary") is not None:
            errors.append("i=99 note_null_boundary")
    elif doc.get("note_null_boundary") != f"n{i}":
        errors.append(f"i={i} note_null_boundary")
    if i >= 98:
        if doc.get("note_drift") is not None:
            errors.append(f"i={i} note_drift")
    elif doc.get("note_drift") != f"d{i}":
        errors.append(f"i={i} note_drift")
    if i < 50 and doc.get("mixed") != f"m{i}":
        errors.append(f"i={i} mixed string")
    if i >= 50:
        try:
            if number_text(doc["mixed"], "int64") != str(i):
                errors.append(f"i={i} mixed long")
        except (AssertionError, KeyError):
            errors.append(f"i={i} mixed long type")
    if i < 97:
        try:
            if number_text(doc["count_drift"], "int64") != str(i):
                errors.append(f"i={i} count_drift long")
        except (AssertionError, KeyError):
            errors.append(f"i={i} count_drift long type")
    else:
        try:
            if number_text(doc["count_drift"], "int32") != str(i):
                errors.append(f"i={i} count_drift int")
        except (AssertionError, KeyError):
            errors.append(f"i={i} count_drift int type")
    if i >= 95 and doc.get("new_attr") != f"new-{i}":
        errors.append(f"i={i} new_attr")
    if i < 89 and doc.get("rare_code") != f"c{i}":
        errors.append(f"i={i} rare_code")
    if not isinstance(doc.get("tags"), list) or not isinstance(doc.get("nested_arr"), list):
        errors.append(f"i={i} arrays")
    if not isinstance(doc.get("payload"), dict) or "$" in "".join(doc.get("payload", {})):
        if bson_type(doc.get("payload")) != "object":
            errors.append(f"i={i} payload")
    if math.isnan(float(i)):
        errors.append("unreachable")


def check_anchor(doc, anchor: dict, errors: list, i: int) -> None:
    mapping = {
        "sku": lambda: doc.get("sku"),
        "units": lambda: number_text(doc["units"], "int64"),
        "small_units": lambda: number_text(doc["small_units"], "int64"),
        "flag": lambda: number_text(doc["flag"], "int32"),
        "created_ms": lambda: date_ms(doc["created"]),
        "uid": lambda: uuid_text(doc["uid"]),
        "blob_base64": lambda: binary_b64(doc["blob"], "00"),
        "note_null_boundary": lambda: doc.get("note_null_boundary"),
        "note_drift": lambda: doc.get("note_drift") if "note_drift" in doc else "MISSING",
        "rare_code": lambda: doc.get("rare_code") if "rare_code" in doc else None,
        "new_attr": lambda: doc.get("new_attr") if "new_attr" in doc else None,
    }
    for key, fn in mapping.items():
        if key not in anchor:
            continue
        try:
            got = fn()
        except (AssertionError, KeyError, ValueError) as exc:
            errors.append(f"anchor {i} {key} {exc}")
            continue
        if got != anchor[key]:
            errors.append(f"anchor {i} {key}")
    if "price" in anchor:
        price = anchor["price"]
        try:
            text = number_text(doc["price"], "decimal128")
        except (AssertionError, KeyError) as exc:
            errors.append(f"anchor {i} price {exc}")
        else:
            if "reject" in price and text != price["reject"]:
                errors.append(f"anchor {i} price reject")
            if "decimal" in price and text != price["decimal"]:
                errors.append(f"anchor {i} price decimal")
    if "weight" in anchor:
        try:
            got = float(doc["weight"]["$numberDouble"])
        except (KeyError, TypeError, ValueError):
            errors.append(f"anchor {i} weight")
        else:
            if got != anchor["weight"]:
                errors.append(f"anchor {i} weight")
    if "mixed" in anchor:
        spec = anchor["mixed"]
        if "string" in spec and doc.get("mixed") != spec["string"]:
            errors.append(f"anchor {i} mixed")
        if "int64" in spec:
            try:
                if number_text(doc["mixed"], "int64") != spec["int64"]:
                    errors.append(f"anchor {i} mixed")
            except (AssertionError, KeyError):
                errors.append(f"anchor {i} mixed")
    if "count_drift" in anchor:
        spec = anchor["count_drift"]
        kind = "int64" if "int64" in spec else "int32"
        try:
            if number_text(doc["count_drift"], kind) != spec[kind]:
                errors.append(f"anchor {i} count_drift")
        except (AssertionError, KeyError):
            errors.append(f"anchor {i} count_drift")
    if "tags" in anchor and doc.get("tags") != anchor["tags"]:
        errors.append(f"anchor {i} tags")


def command_comment(entry: dict) -> str:
    command = entry.get("command")
    if isinstance(command, dict) and isinstance(command.get("comment"), str):
        return command["comment"]
    if isinstance(entry.get("comment"), str):
        return entry["comment"]
    return ""


def command_verbs(entry: dict) -> set:
    command = entry.get("command")
    if not isinstance(command, dict):
        return set()
    return set(command)


def app_name(entry: dict) -> str:
    for key in ("appName", "appname"):
        if isinstance(entry.get(key), str):
            return entry[key]
    return ""


def check_profile(rows: list, errors: list) -> dict:
    reader_rows = []
    dba_inserts = 0
    change_streams = 0
    for entry in rows:
        comment = command_comment(entry)
        app = app_name(entry)
        verbs = command_verbs(entry)
        op = entry.get("op")
        readerish = comment.startswith(READER_PREFIX) or "oo-dl01w1-14-reader" in app
        if comment.startswith(DBA_PREFIX) and "insert" in verbs:
            dba_inserts += 1
        if not readerish:
            continue
        reader_rows.append(entry)
        if op in {"insert", "update", "remove", "delete"}:
            errors.append(f"reader write op {op}")
        bad = verbs & WRITE_VERBS
        if bad:
            errors.append("reader write verbs " + ",".join(sorted(bad)))
        if "$changeStream" in json.dumps(entry.get("command", {})):
            change_streams += 1
        if comment.startswith(READER_PREFIX + "catalog:"):
            if not (verbs & CATALOG_VERBS):
                errors.append("catalog verb missing")
            if verbs & {"find", "getMore", "aggregate", "killCursors"}:
                errors.append("catalog used a batch or stream verb")
        elif comment.startswith(READER_PREFIX + "batch:"):
            if not (verbs & BATCH_VERBS):
                errors.append("batch verb missing")
            if "aggregate" in verbs:
                errors.append("batch aggregate")
        elif comment.startswith(READER_PREFIX + "cancel:"):
            if not (verbs & CANCEL_VERBS):
                errors.append("cancel verb missing")
        elif comment.startswith(READER_PREFIX + "negative-cdc:"):
            if "aggregate" not in verbs:
                errors.append("cdc negative was not aggregate")
        elif comment == "":
            allowed = verbs & (NOISE_VERBS | CATALOG_VERBS | BATCH_VERBS | CANCEL_VERBS | {"aggregate"})
            if not allowed:
                errors.append("uncommented reader command")
    if dba_inserts < 1:
        errors.append("dba insert missing from profiler")
    catalog_cs = [
        row
        for row in reader_rows
        if command_comment(row).startswith(READER_PREFIX + "catalog:")
        or command_comment(row).startswith(READER_PREFIX + "batch:")
    ]
    for row in catalog_cs:
        if "$changeStream" in json.dumps(row.get("command", {})):
            errors.append("implicit change stream during catalog or batch")
    # 8.0.32 rejects $changeStream before the profiler records the aggregate.
    # Zero profiled watches is the observed refusal. More than one would mean a second stream.
    if change_streams > 1:
        errors.append(f"change stream commands {change_streams}")
    return {
        "reader_profile_rows": len(reader_rows),
        "dba_insert_commands": dba_inserts,
        "reader_change_stream_commands": change_streams,
    }


def business_collections(catalog: dict) -> list:
    body = catalog["listCollections"]
    cursor = body.get("cursor", {})
    batch = cursor.get("firstBatch") or cursor.get("nextBatch") or []
    names = []
    for item in batch:
        name = item.get("name")
        if isinstance(name, str) and not name.startswith("system."):
            names.append(name)
    return names


def count_value(value) -> int | None:
    if isinstance(value, int) and not isinstance(value, bool):
        return value
    if isinstance(value, dict) and "$numberLong" in value:
        return int(value["$numberLong"])
    if isinstance(value, dict) and "$numberInt" in value:
        return int(value["$numberInt"])
    return None


def is_ok(value) -> bool:
    return value is True or count_value(value) == 1


def check_relaxed(docs: list, errors: list) -> None:
    by_index = {row_index(doc): doc for doc in docs}
    big = by_index[0]["units"]
    small = by_index[7]["small_units"]
    if big != 9007199254740992:
        errors.append("relaxed int64 above 2^53 did not show the known loss")
    if small != 7:
        errors.append("relaxed small int64 did not drop the type tag")
    if isinstance(big, dict):
        errors.append("relaxed big units kept a type wrapper")


def map_delete(event: dict) -> dict:
    if "fullDocument" in event:
        raise AssertionError("delete fixture has a full document")
    if event.get("operationType") != "delete":
        raise AssertionError("not a delete")
    return {
        "op": "delete",
        "pk_hex": event["documentKey"]["_id"]["$oid"],
        "full_document": None,
    }


def roles_of(catalog: dict) -> list:
    status = catalog["connectionStatus"]
    auth = status.get("authInfo", {})
    roles = auth.get("authenticatedUserRoles") or []
    return [(item.get("role"), item.get("db")) for item in roles]


def evaluate(paths: dict) -> dict:
    expected = json.loads(paths["expected"].read_text())
    docs = json.loads(paths["docs"].read_text())
    relaxed = json.loads(paths["relaxed"].read_text())
    profile = json.loads(paths["profile"].read_text())
    catalog = json.loads(paths["catalog"].read_text())
    cancel = json.loads(paths["cancel"].read_text())
    cdc = json.loads(paths["cdc"].read_text())
    delete_fx = json.loads(paths["delete"].read_text())
    errors = []

    if len(docs) != expected["n"]:
        errors.append(f"doc count {len(docs)}")
    classified = classify(docs)
    for name, spec in expected["fields"].items():
        got = classified["fields"].get(name)
        if got is None:
            errors.append(f"missing field {name}")
            continue
        for key in ("bucket", "dominant", "present", "dominant_count"):
            if got[key] != spec[key]:
                errors.append(f"{name} {key} got {got[key]} want {spec[key]}")
        if spec["bucket"] == "unclassified" and spec.get("default_visible") is not False:
            errors.append(f"{name} visibility")
    extra = set(classified["fields"]) - set(expected["fields"])
    if extra:
        errors.append("unexpected fields " + ",".join(sorted(extra)))

    seen = set()
    for doc in docs:
        try:
            seen.add(row_index(doc))
            check_doc(doc, errors)
        except (AssertionError, KeyError, ValueError) as exc:
            errors.append(f"doc {exc}")
    if seen != set(range(100)):
        errors.append("id set is not 0..99")

    by_index = {}
    for doc in docs:
        try:
            by_index[row_index(doc)] = doc
        except (AssertionError, KeyError, ValueError):
            pass
    for key, anchor in expected["anchors"].items():
        i = int(key)
        if i not in by_index:
            errors.append(f"anchor doc {i}")
            continue
        check_anchor(by_index[i], anchor, errors, i)

    if any(isinstance(doc.get("units"), (int, float)) for doc in docs):
        errors.append("canonical units lost the int64 wrapper")
    if any(isinstance(doc.get("price"), (int, float, str)) for doc in docs):
        errors.append("canonical price lost the decimal wrapper")

    try:
        check_relaxed(relaxed, errors)
    except (AssertionError, KeyError, ValueError) as exc:
        errors.append(f"relaxed {exc}")

    profile_summary = check_profile(profile, errors)
    names = business_collections(catalog)
    if names != ["orders"]:
        errors.append(f"business collections {names}")
    if catalog.get("helloHasSetName") is not False:
        errors.append("hello setName present")
    hello = catalog.get("hello", {})
    if "setName" in hello:
        errors.append("hello document has setName")
    count = count_value(catalog.get("collStats", {}).get("count"))
    if count != 100:
        errors.append(f"collStats count {count}")
    indexes = catalog.get("listIndexes", {}).get("cursor", {}).get("firstBatch", [])
    sku = [item for item in indexes if item.get("name") == "sku_1"]
    if len(sku) != 1 or sku[0].get("unique") is not True:
        errors.append("sku unique index missing")
    roles = roles_of(catalog)
    if ("read", "oo_synth") not in roles:
        errors.append(f"reader role missing {roles}")
    privileged = {"root", "readWrite", "dbAdmin", "userAdmin", "userAdminAnyDatabase", "dbOwner", "clusterAdmin"}
    if any(role in privileged for role, _db in roles):
        errors.append(f"reader has privileged role {roles}")

    if cancel.get("cursor_nonzero") is not True:
        errors.append("cancel cursor was exhausted")
    kill = cancel.get("kill", {})
    if not is_ok(kill.get("ok")):
        errors.append("killCursors failed")
    killed_ids = kill.get("cursorsKilled") or []
    if len(killed_ids) < 1:
        errors.append("no cursor killed")
    follow = cancel.get("getMore", {})
    if is_ok(follow.get("ok")):
        errors.append("getMore succeeded after kill")
    batch = (follow.get("cursor") or {}).get("nextBatch")
    if batch:
        errors.append("getMore returned documents after kill")

    if is_ok(cdc.get("ok")):
        errors.append("change stream succeeded")
    errmsg = str(cdc.get("errmsg", ""))
    code = count_value(cdc.get("code"))
    topology = code == 40573 or "replica set" in errmsg.lower() or "change stream" in errmsg.lower() or "changestream" in errmsg.lower()
    if not topology:
        errors.append(f"cdc refusal was not a topology error code={code}")

    mapped = map_delete(delete_fx["event"])
    if mapped != delete_fx["expect"]:
        errors.append("delete mapping")
    if delete_fx.get("source") != "offline_fixture_not_change_stream":
        errors.append("delete fixture source")

    # IEEE loss the accepted canonical path must not depend on.
    if int(float(9007199254740993)) == 9007199254740993:
        errors.append("float did not lose 2^53+1 in this runtime")

    result = {
        "ok": not errors,
        "errors": errors,
        "classified_buckets": {name: item["bucket"] for name, item in classified["fields"].items()},
        "profile": profile_summary,
        "business_collections": names,
        "reader_roles": roles,
        "cdc_code": code,
        "cdc_code_name": cdc.get("codeName"),
        "docs": len(docs),
    }
    return result


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--docs", type=pathlib.Path, required=True)
    parser.add_argument("--relaxed", type=pathlib.Path, required=True)
    parser.add_argument("--profile", type=pathlib.Path, required=True)
    parser.add_argument("--catalog", type=pathlib.Path, required=True)
    parser.add_argument("--cancel", type=pathlib.Path, required=True)
    parser.add_argument("--cdc", type=pathlib.Path, required=True)
    parser.add_argument("--expected", type=pathlib.Path, required=True)
    parser.add_argument("--delete", type=pathlib.Path, required=True)
    parser.add_argument("--out", type=pathlib.Path, required=True)
    args = parser.parse_args(argv)
    try:
        result = evaluate(
            {
                "docs": args.docs,
                "relaxed": args.relaxed,
                "profile": args.profile,
                "catalog": args.catalog,
                "cancel": args.cancel,
                "cdc": args.cdc,
                "expected": args.expected,
                "delete": args.delete,
            }
        )
    except (OSError, json.JSONDecodeError, KeyError, TypeError) as exc:
        result = {"ok": False, "errors": [f"oracle input {type(exc).__name__}: {exc}"]}
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    print("ORACLE_OK" if result["ok"] else "ORACLE_FAIL")
    for item in result["errors"]:
        print("ORACLE_ERROR " + item)
    return 0 if result["ok"] else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
