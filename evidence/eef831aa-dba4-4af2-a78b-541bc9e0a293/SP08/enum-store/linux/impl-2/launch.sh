#!/bin/bash
# Overlay enum-store on fixed commit c8588faab992af1193ff58dbc489a0b117796aa0 and run one Linux arm64 proof.
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a275/SP11/session-store/linux/impl-7/launch.sh
# sha256 a5ded8116a77af27599a2b103fdad8e7a5ae485843045b3365b5da333741a836
# Change: fixed commit, a293 names, 6g, enum-store archive paths, one postgres required.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a293/SP08/enum-store/linux/impl-2
TOOLS=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a293/SP08/enum-store/linux/tools
ROOT=/tmp/oo-a293-eef831aa-dba4-4af2-a78b-541bc9e0a293/impl-2
FIXED=c8588faab992af1193ff58dbc489a0b117796aa0
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a293-impl-2
NET=oo-a293-eef831aa-net
CARGO_VOL=oo-a293-eef831aa-cargo
TARGET_VOL=oo-a293-eef831aa-target
FIXTURE=/tmp/oo-a293-eef831aa-dba4-4af2-a78b-541bc9e0a293/fixture

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite impl-2 console.log" >&2
  exit 2
fi
if [ ! -f "$FIXTURE/ca.crt" ] || [ ! -f "$FIXTURE/admin.pass" ] || [ ! -f "$FIXTURE/reader.pass" ] || [ ! -f "$FIXTURE/fixture-admin.pass" ]; then
  echo "private postgres fixture is missing" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
rm -rf "$ROOT/archive"
mkdir -p "$ROOT/archive"
git -C "$SRC" archive --format=tar "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml \
  crates/oo-core tools/enum-history \
  | tar -x -C "$ROOT/archive"
if [ ! -f "$ROOT/archive/Cargo.lock" ] || [ ! -f "$ROOT/archive/tools/enum-history/Cargo.toml" ]; then
  echo "archive missing the root lock or enum-history" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
mkdir -p "$ROOT/archive/tools"
cp -R "$SRC/tools/enum-store" "$ROOT/archive/tools/enum-store"
rm -rf "$ROOT/archive/tools/enum-store/target"
if [ ! -f "$ROOT/archive/tools/enum-store/Cargo.lock" ]; then
  echo "overlay is missing the impl-1 child lock" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/fixed-tree.txt"
git -C "$SRC" rev-parse "${FIXED}:tools/enum-history" > "$EVIDENCE/enum-history-tree.txt"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a293 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a293 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a293 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a293 \
  --label oo.kind=cargo-home \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a293 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a293 \
  --label oo.kind=cargo-target \
  "$TARGET_VOL" >/dev/null
own=$(docker ps -q --filter label=oo.owner=a293 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
own_pg=$(docker ps -q --filter label=oo.owner=a293 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own=%s global=%s host_cargo=%s own_postgres=%s cap_global=10 cap_own=1\n' "$own" "$global_builders" "$host_cargo" "$own_pg" > "$EVIDENCE/capacity.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/containers-before.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ] || [ "$own_pg" -ne 1 ]; then
  echo "deferred: builder or postgres capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named impl container already exists" >&2
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
  --mount "type=bind,source=${TOOLS},target=/tools,readonly" \
  --mount "type=bind,source=${FIXTURE},target=/run/oo-a293/fixture,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=512m \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a293 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a293 \
  --label oo.task=enum-store-impl \
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
} > "$EVIDENCE/hostconfig.txt"
docker inspect -f '{{json .Mounts}}' "$NAME" > "$EVIDENCE/mounts.txt"
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
exit "$run_exit"
