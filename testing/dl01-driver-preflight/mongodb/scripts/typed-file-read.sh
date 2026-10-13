#!/bin/bash
# Same-run native read of the owned Mongo fixture. Does not replace batch.arrow.
set -euo pipefail

ROOT="/Users/redux/puppy-one/open-ontology"
SRC="$ROOT/testing/dl01-driver-preflight/mongodb"
EVIDENCE="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-driver-preflight"
OUT="${TYPED_READ_OUT:-$EVIDENCE/raw/typed-file-read}"
SECRETS="/tmp/oo-dl01w1-41-secrets"
PREFIX="oo-dl01w1-41"
MONGO="${PREFIX}-mongodb"
PROBE="${PREFIX}-read"
RUST_IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
MONGO_IMAGE="mongo@sha256:d0d926f94df099bff534b7ee5b5986458131a22489dfff8664509af0c1e2ca9c"
ADMIT="$SRC/scripts/admit.py"
BIN="${TYPED_READ_BIN:-$EVIDENCE/raw/build/bin}"
CUSTOMER="${TYPED_READ_CUSTOMER:-$ROOT/tools/mongodb-preview/examples/customer.shipments.json}"
FROZEN="$EVIDENCE/batch.arrow"
FROZEN_DECODED="$EVIDENCE/raw/probe/decoded.json"
DBA_URI="mongodb://127.0.0.1:27017/admin?appName=oo-dl01w1-41-dba&maxPoolSize=1"
AUDIT_URI="mongodb://127.0.0.1:27017/admin?appName=oo-dl01w1-41-audit&maxPoolSize=1"
LABELS=(
  --label project=open-ontology
  --label owner=dl01-w1-41
  --label session=23df6145-4592-4dc0-82df-fd226dd020fd
  --label task=DL01-W1-41
)

phase() {
  mkdir -p "$OUT"
  printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" | tee -a "$OUT/phase.txt"
}

redact_stream() {
  python3 - "$SECRETS" <<'PY'
import pathlib, sys
root = pathlib.Path(sys.argv[1])
secrets = []
if root.is_dir():
    for path in root.iterdir():
        if path.is_file():
            text = path.read_text(errors="replace").strip()
            if len(text) >= 12:
                secrets.append(text)
data = sys.stdin.buffer.read().decode("utf-8", "replace")
for secret in secrets:
    data = data.replace(secret, "[redacted]")
sys.stdout.write(data)
PY
}

release_rust() {
  docker rm -f "$PROBE" >/dev/null 2>&1 || true
  python3 "$ADMIT" release rust >/dev/null 2>&1 || true
}

cleanup() {
  local ec=$?
  docker rm -f "$PROBE" >/dev/null 2>&1 || true
  python3 "$ADMIT" release rust >/dev/null 2>&1 || true
  if [[ "$ec" -ne 0 ]]; then
    python3 "$ADMIT" release mongodb >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

fail() {
  phase "FAILED $*"
  printf 'FAILED %s\n' "$*" > "$OUT/status.txt"
  exit 1
}

admit_wait() {
  local kind="$1" dest="$2" attempt
  for attempt in $(seq 1 30); do
    if python3 "$ADMIT" acquire "$kind" "$dest"; then
      return 0
    fi
    phase "capacity wait ${kind} attempt ${attempt}"
    sleep 900
  done
  return 3
}

mkdir -p "$OUT"
: > "$OUT/phase.txt"
phase "read start"
docker image inspect "$MONGO_IMAGE" >/dev/null
docker image inspect "$RUST_IMAGE" >/dev/null
test -x "$BIN/oo-mongodb-driver-probe"
test -x "$BIN/oo-mongodb-arrow-decode"
test -f "$FROZEN"
test -f "$FROZEN_DECODED"

docker rm -f "$PROBE" >/dev/null 2>&1 || true
reuse=0
if docker inspect "$MONGO" >/dev/null 2>&1; then
  reuse=1
  phase "reuse existing mongodb"
  test -f "$SECRETS/reader"
  phase "admit mongodb"
  if ! admit_wait mongodb "$OUT/admission-mongodb.json"; then
    fail "mongodb capacity"
  fi
  if [[ "$(docker inspect -f '{{.State.Status}}' "$MONGO")" != "running" ]]; then
    docker start "$MONGO" >"$OUT/mongo-start.out"
  fi
fi
if [[ "$reuse" -eq 0 ]]; then
rm -rf "$SECRETS"
mkdir -m 0700 "$SECRETS"
umask 077
printf 'Aa1%s\n' "$(openssl rand -hex 12)" >"$SECRETS/dba"
printf 'Aa1%s\n' "$(openssl rand -hex 12)" >"$SECRETS/reader"
printf 'Aa1%s\n' "$(openssl rand -hex 12)" >"$SECRETS/audit"
chmod 600 "$SECRETS/dba" "$SECRETS/reader" "$SECRETS/audit"
umask 022
docker volume rm "${PREFIX}-data" >/dev/null 2>&1 || true
if ! docker network inspect "${PREFIX}-net" >/dev/null 2>&1; then
  docker network create \
    --label project=open-ontology --label owner=dl01-w1-41 \
    --label session=23df6145-4592-4dc0-82df-fd226dd020fd \
    --label task=DL01-W1-41 --label kind=mongodb-source \
    "${PREFIX}-net" >/dev/null
fi
docker volume create "${PREFIX}-data" \
  --label project=open-ontology --label owner=dl01-w1-41 \
  --label session=23df6145-4592-4dc0-82df-fd226dd020fd \
  --label task=DL01-W1-41 --label kind=mongodb-source >/dev/null

phase "admit mongodb"
if ! admit_wait mongodb "$OUT/admission-mongodb.json"; then
  fail "mongodb capacity"
fi
docker run -d --name "$MONGO" \
  --platform linux/arm64 --pull never \
  --network "${PREFIX}-net" \
  --memory 2g --memory-swap 2g --cpus 1 \
  --pids-limit 512 \
  -v "${PREFIX}-data:/data/db" \
  -v "$SECRETS:/run/secrets:ro" \
  "${LABELS[@]}" \
  --label kind=mongodb-source \
  "$MONGO_IMAGE" \
  mongod --bind_ip 127.0.0.1 --port 27017 --auth \
  --wiredTigerCacheSizeGB 0.25 \
  --setParameter diagnosticDataCollectionEnabled=false \
  >"$OUT/mongo-run.out"
fi

ready=0
if [[ "$reuse" -eq 1 ]]; then
  ping_eval='const fs=require("fs"); const pw=fs.readFileSync("/run/secrets/dba","utf8").trim(); const admin=db.getSiblingDB("admin"); if (!admin.auth("oo_dl01w1_41_dba", pw)) quit(2); const r=admin.runCommand({ping:1}); if (r.ok===1) quit(0); quit(2);'
else
  ping_eval='const r = db.runCommand({ ping: 1 }); if (r.ok === 1) quit(0); quit(2);'
fi
for attempt in $(seq 1 20); do
  sleep 2
  set +e
  docker exec -u 0 -e HOME=/tmp "$MONGO" \
    mongosh --quiet --norc "$DBA_URI" \
    --eval "$ping_eval" \
    >"$OUT/ping.out" 2>"$OUT/ping.err"
  ping_rc=$?
  set -e
  if [[ "$ping_rc" -eq 0 ]]; then
    ready=1
    break
  fi
done
printf '%s\n' "$ready" >"$OUT/ping.ready"
if [[ "$ready" -ne 1 ]]; then
  docker logs "$MONGO" 2>&1 | redact_stream >"$OUT/mongo-logs.txt" || true
  fail "mongodb ping"
fi

if [[ "$reuse" -eq 1 ]]; then
  phase "fixture skipped; sealed rows stay"
else
phase "fixture"
docker cp "$SRC/scripts/dba-setup.js" "$MONGO:/tmp/oo-dba-setup.js"
set +e
docker exec -u 0 -e HOME=/tmp "$MONGO" \
  mongosh --quiet --norc "$DBA_URI" --file /tmp/oo-dba-setup.js \
  >"$OUT/dba-setup.out" 2>"$OUT/dba-setup.err"
dba_rc=$?
set -e
printf '%s\n' "$dba_rc" >"$OUT/dba-setup.exit"
redact_stream <"$OUT/dba-setup.out" >"$OUT/dba-setup.redacted" || true
if [[ "$dba_rc" -ne 0 ]]; then
  fail "dba-setup $dba_rc"
fi
fi

phase "admit rust-read"
if ! admit_wait rust "$OUT/admission-rust.json"; then
  fail "rust capacity"
fi
rm -rf "$OUT/probe"
mkdir -p "$OUT/probe"
set +e
docker run --name "$PROBE" \
  --platform linux/arm64 --pull never \
  --network "container:${MONGO}" \
  --memory 2g --memory-swap 2g --cpus 1 \
  --pids-limit 512 \
  --entrypoint bash \
  -e OO_OUT=/out \
  -e OO_READER_PASSWORD_FILE=/run/secrets/reader \
  -e OO_W141_OPMSG_LOG=/out/opmsg-names.jsonl \
  -e OO_W141_CUSTOMER=/run/customer.json \
  -e OO_W141_SECRET_REGISTRY=/run/secret-registry.json \
  -e LD_PRELOAD=/binaries/liboo-opmsg-names.so \
  -v "$OUT/probe:/out" \
  -v "$SECRETS/reader:/run/secrets/reader:ro" \
  -v "$CUSTOMER:/run/customer.json:ro" \
  -v "$ROOT/tools/mongodb-preview/examples/secret-registry.json:/run/secret-registry.json:ro" \
  -v "$BIN:/binaries:ro" \
  "${LABELS[@]}" \
  --label kind=rust-builder \
  "$RUST_IMAGE" \
  -c 'printf prebuilt-binary\\n > /out/execution-mode.txt; /binaries/oo-mongodb-driver-probe; echo $? > /out/probe.exit; unset LD_PRELOAD; /binaries/oo-mongodb-arrow-decode --arrow /out/batch.arrow.partial --out /out/decoded.json; echo $? > /out/decode.exit' \
  >"$OUT/probe/docker.stdout" 2>"$OUT/probe/docker.stderr"
probe_rc=$?
set -e
printf '%s\n' "$probe_rc" >"$OUT/probe/docker.exit"
release_rust
phase "probe container ${probe_rc}"
if [[ "$probe_rc" -ne 0 ]]; then
  fail "probe $probe_rc"
fi
if [[ "$(cat "$OUT/probe/probe.exit")" != "0" || "$(cat "$OUT/probe/decode.exit")" != "0" ]]; then
  fail "probe exits"
fi

phase "audit profile"
docker cp "$SRC/scripts/audit.js" "$MONGO:/tmp/oo-audit.js"
set +e
docker exec -u 0 -e HOME=/tmp "$MONGO" \
  mongosh --quiet --norc "$AUDIT_URI" --file /tmp/oo-audit.js \
  >"$OUT/audit.out" 2>"$OUT/audit.err"
audit_rc=$?
set -e
printf '%s\n' "$audit_rc" >"$OUT/audit.exit"
docker cp "$MONGO:/tmp/oo-dl01w1-41-audit/." "$OUT/audit/" || true
if [[ "$audit_rc" -ne 0 ]]; then
  fail "audit $audit_rc"
fi

phase "compare frozen"
set +e
python3 - "$FROZEN" "$FROZEN_DECODED" "$OUT" <<'PY'
import hashlib, json, sys
from pathlib import Path
frozen_arrow, frozen_decoded, out = map(Path, sys.argv[1:])
partial = out / "probe" / "batch.arrow.partial"
decoded = json.loads((out / "probe" / "decoded.json").read_text())
gold = json.loads(frozen_decoded.read_text())
partial_sha = hashlib.sha256(partial.read_bytes()).hexdigest()
frozen_sha = hashlib.sha256(frozen_arrow.read_bytes()).hexdigest()
commands = json.loads((out / "probe" / "commands.json").read_text())
names = commands.get("names") or []
writes = sorted({name for name in names if name in {
    "insert", "update", "delete", "findAndModify", "create", "drop",
    "createIndexes", "dropDatabase", "rename", "compact", "collMod",
}})
records = decoded.get("records") or []
gold_records = gold.get("records") or []
schema = [item.get("name") for item in decoded.get("schema") or []]
report = {
    "protocol": "oo.mongodb-preview.native-read-audit/v1",
    "source_id": "oo_w141",
    "rows": decoded.get("rows"),
    "gold_rows": gold.get("rows"),
    "field_count": len(schema),
    "fields": schema,
    "records_match_frozen_decode": records == gold_records,
    "partial_sha256": partial_sha,
    "frozen_arrow_sha256": frozen_sha,
    "partial_matches_frozen_arrow": partial_sha == frozen_sha,
    "frozen_arrow_replaced": False,
    "reader_write_commands": writes,
    "reader_command_names": names,
    "budget_rows": 16,
}
(out / "read-audit.json").write_text(json.dumps(report, indent=2) + "\n")
if decoded.get("rows") != 16 or len(schema) != 19 or records != gold_records or writes:
    raise SystemExit(1)
if partial_sha != frozen_sha:
    raise SystemExit(2)
PY
cmp_rc=$?
set -e
printf '%s\n' "$cmp_rc" >"$OUT/compare.exit"
if [[ "$cmp_rc" -ne 0 ]]; then
  fail "compare $cmp_rc"
fi

phase "source off"
docker stop "$MONGO" >"$OUT/source-off.out"
printf 'stopped\n' >"$OUT/source-off.txt"
set +e
docker exec "$MONGO" mongosh --quiet --norc "$DBA_URI" --eval 'quit(0)' >"$OUT/source-off-ping.out" 2>"$OUT/source-off-ping.err"
off_ping=$?
set -e
printf '%s\n' "$off_ping" >"$OUT/source-off-ping.exit"
if [[ "$off_ping" -eq 0 ]]; then
  fail "source still answered after stop"
fi

phase "source reopen"
docker start "$MONGO" >"$OUT/source-reopen.out"
reopen_ready=0
reopen_ping_eval='const fs=require("fs"); const pw=fs.readFileSync("/run/secrets/dba","utf8").trim(); const admin=db.getSiblingDB("admin"); if (!admin.auth("oo_dl01w1_41_dba", pw)) quit(2); const r=admin.runCommand({ping:1}); if (r.ok===1) quit(0); quit(2);'
for attempt in $(seq 1 20); do
  sleep 2
  set +e
  docker exec -u 0 -e HOME=/tmp "$MONGO" \
    mongosh --quiet --norc "$DBA_URI" \
    --eval "$reopen_ping_eval" \
    >"$OUT/reopen-ping.out" 2>"$OUT/reopen-ping.err"
  reopen_rc=$?
  set -e
  if [[ "$reopen_rc" -eq 0 ]]; then
    reopen_ready=1
    break
  fi
done
printf '%s\n' "$reopen_ready" >"$OUT/reopen.ready"
if [[ "$reopen_ready" -ne 1 ]]; then
  fail "reopen"
fi
set +e
docker exec -u 0 -e HOME=/tmp "$MONGO" \
  mongosh --quiet --norc "mongodb://127.0.0.1:27017/oo_w141?authSource=admin&appName=oo-dl01w1-41-reader-reopen" \
  --eval 'const fs=require("fs"); const pw=fs.readFileSync("/run/secrets/reader","utf8").trim(); if (!db.getSiblingDB("admin").auth("oo_dl01w1_41_reader", pw)) quit(2); const n=db.getSiblingDB("oo_w141").shipments.find({},{code:1}).limit(16).toArray().length; fs.writeFileSync("/tmp/oo-reopen-count.txt", "rows="+n+" role=read\n"); if (n!==16) quit(3);' \
  >"$OUT/reopen-find.out" 2>"$OUT/reopen-find.err"
reopen_find_rc=$?
set -e
printf '%s\n' "$reopen_find_rc" >"$OUT/reopen-find.exit"
docker cp "$MONGO:/tmp/oo-reopen-count.txt" "$OUT/reopen-count.txt" >/dev/null 2>&1 || true
docker exec -u 0 "$MONGO" rm -f /tmp/oo-reopen-count.txt >/dev/null 2>&1 || true
redact_stream <"$OUT/reopen-find.out" >"$OUT/reopen-find.redacted" || true
redact_stream <"$OUT/reopen-find.err" >"$OUT/reopen-find.err.redacted" || true
rm -f "$OUT/reopen-find.out" "$OUT/reopen-find.err"
if [[ "$reopen_find_rc" -ne 0 ]]; then
  fail "reopen find $reopen_find_rc"
fi
if [[ ! -f "$OUT/reopen-count.txt" ]] || ! grep -q 'rows=16 role=read' "$OUT/reopen-count.txt"; then
  fail "reopen find silent"
fi

phase "source stop after reopen proof"
docker stop "$MONGO" >"$OUT/source-stop-after-reopen.out"
python3 "$ADMIT" release mongodb >/dev/null
# Keep the volume until the product cleanup. The container is stopped.
shasum -a 256 "$FROZEN" >"$OUT/frozen-arrow.sha"
phase "read ok"
printf 'DONE read 0\n' > "$OUT/status.txt"
exit 0
