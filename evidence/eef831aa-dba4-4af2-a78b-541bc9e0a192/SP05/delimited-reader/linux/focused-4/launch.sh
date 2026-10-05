#!/bin/bash
# Focused overlay. Immutable git archive plus the four owned delimited files.
# Reuses the a192 network and cargo/target volumes. No PostgreSQL.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a192/SP05/delimited-reader/linux/focused-4
ROOT=/tmp/oo-a192-focused-4
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a192-focused-4
NET=oo-a192-eef831aa-net
CARGO_VOL=oo-a192-eef831aa-cargo
TARGET_VOL=oo-a192-eef831aa-target

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite focused-4 console.log" >&2
  exit 2
fi
rm -rf "$ROOT/archive"
mkdir -p "$ROOT/archive"
git -C "$SRC" archive --format=tar HEAD Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates contracts \
  | tar -x -C "$ROOT/archive"
mkdir -p "$ROOT/archive/contracts/connectors"
cp "$SRC/crates/oo-connectors/src/delimited.rs" "$ROOT/archive/crates/oo-connectors/src/delimited.rs"
cp "$SRC/crates/oo-connectors/src/lib.rs" "$ROOT/archive/crates/oo-connectors/src/lib.rs"
cp "$SRC/crates/oo-connectors/tests/delimited.rs" "$ROOT/archive/crates/oo-connectors/tests/delimited.rs"
cp "$SRC/contracts/connectors/delimited-reader-v1.md" "$ROOT/archive/contracts/connectors/delimited-reader-v1.md"
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/git-head.txt"
jsonl_head=$(git -C "$SRC" show HEAD:crates/oo-connectors/src/jsonl.rs | sha256sum | awk '{print $1}')
jsonl_archive=$(sha256sum "$ROOT/archive/crates/oo-connectors/src/jsonl.rs" | awk '{print $1}')
if [ "$jsonl_head" != "$jsonl_archive" ]; then
  echo "jsonl.rs in the archive does not match HEAD" >&2
  exit 2
fi
(
  cd "$ROOT/archive"
  sha256sum \
    Cargo.toml Cargo.lock rust-toolchain.toml \
    crates/oo-connectors/src/lib.rs \
    crates/oo-connectors/src/delimited.rs \
    crates/oo-connectors/src/jsonl.rs \
    crates/oo-connectors/tests/delimited.rs \
    crates/oo-connectors/Cargo.toml \
    contracts/connectors/delimited-reader-v1.md \
    contracts/source/pgsql/empty-catalog.json
) > "$EVIDENCE/expect-sha256.txt"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  exit 1
fi
docker network inspect "$NET" >/dev/null
docker network inspect --format '{{.Id}} {{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
own=$(docker ps -q --filter label=oo.owner=a192 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=6\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/containers-before.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 6 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named focused container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL image=pinned-rust-1.94 bash /inside.sh" > "$EVIDENCE/invocation.txt"
set +e
docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory 8g \
  --memory-swap 8g \
  --cpus 4 \
  --pids-limit 4096 \
  --network "$NET" \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=4 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  -e OO_A192_ORACLE_DIR=/evidence/oracle \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a192 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a192 \
  --label oo.task=delimited-reader-focused-4 \
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
  echo "CapAdd=$(docker inspect -f '{{json .HostConfig.CapAdd}}' "$NAME")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
  echo "PortBindings=$(docker inspect -f '{{json .HostConfig.PortBindings}}' "$NAME")"
  echo "Tmpfs=$(docker inspect -f '{{json .HostConfig.Tmpfs}}' "$NAME")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
} > "$EVIDENCE/hostconfig.txt"
docker inspect -f '{{json .Mounts}}' "$NAME" > "$EVIDENCE/mounts.txt"
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
exit "$run_exit"
