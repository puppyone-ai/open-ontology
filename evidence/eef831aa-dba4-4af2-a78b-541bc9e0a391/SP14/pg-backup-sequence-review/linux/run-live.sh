#!/bin/bash
# One private PostgreSQL 17.10. Mechanism probes, then the frozen 100k cut.
# Does not pull, prune, or start a cargo builder.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a391
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP14/pg-backup-sequence-review
ROOT=${EVIDENCE}/runtime
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
PGNAME=oo-a391-pg
BIN=/var/lib/postgresql/a391-work/pg-backup-restore
OPERATOR=/run-in/operator.json
export RUN_ID REPO EVIDENCE ROOT RUST_IMAGE PGNAME BIN OPERATOR

mkdir -p "$EVIDENCE/live/phases" "$EVIDENCE/origin" "$EVIDENCE/pg" "$ROOT/run"
chmod 755 "$EVIDENCE/linux/run-live.sh" "$EVIDENCE/linux/private-glibc.sh" "$EVIDENCE/linux/mechanism.sh" \
  "$EVIDENCE/linux/live-phases.sh" "$EVIDENCE/linux/identity.sh" "$EVIDENCE/linux/psql-exec.sh" \
  "$EVIDENCE/pg/up.sh" "$EVIDENCE/pg/pg-wrap.sh"
if [ ! -f "$EVIDENCE/linux/build/inside.exit" ] || [ "$(cat "$EVIDENCE/linux/build/inside.exit")" != 0 ]; then
  echo "build is missing" >&2
  exit 2
fi
if [ -f "$EVIDENCE/live/run-live.exit" ]; then
  echo "live evidence already exists" >&2
  exit 2
fi
{
  echo "a274 pg-wrap d53f478805b2e721c81d5d8befafe70d4e6bac71af6136d2531149f70afe2bbb"
  echo "a274 up ec1dd1087fddfebfe22c89ee21677ed6c81eba75f7ba4e6a55b2d867daa02c76"
  echo "a274 focused launch e9fd0027356f5fc90ebe56c0070f1078ba9f0f8d6468fbbecd4db98b4ff16910"
  echo "a274 sha256 tool 55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e"
  shasum -a 256 \
    "$REPO/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a274/SP11/pg-onboarding-review/pg/pg-wrap.sh" \
    "$REPO/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a274/SP11/pg-onboarding-review/pg/up.sh" \
    "$REPO/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a274/SP11/pg-onboarding-review/linux/focused/launch.sh" \
    "$REPO/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a274/SP11/pg-onboarding-review/tools/sha256.rs" \
    "$EVIDENCE/pg/pg-wrap.sh"
} > "$EVIDENCE/origin/recipes.txt"
grep -q 'd53f478805b2e721c81d5d8befafe70d4e6bac71af6136d2531149f70afe2bbb  '"$EVIDENCE/pg/pg-wrap.sh" "$EVIDENCE/origin/recipes.txt"

bash "$EVIDENCE/pg/up.sh"
REAL_TOOLDIR=$(docker exec "$PGNAME" sh -c 'dirname "$(command -v pg_dump)"')
export REAL_TOOLDIR
printf '%s\n' "$REAL_TOOLDIR" > "$EVIDENCE/pg/tool-dir.txt"
case "$REAL_TOOLDIR" in
  *homebrew*|*Cellar*) echo "tool dir is a host package path" >&2; exit 1;;
  /*) ;;
  *) echo "tool dir is not absolute" >&2; exit 1;;
esac
docker exec "$PGNAME" "$REAL_TOOLDIR/pg_dump" --version > "$EVIDENCE/pg/pg-dump-version.txt"
docker exec "$PGNAME" "$REAL_TOOLDIR/pg_restore" --version > "$EVIDENCE/pg/pg-restore-version.txt"
docker exec "$PGNAME" "$REAL_TOOLDIR/psql" --version > "$EVIDENCE/pg/psql-version.txt"
grep -q '(PostgreSQL) 17.10' "$EVIDENCE/pg/pg-dump-version.txt"
grep -q '(PostgreSQL) 17.10' "$EVIDENCE/pg/pg-restore-version.txt"
grep -q '(PostgreSQL) 17.10' "$EVIDENCE/pg/psql-version.txt"

install_dump_wrapper() {
  wrap=/var/lib/postgresql/a391-tools
  docker exec -u 0 "$PGNAME" mkdir -p "$wrap" /var/lib/postgresql/a391-work
  cat > "$ROOT/run/pg_dump" <<EOF
#!/bin/bash
set -euo pipefail
real=${REAL_TOOLDIR}/pg_dump
db=""
snap=""
fmt=""
defer=absent
schema=absent
table=absent
prev=""
for arg in "\$@"; do
  case "\$prev" in
    --dbname) db=\$arg ;;
  esac
  case "\$arg" in
    --snapshot=*) snap=\${arg#--snapshot=} ;;
    --format=*) fmt=\${arg#--format=} ;;
    --dbname=*) db=\${arg#--dbname=} ;;
    --schema-only|--schema=*|--table=*) schema=present ;;
    --serializable-deferrable) defer=present ;;
  esac
  case "\$arg" in
    --table) table=present ;;
    --schema) schema=present ;;
  esac
  prev=\$arg
done
if [ -n "\$db" ] && [ -n "\$snap" ]; then
  printf 'dbname=%s snapshot=%s deferrable=%s format=%s schema_filter=%s table_filter=%s\n' "\$db" "\$snap" "\$defer" "\$fmt" "\$schema" "\$table" >> /var/lib/postgresql/a391-work/dump-flags.txt
fi
exec "\$real" "\$@"
EOF
  docker cp "$ROOT/run/pg_dump" "$PGNAME:$wrap/pg_dump"
  docker exec -u 0 "$PGNAME" chmod 755 "$wrap/pg_dump"
  docker exec -u 0 "$PGNAME" ln -sfn "$REAL_TOOLDIR/pg_restore" "$wrap/pg_restore"
  docker exec -u 0 "$PGNAME" ln -sfn "$REAL_TOOLDIR/psql" "$wrap/psql"
  docker exec "$PGNAME" "$wrap/pg_dump" --version > "$EVIDENCE/pg/pg-dump-wrapper-version.txt"
  grep -q '(PostgreSQL) 17.10' "$EVIDENCE/pg/pg-dump-wrapper-version.txt"
  printf '%s\n' "$wrap" > "$EVIDENCE/pg/wrapper-dir.txt"
}

mkdir -p "$ROOT/run"
install_dump_wrapper
TOOLDIR=$(cat "$EVIDENCE/pg/wrapper-dir.txt")
export TOOLDIR

cat > "$ROOT/run/operator.json" <<EOF
{
  "backup_dir": "/var/lib/postgresql/a391-backup",
  "caller": "trusted_local_admin",
  "host": "/var/run/postgresql",
  "lineage_version": "pg-backup-restore/v1",
  "maintenance_database": "postgres",
  "owner": "a356",
  "password_file": "/var/lib/postgresql/a391-admin.pass",
  "port": "5432",
  "report_path": "/var/lib/postgresql/a391-work/readiness.json",
  "restore_databases": ["oo_a356_restore_control", "oo_a356_restore_agent"],
  "schema_root": "/schemas",
  "schemas": [
    {"database": "oo_a356_control", "file": "tools/cdc-offset-store/migrations/oo_cdc_offset_control_v1.sql", "id": "cdc-offset-store-v1"},
    {"database": "oo_a356_control", "file": "tools/session-store/migrations/oo_session_control_v1.sql", "id": "session-store-v1"},
    {"database": "oo_a356_control", "file": "tools/calendar-store/migrations/oo_calendar_control_v1.sql", "id": "calendar-store-v1"},
    {"database": "oo_a356_agent", "file": "tools/enum-store/migrations/oo_enum_control_v1.sql", "id": "enum-store-v1"}
  ],
  "session": "eef831aa-dba4-4af2-a78b-541bc9e0a356",
  "source_cut": "cut-a356-1",
  "target_fence": "fence-a356-restore",
  "source_databases": ["oo_a356_control", "oo_a356_agent"],
  "tool_dir": "${TOOLDIR}",
  "user": "oo_a356_admin",
  "work_dir": "/var/lib/postgresql/a391-work"
}
EOF

docker exec -u 0 "$PGNAME" mkdir -p /var/lib/postgresql/a391-work /var/lib/postgresql/a391-backup
docker exec -u 0 "$PGNAME" install -m 600 /secrets/admin.pass /var/lib/postgresql/a391-admin.pass
docker exec -u 0 "$PGNAME" stat -c '%a %n' /var/lib/postgresql/a391-admin.pass > "$EVIDENCE/pg/password-mode.txt"
grep -q '^600 ' "$EVIDENCE/pg/password-mode.txt"
docker cp "$ROOT/out/pg-backup-restore" "$PGNAME":"$BIN"
docker exec -u 0 "$PGNAME" chmod 755 "$BIN"

# shellcheck source=private-glibc.sh
source "$EVIDENCE/linux/private-glibc.sh"
install_private_glibc
docker cp "$EVIDENCE/linux/psql-exec.sh" "$PGNAME":/var/lib/postgresql/a391-work/psql-exec.sh
docker exec -u 0 "$PGNAME" chmod 755 /var/lib/postgresql/a391-work/psql-exec.sh

set +e
bash "$EVIDENCE/linux/mechanism.sh" > "$EVIDENCE/mechanism-run.out" 2> "$EVIDENCE/mechanism-run.err"
mech_code=$?
set -e
printf '%s\n' "$mech_code" > "$EVIDENCE/mechanism/script.exit"
if [ "$mech_code" -ne 0 ]; then
  echo "mechanism script exit ${mech_code}" >&2
fi

# shellcheck source=live-phases.sh
source "$EVIDENCE/linux/live-phases.sh"
set +e
bash "$EVIDENCE/linux/identity.sh" > "$EVIDENCE/identity-run.out" 2> "$EVIDENCE/identity-run.err"
ident_code=$?
set -e
printf '%s\n' "$ident_code" > "$EVIDENCE/identity/script.exit"

printf '%s\n' 0 > "$EVIDENCE/live/run-live.exit"
echo "live finished"
