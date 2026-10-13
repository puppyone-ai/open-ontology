// DBA setup identity only. Reads passwords from mounted files. Prints no secrets.
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

// Localhost bypass grants createUser and grantRole on admin only.
// Once any user exists, an unauthenticated createUser is Unauthorized rather than
// a duplicate-key error, so an existing DBA authenticates before bootstrap.
const admin = db.getSiblingDB("admin");

function dbaSession() {
  const who = admin.runCommand({ connectionStatus: 1, comment: "oo-dl01w1-14:dba:whoami" });
  return ((who.authInfo && who.authInfo.authenticatedUsers) || []).some(
    (user) => user.user === "oo_dl01w1_14_dba" && user.db === "admin",
  );
}

let authed = false;
try {
  admin.auth("oo_dl01w1_14_dba", dbaPw);
  authed = dbaSession();
} catch (err) {
  authed = false;
}
if (!authed) {
  try {
    const created = admin.runCommand({
      createUser: "oo_dl01w1_14_dba",
      pwd: dbaPw,
      roles: [{ role: "userAdminAnyDatabase", db: "admin" }],
      comment: "oo-dl01w1-14:dba:create-self",
    });
    if (created.ok !== 1) fail("create_dba");
  } catch (err) {
    fail("create_dba" + commandError(err));
  }
  try {
    admin.auth("oo_dl01w1_14_dba", dbaPw);
  } catch (err) {
    fail("auth" + commandError(err));
  }
  if (!dbaSession()) fail("auth");
}

try {
  admin.runCommand({
    grantRolesToUser: "oo_dl01w1_14_dba",
    roles: [
      { role: "readWrite", db: "oo_synth" },
      { role: "dbAdmin", db: "oo_synth" },
    ],
    comment: "oo-dl01w1-14:dba:grant-self",
  });
} catch (err) {
  fail("grant_self" + commandError(err));
}

const synth = db.getSiblingDB("oo_synth");
// Keep user-management commands out of the profiler. Level 2 starts after this.
try {
  // slowms is process-global in 8.0.32 and needs a privilege beyond database dbAdmin.
  // The server default is already 100ms, so level 2 alone is the least-privilege switch.
  synth.runCommand({ profile: 0, comment: "oo-dl01w1-14:dba:profiler-off" });
} catch (err) {
  if (err.code !== 26) fail("profiler_off" + commandError(err));
}

try {
  const role = synth.runCommand({
    createRole: "ooProfileRead",
    privileges: [
      {
        resource: { db: "oo_synth", collection: "system.profile" },
        actions: ["find"],
      },
    ],
    roles: [],
    comment: "oo-dl01w1-14:dba:create-role",
  });
  if (role.ok !== 1) fail("create_role");
} catch (err) {
  if (err.code !== 51002 && err.code !== 11000) fail("create_role" + commandError(err));
}

try {
  const reader = admin.runCommand({
    createUser: "oo_dl01w1_14_reader",
    pwd: readerPw,
    roles: [{ role: "read", db: "oo_synth" }],
    comment: "oo-dl01w1-14:dba:create-reader",
  });
  if (reader.ok !== 1) fail("create_reader");
} catch (err) {
  if (err.code !== 11000 && err.code !== 51003) fail("create_reader" + commandError(err));
}

try {
  const audit = admin.runCommand({
    createUser: "oo_dl01w1_14_audit",
    pwd: auditPw,
    roles: [{ role: "ooProfileRead", db: "oo_synth" }],
    comment: "oo-dl01w1-14:dba:create-audit",
  });
  if (audit.ok !== 1) fail("create_audit");
} catch (err) {
  if (err.code !== 11000 && err.code !== 51003) fail("create_audit" + commandError(err));
}

try {
  synth.runCommand({
    drop: "orders",
    comment: "oo-dl01w1-14:dba:reset-fixture",
  });
} catch (err) {
  if (err.code !== 26) fail("reset_fixture" + commandError(err));
}

const indexes = synth.runCommand({
  createIndexes: "orders",
  indexes: [{ key: { sku: 1 }, name: "sku_1", unique: true }],
  comment: "oo-dl01w1-14:dba:index",
});
if (indexes.ok !== 1) fail("create_index");

try {
  synth.runCommand({
    drop: "system.profile",
    comment: "oo-dl01w1-14:dba:reset-profile",
  });
} catch (err) {
  if (err.code !== 26) fail("reset_profile" + commandError(err));
}

const profile = synth.runCommand({
  profile: 2,
  comment: "oo-dl01w1-14:dba:profiler",
});
if (profile.ok !== 1) fail("profiler");

function priceFor(i) {
  if (i === 0) return Decimal128.fromString("NaN");
  if (i === 1) return Decimal128.fromString("Infinity");
  if (i === 2) return Decimal128.fromString("-Infinity");
  const whole = (10n ** 20n + BigInt(i)).toString();
  const frac = String(i).padStart(4, "0") + "0001";
  return Decimal128.fromString(whole + "." + frac);
}

function blobB64(i) {
  return Buffer.from([i & 255, 0x0a, 0x00, i & 255]).toString("base64");
}

const docs = [];
for (let i = 0; i < 100; i += 1) {
  const hex = "64b0b1c0e1d2f3a4" + i.toString(16).padStart(8, "0");
  const doc = {
    _id: ObjectId(hex),
    sku: "SKU-" + String(i).padStart(3, "0"),
    units: Long((9007199254740993n + BigInt(i)).toString()),
    small_units: Long(String(i)),
    price: priceFor(i),
    warehouse: "wh-east",
    flag: Int32(i % 5),
    created: new Date(1700000000000 + i * 1000),
    uid: new UUID("00000000-0000-4000-8000-" + String(i).padStart(12, "0")),
    blob: Binary.createFromBase64(blobB64(i), 0),
    weight: i + 0.5,
    note_null_boundary: i === 99 ? null : "n" + i,
    note_drift: i >= 98 ? null : "d" + i,
    tags: i % 2 === 0 ? ["a", "b"] : ["a", Long(String(i))],
    nested_arr: [{ sku: "SKU-" + String(i).padStart(3, "0"), q: Int32(1) }],
    payload: { kind: "box", n: Int32(i) },
    explicit_null: null,
  };
  if (i < 89) doc.rare_code = "c" + i;
  if (i < 50) doc.mixed = "m" + i;
  else doc.mixed = Long(String(i));
  if (i < 97) doc.count_drift = Long(String(i));
  else doc.count_drift = Int32(i);
  if (i >= 95) doc.new_attr = "new-" + i;
  docs.push(doc);
}

let inserted = 0;
for (let start = 0; start < docs.length; start += 10) {
  const slice = docs.slice(start, start + 10);
  const result = synth.runCommand({
    insert: "orders",
    documents: slice,
    ordered: true,
    comment: "oo-dl01w1-14:dba:load",
  });
  if (result.ok !== 1 || result.n !== slice.length) fail("insert");
  inserted += result.n;
}

fs.mkdirSync("/tmp/oo-dl01w1-14-out", { recursive: true });
fs.writeFileSync(
  "/tmp/oo-dl01w1-14-out/dba-result.json",
  JSON.stringify({
    ok: true,
    inserted: inserted,
    users: ["oo_dl01w1_14_dba", "oo_dl01w1_14_reader", "oo_dl01w1_14_audit"],
    role: "ooProfileRead",
    profiler: 2,
  }),
);
print("DBA_OK inserted=" + inserted);
quit(0);
