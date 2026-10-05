#!/bin/bash
# Fixed archive c2db9c430b822a824a1062b6c3c7345924df7dd8 plus this task's overlay.
# Adapted from a204 overlay-3 launch 78c8ef8c179c59317850c1b6a9ae53d3f883b8a1b24c2567fa0d3d26f1997eee
# and this task's preoverlay launch. Builder is 6 GiB and 1 CPU. Does not prune.
# Does not overwrite an existing console.log.
set -u
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a219/SP04/target-statistics
EVIDENCE=${REVIEW}/linux/overlay
ROOT=/tmp/oo-a219-eef831aa-dba4-4af2-a78b-541bc9e0a219/overlay
PUB=/tmp/oo-a219-eef831aa-dba4-4af2-a78b-541bc9e0a219/builder-secrets
FIXED=c2db9c430b822a824a1062b6c3c7345924df7dd8
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a219-overlay
NET=oo-a219-eef831aa-net
CARGO_VOL=oo-a219-eef831aa-cargo
TARGET_VOL=oo-a219-eef831aa-target

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh" "$EVIDENCE/extract-libclang.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite overlay console.log" >&2
  exit 2
fi
if [ ! -f "$PUB/ca.crt" ] || [ ! -f "$PUB/admin.pass" ] || [ ! -f "$PUB/login.pass" ]; then
  echo "builder secrets are missing" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if find "$PUB" -type f -name '*.key' -o -name 'ca.key' -o -name 'server.key' | grep -q .; then
  echo "refusing to mount a private key into the builder" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
rm -rf "$ROOT/archive"
mkdir -p "$ROOT/archive" "$EVIDENCE/live"
git -C "$SRC" archive --format=tar "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates contracts \
  | tar -x -C "$ROOT/archive"
if [ -f "$ROOT/archive/crates/oo-pipeline/src/target_statistics.rs" ]; then
  echo "fixed archive already contains target_statistics.rs" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
cp "$SRC/crates/oo-pipeline/src/target_statistics.rs" \
  "$ROOT/archive/crates/oo-pipeline/src/target_statistics.rs"
cp "$SRC/crates/oo-pipeline/src/lib.rs" \
  "$ROOT/archive/crates/oo-pipeline/src/lib.rs"
cp "$SRC/crates/oo-pipeline/tests/target_statistics_pg_live.rs" \
  "$ROOT/archive/crates/oo-pipeline/tests/target_statistics_pg_live.rs"
cp "$SRC/contracts/pipeline/target-statistics-v1.md" \
  "$ROOT/archive/contracts/pipeline/target-statistics-v1.md"
cp "$REVIEW/overlay/catalog_proof.rs" \
  "$ROOT/archive/crates/oo-gateway/tests/target_statistics_catalog_proof.rs"
{
  echo "target_statistics.rs"
  echo "lib.rs"
  echo "target_statistics_pg_live.rs"
  echo "target-statistics-v1.md"
  echo "oo-gateway/tests/target_statistics_catalog_proof.rs"
} > "$EVIDENCE/archive-overlay.txt"
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/fixed-tree.txt"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker network inspect "$NET" >/dev/null 2>&1 || {
  echo "task network is missing" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
}
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
docker volume inspect "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null
own=$(docker ps -q --filter label=oo.owner=a219 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
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
  echo "named overlay container already exists" >&2
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
  -e OO_TARGET_STATS_HOST=oo-a219-pg \
  -e OO_TARGET_STATS_PORT=5432 \
  -e OO_TARGET_STATS_ADMIN_USER=oo_a219_admin \
  -e OO_TARGET_STATS_ADMIN_PASSWORD_FILE=/secrets/admin.pass \
  -e OO_TARGET_STATS_READER_PASSWORD_FILE=/secrets/reader.pass \
  -e OO_TARGET_STATS_RUNTIME_PASSWORD_FILE=/secrets/runtime.pass \
  -e OO_TARGET_STATS_READONLY_PASSWORD_FILE=/secrets/readonly.pass \
  -e OO_TARGET_STATS_LOGIN_PASSWORD_FILE=/secrets/login.pass \
  -e OO_TARGET_STATS_CA_FILE=/secrets/ca.crt \
  -e OO_TARGET_STATS_EVIDENCE=/evidence/live \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${REVIEW}/tools,target=/tools,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE}/extract-libclang.sh,target=/extract-libclang.sh,readonly" \
  --mount "type=bind,source=${PUB},target=/secrets,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a219 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a219 \
  --label oo.task=target-statistics-overlay \
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
docker inspect -f '{{range .Mounts}}{{.Type}} {{.Destination}} rw={{.RW}}{{println}}{{end}}' "$NAME" > "$EVIDENCE/mounts.txt"
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
exit "$run_exit"
