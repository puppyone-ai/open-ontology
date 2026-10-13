// Second read-only export. The first file failed because mongosh has no BSON global.
const fs = require("fs");

function fail(message) {
  print("FAIL " + message);
  quit(2);
}

let api = null;
if (typeof BSON !== "undefined" && typeof BSON.serialize === "function") api = BSON;
if (!api && typeof require === "function") {
  try {
    const loaded = require("bson");
    if (loaded && typeof loaded.serialize === "function") api = loaded;
    else if (loaded && loaded.BSON && typeof loaded.BSON.serialize === "function") api = loaded.BSON;
  } catch (err) {
    print("REQUIRE " + String(err && err.message ? err.message : err));
  }
}
if (!api) fail("bson");

const password = fs.readFileSync("/run/secrets/reader", "utf8").trim();
const admin = db.getSiblingDB("admin");
if (!admin.auth("oo_dl01w1_41_reader", password)) fail("auth");
const data = db.getSiblingDB("oo_w141");
const found = data.runCommand({
  find: "shipments",
  filter: {},
  sort: { _id: 1 },
  limit: 32,
  batchSize: 32,
  comment: "oo-dl01w1-41:docseal:reader:export-2",
});
if (!found || found.ok !== 1) fail("find");
const docs = found.cursor.firstBatch || [];
if (found.cursor.id && found.cursor.id.toString() !== "0") fail("cursor");
print("COUNT " + docs.length);
docs.forEach((doc) => {
  print("ROW " + doc._id.toHexString() + " " + Object.keys(doc).join(","));
  print(Buffer.from(api.serialize(doc)).toString("hex"));
});
print("READER_DML 0");
quit(0);
