#!/bin/bash
# Linux arm64 Rust 1.94 proof launch for description review.
# Origin launch: a204 commit b2b3adabd05de94723d8ed13b33be0144a0de1f9
#   evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/overlay-3/launch.sh
#   blob 03c8e15c60a376ef0a757c062ebd110063a0ffe6
# Immediate shape: a277 preoverlay launch, which followed a263.
# Changes: proof container, review overlay mount, 6 GiB, 1 CPU, 0 PostgreSQL.
# a200 commit a587031c719a484830d78e493605f48e2056cae4 is the PG recipe and is not applied.
# Does not overwrite an existing console.log. Does not prune. Does not pull.
# Retry after linux/proof-2 test compile exit 101. proof and proof-2 stay unchanged.
set -u
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a277/SP07/description-review
EVIDENCE=${REVIEW}/linux/proof-3
ROOT=/tmp/oo-a277-eef831aa-dba4-4af2-a78b-541bc9e0a277
FIXED=8a6395aef5b1d3b16d1b3d72833d2df2ebaa536e
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a277-proof-3
NET=oo-a277-eef831aa-net
CARGO_VOL=oo-a277-eef831aa-cargo
TARGET_VOL=oo-a277-eef831aa-target

die() {
  printf '%s\n' "$1" > "$EVIDENCE/docker-run.exit"
  exit "$1"
}

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite proof console.log" >&2
  die 2
fi
mkdir -p "$EVIDENCE/phases" "$ROOT"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  die 1
fi
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/fixed-tree.txt"
if [ ! -f "$ROOT/archive/crates/oo-builder/src/description_candidate.rs" ]; then
  rm -rf "$ROOT/archive"
  mkdir -p "$ROOT/archive"
  git -C "$SRC" archive --format=tar "$FIXED" \
    Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates contracts \
    | tar -x -C "$ROOT/archive"
fi
printf '%s\n' "$FIXED" > "$ROOT/archive/.fixed-base-sha"
if [ -e "$ROOT/archive/tools/description-review" ]; then
  echo "archive already contains description review" >&2
  die 2
fi
want=$(git -C "$SRC" rev-parse "$FIXED:crates/oo-builder/src/description_candidate.rs")
got=$(git -C "$SRC" hash-object "$ROOT/archive/crates/oo-builder/src/description_candidate.rs")
printf 'description_candidate.rs want=%s got=%s\n' "$want" "$got" > "$EVIDENCE/archive-blobs.txt"
if [ "$want" != "$got" ]; then
  echo "archive blob mismatch" >&2
  die 2
fi
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a277 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a277 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a277 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a277 \
  --label oo.kind=cargo-home \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a277 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a277 \
  --label oo.kind=cargo-target \
  "$TARGET_VOL" >/dev/null
docker volume inspect --format '{{.Name}} {{.Mountpoint}}' "$CARGO_VOL" > "$EVIDENCE/volume-cargo.txt"
docker volume inspect --format '{{.Name}} {{.Mountpoint}}' "$TARGET_VOL" > "$EVIDENCE/volume-target.txt"
own=$(docker ps -q --filter label=oo.owner=a277 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  die 3
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt" || { echo "image is not arm64" >&2; die 1; }
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named proof container already exists" >&2
  die 2
fi
docker ps -a --format '{{.Names}}' > "$EVIDENCE/containers-before.txt"
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
  --mount "type=bind,source=${REVIEW}/tools,target=/tools,readonly" \
  --mount "type=bind,source=${SRC}/tools/description-review,target=/review,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${REVIEW},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a277 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a277 \
  --label oo.task=description-review-proof \
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
