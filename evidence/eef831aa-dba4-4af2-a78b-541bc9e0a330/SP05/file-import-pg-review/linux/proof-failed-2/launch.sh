#!/bin/bash
# One 6GiB arm64 Rust builder for the file-import-pg review. Does not prune.
# Origin: a204 preoverlay launch.sh 08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841
# at b2b3adabd05de94723d8ed13b33be0144a0de1f9.
# Changes: a330 names, 6GiB and 1 CPU, private network to the review Postgres,
# fixed commit 15558917 archive including tools/file-import-pg, client secrets
# only, no product write-back.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a330/SP05/file-import-pg-review
EVIDENCE=${REVIEW}/linux/proof
ROOT=/tmp/oo-a330-eef831aa-dba4-4af2-a78b-541bc9e0a330
FIXED=15558917e2c5be12b74dd1d64fb542b7d05a03af
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a330-proof
NET=oo-a330-eef831aa-net
CARGO_VOL=oo-a330-eef831aa-cargo
TARGET_VOL=oo-a330-eef831aa-target
CLIENT=${ROOT}/client-secrets

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite proof console.log" >&2
  exit 2
fi
mkdir -p "$EVIDENCE/phases"
if [ ! -f "$CLIENT/ca.crt" ] || [ ! -f "$CLIENT/admin.pass" ] || [ ! -f "$CLIENT/runtime.pass" ] || [ ! -f "$CLIENT/agent.pass" ]; then
  echo "private postgres client secrets are absent" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ -e "$CLIENT/ca.key" ] || [ -e "$CLIENT/server.key" ]; then
  echo "client secret directory contains a private key" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
mkdir -p "$ROOT/archive"
if [ ! -f "$ROOT/archive/.fixed-base-sha" ] || [ "$(cat "$ROOT/archive/.fixed-base-sha")" != "$FIXED" ]; then
  rm -rf "$ROOT/archive"
  mkdir -p "$ROOT/archive"
  git -C "$SRC" archive "$FIXED" Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates tools/file-import-pg | tar -x -C "$ROOT/archive"
  printf '%s\n' "$FIXED" > "$ROOT/archive/.fixed-base-sha"
fi
if [ ! -f "$ROOT/archive/crates/oo-connectors/src/bin/oo-file-parser-worker.rs" ] || [ ! -f "$ROOT/archive/tools/file-import-pg/Cargo.lock" ]; then
  echo "fixed archive is incomplete" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/fixed-commit.txt"
git -C "$SRC" merge-base --is-ancestor "$FIXED" HEAD
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/fixed-tree.txt"
want_files=$(git -C "$SRC" ls-tree -r --name-only "$FIXED" -- Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates tools/file-import-pg | wc -l | awk '{print $1}')
got_files=$(find "$ROOT/archive" -type f ! -name '.fixed-base-sha' | wc -l | awk '{print $1}')
printf 'listed=%s extracted=%s\n' "$want_files" "$got_files" > "$EVIDENCE/archive-count.txt"
if [ "$want_files" != "$got_files" ]; then
  echo "archive file count mismatch" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
if ! docker volume inspect "$CARGO_VOL" >/dev/null 2>&1; then
  docker volume create --label oo.project=open-ontology --label oo.owner=a330 --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a330 --label oo.kind=cargo "$CARGO_VOL" > "$EVIDENCE/volume-cargo.txt"
fi
if ! docker volume inspect "$TARGET_VOL" >/dev/null 2>&1; then
  docker volume create --label oo.project=open-ontology --label oo.owner=a330 --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a330 --label oo.kind=target "$TARGET_VOL" > "$EVIDENCE/volume-target.txt"
fi
docker network inspect "$NET" >/dev/null
docker network inspect --format 'id={{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
own=$(docker ps -q --filter label=oo.owner=a330 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
own_pg=$(docker ps -q --filter label=oo.owner=a330 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1 postgres=%s\n' "$own" "$global_builders" "$host_cargo" "$own_pg" > "$EVIDENCE/capacity.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/containers-before.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if [ "$own_pg" -ne 1 ]; then
  echo "private postgres is not running" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
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
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=1 \
  --mount "type=bind,source=${ROOT}/archive,target=/fixed,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE}/lock_subset.rs,target=/lock_subset.rs,readonly" \
  --mount "type=bind,source=${REVIEW}/tools,target=/tools,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${CLIENT},target=/secrets,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=256m \
  --label oo.project=open-ontology \
  --label oo.owner=a330 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a330 \
  --label oo.task=file-import-pg-review \
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
