#!/usr/bin/env bash
# DL01-W1-10 live Oracle Database Free preparation. No Rust compile.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../../../.." && pwd)"
SRC="$ROOT/testing/dl01-targets/oracle"
EVIDENCE="$ROOT/evidence/18da73a5-ad34-477a-8edf-0dc51122c1dd/DL01/oracle"
RAW="$EVIDENCE/raw"
SECRETS="/tmp/oo-dl01w1-10-secrets"
SLOT="/tmp/oo-dl01-wave1/slots/DL01-W1-10.json"
IMAGE="docker.io/gvenzl/oracle-free@sha256:cdc5b3a3685da4f1785daeee24f74de8a9768b7629192ea9abdaeeea343d12a8"
CONTAINER="oo-dl01w1-10-oracle"
VOLUME="oo-dl01w1-10-oradata"
NETWORK="oo-dl01w1-10-net"
OH=""
CLEANED=0
JUDGED=0

mkdir -p "$RAW"
chmod 777 "$EVIDENCE" "$RAW"
: >"$RAW/phase-exits.txt"
rm -f "$RAW/critical-fail.txt"

cleanup() {
  if [[ "$CLEANED" -eq 1 ]]; then
    return
  fi
  CLEANED=1
  if docker container inspect "$CONTAINER" >/dev/null 2>&1; then
    owner="$(docker inspect -f '{{index .Config.Labels "owner"}}' "$CONTAINER" 2>/dev/null || true)"
    docker logs "$CONTAINER" >"$RAW/docker-logs.txt" 2>&1 || true
    if [[ "$owner" == "dl01-w1-10" ]]; then
      docker rm -f "$CONTAINER" >"$RAW/docker-rm.txt" 2>&1 || true
    else
      echo "cleanup_skipped_foreign_container owner=${owner}" >>"$RAW/docker-rm.txt"
    fi
  fi
  if docker volume inspect "$VOLUME" >/dev/null 2>&1; then
    owner="$(docker volume inspect -f '{{index .Labels "owner"}}' "$VOLUME" 2>/dev/null || true)"
    if [[ "$owner" == "dl01-w1-10" ]]; then
      docker volume rm "$VOLUME" >"$RAW/docker-volume-rm.txt" 2>&1 || true
    fi
  fi
  if docker network inspect "$NETWORK" >/dev/null 2>&1; then
    owner="$(docker network inspect -f '{{index .Labels "owner"}}' "$NETWORK" 2>/dev/null || true)"
    if [[ "$owner" == "dl01-w1-10" ]]; then
      docker network rm "$NETWORK" >"$RAW/docker-network-rm.txt" 2>&1 || true
    fi
  fi
  rm -rf "$SECRETS"
  rm -f "$SLOT"
  python3 "$SRC/scripts/redact.py" "$EVIDENCE" || true
  if [[ "$JUDGED" -eq 0 ]]; then
    python3 "$SRC/scripts/judge.py" "$EVIDENCE" "$SRC/fixtures/expected.json" "$SRC/sql" >"$RAW/judge-stdout.txt" 2>"$RAW/judge-stderr.txt" || true
    JUDGED=1
  fi
  {
    echo "=== containers ==="
    docker ps -a --filter name=oo-dl01w1-10 --format '{{.Names}}|{{.Image}}|{{.Status}}|{{.Labels}}' || true
    echo "=== volumes ==="
    docker volume ls --filter name=oo-dl01w1-10 --format '{{.Name}}' || true
    echo "=== networks ==="
    docker network ls --filter name=oo-dl01w1-10 --format '{{.Name}}' || true
    echo "=== image_retained ==="
    docker image inspect "$IMAGE" --format 'id={{.Id}} arch={{.Architecture}} os={{.Os}} size={{.Size}}' || true
    echo "=== secrets ==="
    if [[ -d "$SECRETS" ]]; then echo present; else echo removed; fi
    echo "=== slot ==="
    if [[ -f "$SLOT" ]]; then echo present; else echo removed; fi
  } >"$RAW/cleanup-proof.txt"
}
trap cleanup EXIT

run_phase() {
  local name="$1"
  shift
  printf '%s\n' "$name" >"$RAW/current-phase"
  set +e
  "$@" >"$RAW/${name}.stdout" 2>"$RAW/${name}.stderr"
  local code=$?
  set -e
  printf '%s\n' "$code" >"$RAW/${name}.exit"
  printf '%s %s\n' "$name" "$code" >>"$RAW/phase-exits.txt"
  sed 's/^[[:space:]]*//;s/[[:space:]]*$//' "$RAW/${name}.stdout" >"$RAW/${name}.out" || true
  return "$code"
}

fail() {
  printf '%s\n' "$1" | tee "$RAW/critical-fail.txt" >&2
  exit 1
}

pre_clean_owned() {
  if docker container inspect "$CONTAINER" >/dev/null 2>&1; then
    owner="$(docker inspect -f '{{index .Config.Labels "owner"}}' "$CONTAINER")"
    if [[ "$owner" != "dl01-w1-10" ]]; then
      fail "refusing foreign container ${CONTAINER} owner=${owner}"
    fi
    docker rm -f "$CONTAINER" >"$RAW/pre-clean-container.txt" 2>&1
  fi
  if docker volume inspect "$VOLUME" >/dev/null 2>&1; then
    owner="$(docker volume inspect -f '{{index .Labels "owner"}}' "$VOLUME")"
    if [[ "$owner" != "dl01-w1-10" ]]; then
      fail "refusing foreign volume ${VOLUME}"
    fi
    docker volume rm "$VOLUME" >"$RAW/pre-clean-volume.txt" 2>&1 || true
  fi
  if docker network inspect "$NETWORK" >/dev/null 2>&1; then
    owner="$(docker network inspect -f '{{index .Labels "owner"}}' "$NETWORK")"
    if [[ "$owner" != "dl01-w1-10" ]]; then
      fail "refusing foreign network ${NETWORK}"
    fi
    docker network rm "$NETWORK" >"$RAW/pre-clean-network.txt" 2>&1 || true
  fi
}

dba_sql() {
  local name="$1"
  local sql="$2"
  shift 2
  local extra=""
  if [[ $# -gt 0 ]]; then
    extra="$*"
  fi
  run_phase "$name" docker exec -i -u oracle \
    -e ORACLE_HOME="$OH" \
    -e NLS_LANG=AMERICAN_AMERICA.AL32UTF8 \
    "$CONTAINER" \
    bash -c 'export PATH="$ORACLE_HOME/bin:$PATH"; export LD_LIBRARY_PATH="$ORACLE_HOME/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"; exec sqlplus -s /nolog' <<EOF
set trimout on
set tab off
set echo off
set feedback off
set heading off
set pagesize 0
set linesize 500
whenever sqlerror exit sql.sqlcode
connect / as sysdba
@/opt/oo/sql/${sql} ${extra}
exit
EOF
}

reader_sql() {
  local name="$1"
  local user="$2"
  local sql="$3"
  local arg="${4:-}"
  if [[ -n "$arg" ]]; then
    run_phase "$name" docker exec -u oracle \
      -e ORACLE_HOME="$OH" \
      -e NLS_LANG=AMERICAN_AMERICA.AL32UTF8 \
      "$CONTAINER" \
      bash /opt/oo/scripts/sqlplus-user.sh "$user" "/opt/oo/sql/${sql}" "$arg"
  else
    run_phase "$name" docker exec -u oracle \
      -e ORACLE_HOME="$OH" \
      -e NLS_LANG=AMERICAN_AMERICA.AL32UTF8 \
      "$CONTAINER" \
      bash /opt/oo/scripts/sqlplus-user.sh "$user" "/opt/oo/sql/${sql}"
  fi
}

{
  echo "=== date ==="
  date -u
  echo "=== uname ==="
  uname -a
  echo "=== sw_vers ==="
  sw_vers || true
  echo "=== git_head ==="
  git -C "$ROOT" rev-parse HEAD
  echo "=== base ==="
  git -C "$ROOT" rev-parse 577160c22de63adb9b38e0147f3bc2da893c4d9d
  echo "=== sqlplus_host ==="
  if command -v sqlplus >/dev/null 2>&1; then command -v sqlplus; else echo sqlplus_absent_on_host; fi
  echo "=== linux_msrv ==="
  echo pending_until_rust_consumer
  echo "=== rust_image_not_started ==="
  docker image inspect "docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5" --format 'id={{.Id}} arch={{.Architecture}} os={{.Os}} size={{.Size}}' || echo rust_image_missing
} >"$RAW/preflight-host.txt" 2>&1

python3 - <<'PY'
import os
import secrets
import string
from pathlib import Path
root = Path("/tmp/oo-dl01w1-10-secrets")
root.mkdir(parents=True, exist_ok=True)
os.chmod(root, 0o755)
alphabet = string.ascii_letters + string.digits
for name in ("oracle-passwd", "oo_reader-passwd", "oo_noflash-passwd"):
    token = "Aa1" + "".join(secrets.choice(alphabet) for _ in range(24))
    path = root / name
    path.write_bytes(token.encode("ascii"))
    os.chmod(path, 0o644)
print("secrets_written", len(list(root.iterdir())))
PY

if ! python3 "$SRC/scripts/admit.py" snapshot "$RAW/admission-before-pull.json"; then
  python3 - <<'PY'
import json
from pathlib import Path
snap = json.loads(Path("/Users/redux/puppy-one/open-ontology/evidence/18da73a5-ad34-477a-8edf-0dc51122c1dd/DL01/oracle/raw/admission-before-pull.json").read_text())
pending = {"status": "pending-capacity", "reason": "admission refused before pull", "snapshot": snap}
path = Path("/Users/redux/puppy-one/open-ontology/evidence/18da73a5-ad34-477a-8edf-0dc51122c1dd/DL01/oracle/pending-capacity-receipt.json")
path.write_text(json.dumps(pending, indent=2) + "\n")
print("pending-capacity")
PY
  exit 3
fi

if ! run_phase docker-pull docker pull "$IMAGE"; then
  fail "docker pull failed"
fi

docker image inspect "$IMAGE" --format 'architecture={{.Architecture}}
os={{.Os}}
id={{.Id}}
size={{.Size}}
created={{.Created}}
digests={{json .RepoDigests}}' >"$RAW/image-inspect.txt"
if ! grep -q 'architecture=arm64' "$RAW/image-inspect.txt"; then
  fail "image architecture is not arm64"
fi

pre_clean_owned

if ! python3 "$SRC/scripts/admit.py" start "$SRC" "$EVIDENCE" "$SECRETS"; then
  code=$?
  if [[ "$code" -eq 3 ]]; then
    exit 3
  fi
  fail "docker start failed with ${code}"
fi

ready=0
for i in $(seq 1 90); do
  status="$(docker inspect -f '{{.State.Status}}' "$CONTAINER" 2>/dev/null || echo missing)"
  printf '%s %s\n' "$i" "$status" >>"$RAW/startup-status.txt"
  if [[ "$status" == "exited" || "$status" == "dead" || "$status" == "missing" ]]; then
    fail "container status ${status}"
  fi
  if [[ -z "$OH" ]]; then
    OH="$(docker exec -u oracle "$CONTAINER" bash -c 'ls -d /opt/oracle/product/*/dbhome* 2>/dev/null' | head -1 | tr -d '\r' || true)"
    if [[ -n "$OH" ]]; then
      printf '%s\n' "$OH" >"$RAW/oracle-home.txt"
    fi
  fi
  if [[ -n "$OH" ]]; then
    dba_sql "ping-${i}" ping.sql || true
    if grep -q 'pdb|FREEPDB1|READ WRITE' "$RAW/ping-${i}.out"; then
      if docker exec -u oracle -e ORACLE_HOME="$OH" "$CONTAINER" \
        bash -c 'export PATH="$ORACLE_HOME/bin:$PATH"; export LD_LIBRARY_PATH="$ORACLE_HOME/lib"; lsnrctl status' \
        >"$RAW/listener.txt" 2>"$RAW/listener.err"; then
        if grep -qi 'freepdb1' "$RAW/listener.txt"; then
          ready=1
          break
        fi
      fi
    fi
  fi
  sleep 10
done
if [[ "$ready" -ne 1 ]]; then
  fail "database was not ready"
fi

docker inspect "$CONTAINER" --format 'name={{.Name}} status={{.State.Status}} memory={{.HostConfig.Memory}} nano_cpus={{.HostConfig.NanoCpus}} shm={{.HostConfig.ShmSize}} image={{.Config.Image}} labels={{json .Config.Labels}}' >"$RAW/container-inspect.txt"
docker stats --no-stream --format '{{.Name}} {{.MemUsage}} {{.MemPerc}} {{.CPUPerc}}' "$CONTAINER" >"$RAW/docker-stats.txt" || true

if ! run_phase native-client docker exec -u oracle -e ORACLE_HOME="$OH" "$CONTAINER" \
  bash -c 'export PATH="$ORACLE_HOME/bin:$PATH"; export LD_LIBRARY_PATH="$ORACLE_HOME/lib"; sqlplus -version'; then
  fail "sqlplus version failed"
fi
cp "$RAW/native-client.out" "$RAW/native-client.txt"

if ! docker exec -u oracle "$CONTAINER" bash -c 'touch /evidence/raw/.write-probe && rm /evidence/raw/.write-probe'; then
  fail "evidence mount is not writable by the oracle user"
fi

if ! dba_sql dba-setup dba-setup.sql; then
  fail "dba setup failed"
fi
if ! run_phase create-users docker exec -u oracle -e ORACLE_HOME="$OH" -e NLS_LANG=AMERICAN_AMERICA.AL32UTF8 "$CONTAINER" \
  bash /opt/oo/scripts/create-users.sh; then
  fail "create users failed"
fi
if ! dba_sql dba-grants dba-grants.sql; then
  fail "grants failed"
fi

dba_sql dba-observe dba-observe.sql || true
cp "$RAW/dba-observe.out" "$RAW/observe-before.out"
dba_sql dba-watermark dba-watermark.sql || true
WM="$(sed -n 's/^watermark|//p' "$RAW/dba-watermark.out" | head -1 | tr -d '[:space:]')"
printf '%s\n' "$WM" >"$RAW/watermark.txt"
if [[ ! "$WM" =~ ^[0-9]{17}$ ]]; then
  fail "watermark missing"
fi
sleep 1
dba_sql mods-baseline dba-mods.sql || true
reader_sql reader-positive oo_reader reader-positive.sql || true
dba_sql mods-after dba-mods.sql || true
dba_sql observe-after-read dba-observe.sql || true
sleep 2
dba_sql audit-positive dba-audit.sql "$WM" || true
dba_sql dba-scn-mutate dba-scn-mutate.sql || true
SCN="$(sed -n 's/^scn_before|//p' "$RAW/dba-scn-mutate.out" | head -1 | tr -d '[:space:]')"
printf '%s\n' "$SCN" >"$RAW/scn-before.txt"
sleep 3
if [[ "$SCN" =~ ^[0-9]+$ ]]; then
  reader_sql reader-scn oo_reader reader-scn.sql "$SCN" || true
  reader_sql noflash-negative oo_noflash noflash-negative.sql "$SCN" || true
else
  echo "scn_missing" >"$RAW/scn-before.txt"
fi
reader_sql reader-stale oo_reader reader-stale.sql || true
reader_sql reader-negative oo_reader reader-negative.sql || true
dba_sql audit-after-negative dba-audit.sql "$WM" || true

wait_reader_clear() {
  local label="$1"
  local i
  for i in $(seq 1 30); do
    dba_sql "cancel-clear-${label}-${i}" dba-find-cancel.sql || true
    if ! grep -q '^sess|' "$RAW/cancel-clear-${label}-${i}.out"; then
      return 0
    fi
    sleep 1
  done
  echo "${label} previous_reader_still_open" >>"$RAW/cancel-attempt.txt"
  return 1
}

cancel_one() {
  local label="$1"
  local sql="$2"
  local spool="/evidence/raw/cancel-reader-${label}.out"
  if ! wait_reader_clear "$label"; then
    return 1
  fi
  : >"$RAW/cancel-reader-${label}.out"
  if ! docker exec -d -u oracle \
    -e ORACLE_HOME="$OH" \
    -e NLS_LANG=AMERICAN_AMERICA.AL32UTF8 \
    -e "SPOOL=${spool}" \
    "$CONTAINER" \
    bash /opt/oo/scripts/sqlplus-user.sh oo_reader "/opt/oo/sql/${sql}"; then
    echo "${label} detach_failed" >>"$RAW/cancel-attempt.txt"
    return 1
  fi
  local found=""
  local i line sid serial
  for i in $(seq 1 20); do
    dba_sql "cancel-find-${label}-${i}" dba-find-cancel.sql || true
    if grep -q '^marker|' "$RAW/cancel-find-${label}-${i}.out"; then
      found="$RAW/cancel-find-${label}-${i}.out"
      break
    fi
    if [[ "$i" -ge 8 ]] && grep -q '^active|' "$RAW/cancel-find-${label}-${i}.out"; then
      found="$RAW/cancel-find-${label}-${i}.out"
      break
    fi
    sleep 1
  done
  if [[ -z "$found" ]]; then
    echo "${label} no_live_session" >>"$RAW/cancel-attempt.txt"
    return 1
  fi
  line="$(grep '^marker|' "$found" | head -1 || true)"
  if [[ -z "$line" ]]; then
    line="$(grep '^active|' "$found" | head -1 || true)"
  fi
  sid="$(printf '%s' "$line" | cut -d'|' -f2 | tr -d '[:space:]')"
  serial="$(printf '%s' "$line" | cut -d'|' -f3 | tr -d '[:space:]')"
  echo "${label} sid=${sid} serial=${serial} line=${line}" >>"$RAW/cancel-attempt.txt"
  if ! dba_sql "cancel-exec-${label}" dba-cancel-exec.sql "$sid" "$serial"; then
    dba_sql "cancel-exec-pdb-${label}" dba-cancel-exec-pdb.sql "$sid" "$serial" || true
  fi
  local n
  for n in $(seq 1 30); do
    if grep -q 'ORA-01013' "$RAW/cancel-reader-${label}.out" 2>/dev/null; then
      echo "${label} observed_01013" >>"$RAW/cancel-attempt.txt"
      return 0
    fi
    sleep 1
  done
  echo "${label} cancel_not_observed" >>"$RAW/cancel-attempt.txt"
  return 1
}

: >"$RAW/cancel-attempt.txt"
if ! cancel_one cartesian reader-cancel.sql; then
  if ! cancel_one sleep reader-cancel-sleep.sql; then
    cancel_one loop reader-cancel-loop.sql || true
  fi
fi
dba_sql cancel-check dba-cancel-check.sql || true
docker stats --no-stream --format '{{.Name}} {{.MemUsage}} {{.MemPerc}} {{.CPUPerc}}' "$CONTAINER" >>"$RAW/docker-stats.txt" || true

python3 "$SRC/scripts/redact.py" "$EVIDENCE" || true
set +e
python3 "$SRC/scripts/judge.py" "$EVIDENCE" "$SRC/fixtures/expected.json" "$SRC/sql" >"$RAW/judge-stdout.txt" 2>"$RAW/judge-stderr.txt"
judge_code=$?
set -e
printf '%s\n' "$judge_code" >"$RAW/judge.exit"
JUDGED=1
exit "$judge_code"
