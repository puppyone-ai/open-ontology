#!/usr/bin/env python3
"""Thin replica-set fixture and C1 orchestration. The capture itself is the Rust binary."""

import json
import pathlib
import subprocess
import sys
import time

ROOT = pathlib.Path("/tmp/oo-mongodb-connection-whole")
SOURCE = "oo-mongodb-connection-whole-source"
RS = "oo-mongodb-connection-whole-rs"
RUNTIME = "oo-mongodb-connection-whole-runtime"
CONTROL = ROOT / "c1-run"
BIN = ROOT / "c1-target" / "debug" / "mongodb-preview"
PRIVATE = "/private"


def run(args, check=True, timeout=120):
    result = subprocess.run(args, text=True, capture_output=True, timeout=timeout)
    if check and result.returncode != 0:
        tail = (result.stderr or "")[-800:] + (result.stdout or "")[-800:]
        raise SystemExit(f"{args[0]} {args[-1]} failed\n{tail}")
    return result


def mongosh(container, name, script, timeout=120):
    path = ROOT / name
    path.write_text(script)
    return run(["docker", "exec", container, "mongosh", "--quiet", "--file", f"{PRIVATE}/{name}"], timeout=timeout)


def up():
    mongosh(SOURCE, "c1-export.js", """
const fs = require('fs');
const pwd = fs.readFileSync('/private/fixture-admin.password', 'utf8').trim();
const admin = db.getSiblingDB('admin');
if (!admin.auth('connection_fixture_admin', pwd)) quit(2);
const docs = db.getSiblingDB('oo_w141').shipments.find().toArray();
fs.writeFileSync('/private/c1-live-seed.ejson', EJSON.stringify(docs, {relaxed: false}));
print('SEED ' + docs.length);
""")
    deadline = time.time() + 90
    while time.time() < deadline:
        probe = run(["docker", "exec", RS, "mongosh", "--quiet", "--eval", "db.runCommand({ping:1}).ok"], check=False)
        if probe.returncode == 0 and probe.stdout.strip() == "1":
            break
        time.sleep(1)
    else:
        raise SystemExit("replica set did not answer ping")
    mongosh(RS, "c1-init.js", """
const fs = require('fs');
const adminPwd = fs.readFileSync('/private/fixture-admin.password', 'utf8').trim();
const readerPwd = fs.readFileSync('/private/reader.password', 'utf8').trim();
try { rs.initiate({_id: 'rs0', members: [{_id: 0, host: 'oo-mongodb-connection-whole-rs:27017'}]}); } catch (err) {}
const started = Date.now();
while (!db.hello().isWritablePrimary) {
  if (Date.now() - started > 60000) quit(3);
  sleep(500);
}
const admin = db.getSiblingDB('admin');
try { admin.createUser({user: 'connection_fixture_admin', pwd: adminPwd, roles: ['root']}); }
catch (err) { if (err.code !== 51003) print('admin-create ' + err.code); }
if (!admin.auth('connection_fixture_admin', adminPwd)) quit(2);
try { admin.createUser({user: 'connection_reader', pwd: readerPwd, roles: [{role: 'read', db: 'oo_w141'}]}); }
catch (err) { if (err.code !== 51003) print('reader-create ' + err.code); }
const dbx = db.getSiblingDB('oo_w141');
const docs = EJSON.parse(fs.readFileSync('/private/c1-live-seed.ejson', 'utf8'), {relaxed: false});
if (dbx.shipments.countDocuments({}) === 0) dbx.shipments.insertMany(docs);
const mod = dbx.runCommand({collMod: 'shipments', changeStreamPreAndPostImages: {enabled: true}});
if (mod.ok !== 1) quit(4);
const info = dbx.getCollectionInfos({name: 'shipments'})[0];
const reader = new Mongo('mongodb://127.0.0.1:27017/admin');
if (!reader.getDB('admin').auth('connection_reader', readerPwd)) quit(5);
let writeCode = 0;
try { reader.getDB('oo_w141').shipments.insertOne({_id: 'reader-must-not-write'}); writeCode = -1; }
catch (err) { writeCode = err.code; }
const body = {
  storage_engine: db.serverStatus().storageEngine.name,
  set_name: rs.status().set,
  pre_and_post_images: info.options.changeStreamPreAndPostImages,
  rows: dbx.shipments.countDocuments({}),
  reader_write_code: writeCode,
  oplog_max_mb: rs.status().members ? db.getSiblingDB('local').oplog.rs.stats().maxSize : null
};
fs.writeFileSync('/private/c1-dba.json', JSON.stringify(body));
print(JSON.stringify(body));
""")
    body = json.loads((ROOT / "c1-dba.json").read_text())
    if body.get("storage_engine") != "wiredTiger" or body.get("set_name") != "rs0":
        raise SystemExit("replica set storage or name was not wiredTiger/rs0")
    if body.get("reader_write_code") != 13:
        raise SystemExit("reader write was not unauthorized")
    images = body.get("pre_and_post_images") or {}
    if images.get("enabled") is not True:
        raise SystemExit("pre/post images are not enabled")
    print(json.dumps({"up": True, "rows": body.get("rows"), "engine": body.get("storage_engine")}))


def wait_flag(name, seconds):
    deadline = time.time() + seconds
    path = CONTROL / name
    while time.time() < deadline:
        if path.is_file():
            return
        time.sleep(0.2)
    raise SystemExit(f"{name} missing")


def admin_docs():
    result = mongosh(RS, "c1-gold.js", """
const fs = require('fs');
const pwd = fs.readFileSync('/private/fixture-admin.password', 'utf8').trim();
if (!db.getSiblingDB('admin').auth('connection_fixture_admin', pwd)) quit(2);
print(EJSON.stringify(db.getSiblingDB('oo_w141').shipments.find().toArray(), {relaxed: false}));
""")
    text = result.stdout.strip().splitlines()[-1]
    return json.loads(text)


def key_of(value):
    if isinstance(value, str):
        return value
    if isinstance(value, dict) and "$oid" in value:
        return value["$oid"].lower()
    return json.dumps(value, sort_keys=True, separators=(",", ":"))


def dml(name, script, timeout=120):
    mongosh(RS, name, script, timeout=timeout)


def reset_seed():
    dml("c1-reset.js", """
const fs = require('fs');
const pwd = fs.readFileSync('/private/fixture-admin.password', 'utf8').trim();
if (!db.getSiblingDB('admin').auth('connection_fixture_admin', pwd)) quit(2);
const docs = EJSON.parse(fs.readFileSync('/private/c1-live-seed.ejson', 'utf8'), {relaxed: false});
const coll = db.getSiblingDB('oo_w141').shipments;
coll.deleteMany({});
coll.insertMany(docs);
print('RESET ' + coll.countDocuments({}));
""")


def prove():
    if not BIN.is_file():
        raise SystemExit("c1 binary missing")
    reset_seed()
    if CONTROL.exists():
        for child in CONTROL.iterdir():
            if child.is_file():
                child.unlink()
    else:
        CONTROL.mkdir(parents=True)
    config = {
        "host": RS,
        "port": 27017,
        "database": "oo_w141",
        "collection": "shipments",
        "username": "connection_reader",
        "auth_source": "admin",
        "password_file": str(ROOT / "reader.password"),
        "replica_set": "rs0",
        "control_dir": str(CONTROL),
        "dba_file": str(ROOT / "c1-dba.json"),
    }
    (CONTROL / "config.json").write_text(json.dumps(config))
    log = open(CONTROL / "reader.log", "w")
    code = 1
    proc = subprocess.Popen(
        ["docker", "exec", "-i", RUNTIME, str(BIN), "--c1-run", "--config", str(CONTROL / "config.json")],
        stdout=log,
        stderr=subprocess.STDOUT,
    )
    try:
        wait_flag("snapshot-open", 60)
        dml("c1-race.js", """
const fs = require('fs');
const pwd = fs.readFileSync('/private/fixture-admin.password', 'utf8').trim();
if (!db.getSiblingDB('admin').auth('connection_fixture_admin', pwd)) quit(2);
const coll = db.getSiblingDB('oo_w141').shipments;
coll.insertOne({
  _id: 'c1-race', code: 'C1-RACE', flag: 7,
  units: NumberLong('9007199254744111'), price: Decimal128('12.50'),
  created: ISODate('2026-10-14T00:00:00.000Z'),
  uid: UUID('6ba7b810-9dad-11d1-80b4-00c04fd430c8'),
  blob: BinData(0, 'aGVsbG8='), weight: 1.25, warehouse: 'c1', whCode: 'C1',
  lane: 'race', memo: null, marks: ['c1'], pack: {n: 1}, secretNote: 'hide-me'
});
print('RACE');
""")
        (CONTROL / "race-written").write_text("1\n")
        wait_flag("ready", 60)
        applied = json.loads((CONTROL / "applied.json").read_text())
        keys = [row["key"] for row in applied["rows"]]
        originals = [key for key in keys if key not in {"c1-race"}]
        if len(originals) < 3:
            raise SystemExit("snapshot rows missing")
        left, right, victim = originals[0], originals[1], originals[2]
        dml("c1-txn.js", f"""
const fs = require('fs');
const pwd = fs.readFileSync('/private/fixture-admin.password', 'utf8').trim();
const mongo = new Mongo('mongodb://127.0.0.1:27017/admin');
const admin = mongo.getDB('admin');
if (!admin.auth('connection_fixture_admin', pwd)) quit(2);
const session = mongo.startSession();
session.startTransaction();
const coll = session.getDatabase('oo_w141').shipments;
coll.updateOne({{_id: ObjectId('{left}')}}, {{$set: {{lane: 'txn-a'}}}});
coll.updateOne({{_id: ObjectId('{right}')}}, {{$set: {{lane: 'txn-b'}}}});
session.commitTransaction();
session.endSession();
print('TXN');
""")
        dml("c1-iud.js", f"""
const fs = require('fs');
const pwd = fs.readFileSync('/private/fixture-admin.password', 'utf8').trim();
if (!db.getSiblingDB('admin').auth('connection_fixture_admin', pwd)) quit(2);
const coll = db.getSiblingDB('oo_w141').shipments;
coll.insertOne({{_id: 'c1-miss', code: 'C1-MISS', flag: 1, units: NumberLong('3'), price: Decimal128('1'), created: ISODate('2026-10-14T00:00:01.000Z'), weight: 1, warehouse: 'c1', whCode: 'C1', lane: 'miss'}});
coll.updateOne({{_id: 'c1-race'}}, {{$set: {{lane: 'raced'}}}});
coll.deleteOne({{_id: ObjectId('{victim}')}});
print('IUD');
""")
        deadline = time.time() + 40
        while time.time() < deadline:
            body = json.loads((CONTROL / "applied.json").read_text())
            have = {row["key"] for row in body["rows"]}
            if "c1-miss" in have and victim not in have and body.get("pre_images", 0) >= 1:
                break
            time.sleep(0.4)
        else:
            raise SystemExit("stream did not apply the post-barrier changes")
        (CONTROL / "pause").write_text("1\n")
        wait_flag("paused", 30)
        dml("c1-gap.js", """
const fs = require('fs');
const pwd = fs.readFileSync('/private/fixture-admin.password', 'utf8').trim();
if (!db.getSiblingDB('admin').auth('connection_fixture_admin', pwd)) quit(2);
db.getSiblingDB('oo_w141').shipments.insertOne({_id: 'c1-gap', code: 'C1-GAP', flag: 1, units: NumberLong('4'), price: Decimal128('1'), created: ISODate('2026-10-14T00:00:02.000Z'), weight: 1, warehouse: 'c1', whCode: 'C1', lane: 'gap'});
print('GAP');
""")
        (CONTROL / "resume").write_text("1\n")
        wait_flag("resumed", 30)
        deadline = time.time() + 30
        while time.time() < deadline:
            body = json.loads((CONTROL / "applied.json").read_text())
            have = {row["key"] for row in body["rows"]}
            if "c1-gap" in have:
                break
            time.sleep(0.4)
        else:
            raise SystemExit("resume missed the gap insert")
        dml("c1-gap-del.js", """
const fs = require('fs');
const pwd = fs.readFileSync('/private/fixture-admin.password', 'utf8').trim();
if (!db.getSiblingDB('admin').auth('connection_fixture_admin', pwd)) quit(2);
db.getSiblingDB('oo_w141').shipments.deleteOne({_id: 'c1-gap'});
print('GAPDEL');
""")
        deadline = time.time() + 30
        while time.time() < deadline:
            body = json.loads((CONTROL / "applied.json").read_text())
            have = {row["key"] for row in body["rows"]}
            if "c1-gap" not in have:
                break
            time.sleep(0.4)
        else:
            raise SystemExit("resume delete missed")
        (CONTROL / "quiesce").write_text("1\n")
        wait_flag("quiesced", 30)
        dml(
            "c1-roll.js",
            """
const fs = require('fs');
const crypto = require('crypto');
const pwd = fs.readFileSync('/private/fixture-admin.password', 'utf8').trim();
if (!db.getSiblingDB('admin').auth('connection_fixture_admin', pwd)) quit(2);
const token = JSON.parse(fs.readFileSync('/private/c1-run/l0.json', 'utf8')).resume_token;
const pad = db.getSiblingDB('oo_w141').c1_pad;
pad.drop();
function historyLost() {
  let cursor;
  try {
    cursor = db.getSiblingDB('oo_w141').shipments.watch([], {resumeAfter: token, maxAwaitTimeMS: 400});
    cursor.tryNext();
    return false;
  } catch (err) {
    return err.code === 286 || err.code === 136;
  } finally {
    try { if (cursor) cursor.close(); } catch (ignore) {}
  }
}
const deadline = Date.now() + 400000;
let n = 0;
while (!historyLost()) {
  if (Date.now() > deadline) {
    print('ROLL_FAIL n=' + n);
    pad.drop();
    quit(6);
  }
  if (n < 20) {
    pad.insertOne({_id: n, blob: Binary(crypto.randomBytes(700 * 1024))});
    n += 1;
  }
  sleep(2000);
}
pad.drop();
print('ROLLED n=' + n);
""",
            timeout=420,
        )
        (CONTROL / "expire").write_text("1\n")
        deadline = time.time() + 30
        while time.time() < deadline and not (CONTROL / "expire.json").is_file():
            time.sleep(0.2)
        if not (CONTROL / "expire.json").is_file():
            raise SystemExit("expire result missing")
        (CONTROL / "stop").write_text("1\n")
        code = proc.wait(timeout=30)
    finally:
        if proc.poll() is None:
            (CONTROL / "stop").write_text("1\n")
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                proc.kill()
    if code != 0:
        raise SystemExit(f"c1 reader exited {code}")
    expire = json.loads((CONTROL / "expire.json").read_text())
    if expire.get("status") != "POSITION_EXPIRED" or expire.get("continued") is not False:
        raise SystemExit("POSITION_EXPIRED was not proven")
    applied = json.loads((CONTROL / "applied.json").read_text())
    live = {key_of(doc["_id"]): doc for doc in admin_docs()}
    got = {row["key"]: row["doc"] for row in applied["rows"]}
    diffs = []
    for key in sorted(set(live) | set(got)):
        if live.get(key) != got.get(key):
            diffs.append(key)
    receipt = json.loads((CONTROL / "receipt.json").read_text())
    summary = {
        "diffs": diffs,
        "grade": applied.get("grade"),
        "rows": len(got),
        "hidden": applied.get("hidden_fields"),
        "null_memo": applied.get("null_memo"),
        "missing_memo": applied.get("missing_memo"),
        "pre_images": applied.get("pre_images"),
        "post_images": applied.get("post_images"),
        "multi_commits": applied.get("multi_commits"),
        "reader_writes": receipt.get("reader_write_commands"),
        "expire": expire.get("status"),
        "apply_error": receipt.get("apply_gap", {}).get("error"),
        "model_called": receipt.get("model_called"),
        "import_invoked": receipt.get("import_invoked"),
    }
    (CONTROL / "gold.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps(summary))
    if diffs or applied.get("grade") not in {"T", "S0"}:
        raise SystemExit("gold failed")
    if not applied.get("null_memo") or not applied.get("missing_memo"):
        raise SystemExit("null and missing memo were not both observed")
    if "secretNote" not in (applied.get("hidden_fields") or []):
        raise SystemExit("new field was not hidden")
    if receipt.get("reader_write_commands"):
        raise SystemExit("reader wrote")
    if applied.get("pre_images", 0) < 1 or applied.get("post_images", 0) < 1:
        raise SystemExit("pre/post image missing")


if __name__ == "__main__":
    if len(sys.argv) != 2 or sys.argv[1] not in {"up", "prove"}:
        raise SystemExit("usage: c1_fixture.py up|prove")
    up() if sys.argv[1] == "up" else prove()
