#!/bin/bash
# One private PostgreSQL 17.10 and the writer-blocked cut proof.
# Does not pull, prune, or start a cargo builder.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a368
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP14/pg-backup-quiescent-cut
ROOT=/tmp/oo-a368-${RUN_ID}
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
PGNAME=oo-a368-pg
BIN=/var/lib/postgresql/a368-work/pg-backup-restore
OPERATOR=/run-in/operator.json

mkdir -p "$EVIDENCE/live/phases" "$EVIDENCE/origin" "$EVIDENCE/pg"
chmod 755 "$EVIDENCE/linux/run-live.sh" "$EVIDENCE/linux/private-glibc.sh" "$EVIDENCE/linux/live-phases.sh" \
  "$EVIDENCE/linux/psql-exec.sh" "$EVIDENCE/pg/up.sh" "$EVIDENCE/pg/pg-wrap.sh"
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
TOOLDIR=$(docker exec "$PGNAME" sh -c 'dirname "$(command -v pg_dump)"')
printf '%s\n' "$TOOLDIR" > "$EVIDENCE/pg/tool-dir.txt"
case "$TOOLDIR" in
  *homebrew*|*Cellar*) echo "tool dir is a host package path" >&2; exit 1;;
  /*) ;;
  *) echo "tool dir is not absolute" >&2; exit 1;;
esac
docker exec "$PGNAME" "$TOOLDIR/pg_dump" --version > "$EVIDENCE/pg/pg-dump-version.txt"
docker exec "$PGNAME" "$TOOLDIR/pg_restore" --version > "$EVIDENCE/pg/pg-restore-version.txt"
docker exec "$PGNAME" "$TOOLDIR/psql" --version > "$EVIDENCE/pg/psql-version.txt"
grep -q '(PostgreSQL) 17.10' "$EVIDENCE/pg/pg-dump-version.txt"
grep -q '(PostgreSQL) 17.10' "$EVIDENCE/pg/pg-restore-version.txt"
grep -q '(PostgreSQL) 17.10' "$EVIDENCE/pg/psql-version.txt"

mkdir -p "$ROOT/run"
cat > "$ROOT/run/operator.json" <<EOF
{
  "backup_dir": "/var/lib/postgresql/a368-backup",
  "caller": "trusted_local_admin",
  "host": "/var/run/postgresql",
  "lineage_version": "pg-backup-restore/v1",
  "maintenance_database": "postgres",
  "owner": "a356",
  "password_file": "/var/lib/postgresql/a368-admin.pass",
  "port": "5432",
  "report_path": "/var/lib/postgresql/a368-work/readiness.json",
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
  "source_databases": ["oo_a356_control", "oo_a356_agent"],
  "target_fence": "fence-a356-restore",
  "tool_dir": "${TOOLDIR}",
  "user": "oo_a356_admin",
  "work_dir": "/var/lib/postgresql/a368-work"
}
EOF

docker exec -u 0 "$PGNAME" mkdir -p /var/lib/postgresql/a368-work /var/lib/postgresql/a368-backup
docker exec -u 0 "$PGNAME" install -m 600 /secrets/admin.pass /var/lib/postgresql/a368-admin.pass
docker exec -u 0 "$PGNAME" stat -c '%a %n' /var/lib/postgresql/a368-admin.pass > "$EVIDENCE/pg/password-mode.txt"
grep -q '^600 ' "$EVIDENCE/pg/password-mode.txt"
docker cp "$ROOT/out/pg-backup-restore" "$PGNAME":"$BIN"
docker cp "$ROOT/out/cut-check" "$PGNAME":/var/lib/postgresql/a368-work/cut-check
docker exec -u 0 "$PGNAME" chmod 755 "$BIN" /var/lib/postgresql/a368-work/cut-check

# shellcheck source=private-glibc.sh
source "$EVIDENCE/linux/private-glibc.sh"
install_private_glibc
# shellcheck source=live-phases.sh
source "$EVIDENCE/linux/live-phases.sh"
