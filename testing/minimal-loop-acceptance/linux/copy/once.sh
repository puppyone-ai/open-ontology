#!/bin/bash
# One fresh source copy on the a407 databases. Does not touch oo-a410-src or its grants.
# Binary: a410 target volume, read only, sha256 recorded below.
set -eu
SRC=/Users/redux/puppy-one/open-ontology
EV="$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a407/SP11/minimal-loop-acceptance/copy"
SQL="$SRC/testing/minimal-loop-acceptance/linux/copy/source.sql"
SEC=/tmp/oo-a407-eef831aa-dba4-4af2-a78b-541bc9e0a407/secrets
HAND=/tmp/oo-a407-eef831aa-dba4-4af2-a78b-541bc9e0a407/handoff-411
RUST=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
PG=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
NET=oo-a407-eef831aa-net
NAME=oo-a407-copy
BIN_VOL=oo-a410-eef831aa-target
BIN_SHA=72d286f9f377ff70a812eda77b57b1f0af76e09af7e46bf5bdbb98fa824456db
OWNER=eef831aa-dba4-4af2-a78b-541bc9e0a407
mkdir -p "$EV"
if [ -f "$EV/bulk.manifest.json" ]; then
  echo "refusing to repeat a stored copy" >&2
  exit 2
fi
for need in "$SEC/src/ca.crt" "$SEC/src/admin.pass" "$SEC/dst/ca.crt" "$SEC/dst/admin.pass" "$SEC/dst/reader.pass"; do
  if [ ! -f "$need" ]; then
    echo "a407 secret material is absent" >&2
    exit 1
  fi
done
if ! docker image inspect "$RUST" >/dev/null 2>&1 || ! docker image inspect "$PG" >/dev/null 2>&1; then
  echo "pinned image is not present locally; not pulling" >&2
  exit 1
fi
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  echo "a407 network is absent; not creating it" >&2
  exit 1
fi
umask 077
if [ ! -f "$SEC/src/src-read.pass" ]; then
  openssl rand -hex 24 > "$SEC/src/src-read.pass"
  chmod 600 "$SEC/src/src-read.pass"
fi
if [ ! -f "$SEC/dst/runtime.pass" ]; then
  openssl rand -hex 24 > "$SEC/dst/runtime.pass"
  chmod 600 "$SEC/dst/runtime.pass"
fi
mkdir -p "$HAND"
chmod 700 "$HAND"

role_sql() {
  container=$1
  file=$2
  role=$3
  docker exec -u postgres -i "$container" bash -s -- "$file" "$role" <<'EOS'
set -eu
file=$1
role=$2
exists=$(psql -v ON_ERROR_STOP=1 -U oo_a407_admin -d postgres -tA -c "SELECT 1 FROM pg_roles WHERE rolname = '$role'" || true)
if [ "$exists" = "1" ]; then
  exit 0
fi
pw=$(cat "/secrets/$file")
psql -v ON_ERROR_STOP=1 -U oo_a407_admin -d postgres <<SQL
CREATE ROLE ${role} LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION PASSWORD '${pw}';
SQL
unset pw
EOS
}
role_sql oo-a407-src-pg src-read.pass oo_a407_src_read
role_sql oo-a407-dst-pg runtime.pass oo_a407_runtime

present=$(docker exec -u postgres oo-a407-src-pg psql -v ON_ERROR_STOP=1 -U oo_a407_admin -d oo_a407_src -tA -c "SELECT COALESCE(to_regclass('public.batch_a')::text, 'absent')")
present=$(printf '%s' "$present" | tr -d '[:space:]')
if [ "$present" = "absent" ]; then
  docker exec -u postgres -i oo-a407-src-pg psql -v ON_ERROR_STOP=1 -U oo_a407_admin -d oo_a407_src < "$SQL" > "$EV/load.stdout" 2> "$EV/load.stderr"
  printf '%s\n' 0 > "$EV/load.exit"
else
  printf '%s\n' "already-present" > "$EV/load.stdout"
fi

{
  printf '%s\n' '\pset tuples_only on'
  printf '%s\n' '\pset format unaligned'
  printf '%s\n' "SELECT 'source_system=' || system_identifier FROM pg_control_system();"
  printf '%s\n' "SELECT 'batch_a=' || count(*) FROM public.batch_a;"
  printf '%s\n' "SELECT 'batch_b=' || count(*) FROM public.batch_b;"
  printf '%s\n' "SELECT 'batch_c=' || count(*) FROM public.batch_c;"
  printf '%s\n' "SELECT 'writes=' || COALESCE(sum(n_tup_ins),0) || ',' || COALESCE(sum(n_tup_upd),0) || ',' || COALESCE(sum(n_tup_del),0) FROM pg_stat_user_tables WHERE relname IN ('batch_a','batch_b','batch_c');"
} | docker exec -u postgres -i oo-a407-src-pg psql -v ON_ERROR_STOP=1 -U oo_a407_admin -d oo_a407_src > "$EV/source-before.txt"
{
  printf '%s\n' '\pset tuples_only on'
  printf '%s\n' '\pset format unaligned'
  printf '%s\n' "SELECT 'target_system=' || system_identifier FROM pg_control_system();"
  printf '%s\n' "SELECT 'reader_super=' || rolsuper FROM pg_roles WHERE rolname = 'oo_a407_reader';"
  printf '%s\n' "SELECT 'runtime_login=' || rolcanlogin FROM pg_roles WHERE rolname = 'oo_a407_runtime';"
  printf '%s\n' "SELECT 'runtime_super=' || rolsuper FROM pg_roles WHERE rolname = 'oo_a407_runtime';"
} | docker exec -u postgres -i oo-a407-dst-pg psql -v ON_ERROR_STOP=1 -U oo_a407_admin -d oo_a407_dst > "$EV/target-before.txt"
grep -q 'batch_a=2000' "$EV/source-before.txt"
grep -q 'batch_b=8000' "$EV/source-before.txt"
grep -q 'batch_c=90000' "$EV/source-before.txt"
grep -q 'runtime_login=t' "$EV/target-before.txt"
grep -q 'runtime_super=f' "$EV/target-before.txt"
grep -q 'reader_super=f' "$EV/target-before.txt"

python3 - "$EV" "$SEC/src/ca.crt" "$SEC/dst/ca.crt" "$OWNER" <<'PY'
import hashlib, json, pathlib, sys
ev, src_ca, dst_ca, owner = sys.argv[1:]
def sha(path):
    return hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest()
body = {
    "protocol": "oo.pipeline.local-pg-mirror/v1",
    "engine": "postgresql",
    "source_id": "a407bulk",
    "host": "oo-a407-src-pg",
    "port": 5432,
    "database": "oo_a407_src",
    "user": "oo_a407_src_read",
    "server_name": "oo-a407-src-pg",
    "ca_sha256": sha(src_ca),
    "secret_ref": "secret://fixture/a407/src",
    "tables": [
        {"schema": "public", "table": "batch_a"},
        {"schema": "public", "table": "batch_b"},
        {"schema": "public", "table": "batch_c"},
    ],
    "connect_timeout_ms": 10000,
    "statement_timeout_ms": 30000,
    "lock_timeout_ms": 1000,
    "max_rows": 100000,
    "max_bytes": 1048576,
    "batch_rows": 1000,
    "snapshot_deadline_ms": 300000,
    "generation": 1,
    "shard": "bulk",
    "owner": owner,
    "target_host": "oo-a407-dst-pg",
    "target_port": 5432,
    "target_database": "oo_a407_dst",
    "target_admin_user": "oo_a407_admin",
    "target_runtime_user": "oo_a407_runtime",
    "target_reader_user": "oo_a407_reader",
    "target_server_name": "oo-a407-dst-pg",
    "target_ca_sha256": sha(dst_ca),
}
path = pathlib.Path(ev) / "source.config.json"
path.write_text(json.dumps(body, separators=(",", ":")) + "\n")
PY
cp "$EV/source.config.json" "$HAND/source.config.json"
chmod 600 "$HAND/source.config.json"
umask 077
cat > "$HAND/PATHS.txt" <<EOF
consumer=a411
scope=pipeline-only
network=$NET
source_host=oo-a407-src-pg
source_database=oo_a407_src
source_user=oo_a407_src_read
source_config=$HAND/source.config.json
source_password_file=$SEC/src/src-read.pass
source_ca_file=$SEC/src/ca.crt
target_host=oo-a407-dst-pg
target_database=oo_a407_dst
target_admin_user=oo_a407_admin
target_admin_password_file=$SEC/dst/admin.pass
target_runtime_user=oo_a407_runtime
target_runtime_password_file=$SEC/dst/runtime.pass
target_reader_user=oo_a407_reader
target_reader_password_file=$SEC/dst/reader.pass
target_ca_file=$SEC/dst/ca.crt
binary_volume=$BIN_VOL
binary_path=tool/debug/local-pg-mirror
binary_sha256=$BIN_SHA
a410_source=do-not-regrant
EOF
chmod 600 "$HAND/PATHS.txt"

rm -f "$EV/copy.done"
(
  peak=0
  while [ ! -f "$EV/copy.done" ]; do
    n=$(docker exec -u postgres oo-a407-src-pg psql -U oo_a407_admin -d oo_a407_src -tA -c "SELECT count(*) FROM pg_stat_activity WHERE usename = 'oo_a407_src_read'" 2>/dev/null || printf '0')
    n=$(printf '%s' "$n" | tr -d '[:space:]')
    case "$n" in ''|*[!0-9]*) n=0 ;; esac
    if [ "$n" -gt "$peak" ]; then peak=$n; fi
    sleep 0.3
  done
  printf '%s\n' "$peak" > "$EV/source-role-peak.txt"
) &
sampler=$!
printf '%s\n' "docker run --name ${NAME} --network ${NET} --cap-drop ALL --memory 1g --cpus 1 --entrypoint bash image=pinned-rust-1.94 binary=a410-volume-ro" > "$EV/invocation.txt"
set +e
docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --entrypoint bash \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory 1g \
  --memory-swap 1g \
  --cpus 1 \
  --pids-limit 128 \
  --network "$NET" \
  -e HOME=/tmp \
  --mount "type=volume,source=${BIN_VOL},target=/a410,readonly" \
  --mount "type=bind,source=${EV},target=/work" \
  --mount "type=bind,source=${SEC}/src/src-read.pass,target=/secrets/src-read.pass,readonly" \
  --mount "type=bind,source=${SEC}/src/ca.crt,target=/secrets/src-ca.crt,readonly" \
  --mount "type=bind,source=${SEC}/dst/admin.pass,target=/secrets/tgt-admin.pass,readonly" \
  --mount "type=bind,source=${SEC}/dst/runtime.pass,target=/secrets/runtime.pass,readonly" \
  --mount "type=bind,source=${SEC}/dst/ca.crt,target=/secrets/tgt-ca.crt,readonly" \
  --tmpfs /tmp:rw,noexec,nosuid,size=64m \
  --label oo.project=open-ontology \
  --label oo.owner=a407 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a407 \
  --label oo.kind=copy-runner \
  "$RUST" \
  -c 'set +e
sha=$(sha256sum /a410/tool/debug/local-pg-mirror | awk "{print \$1}")
printf "%s\n" "$sha" > /work/binary.sha256
if [ "$sha" != "72d286f9f377ff70a812eda77b57b1f0af76e09af7e46bf5bdbb98fa824456db" ]; then
  echo "binary hash mismatch" > /work/discover.stderr
  echo 1 > /work/discover.exit
  exit 1
fi
/a410/tool/debug/local-pg-mirror discover /work/source.config.json /work/bulk.plan.json /secrets/src-read.pass /secrets/src-ca.crt > /work/discover.stdout 2> /work/discover.stderr
de=$?
printf "%s\n" "$de" > /work/discover.exit
if [ "$de" -ne 0 ]; then
  exit "$de"
fi
/a410/tool/debug/local-pg-mirror copy /work/bulk.plan.json /secrets/src-read.pass /secrets/src-ca.crt /secrets/tgt-admin.pass /secrets/runtime.pass /work/bulk.manifest.json /work/bulk.target.json /secrets/tgt-ca.crt finish > /work/copy.stdout 2> /work/copy.stderr
ce=$?
printf "%s\n" "$ce" > /work/copy.exit
exit "$ce"
' > "$EV/console.log" 2>&1
run_exit=$?
touch "$EV/copy.done"
wait "$sampler" || true
set +e
printf '%s\n' "$run_exit" > "$EV/docker-run.exit"
docker inspect -f 'ExitCode={{.State.ExitCode}} OOMKilled={{.State.OOMKilled}} Privileged={{.HostConfig.Privileged}} NetworkMode={{.HostConfig.NetworkMode}} Memory={{.HostConfig.Memory}}' "$NAME" > "$EV/hostconfig.txt" 2>/dev/null || true
docker rm "$NAME" >/dev/null 2>&1 || true
{
  printf '%s\n' '\pset tuples_only on'
  printf '%s\n' '\pset format unaligned'
  printf '%s\n' "SELECT 'writes=' || COALESCE(sum(n_tup_ins),0) || ',' || COALESCE(sum(n_tup_upd),0) || ',' || COALESCE(sum(n_tup_del),0) FROM pg_stat_user_tables WHERE relname IN ('batch_a','batch_b','batch_c');"
} | docker exec -u postgres -i oo-a407-src-pg psql -v ON_ERROR_STOP=1 -U oo_a407_admin -d oo_a407_src > "$EV/source-after.txt" 2>> "$EV/console.log"
if grep -F -q -f "$SEC/src/src-read.pass" "$EV/console.log" "$EV/discover.stdout" "$EV/discover.stderr" "$EV/copy.stdout" "$EV/copy.stderr" "$EV/source.config.json" 2>/dev/null; then
  echo "password bytes reached evidence" >&2
  exit 1
fi
if [ "$run_exit" -ne 0 ]; then
  echo "fresh copy failed" >&2
  exit "$run_exit"
fi
set -e
python3 - "$EV" <<'PY'
import json, pathlib, sys
ev = pathlib.Path(sys.argv[1])
manifest = json.loads((ev / "bulk.manifest.json").read_text())
keep = [
    "protocol", "mirror_state", "customer_complete", "consistency", "rows",
    "source_socket_peak", "source_socket_fact", "source_write_tuples", "source_read_only",
    "import_generation", "import_hash", "snapshot_id", "failure",
]
lines = []
for key in keep:
    lines.append(f"{key}={manifest.get(key)}")
for table in manifest.get("tables_completed", []):
    lines.append(f"table={table.get('schema')}.{table.get('name')} rows={table.get('rows')}")
(ev / "manifest-summary.txt").write_text("\n".join(lines) + "\n")
allowed = {
    ("public", "batch_a"): {"id", "amount", "label"},
    ("public", "batch_b"): {"id"},
    ("public", "batch_c"): {"id", "qty", "amount", "note"},
}
grants = [
    "CREATE ROLE oo_r_devread NOLOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION;",
    "GRANT oo_r_devread TO oo_a407_reader WITH INHERIT FALSE, SET TRUE;",
    "GRANT USAGE ON SCHEMA _oo TO oo_r_devread;",
    "GRANT USAGE ON SCHEMA _oo TO oo_a407_reader;",
    "GRANT SELECT ON TABLE _oo.physical_mappings TO oo_a407_reader;",
    "GRANT EXECUTE ON FUNCTION _oo.lookup_physical_mapping(text, text, bigint, text, text, text, text) TO oo_a407_reader;",
]
for table in manifest["tables_completed"]:
    key = (table["schema"], table["name"])
    if key not in allowed:
        continue
    cols = [column for column in table["columns"] if column["name"] in allowed[key]]
    if len(cols) != len(allowed[key]):
        raise SystemExit(f"missing columns for {key}")
    listed = ", ".join('"' + column["physical_name"] + '"' for column in cols)
    grants.append(
        f'GRANT SELECT ({listed}) ON TABLE "_oo"."{table["physical_name"]}" TO oo_r_devread;'
    )
(ev / "preview-grant.sql").write_text("\n".join(grants) + "\n")
PY
docker exec -u postgres -i oo-a407-dst-pg psql -v ON_ERROR_STOP=1 -U oo_a407_admin -d oo_a407_dst < "$EV/preview-grant.sql" > "$EV/preview-grant.stdout" 2> "$EV/preview-grant.stderr"
{
  printf '%s\n' '\pset tuples_only on'
  printf '%s\n' '\pset format unaligned'
  printf '%s\n' "SELECT rolname || '|' || rolcanlogin || '|' || rolsuper || '|' || rolcreatedb || '|' || rolcreaterole || '|' || rolinherit || '|' || rolbypassrls || '|' || rolreplication FROM pg_roles WHERE rolname = 'oo_r_devread';"
  printf '%s\n' "SELECT member.rolname || '|' || m.inherit_option || '|' || m.set_option || '|' || m.admin_option FROM pg_auth_members m JOIN pg_roles granted ON granted.oid = m.roleid JOIN pg_roles member ON member.oid = m.member WHERE granted.rolname = 'oo_r_devread';"
  printf '%s\n' "SELECT COUNT(*) FROM information_schema.role_table_grants WHERE grantee = 'oo_r_devread';"
} | docker exec -u postgres -i oo-a407-dst-pg psql -v ON_ERROR_STOP=1 -U oo_a407_admin -d oo_a407_dst > "$EV/role-readback.txt"
echo "fresh_copy=ok"
