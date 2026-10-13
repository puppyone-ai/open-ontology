#!/bin/bash
# Read the fresh a407 copy back as the reader. No source password. No DDL.
set -eu
SRC=/Users/redux/puppy-one/open-ontology
EV="$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a407/SP11/minimal-loop-acceptance/copy"
SEC=/tmp/oo-a407-eef831aa-dba4-4af2-a78b-541bc9e0a407/secrets
IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
NET=oo-a407-eef831aa-net
NAME=oo-a407-copy-verify
if [ -f "$EV/verify.stdout" ]; then
  echo "refusing to overwrite copy verify" >&2
  exit 2
fi
python3 - "$EV/bulk.manifest.json" "$EV/verify.sql" <<'PY'
import json, sys
manifest = json.load(open(sys.argv[1]))
tables = {(item["schema"], item["name"]): item for item in manifest["tables_completed"]}
def col(table, name):
    item = tables[("public", table)]
    found = next(column for column in item["columns"] if column["name"] == name)
    return item["physical_name"], found["physical_name"]
a, a_id = col("batch_a", "id")
_, a_amount = col("batch_a", "amount")
c, c_id = col("batch_c", "id")
_, c_qty = col("batch_c", "qty")
_, c_amount = col("batch_c", "amount")
sql = f"""
\\pset tuples_only on
\\pset format unaligned
SELECT 'count_c=' || count(*)::text FROM "_oo"."{c}";
SELECT 'overlap=' || count(*)::text FROM "_oo"."{a}" AS a JOIN "_oo"."{c}" AS c ON a."{a_id}" = c."{c_id}";
SELECT 'sum_qty_above_2=' || COALESCE(sum("{c_amount}")::text, 'null') FROM "_oo"."{c}" WHERE "{c_qty}" > 2;
SELECT 'null_amount_a=' || count(*)::text FROM "_oo"."{a}" WHERE "{a_amount}" IS NULL;
SELECT 'amount_a_2=' || COALESCE("{a_amount}"::text, 'null') FROM "_oo"."{a}" WHERE "{a_id}" = 2;
SELECT 'qty_1=' || "{c_qty}"::text FROM "_oo"."{c}" WHERE "{c_id}" = 1;
SELECT 'qty_2=' || "{c_qty}"::text FROM "_oo"."{c}" WHERE "{c_id}" = 2;
"""
open(sys.argv[2], "w").write(sql)
PY
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
  --mount "type=bind,source=${SEC}/dst/ca.crt,target=/secrets/ca.crt,readonly" \
  --mount "type=bind,source=${SEC}/dst/reader.pass,target=/secrets/reader.pass,readonly" \
  --mount "type=bind,source=${EV}/verify.sql,target=/sql/verify.sql,readonly" \
  --mount "type=bind,source=${EV},target=/out" \
  --label oo.project=open-ontology \
  --label oo.owner=a407 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a407 \
  --label oo.kind=target-reader \
  "$IMAGE" \
  -c 'umask 077
set -e
pass=$(cat /secrets/reader.pass)
printf "%s\n" "oo-a407-dst-pg:5432:oo_a407_dst:oo_a407_reader:${pass}" > /tmp/.pgpass
chmod 600 /tmp/.pgpass
unset pass
export PGPASSFILE=/tmp/.pgpass PGSSLMODE=verify-full PGSSLROOTCERT=/secrets/ca.crt
export PGHOST=oo-a407-dst-pg PGPORT=5432 PGDATABASE=oo_a407_dst PGUSER=oo_a407_reader
psql -v ON_ERROR_STOP=1 -f /sql/verify.sql > /out/verify.stdout
rm -f /tmp/.pgpass
'
code=$?
docker inspect -f 'ExitCode={{.State.ExitCode}} OOMKilled={{.State.OOMKilled}} Privileged={{.HostConfig.Privileged}} NetworkMode={{.HostConfig.NetworkMode}} Memory={{.HostConfig.Memory}}' "$NAME" > "$EV/verify-hostconfig.txt" || true
docker rm "$NAME" >/dev/null
if [ "$code" -ne 0 ]; then
  echo "copy verify failed" >&2
  exit "$code"
fi
if grep -F -q -f "$SEC/dst/reader.pass" "$EV/verify.stdout"; then
  echo "password bytes reached evidence" >&2
  exit 1
fi
echo "copy_verify=ok"
