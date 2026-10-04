#!/bin/bash
# One probe builder after the smoke builder has exited. Private PG is already up.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a157/SP07/captured-pg-draft-review
ROOT=/tmp/oo-a157-eef831aa
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a157-probe
CARGO_VOL=oo-a157-eef831aa-cargo
TARGET_VOL=oo-a157-eef831aa-target
NET=oo-a157-eef831aa-net
LOGVOL=oo-a157-eef831aa-pglog
FIXED=63466471a6d403e0a2a29c8d1ae064179da65bd8
SNAP=${ROOT}/probe-snapshot
OUT=${EVIDENCE}/probe/out
AUDIT=${EVIDENCE}/audit
SEC=/tmp/oo-a157-eef831aa-dba4-4af2-a78b-541bc9e0a157/secrets

mkdir -p "$OUT" "$AUDIT" "$ROOT"
chmod 755 "$EVIDENCE/probe/inside.sh" "$EVIDENCE/probe/launch.sh"

docker ps --filter label=oo.project=open-ontology --format 'name={{.Names}} kind={{.Label "oo.kind"}} owner={{.Label "oo.owner"}} status={{.Status}}' > "$EVIDENCE/probe/containers-before.txt" || true
running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a157 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
own_pg=$(docker ps -q --filter label=oo.owner=a157 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
printf 'running_rust_image_or_label=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s own_postgres_running=%s\n' "$running_rust" "$own" "$host_cargo" "$own_pg" > "$EVIDENCE/probe/capacity.txt"
if [ "$own" -ge 1 ] || [ "$((running_rust + host_cargo))" -ge 2 ]; then
  echo "deferred: rust builder capacity is full" >&2
  printf '%s\n' 3 > "$EVIDENCE/probe/launch.exit"
  exit 3
fi
if [ "$own_pg" -ne 1 ]; then
  echo "postgres is not running" >&2
  printf '%s\n' 2 > "$EVIDENCE/probe/launch.exit"
  exit 2
fi
test -f "$SEC/reader.pass"
test -f "$SEC/ca.crt"
test -f "$SEC/comment.txt"

rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$SRC" archive "$FIXED" Cargo.toml Cargo.lock rust-toolchain.toml crates .cargo | tar -x -C "$SNAP"
mkdir -p "$SNAP/probe/src/bin"
cp "$EVIDENCE/probe/Cargo.toml" "$SNAP/probe/Cargo.toml"
cp "$EVIDENCE/probe/src/main.rs" "$SNAP/probe/src/main.rs"
cp "$EVIDENCE/probe/src/bin/sql_audit.rs" "$SNAP/probe/src/bin/sql_audit.rs"
awk '
  $0 == "    \"crates/xtask\"," { print; print "    \"probe\","; next }
  { print }
' "$SNAP/Cargo.toml" > "$SNAP/Cargo.toml.new"
mv "$SNAP/Cargo.toml.new" "$SNAP/Cargo.toml"
grep -q '"probe"' "$SNAP/Cargo.toml"
test -f "$SNAP/crates/oo-builder/src/captured_pg.rs"
test -f "$SNAP/crates/oo-connectors/vendor/sqlx-core/Cargo.toml"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/probe/source-tree.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/probe/source-commit.txt"
shasum -a 256 \
  "$SNAP/crates/oo-builder/src/captured_pg.rs" \
  "$SNAP/crates/oo-builder/src/draft.rs" \
  "$SNAP/crates/oo-builder/src/pgsql.rs" \
  "$SNAP/crates/oo-builder/src/lib.rs" \
  "$SNAP/probe/src/main.rs" \
  "$SNAP/probe/src/bin/sql_audit.rs" \
  > "$EVIDENCE/probe/overlay.sha256"
cp "$SNAP/Cargo.lock" "$OUT/Cargo.lock.before"

rm -f "$OUT/capture.ready" "$OUT/alter.ready" "$OUT/capture.exit" "$OUT/stale.exit"

if docker container inspect "$NAME" >/dev/null 2>&1; then
  prior=$(docker inspect -f '{{.State.Status}}' "$NAME")
  if [ "$prior" = "running" ]; then
    echo "deferred: probe container is already running" >&2
    printf '%s\n' 3 > "$EVIDENCE/probe/launch.exit"
    exit 3
  fi
  docker rm "$NAME" >/dev/null
fi

cat > "$EVIDENCE/probe/invocation.txt" <<EOF
source_commit=${FIXED}
tree=$(cat "$EVIDENCE/probe/source-tree.txt")
snapshot_method=git archive of ${FIXED} plus evidence-only probe member
image=${RUST_IMAGE}
platform=linux/arm64
toolchain=1.94.0
container=${NAME}
network=${NET}
pull=never
privileged=no
pg=oo-a157-pg
EOF

set +e
docker run \
  --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory 8g \
  --memory-swap 8g \
  --cpus 4 \
  --pids-limit 4096 \
  --network "$NET" \
  --workdir /src \
  --entrypoint bash \
  -e CARGO_TERM_COLOR=never \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=4 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/tmp \
  -e PGHOST=oo-a157-pg \
  -e PGPORT=5432 \
  -e PGDATABASE=oo_a157 \
  -e PGUSER=oo_a157_reader \
  -e PASSWORD_FILE=/secrets/reader.pass \
  -e CA_FILE=/secrets/ca.crt \
  -e OUT_DIR=/evidence \
  --mount "type=bind,source=${SNAP},target=/src" \
  --mount "type=bind,source=${EVIDENCE}/probe/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${OUT},target=/evidence" \
  --mount "type=bind,source=${AUDIT},target=/audit" \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=volume,source=${LOGVOL},target=/pglog,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a157 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a157 \
  --label oo.task=captured-pg-draft-review-probe \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$RUST_IMAGE" \
  /inside.sh > "$EVIDENCE/probe/console.log" 2>&1 &
run_pid=$!

seen=0
for _ in $(seq 1 240); do
  if [ -f "$OUT/capture.ready" ]; then
    seen=1
    break
  fi
  if ! kill -0 "$run_pid" 2>/dev/null; then
    break
  fi
  sleep 5
done
if [ "$seen" -ne 1 ]; then
  wait "$run_pid" || true
  printf '%s\n' 1 > "$EVIDENCE/probe/launch.exit"
  exit 1
fi
set +e
docker exec -u postgres oo-a157-pg bash /fixture/alter.sh > "$EVIDENCE/pg/alter.stdout" 2> "$EVIDENCE/pg/alter.stderr"
alter_exit=$?
set -e
printf '%s\n' "$alter_exit" > "$EVIDENCE/pg/alter.exit"
if [ "$alter_exit" -ne 0 ]; then
  printf '%s\n' 1 > "$EVIDENCE/probe/launch.exit"
  wait "$run_pid" || true
  exit 1
fi
touch "$OUT/alter.ready"
set +e
wait "$run_pid"
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EVIDENCE/probe/launch.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
  echo "Status=$(docker inspect -f '{{.State.Status}}' "$NAME")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME")"
  echo "CapAdd=$(docker inspect -f '{{json .HostConfig.CapAdd}}' "$NAME")"
  echo "PortBindings=$(docker inspect -f '{{json .HostConfig.PortBindings}}' "$NAME")"
} > "$EVIDENCE/probe/hostconfig.txt"
if [ -f "$OUT/Cargo.lock.before" ]; then
  diff -u "$OUT/Cargo.lock.before" "$SNAP/Cargo.lock" > "$OUT/Cargo.lock.diff" || true
fi
exit "$run_exit"
