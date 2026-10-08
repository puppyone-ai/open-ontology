#!/bin/bash
# One linux/arm64 Rust 1.94 builder. Postgres oo-a404-pg is already running.
# Product and identity come from immutable git trees, not the moving worktree.
set -euo pipefail
OWNER=a404
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a404
PRODUCT=fbd0467617a3c1514e7ffbef52f1138e20416ac6
IDENTITY=a6f92da8a0bf872f8a30efb116c2c7b14463505b
PRODUCER=9d5df65b63d3fe6c94d92d02c41d67b2b5c8aed3
PRODUCT_TREE=7375587fb4758d41ef02727451daf3473c29cac4
IDENTITY_TREE=45f330af6e326d8f1cc1038ef47de21031ef751a
PRODUCER_TREE=5a3692de6f1e5609d684164a34af672b11c163b6
BUILDER_TREE=40f74d7e9aac9c13e95d1da3d61d689184773c77
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE="$REPO/evidence/${RUN_ID}/SP08/release-control-api-review"
BUILD_EVIDENCE="$EVIDENCE/linux/build-1"
ROOT=/tmp/oo-a404-${RUN_ID}
ARCH="$ROOT/archive"
NET=oo-a404-eef831aa-net
PG_HOST=oo-a404-pg
CARGO_VOL=oo-a404-eef831aa-cargo
TARGET_VOL=oo-a404-eef831aa-target
BUILD=oo-a404-build
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
INSIDE="$EVIDENCE/linux/harness/oo-a404-inside.sh"

mkdir -p "$BUILD_EVIDENCE" "$EVIDENCE/linux/archive" "$ARCH"
cd "$REPO"
pin() {
  local name="$1" commit="$2" path="$3" expect="$4"
  local got
  got=$(git rev-parse "${commit}:${path}")
  printf '%s %s %s %s\n' "$name" "$commit" "$path" "$got" >> "$EVIDENCE/linux/archive/trees.txt"
  if [ "$got" != "$expect" ]; then
    echo "tree pin mismatch $name" >&2
    exit 2
  fi
  git archive "$commit" "$path" | tar -x -C "$ARCH"
}
: > "$EVIDENCE/linux/archive/trees.txt"
pin product "$PRODUCT" tools/release-control-api "$PRODUCT_TREE"
pin identity "$IDENTITY" tools/control-identity "$IDENTITY_TREE"
pin producer "$PRODUCER" tools/release-coordinator "$PRODUCER_TREE"
pin oo-builder "$PRODUCT" crates/oo-builder "$BUILDER_TREE"
cp "$REPO/Cargo.lock" "$ROOT/Cargo.lock"
shasum -a 256 "$ROOT/Cargo.lock" | awk '{print $1}' > "$EVIDENCE/linux/archive/root-lock.sha256"
git rev-parse HEAD > "$BUILD_EVIDENCE/worktree-head.txt"
git status --porcelain -- tools/control-identity > "$BUILD_EVIDENCE/identity-worktree.txt" || true

own=$(docker ps -aq --filter label=oo.kind=rust-builder --filter label=oo.owner="$OWNER" | wc -l | tr -d ' ')
global=$(docker ps -q --filter label=oo.kind=rust-builder | wc -l | tr -d ' ')
rust_images=$(docker ps -q --filter "ancestor=${RUST_IMAGE}" | wc -l | tr -d ' ')
host_tools=$(ps ax -o command= | awk 'BEGIN{n=0} $1 ~ /(^|\/)(cargo|rustc|rustdoc|rustup)$/ {n++} END{print n+0}')
printf 'own=%s global=%s rust_images=%s host_tools=%s\n' "$own" "$global" "$rust_images" "$host_tools" | tee "$BUILD_EVIDENCE/capacity-build.txt"
if [ "$own" != 0 ] || [ "$global" -ge 10 ] || [ "$rust_images" -ge 10 ] || [ "$host_tools" != 0 ]; then
  echo "builder capacity refused" >&2
  printf '%s\n' 2 > "$BUILD_EVIDENCE/docker-run.exit"
  exit 2
fi
if ! docker inspect oo-a404-pg --format '{{.State.Running}}' | grep -qx true; then
  echo "own pg is not running" >&2
  printf '%s\n' 2 > "$BUILD_EVIDENCE/docker-run.exit"
  exit 2
fi

docker volume create "$CARGO_VOL" >/dev/null
docker volume create "$TARGET_VOL" >/dev/null
printf '%s\n' "docker run --name $BUILD --platform linux/arm64 --pull never --network $NET --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 bash /inside.sh" > "$BUILD_EVIDENCE/invocation.txt"

set +e
docker run --name "$BUILD" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --memory 6g \
  --memory-swap 6g \
  --cpus 1 \
  --pids-limit 4096 \
  --network "$NET" \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e RUSTUP_HOME=/usr/local/rustup \
  -e PATH=/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
  -e CARGO_TERM_COLOR=never \
  -e OO_A392_PG_HOST="$PG_HOST" \
  -e OO_A392_PG_PORT=5432 \
  -e OO_A392_PG_DATABASE=oo_a404 \
  -e OO_A392_API_PASSWORD_FILE=/secrets/api.pass \
  -e OO_A392_PRODUCER_PASSWORD_FILE=/secrets/producer.pass \
  -e OO_A392_READER_PASSWORD_FILE=/secrets/reader.pass \
  -e OO_A392_IDENTITY_PASSWORD_FILE=/secrets/identity.pass \
  -e OO_A392_PG_CA=/secrets/ca.crt \
  -e OO_A392_CERT_DIR=/secrets \
  -e OO_A392_PROOF=/evidence/linux/proof/private-pg.json \
  -e OO_A404_PG_HOST="$PG_HOST" \
  -e OO_A404_PG_PORT=5432 \
  -e OO_A404_PG_DATABASE=oo_a404_csrf \
  -e OO_A404_OBSERVE_DATABASE=oo_a404 \
  -e OO_A404_PG_CA=/secrets/ca.crt \
  -e OO_A404_CERT_DIR=/secrets \
  -e OO_A404_IDENTITY_PASSWORD_FILE=/secrets/identity.pass \
  -e OO_A404_PRODUCER_PASSWORD_FILE=/secrets/producer.pass \
  -e OO_A404_READER_PASSWORD_FILE=/secrets/reader.pass \
  -e OO_A404_CSRF_OUT=/evidence/linux/csrf/probe.json \
  -e OO_A404_OBSERVE_OUT=/evidence/linux/csrf/observe.json \
  --mount "type=bind,source=${REPO},target=/repo,readonly" \
  --mount "type=bind,source=${ARCH}/tools/release-control-api,target=/repo/tools/release-control-api,readonly" \
  --mount "type=bind,source=${ARCH}/tools/control-identity,target=/repo/tools/control-identity,readonly" \
  --mount "type=bind,source=${ARCH}/tools/release-coordinator,target=/repo/tools/release-coordinator,readonly" \
  --mount "type=bind,source=${ARCH}/crates/oo-builder,target=/repo/crates/oo-builder,readonly" \
  --mount "type=bind,source=${ROOT}/Cargo.lock,target=/repo/Cargo.lock,readonly" \
  --mount "type=bind,source=${ROOT},target=/secrets,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${INSIDE},target=/inside.sh,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner="$OWNER" \
  --label oo.session="$RUN_ID" \
  --label oo.kind=rust-builder \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  "$RUST_IMAGE" \
  bash /inside.sh > "$BUILD_EVIDENCE/console.log" 2>&1
build_exit=$?
set -e
printf '%s\n' "$build_exit" > "$BUILD_EVIDENCE/docker-run.exit"
docker inspect "$BUILD" --format 'oom={{.State.OOMKilled}} exit={{.State.ExitCode}} privileged={{.HostConfig.Privileged}} memory={{.HostConfig.Memory}} cpus={{.HostConfig.NanoCpus}}' > "$BUILD_EVIDENCE/hostconfig.txt" || true
docker rm "$BUILD" >/dev/null || true
if [ "$build_exit" -ne 0 ]; then
  echo "BUILD_FAILED exit=$build_exit"
  exit "$build_exit"
fi
echo BUILD_OK
