#!/bin/bash
# DL01-W1-34 Oracle thin-driver preflight. Owned containers only.
set -euo pipefail

ROOT="/Users/redux/puppy-one/open-ontology"
SRC="$ROOT/testing/dl01-driver-preflight/oracle"
EVIDENCE="$ROOT/evidence/4264ed49-a669-48b2-a819-dd18a00b1f27/DL01/oracle-driver-preflight"
SECRETS="/tmp/oo-dl01w1-34-secrets"
PREFIX="oo-dl01w1-34"
ORA="${PREFIX}-oracle"
RUST="${PREFIX}-rust"
RUST_IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
ORA_IMAGE="docker.io/gvenzl/oracle-free@sha256:cdc5b3a3685da4f1785daeee24f74de8a9768b7629192ea9abdaeeea343d12a8"
EXPECT_SUM="d729181747bd69ba53cde2956ea9ddc06cf09d0badaf4a9e89944b7c0b7b4397"
ADMIT="$SRC/scripts/admit.py"
LABELS=(
  --label project=open-ontology
  --label owner=dl01-w1-34
  --label "session=4264ed49-a669-48b2-a819-dd18a00b1f27"
  --label task=DL01-W1-34
)

mkdir -p "$EVIDENCE/raw/build" "$EVIDENCE/raw/oracle" "$EVIDENCE/raw/probe"
oracle_ready=0
final_ok=0
cleanup_done=0

rand_pw() {
  printf 'Aa1%s' "$(openssl rand -hex 12)"
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
            if text and text != "absent":
                secrets.append(text)
data = sys.stdin.read()
for secret in secrets:
    data = data.replace(secret, "[redacted]")
sys.stdout.write(data)
PY
}

owned_ids() {
  docker ps -aq --filter "name=${PREFIX}"
}

remove_owned_containers() {
  local id name owner
  for id in $(owned_ids); do
    name=$(docker inspect -f '{{.Name}}' "$id")
    owner=$(docker inspect -f '{{index .Config.Labels "owner"}}' "$id")
    if [[ "$owner" == "dl01-w1-34" ]]; then
      docker rm -f "$id" >/dev/null
      echo "removed ${name}"
    fi
  done
}

sql_sys() {
  local script="$1"
  local dest="$2"
  shift 2
  docker exec -e NLS_LANG=AMERICAN_AMERICA.AL32UTF8 "$ORA" \
    bash -lc "sqlplus -s / as sysdba @${script} $*" >"$dest" 2>&1
}

admit_wait() {
  local kind="$1"
  local dest="$2"
  local attempt
  for attempt in $(seq 1 45); do
    if python3 "$ADMIT" acquire "$kind" "$dest"; then
      return 0
    fi
    echo "capacity wait ${kind} attempt ${attempt}" >&2
    sleep 20
  done
  echo "capacity denied for ${kind}" >&2
  return 3
}

cleanup() {
  if [[ "$cleanup_done" == "1" ]]; then
    return
  fi
  cleanup_done=1
  set +e
  if [[ "$oracle_ready" == "1" ]]; then
    sql_sys /opt/oo/sql/dba-cleanup.sql "$EVIDENCE/raw/oracle/dba-cleanup.out"
  fi
  if docker inspect "$ORA" >/dev/null 2>&1; then
    docker logs "$ORA" 2>&1 | redact_stream >"$EVIDENCE/raw/oracle/docker-logs.txt" || true
    docker inspect "$ORA" | redact_stream >"$EVIDENCE/raw/oracle/docker-inspect.json" || true
  fi
  if docker inspect "$RUST" >/dev/null 2>&1; then
    docker inspect "$RUST" | redact_stream >"$EVIDENCE/raw/build/rust-inspect-last.json" || true
  fi
  remove_owned_containers
  docker volume rm "${PREFIX}-oradata" >/dev/null 2>&1 || true
  if [[ "$final_ok" == "1" ]]; then
    docker volume rm "${PREFIX}-cargo" "${PREFIX}-target" >/dev/null 2>&1 || true
  fi
  docker network rm "${PREFIX}-net" >/dev/null 2>&1 || true
  rm -rf "$SECRETS"
  python3 "$ADMIT" release all >/dev/null 2>&1 || true
  python3 - <<PY
import json, pathlib
final_ok = int("$final_ok")
path = pathlib.Path("$EVIDENCE/resource-receipt.json")
body = {
    "task_id": "DL01-W1-34",
    "prefix": "$PREFIX",
    "promoted": final_ok == 1,
    "kept_cargo_and_target_volumes": final_ok != 1,
    "images_retained": ["$RUST_IMAGE", "$ORA_IMAGE"],
    "secrets_removed": True,
    "foreign_containers_untouched": True,
}
path.write_text(json.dumps(body, indent=2) + "\n")
PY
}

trap cleanup EXIT

python3 "$ADMIT" release all >/dev/null
remove_owned_containers
docker volume rm "${PREFIX}-oradata" >/dev/null 2>&1 || true
docker image inspect "$RUST_IMAGE" >/dev/null
docker image inspect "$ORA_IMAGE" >/dev/null
if ! docker network inspect "${PREFIX}-net" >/dev/null 2>&1; then
  docker network create \
    --label project=open-ontology --label owner=dl01-w1-34 \
    --label session=4264ed49-a669-48b2-a819-dd18a00b1f27 --label task=DL01-W1-34 \
    "${PREFIX}-net" >/dev/null
fi
docker volume inspect "${PREFIX}-cargo" >/dev/null
if ! docker volume inspect "${PREFIX}-target" >/dev/null 2>&1; then
  docker volume create "${PREFIX}-target" \
    --label project=open-ontology --label owner=dl01-w1-34 \
    --label session=4264ed49-a669-48b2-a819-dd18a00b1f27 --label task=DL01-W1-34 >/dev/null
fi
docker volume create "${PREFIX}-oradata" \
  --label project=open-ontology --label owner=dl01-w1-34 \
  --label session=4264ed49-a669-48b2-a819-dd18a00b1f27 --label task=DL01-W1-34 >/dev/null

umask 077
rm -rf "$SECRETS"
mkdir -m 700 "$SECRETS"
printf '%s' "$(rand_pw)" >"$SECRETS/oracle-passwd"
printf '%s' "$(rand_pw)" >"$SECRETS/OO34_READER-passwd"
printf '%s' "$(rand_pw)" >"$SECRETS/OO34_NOFLASH-passwd"
printf '%s' absent >"$SECRETS/tls-ca-ref"
chmod 600 "$SECRETS"/*

admit_wait rust "$EVIDENCE/raw/build/admission.json"
set +e
docker run --name "$RUST" \
  --platform linux/arm64 --pull never \
  --network "${PREFIX}-net" \
  --memory 6g --memory-swap 6g --cpus 1 \
  -e CARGO_BUILD_JOBS=1 \
  -e CARGO_HOME=/cargo \
  -e CARGO_TERM_COLOR=never \
  -e CARGO_TARGET_DIR=/target \
  -v "${PREFIX}-cargo:/cargo" \
  -v "${PREFIX}-target:/target" \
  -v "$SRC:/src" \
  -v "$EVIDENCE/raw/build:/evidence" \
  "${LABELS[@]}" --label kind=rust-builder \
  "$RUST_IMAGE" \
  bash /src/scripts/build-inside.sh
build_rc=$?
set -e
echo "$build_rc" >"$EVIDENCE/raw/build/docker-build.exit"
docker rm "$RUST" >/dev/null 2>&1 || true
python3 "$ADMIT" release rust >/dev/null
if [[ "$build_rc" != "0" ]]; then
  echo "locked build failed" >&2
  exit 1
fi
python3 - "$SRC/Cargo.lock" "$EXPECT_SUM" "$EVIDENCE/raw/build/oracledb-lock-checksum.txt" <<'PY'
import pathlib, sys
text = pathlib.Path(sys.argv[1]).read_text()
expect = sys.argv[2]
blocks = text.split("[[package]]")
found = ""
version = ""
for block in blocks:
    if '\nname = "oracledb"' in block or block.startswith('\nname = "oracledb"') or 'name = "oracledb"' in block.split("\n", 3)[0:4].__repr__():
        pass
    lines = block.strip().splitlines()
    if not lines or lines[0] != 'name = "oracledb"':
        continue
    for line in lines:
        if line.startswith("version = "):
            version = line.split("=", 1)[1].strip().strip('"')
        if line.startswith("checksum = "):
            found = line.split("=", 1)[1].strip().strip('"')
    break
pathlib.Path(sys.argv[3]).write_text(f"version={version}\nchecksum={found}\nmatch={found == expect}\n")
if found != expect or version != "26.0.0-beta.4":
    raise SystemExit(f"oracledb lock {version} {found}")
PY

admit_wait oracle "$EVIDENCE/raw/oracle/admission.json"
docker run -d --name "$ORA" \
  --platform linux/arm64 --pull never \
  --network "${PREFIX}-net" \
  --memory 6g --memory-swap 6g --cpus 1 \
  --shm-size 1g \
  -e ORACLE_PASSWORD_FILE=/run/secrets/oracle-passwd \
  -e TZ=UTC \
  -v "${PREFIX}-oradata:/opt/oracle/oradata" \
  -v "$SECRETS:/run/secrets:ro" \
  -v "$SRC:/opt/oo:ro" \
  "${LABELS[@]}" --label kind=oracle-database-free \
  "$ORA_IMAGE" >"$EVIDENCE/raw/oracle/docker-run.stdout"
echo "$?" >"$EVIDENCE/raw/oracle/docker-run.exit"

ready=0
for _ in $(seq 1 90); do
  running=$(docker inspect -f '{{.State.Running}}' "$ORA" 2>/dev/null || echo false)
  if [[ "$running" != "true" ]]; then
    echo "oracle container is not running" >&2
    exit 1
  fi
  docker exec "$ORA" bash -lc 'sqlplus -s / as sysdba @/opt/oo/sql/ping.sql' \
    >"$EVIDENCE/raw/oracle/ping.out" 2>"$EVIDENCE/raw/oracle/ping.err" || true
  if grep -q 'pdb|FREEPDB1|READ WRITE' "$EVIDENCE/raw/oracle/ping.out"; then
    docker exec "$ORA" bash -lc 'lsnrctl status' >"$EVIDENCE/raw/oracle/listener.out" 2>&1 || true
    if grep -qi freepdb1 "$EVIDENCE/raw/oracle/listener.out"; then
      ready=1
      break
    fi
  fi
  sleep 10
done
if [[ "$ready" != "1" ]]; then
  echo "oracle did not become ready" >&2
  exit 1
fi
oracle_ready=1
docker exec "$ORA" bash -lc 'printf "%s\n" "$ORACLE_HOME"' >"$EVIDENCE/raw/oracle/oracle-home.txt"

docker exec -u 0 "$ORA" mkdir -p /home/oracle/oo-secrets
docker cp "$SECRETS/OO34_READER-passwd" "$ORA":/home/oracle/oo-secrets/OO34_READER-passwd >/dev/null
docker cp "$SECRETS/OO34_NOFLASH-passwd" "$ORA":/home/oracle/oo-secrets/OO34_NOFLASH-passwd >/dev/null
docker cp "$SRC/scripts/create-users.sh" "$ORA":/home/oracle/oo-secrets/create-users.sh >/dev/null
docker exec -u 0 "$ORA" bash -lc 'g=$(id -gn oracle); chown -R oracle:"$g" /home/oracle/oo-secrets; chmod 700 /home/oracle/oo-secrets; chmod 600 /home/oracle/oo-secrets/*; chmod 700 /home/oracle/oo-secrets/create-users.sh'
sql_sys /opt/oo/sql/dba-setup.sql "$EVIDENCE/raw/oracle/dba-setup.out"
docker exec -u oracle -e NLS_LANG=AMERICAN_AMERICA.AL32UTF8 "$ORA" \
  bash -lc '/home/oracle/oo-secrets/create-users.sh' \
  >"$EVIDENCE/raw/oracle/create-users.out" 2>&1
sql_sys /opt/oo/sql/dba-grants.sql "$EVIDENCE/raw/oracle/dba-grants.out"
sql_sys /opt/oo/sql/dba-scn-anchor.sql "$EVIDENCE/raw/oracle/dba-scn-anchor.out"
sql_sys /opt/oo/sql/dba-observe.sql "$EVIDENCE/raw/oracle/dba-observe.out"
sql_sys /opt/oo/sql/dba-watermark.sql "$EVIDENCE/raw/oracle/watermark.out"
mark=$(grep -E '^[0-9]{20}$' "$EVIDENCE/raw/oracle/watermark.out" | head -1)
if [[ -z "${mark}" ]]; then
  echo "watermark missing" >&2
  exit 1
fi
printf '%s\n' "$mark" >"$EVIDENCE/raw/oracle/watermark.txt"

docker exec -e NLS_LANG=AMERICAN_AMERICA.AL32UTF8 "$ORA" \
  bash -lc 'sqlplus -s / as sysdba @/opt/oo/sql/dba-observe-loop.sql' \
  >"$EVIDENCE/raw/oracle/observe-loop.out" 2>&1 &
obs_pid=$!

admit_wait rust "$EVIDENCE/raw/probe/admission.json"
docker run -d --name "$RUST" \
  --platform linux/arm64 --pull never \
  --network "${PREFIX}-net" \
  --memory 6g --memory-swap 6g --cpus 1 \
  -v "${PREFIX}-target:/target:ro" \
  -v "$SRC:/src:ro" \
  -v "$SECRETS:/secrets:ro" \
  -v "$EVIDENCE:/evidence" \
  "${LABELS[@]}" --label kind=rust-builder \
  "$RUST_IMAGE" sleep 7200 >"$EVIDENCE/raw/probe/rust-run.stdout"

rm -f "$EVIDENCE/raw/probe/dba-mutated.flag" "$EVIDENCE/raw/probe/scn.txt" \
  "$EVIDENCE/raw/probe/batch.arrow" "$EVIDENCE/raw/probe/batch.arrow.partial"
docker exec "$RUST" /target/debug/oracle-driver-probe business \
  --secrets /secrets --out /evidence/raw/probe \
  --host "$ORA" --gold /src/fixtures/gold.json \
  >"$EVIDENCE/raw/probe/business.stdout" 2>"$EVIDENCE/raw/probe/business.stderr" &
probe_pid=$!

scn=""
for _ in $(seq 1 240); do
  if [[ -f "$EVIDENCE/raw/probe/scn.txt" ]]; then
    scn=$(tr -cd '0-9' <"$EVIDENCE/raw/probe/scn.txt")
    break
  fi
  if ! kill -0 "$probe_pid" 2>/dev/null; then
    break
  fi
  sleep 0.5
done
if [[ -n "$scn" ]]; then
  sql_sys /opt/oo/sql/dba-mutate.sql "$EVIDENCE/raw/oracle/dba-mutate.out"
  printf 'mutated\n' >"$EVIDENCE/raw/probe/dba-mutated.flag"
fi
set +e
wait "$probe_pid"
echo $? >"$EVIDENCE/raw/probe/business.exit"
set -e

sleep 2
sql_sys /opt/oo/sql/dba-obs-max.sql "$EVIDENCE/raw/oracle/obs-business-max.txt"
if [[ -f "$EVIDENCE/raw/probe/cancel-start.txt" ]]; then
  pair=$(tr -cd '0-9,' <"$EVIDENCE/raw/probe/cancel-start.txt")
  sid=${pair%%,*}
  serial=${pair##*,}
  if [[ "$sid" =~ ^[0-9]+$ && "$serial" =~ ^[0-9]+$ ]]; then
    sql_sys /opt/oo/sql/dba-session-status.sql "$EVIDENCE/raw/oracle/dba-session-status.out" "$sid" "$serial"
    if grep -q 'status|ACTIVE|' "$EVIDENCE/raw/oracle/dba-session-status.out"; then
      sql_sys /opt/oo/sql/dba-cancel-exec.sql "$EVIDENCE/raw/oracle/dba-cancel.out" "$sid" "$serial" || true
    fi
  fi
fi

set +e
docker exec "$RUST" /target/debug/oracle-driver-probe negative \
  --secrets /secrets --out /evidence/raw/probe --host "$ORA" \
  >"$EVIDENCE/raw/probe/negative.stdout" 2>"$EVIDENCE/raw/probe/negative.stderr"
echo $? >"$EVIDENCE/raw/probe/negative.exit"
if [[ -f "$EVIDENCE/raw/probe/batch.arrow.partial" ]]; then
  docker exec "$RUST" /target/debug/oracle-arrow-decode \
    --arrow /evidence/raw/probe/batch.arrow.partial \
    --gold /src/fixtures/gold.json \
    --out /evidence/raw/probe/arrow-decode.json \
    >"$EVIDENCE/raw/probe/decode.stdout" 2>"$EVIDENCE/raw/probe/decode.stderr"
  echo $? >"$EVIDENCE/raw/probe/decode.exit"
else
  echo "no partial" >"$EVIDENCE/raw/probe/decode.stdout"
  echo 2 >"$EVIDENCE/raw/probe/decode.exit"
fi
set -e

sql_sys /opt/oo/sql/dba-observe-stop.sql "$EVIDENCE/raw/oracle/dba-observe-stop.out" || true
set +e
wait "$obs_pid"
set -e
sql_sys /opt/oo/sql/dba-obs-dump.sql "$EVIDENCE/raw/oracle/dba-obs.out"
sql_sys /opt/oo/sql/dba-audit.sql "$EVIDENCE/raw/oracle/dba-audit.out" "$mark"
python3 "$SRC/scripts/classify.py" "$EVIDENCE/raw" >"$EVIDENCE/raw/classification.stdout"

set +e
python3 - <<PY
import json, pathlib, shutil
raw = pathlib.Path("$EVIDENCE/raw")
cls = json.loads((raw / "classification.json").read_text())
business = (raw / "probe" / "business.exit").read_text().strip()
decode = (raw / "probe" / "decode.exit").read_text().strip()
negative = (raw / "probe" / "negative.exit").read_text().strip()
partial = raw / "probe" / "batch.arrow.partial"
ready = {
    "business_exit": int(business),
    "decode_exit": int(decode),
    "negative_exit": int(negative),
    "reader_peak": cls.get("business_reader_peak"),
    "cancel_client_timeout": cls.get("cancel_client_timeout"),
    "cancel_server_seen_active": cls.get("cancel_server_seen_active"),
    "cancel_server_still_active": cls.get("cancel_server_still_active"),
    "dba_kill_used": cls.get("dba_kill_used"),
    "platform_dml_success": cls.get("platform_dml_success"),
    "negative_denied": cls.get("negative_denied"),
    "audit_unknown": cls.get("audit_unknown"),
}
promote = (
    ready["business_exit"] == 0
    and ready["decode_exit"] == 0
    and ready["negative_exit"] == 0
    and ready["reader_peak"] == 1
    and ready["cancel_client_timeout"] is True
    and ready["cancel_server_seen_active"] is True
    and ready["cancel_server_still_active"] is False
    and ready["dba_kill_used"] is False
    and ready["negative_denied"] is True
    and not ready["platform_dml_success"]
    and not ready["audit_unknown"]
)
ready["promoted"] = promote
if promote and partial.is_file():
    shutil.move(str(partial), str(raw / "probe" / "batch.arrow"))
    ready["arrow_file"] = "batch.arrow"
else:
    ready["arrow_file"] = "batch.arrow.partial" if partial.is_file() else None
ready_body = {"ready": promote, "reason": ready}
(raw / "probe" / "arrow-ready.json").write_text(json.dumps(ready_body, indent=2) + "\n")
pathlib.Path("$EVIDENCE/raw/run-result.json").write_text(json.dumps(ready, indent=2) + "\n")
pathlib.Path("$EVIDENCE/raw/promote.exit").write_text("0\n" if promote else "2\n")
PY
set -e
if [[ -f "$EVIDENCE/raw/probe/batch.arrow" ]]; then
  shasum -a 256 "$EVIDENCE/raw/probe/batch.arrow" >"$EVIDENCE/raw/probe/batch.arrow.sha256"
elif [[ -f "$EVIDENCE/raw/probe/batch.arrow.partial" ]]; then
  shasum -a 256 "$EVIDENCE/raw/probe/batch.arrow.partial" >"$EVIDENCE/raw/probe/batch.arrow.sha256"
fi
promote_rc=$(tr -cd '0-9' <"$EVIDENCE/raw/promote.exit")
if [[ "$promote_rc" == "0" ]]; then
  final_ok=1
fi
exit "$promote_rc"
