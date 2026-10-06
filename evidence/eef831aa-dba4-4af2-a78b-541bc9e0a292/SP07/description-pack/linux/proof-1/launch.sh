#!/bin/bash
# Smoke the sealed description-review archive, then copy description-pack.
# Does not overwrite an existing console.log. Does not prune. Starts no PostgreSQL.
# Does not write the product tree, the root lock, or tools/description-review.
# Recipe origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a288/SP07/description-review-review/linux/proof-4/launch.sh
# content sha256 0cf5c779d5b593619aa5b438fc97ef902cc411163d93e5e78e5e30e1734c0157
# which follows a204 overlay-3 at b2b3adabd05de94723d8ed13b33be0144a0de1f9
# blob 03c8e15c60a376ef0a757c062ebd110063a0ffe6.
# a200 PostgreSQL launch is not applied.
# Change: owner a292, fixed archive c8588faab992af1193ff58dbc489a0b117796aa0,
# 6 GiB, 1 CPU, private network and volumes, no pull, no prune, no PostgreSQL.
set -u
set -o pipefail
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a292/SP07/description-pack
A288=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a288/SP07/description-review-review
EVIDENCE=${REVIEW}/linux/proof-1
ROOT=/tmp/oo-a292-eef831aa-dba4-4af2-a78b-541bc9e0a292/proof-1
FIXED=c8588faab992af1193ff58dbc489a0b117796aa0
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a292-proof1
NET=oo-a292-eef831aa-net
CARGO_VOL=oo-a292-eef831aa-cargo
TARGET_VOL=oo-a292-eef831aa-target

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite proof console.log" >&2
  exit 2
fi
rm -rf "$ROOT/archive" "$ROOT/work"
mkdir -p "$ROOT/archive" "$ROOT/work" "$EVIDENCE/blobs"
find "$EVIDENCE/blobs" -mindepth 1 -maxdepth 1 -exec rm -rf {} +
cp -R "$SRC/tools/description-pack/." "$EVIDENCE/blobs/"
find "$EVIDENCE/blobs" -name '._*' -delete
find "$EVIDENCE/blobs" -name '.DS_Store' -delete
if [ ! -f "$EVIDENCE/blobs/Cargo.toml" ] || [ ! -f "$EVIDENCE/blobs/Cargo.lock" ]; then
  echo "overlay snapshot is missing the pack manifest or lock" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
git -C "$SRC" archive --format=tar "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml crates tools/description-review \
  | tar -x -C "$ROOT/archive"
find "$ROOT/archive" -name '._*' -delete
find "$ROOT/archive" -name '.DS_Store' -delete
if [ -d "$ROOT/archive/tools/description-pack" ]; then
  echo "archive already contains description-pack" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ ! -f "$ROOT/archive/tools/description-review/src/lib.rs" ]; then
  echo "archive missing description-review lib.rs" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ ! -f "$ROOT/archive/tools/description-review/Cargo.lock" ]; then
  echo "archive missing description-review Cargo.lock" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
{
  for rel in crates/oo-builder/src/description_candidate.rs tools/description-review/src/lib.rs; do
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
  --label oo.owner=a292 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a292 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a292 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a292 \
  --label oo.kind=cargo-home \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a292 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a292 \
  --label oo.kind=cargo-target \
  "$TARGET_VOL" >/dev/null
own=$(docker ps -q --filter label=oo.owner=a292 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
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
  --mount "type=bind,source=${EVIDENCE}/blobs,target=/blobs,readonly" \
  --mount "type=bind,source=${SRC}/tools/description-pack,target=/overlay,readonly" \
  --mount "type=bind,source=${A288}/tools/lock_subset.rs,target=/tools/lock_subset_review.rs,readonly" \
  --mount "type=bind,source=${REVIEW}/tools/lock_subset.rs,target=/tools/lock_subset.rs,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a292 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a292 \
  --label oo.task=description-pack-proof \
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
