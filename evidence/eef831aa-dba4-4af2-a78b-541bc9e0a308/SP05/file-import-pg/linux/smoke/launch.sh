#!/bin/bash
# Pre-edit Linux arm64 Rust 1.94 smoke of fixed 72dc9e2. No file-import-pg overlay.
# Origin: a226 linux/smoke/launch.sh blob 403244658c0a1fb5520c163016cc309ac10e8cd7,
# which records a211 blob 4e35d55d9bd49eb437dbb99d2f24f402969913e3,
# a200 blob 20b391304358e474c189203eeb525b46fc2ea194,
# and a204 blob 3737c1a8321814ff3b1070ddf8b89d6864c63f6f.
# a220 file-worker preoverlay launch blob ec1e0da16b79372a4c4160683637eec6bcb542a5
# is the focused test shape. This builder is 6g and 1 CPU. Global rust-builder cap is 10.
# Archive is the workspace pins and crates only. The fixed tree has 58103 paths;
# cargo does not read the evidence tree.
set -u
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a308/SP05/file-import-pg
EVIDENCE=${REVIEW}/linux/smoke
ROOT=/tmp/oo-a308-eef831aa-dba4-4af2-a78b-541bc9e0a308
FIXED=72dc9e2db841de4de368216bbbe13d738f850ea9
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a308-smoke
NET=oo-a308-eef831aa-net
CARGO_VOL=oo-a308-eef831aa-cargo
TARGET_VOL=oo-a308-eef831aa-target

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite smoke console.log" >&2
  exit 2
fi
mkdir -p "$EVIDENCE/phases" "$REVIEW/source/blobs" "$ROOT"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/fixed-tree.txt"
if [ ! -f "$ROOT/archive/crates/oo-connectors/tests/file_parser_worker.rs" ]; then
  rm -rf "$ROOT/archive"
  mkdir -p "$ROOT/archive"
  git -C "$SRC" archive --format=tar "$FIXED" -- \
    Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates \
    | tar -x -C "$ROOT/archive"
fi
printf '%s\n' "$FIXED" > "$ROOT/archive/.fixed-base-sha"
if [ -e "$ROOT/archive/tools/file-import-pg" ]; then
  echo "archive already contains the file-import-pg overlay" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
want_files=$(git -C "$SRC" ls-tree -r --name-only "$FIXED" -- Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates | wc -l | awk '{print $1}')
got_files=$(find "$ROOT/archive" -type f ! -name '.fixed-base-sha' | wc -l | awk '{print $1}')
printf 'listed=%s extracted=%s\n' "$want_files" "$got_files" > "$EVIDENCE/archive-count.txt"
if [ "$want_files" != "$got_files" ]; then
  echo "archive file count mismatch" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
blob_ok=0
{
  while IFS='|' read -r path name; do
    [ -n "$path" ] || continue
    want=$(git -C "$SRC" rev-parse "$FIXED:$path")
    got=$(git -C "$SRC" hash-object "$ROOT/archive/$path")
    printf '%s %s want=%s got=%s\n' "$path" "$name" "$want" "$got"
    if [ "$want" != "$got" ]; then
      echo "blob mismatch $path" >&2
      blob_ok=1
    fi
    cp "$ROOT/archive/$path" "$REVIEW/source/blobs/$name"
  done <<'BLOBS'
crates/oo-connectors/src/bin/oo-file-parser-worker.rs|oo-file-parser-worker.rs
crates/oo-connectors/tests/file_parser_worker.rs|file_parser_worker.rs
crates/oo-connectors/src/file_worker_parent.rs|file_worker_parent.rs
crates/oo-connectors/Cargo.toml|oo-connectors-Cargo.toml
Cargo.lock|Cargo.lock
Cargo.toml|Cargo.toml
rust-toolchain.toml|rust-toolchain.toml
BLOBS
} > "$EVIDENCE/archive-blobs.txt"
if [ "$blob_ok" -ne 0 ]; then
  echo "archive blob mismatch" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a308 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a308 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a308 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a308 \
  --label oo.kind=cargo-home \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a308 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a308 \
  --label oo.kind=cargo-target \
  "$TARGET_VOL" >/dev/null
docker volume inspect --format '{{.Name}} {{.Mountpoint}}' "$CARGO_VOL" > "$EVIDENCE/volume-cargo.txt"
docker volume inspect --format '{{.Name}} {{.Mountpoint}}' "$TARGET_VOL" > "$EVIDENCE/volume-target.txt"
own=$(docker ps -q --filter label=oo.owner=a308 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
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
grep -q 'arch=arm64' "$EVIDENCE/image.txt" || { echo "image is not arm64" >&2; printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"; exit 1; }
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
  --mount "type=bind,source=${REVIEW}/tools,target=/tools,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${REVIEW},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a308 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a308 \
  --label oo.task=file-import-pg-smoke \
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
