#!/usr/bin/env python3
"""Compare the Arrow decoder with a live mongosh extract and the server profile."""

from __future__ import annotations

import base64
import json
import pathlib
import re
import sys
from datetime import datetime, timezone

PRICE_SCALE = 7
PRICE_PRECISION = 34
TWO53 = 9007199254740992
EXPECTED_SCHEMA = [
    ("_id", "fixedsizebinary(12)", False),
    ("code", "utf8", False),
    ("flag", "int32", False),
    ("units", "int64", False),
    ("price", "decimal128(34,7)", False),
    ("created", "timestamp(ms,utc)", False),
    ("uid", "fixedsizebinary(16)", False),
    ("blob", "binary", False),
    ("blob_presence", "uint8", False),
    ("weight", "float64", False),
    ("warehouse", "utf8", False),
    ("whCode", "utf8", False),
    ("lane", "union(dense,0:utf8,1:int64)", False),
    ("memo", "utf8", True),
    ("memo_presence", "uint8", False),
    ("marks", "list(utf8)", True),
    ("marks_presence", "uint8", False),
    ("pack", "struct(kind:utf8,n:int32)", True),
    ("pack_presence", "uint8", False),
]
RECORD_KEYS = [
    "_id", "code", "flag", "units", "price", "created_ms", "uid_hex", "blob_hex",
    "blob_subtype", "blob_subtype_stored", "blob_presence", "weight", "warehouse",
    "whCode", "lane_tag", "lane_text", "lane_num", "memo_presence", "memo",
    "marks_presence", "marks", "pack_presence", "pack_kind", "pack_n",
]
FORBIDDEN = (
    "SKU-", "wh-east", "9007199254740993", "64b0b1c0e1d2f3a4", "oo_synth",
    "oo_dl01w1_14", "P31-", "wh-north", "9007199254743007", "a131f00d31a1b2c3",
    "oo_w131", "1700000000000",
)
WRITES = {
    "insert", "update", "delete", "findAndModify", "drop", "create", "createIndexes",
    "profile", "compact", "rename", "dropDatabase", "collMod", "killOp",
}
ALLOWED = {
    "hello", "ismaster", "isMaster", "saslStart", "saslContinue", "find", "getMore",
    "killCursors", "endSessions", "listCollections", "listIndexes", "collStats",
    "connectionStatus", "aggregate", "ping", "buildInfo",
}


def unscaled(text: str) -> int:
    if text in {"NaN", "Infinity", "-Infinity", "Inf", "-Inf", "sNaN"}:
        raise ValueError(f"DECIMAL128_NON_FINITE:{text}")
    mantissa = text
    exponent = 0
    for mark in ("e", "E"):
        if mark in text:
            mantissa, exponent_text = text.split(mark, 1)
            exponent = int(exponent_text)
            break
    negative = mantissa.startswith("-")
    mantissa = mantissa.lstrip("+-")
    if "." in mantissa:
        whole, frac = mantissa.split(".", 1)
    else:
        whole, frac = mantissa, ""
    if not whole or not whole.isdigit() or not frac.isdigit():
        raise ValueError(f"DECIMAL128_PRECISION:{text}")
    digits = f"{whole}{frac}"
    scale = len(frac) - exponent
    trimmed = digits.lstrip("0")
    if not trimmed:
        return 0
    if len(trimmed) > PRICE_PRECISION:
        raise ValueError(f"DECIMAL128_PRECISION:{text}")
    if scale > PRICE_SCALE:
        extra = scale - PRICE_SCALE
        if extra > len(trimmed):
            raise ValueError(f"DECIMAL128_PRECISION:{text}")
        keep, drop = trimmed[:-extra], trimmed[-extra:]
        if any(char != "0" for char in drop):
            raise ValueError(f"DECIMAL128_PRECISION:{text}")
        value = int(keep or "0")
        return -value if negative else value
    pad = PRICE_SCALE - scale
    if len(trimmed) + pad > PRICE_PRECISION:
        raise ValueError(f"DECIMAL128_PRECISION:{text}")
    value = int(trimmed + ("0" * pad))
    return -value if negative else value


def unwrap_number(value):
    kind = ejson_kind(value)
    if kind in {"$numberInt", "$numberLong"}:
        return int(value[kind])
    if kind == "$numberDouble":
        return float(value[kind])
    return value


def is_ok(value) -> bool:
    number = unwrap_number(value)
    return number == 1


def ejson_kind(value):
    if isinstance(value, dict) and len(value) == 1:
        key = next(iter(value))
        if key.startswith("$"):
            return key
    return None


def date_ms(value) -> int:
    if ejson_kind(value) != "$date":
        raise ValueError("date")
    inner = value["$date"]
    if isinstance(inner, dict) and "$numberLong" in inner:
        return int(inner["$numberLong"])
    if isinstance(inner, str):
        text = inner[:-1] + "+00:00" if inner.endswith("Z") else inner
        parsed = datetime.fromisoformat(text)
        if parsed.tzinfo is None:
            parsed = parsed.replace(tzinfo=timezone.utc)
        epoch = datetime(1970, 1, 1, tzinfo=timezone.utc)
        delta = parsed.astimezone(timezone.utc) - epoch
        return int(delta.total_seconds() * 1000)
    if isinstance(inner, int) and not isinstance(inner, bool):
        return inner
    raise ValueError("date")


def as_int(value) -> int:
    kind = ejson_kind(value)
    if kind == "$numberInt":
        return int(value["$numberInt"])
    if kind == "$numberLong":
        return int(value["$numberLong"])
    if isinstance(value, int) and not isinstance(value, bool):
        return value
    raise ValueError(f"int {value!r}")


def as_long(value) -> str:
    kind = ejson_kind(value)
    if kind == "$numberLong":
        return str(value["$numberLong"])
    if isinstance(value, int) and not isinstance(value, bool):
        return str(value)
    raise ValueError("long")


def as_double(value) -> float:
    kind = ejson_kind(value)
    if kind == "$numberDouble":
        return float(value["$numberDouble"])
    if isinstance(value, float):
        return value
    if isinstance(value, int) and not isinstance(value, bool):
        return float(value)
    raise ValueError("double")


def binary_parts(value):
    kind = ejson_kind(value)
    if kind == "$binary":
        body = value["$binary"]
        if isinstance(body, dict):
            raw = base64.b64decode(body["base64"])
            subtype = int(str(body.get("subType", "0")), 16)
            return raw, subtype
    if kind == "$uuid":
        return bytes.fromhex(value["$uuid"].replace("-", "")), 4
    raise ValueError("binary")


def oid_hex(value) -> str:
    if ejson_kind(value) != "$oid":
        raise ValueError("oid")
    return value["$oid"].lower()


def memo_fields(doc):
    if "memo" not in doc:
        return 0, None
    value = doc["memo"]
    if value is None:
        return 1, None
    if isinstance(value, str):
        return (2, "") if value == "" else (3, value)
    raise ValueError("memo")


def marks_fields(doc):
    if "marks" not in doc:
        return 0, None
    value = doc["marks"]
    if value is None:
        return 1, None
    if isinstance(value, list):
        if not value:
            return 2, []
        items = []
        for item in value:
            if not isinstance(item, str):
                raise ValueError(f"marks {ejson_kind(item) or type(item).__name__}")
            items.append(item)
        return 3, items
    raise ValueError("marks")


def pack_fields(doc):
    if "pack" not in doc:
        return 0, None, None
    value = doc["pack"]
    if value is None:
        return 1, None, None
    if isinstance(value, dict) and ejson_kind(value) is None:
        if not value:
            return 2, None, None
        kind = value.get("kind")
        if not isinstance(kind, str):
            raise ValueError("pack.kind")
        return 3, kind, as_int(value.get("n"))
    raise ValueError("pack")


def lane_fields(value):
    if isinstance(value, str):
        return "utf8", value, None
    if ejson_kind(value) == "$numberLong":
        return "int64", None, str(value["$numberLong"])
    raise ValueError("lane")


def normalize_shipment(doc) -> dict:
    memo_presence, memo = memo_fields(doc)
    marks_presence, marks = marks_fields(doc)
    pack_presence, pack_kind, pack_n = pack_fields(doc)
    uid, uid_subtype = binary_parts(doc["uid"])
    if uid_subtype != 4 or len(uid) != 16:
        raise ValueError(f"uid subtype {uid_subtype} len {len(uid)}")
    blob, blob_subtype = binary_parts(doc["blob"])
    if blob_subtype != 0:
        raise ValueError(f"blob subtype {blob_subtype}")
    lane_tag, lane_text, lane_num = lane_fields(doc["lane"])
    return {
        "_id": oid_hex(doc["_id"]),
        "code": doc["code"],
        "flag": as_int(doc["flag"]),
        "units": as_long(doc["units"]),
        "price": str(unscaled(doc["price"]["$numberDecimal"])),
        "created_ms": date_ms(doc["created"]),
        "uid_hex": uid.hex(),
        "blob_hex": blob.hex(),
        "blob_subtype": None,
        "blob_subtype_stored": False,
        "blob_presence": 2 if not blob else 3,
        "weight": as_double(doc["weight"]),
        "warehouse": doc["warehouse"],
        "whCode": doc["whCode"],
        "lane_tag": lane_tag,
        "lane_text": lane_text,
        "lane_num": lane_num,
        "memo_presence": memo_presence,
        "memo": memo,
        "marks_presence": marks_presence,
        "marks": marks,
        "pack_presence": pack_presence,
        "pack_kind": pack_kind,
        "pack_n": pack_n,
    }


def self_check() -> int:
    plain = unscaled("314159265358979.0000041")
    scientific = unscaled("3.141592653589790000041E+14")
    if plain != 3141592653589790000041 or scientific != plain:
        print("price self-check failed")
        return 2
    for text in ("NaN", "Infinity", "-Infinity"):
        try:
            unscaled(text)
        except ValueError as err:
            if "NON_FINITE" not in str(err):
                print(err)
                return 2
        else:
            print(f"{text} was accepted")
            return 2
    if "PRECISION" not in str(expect_error("1.00000001")):
        print("precision self-check failed")
        return 2
    stamp = date_ms({"$date": "2024-08-30T06:40:00.000Z"})
    if stamp != 1725000000000:
        print(f"date self-check {stamp}")
        return 2
    print("oracle self-check ok")
    return 0


def expect_error(text: str) -> str:
    try:
        unscaled(text)
    except ValueError as err:
        return str(err)
    return ""


def parse_args(argv: list[str]) -> dict:
    parsed = {"self_check": False}
    index = 0
    while index < len(argv):
        item = argv[index]
        if item == "--self-check":
            parsed["self_check"] = True
            index += 1
            continue
        if not item.startswith("--") or index + 1 >= len(argv):
            raise SystemExit(f"bad argument {item}")
        parsed[item[2:].replace("-", "_")] = argv[index + 1]
        index += 2
    return parsed


def load_json(path: pathlib.Path):
    return json.loads(path.read_text())


def load_jsonl(path: pathlib.Path) -> list:
    if not path.is_file():
        raise FileNotFoundError(path)
    rows = []
    for line in path.read_text().splitlines():
        if line.strip():
            rows.append(json.loads(line))
    return rows


def auth_handshake(wire: list) -> tuple:
    """The monitor hello can precede the handshake that carries SCRAM."""
    for index, row in enumerate(wire):
        if not isinstance(row, dict):
            continue
        command = str(row.get("command"))
        later = [str(item.get("command")) for item in wire[index + 1 :] if isinstance(item, dict)]
        if command == "saslStart":
            return command, False, "saslStart"
        if command in {"hello", "ismaster", "isMaster"} and row.get("speculative_authenticate") is True and "saslContinue" in later:
            return command, True, "speculativeAuthenticate+saslContinue"
    return None, False, "absent"


def check_wire(path: pathlib.Path, command_names: list, failures: list[str], result: dict) -> None:
    try:
        wire = load_jsonl(path)
    except (OSError, json.JSONDecodeError, FileNotFoundError) as err:
        failures.append(f"wire audit unreadable: {err.__class__.__name__}")
        return
    for row in wire:
        if isinstance(row, dict) and set(row) - {"command", "speculative_authenticate"}:
            failures.append("wire audit recorded a field other than the command name")
            break
    names = [str(row.get("command")) for row in wire if isinstance(row, dict)]
    result["wire_commands"] = sorted(set(names))
    result["handshake_source"] = "opmsg-wire"
    monitored = {"hello", "ismaster", "isMaster", "saslStart", "saslContinue"}
    result["command_monitoring_omits_handshake"] = not any(name in monitored for name in command_names)
    if not names:
        failures.append("wire audit is empty")
        return
    if any(name in {"unlisted", "oversize", "unparsed"} for name in names):
        failures.append("wire audit saw an unparsed command")
    unknown = sorted({name for name in names if name not in ALLOWED and name not in {"unlisted", "oversize", "unparsed"}})
    if unknown:
        failures.append("wire command outside the reader allow-list: " + ",".join(unknown))
    if any(name in WRITES for name in names):
        failures.append("wire audit saw a write command")
    first = wire[0] if isinstance(wire[0], dict) else {}
    handshake = str(first.get("command"))
    result["handshake_command"] = handshake
    if handshake not in {"hello", "ismaster", "isMaster"}:
        failures.append("wire audit handshake is not hello")
    auth_command, speculative, observation = auth_handshake(wire)
    result["auth_handshake_command"] = auth_command
    result["handshake_speculative_authenticate"] = speculative
    result["auth_observation"] = observation
    if observation == "absent":
        failures.append("wire audit missing auth handshake")
    for required in ("find", "getMore", "killCursors", "endSessions"):
        if required not in names:
            failures.append(f"wire audit missing {required}")


def check_monitor(sdam, failures: list[str], result: dict) -> None:
    if not isinstance(sdam, list):
        failures.append("sdam events were not recorded")
        return
    reply_keys = {"ok", "maxWireVersion", "minWireVersion", "hello_ok", "set_name_present", "speculative_authenticate_present"}
    started = [row for row in sdam if isinstance(row, dict) and row.get("kind") == "ServerHeartbeatStarted"]
    ids = sorted({row.get("driver_connection_id") for row in started if isinstance(row.get("driver_connection_id"), int)})
    result["monitor_connection_ids"] = ids
    result["monitor_heartbeats"] = len(started)
    if not started:
        failures.append("driver emitted no server heartbeat")
    if len(ids) > 2:
        failures.append("monitor connections are not bounded")
    for row in sdam:
        if not isinstance(row, dict):
            continue
        reply = row.get("reply")
        if isinstance(reply, dict) and set(reply) - reply_keys:
            failures.append("sdam reply was not redacted")
            break


def secrets_of(path: pathlib.Path) -> list[str]:
    found = []
    if not path.is_dir():
        return found
    for item in sorted(path.iterdir()):
        if item.is_file():
            text = item.read_text(errors="replace").strip()
            if len(text) >= 12:
                found.append(text)
    return found


def scan_secrets(roots: list[pathlib.Path], secrets: list[str]) -> list[str]:
    needles = [item.encode() for item in secrets]
    hits = []
    for root in roots:
        if not root.exists():
            continue
        for path in root.rglob("*"):
            if not path.is_file() or path.name == "oracle-result.json":
                continue
            data = path.read_bytes()
            if not any(needle in data for needle in needles):
                continue
            hits.append(str(path))
            if path.suffix in {".json", ".txt", ".out", ".err", ".md", ".log"} or path.name in {"phase.txt"}:
                text = data.decode("utf-8", "replace")
                for secret in secrets:
                    text = text.replace(secret, "[redacted]")
                path.write_text(text)
    return hits


def cursor_for(events: list, comment: str):
    request = None
    for event in events:
        if event.get("phase") == "started" and event.get("command") == "find" and event.get("comment") == comment:
            request = event.get("request_id")
    if request is None:
        return None
    for event in events:
        if event.get("phase") == "succeeded" and event.get("request_id") == request:
            return event.get("cursor_id")
    return None


def driver_cursors(events: list) -> set[str]:
    found = set()
    for event in events:
        cursor = event.get("cursor_id")
        if isinstance(cursor, str) and cursor != "0":
            found.add(cursor)
        for cursor in event.get("kill_cursor_ids") or []:
            if isinstance(cursor, str) and cursor != "0":
                found.add(cursor)
    return found


def bucket(row: dict, cursors: set[str]) -> str:
    user = str(row.get("user") or "")
    app = str(row.get("appName") or "")
    comment = str(row.get("comment") or "")
    if "oo_dl01w1_41_reader" in user or app == "oo-dl01w1-41-reader" or ":reader:" in comment:
        return "reader"
    if "oo_dl01w1_41_dba" in user or app == "oo-dl01w1-41-dba" or ":dba:" in comment:
        return "dba"
    if "oo_dl01w1_41_audit" in user or app == "oo-dl01w1-41-audit" or ":audit:" in comment:
        return "audit"
    ids = [row.get("cursorid"), row.get("getMore")] + list(row.get("kill_cursors") or [])
    if any(isinstance(item, str) and item in cursors for item in ids):
        return "reader"
    return "unattributed"


def index_view(doc: dict) -> dict:
    keys = []
    for field, direction in (doc.get("key") or {}).items():
        keys.append([field, as_int(direction)])
    collation = doc.get("collation")
    rendered = None
    if isinstance(collation, dict) and ejson_kind(collation) is None:
        strength = collation.get("strength")
        rendered = {
            "locale": collation.get("locale"),
            "strength": None if strength is None else as_int(strength),
        }
    unique = doc.get("unique")
    return {
        "name": doc.get("name"),
        "keys": keys,
        "unique": unique if isinstance(unique, bool) else None,
        "collation": rendered,
    }


def same_index(driver: dict, source: dict) -> bool:
    return (
        driver.get("name") == source.get("name")
        and driver.get("keys") == source.get("keys")
        and (driver.get("unique") if isinstance(driver.get("unique"), bool) else None) == source.get("unique")
        and driver.get("collation") == source.get("collation")
    )


def main() -> int:
    args = parse_args(sys.argv[1:])
    if args["self_check"]:
        return self_check()
    out_path = pathlib.Path(args["out"])
    failures: list[str] = []
    result = {"ready": False, "failures": failures}
    try:
        evaluate(args, failures, result)
    except Exception as err:
        failures.append(f"oracle exception: {err}")
    result["ready"] = not failures
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    print("oracle ready=" + str(result["ready"]).lower() + f" failures={len(failures)}")
    return 0 if result["ready"] else 1


def evaluate(args: dict, failures: list[str], result: dict) -> None:
    evidence = pathlib.Path(args["evidence"])
    source = pathlib.Path(args["source"])
    secrets = secrets_of(pathlib.Path(args["secrets"]))
    hits = scan_secrets([evidence, source], secrets)
    result["secret_hits"] = hits
    if hits:
        failures.append("secret string found in evidence or source")
    ejson = load_json(pathlib.Path(args["ejson"]))
    decoded = load_json(pathlib.Path(args["decoded"]))
    truncated = load_json(pathlib.Path(args["decoded_truncated"]))
    gates = load_json(pathlib.Path(args["gates"]))
    commands = load_json(pathlib.Path(args["commands"]))
    connections = load_json(pathlib.Path(args["connections"]))
    catalog = load_json(pathlib.Path(args["catalog"]))
    census = load_json(pathlib.Path(args["census"]))
    rejections = load_json(pathlib.Path(args["rejections"]))
    timeout = load_json(pathlib.Path(args["timeout"]))
    change = load_json(pathlib.Path(args["change"]))
    cancel = load_json(pathlib.Path(args["cancel"]))
    truncation = load_json(pathlib.Path(args["truncation"]))
    options = load_json(pathlib.Path(args["options"]))
    audit = load_json(pathlib.Path(args["audit"]))
    admin_profile = load_json(pathlib.Path(args["profile_admin"]))
    data_profile = load_json(pathlib.Path(args["profile_db"]))
    batch_arrow = pathlib.Path(args["batch_arrow"])
    partial = pathlib.Path(args["partial"])
    mode_path = pathlib.Path(args["execution_mode"])
    decode_exit = pathlib.Path(args["decode_exit"]).read_text().strip()
    trunc_exit = pathlib.Path(args["decode_truncated_exit"]).read_text().strip()

    if batch_arrow.exists():
        failures.append("batch.arrow existed before promotion")
    if not partial.is_file():
        failures.append("batch.arrow.partial missing")
    mode = mode_path.read_text() if mode_path.is_file() else ""
    if "prebuilt-binary" not in mode:
        failures.append("probe execution mode is not a prebuilt binary")
    if "cargo-invoked" in mode:
        failures.append("probe command invoked cargo or rustc")
    if decode_exit != "0" or decoded.get("ready") is not True or decoded.get("driver") != "none":
        failures.append(f"decoder exit {decode_exit} ready {decoded.get('ready')}")
    if trunc_exit == "0" or truncated.get("ready") is not False:
        failures.append("truncated IPC produced a ready decoder result")
    if truncation.get("promoted") is not False or truncation.get("reason") != "BYTE_CAP":
        failures.append("truncation file is not an unpromoted byte cap")
    if cancel.get("promoted") is not False or cancel.get("dba_kill") is not False:
        failures.append("cancel file claims promotion or a DBA kill")
    if gates.get("hello_seen") is not True:
        failures.append("statement audit omitted hello")
    if gates.get("sasl_start_seen") is not True:
        failures.append("statement audit omitted saslStart")
    if gates.get("secret_body_recorded") is not False:
        failures.append("statement audit recorded a secret body")
    pre = load_json(pathlib.Path(args["pre_read"]))
    if pre.get("refused") is not True or pre.get("find_sent") is not False or pre.get("password_read") is not False:
        failures.append("unapproved object was not refused before the read")
    if pre.get("reason") != "unapproved_object":
        failures.append("pre-read denial reason is not unapproved_object")
    deny_object = str(pre.get("object") or "")
    deny_collection = deny_object.split(".", 1)[-1] if "." in deny_object else deny_object
    if gates.get("ready") is not False or gates.get("promoted") is not False:
        failures.append("probe gates claimed ready")
    if gates.get("rows") != 16 or int(gates.get("ipc_bytes") or 0) > 262144:
        failures.append("row or byte cap missed")
    if options.get("retry_writes") is not False or options.get("retry_reads") is not False:
        failures.append("retryWrites or retryReads was not false")
    if options.get("direct_connection") is not True or options.get("max_pool_size") != 1:
        failures.append("pool or direct connection was not bounded")
    if options.get("min_pool_size") != 0 or options.get("max_connecting") != 1 or options.get("tls") is not False:
        failures.append("connection options drifted")

    info = ejson["buildInfo"]
    result["server_version"] = info.get("version")
    result["server_git"] = info.get("gitVersion")
    result["server_modules"] = info.get("modules")
    if info.get("version") != "8.0.32" or info.get("gitVersion") != "f9eb55a7cc900f33a722a5b32a0fdbf54385c749":
        failures.append("buildInfo does not match the pinned 8.0.32 community server")
    if info.get("modules") not in ([], None):
        failures.append("buildInfo modules are not empty")
    hello = ejson.get("hello") or {}
    result["set_name_present"] = "setName" in hello
    if "setName" in hello:
        failures.append("hello reported a replica set name")
    cmdline = ejson.get("getCmdLineOpts") or {}
    result["cmdline_ok"] = unwrap_number(cmdline.get("ok"))
    result["cmdline_code"] = unwrap_number(cmdline.get("code"))
    if is_ok(cmdline.get("ok")):
        parsed = cmdline.get("parsed") or {}
        bind = ((parsed.get("net") or {}).get("bindIp"))
        auth = ((parsed.get("security") or {}).get("authorization"))
        result["bind_ip"] = bind
        result["authorization"] = auth
        if bind != "127.0.0.1" or auth != "enabled":
            failures.append("getCmdLineOpts bind or authorization differs from the fixture")

    source_indexes = [index_view(doc) for doc in ejson["listIndexes"]["documents"]]
    result["source_indexes"] = source_indexes
    by_name = {row["name"]: row for row in source_indexes}
    if "code_1" not in by_name or by_name["code_1"]["unique"] is not True:
        failures.append("code_1 unique index missing")
    if by_name.get("code_1", {}).get("collation") != {"locale": "en", "strength": 2}:
        failures.append("code_1 collation is not locale en strength 2")
    if by_name.get("_id_", {}).get("unique") not in (None, False):
        failures.append("_id index unique was invented")
    for driver_index in catalog.get("indexes") or []:
        source_index = by_name.get(driver_index.get("name"))
        if source_index is None or not same_index(driver_index, source_index):
            failures.append(f"driver index {driver_index.get('name')} differs from listIndexes")
    shipments = [doc for doc in (ejson.get("listCollections") or {}).get("documents") or [] if doc.get("name") == "shipments"]
    if not shipments or "validator" in (shipments[0].get("options") or {}):
        failures.append("shipments collection is missing or has a validator")
    driver_shipments = [row for row in catalog.get("collections") or [] if row.get("name") == "shipments"]
    if not driver_shipments or driver_shipments[0].get("validator_present") is not False:
        failures.append("driver catalog reports a validator")
    if catalog.get("sample_not_enforced_schema") is not True or census.get("sample_not_enforced_schema") is not True:
        failures.append("sample census was labeled as an enforced schema")
    who = catalog.get("connection_status") or {}
    roles = who.get("authenticated_roles")
    users = who.get("authenticated_users")
    result["reader_roles"] = roles
    result["reader_users"] = users
    if roles != [{"role": "read", "db": "oo_w141"}] or users != [{"user": "oo_dl01w1_41_reader", "db": "admin"}]:
        failures.append("reader role is not exactly read on oo_w141")

    records = decoded.get("records") or []
    schema = [(row.get("name"), row.get("type"), row.get("nullable")) for row in decoded.get("schema") or []]
    if schema != EXPECTED_SCHEMA:
        failures.append(f"schema {schema}")
    metadata = decoded.get("metadata") or {}
    if metadata.get("encoder") != "oo-dl01w1-41-typed-arrow" or metadata.get("json_document_column") != "absent":
        failures.append("schema metadata is not the typed encoder")
    names = [row[0] for row in schema]
    if any(name in {"json", "document", "row"} for name in names):
        failures.append("a column masquerades as a document blob")
    try:
        expected = [normalize_shipment(doc) for doc in ejson["shipments"]]
    except Exception as err:
        failures.append(f"extract normalize: {err}")
        expected = []
    if len(records) != 16 or len(expected) != 16:
        failures.append(f"record count decoder {len(records)} extract {len(expected)}")
    for index, (actual, want) in enumerate(zip(records, expected)):
        if set(actual) != set(RECORD_KEYS):
            failures.append(f"row {index} keys {sorted(set(actual) ^ set(RECORD_KEYS))}")
            break
        for key in RECORD_KEYS:
            if actual.get(key) != want.get(key):
                failures.append(f"row {index} {key}")
                break
        code = str(actual.get("code"))
        units = int(actual.get("units"))
        if not re.fullmatch(r"S41-\d{3}", code) or actual.get("warehouse") != "wh-west" or actual.get("whCode") != "W41":
            failures.append(f"row {index} identity")
        if units <= TWO53:
            failures.append(f"row {index} units fit in a float")
    blob = json.dumps(ejson) + json.dumps(decoded)
    for marker in FORBIDDEN:
        if marker in blob:
            failures.append(f"forbidden marker {marker}")
    odd_codes = {row.get("code"): row.get("reason") for row in rejections.get("decimal") or []}
    mixed_codes = {row.get("code"): row.get("reason") for row in rejections.get("heterogeneous_array") or []}
    if odd_codes != {"ODD-NAN": "DECIMAL128_NON_FINITE", "ODD-INF": "DECIMAL128_NON_FINITE", "ODD-NINF": "DECIMAL128_NON_FINITE"}:
        failures.append("non-finite decimal rejection mismatch")
    if mixed_codes != {"MIX-1": "HETEROGENEOUS_ARRAY_ELEMENT", "MIX-2": "HETEROGENEOUS_ARRAY_ELEMENT"}:
        failures.append("heterogeneous array rejection mismatch")
    batch_codes = {row.get("code") for row in records}
    if batch_codes & set(odd_codes) or batch_codes & set(mixed_codes):
        failures.append("rejected rows were present in the promoted batch")
    fields = {row.get("field"): row for row in census.get("fields") or []}
    if "whCode" not in fields or fields["whCode"].get("present") != 16:
        failures.append("whCode census missing")
    if fields.get("memo", {}).get("present") == 16 or fields.get("marks", {}).get("present") == 16:
        failures.append("optional memo or marks looked required in the sample")

    events = commands.get("events") or []
    if any(event.get("command") == "find" and event.get("collection") == deny_collection for event in events):
        failures.append("find was sent for the denied object")
    names_seen = list(commands.get("names") or [])
    result["driver_commands"] = sorted(set(names_seen))
    unknown = sorted({name for name in names_seen if name not in ALLOWED})
    if unknown:
        failures.append("driver command outside the reader allow-list: " + ",".join(unknown))
    if any(name in WRITES for name in names_seen):
        failures.append("driver emitted a write or operational command")
    if "endSessions" not in names_seen:
        failures.append("driver events missing endSessions")
    check_wire(pathlib.Path(args["wire"]), names_seen, failures, result)
    batch_cursor = cursor_for(events, "oo-dl01w1-41:reader:batch")
    cancel_cursor = cursor_for(events, "oo-dl01w1-41:reader:cancel")
    result["batch_cursor_id"] = batch_cursor
    result["cancel_cursor_id"] = cancel_cursor
    if not batch_cursor or batch_cursor == "0":
        failures.append("batch cursor was absent or exhausted in the first reply")
    elif not any(event.get("command") == "getMore" and event.get("cursor_id") == batch_cursor for event in events):
        failures.append("batch cursor produced no getMore")
    if not cancel_cursor or cancel_cursor == "0":
        failures.append("cancel cursor was absent or exhausted before killCursors")
    elif not any(cancel_cursor in (event.get("kill_cursor_ids") or []) for event in events):
        failures.append("driver events have no killCursors for the cancel cursor")
    if batch_cursor and cancel_cursor and batch_cursor == cancel_cursor:
        failures.append("cancel cursor id reused the batch cursor")

    profiles = []
    for label, body in (("admin", admin_profile), ("oo_w141", data_profile)):
        if not isinstance(body, list):
            failures.append(f"{label} profile was not readable")
            result[f"{label}_profile_error"] = body if isinstance(body, dict) else "unreadable"
            continue
        profiles.extend(body)
    if audit.get("ok") is not True:
        failures.append("audit identity could not read both profiles")
        result["audit"] = audit
    cursors = driver_cursors(events)
    grouped = {"reader": [], "dba": [], "audit": [], "unattributed": []}
    kill_in_profile = False
    end_in_profile = False
    for row in profiles:
        if not isinstance(row, dict):
            continue
        kind = bucket(row, cursors)
        command = str(row.get("command") or "")
        grouped[kind].append(command)
        if command == "endSessions":
            end_in_profile = True
        if command == "killCursors" and cancel_cursor in (row.get("kill_cursors") or []):
            kill_in_profile = True
        if command == "killOp":
            failures.append("profile contains killOp")
        if kind == "reader" and command not in ALLOWED:
            failures.append(f"reader profile command {command}")
        if kind == "reader" and command in WRITES:
            failures.append(f"reader profile write {command}")
        if kind == "unattributed" and command in WRITES:
            failures.append(f"unattributed profile write {command}")
    result["profile_commands"] = {key: sorted(set(value)) for key, value in grouped.items()}
    result["dba_write_commands"] = sorted({item for item in grouped["dba"] if item in WRITES})
    result["end_sessions_profile_gap"] = "endSessions" in names_seen and not end_in_profile
    if cancel_cursor and cancel_cursor != "0" and not kill_in_profile:
        failures.append("pending gate: read-role killCursors was not in the server profile")
    if timeout.get("client_cancel") is not False:
        failures.append("maxTimeMS result was labeled as a client cancel")
    result["timeout_label"] = timeout.get("label")
    result["timeout_code"] = timeout.get("code")
    if timeout.get("label") != "timeout" or timeout.get("code") != 50 or timeout.get("server_ended") is not True:
        failures.append("source deadline did not end with MaxTimeMSExpired")
    result["change_stream_code"] = change.get("code")
    result["change_stream_opened"] = change.get("opened")
    if change.get("opened") is True:
        result["change_stream_limitation"] = False
    elif change.get("code") == 40573:
        result["change_stream_limitation"] = True
    else:
        failures.append("change stream returned neither 40573 nor an open cursor")
    created = connections.get("connection_created")
    result["connection_created"] = created
    pool = connections.get("pool_bound") or {}
    if created != 1:
        failures.append(f"business connection count {created}")
    check_monitor(connections.get("sdam"), failures, result)
    if pool.get("max_pool_size") != 1 or pool.get("min_pool_size") != 0 or pool.get("max_connecting") != 1:
        failures.append("pool bound was not recorded")
    if connections.get("server_monitoring_mode") != "Poll":
        failures.append("server monitoring mode was not Poll")
    result["capture_limitations"] = catalog.get("capture_limitations")
    if not result["capture_limitations"]:
        failures.append("catalog omitted capture limitations")


if __name__ == "__main__":
    sys.exit(main())
