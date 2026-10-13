// Audit identity. Reads system.profile only. Prints no secrets.
const fs = require("fs");

const pw = fs.readFileSync("/run/secrets/audit", "utf8").trim();

function fail(message) {
  print("AUDIT_FAIL " + message);
  quit(5);
}

const admin = db.getSiblingDB("admin");
try {
  admin.auth("oo_dl01w1_14_audit", pw);
} catch (err) {
  fail("auth");
}
const who = admin.runCommand({ connectionStatus: 1 });
const authed = ((who.authInfo && who.authInfo.authenticatedUsers) || []).some(
  (user) => user.user === "oo_dl01w1_14_audit",
);
if (!authed) fail("auth");

const synth = db.getSiblingDB("oo_synth");
const found = synth.runCommand({
  find: "system.profile",
  batchSize: 500,
  comment: "oo-dl01w1-14:audit:profile",
});
if (found.ok !== 1) fail("find");

const rows = [];
function take(response) {
  const batch = response.cursor.firstBatch || response.cursor.nextBatch || [];
  for (const row of batch) rows.push(JSON.parse(EJSON.stringify(row, { relaxed: false })));
  return response.cursor.id;
}
let cursorId = take(found);
let guard = 0;
while (cursorId && cursorId.toString() !== "0") {
  guard += 1;
  if (guard > 50) fail("loop");
  const more = synth.runCommand({
    getMore: cursorId,
    collection: "system.profile",
    batchSize: 500,
    comment: "oo-dl01w1-14:audit:profile-more",
  });
  if (more.ok !== 1) fail("getmore");
  cursorId = take(more);
}

fs.mkdirSync("/tmp/oo-dl01w1-14-out", { recursive: true });
fs.writeFileSync("/tmp/oo-dl01w1-14-out/profile.json", JSON.stringify(rows));
fs.writeFileSync(
  "/tmp/oo-dl01w1-14-out/audit-result.json",
  JSON.stringify({ ok: true, rows: rows.length }),
);
print("AUDIT_OK rows=" + rows.length);
quit(0);
