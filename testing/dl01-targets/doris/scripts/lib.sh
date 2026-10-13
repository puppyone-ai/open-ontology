#!/bin/bash
# Shared helpers for the Doris target preparation. No secret values are printed.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO="$(cd "$ROOT/../../.." && pwd)"
# shellcheck disable=SC1091
source "$ROOT/topology.env"

SECRETS="${SECRETS:-/tmp/oo-dl01w1-12-secrets}"
RUNDIR="${RUNDIR:-/tmp/oo-dl01w1-12-run}"
EVIDENCE="$REPO/evidence/${SESSION}/DL01/doris"
RAW="$EVIDENCE/raw"
LOCK_RESOURCES="/tmp/oo-dl01-wave1-resources.lock"
LOCK_GIT="/tmp/oo-dl01-wave1-git.lock"

mkdir -p "$RAW" "$RUNDIR"

labels=(
  --label "oo.project=${PROJECT}"
  --label "oo.owner=${OWNER}"
  --label "oo.session=${SESSION}"
  --label "oo.prefix=${PREFIX}"
  --label "oo.task=${OWNER}"
)

mysql_client() {
  local cnf="$1" sql="$2" out="$3" err="$4" exitf="$5"
  # Sixth arg unset means the fixture database. An explicit empty string connects
  # without a default schema, which is required before CREATE DATABASE.
  local db="${6-$DB_NAME}"
  : >"$out"
  : >"$err"
  # Bash 3.2 treats an empty "${array[@]}" as unbound under set -u.
  local -a cmd
  cmd=(
    docker run --rm -i
    --platform "$CLIENT_PLATFORM"
    --network "$NETWORK"
    --memory "$CLIENT_MEMORY"
    "${labels[@]}"
    --label oo.kind=doris-protocol-client
    --entrypoint mysql
    -v "$cnf:/client.cnf:ro"
    "$CLIENT_REF"
    --defaults-extra-file=/client.cnf
    --batch
    --raw
    --default-character-set=utf8mb4
    --connect-timeout=15
  )
  if [[ -n "$db" ]]; then
    cmd+=("$db")
  fi
  set +e
  "${cmd[@]}" <"$sql" >"$out" 2>"$err"
  local ec=$?
  set -e
  printf '%s\n' "$ec" >"$exitf"
}

redact_evidence() {
  python3 - "$SECRETS" "$RAW" <<'PY'
import pathlib, sys
secrets = pathlib.Path(sys.argv[1])
raw = pathlib.Path(sys.argv[2])
passwords = []
if secrets.is_dir():
    for path in secrets.glob("*.password"):
        value = path.read_text(encoding="utf-8").strip()
        if value:
            passwords.append(value)
if not passwords or not raw.is_dir():
    raise SystemExit(0)
for path in raw.rglob("*"):
    if not path.is_file():
        continue
    data = path.read_bytes()
    try:
        text = data.decode("utf-8")
    except UnicodeDecodeError:
        continue
    updated = text
    for value in passwords:
        updated = updated.replace(value, "[redacted]")
    if updated != text:
        path.write_text(updated, encoding="utf-8")
PY
}

dump_runtime() {
  local dir="$RAW/runtime"
  mkdir -p "$dir"
  docker logs "$FE_NAME" >"$dir/fe-console.log" 2>"$dir/fe-console.err" || true
  docker logs "$BE_NAME" >"$dir/be-console.log" 2>"$dir/be-console.err" || true
  docker exec "$FE_NAME" sh -c 'echo === heap ===; grep -n "Xmx" /opt/apache-doris/fe/conf/fe.conf | head -n 5; echo === audit ===; if [ -f /opt/apache-doris/fe/log/fe.audit.log ]; then cat /opt/apache-doris/fe/log/fe.audit.log; else echo AUDIT_FILE_MISSING; ls -la /opt/apache-doris/fe/log || true; fi' \
    >"$dir/fe-audit.txt" 2>"$dir/fe-audit.err" || true
  docker exec "$BE_NAME" sh -c 'echo === conf ===; grep -nE "Xmx|^mem_limit|priority_networks" /opt/apache-doris/be/conf/be.conf; echo === out ===; tail -n 120 /opt/apache-doris/be/log/be.out 2>/dev/null || true' \
    >"$dir/be-status.txt" 2>"$dir/be-status.err" || true
  redact_evidence
}

write_cnf() {
  local path="$1" user="$2" password="$3"
  umask 077
  printf '[client]\nuser=%s\npassword=%s\nprotocol=TCP\nhost=%s\nport=%s\n' \
    "$user" "$password" "$FE_NAME" "$QUERY_PORT" >"$path"
  chmod 600 "$path"
}
