// DL01-W1-41 fixture. Identity split and profiler level 2 follow
// testing/dl01-targets/mongodb/scripts/dba-setup.js at b24610ca75a7f9aa5d245a171aac1fa681409c55.
// Database, collection, account, and document values belong to oo-dl01w1-41.
const fs = require("fs");

const dbaPw = fs.readFileSync("/run/secrets/dba", "utf8").trim();
const readerPw = fs.readFileSync("/run/secrets/reader", "utf8").trim();
const auditPw = fs.readFileSync("/run/secrets/audit", "utf8").trim();

function fail(message) {
  print("DBA_FAIL " + message);
  quit(2);
}

function commandError(err) {
  return " code=" + err.code + " name=" + err.codeName;
}

const admin = db.getSiblingDB("admin");

function dbaSession() {
  const who = admin.runCommand({ connectionStatus: 1, comment: "oo-dl01w1-41:dba:whoami" });
  return ((who.authInfo && who.authInfo.authenticatedUsers) || []).some(
    (user) => user.user === "oo_dl01w1_41_dba" && user.db === "admin",
  );
}

let authed = false;
try {
  admin.auth("oo_dl01w1_41_dba", dbaPw);
  authed = dbaSession();
} catch (err) {
  authed = false;
}
if (!authed) {
  try {
    const created = admin.runCommand({
      createUser: "oo_dl01w1_41_dba",
      pwd: dbaPw,
      roles: [{ role: "userAdminAnyDatabase", db: "admin" }],
      comment: "oo-dl01w1-41:dba:create-self",
    });
    if (created.ok !== 1) fail("create_dba");
  } catch (err) {
    fail("create_dba" + commandError(err));
  }
  try {
    admin.auth("oo_dl01w1_41_dba", dbaPw);
  } catch (err) {
    fail("auth" + commandError(err));
  }
  if (!dbaSession()) fail("auth");
}

try {
  const granted = admin.runCommand({
    grantRolesToUser: "oo_dl01w1_41_dba",
    roles: [
      { role: "readWrite", db: "oo_w141" },
      { role: "dbAdmin", db: "oo_w141" },
      { role: "dbAdmin", db: "admin" },
    ],
    comment: "oo-dl01w1-41:dba:grant-self",
  });
  if (granted.ok !== 1) fail("grant_self");
} catch (err) {
  fail("grant_self" + commandError(err));
}

try {
  const role = admin.runCommand({
    createRole: "ooW141ProfileRead",
    privileges: [
      { resource: { db: "oo_w141", collection: "system.profile" }, actions: ["find"] },
      { resource: { db: "admin", collection: "system.profile" }, actions: ["find"] },
    ],
    roles: [],
    comment: "oo-dl01w1-41:dba:create-role",
  });
  if (role.ok !== 1) fail("create_role");
} catch (err) {
  if (err.code !== 51002 && err.code !== 11000) fail("create_role" + commandError(err));
}

try {
  const reader = admin.runCommand({
    createUser: "oo_dl01w1_41_reader",
    pwd: readerPw,
    roles: [{ role: "read", db: "oo_w141" }],
    comment: "oo-dl01w1-41:dba:create-reader",
  });
  if (reader.ok !== 1) fail("create_reader");
} catch (err) {
  if (err.code !== 11000 && err.code !== 51003) fail("create_reader" + commandError(err));
}

try {
  const audit = admin.runCommand({
    createUser: "oo_dl01w1_41_audit",
    pwd: auditPw,
    roles: [{ role: "ooW141ProfileRead", db: "admin" }],
    comment: "oo-dl01w1-41:dba:create-audit",
  });
  if (audit.ok !== 1) fail("create_audit");
} catch (err) {
  if (err.code !== 11000 && err.code !== 51003) fail("create_audit" + commandError(err));
}

const data = db.getSiblingDB("oo_w141");
const profileData = data.runCommand({ profile: 2, comment: "oo-dl01w1-41:dba:profiler" });
if (profileData.ok !== 1) fail("profiler_data");
const profileAdmin = admin.runCommand({ profile: 2, comment: "oo-dl01w1-41:dba:profiler-admin" });
if (profileAdmin.ok !== 1) fail("profiler_admin");

const indexes = data.runCommand({
  createIndexes: "shipments",
  indexes: [
    {
      key: { code: 1 },
      name: "code_1",
      unique: true,
      collation: { locale: "en", strength: 2 },
    },
  ],
  comment: "oo-dl01w1-41:dba:index",
});
if (indexes.ok !== 1) fail("create_index");

function shipment(i) {
  const hex = i.toString(16).padStart(8, "0");
  const whole = (314159265358979n + BigInt(i)).toString();
  const doc = {
    _id: ObjectId("c141d04141eef041" + hex),
    code: "S41-" + String(i).padStart(3, "0"),
    flag: Int32(100 + i),
    units: Long((9007199254744101n + BigInt(i)).toString()),
    price: Decimal128.fromString(whole + ".0000041"),
    created: new Date(1725000000000 + i * 1000),
    uid: new UUID("11111111-2222-4000-8000-" + String(i).padStart(12, "0")),
    blob: i === 8
      ? new Binary(Buffer.alloc(0), 0)
      : Binary.createFromBase64(Buffer.from([i, 0x41, 0x00]).toString("base64"), 0),
    weight: 41.25 + i,
    warehouse: "wh-west",
    whCode: "W41",
    lane: i % 5 === 4 ? Long(String(4100 + i)) : "lane-" + i,
  };
  if (i === 1) doc.memo = null;
  else if (i === 2) doc.memo = "";
  else if (i !== 0) doc.memo = "memo-" + i;
  if (i === 4) doc.marks = [];
  else if (i !== 3) doc.marks = ["m", String(i)];
  if (i === 6) doc.pack = null;
  else if (i === 7) doc.pack = {};
  else if (i !== 5) doc.pack = { kind: "crate", n: Int32(i) };
  return doc;
}

const shipments = [];
for (let i = 0; i < 16; i += 1) shipments.push(shipment(i));

function insertAll(database, name, documents) {
  const result = database.runCommand({
    insert: name,
    documents: documents,
    ordered: true,
    comment: "oo-dl01w1-41:dba:load",
  });
  if (result.ok !== 1 || result.n !== documents.length) fail("insert " + name);
  return result.n;
}

const insertedShipments = insertAll(data, "shipments", shipments);
const insertedOdd = insertAll(data, "price_odd", [
  { _id: ObjectId("c141d04141eef04200000001"), code: "ODD-NAN", price: Decimal128.fromString("NaN") },
  { _id: ObjectId("c141d04141eef04200000002"), code: "ODD-INF", price: Decimal128.fromString("Infinity") },
  { _id: ObjectId("c141d04141eef04200000003"), code: "ODD-NINF", price: Decimal128.fromString("-Infinity") },
]);
const insertedMixed = insertAll(data, "mixed_marks", [
  { _id: ObjectId("c141d04141eef04300000001"), code: "MIX-1", marks: ["a", Long("7")] },
  { _id: ObjectId("c141d04141eef04300000002"), code: "MIX-2", marks: [Int32(3), "b"] },
]);

fs.mkdirSync("/tmp/oo-dl01w1-41-out", { recursive: true });
fs.writeFileSync(
  "/tmp/oo-dl01w1-41-out/dba-result.json",
  JSON.stringify({
    ok: true,
    database: "oo_w141",
    shipments: insertedShipments,
    price_odd: insertedOdd,
    mixed_marks: insertedMixed,
    users: ["oo_dl01w1_41_dba", "oo_dl01w1_41_reader", "oo_dl01w1_41_audit"],
    role: "ooW141ProfileRead",
    profiler: { oo_w141: 2, admin: 2 },
    index: "code_1",
  }),
);
print("DBA_OK shipments=" + insertedShipments);
quit(0);
