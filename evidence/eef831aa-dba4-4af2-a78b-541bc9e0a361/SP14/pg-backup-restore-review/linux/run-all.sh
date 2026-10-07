#!/bin/bash
# One Rust builder, then one private PostgreSQL 17.10, then the independent restore proof.
# Does not pull, prune, publish a port, or use the host compiler.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a361
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP14/pg-backup-restore-review
ROOT=/tmp/oo-a361-${RUN_ID}
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NET=oo-a361-eef831aa-net
BNAME=oo-a361-build
PGNAME=oo-a361-pg
CARGO_VOL=oo-a361-eef831aa-cargo
TARGET_VOL=oo-a361-eef831aa-target
BIN=/var/lib/postgresql/a361-work/pg-backup-restore
OPERATOR=/run-in/operator.json

mkdir -p "$EVIDENCE/linux/build/phases" "$EVIDENCE/live/phases" "$EVIDENCE/pg" "$EVIDENCE/origin" "$ROOT/out" "$ROOT/run"
chmod 755 "$EVIDENCE/linux/inside.sh" "$EVIDENCE/linux/run-all.sh" "$EVIDENCE/linux/cleanup.sh" \
  "$EVIDENCE/linux/private-glibc.sh" "$EVIDENCE/linux/live-phases.sh" "$EVIDENCE/linux/psql-exec.sh" \
  "$EVIDENCE/linux/psql-script.sh" "$EVIDENCE/pg/up.sh" "$EVIDENCE/pg/pg-wrap.sh"
if [ ! -f "$EVIDENCE/linux/pre-edit-smoke/inside.exit" ] || [ "$(cat "$EVIDENCE/linux/pre-edit-smoke/inside.exit")" != 0 ]; then
  echo "pre-edit smoke is missing" >&2
  exit 2
fi
if [ -f "$EVIDENCE/linux/build/inside.exit" ]; then
  echo "build evidence already exists" >&2
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

if ! docker image inspect "$RUST_IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/linux/build/docker-run.exit"
  exit 1
fi
docker image inspect "$RUST_IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$EVIDENCE/linux/build/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/linux/build/image.txt"
own=$(docker ps -q --filter label=oo.owner=a361 --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/linux/build/capacity.txt"
printf '%s\n' "$own" > "$EVIDENCE/linux/build/builders-before.txt"
printf '%s\n' "$host_cargo" > "$EVIDENCE/linux/build/host-cargo.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/linux/build/docker-run.exit"
  exit 3
fi
if docker container inspect "$BNAME" >/dev/null 2>&1; then
  echo "named builder already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/linux/build/docker-run.exit"
  exit 2
fi
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create \
    --label oo.project=open-ontology \
    --label oo.owner=a361 \
    --label oo.session="${RUN_ID}" \
    --label oo.kind=network \
    "$NET" >/dev/null
fi
ensure_volume() {
  vol=$1
  kind=$2
  if docker volume inspect "$vol" >/dev/null 2>&1; then
    owner=$(docker volume inspect --format '{{index .Labels "oo.owner"}}' "$vol")
    session=$(docker volume inspect --format '{{index .Labels "oo.session"}}' "$vol")
    if [ "$owner" != "a361" ] || [ "$session" != "$RUN_ID" ]; then
      echo "volume ${vol} is not owned by this session" >&2
      exit 2
    fi
    printf '%s\n' "reused ${vol}"
  else
    docker volume create --label oo.project=open-ontology --label oo.owner=a361 --label oo.session="${RUN_ID}" --label oo.kind="${kind}" "$vol"
  fi
}
ensure_volume "$CARGO_VOL" rust-builder > "$EVIDENCE/linux/build/volume-cargo.txt"
ensure_volume "$TARGET_VOL" rust-builder > "$EVIDENCE/linux/build/volume-target.txt"
printf '%s\n' "1d88fe8ad9fa4a113c105e1df4d0903e517fc9a9" > "$EVIDENCE/linux/build/fixed-commit.txt"
printf '%s\n' "docker run --name ${BNAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 pinned-rust-1.94 bash /inside.sh" > "$EVIDENCE/linux/build/invocation.txt"

set +e
docker run --name "$BNAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory 6g \
  --memory-swap 6g \
  --cpus 1 \
  --pids-limit 4096 \
  --network "$NET" \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target/review \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${REPO}/tools/pg-backup-restore,target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/oracle,target=/oracle,readonly" \
  --mount "type=bind,source=${EVIDENCE}/linux/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${ROOT}/out,target=/out" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a361 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=rust-builder \
  --label oo.task=pg-backup-restore-review \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  "$RUST_IMAGE" \
  bash /inside.sh > "$EVIDENCE/linux/build/console.log" 2>&1
build_code=$?
set -e
printf '%s\n' "$build_code" > "$EVIDENCE/linux/build/docker-run.exit"
docker inspect "$BNAME" --format 'oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} memory={{.HostConfig.Memory}} nano={{.HostConfig.NanoCpus}}' > "$EVIDENCE/linux/build/hostconfig.txt" || true
docker inspect "$BNAME" --format '{{json .Mounts}}' > "$EVIDENCE/linux/build/mounts.txt" || true
docker rm -f "$BNAME" >/dev/null
printf '%s\n' removed > "$EVIDENCE/linux/build/container-removed.txt"
if [ "$build_code" -ne 0 ]; then
  echo "builder exit ${build_code}" >&2
  exit "$build_code"
fi
if [ ! -x "$ROOT/out/pg-backup-restore" ] || [ ! -x "$ROOT/out/independent-gold" ]; then
  echo "release binary is missing" >&2
  exit 1
fi
mkdir -p /tmp/oo-a361-hash-bin
cp "$ROOT/out/pg-backup-restore" /tmp/oo-a361-hash-bin/pg-backup-restore
chmod 755 /tmp/oo-a361-hash-bin/pg-backup-restore

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

cat > "$ROOT/run/operator.json" <<EOF
{
  "backup_dir": "/var/lib/postgresql/a361-backup",
  "caller": "trusted_local_admin",
  "host": "/var/run/postgresql",
  "lineage_version": "pg-backup-restore/v1",
  "maintenance_database": "postgres",
  "owner": "a356",
  "password_file": "/var/lib/postgresql/a361-admin.pass",
  "port": "5432",
  "report_path": "/var/lib/postgresql/a361-work/readiness.json",
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
  "work_dir": "/var/lib/postgresql/a361-work"
}
EOF

docker exec -u 0 "$PGNAME" mkdir -p /var/lib/postgresql/a361-work /var/lib/postgresql/a361-backup
docker exec -u 0 "$PGNAME" install -m 600 /secrets/admin.pass /var/lib/postgresql/a361-admin.pass
docker exec -u 0 "$PGNAME" stat -c '%a %n' /var/lib/postgresql/a361-admin.pass > "$EVIDENCE/pg/password-mode.txt"
grep -q '^600 ' "$EVIDENCE/pg/password-mode.txt"
docker cp "$ROOT/out/pg-backup-restore" "$PGNAME":"$BIN"
docker cp "$ROOT/out/independent-gold" "$PGNAME":/var/lib/postgresql/a361-work/independent-gold
docker exec -u 0 "$PGNAME" chmod 755 "$BIN" /var/lib/postgresql/a361-work/independent-gold

# shellcheck source=private-glibc.sh
source "$EVIDENCE/linux/private-glibc.sh"
install_private_glibc
# shellcheck source=live-phases.sh
source "$EVIDENCE/linux/live-phases.sh"
