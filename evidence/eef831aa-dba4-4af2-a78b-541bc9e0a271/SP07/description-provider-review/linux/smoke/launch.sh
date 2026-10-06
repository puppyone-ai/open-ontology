#!/bin/bash
# Smoke the fixed 9951cda7 archive before the review overlay.
# Origin: a204 preoverlay launch, via the a264 smoke pair.
# Memory 6g, one CPU, no PostgreSQL, no pull, no prune.
set -u
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a271/SP07/description-provider-review
EVIDENCE=${REVIEW}/linux/smoke
ROOT=/tmp/oo-a271-eef831aa-dba4-4af2-a78b-541bc9e0a271
FIXED=9951cda7f128e30f1f6ce02e409d542d20a9e4fb
PROV=aba067efe4b19bae868e15e23157ebbcf0a4969f
REP=56b791368d7b5ec050aa36ac91d33bc2234bc4d2
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a271-smoke
NET=oo-a271-eef831aa-net
CARGO_VOL=oo-a271-eef831aa-cargo
TARGET_VOL=oo-a271-eef831aa-target

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite smoke console.log" >&2
  exit 2
fi
mkdir -p "$REVIEW/source/blobs/tools/description-provider/src" \
  "$REVIEW/source/blobs/tools/description-provider/tests" \
  "$REVIEW/source/blobs/crates/oo-builder/src/bin" \
  "$ROOT" "$EVIDENCE/phases"
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/fixed-tree.txt"

same() {
  local left right
  left=$(git -C "$SRC" rev-parse "$1:$3")
  right=$(git -C "$SRC" rev-parse "$2:$3")
  if [ "$left" != "$right" ]; then
    echo "blob mismatch $3" >&2
    echo "  $1 $left" >&2
    echo "  $2 $right" >&2
    return 1
  fi
  printf '%s %s %s\n' "$3" "$1" "$left"
}

set +e
{
  set -e
  echo "fixed=$FIXED"
  echo "fixed_tree=$(git -C "$SRC" rev-parse "${FIXED}^{tree}")"
  echo "provider_commit=$PROV"
  echo "repaired_library_commit=$REP"
  same "$FIXED" "$PROV" tools/description-provider/CONTRACT.md
  same "$FIXED" "$PROV" tools/description-provider/Cargo.lock
  same "$FIXED" "$PROV" tools/description-provider/Cargo.toml
  same "$FIXED" "$PROV" tools/description-provider/src/chat.rs
  same "$FIXED" "$PROV" tools/description-provider/src/config.rs
  same "$FIXED" "$PROV" tools/description-provider/src/error.rs
  same "$FIXED" "$PROV" tools/description-provider/src/lib.rs
  same "$FIXED" "$PROV" tools/description-provider/src/main.rs
  same "$FIXED" "$PROV" tools/description-provider/src/price.rs
  same "$FIXED" "$PROV" tools/description-provider/src/transport.rs
  same "$FIXED" "$PROV" tools/description-provider/tests/provider.rs
  same "$FIXED" "$PROV" tools/description-provider/.gitignore
  same "$FIXED" "$REP" crates/oo-builder/src/description_candidate.rs
  same "$FIXED" "$REP" crates/oo-builder/src/lib.rs
  same "$FIXED" "$REP" crates/oo-builder/src/bin/description-candidate.rs
  same "$FIXED" "$REP" Cargo.lock
} > "$EVIDENCE/source-identity.txt"
identity=$?
set -e
if [ "$identity" -ne 0 ]; then
  echo "fixed tree does not match aba067ef and 56b79136" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi

save_blob() {
  git -C "$SRC" show "$FIXED:$1" > "$REVIEW/source/blobs/$1"
}
save_blob tools/description-provider/CONTRACT.md
save_blob tools/description-provider/Cargo.lock
save_blob tools/description-provider/Cargo.toml
save_blob tools/description-provider/src/chat.rs
save_blob tools/description-provider/src/config.rs
save_blob tools/description-provider/src/error.rs
save_blob tools/description-provider/src/lib.rs
save_blob tools/description-provider/src/main.rs
save_blob tools/description-provider/src/price.rs
save_blob tools/description-provider/src/transport.rs
save_blob tools/description-provider/tests/provider.rs
save_blob tools/description-provider/.gitignore
save_blob crates/oo-builder/src/description_candidate.rs
save_blob crates/oo-builder/src/lib.rs
save_blob crates/oo-builder/src/bin/description-candidate.rs
save_blob Cargo.lock

rm -rf "$ROOT/archive"
mkdir -p "$ROOT/archive"
git -C "$SRC" archive --format=tar "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates contracts tools/description-provider \
  | tar -x -C "$ROOT/archive"
if [ ! -f "$ROOT/archive/tools/description-provider/Cargo.toml" ]; then
  echo "archive missing the committed description provider" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ ! -f "$ROOT/archive/crates/oo-builder/src/bin/description-candidate.rs" ]; then
  echo "archive missing the description-candidate CLI" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ -f "$ROOT/archive/tools/description-provider/tests/provider_a271.rs" ]; then
  echo "archive already contains the review overlay" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if ! cmp -s "$ROOT/archive/tools/description-provider/src/lib.rs" "$REVIEW/source/blobs/tools/description-provider/src/lib.rs" \
  || ! cmp -s "$ROOT/archive/tools/description-provider/tests/provider.rs" "$REVIEW/source/blobs/tools/description-provider/tests/provider.rs" \
  || ! cmp -s "$ROOT/archive/tools/description-provider/Cargo.lock" "$REVIEW/source/blobs/tools/description-provider/Cargo.lock" \
  || ! cmp -s "$ROOT/archive/crates/oo-builder/src/description_candidate.rs" "$REVIEW/source/blobs/crates/oo-builder/src/description_candidate.rs" \
  || ! cmp -s "$ROOT/archive/Cargo.lock" "$REVIEW/source/blobs/Cargo.lock"
then
  echo "archive drifted from the saved blobs" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
printf '%s\n' "$FIXED" > "$ROOT/archive/.fixed-base-sha"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a271 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a271 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a271 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a271 \
  --label oo.kind=cargo-home \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a271 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a271 \
  --label oo.kind=cargo-target \
  "$TARGET_VOL" >/dev/null
own=$(docker ps -q --filter label=oo.owner=a271 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/containers-before.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt" || {
  echo "image is not arm64" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
}
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named smoke container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 bash /inside.sh" > "$EVIDENCE/invocation.txt"
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
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${REVIEW}/source/blobs,target=/blobs,readonly" \
  --mount "type=bind,source=${REVIEW}/tools,target=/tools,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a271 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a271 \
  --label oo.task=description-provider-review-smoke \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME")"
  echo "ReadonlyRootfs=$(docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' "$NAME")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
} > "$EVIDENCE/hostconfig.txt"
docker inspect -f '{{json .HostConfig.Tmpfs}}' "$NAME" > "$EVIDENCE/tmpfs.txt"
docker inspect -f '{{range .Mounts}}{{.Type}} {{.Source}} {{.Destination}} rw={{.RW}}{{println}}{{end}}' "$NAME" > "$EVIDENCE/mounts.txt"
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
exit "$run_exit"
