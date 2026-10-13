// DBA window. Turns profiling back on after a mongod restart.
// The read role never issues profile. Command bodies are not printed.
const fs = require("fs");

const pw = fs.readFileSync("/run/secrets/dba", "utf8").trim();
const admin = db.getSiblingDB("admin");
if (!admin.auth("oo_dl01w1_41_dba", pw)) {
  print("PROFILE_ENABLE_FAIL auth");
  quit(2);
}

function current(database) {
  return database.runCommand({ profile: -1 });
}

const data = db.getSiblingDB("oo_w141");
const beforeData = current(data);
const beforeAdmin = current(admin);
const dataSet = data.runCommand({ profile: 2, comment: "oo-dl01w1-41:dba:profiler" });
const adminSet = admin.runCommand({ profile: 2, comment: "oo-dl01w1-41:dba:profiler-admin" });
const afterData = current(data);
const afterAdmin = current(admin);
const ok = dataSet.ok === 1 && adminSet.ok === 1 && afterData.was === 2 && afterAdmin.was === 2;
print(JSON.stringify({
  ok: ok,
  data_before: beforeData.was == null ? null : beforeData.was,
  admin_before: beforeAdmin.was == null ? null : beforeAdmin.was,
  data_after: afterData.was == null ? null : afterData.was,
  admin_after: afterAdmin.was == null ? null : afterAdmin.was,
  data_slowms: afterData.slowms == null ? null : afterData.slowms,
  admin_slowms: afterAdmin.slowms == null ? null : afterAdmin.slowms,
}));
quit(ok ? 0 : 4);
