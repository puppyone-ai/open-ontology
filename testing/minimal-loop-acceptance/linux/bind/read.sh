#!/bin/bash
# Read the sealed a410 target as oo_mirror_read. No DDL, no restart, no drop.
# Mounts only the SHARE reader CA and reader password. Does not open source secrets.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EV="$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a407/SP11/minimal-loop-acceptance/bind"
A410="$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a410/SP03/local-mirror-single-source"
IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
NET=oo-a410-eef831aa-net
NAME=oo-a407-bind-read
CA=/tmp/oo-a410-eef831aa-dba4-4af2-a78b-541bc9e0a410/secrets/ca.crt
PASS=/tmp/oo-a410-eef831aa-dba4-4af2-a78b-541bc9e0a410/secrets/reader.pass
CA_SHA=db25c207b268e7ef968155884413ab66bd7550074321ede8b75e366f9eaf9d09
PIN=5df69049267bf16eca36d311a1b3082f27aa9754
RECORD=6663e12d1fe2e1ae2a876c56110c9abcf3ebc151
mkdir -p "$EV"
if [ -f "$EV/read.stdout" ]; then
  echo "refusing to overwrite bind read" >&2
  exit 2
fi
{
  echo "producer=$PIN"
  echo "hash_record=$RECORD"
  git -C "$SRC" log -1 --format='producer_subject=%s' "$PIN"
  git -C "$SRC" log -1 --format='record_subject=%s' "$RECORD"
  git -C "$SRC" diff --stat "$PIN" "$RECORD" -- tools/local-pg-mirror | awk 'BEGIN{c=0} {c++} END{print "mirror_tree_diff_lines=" c+0}'
  git -C "$SRC" rev-parse "$PIN:tools/agent-entry" >/dev/null 2>&1
  echo "agent_entry_at_producer=$?"
  git -C "$SRC" rev-parse "$RECORD:tools/agent-entry" >/dev/null 2>&1
  echo "agent_entry_at_record=$?"
} > "$EV/producer.txt"
set -e
mismatch=0
while read -r sum name; do
  [ -n "$sum" ] || continue
  blob=$(git -C "$SRC" rev-parse "$PIN:tools/local-pg-mirror/$name")
  got=$(git -C "$SRC" cat-file blob "$blob" | shasum -a 256 | awk '{print $1}')
  if [ "$got" != "$sum" ]; then
    echo "mismatch $name" >> "$EV/producer.txt"
    mismatch=1
  fi
done < "$A410/linux/proof/source-sha256.txt"
echo "source_sha256_mismatch=$mismatch" >> "$EV/producer.txt"
if [ "$mismatch" -ne 0 ]; then
  echo "producer tree does not match source-sha256.txt" >&2
  exit 1
fi
python3 - "$A410/linux/proof/source-sha256.txt" >> "$EV/producer.txt" <<'PY'
import pathlib, sys
text = pathlib.Path(sys.argv[1]).read_text().strip().splitlines()
print(f"source_sha256_files={len(text)}")
PY
shasum -a 256 "$A410/linux/proof/cases/bulk.target.json" "$A410/linux/proof/cases/bulk.manifest.json" "$CA" \
  | awk '{print $1, $2}' > "$EV/file-hashes.txt"
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
  --mount "type=bind,source=${SRC}/testing/minimal-loop-acceptance/linux/bind/read.sql,target=/sql/read.sql,readonly" \
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
psql -v ON_ERROR_STOP=1 -tA -f /sql/read.sql > /out/read.stdout
set +e
psql -v ON_ERROR_STOP=1 -v VERBOSITY=verbose -tA -c "SELECT 1 FROM \"_oo\".\"pt_ab4c1ae23e1e6e45820a2a5bb0d37130ab08e916890b3036c17cb19bba40\"" > /out/cancel.stdout 2> /out/cancel.stderr
echo $? > /out/cancel.exit
psql -v ON_ERROR_STOP=1 -v VERBOSITY=verbose -tA -c "SELECT 1 FROM \"_oo\".\"pt_b715e6dfd3b92125fc0677c10c3679210463041e01a8779be5339e0eb322\"" > /out/partial.stdout 2> /out/partial.stderr
echo $? > /out/partial.exit
rm -f /tmp/.pgpass
' > "$EV/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EV/docker-run.exit"
docker inspect -f 'ExitCode={{.State.ExitCode}} OOMKilled={{.State.OOMKilled}} Privileged={{.HostConfig.Privileged}} NetworkMode={{.HostConfig.NetworkMode}} Memory={{.HostConfig.Memory}}' "$NAME" > "$EV/hostconfig.txt" 2>/dev/null || true
docker rm "$NAME" >/dev/null 2>&1 || true
if [ "$run_exit" -ne 0 ]; then
  echo "target read failed" >&2
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
def denied(name):
    err = (ev / f"{name}.stderr").read_text(errors="replace")
    exit_code = (ev / f"{name}.exit").read_text().strip()
    if "42501" in err or "permission denied" in err.lower():
        return "42501"
    return f"unexpected:{exit_code}"
rows["cancel_select"] = denied("cancel")
rows["partial_select"] = denied("partial")
rows["runner_source_connects"] = "0"
rows["target_host"] = "oo-a410-tgt"
rows["import_hash"] = "49dbba9719dadd3b7a95b4376d9b47b171ab7ae0ba33623df75d91c5ac85db73"
rows["generation"] = "1"
rows["snapshot_id"] = "00000004-00000013-1"
order = [
    "import_hash", "generation", "snapshot_id", "target_host", "reader", "database",
    "server_version", "runner_source_connects", "public_batch_a", "public_batch_b",
    "public_batch_c", "batch_a_rows", "batch_b_rows", "batch_c_rows",
    "batch_a_id_oid", "batch_a_id_typmod", "batch_a_amount_oid", "batch_a_amount_typmod",
    "batch_b_time_oid", "batch_b_time_typmod", "batch_b_blob_oid", "batch_b_blob_typmod",
    "batch_c_qty_oid", "batch_c_qty_typmod", "batch_c_amount_oid", "batch_c_amount_typmod",
    "batch_a_null", "batch_a_amount_2", "batch_b_time_1", "batch_b_blob_1",
    "batch_c_qty_1", "batch_c_qty_2", "cancel_select", "partial_select",
]
missing = [key for key in order if key not in rows]
if missing:
    raise SystemExit("missing " + ",".join(missing))
(ev / "probe.txt").write_text("".join(f"{key}={rows[key]}\n" for key in order))
PY
if grep -F -q -f "$PASS" "$EV/read.stdout" "$EV/probe.txt" "$EV/console.log" "$EV/cancel.stderr" "$EV/partial.stderr"; then
  echo "password bytes reached evidence" >&2
  exit 1
fi
echo "bind_read=ok"
