#!/bin/bash
# Fresh read identity for the owned-cursor kill and the profile gate.
# Does not recreate Mongo, does not replace the frozen Arrow, does not edit older raw dirs.
set -euo pipefail

ROOT="/Users/redux/puppy-one/open-ontology"
SRC="$ROOT/testing/dl01-driver-preflight/mongodb"
EVIDENCE="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-driver-preflight"
OUT="${CANCEL_READ_OUT:-$EVIDENCE/raw/repair-61-cancel}"
SECRETS="/tmp/oo-dl01w1-41-secrets"
PREFIX="oo-dl01w1-41"
MONGO="${PREFIX}-mongodb"
PROBE="${PREFIX}-read"
RUST="${PREFIX}-rust-repair"
RUST_IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
ADMIT="$SRC/scripts/admit.py"
BIN="${CANCEL_READ_BIN:-$EVIDENCE/raw/repair-60-cancel-build/bin}"
CUSTOMER="$SRC/scripts/customer.preflight.json"
REGISTRY="$ROOT/tools/mongodb-preview/examples/secret-registry.json"
FROZEN="$EVIDENCE/batch.arrow"
EJSON="$EVIDENCE/raw/fixture/canonical-ejson.json"
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
  python3 - "$SECRETS" "$1" <<'PY'
import pathlib, sys
root = pathlib.Path(sys.argv[1])
source = pathlib.Path(sys.argv[2])
secrets = []
if root.is_dir():
    for path in root.iterdir():
        if path.is_file():
            text = path.read_text(errors="replace").strip()
            if len(text) >= 12:
                secrets.append(text)
data = source.read_text(errors="replace")
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

if docker ps -a --format '{{.Names}}' | grep -qx "$RUST"; then
  echo "named container $RUST still present" >&2
  exit 2
fi
if docker ps -a --format '{{.Names}}' | grep -qx "$PROBE"; then
  echo "named container $PROBE still present" >&2
  exit 2
fi
if [[ -e "$OUT" ]]; then
  echo "fresh directory already exists: $OUT" >&2
  exit 2
fi
mkdir -p "$OUT"
: > "$OUT/phase.txt"
phase "cancel read start"
docker image inspect "$RUST_IMAGE" >/dev/null
test -x "$BIN/oo-mongodb-driver-probe"
test -x "$BIN/oo-mongodb-arrow-decode"
test -f "$BIN/liboo-opmsg-names.so"
test -f "$FROZEN"
test -f "$EJSON"
test -f "$CUSTOMER"
test ! -e "$OUT/batch.arrow"

phase "reuse existing mongodb"
docker inspect "$MONGO" >/dev/null
test -f "$SECRETS/reader"
test -f "$SECRETS/dba"
test -f "$SECRETS/audit"
if ! admit_wait mongodb "$OUT/admission-mongodb.json"; then
  fail "mongodb capacity"
fi
if [[ "$(docker inspect -f '{{.State.Status}}' "$MONGO")" != "running" ]]; then
  docker start "$MONGO" >"$OUT/mongo-start.out"
fi

ready=0
ping_eval='const fs=require("fs"); const pw=fs.readFileSync("/run/secrets/dba","utf8").trim(); const admin=db.getSiblingDB("admin"); if (!admin.auth("oo_dl01w1_41_dba", pw)) quit(2); const r=admin.runCommand({ping:1}); if (r.ok===1) quit(0); quit(2);'
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
  fail "mongodb ping"
fi

phase "dba profile level 2"
docker cp "$SRC/scripts/profile-enable.js" "$MONGO:/tmp/oo-profile-enable.js"
set +e
docker exec -u 0 -e HOME=/tmp "$MONGO" \
  mongosh --quiet --norc "$DBA_URI" --file /tmp/oo-profile-enable.js \
  >"$OUT/profile-enable.out" 2>"$OUT/profile-enable.err"
profile_rc=$?
set -e
printf '%s\n' "$profile_rc" >"$OUT/profile-enable.exit"
redact_stream "$OUT/profile-enable.out" >"$OUT/profile-enable.redacted" || true
if [[ "$profile_rc" -ne 0 ]]; then
  fail "profile enable $profile_rc"
fi

phase "admit rust-read"
if ! admit_wait rust "$OUT/admission-rust.json"; then
  fail "rust capacity"
fi
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
  -v "$OUT/probe:/out" \
  -v "$SECRETS/reader:/run/secrets/reader:ro" \
  -v "$CUSTOMER:/run/customer.json:ro" \
  -v "$REGISTRY:/run/secret-registry.json:ro" \
  -v "$BIN:/binaries:ro" \
  -v "$SRC/scripts/probe-inside.sh:/probe-inside.sh:ro" \
  "${LABELS[@]}" \
  --label kind=rust-builder \
  "$RUST_IMAGE" \
  /probe-inside.sh \
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
if [[ "$(cat "$OUT/probe/decode-truncated.exit")" == "0" ]]; then
  fail "truncated decode was ready"
fi

phase "audit profile"
mkdir -p "$OUT/audit"
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
python3 - "$FROZEN" "$OUT" <<'PY'
import hashlib, json, sys
from pathlib import Path
frozen_arrow, out = map(Path, sys.argv[1:])
partial = out / "probe" / "batch.arrow.partial"
partial_sha = hashlib.sha256(partial.read_bytes()).hexdigest()
frozen_sha = hashlib.sha256(frozen_arrow.read_bytes()).hexdigest()
report = {
    "partial_sha256": partial_sha,
    "frozen_arrow_sha256": frozen_sha,
    "partial_matches_frozen_arrow": partial_sha == frozen_sha,
    "frozen_arrow_replaced": False,
}
(out / "frozen-compare.json").write_text(json.dumps(report, indent=2) + "\n")
if partial_sha != frozen_sha:
    raise SystemExit(2)
PY
cmp_rc=$?
set -e
printf '%s\n' "$cmp_rc" >"$OUT/compare.exit"
if [[ "$cmp_rc" -ne 0 ]]; then
  fail "compare $cmp_rc"
fi

phase "oracle before promotion"
test ! -e "$OUT/batch.arrow"
set +e
python3 "$SRC/scripts/oracle.py" \
  --ejson "$EJSON" \
  --decoded "$OUT/probe/decoded.json" \
  --decoded-truncated "$OUT/probe/decoded-truncated.json" \
  --decode-exit "$OUT/probe/decode.exit" \
  --decode-truncated-exit "$OUT/probe/decode-truncated.exit" \
  --gates "$OUT/probe/gates.json" \
  --commands "$OUT/probe/commands.json" \
  --connections "$OUT/probe/connections.json" \
  --wire "$OUT/probe/opmsg-names.jsonl" \
  --pre-read "$OUT/probe/pre-read-deny.json" \
  --catalog "$OUT/probe/catalog.json" \
  --census "$OUT/probe/field-census.json" \
  --rejections "$OUT/probe/rejections.json" \
  --timeout "$OUT/probe/timeout.json" \
  --change "$OUT/probe/change-stream.json" \
  --cancel "$OUT/probe/cancel.json" \
  --truncation "$OUT/probe/truncation.json" \
  --options "$OUT/probe/client-options.json" \
  --profile-admin "$OUT/audit/profile-admin.json" \
  --profile-db "$OUT/audit/profile-oo_w141.json" \
  --audit "$OUT/audit/audit-result.json" \
  --evidence "$EVIDENCE" \
  --source "$SRC" \
  --secrets "$SECRETS" \
  --batch-arrow "$OUT/batch.arrow" \
  --partial "$OUT/probe/batch.arrow.partial" \
  --execution-mode "$OUT/probe/execution-mode.txt" \
  --out "$OUT/oracle-result.json"
oracle_rc=$?
set -e
printf '%s\n' "$oracle_rc" >"$OUT/oracle.exit"
if [[ "$oracle_rc" -ne 0 ]]; then
  printf '%s\n' false >"$OUT/promotion.txt"
  fail "oracle $oracle_rc"
fi

phase "promote fresh batch.arrow"
python3 - "$OUT/probe/batch.arrow.partial" "$OUT/batch.arrow" "$FROZEN" "$OUT/oracle-result.json" <<'PY'
import hashlib, json, pathlib, shutil, sys
partial, dest, frozen, result = map(pathlib.Path, sys.argv[1:])
body = json.loads(result.read_text())
if body.get("ready") is not True or not partial.is_file() or dest.exists():
    raise SystemExit(1)
if hashlib.sha256(partial.read_bytes()).digest() != hashlib.sha256(frozen.read_bytes()).digest():
    raise SystemExit(2)
shutil.copyfile(partial, dest)
print("promoted-fresh")
PY
printf '%s\n' true >"$OUT/promotion.txt"
if ! cmp -s "$OUT/batch.arrow" "$FROZEN"; then
  fail "fresh arrow differs from frozen"
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
redact_stream "$OUT/reopen-find.out" >"$OUT/reopen-find.redacted" || true
redact_stream "$OUT/reopen-find.err" >"$OUT/reopen-find.err.redacted" || true
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
phase "cancel read ok"
printf 'DONE cancel-read 0\n' > "$OUT/status.txt"
exit 0
