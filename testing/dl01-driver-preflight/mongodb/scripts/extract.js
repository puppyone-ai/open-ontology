// DBA extract of live server values. The reader probe does not see this file.
const fs = require("fs");

const dbaPw = fs.readFileSync("/run/secrets/dba", "utf8").trim();
const admin = db.getSiblingDB("admin");

function fail(message) {
  print("EXTRACT_FAIL " + message);
  quit(4);
}

try {
  admin.auth("oo_dl01w1_41_dba", dbaPw);
} catch (err) {
  fail("auth code=" + err.code);
}

function collectionFromNs(ns) {
  const dot = String(ns || "").indexOf(".");
  return dot === -1 ? String(ns || "") : String(ns).slice(dot + 1);
}

function findAll(database, command) {
  let first;
  try {
    first = database.runCommand(command);
  } catch (err) {
    return { ok: 0, code: err.code, codeName: err.codeName };
  }
  if (!first || first.ok !== 1 || !first.cursor) return first;
  const documents = (first.cursor.firstBatch || []).slice();
  let id = first.cursor.id;
  let guard = 0;
  while (id && id.toString() !== "0") {
    guard += 1;
    if (guard > 40) return { ok: 0, codeName: "ExtractLoop" };
    let more;
    try {
      more = database.runCommand({
        getMore: id,
        collection: collectionFromNs(first.cursor.ns),
        batchSize: 200,
        comment: "oo-dl01w1-41:dba:extract-more",
      });
    } catch (err) {
      return { ok: 0, code: err.code, codeName: err.codeName };
    }
    if (!more || more.ok !== 1) return more;
    documents.push.apply(documents, more.cursor.nextBatch || []);
    id = more.cursor.id;
  }
  return { ok: 1, ns: first.cursor.ns, documents: documents };
}

const data = db.getSiblingDB("oo_w141");
const shipments = findAll(data, {
  find: "shipments",
  sort: { _id: 1 },
  batchSize: 100,
  comment: "oo-dl01w1-41:dba:extract",
});
const priceOdd = findAll(data, {
  find: "price_odd",
  sort: { _id: 1 },
  batchSize: 20,
  comment: "oo-dl01w1-41:dba:extract-odd",
});
const mixedMarks = findAll(data, {
  find: "mixed_marks",
  sort: { _id: 1 },
  batchSize: 20,
  comment: "oo-dl01w1-41:dba:extract-mixed",
});
const indexes = findAll(data, {
  listIndexes: "shipments",
  comment: "oo-dl01w1-41:dba:listIndexes",
});
const collections = findAll(data, {
  listCollections: 1,
  nameOnly: false,
  comment: "oo-dl01w1-41:dba:listCollections",
});

let buildInfo;
let hello;
let cmdline;
try {
  buildInfo = admin.runCommand({ buildInfo: 1, comment: "oo-dl01w1-41:dba:buildInfo" });
} catch (err) {
  buildInfo = { ok: 0, code: err.code, codeName: err.codeName };
}
try {
  hello = admin.runCommand({ hello: 1, comment: "oo-dl01w1-41:dba:hello" });
} catch (err) {
  hello = { ok: 0, code: err.code, codeName: err.codeName };
}
try {
  cmdline = admin.runCommand({ getCmdLineOpts: 1, comment: "oo-dl01w1-41:dba:cmdline" });
} catch (err) {
  cmdline = { ok: 0, code: err.code, codeName: err.codeName };
}

if (!shipments || shipments.ok !== 1) fail("shipments");
if (!priceOdd || priceOdd.ok !== 1) fail("price_odd");
if (!mixedMarks || mixedMarks.ok !== 1) fail("mixed_marks");

const body = {
  buildInfo: buildInfo,
  hello: hello,
  getCmdLineOpts: cmdline,
  listCollections: collections,
  listIndexes: indexes,
  shipments: shipments.documents,
  price_odd: priceOdd.documents,
  mixed_marks: mixedMarks.documents,
};
fs.mkdirSync("/tmp/oo-dl01w1-41-out", { recursive: true });
fs.writeFileSync("/tmp/oo-dl01w1-41-out/canonical-ejson.json", EJSON.stringify(body, { relaxed: false }));
print("EXTRACT_OK shipments=" + shipments.documents.length);
quit(0);
