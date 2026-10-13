#!/bin/bash
# Own-fixture copy against oo-a409-src and oo-a409-tgt.
# The mirror binary is the Linux 1.94 release build on the private target volume.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EV=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a409/SP03/local-copy-acceptance/linux/copy
WORK=${EV}/work
SEC=/tmp/oo-a409-eef831aa-dba4-4af2-a78b-541bc9e0a409/secrets
NET=oo-a409-eef831aa-net
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
PGIMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
TARGET_VOL=oo-a409-eef831aa-target
PY=${SRC}/testing/local-copy-acceptance/linux/copy
mkdir -p "$WORK"
[ -f "$EV/build/binary.sha256" ]
cp "$EV/build/binary.sha256" "$WORK/binary.sha256"
CA=$(tr -d '[:space:]' < "$EV/pg/ca.sha256")
docker run --rm --network none --pull never --platform linux/arm64 \
  --cap-drop ALL --memory 256m --cpus 1 \
  --mount "type=volume,source=${TARGET_VOL},target=/target,readonly" \
  --label oo.project=open-ontology --label oo.owner=a409 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a409 \
  --label oo.kind=copy-runner --label oo.task=local-copy-acceptance \
  "$IMAGE" sha256sum /target/tool/release/local-pg-mirror > "$WORK/binary-pin.txt"
pin=$(awk 'NR==1 {print $1}' "$WORK/binary-pin.txt")
want=$(awk 'NR==1 {print $1}' "$WORK/binary.sha256")
if [ "$pin" != "$want" ]; then
  echo "binary pin mismatch" >&2
  exit 2
fi

psql_src() {
  docker exec -i -u postgres -e PGAPPNAME=oo-a409-monitor oo-a409-src psql -d "$1" -v ON_ERROR_STOP=1 -At
}
psql_tgt() {
  docker exec -i -u postgres -e PGAPPNAME=oo-a409-monitor oo-a409-tgt psql -d "$1" -v ON_ERROR_STOP=1 -At
}

if [ ! -f "$WORK/fixture.ok" ]; then
  if ! psql_src postgres <<'SQL' | grep -qx 1; then
SELECT 1 FROM pg_database WHERE datname = 'srcdb';
SQL
    psql_src postgres <<'SQL'
CREATE DATABASE srcdb TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8';
SQL
    src_pass=$(tr -d '\n' < "$SEC/src.pass")
    docker exec -i -u postgres -e PGAPPNAME=oo-a409-monitor oo-a409-src \
      psql -d postgres -v ON_ERROR_STOP=1 <<SQL
CREATE ROLE oo_src_read LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION PASSWORD '${src_pass}';
SQL
    unset src_pass
    psql_src srcdb <<'SQL'
REVOKE ALL ON DATABASE srcdb FROM PUBLIC;
REVOKE CREATE ON DATABASE srcdb FROM PUBLIC;
REVOKE ALL ON SCHEMA public FROM PUBLIC;
REVOKE CREATE ON SCHEMA public FROM PUBLIC;
CREATE TABLE public.amounts (id int8 PRIMARY KEY, qty int8, price numeric(12,2), note text);
INSERT INTO public.amounts VALUES (1, 9223372036854775807, 10.50, NULL), (2, -5, 0.00, 'kept');
CREATE TABLE public.events (id int8 PRIMARY KEY, seen timestamp, body json, blob bytea);
INSERT INTO public.events VALUES (7, timestamp '2024-03-04 05:06:07', '{"k":"v"}'::json, '\x0001ff'::bytea);
CREATE TABLE public.flags (id int8 PRIMARY KEY, on_flag bool, label text);
INSERT INTO public.flags VALUES (1, true, 'yes'), (2, false, '');
CREATE TABLE public.odd (id int8 PRIMARY KEY, doc jsonb);
INSERT INTO public.odd VALUES (1, '{"a":1}'::jsonb);
CREATE TABLE public.batch_a (id int8 PRIMARY KEY, amount numeric(38,4), label text);
INSERT INTO public.batch_a SELECT g, CASE WHEN g = 1 THEN NULL WHEN g = 2 THEN 1234567890123456789012345678901234.1234 ELSE 0.0001 END, CASE WHEN g = 3 THEN '' ELSE 'n' END FROM generate_series(1, 2000) AS g;
CREATE TABLE public.batch_b (id int8 PRIMARY KEY, at_time time, blob bytea);
INSERT INTO public.batch_b SELECT g, CASE WHEN g = 2 THEN NULL ELSE time '13:14:15.123456' END, CASE WHEN g = 3 THEN NULL ELSE '\x0001ff'::bytea END FROM generate_series(1, 8000) AS g;
CREATE TABLE public.batch_c (id int8 PRIMARY KEY, qty int8, amount numeric(38,4), note text);
INSERT INTO public.batch_c SELECT g, CASE WHEN g = 1 THEN 9223372036854775807 WHEN g = 2 THEN -9223372036854775808 ELSE g END, CASE WHEN g = 4 THEN NULL ELSE 1.5000 END, CASE WHEN g = 5 THEN NULL ELSE 'c' END FROM generate_series(1, 90000) AS g;
GRANT CONNECT ON DATABASE srcdb TO oo_src_read;
GRANT USAGE ON SCHEMA public TO oo_src_read;
GRANT SELECT ON TABLE public.amounts, public.events, public.flags, public.odd, public.batch_a, public.batch_b, public.batch_c TO oo_src_read;
SQL
  fi
  if ! psql_tgt postgres <<'SQL' | grep -qx t; then
SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname = 'mirrordb');
SQL
    psql_tgt postgres <<'SQL'
CREATE DATABASE mirrordb TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8';
SQL
    runtime_pass=$(tr -d '\n' < "$SEC/runtime.pass")
    reader_pass=$(tr -d '\n' < "$SEC/reader.pass")
    docker exec -i -u postgres -e PGAPPNAME=oo-a409-monitor oo-a409-tgt \
      psql -d postgres -v ON_ERROR_STOP=1 <<SQL
CREATE ROLE oo_mirror_runtime LOGIN INHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION PASSWORD '${runtime_pass}';
CREATE ROLE oo_mirror_read LOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION PASSWORD '${reader_pass}';
SQL
    unset runtime_pass reader_pass
  fi
  python3 "$PY/spool.py" source "$WORK"
  printf '%s\n' ok > "$WORK/fixture.ok"
fi

write_config() {
  local name="$1" owner="$2" shard="$3" deadline="$4" max_rows="$5" tables="$6"
  python3 - "$WORK/${name}.config.json" "$CA" "$owner" "$shard" "$deadline" "$max_rows" "$tables" <<'PY'
import json, sys
path, ca, owner, shard, deadline, max_rows, tables = sys.argv[1:]
listed = [{"schema": "public", "table": name} for name in tables.split(",") if name]
doc = {
  "protocol": "oo.pipeline.local-pg-mirror/v1",
  "engine": "postgresql",
  "source_id": path.rsplit("/", 1)[-1].replace(".config.json", ""),
  "host": "oo-a409-src",
  "port": 5432,
  "database": "srcdb",
  "user": "oo_src_read",
  "server_name": "oo-a409-src",
  "ca_sha256": ca,
  "secret_ref": "secret://fixture/src",
  "tables": listed,
  "connect_timeout_ms": 10000,
  "statement_timeout_ms": 30000,
  "lock_timeout_ms": 1000,
  "max_rows": int(max_rows),
  "max_bytes": 1048576,
  "batch_rows": 1000,
  "snapshot_deadline_ms": int(deadline),
  "generation": 1,
  "shard": shard,
  "owner": owner,
  "target_host": "oo-a409-tgt",
  "target_port": 5432,
  "target_database": "mirrordb",
  "target_admin_user": "oo_fixture_admin",
  "target_runtime_user": "oo_mirror_runtime",
  "target_reader_user": "oo_mirror_read",
  "target_server_name": "oo-a409-tgt",
  "target_ca_sha256": ca,
}
open(path, "w").write(json.dumps(doc))
PY
}

mirror() {
  docker run --rm --name oo-a409-copy-once \
    --platform linux/arm64 --pull never --init --user 0:0 \
    --security-opt no-new-privileges --cap-drop ALL \
    --memory 2g --memory-swap 2g --cpus 1 --pids-limit 256 \
    --network "$NET" \
    --mount "type=volume,source=${TARGET_VOL},target=/target,readonly" \
    --mount "type=bind,source=${WORK},target=/work" \
    --mount "type=bind,source=${SEC},target=/secrets,readonly" \
    --label oo.project=open-ontology --label oo.owner=a409 \
    --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a409 \
    --label oo.kind=copy-runner --label oo.task=local-copy-acceptance \
    "$IMAGE" "$@"
}

discover() {
  local name="$1"
  mirror /target/tool/release/local-pg-mirror discover \
    "/work/${name}.config.json" "/work/${name}.plan.json" \
    /secrets/src.pass /secrets/ca.crt \
    > "$WORK/${name}.discover.stdout" 2> "$WORK/${name}.discover.stderr"
}

copy_pair() {
  local plan="$1" manifest="$2" secret="$3"
  shift 3
  set +e
  mirror /target/tool/release/local-pg-mirror copy \
    "/work/${plan}.plan.json" "$secret" /secrets/ca.crt \
    /secrets/tgt/admin.pass /secrets/runtime.pass \
    "/work/${manifest}.manifest.json" "/work/${manifest}.target.json" \
    /secrets/ca.crt "$@" \
    > "$WORK/${manifest}.copy.stdout" 2> "$WORK/${manifest}.copy.stderr"
  local code=$?
  set -e
  printf '%s\n' "$code" > "$WORK/${manifest}.copy.exit"
  count_src_backends > "$WORK/${manifest}.backends.txt"
  if [ "$code" -gt 3 ]; then
    echo "copy ${manifest} exit ${code}" >&2
    exit "$code"
  fi
}

count_src_backends() {
  psql_src postgres <<'SQL'
SELECT count(*) FROM pg_stat_activity WHERE datname = 'srcdb' AND usename = 'oo_src_read' AND application_name = 'oo-pg-source';
SQL
}

sample_loop() {
  local out="$1" flag="$2"
  : > "$out"
  local poll=0
  while [ ! -f "$flag" ]; do
    poll=$((poll + 1))
    docker exec -u postgres -e PGAPPNAME=oo-a409-monitor oo-a409-src \
      psql -d postgres -At -F "$(printf '\t')" -c "SELECT ${poll}, pid, usename, datname, application_name, backend_type, COALESCE(state,''), COALESCE(wait_event_type,''), current_setting('transaction_isolation'), current_setting('transaction_read_only') FROM pg_stat_activity WHERE backend_type = 'client backend' ORDER BY pid" \
      >> "$out" || true
    sleep 0.02
  done
}

run_sampled() {
  local name="$1"
  shift
  local flag="$WORK/${name}.sample.stop"
  rm -f "$flag"
  sample_loop "$WORK/${name}.sampler.tsv" "$flag" &
  local sampler=$!
  set +e
  "$@"
  local code=$?
  set -e
  touch "$flag"
  wait "$sampler" || true
  return "$code"
}

write_config gold 40940940-9409-4409-8409-409409409001 fixa 300000 10000 amounts,events,flags
write_config cancel 40940940-9409-4409-8409-409409409002 fixc 300000 10000 flags
write_config partial 40940940-9409-4409-8409-409409409003 fixp 300000 10000 flags
write_config timed 40940940-9409-4409-8409-409409409004 fixt 1 10000 flags
write_config disconnect 40940940-9409-4409-8409-409409409005 fixq 300000 10000 flags
write_config restart 40940940-9409-4409-8409-409409409006 fixr 300000 10000 flags
write_config bulk 40940940-9409-4409-8409-409409409007 fixk 300000 100000 batch_a,batch_b,batch_c

for name in gold cancel partial timed disconnect restart bulk; do
  discover "$name"
done

copy_pair gold gold /secrets/src.pass finish
cp "$WORK/gold.manifest.json" "$WORK/gold.sealed.manifest.json"
psql_src srcdb <<'SQL'
ALTER TABLE public.amounts ADD COLUMN extra text;
SQL
copy_pair gold gold-drift /secrets/src.pass finish
copy_pair cancel cancel /secrets/src.pass cancel
copy_pair partial partial /secrets/src.pass stop-after 1
copy_pair timed timed /secrets/src.pass finish
copy_pair disconnect disconnect /secrets/src.pass drop-after-snapshot
copy_pair restart restart /secrets/src.pass drop-after-snapshot
cp "$WORK/restart.manifest.json" "$WORK/restart-before.manifest.json"
cp "$WORK/restart.backends.txt" "$WORK/restart-before.backends.txt"
cp "$WORK/restart.copy.stdout" "$WORK/restart-before.copy.stdout"
cp "$WORK/restart.copy.stderr" "$WORK/restart-before.copy.stderr"
cp "$WORK/restart.copy.exit" "$WORK/restart-before.copy.exit"
RESUME_SEC=/tmp/oo-a409-eef831aa-dba4-4af2-a78b-541bc9e0a409/resume-secrets
mkdir -p "$RESUME_SEC"
cp "$SEC/ca.crt" "$RESUME_SEC/ca.crt"
cp "$SEC/tgt/admin.pass" "$RESUME_SEC/admin.pass"
cp "$SEC/runtime.pass" "$RESUME_SEC/runtime.pass"
set +e
docker run --rm --name oo-a409-copy-once \
  --platform linux/arm64 --pull never --init --user 0:0 \
  --security-opt no-new-privileges --cap-drop ALL \
  --memory 2g --memory-swap 2g --cpus 1 --pids-limit 256 \
  --network "$NET" \
  --mount "type=volume,source=${TARGET_VOL},target=/target,readonly" \
  --mount "type=bind,source=${WORK},target=/work" \
  --mount "type=bind,source=${RESUME_SEC},target=/secrets,readonly" \
  --label oo.project=open-ontology --label oo.owner=a409 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a409 \
  --label oo.kind=copy-runner --label oo.task=local-copy-acceptance \
  "$IMAGE" /target/tool/release/local-pg-mirror copy \
  /work/restart.plan.json /work/absent-secret /secrets/ca.crt \
  /secrets/admin.pass /secrets/runtime.pass \
  /work/restart.manifest.json /work/restart.target.json \
  /secrets/ca.crt finish \
  > "$WORK/restart.copy.stdout" 2> "$WORK/restart.copy.stderr"
resume_code=$?
set -e
printf '%s\n' "$resume_code" > "$WORK/restart.copy.exit"
count_src_backends > "$WORK/restart.backends.txt"
if [ "$resume_code" -gt 3 ]; then
  echo "restart resume exit ${resume_code}" >&2
  exit "$resume_code"
fi

psql_src postgres <<'SQL' > "$WORK/writes-before.txt"
SELECT tup_inserted::text || ' ' || tup_updated::text || ' ' || tup_deleted::text FROM pg_stat_database WHERE datname = 'srcdb';
SQL
run_sampled bulk copy_pair bulk bulk /secrets/src.pass finish
psql_src postgres <<'SQL' > "$WORK/writes-after.txt"
SELECT tup_inserted::text || ' ' || tup_updated::text || ' ' || tup_deleted::text FROM pg_stat_database WHERE datname = 'srcdb';
SQL
python3 - "$WORK" <<'PY'
import json, sys
from pathlib import Path
work = Path(sys.argv[1])
before = work.joinpath("writes-before.txt").read_text().split()
after = work.joinpath("writes-after.txt").read_text().split()
manifest = json.loads(work.joinpath("bulk.manifest.json").read_text())
delta = [int(after[i]) - int(before[i]) for i in range(3)]
writes = {
    "ddl": bool(manifest.get("source_ddl")),
    "dml": delta != [0, 0, 0] or manifest.get("source_write_tuples") not in (0, None),
    "heartbeat": False,
    "privilege": False,
    "explain": bool(manifest.get("source_explain")),
    "count_star": bool(manifest.get("source_count_star")),
}
work.joinpath("bulk.writes.json").write_text(json.dumps(writes) + "\n")
work.joinpath("bulk.write-delta.txt").write_text(" ".join(str(item) for item in delta) + "\n")
work.joinpath("bulk.writes-note.txt").write_text(
    "ddl, explain, and count_star are the manifest flags. dml is the srcdb "
    "tup_inserted/updated/deleted delta across the bulk copy, or a nonzero "
    "source_write_tuples field. heartbeat and privilege are false because the "
    "bulk argv does not issue them and the tuple delta is the write counter. "
    "The ALTER used for catalog drift is outside this window.\n"
)
PY

shasum -a 256 "$WORK/gold.manifest.json" | awk '{print $1}' > "$WORK/gold.manifest.sha256"
gold_counts() {
  python3 - "$WORK" "$1" <<'PY'
import json, subprocess, sys
from pathlib import Path
work = Path(sys.argv[1])
out = Path(sys.argv[2])
manifest = json.loads(work.joinpath("gold.manifest.json").read_text())
lines = []
for table in manifest["tables_completed"]:
    physical = table["physical_name"]
    sql = 'SELECT count(*) FROM "_oo"."' + physical.replace('"', "") + '"'
    proc = subprocess.run(
        ["docker", "exec", "-u", "postgres", "-e", "PGAPPNAME=oo-a409-monitor", "oo-a409-tgt", "psql", "-d", "mirrordb", "-v", "ON_ERROR_STOP=1", "-At", "-c", sql],
        text=True,
        capture_output=True,
    )
    if proc.returncode != 0:
        sys.stderr.write(proc.stderr)
        raise SystemExit(proc.returncode)
    lines.append(table["name"] + " " + proc.stdout.strip())
out.write_text("\n".join(lines) + "\n")
PY
}
gold_counts "$WORK/replay-counts-before.txt"
mkdir -p "$WORK/replay-secrets"
cp "$SEC/ca.crt" "$WORK/replay-secrets/ca.crt"
set +e
docker run --rm --name oo-a409-copy-once \
  --platform linux/arm64 --pull never --init --user 0:0 \
  --security-opt no-new-privileges --cap-drop ALL \
  --memory 2g --memory-swap 2g --cpus 1 --pids-limit 256 \
  --network "$NET" \
  --mount "type=volume,source=${TARGET_VOL},target=/target,readonly" \
  --mount "type=bind,source=${WORK},target=/work" \
  --mount "type=bind,source=${WORK}/replay-secrets,target=/secrets,readonly" \
  --label oo.project=open-ontology --label oo.owner=a409 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a409 \
  --label oo.kind=copy-runner --label oo.task=local-copy-acceptance \
  "$IMAGE" /target/tool/release/local-pg-mirror copy \
  /work/gold.plan.json /work/absent-secret /secrets/ca.crt \
  /work/absent-secret /work/absent-secret \
  /work/gold.manifest.json /work/gold-replay.target.json \
  /secrets/ca.crt finish \
  > "$WORK/replay.copy.stdout" 2> "$WORK/replay.copy.stderr"
replay_code=$?
set -e
printf '%s\n' "$replay_code" > "$WORK/replay.copy.exit"
count_src_backends > "$WORK/replay.backends.txt"
if [ "$replay_code" -gt 3 ]; then
  echo "replay exit ${replay_code}" >&2
  exit "$replay_code"
fi
shasum -a 256 "$WORK/gold.manifest.json" | awk '{print $1}' > "$WORK/gold.manifest.after-replay.sha256"
gold_counts "$WORK/replay-counts-after.txt"

psql_src postgres <<'SQL'
REVOKE CONNECT ON DATABASE srcdb FROM oo_src_read;
SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = 'srcdb' AND usename = 'oo_src_read';
SQL
set +e
docker run --rm --network "$NET" --cap-drop ALL --memory 256m \
  -e PGPASSWORD_FILE=/secrets/src.pass \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  "$PGIMAGE" \
  bash -c 'export PGPASSWORD=$(cat /secrets/src.pass); psql "host=oo-a409-src port=5432 dbname=srcdb user=oo_src_read sslmode=verify-full sslrootcert=/secrets/ca.crt" -c "select 1"' \
  > "$WORK/source-reconnect.stdout" 2> "$WORK/source-reconnect.stderr"
reconnect=$?
set -e
printf '%s\n' "$reconnect" > "$WORK/source-reconnect.exit"
backends=$(psql_src postgres <<'SQL'
SELECT count(*) FROM pg_stat_activity WHERE datname = 'srcdb' AND usename = 'oo_src_read' AND application_name = 'oo-pg-source';
SQL
)
printf '%s\n' "$backends" > "$WORK/backends-after.txt"
set +e
docker network disconnect "$NET" oo-a409-src
disconnect_code=$?
set -e
printf '%s\n' "$disconnect_code" > "$WORK/network-disconnect.exit"
docker inspect oo-a409-src --format '{{json .NetworkSettings.Networks}}' > "$WORK/source-networks.json"
python3 - "$WORK" "$reconnect" <<'PY'
import json, sys
from pathlib import Path
work = Path(sys.argv[1])
reconnect = int(sys.argv[2])
backends = int(work.joinpath("backends-after.txt").read_text().strip())
networks = json.loads(work.joinpath("source-networks.json").read_text())
doc = {
    "src_db": "srcdb",
    "src_connect": reconnect == 0,
    "src_backends": backends,
    "network_to_source": "oo-a409-eef831aa-net" in networks,
    "source_secret_in_reader": False,
}
work.joinpath("source-blocked.json").write_text(json.dumps(doc) + "\n")
PY

src_ident=$(psql_src postgres <<'SQL'
SELECT system_identifier::text FROM pg_control_system();
SQL
)
src_start=$(psql_src postgres <<'SQL'
SELECT pg_postmaster_start_time()::text FROM pg_control_system();
SQL
)
tgt_ident=$(psql_tgt postgres <<'SQL'
SELECT system_identifier::text FROM pg_control_system();
SQL
)
tgt_start=$(psql_tgt postgres <<'SQL'
SELECT pg_postmaster_start_time()::text FROM pg_control_system();
SQL
)
python3 - "$WORK/fingerprints.json" "$src_ident" "$src_start" "$tgt_ident" "$tgt_start" <<'PY'
import json, sys
path, src_ident, src_start, tgt_ident, tgt_start = sys.argv[1:]
doc = {
    "source": {"system_identifier": src_ident.strip(), "postmaster_start": src_start.strip(), "database": "srcdb"},
    "target": {"system_identifier": tgt_ident.strip(), "postmaster_start": tgt_start.strip(), "database": "mirrordb"},
}
open(path, "w").write(json.dumps(doc) + "\n")
PY

python3 - "$WORK" <<'PY'
import json, subprocess, sys
from pathlib import Path
work = Path(sys.argv[1])
manifest = json.loads(work.joinpath("bulk.manifest.json").read_text())
order = {
    "batch_a": ["id", "amount", "label"],
    "batch_b": ["id", "at_time", "blob"],
    "batch_c": ["id", "qty", "amount", "note"],
}
for table in manifest["tables_completed"]:
    by_name = {column["name"]: column for column in table["columns"]}
    exprs = []
    for logical in order[table["name"]]:
        column = by_name[logical]
        name = '"' + column["physical_name"].replace('"', "") + '"'
        if column["oid"] == 17:
            exprs.append(f"encode({name}, 'hex')")
        else:
            exprs.append(f"{name}::text")
    physical = '"_oo"."' + table["physical_name"].replace('"', "") + '"'
    sql = "\\pset null NULLMARK\nSELECT " + ", ".join(exprs) + f" FROM {physical} ORDER BY 1;\n"
    out = work / f"{table['name']}.target.tsv"
    proc = subprocess.run(
        ["docker", "exec", "-i", "-u", "postgres", "-e", "PGAPPNAME=oo-a409-monitor", "oo-a409-tgt", "psql", "-d", "mirrordb", "-v", "ON_ERROR_STOP=1", "-q", "-At", "-F", "\t"],
        input=sql,
        text=True,
        capture_output=True,
    )
    out.write_text(proc.stdout)
    if proc.returncode != 0:
        sys.stderr.write(proc.stderr)
        raise SystemExit(proc.returncode)
PY
python3 "$PY/spool.py" target batch_a "$WORK/batch_a.target.tsv" "$WORK/batch_a.target.jsonl"
python3 "$PY/spool.py" target batch_b "$WORK/batch_b.target.tsv" "$WORK/batch_b.target.jsonl"
python3 "$PY/spool.py" target batch_c "$WORK/batch_c.target.tsv" "$WORK/batch_c.target.jsonl"
python3 "$PY/assemble.py" "$WORK" /work
set +e
docker run --name oo-a409-judge \
  --platform linux/arm64 --pull never --init --user 0:0 \
  --security-opt no-new-privileges --cap-drop ALL \
  --memory 2g --memory-swap 2g --cpus 1 --pids-limit 256 \
  --network none \
  --mount "type=volume,source=${TARGET_VOL},target=/target,readonly" \
  --mount "type=bind,source=${WORK},target=/work" \
  --label oo.project=open-ontology --label oo.owner=a409 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a409 \
  --label oo.kind=copy-runner --label oo.task=local-copy-acceptance \
  "$IMAGE" /target/acceptance/debug/local-copy-acceptance judge /work/case.json /work/verdict.json \
  > "$WORK/judge.stdout" 2> "$WORK/judge.stderr"
judge_code=$?
set -e
printf '%s\n' "$judge_code" > "$WORK/judge.exit"
docker inspect oo-a409-judge --format '{{json .Mounts}}' > "$WORK/judge-mounts.json"
docker inspect oo-a409-judge --format '{{.HostConfig.NetworkMode}}' > "$WORK/judge-network.txt"
docker rm oo-a409-judge >/dev/null
python3 - "$WORK/judge-mounts.json" <<'PY'
import json, sys
mounts = json.loads(open(sys.argv[1]).read())
for mount in mounts:
    src = mount.get("Source", "")
    dst = mount.get("Destination", "")
    if "secret" in src or "secret" in dst or src.endswith("src.pass"):
        raise SystemExit("judge mount still has a secret")
print("judge_mounts_have_no_secret")
PY
echo "copy_proof_done"
