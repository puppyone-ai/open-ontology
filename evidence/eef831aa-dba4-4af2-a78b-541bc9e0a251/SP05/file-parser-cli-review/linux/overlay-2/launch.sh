#!/bin/bash
# Copy the sealed 0a30fcda archive, add only the review harness, and run it.
# Adapted from a204 overlay-3 launch 78c8ef8c179c59317850c1b6a9ae53d3f883b8a1b24c2567fa0d3d26f1997eee.
# One builder. 6 GiB. 1 CPU. Does not prune. Does not overwrite console.log.
set -u
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a251/SP05/file-parser-cli-review
EVIDENCE=${REVIEW}/linux/overlay-2
ROOT=/tmp/oo-a251-eef831aa-dba4-4af2-a78b-541bc9e0a251/overlay-2
FIXED=0a30fcda7fb586bd8d6955e03ae306888374b915
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a251-overlay2
NET=oo-a251-eef831aa-net
CARGO_VOL=oo-a251-eef831aa-cargo
TARGET_VOL=oo-a251-eef831aa-target

echo $$ > "$EVIDENCE/launch.pid"
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite overlay-2 console.log" >&2
  exit 2
fi
mkdir -p "$EVIDENCE/oracle"
rm -rf "$ROOT/archive"
mkdir -p "$ROOT/archive"
git -C "$SRC" archive --format=tar "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates contracts \
  | tar -x -C "$ROOT/archive"
cp "$REVIEW/overlay/parse_file_a251_review.rs" \
  "$ROOT/archive/crates/oo-connectors/tests/parse_file_a251_review.rs"
cp "$REVIEW/overlay/parse_file_a251_review.rs" "$EVIDENCE/expect-review.rs"
if [ ! -f "$ROOT/archive/crates/oo-connectors/src/bin/parse-file.rs" ]; then
  echo "archive missing parse-file.rs" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if ! cmp -s "$ROOT/archive/crates/oo-connectors/src/bin/parse-file.rs" "$REVIEW/source/blobs/parse-file.rs" \
  || ! cmp -s "$ROOT/archive/crates/oo-connectors/tests/parse_file_cli.rs" "$REVIEW/source/blobs/parse_file_cli.rs" \
  || ! cmp -s "$ROOT/archive/contracts/connectors/parse-file-cli-v1.md" "$REVIEW/source/blobs/parse-file-cli-v1.md"
then
  echo "archive drifted from the saved blobs" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
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
  --label oo.owner=a251 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a251 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a251 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a251 \
  --label oo.kind=cargo-home \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a251 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a251 \
  --label oo.kind=cargo-target \
  "$TARGET_VOL" >/dev/null
own=$(docker ps -q --filter label=oo.owner=a251 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_own=1 cap_global=10 memory=6g cpus=1\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
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
  -e OO_A251_ORACLE_DIR=/evidence/oracle \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${REVIEW}/source/blobs,target=/blobs,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a251 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a251 \
  --label oo.task=file-parser-cli-review-overlay2 \
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
