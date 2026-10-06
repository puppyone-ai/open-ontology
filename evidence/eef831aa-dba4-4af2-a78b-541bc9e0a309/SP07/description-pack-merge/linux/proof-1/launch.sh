#!/bin/bash
# Occupied-review extension proof. One builder, no PostgreSQL.
# Phase 1 smokes the description-pack already in the fixed archive.
# Phase 2 overlays the working tree and runs the expanded pack tests.
# Does not overwrite an existing console.log. Does not prune. Does not pull.
# Does not write the product tree, the root lock, or tools/description-review.
# Recipe origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a292/SP07/description-pack/linux/proof-3/launch.sh
# which follows a288 proof-4 content sha256 0cf5c779d5b593619aa5b438fc97ef902cc411163d93e5e78e5e30e1734c0157
# and a204 overlay-3 at b2b3adabd05de94723d8ed13b33be0144a0de1f9
# blob 03c8e15c60a376ef0a757c062ebd110063a0ffe6
# content sha256 78c8ef8c179c59317850c1b6a9ae53d3f883b8a1b24c2567fa0d3d26f1997eee.
# a200 PostgreSQL launch is not applied.
# Change: owner a309, fixed archive cab5e8e219d1924070df7336f5ab5d4ce7cf40dc,
# archive includes tools/description-pack, 6 GiB, 1 CPU, private network and volumes.
set -u
set -o pipefail
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a309/SP07/description-pack-merge
A302=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a302/SP07/description-pack-review
EVIDENCE=${REVIEW}/linux/proof-1
ROOT=/tmp/oo-a309-eef831aa-dba4-4af2-a78b-541bc9e0a309/proof-1
FIXED=cab5e8e219d1924070df7336f5ab5d4ce7cf40dc
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a309-proof1
NET=oo-a309-eef831aa-net
CARGO_VOL=oo-a309-eef831aa-cargo
TARGET_VOL=oo-a309-eef831aa-target

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
  Cargo.toml Cargo.lock rust-toolchain.toml crates tools/description-review tools/description-pack \
  | tar -x -C "$ROOT/archive"
find "$ROOT/archive" -name '._*' -delete
find "$ROOT/archive" -name '.DS_Store' -delete
if [ ! -f "$ROOT/archive/tools/description-pack/src/apply.rs" ]; then
  echo "archive is missing description-pack" >&2
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
  for rel in \
    crates/oo-builder/src/draft.rs \
    crates/oo-builder/src/captured_pg.rs \
    tools/description-review/src/lib.rs \
    tools/description-pack/src/apply.rs \
    tools/description-pack/tests/pack.rs \
    tools/description-pack/Cargo.lock \
    Cargo.lock
  do
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
git -C "$SRC" status --short -- tools/description-pack > "$EVIDENCE/worktree-status.txt"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a309 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a309 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a309 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a309 \
  --label oo.kind=cargo-home \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a309 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a309 \
  --label oo.kind=cargo-target \
  "$TARGET_VOL" >/dev/null
own=$(docker ps -q --filter label=oo.owner=a309 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
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
if [ ! -f "$A302/overlay/a302_sealed.rs" ]; then
  echo "a302 boundary oracle is missing" >&2
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
  --mount "type=bind,source=${REVIEW}/tools/lock_subset.rs,target=/tools/lock_subset.rs,readonly" \
  --mount "type=bind,source=${A302}/overlay/a302_sealed.rs,target=/boundary/a302_sealed.rs,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a309 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a309 \
  --label oo.task=description-pack-merge-proof \
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
