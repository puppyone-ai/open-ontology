#!/bin/bash
# Independent description-pack review. One container. Does not overwrite an
# existing console.log. Does not prune. Starts no PostgreSQL. Does not write
# the product tree, the root lock, or tools/description-pack.
# Recipe origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/overlay-3/launch.sh
# at b2b3adabd05de94723d8ed13b33be0144a0de1f9, blob 03c8e15c60a376ef0a757c062ebd110063a0ffe6,
# content sha256 78c8ef8c179c59317850c1b6a9ae53d3f883b8a1b24c2567fa0d3d26f1997eee.
# Review shape: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a288/SP07/description-review-review/linux/proof-4/launch.sh
# content sha256 0cf5c779d5b593619aa5b438fc97ef902cc411163d93e5e78e5e30e1734c0157.
# a200 PostgreSQL launch is not applied.
# Change: owner a302, fixed archive 25dfc9022780c3deff12f37b7ad7ea4ed1fb6480,
# 6 GiB, 1 CPU, private network and volumes, description-pack is inside the
# archive, overlay is one test file copied after the author smoke.
set -u
set -e
set -o pipefail
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a302/SP07/description-pack-review
EVIDENCE=${REVIEW}/linux/proof-1
ROOT=/tmp/oo-a302-eef831aa-dba4-4af2-a78b-541bc9e0a302/proof-1
FIXED=25dfc9022780c3deff12f37b7ad7ea4ed1fb6480
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a302-proof1
NET=oo-a302-eef831aa-net
CARGO_VOL=oo-a302-eef831aa-cargo
TARGET_VOL=oo-a302-eef831aa-target

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite proof console.log" >&2
  exit 2
fi
rm -rf "$ROOT/archive" "$ROOT/work" "$REVIEW/source/blobs" "$REVIEW/source/description-pack"
mkdir -p "$ROOT/archive" "$ROOT/work" "$REVIEW/source"
git -C "$SRC" archive --format=tar "$FIXED" tools/description-pack \
  | tar -x -C "$REVIEW/source" --strip-components=1
if [ ! -f "$REVIEW/source/description-pack/Cargo.toml" ]; then
  echo "fixed archive did not yield tools/description-pack" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
mkdir -p "$REVIEW/source/blobs"
cp -R "$REVIEW/source/description-pack/." "$REVIEW/source/blobs/"
rm -rf "$REVIEW/source/description-pack"
find "$REVIEW/source/blobs" -name '._*' -delete
find "$REVIEW/source/blobs" -name '.DS_Store' -delete
printf '%s\n' "$FIXED" > "$REVIEW/source/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$REVIEW/source/fixed-tree.txt"
printf '%s\n' '5ce8b1c4186eb80f9b876a306ae94309f10933ae' > "$REVIEW/source/implementation-commit.txt"
git -C "$SRC" rev-parse "${FIXED}:tools/description-pack" > "$REVIEW/source/pack-tree.txt"
git -C "$SRC" rev-parse '5ce8b1c4186eb80f9b876a306ae94309f10933ae:tools/description-pack' > "$REVIEW/source/implementation-pack-tree.txt"
git -C "$SRC" archive --format=tar "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml crates tools/description-review tools/description-pack \
  | tar -x -C "$ROOT/archive"
find "$ROOT/archive" -name '._*' -delete
find "$ROOT/archive" -name '.DS_Store' -delete
if [ ! -f "$ROOT/archive/tools/description-pack/src/lib.rs" ]; then
  echo "archive missing description-pack lib.rs" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ -f "$ROOT/archive/tools/description-pack/tests/a302_sealed.rs" ]; then
  echo "archive already contains the review overlay" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
{
  for rel in crates/oo-builder/src/description_candidate.rs tools/description-pack/src/lib.rs tools/description-review/src/lib.rs; do
    want=$(git -C "$SRC" rev-parse "$FIXED:$rel")
    got=$(git -C "$SRC" hash-object "$ROOT/archive/$rel")
    printf '%s want=%s got=%s\n' "$rel" "$want" "$got"
    if [ "$want" != "$got" ]; then
      echo "blob mismatch $rel" >&2
      printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
      exit 2
    fi
  done
} > "$EVIDENCE/archive-blobs.txt"
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/fixed-tree.txt"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a302 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a302 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a302 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a302 \
  --label oo.kind=cargo-home \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a302 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a302 \
  --label oo.kind=cargo-target \
  "$TARGET_VOL" >/dev/null
own=$(docker ps -q --filter label=oo.owner=a302 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1 postgres=0\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/containers-before.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt" || {
  echo "pinned image is not arm64" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
}
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named proof container already exists" >&2
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
  --mount "type=bind,source=${ROOT}/work,target=/work" \
  --mount "type=bind,source=${REVIEW}/source/blobs,target=/blobs,readonly" \
  --mount "type=bind,source=${REVIEW}/overlay,target=/overlay,readonly" \
  --mount "type=bind,source=${REVIEW}/tools/lock_subset.rs,target=/tools/lock_subset.rs,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a302 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a302 \
  --label oo.task=description-pack-review \
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
