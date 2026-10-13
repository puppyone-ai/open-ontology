#!/bin/sh
# Run DBA setup, one reader session, one audit session, then the typed oracle.
# Passwords stay in /run/secrets inside the container. They are not arguments.
set -u

ROOT="$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)"
EVID="${1:?evidence directory}"
CONTAINER="${2:-oo-dl01w1-14-mongodb}"
RAW="$EVID/raw/journey"
mkdir -p "$RAW/out"

run_phase() {
  name="$1"
  shift
  "$@" >"$RAW/$name.out" 2>"$RAW/$name.err"
  status=$?
  echo "$status" >"$RAW/$name.exit"
  echo "$name $status"
  return 0
}

run_phase dba-copy docker cp "$ROOT/scripts/dba-setup.js" "$CONTAINER:/tmp/oo-dba-setup.js"
run_phase dba-setup docker exec -u 0 -e HOME=/tmp "$CONTAINER" \
  mongosh --quiet --norc \
  "mongodb://127.0.0.1:27017/admin?appName=oo-dl01w1-14-dba&maxPoolSize=1" \
  --file /tmp/oo-dba-setup.js

run_phase reader-copy docker cp "$ROOT/scripts/reader-probes.js" "$CONTAINER:/tmp/oo-reader-probes.js"
run_phase reader docker exec -u 0 -e HOME=/tmp "$CONTAINER" \
  mongosh --quiet --norc \
  "mongodb://127.0.0.1:27017/oo_synth?authSource=admin&appName=oo-dl01w1-14-reader&maxPoolSize=1" \
  --file /tmp/oo-reader-probes.js

run_phase audit-copy docker cp "$ROOT/scripts/audit-profile.js" "$CONTAINER:/tmp/oo-audit-profile.js"
run_phase audit docker exec -u 0 -e HOME=/tmp "$CONTAINER" \
  mongosh --quiet --norc \
  "mongodb://127.0.0.1:27017/oo_synth?authSource=admin&appName=oo-dl01w1-14-audit&maxPoolSize=1" \
  --file /tmp/oo-audit-profile.js

run_phase collect docker cp "$CONTAINER:/tmp/oo-dl01w1-14-out/." "$RAW/out/"
run_phase server-log docker logs "$CONTAINER"

if [ ! -f "$RAW/out/canonical-docs.json" ]; then
  echo "journey missing canonical export" >"$RAW/oracle.out"
  echo 2 >"$RAW/oracle.exit"
  echo oracle 2
  exit 2
fi

run_phase oracle python3 "$ROOT/scripts/oracle.py" \
  --docs "$RAW/out/canonical-docs.json" \
  --relaxed "$RAW/out/relaxed-docs.json" \
  --profile "$RAW/out/profile.json" \
  --catalog "$RAW/out/catalog.json" \
  --cancel "$RAW/out/cancel.json" \
  --cdc "$RAW/out/cdc.json" \
  --expected "$ROOT/fixtures/expected-decisions.json" \
  --delete "$ROOT/fixtures/delete-event.json" \
  --out "$RAW/oracle-result.json"

python3 - "$EVID" "$ROOT" << 'PY'
import pathlib, sys
evidence = pathlib.Path(sys.argv[1])
root = pathlib.Path(sys.argv[2])
secret_dir = pathlib.Path("/tmp/oo-dl01w1-14/secrets")
secrets = []
for path in sorted(secret_dir.iterdir()):
    if path.is_file():
        text = path.read_text().strip()
        if len(text) >= 16:
            secrets.append(text.encode())
hits = []
for base in (evidence, root):
    for path in base.rglob("*"):
        if not path.is_file():
            continue
        blob = path.read_bytes()
        if any(secret in blob for secret in secrets):
            replaced = blob
            for secret in secrets:
                replaced = replaced.replace(secret, b"[redacted]")
            path.write_bytes(replaced)
            hits.append(str(path))
note = evidence / "raw" / "journey" / "secret-scan.txt"
if hits:
    note.write_text("secret_scan_redacted\n" + "\n".join(hits) + "\n")
    print("secret_scan redacted", len(hits))
    sys.exit(6)
note.write_text("secret_scan_ok\n")
print("secret_scan ok")
PY
scan=$?
echo "$scan" >"$RAW/secret-scan.exit"
echo "secret-scan $scan"

dba=$(cat "$RAW/dba-setup.exit")
reader=$(cat "$RAW/reader.exit")
audit=$(cat "$RAW/audit.exit")
oracle=$(cat "$RAW/oracle.exit")
if [ "$dba" = 0 ] && [ "$reader" = 0 ] && [ "$audit" = 0 ] && [ "$oracle" = 0 ] && [ "$scan" = 0 ]; then
  echo 0 >"$RAW/journey.exit"
  exit 0
fi
echo 1 >"$RAW/journey.exit"
exit 1
