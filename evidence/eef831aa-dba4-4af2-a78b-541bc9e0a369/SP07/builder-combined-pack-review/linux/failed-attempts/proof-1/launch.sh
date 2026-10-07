#!/bin/bash
# Independent review launch. Recipe origin, not a byte copy:
# evidence/eef831aa-dba4-4af2-a78b-541bc9e0a362/SP07/builder-combined-pack/linux/proof-4/launch.sh
# sha256 d69865d6b3a1a4cea942d83656eabb63e25dab1c29d12db04a9f5b2419f16683
# The source tar is git archive of the full 40-character repair commit.
# Stored a328 receipts and the a357 oracle are mounted read-only.
set -u
set -e
set -o pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a369/SP07/builder-combined-pack-review/linux/proof
RECEIPTS=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a328/SP07/pg-builder
A357=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a357/SP07/pg-builder-review/overlay
ORACLE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a369/SP07/builder-combined-pack-review/overlay
ROOT=/tmp/oo-a369-eef831aa-dba4-4af2-a78b-541bc9e0a369/proof
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a369-proof
NET=oo-a369-eef831aa-net
CARGO_VOL=oo-a369-eef831aa-cargo
TARGET_VOL=oo-a369-eef831aa-target
EXPECTED=be9acbc23a9e4c477b9cf6bd4b19ad22fad3abba

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite proof console.log" >&2
  exit 2
fi
mkdir -p "$EVIDENCE/phases"
FULL=$(git -C "$SRC" rev-parse --verify "${EXPECTED}^{commit}")
if [ "$FULL" != "$EXPECTED" ]; then
  echo "full sha did not resolve to the repair commit" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ "$(printf '%s' "$FULL" | wc -c | tr -d ' ')" -ne 40 ]; then
  echo "resolved sha is not 40 characters" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
printf '%s\n' "$FULL" > "$EVIDENCE/archive-commit.txt"
rm -rf "$ROOT/archive"
mkdir -p "$ROOT/archive"
git -C "$SRC" archive --format=tar "$FULL" \
  Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates \
  tools/description-review tools/description-pack tools/pg-builder \
  | tar -x -C "$ROOT/archive"
find "$ROOT/archive" -name '._*' -delete
find "$ROOT/archive" -name '.DS_Store' -delete
if [ ! -f "$ROOT/archive/tools/pg-builder/src/bridge.rs" ]; then
  echo "archive missing pg-builder" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ -f "$ROOT/archive/tools/pg-builder/tests/a369_review.rs" ]; then
  echo "archive contains the review oracle" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
git -C "$SRC" show "${FULL}:tools/pg-builder/src/bridge.rs" | shasum -a 256 > "$EVIDENCE/archive-bridge.sha256"
shasum -a 256 "$ROOT/archive/tools/pg-builder/src/bridge.rs" > "$EVIDENCE/workdir-bridge.sha256"
git_hash=$(awk '{print $1}' "$EVIDENCE/archive-bridge.sha256")
work_hash=$(awk '{print $1}' "$EVIDENCE/workdir-bridge.sha256")
if [ "$git_hash" != "$work_hash" ]; then
  echo "archived bridge.rs does not match the commit blob" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt" || {
  echo "pinned image is not arm64" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
}
if [ ! -d "$RECEIPTS/live/ready/pass1-calls" ]; then
  echo "stored receipts are missing" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
own=$(docker ps -q --filter label=oo.owner=a369 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1 postgres=0\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/containers-before.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a369 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a369 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a369 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a369 \
  --label oo.kind=cargo-home \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a369 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a369 \
  --label oo.kind=cargo-target \
  "$TARGET_VOL" >/dev/null
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named proof container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 --pids-limit 256 image=pinned-rust-1.94 bash /inside.sh" > "$EVIDENCE/invocation.txt"
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
  --pids-limit 256 \
  --network "$NET" \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target/a369-review \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${RECEIPTS},target=/receipts,readonly" \
  --mount "type=bind,source=${A357},target=/a357,readonly" \
  --mount "type=bind,source=${ORACLE},target=/oracle,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=256m \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a369 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a369 \
  --label oo.task=builder-combined-pack-review \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  {
    echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
    echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
    echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
    echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
    echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME")"
    echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
    echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME")"
    echo "PidsLimit=$(docker inspect -f '{{.HostConfig.PidsLimit}}' "$NAME")"
    echo "ReadonlyRootfs=$(docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' "$NAME")"
    echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
  } > "$EVIDENCE/hostconfig.txt"
  docker inspect -f '{{json .HostConfig.Tmpfs}}' "$NAME" > "$EVIDENCE/tmpfs.txt"
  docker inspect -f '{{range .Mounts}}{{.Type}} {{.Destination}} rw={{.RW}}{{println}}{{end}}' "$NAME" > "$EVIDENCE/mounts.txt"
  docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
fi
exit "$run_exit"
