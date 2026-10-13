#!/bin/bash
# Live C4 window. Keeps volumes. Does not remove the rust smoke container or its volumes.
set -u
ROOT="/Users/redux/puppy-one/open-ontology"
EV="$ROOT/evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product/raw/run/product-run-14-c4"
ADMIT="$ROOT/testing/dl01-driver-preflight/clickhouse/scripts/admit.py"
SCRIPTS="$ROOT/tools/clickhouse-preview/scripts"
SECRETS="/tmp/oo-dl01w1-40-return/secrets"
LIMITS="$ROOT/testing/dl01-driver-preflight/clickhouse/config/limits.xml"
CH_IMAGE="clickhouse/clickhouse-server@sha256:4ea38f8345ad67dc437b41638804421ee6d21e6d8c6f794bf4b28305541de2aa"
PG_IMAGE="postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb"
mkdir -p "$EV"
exec >>"$EV/launcher.log" 2>&1
echo "C4_START $(date -u +%Y-%m-%dT%H:%M:%SZ)"
fail() {
  printf '%s\n' "$1" > "$EV/C4_FAIL"
  echo "C4_FAIL $1"
  exit 1
}
test -f "$SECRETS/reader.password" || fail "reader secret file missing"
test -f "$SECRETS/dba.password" || fail "dba secret file missing"
test -f "$SECRETS/users.d/oo-users.xml" || fail "users xml missing"
if docker ps -a --format '{{.Names}}' | grep -Eq '^oo-dl01w1-40-(clickhouse|pg)$'; then
  fail "clickhouse or postgres container already present"
fi
docker inspect oo-dl01w1-06-pg --format '{{.Id}} {{.State.Status}}' > "$EV/foreign-pg-before.txt" || fail "foreign pg missing"
docker network inspect oo-dl01w1-40-net >/dev/null 2>&1 || docker network create --subnet 172.16.40.0/24 --gateway 172.16.40.1 oo-dl01w1-40-net >/dev/null || fail "network"
cd "$SCRIPTS"
python3 -m unittest test_c4_capture.py > "$EV/unit.stdout" 2> "$EV/unit.stderr"
printf '%s\n' "$?" > "$EV/unit.exit"
test "$(tr -d '[:space:]' < "$EV/unit.exit")" = "0" || fail "unit tests"
python3 - <<'PY' > "$EV/blocked-interfaces.json"
import json, os, shutil, subprocess
root = "/Users/redux/puppy-one/open-ontology"
def rev(prefix):
    proc = subprocess.run(["git", "-C", root, "rev-parse", "--verify", prefix], text=True, capture_output=True)
    return proc.stdout.strip() if proc.returncode == 0 else proc.stderr.strip()
token = "/tmp/oo-customer-demo-20261007/llm-token"
body = {
    "llm_token_path": token,
    "llm_token_is_directory": os.path.isdir(token),
    "llm_token_is_file": os.path.isfile(token),
    "llm_called": False,
    "reason": "The approved bearer path is a directory. Root has asked for a legal file path. This run does not search for a token, invent one, or call the model.",
    "self_build_commit_probe": rev("13ad15b4"),
    "gateway_commit_probe": rev("ac896"),
    "source_bound_commit_probe": rev("9d922"),
    "oo_cli": shutil.which("oo"),
    "pack_written": False,
    "gateway_called": False,
    "agent_db_used_as_gateway": False,
}
print(json.dumps(body, indent=2))
PY
cleanup() {
  docker stop oo-dl01w1-40-clickhouse oo-dl01w1-40-pg >/dev/null 2>&1 || true
  docker rm oo-dl01w1-40-clickhouse oo-dl01w1-40-pg >/dev/null 2>&1 || true
  python3 "$ADMIT" release clickhouse >/dev/null 2>&1 || true
  python3 "$ADMIT" release postgres >/dev/null 2>&1 || true
  docker inspect oo-dl01w1-06-pg --format '{{.Id}} {{.State.Status}}' > "$EV/foreign-pg-after.txt" 2>/dev/null || true
  docker volume ls --filter name=oo-dl01w1-40 --format '{{.Name}}' > "$EV/volumes-kept.txt" 2>/dev/null || true
}
trap cleanup EXIT
acquire() {
  local kind="$1"
  shift
  local out="$EV/acquire-${kind}.json"
  if python3 "$ADMIT" acquire-start "$kind" "$out" -- "$@"; then
    return 0
  fi
  echo "REFUSED $kind"
  if [ "${WAITED:-0}" = "1" ]; then
    fail "second capacity refusal for $kind"
  fi
  WAITED=1
  echo "WAIT_900 $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  python3 -c 'import time; time.sleep(900)'
  python3 "$ADMIT" acquire-start "$kind" "$out" -- "$@" || fail "admission refused for $kind"
}
acquire clickhouse docker run -d --name oo-dl01w1-40-clickhouse \
  --platform linux/arm64 --pull never \
  --network oo-dl01w1-40-net \
  --memory 2g --memory-swap 2g --cpus 1 \
  --ulimit nofile=262144:262144 \
  --mount type=volume,source=oo-dl01w1-40-chdata,target=/var/lib/clickhouse \
  --mount type=bind,source="$LIMITS",target=/etc/clickhouse-server/config.d/oo-limits.xml,readonly \
  --mount type=bind,source="$SECRETS/users.d/oo-users.xml",target=/etc/clickhouse-server/users.d/oo-users.xml,readonly \
  --mount type=bind,source="$SECRETS/dba.password",target=/etc/oo/dba.password,readonly \
  --mount type=bind,source="$SECRETS/reader.password",target=/etc/oo/reader.password,readonly \
  --label oo.project=open-ontology \
  --label oo.owner=oo-dl01w1-40 \
  --label oo.session=4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de \
  --label oo.task=DL01-W1-40 \
  --label oo.kind=clickhouse-source \
  "$CH_IMAGE"
acquire postgres docker run -d --name oo-dl01w1-40-pg \
  --platform linux/arm64 --pull never \
  --network oo-dl01w1-40-net \
  --memory 1g --memory-swap 1g --cpus 1 \
  -e POSTGRES_HOST_AUTH_METHOD=trust \
  -e POSTGRES_USER=oo \
  -e POSTGRES_DB=oo_shadow \
  --mount type=volume,source=oo-dl01w1-40-pgdata,target=/var/lib/postgresql/data \
  --label oo.project=open-ontology \
  --label oo.owner=oo-dl01w1-40 \
  --label oo.session=4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de \
  --label oo.task=DL01-W1-40 \
  --label oo.kind=acceptance-shadow \
  "$PG_IMAGE"
echo "DATABASES_STARTED $(date -u +%Y-%m-%dT%H:%M:%SZ)"
export OO_C4_OUT="$EV"
export OO_C4_SECRET="$SECRETS/reader.password"
python3 "$SCRIPTS/run_c4_live.py"
code=$?
printf '%s\n' "$code" > "$EV/live.exit"
if [ "$code" != "0" ]; then
  fail "live exit $code"
fi
echo "C4_DONE $(date -u +%Y-%m-%dT%H:%M:%SZ)"
