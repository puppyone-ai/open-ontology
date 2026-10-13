// C5 capability probe and reads. Passwords stay in the secret files.
const fs = require("fs");

const mode = process.env.C5_MODE || "";
const out = process.env.C5_OUT || "";

function fail(message) {
  print("C5_FAIL " + message);
  quit(3);
}

function auth(user, secretName) {
  const pw = fs.readFileSync("/run/secrets/" + secretName, "utf8").trim();
  const admin = db.getSiblingDB("admin");
  try {
    admin.auth(user, pw);
  } catch (err) {
    print("C5_AUTH code=" + err.code);
    quit(2);
  }
  return admin;
}

const APPROVED = [
  "_id", "code", "flag", "units", "price", "created", "uid", "blob", "blob_presence",
  "weight", "warehouse", "whCode", "lane", "memo", "memo_presence", "marks",
  "marks_presence", "pack", "pack_presence",
];

function presenceBlob(value) {
  if (value === undefined) return 0;
  if (value === null) return 1;
  const size = value.length ? value.length() : 0;
  return size === 0 ? 2 : 3;
}

function presenceValue(value, empty) {
  if (value === undefined) return 0;
  if (value === null) return 1;
  return empty ? 2 : 3;
}

function scalar(value) {
  if (value === undefined || value === null) return "";
  if (value._bsontype === "ObjectId" || value._bsontype === "Long" || value._bsontype === "Decimal128" || value._bsontype === "Int32") {
    return value.toString();
  }
  if (value instanceof Date) return String(value.getTime());
  if (value._bsontype === "UUID") return value.toString();
  if (value._bsontype === "Binary") return Buffer.from(value.buffer).toString("base64");
  return String(value);
}

function canonical(doc) {
  const marks = doc.marks;
  const pack = doc.pack;
  const lines = [
    "_id=" + scalar(doc._id),
    "code=" + scalar(doc.code),
    "flag=" + scalar(doc.flag),
    "units=" + scalar(doc.units),
    "price=" + scalar(doc.price),
    "created=" + scalar(doc.created),
    "uid=" + scalar(doc.uid),
    "blob=" + (doc.blob && doc.blob._bsontype === "Binary" ? Buffer.from(doc.blob.buffer).toString("base64") : ""),
    "blob_presence=" + presenceBlob(doc.blob),
    "weight=" + scalar(doc.weight),
    "warehouse=" + scalar(doc.warehouse),
    "whCode=" + scalar(doc.whCode),
    "lane=" + scalar(doc.lane),
    "memo=" + (doc.memo == null ? "" : String(doc.memo)),
    "memo_presence=" + presenceValue(doc.memo, doc.memo === ""),
    "marks=" + (marks == null ? "" : JSON.stringify(marks)),
    "marks_presence=" + presenceValue(marks, Array.isArray(marks) && marks.length === 0),
    "pack=" + (pack == null ? "" : JSON.stringify({ kind: pack.kind == null ? null : pack.kind, n: pack.n == null ? null : scalar(pack.n) })),
    "pack_presence=" + presenceValue(pack, pack && Object.keys(pack).length === 0),
  ];
  return lines.join("\n");
}

function readShipments(comment) {
  const projection = {};
  for (let i = 0; i < APPROVED.length; i += 1) projection[APPROVED[i]] = 1;
  const data = db.getSiblingDB("oo_w141");
  const found = data.runCommand({
    find: "shipments",
    projection: projection,
    sort: { _id: 1 },
    batchSize: 32,
    comment: comment,
  });
  if (!found || found.ok !== 1 || !found.cursor) fail("find");
  const docs = found.cursor.firstBatch || [];
  if (found.cursor.id && found.cursor.id.toString() !== "0") fail("cursor_left_open");
  const rows = docs.map((doc) => ({
    key: scalar(doc._id),
    canonical: canonical(doc),
  }));
  return rows;
}

function writeJson(body) {
  if (!out) fail("out");
  fs.writeFileSync(out, JSON.stringify(body));
}

if (mode === "cdc") {
  const admin = auth("oo_dl01w1_41_reader", "reader");
  const hello = admin.runCommand({ hello: 1, comment: "oo-dl01w1-41:c5:reader:hello" });
  if (!hello || hello.ok !== 1) fail("hello");
  const who = admin.runCommand({ connectionStatus: 1, comment: "oo-dl01w1-41:c5:reader:who" });
  const users = ((who.authInfo && who.authInfo.authenticatedUsers) || []).map((user) => ({
    user: user.user,
    db: user.db,
  }));
  const data = db.getSiblingDB("oo_w141");
  let code = null;
  let codeName = null;
  let ok = null;
  try {
    const watched = data.runCommand({
      aggregate: "shipments",
      pipeline: [{ $changeStream: {} }],
      cursor: {},
      comment: "oo-dl01w1-41:c5:reader:cdc-probe",
    });
    ok = watched && watched.ok;
    if (watched && watched.ok === 1 && watched.cursor && watched.cursor.id) {
      data.runCommand({
        killCursors: "shipments",
        cursors: [watched.cursor.id],
        comment: "oo-dl01w1-41:c5:reader:cdc-kill",
      });
      fail("change_stream_opened");
    }
    code = watched && watched.code;
    codeName = watched && watched.codeName;
  } catch (err) {
    ok = 0;
    code = err.code;
    codeName = err.codeName;
  }
  writeJson({
    ok: ok,
    setName: hello.setName || null,
    setVersion: hello.setVersion || null,
    topology: hello.setName ? "replica_or_sharded" : "standalone",
    code: code,
    codeName: codeName,
    users: users,
    find_sent: false,
  });
  if (hello.setName || ok === 1) fail("cdc_not_refused");
  quit(0);
}

if (mode === "read") {
  auth("oo_dl01w1_41_reader", "reader");
  const rows = readShipments(process.env.C5_COMMENT || "oo-dl01w1-41:c5:reader:read");
  writeJson({ rows: rows, count: rows.length, find_sent_unapproved: false });
  quit(0);
}

if (mode === "keys") {
  auth("oo_dl01w1_41_reader", "reader");
  const data = db.getSiblingDB("oo_w141");
  const found = data.runCommand({
    aggregate: "shipments",
    pipeline: [{
      $project: {
        pairs: {
          $map: {
            input: { $objectToArray: "$$ROOT" },
            as: "pair",
            in: { k: "$$pair.k", t: { $type: "$$pair.v" } },
          },
        },
      },
    }],
    cursor: { batchSize: 32 },
    comment: "oo-dl01w1-41:c5:reader:keys",
  });
  if (!found || found.ok !== 1) fail("keys");
  const docs = found.cursor.firstBatch || [];
  if (found.cursor.id && found.cursor.id.toString() !== "0") fail("keys_cursor");
  const types = {};
  docs.forEach((doc) => {
    (doc.pairs || []).forEach((pair) => {
      if (!types[pair.k]) types[pair.k] = {};
      types[pair.k][pair.t] = true;
    });
  });
  const fieldTypes = {};
  Object.keys(types).sort().forEach((field) => {
    fieldTypes[field] = Object.keys(types[field]).sort();
  });
  writeJson({ fields: Object.keys(fieldTypes), field_types: fieldTypes, values_retained: false });
  quit(0);
}

if (mode === "dba-a") {
  auth("oo_dl01w1_41_dba", "dba");
  const data = db.getSiblingDB("oo_w141");
  const updated = data.runCommand({
    update: "shipments",
    updates: [{
      q: { _id: ObjectId("c141d04141eef04100000000") },
      u: { $set: { memo: "memo-c5-period", hiddenNote: "not-approved-field" } },
    }],
    comment: "oo-dl01w1-41:c5:dba:update",
  });
  if (!updated || updated.ok !== 1 || updated.nModified !== 1) fail("update");
  const removed = data.runCommand({
    delete: "shipments",
    deletes: [{ q: { _id: ObjectId("c141d04141eef0410000000f") }, limit: 1 }],
    comment: "oo-dl01w1-41:c5:dba:delete",
  });
  if (!removed || removed.ok !== 1 || removed.n !== 1) fail("delete");
  writeJson({ updated: updated.nModified, deleted: removed.n });
  quit(0);
}

if (mode === "dba-b") {
  auth("oo_dl01w1_41_dba", "dba");
  const data = db.getSiblingDB("oo_w141");
  const inserted = data.runCommand({
    insert: "shipments",
    documents: [{
      _id: ObjectId("c141d04141eef04100000010"),
      code: "S41-C5",
      flag: Int32(216),
      units: Long("9007199254744117"),
      price: Decimal128.fromString("314159265358995.0000041"),
      created: new Date(1725000016000),
      uid: new UUID("11111111-2222-4000-8000-000000000010"),
      blob: Binary.createFromBase64(Buffer.from([16, 0x41, 0x00]).toString("base64"), 0),
      weight: 57.25,
      warehouse: "wh-west",
      whCode: "W41",
      lane: Long("4116"),
      memo: "memo-new",
      marks: ["m", "new"],
      pack: { kind: "crate", n: Int32(16) },
      hiddenNote: Int32(7),
    }],
    comment: "oo-dl01w1-41:c5:dba:insert",
  });
  if (!inserted || inserted.ok !== 1 || inserted.n !== 1) fail("insert");
  const shipments = data.runCommand({ count: "shipments", comment: "oo-dl01w1-41:c5:dba:count" });
  const odd = data.runCommand({ count: "price_odd", comment: "oo-dl01w1-41:c5:dba:count-odd" });
  const mixed = data.runCommand({ count: "mixed_marks", comment: "oo-dl01w1-41:c5:dba:count-mixed" });
  writeJson({
    inserted: inserted.n,
    shipments: shipments.n,
    price_odd: odd.n,
    mixed_marks: mixed.n,
  });
  quit(0);
}

if (mode === "dba-audit") {
  auth("oo_dl01w1_41_dba", "dba");
  const data = db.getSiblingDB("oo_w141");
  const admin = db.getSiblingDB("admin");
  const profileData = data.runCommand({ profile: 2, comment: "oo-dl01w1-41:c5:dba:profile-on" });
  const profileAdmin = admin.runCommand({ profile: 2, comment: "oo-dl01w1-41:c5:dba:profile-on-admin" });
  if (!profileData || profileData.ok !== 1 || !profileAdmin || profileAdmin.ok !== 1) fail("profile");
  const inserted = data.runCommand({
    insert: "shipments",
    documents: [{
      _id: ObjectId("c141d04141eef04100000011"),
      code: "S41-AUD",
      flag: Int32(217),
      units: Long("9007199254744118"),
      price: Decimal128.fromString("314159265358996.0000041"),
      created: new Date(1725000017000),
      uid: new UUID("11111111-2222-4000-8000-000000000011"),
      blob: Binary.createFromBase64(Buffer.from([17, 0x41, 0x00]).toString("base64"), 0),
      weight: 58.25,
      warehouse: "wh-west",
      whCode: "W41",
      lane: "lane-audit",
      memo: "memo-audit",
      marks: ["m", "audit"],
      pack: { kind: "crate", n: Int32(17) },
    }],
    comment: "oo-dl01w1-41:c5:dba:audit-insert",
  });
  if (!inserted || inserted.ok !== 1 || inserted.n !== 1) fail("audit_insert");
  const removed = data.runCommand({
    delete: "shipments",
    deletes: [{ q: { _id: ObjectId("c141d04141eef04100000011") }, limit: 1 }],
    comment: "oo-dl01w1-41:c5:dba:audit-delete",
  });
  if (!removed || removed.ok !== 1 || removed.n !== 1) fail("audit_delete");
  const updated = data.runCommand({
    update: "shipments",
    updates: [{
      q: { _id: ObjectId("c141d04141eef04100000000") },
      u: { $set: { memo: "memo-c5-period" } },
    }],
    comment: "oo-dl01w1-41:c5:dba:audit-update",
  });
  if (!updated || updated.ok !== 1) fail("audit_update");
  writeJson({
    profile: profileData.was,
    inserted: inserted.n,
    deleted: removed.n,
    updated: updated.n,
    nModified: updated.nModified,
  });
  quit(0);
}

if (mode === "audit") {
  auth("oo_dl01w1_41_audit", "audit");
  function commandName(cmd, op) {
    const names = ["find", "aggregate", "insert", "update", "delete", "findAndModify", "hello", "connectionStatus", "count", "killCursors", "getMore", "create", "drop", "createIndexes", "collMod", "dropDatabase", "profile"];
    for (let i = 0; i < names.length; i += 1) {
      if (Object.prototype.hasOwnProperty.call(cmd, names[i])) return names[i];
    }
    if (Object.prototype.hasOwnProperty.call(cmd, "q") && Object.prototype.hasOwnProperty.call(cmd, "u")) return "update";
    if (Object.prototype.hasOwnProperty.call(cmd, "q") && Object.prototype.hasOwnProperty.call(cmd, "limit")) return "delete";
    if (op === "update") return "update";
    if (op === "remove") return "delete";
    if (op === "insert") return "insert";
    const keys = Object.keys(cmd);
    return keys.length ? keys[0] : (op || "");
  }
  function pullNamed(databaseName) {
    const database = db.getSiblingDB(databaseName);
    const found = database.runCommand({
      find: "system.profile",
      filter: { "command.comment": { $regex: "^oo-dl01w1-41:c5:" } },
      batchSize: 400,
      comment: "oo-dl01w1-41:c5:audit:profile-named",
    });
    if (!found || found.ok !== 1) return [];
    const batch = (found.cursor && found.cursor.firstBatch) || [];
    return batch.map((row) => ({
      ns: row.ns || null,
      user: row.user || null,
      appName: row.appName || null,
      op: row.op || null,
      command: commandName(row.command || {}, row.op || ""),
      comment: row.command && row.command.comment || null,
    }));
  }
  const rows = pullNamed("oo_w141").concat(pullNamed("admin"));
  const dml = ["insert", "update", "delete", "remove", "findAndModify", "create", "drop", "createIndexes", "collMod", "dropDatabase"];
  const readerDml = rows.filter((row) => String(row.user || "").indexOf("oo_dl01w1_41_reader") === 0 && (dml.indexOf(row.command) !== -1 || dml.indexOf(row.op) !== -1));
  writeJson({ rows: rows, reader_dml: readerDml.length });
  if (readerDml.length !== 0) fail("reader_dml");
  quit(0);
}

fail("mode");
