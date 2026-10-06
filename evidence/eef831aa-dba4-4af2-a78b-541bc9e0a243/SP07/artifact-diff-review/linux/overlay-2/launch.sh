#!/bin/bash
# Overlay proof of fixed 701f80aa. The review oracle is copied onto the archive only.
# Recipe origin: a204 preoverlay launch.sh
#   commit b2b3adabd05de94723d8ed13b33be0144a0de1f9
#   path evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh
#   sha256 08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841
# Changes: UUID a243, fixed 701f80aa, examples included, 6GiB, 1 CPU,
# CARGO_BUILD_JOBS=1, global cap 10, NanoCpus recorded, review oracle copied
# into the ephemeral archive. a200 PG launch is not used. 0 PG. No prune.
set -u
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a243/SP07/artifact-diff-review
EVIDENCE=${REVIEW}/linux/overlay-2
ROOT=/tmp/oo-a243-eef831aa-dba4-4af2-a78b-541bc9e0a243/overlay-2
FIXED=701f80aa6f2089a865d2ff99b5aa5c1bcda1de6a
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a243-overlay-2
NET=oo-a243-eef831aa-net
CARGO_VOL=oo-a243-eef831aa-cargo
TARGET_VOL=oo-a243-eef831aa-target
BIN_REL=crates/oo-builder/src/bin/ontology-artifact-diff.rs
TEST_REL=crates/oo-builder/tests/ontology_artifact_diff.rs
REVIEW_REL=crates/oo-builder/tests/ontology_artifact_diff_a243_review.rs

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite overlay-2 console.log" >&2
  exit 2
fi
if [ ! -f "$REVIEW/linux/preoverlay/docker-run.exit" ]; then
  echo "preoverlay has not finished" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
rm -rf "$ROOT/archive"
mkdir -p "$ROOT/archive"
git -C "$SRC" archive --format=tar "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates contracts examples \
  | tar -x -C "$ROOT/archive"
if [ ! -f "$ROOT/archive/$BIN_REL" ] || [ ! -f "$ROOT/archive/$TEST_REL" ]; then
  echo "fixed archive is missing the artifact-diff sources" >&2
  exit 2
fi
if [ -f "$ROOT/archive/$REVIEW_REL" ]; then
  echo "fixed archive already contains the review oracle" >&2
  exit 2
fi
if ! cmp -s "$ROOT/archive/$BIN_REL" "$REVIEW/source/blobs/ontology-artifact-diff.rs" \
  || ! cmp -s "$ROOT/archive/$TEST_REL" "$REVIEW/source/blobs/ontology_artifact_diff.rs" \
  || ! cmp -s "$ROOT/archive/contracts/builder/artifact-diff-v1.md" "$REVIEW/source/blobs/artifact-diff-v1.md"
then
  echo "fixed archive drifted from the saved blobs" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
cp "$REVIEW/oracle/ontology_artifact_diff_a243_review.rs" "$ROOT/archive/$REVIEW_REL"
(
  cd "$ROOT/archive"
  shasum -a 256 \
    "$BIN_REL" \
    "$TEST_REL" \
    "$REVIEW_REL" \
    contracts/builder/artifact-diff-v1.md \
    > "$EVIDENCE/overlay-sha256.txt"
)
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
  --label oo.owner=a243 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a243 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a243 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a243 \
  --label oo.kind=cargo-home \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a243 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a243 \
  --label oo.kind=cargo-target \
  "$TARGET_VOL" >/dev/null
own=$(docker ps -q --filter label=oo.owner=a243 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
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
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named overlay-2 container already exists" >&2
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
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a243 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a243 \
  --label oo.task=artifact-diff-review-overlay-2 \
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
