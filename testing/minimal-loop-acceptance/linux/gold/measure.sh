#!/bin/bash
# Read six sealed-bulk gold cells from oo-a410-tgt. No DDL. No source secret.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EV="$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a407/SP11/minimal-loop-acceptance/gold"
SQL="$SRC/testing/minimal-loop-acceptance/linux/gold/measure.sql"
IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
NET=oo-a410-eef831aa-net
NAME=oo-a407-gold-read
CA=/tmp/oo-a410-eef831aa-dba4-4af2-a78b-541bc9e0a410/secrets/ca.crt
PASS=/tmp/oo-a410-eef831aa-dba4-4af2-a78b-541bc9e0a410/secrets/reader.pass
CA_SHA=db25c207b268e7ef968155884413ab66bd7550074321ede8b75e366f9eaf9d09
mkdir -p "$EV"
if [ -f "$EV/read.stdout" ]; then
  echo "refusing to overwrite gold read" >&2
  exit 2
fi
if [ ! -f "$CA" ] || [ ! -f "$PASS" ]; then
  echo "SHARE reader material is absent" >&2
  exit 1
fi
ca_got=$(shasum -a 256 "$CA" | awk '{print $1}')
if [ "$ca_got" != "$CA_SHA" ]; then
  echo "reader CA hash does not match SHARE" >&2
  exit 1
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned postgres image is not present locally; not pulling" >&2
  exit 1
fi
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  echo "a410 network is absent; not creating it" >&2
  exit 1
fi
printf '%s\n' "docker run --name ${NAME} --network ${NET} --cap-drop ALL --memory 256m --cpus 1 --entrypoint bash image=pinned-postgres-17.10" > "$EV/invocation.txt"
set +e
docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --entrypoint bash \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory 256m \
  --memory-swap 256m \
  --cpus 1 \
  --pids-limit 64 \
  --network "$NET" \
  --mount "type=bind,source=${CA},target=/secrets/ca.crt,readonly" \
  --mount "type=bind,source=${PASS},target=/secrets/reader.pass,readonly" \
  --mount "type=bind,source=${SQL},target=/sql/measure.sql,readonly" \
  --mount "type=bind,source=${EV},target=/out" \
  --label oo.project=open-ontology \
  --label oo.owner=a407 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a407 \
  --label oo.kind=target-reader \
  "$IMAGE" \
  -c 'umask 077
set -e
pass=$(cat /secrets/reader.pass)
printf "%s\n" "oo-a410-tgt:5432:mirrordb:oo_mirror_read:${pass}" > /tmp/.pgpass
chmod 600 /tmp/.pgpass
unset pass
export PGPASSFILE=/tmp/.pgpass
export PGSSLMODE=verify-full
export PGSSLROOTCERT=/secrets/ca.crt
export PGHOST=oo-a410-tgt
export PGPORT=5432
export PGDATABASE=mirrordb
export PGUSER=oo_mirror_read
psql -v ON_ERROR_STOP=1 -f /sql/measure.sql > /out/read.stdout
rm -f /tmp/.pgpass
' > "$EV/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EV/docker-run.exit"
docker inspect -f 'ExitCode={{.State.ExitCode}} OOMKilled={{.State.OOMKilled}} Privileged={{.HostConfig.Privileged}} NetworkMode={{.HostConfig.NetworkMode}} Memory={{.HostConfig.Memory}}' "$NAME" > "$EV/hostconfig.txt" 2>/dev/null || true
docker rm "$NAME" >/dev/null 2>&1 || true
if [ "$run_exit" -ne 0 ]; then
  echo "gold read failed" >&2
  exit "$run_exit"
fi
set -e
python3 - "$EV" <<'PY'
import pathlib, sys
ev = pathlib.Path(sys.argv[1])
rows = {}
for line in (ev / "read.stdout").read_text().splitlines():
    if "=" in line:
        key, value = line.split("=", 1)
        rows[key] = value
rows["runner_source_connects"] = "0"
rows["target_host"] = "oo-a410-tgt"
rows["import_hash"] = "49dbba9719dadd3b7a95b4376d9b47b171ab7ae0ba33623df75d91c5ac85db73"
rows["generation"] = "1"
rows["snapshot_id"] = "00000004-00000013-1"
order = [
    "import_hash", "generation", "snapshot_id", "target_host", "reader", "database",
    "server_version", "runner_source_connects", "count_batch_c", "id_overlap",
    "amount_sum_qty_above_2", "null_amount_a", "amount_a_id_2", "qty_c_id_1", "qty_c_id_2",
]
missing = [key for key in order if key not in rows]
if missing:
    raise SystemExit("missing " + ",".join(missing))
(ev / "probe.txt").write_text("".join(f"{key}={rows[key]}\n" for key in order))
PY
if grep -F -q -f "$PASS" "$EV/read.stdout" "$EV/probe.txt" "$EV/console.log"; then
  echo "password bytes reached evidence" >&2
  exit 1
fi
echo "gold_read=ok"
