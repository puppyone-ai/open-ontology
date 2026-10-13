#!/bin/bash
# Runs inside the copy container. Reads the reader secret and does not print it.
set -u
PW=$(tr -d '\n\r' < /run/secrets/reader)
dump_rc=0
mongodump \
  --host 127.0.0.1 --port 27017 \
  --username oo_dl01w1_41_reader \
  --authenticationDatabase admin \
  --db oo_w141 \
  --collection shipments \
  --out /tmp/docseal-dump \
  --password "$PW" \
  > /tmp/docseal-dump.stdout 2> /tmp/docseal-dump.stderr || dump_rc=$?
mongosh --nodb --quiet --norc --eval '
const fs = require("fs");
const pw = fs.readFileSync("/run/secrets/reader", "utf8").trim();
for (const name of ["/tmp/docseal-dump.stdout", "/tmp/docseal-dump.stderr"]) {
  let text = "";
  try { text = fs.readFileSync(name, "utf8"); } catch (err) { text = ""; }
  fs.writeFileSync(name + ".redacted", text.split(pw).join("[redacted]"));
}
'
printf '%s\n' "$dump_rc" > /tmp/docseal-dump.exit
exit "$dump_rc"
