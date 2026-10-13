// Profile read for admin and oo_w141. Command bodies are reduced to names, comments, and cursor ids.
const fs = require("fs");

const pw = fs.readFileSync("/run/secrets/audit", "utf8").trim();
const admin = db.getSiblingDB("admin");

function fail(message) {
  print("AUDIT_FAIL " + message);
  quit(5);
}

try {
  admin.auth("oo_dl01w1_41_audit", pw);
} catch (err) {
  fail("auth code=" + err.code);
}

const COMMANDS = [
  "find", "getMore", "killCursors", "insert", "update", "delete", "findAndModify",
  "hello", "ismaster", "isMaster", "saslStart", "saslContinue", "endSessions",
  "listCollections", "listIndexes", "collStats", "connectionStatus", "aggregate",
  "ping", "buildInfo", "createIndexes", "create", "drop", "profile", "collMod",
  "dropDatabase", "rename", "compact", "createUser", "grantRolesToUser", "createRole",
  "usersInfo", "getCmdLineOpts", "killOp",
];

function cursorString(value) {
  if (value == null) return null;
  return value.toString();
}

function commandName(cmd) {
  for (let i = 0; i < COMMANDS.length; i += 1) {
    if (Object.prototype.hasOwnProperty.call(cmd, COMMANDS[i])) return COMMANDS[i];
  }
  const keys = Object.keys(cmd);
  return keys.length ? keys[0] : "";
}

function redact(row) {
  const cmd = row.command || {};
  const cursors = Array.isArray(cmd.cursors) ? cmd.cursors.map(cursorString) : [];
  return {
    op: row.op || null,
    ns: row.ns || null,
    appName: row.appName || null,
    user: row.user || null,
    millis: row.millis == null ? null : row.millis,
    command: commandName(cmd),
    comment: typeof cmd.comment === "string" ? cmd.comment : null,
    keys: Object.keys(cmd),
    cursorid: cursorString(row.cursorid),
    getMore: cmd.getMore == null ? null : cursorString(cmd.getMore),
    kill_cursors: cursors,
    nreturned: row.nreturned == null ? null : row.nreturned,
    docsExamined: row.docsExamined == null ? null : row.docsExamined,
    planSummary: row.planSummary || null,
  };
}

function readProfile(databaseName) {
  const database = db.getSiblingDB(databaseName);
  let found;
  try {
    found = database.runCommand({
      find: "system.profile",
      batchSize: 200,
      comment: "oo-dl01w1-41:audit:profile",
    });
  } catch (err) {
    return { ok: false, code: err.code, codeName: err.codeName };
  }
  if (!found || found.ok !== 1 || !found.cursor) {
    return { ok: false, code: found && found.code, codeName: found && found.codeName };
  }
  const rows = [];
  function take(response) {
    const batch = (response.cursor && (response.cursor.firstBatch || response.cursor.nextBatch)) || [];
    for (let i = 0; i < batch.length; i += 1) rows.push(redact(batch[i]));
    return response.cursor.id;
  }
  let cursorId = take(found);
  let guard = 0;
  while (cursorId && cursorId.toString() !== "0") {
    guard += 1;
    if (guard > 40) return { ok: false, codeName: "AuditLoop", rows: rows };
    let more;
    try {
      more = database.runCommand({
        getMore: cursorId,
        collection: "system.profile",
        batchSize: 200,
        comment: "oo-dl01w1-41:audit:profile-more",
      });
    } catch (err) {
      return { ok: false, code: err.code, codeName: err.codeName, rows: rows };
    }
    if (!more || more.ok !== 1) return { ok: false, code: more && more.code, codeName: more && more.codeName, rows: rows };
    cursorId = take(more);
  }
  return { ok: true, rows: rows };
}

const adminProfile = readProfile("admin");
const dataProfile = readProfile("oo_w141");
fs.mkdirSync("/tmp/oo-dl01w1-41-audit", { recursive: true });
fs.writeFileSync("/tmp/oo-dl01w1-41-audit/profile-admin.json", JSON.stringify(adminProfile.rows || adminProfile));
fs.writeFileSync("/tmp/oo-dl01w1-41-audit/profile-oo_w141.json", JSON.stringify(dataProfile.rows || dataProfile));
fs.writeFileSync(
  "/tmp/oo-dl01w1-41-audit/audit-result.json",
  JSON.stringify({
    ok: adminProfile.ok === true && dataProfile.ok === true,
    admin_ok: adminProfile.ok === true,
    oo_w141_ok: dataProfile.ok === true,
    admin_rows: (adminProfile.rows || []).length,
    oo_w141_rows: (dataProfile.rows || []).length,
    admin_code: adminProfile.ok === true ? null : adminProfile.code || null,
    admin_code_name: adminProfile.ok === true ? null : adminProfile.codeName || null,
    oo_w141_code: dataProfile.ok === true ? null : dataProfile.code || null,
    oo_w141_code_name: dataProfile.ok === true ? null : dataProfile.codeName || null,
  }),
);
print("AUDIT_OK admin=" + (adminProfile.rows || []).length + " data=" + (dataProfile.rows || []).length);
quit(0);
