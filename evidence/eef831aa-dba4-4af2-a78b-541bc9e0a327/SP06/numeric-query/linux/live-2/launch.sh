#!/bin/bash
# Bounded numeric proof builder. Fixed archive plus the six owned Rust files.
# Origin: evidence/.../numeric-query/linux/live/launch.sh
# Changes: live-2 evidence, policy.rs and policy_catalog.rs overlay, archive extract.
# Does not overwrite smoke, postedit, or linux/live logs.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a327/SP06/numeric-query
LIVE=${EVIDENCE}/linux/live-2
ROOT=/Users/redux/puppy-one/oo-a327-eef831aa-dba4-4af2-a78b-541bc9e0a327
SEC=${ROOT}/secrets
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
FIXED=e6ebf93b5ac261c097a24d145611e0300e4f88b5
NAME=oo-a327-live2
NET=oo-a327-eef831aa-net
CARGO_VOL=oo-a327-eef831aa-cargo
TARGET_VOL=oo-a327-eef831aa-target

chmod 755 "$LIVE/inside.sh" "$LIVE/launch.sh" "$LIVE/extract-libclang.sh"
mkdir -p "$LIVE/phases"
if [ -f "$LIVE/console.log" ]; then
  echo "refusing to overwrite live-2 console.log" >&2
  printf '%s\n' 2 > "$LIVE/docker-run.exit"
  exit 2
fi
if [ ! -f "$SEC/ca.crt" ] || [ ! -f "$SEC/admin.pass" ]; then
  echo "private secrets are missing" >&2
  printf '%s\n' 2 > "$LIVE/docker-run.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$LIVE/docker-run.exit"
  exit 1
fi

git -C "$SRC" rev-parse "$FIXED" > "$LIVE/fixed-commit.txt"
mkdir -p "$ROOT"
rm -rf "$ROOT/archive" "$ROOT/live-work"
mkdir -p "$ROOT/archive"
git -C "$SRC" archive --format=tar "$FIXED" Cargo.toml Cargo.lock rust-toolchain.toml crates \
  | tar -xf - -C "$ROOT/archive"
if [ -f "$ROOT/archive/crates/oo-gateway/tests/a327_numeric.rs" ]; then
  echo "archive already contains the numeric test" >&2
  printf '%s\n' 2 > "$LIVE/docker-run.exit"
  exit 2
fi
mkdir -p "$ROOT/live-work"
cp -a "$ROOT/archive/." "$ROOT/live-work/"
cp "$SRC/crates/oo-gateway/src/reviewed_policy.rs" "$ROOT/live-work/crates/oo-gateway/src/reviewed_policy.rs"
cp "$SRC/crates/oo-gateway/src/query_executor.rs" "$ROOT/live-work/crates/oo-gateway/src/query_executor.rs"
cp "$SRC/crates/oo-gateway/src/bin/query-pg.rs" "$ROOT/live-work/crates/oo-gateway/src/bin/query-pg.rs"
cp "$SRC/crates/oo-gateway/tests/a327_numeric.rs" "$ROOT/live-work/crates/oo-gateway/tests/a327_numeric.rs"
cp "$SRC/crates/oo-gateway/src/policy.rs" "$ROOT/live-work/crates/oo-gateway/src/policy.rs"
cp "$SRC/crates/oo-gateway/src/policy_catalog.rs" "$ROOT/live-work/crates/oo-gateway/src/policy_catalog.rs"
if ! grep -q 'fn grantable_base_type' "$ROOT/live-work/crates/oo-gateway/src/policy_catalog.rs"; then
  echo "overlay copy is missing the numeric bound" >&2
  printf '%s\n' 2 > "$LIVE/docker-run.exit"
  exit 2
fi

docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a327 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a327 \
  --label oo.kind=network \
  "$NET" >/dev/null

ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}' > "$LIVE/host-cargo-count.txt"
host_n=$(cat "$LIVE/host-cargo-count.txt")
docker ps --filter status=running --filter label=oo.kind=rust-builder --format '{{.Names}}' > "$LIVE/builders-before.txt" || true
builder_n=$(grep -c . "$LIVE/builders-before.txt" || true)
printf 'host_cargo=%s rust_builders=%s cap_global=10 cap_own=1\n' "$host_n" "$builder_n" > "$LIVE/capacity.txt"
if [ "$builder_n" -ge 10 ] || [ "$host_n" -ge 1 ]; then
  echo "deferred: rust builder capacity host=$host_n builders=$builder_n" >&2
  printf '%s\n' 3 > "$LIVE/docker-run.exit"
  exit 3
fi
own=$(docker ps -q --filter status=running --filter label=oo.owner=a327 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$LIVE/docker-run.exit"
  exit 3
fi

docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a327 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a327 \
  --label oo.kind=cargo \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a327 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a327 \
  --label oo.kind=target \
  "$TARGET_VOL" >/dev/null

printf '%s\n' "$FIXED" > "$LIVE/fixed-commit.txt"
cat > "$LIVE/invocation.txt" <<EOF
run_id=eef831aa-dba4-4af2-a78b-541bc9e0a327
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
container=${NAME}
memory=6g
cpus=1
privileged=no
network=${NET}
pull=never
phase=live-2
archive=${ROOT}/archive
work=${ROOT}/live-work
fixed=${FIXED}
allowlist=e9cba7df4b8d20ea8b3794f9efcc813434d7b1dd
addendum=c4d9316d28ceefefc7bc21cd8ac61ce949232d07
origin=evidence/eef831aa-dba4-4af2-a78b-541bc9e0a327/SP06/numeric-query/linux/live/launch.sh
private_root=${ROOT}
private_root_reason=docker_cannot_bind_mount_/tmp
EOF

set +e
docker run --name "$NAME" \
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
  -e CARGO_TARGET_DIR=/target/a327 \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  -e OO_A327_HOST=oo-a327-pg \
  -e OO_A327_PORT=5432 \
  -e OO_A327_ADMIN_USER=oo_fixture_admin \
  -e OO_A327_ADMIN_PASSWORD_FILE=/secrets/admin.pass \
  -e OO_A327_READER_PASSWORD_FILE=/secrets/reader.pass \
  -e OO_A327_RUNTIME_PASSWORD_FILE=/secrets/runtime.pass \
  -e OO_A327_LOGIN_PASSWORD_FILE=/secrets/login.pass \
  -e OO_A327_CA_FILE=/secrets/ca.crt \
  -e OO_A327_EVIDENCE=/evidence/linux/live-2 \
  -e OO_A327_PIPELINE=/target/a327/debug/oo-pipeline \
  -e OO_A327_BUILDER=/target/a327/debug/oo-builder \
  -e OO_A327_REVIEWED_EFFECTIVE=/target/a327/debug/reviewed-effective-oos \
  -e OO_A327_ROLE_PACK_BUNDLE=/target/a327/debug/role-pack-bundle \
  -e OO_A327_REVIEWED_POLICY=/target/a327/debug/reviewed-policy \
  -e OO_A327_INSTALL=/target/a327/debug/install-column-acl \
  -e OO_A327_MAPPING=/target/a327/debug/provision-mapping-reader \
  -e OO_A327_ANALYZE=/target/a327/debug/analyze-pg \
  -e OO_A327_INIT=/target/a327/debug/init-pg \
  -e OO_A327_QUERY=/target/a327/debug/query-pg \
  --mount "type=bind,source=${ROOT}/live-work,target=/src,readonly" \
  --mount "type=bind,source=${LIVE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${LIVE}/extract-libclang.sh,target=/extract-libclang.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE}/tools/sha256.rs,target=/tools/sha256.rs,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a327 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a327 \
  --label oo.task=numeric-query-live-2 \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$LIVE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$LIVE/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME" 2>/dev/null || echo missing)"
} > "$LIVE/hostconfig.txt"
docker rm "$NAME" >/dev/null 2>&1 || true
exit "$run_exit"
