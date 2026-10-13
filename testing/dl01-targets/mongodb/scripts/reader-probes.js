// Least-privilege reader. One process: catalog, batch, cancel, then one explicit change-stream refusal.
const fs = require("fs");

const pw = fs.readFileSync("/run/secrets/reader", "utf8").trim();

function fail(message) {
  print("READER_FAIL " + message);
  quit(3);
}

function canon(value) {
  return JSON.parse(EJSON.stringify(value, { relaxed: false }));
}

function relax(value) {
  return JSON.parse(EJSON.stringify(value, { relaxed: true }));
}

function idString(value) {
  if (value === null || value === undefined) return null;
  return value.toString();
}

const admin = db.getSiblingDB("admin");
try {
  admin.auth("oo_dl01w1_14_reader", pw);
} catch (err) {
  fail("auth");
}
const who = admin.runCommand({ connectionStatus: 1 });
const authed = ((who.authInfo && who.authInfo.authenticatedUsers) || []).some(
  (user) => user.user === "oo_dl01w1_14_reader",
);
if (!authed) fail("auth");

const synth = db.getSiblingDB("oo_synth");
const outDir = "/tmp/oo-dl01w1-14-out";
fs.mkdirSync(outDir, { recursive: true });

const connectionStatus = synth.runCommand({
  connectionStatus: 1,
  showPrivileges: true,
  comment: "oo-dl01w1-14:reader:catalog:connection-status",
});
const hello = synth.runCommand({
  hello: 1,
  comment: "oo-dl01w1-14:reader:catalog:hello",
});
const listCollections = synth.runCommand({
  listCollections: 1,
  nameOnly: false,
  comment: "oo-dl01w1-14:reader:catalog:list-collections",
});
const listIndexes = synth.runCommand({
  listIndexes: "orders",
  comment: "oo-dl01w1-14:reader:catalog:list-indexes",
});
const collStats = synth.runCommand({
  collStats: "orders",
  comment: "oo-dl01w1-14:reader:catalog:coll-stats",
});
const dbStats = synth.runCommand({
  dbStats: 1,
  scale: 1,
  comment: "oo-dl01w1-14:reader:catalog:db-stats",
});

if (connectionStatus.ok !== 1 || hello.ok !== 1 || listCollections.ok !== 1) fail("catalog");
if (listIndexes.ok !== 1 || collStats.ok !== 1 || dbStats.ok !== 1) fail("catalog_stats");

const catalog = {
  connectionStatus: canon(connectionStatus),
  hello: canon(hello),
  helloHasSetName: Object.prototype.hasOwnProperty.call(hello, "setName"),
  listCollections: canon(listCollections),
  listIndexes: canon(listIndexes),
  collStats: canon(collStats),
  dbStats: canon(dbStats),
};
fs.writeFileSync(outDir + "/catalog.json", JSON.stringify(catalog));

const canonicalDocs = [];
const relaxedDocs = [];
let findRes = synth.runCommand({
  find: "orders",
  sort: { _id: 1 },
  batchSize: 20,
  comment: "oo-dl01w1-14:reader:batch:find",
});
if (findRes.ok !== 1) fail("find");
let cursorId = findRes.cursor.id;
function take(response) {
  const batch = response.cursor.firstBatch || response.cursor.nextBatch || [];
  for (const doc of batch) {
    canonicalDocs.push(canon(doc));
    relaxedDocs.push(relax(doc));
  }
  return response.cursor.id;
}
take(findRes);
let guard = 0;
while (idString(cursorId) !== "0") {
  guard += 1;
  if (guard > 20) fail("getmore_loop");
  const more = synth.runCommand({
    getMore: cursorId,
    collection: "orders",
    batchSize: 20,
    comment: "oo-dl01w1-14:reader:batch:getMore",
  });
  if (more.ok !== 1) fail("getmore");
  cursorId = take(more);
}
fs.writeFileSync(outDir + "/canonical-docs.json", JSON.stringify(canonicalDocs));
fs.writeFileSync(outDir + "/relaxed-docs.json", JSON.stringify(relaxedDocs));

const opened = synth.runCommand({
  find: "orders",
  sort: { _id: 1 },
  batchSize: 1,
  comment: "oo-dl01w1-14:reader:cancel:find",
});
if (opened.ok !== 1) fail("cancel_find");
const cancelId = opened.cursor.id;
if (idString(cancelId) === "0") {
  fs.writeFileSync(
    outDir + "/cancel.json",
    JSON.stringify({
      cursor_id: idString(cancelId),
      cursor_nonzero: false,
      kill: null,
      getMore: null,
    }),
  );
  fail("cancel_cursor_exhausted");
}
const killed = synth.runCommand({
  killCursors: "orders",
  cursors: [cancelId],
  comment: "oo-dl01w1-14:reader:cancel:kill",
});
let after;
try {
  after = synth.runCommand({
    getMore: cancelId,
    collection: "orders",
    comment: "oo-dl01w1-14:reader:cancel:getMore",
  });
} catch (err) {
  // A killed cursor surfaces as CursorNotFound. That is the cancel observation.
  after = { ok: 0, code: err.code, codeName: err.codeName };
}
const cancel = {
  cursor_id: idString(cancelId),
  cursor_nonzero: idString(cancelId) !== "0",
  kill: canon(killed),
  getMore: canon(after),
};
fs.writeFileSync(outDir + "/cancel.json", JSON.stringify(cancel));
if (killed.ok !== 1) fail("kill");
if (after.ok === 1) fail("getmore_after_kill");

let cdc;
try {
  cdc = synth.runCommand({
    aggregate: "orders",
    pipeline: [{ $changeStream: {} }],
    cursor: { batchSize: 0 },
    maxTimeMS: 3000,
    comment: "oo-dl01w1-14:reader:negative-cdc:watch",
  });
} catch (err) {
  const message = String(err.errmsg || err.message || "");
  cdc = {
    ok: 0,
    code: err.code,
    codeName: err.codeName,
    errmsg: message.slice(0, 240),
  };
}
const cdcOut = canon(cdc);
fs.writeFileSync(outDir + "/cdc.json", JSON.stringify(cdcOut));
if (cdc.ok === 1) {
  const stray = cdc.cursor && cdc.cursor.id;
  if (stray && idString(stray) !== "0") {
    synth.runCommand({
      killCursors: "orders",
      cursors: [stray],
      comment: "oo-dl01w1-14:reader:negative-cdc:kill-unexpected",
    });
  }
  print("READER_FAIL change_stream_opened");
  quit(4);
}

fs.writeFileSync(
  outDir + "/reader-result.json",
  JSON.stringify({
    ok: true,
    docs: canonicalDocs.length,
    helloHasSetName: catalog.helloHasSetName,
    cancel_ok: killed.ok,
    getMore_ok: after.ok,
    cdc_ok: cdc.ok,
  }),
);
print("READER_OK docs=" + canonicalDocs.length);
quit(0);
