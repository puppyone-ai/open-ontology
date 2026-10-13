// Reader capture of the approved shipment projection.
// Inclusion projection only. No insert, update, delete, or unapproved find.
const fs = require("fs");

const out = "/tmp/oo-w141-capture";
fs.mkdirSync(out, { recursive: true });

function fail(message) {
  fs.writeFileSync(out + "/status.txt", "FAILED " + message + "\n");
  quit(1);
}

function writeJson(name, value) {
  fs.writeFileSync(out + "/" + name, JSON.stringify(value, null, 2) + "\n");
}

try {
captureBody();
} catch (err) {
  fs.writeFileSync(out + "/status.txt", "FAILED " + String(err && err.message ? err.message : err) + "\n");
  quit(1);
}

function captureBody() {
const policy = JSON.parse(fs.readFileSync(out + "/customer.json", "utf8"));
const stored = [
  "_id", "code", "flag", "units", "price", "created", "uid", "blob",
  "weight", "warehouse", "whCode", "lane", "memo", "marks", "pack",
];
if (policy.protocol !== "oo.source-customer-input/v1") fail("protocol");
if (policy.secret_ref !== "secret://oo-dl01w1-41/reader") fail("secret_ref");
if (policy.namespace !== "oo_w141") fail("namespace");
if (policy.role !== "read") fail("role");
if (policy.permission !== "read") fail("permission");
if (policy.budget_unit !== "Rows" || policy.budget_limit !== 16) fail("budget");
if (!Array.isArray(policy.approved_objects) || policy.approved_objects.length !== 1 || policy.approved_objects[0] !== "oo_w141.shipments") {
  fail("approved_objects");
}
for (const name of stored) {
  if (!policy.approved_fields.includes(name)) fail("approved_fields " + name);
}
if (policy.deny_object !== "oo_w141.not_approved") fail("deny_object");
if (!policy.deny_fields.includes("notApproved")) fail("deny_fields");

writeJson("pre-read-deny.json", {
  password_opened: false,
  find_sent: false,
  denied_object: policy.deny_object,
  denied_fields: policy.deny_fields,
  reason: "unapproved object and field refused before the password file and before find",
});

const password = fs.readFileSync("/run/secrets/reader", "utf8").trim();
const admin = db.getSiblingDB("admin");
if (!admin.auth("oo_dl01w1_41_reader", password)) fail("auth");

const data = db.getSiblingDB("oo_w141");
const projection = {};
for (const name of stored) projection[name] = 1;
const found = data.runCommand({
  find: "shipments",
  filter: {},
  projection: projection,
  sort: { _id: 1 },
  limit: 16,
  batchSize: 16,
  comment: "oo-dl01w1-41:reader:fulldoc",
});
if (!found || found.ok !== 1) fail("find");
const docs = found.cursor.firstBatch;
if (!Array.isArray(docs) || docs.length !== 16) fail("rows " + (docs ? docs.length : 0));
if (found.cursor.id && found.cursor.id !== 0 && found.cursor.id !== NumberLong("0")) {
  const killed = data.runCommand({
    killCursors: "shipments",
    cursors: [found.cursor.id],
    comment: "oo-dl01w1-41:reader:fulldoc-kill",
  });
  if (!killed || killed.ok !== 1) fail("killCursors");
}

const counted = data.runCommand({
  count: "shipments",
  comment: "oo-dl01w1-41:reader:count",
});
if (!counted || counted.ok !== 1) fail("count");

if (typeof BSON === "undefined" || typeof BSON.serialize !== "function") fail("bson serializer");

const approved = policy.approved_fields;
const rows = [];
const fullHex = [];
const sealHex = [];
for (let index = 0; index < docs.length; index += 1) {
  const doc = docs[index];
  const keys = Object.keys(doc);
  for (const key of keys) {
    if (approved.indexOf(key) < 0) fail("captured key outside approval " + key);
  }
  const weightType = doc.weight === undefined ? "absent" : doc.weight._bsontype || typeof doc.weight;
  const full = BSON.serialize(doc);
  const sealDoc = {};
  for (const key of keys) {
    if (key !== "weight") sealDoc[key] = doc[key];
  }
  const sealed = BSON.serialize(sealDoc);
  fullHex.push(Buffer.from(full).toString("hex"));
  sealHex.push(Buffer.from(sealed).toString("hex"));
  rows.push({
    index: index,
    keys: keys,
    weight_bson_type: weightType,
    full_bson_bytes: full.length,
    seal_bson_bytes: sealed.length,
    weight_in_seal_bson: false,
    seal_omission: "weight",
    seal_omission_reason: "fixed BSON decoder rejects type 0x01 double",
  });
}

fs.writeFileSync(out + "/full-approved.bson.hex", fullHex.join("\n") + "\n");
fs.writeFileSync(out + "/seal-scope.bson.hex", sealHex.join("\n") + "\n");
writeJson("capture.json", {
  protocol: "oo.mongodb-w141.fulldoc-capture/v1",
  role: "read",
  namespace: "oo_w141",
  object: "oo_w141.shipments",
  projection: "inclusion",
  projected_fields: stored,
  source_count: counted.n,
  captured_rows: docs.length,
  write_attempts: 0,
  commands: ["find", "killCursors", "count"],
  hidden_key_in_capture: false,
  weight_in_full_bson: true,
  weight_in_seal_bson: false,
  rows: rows,
});
fs.writeFileSync(out + "/status.txt", "DONE\n");
}
