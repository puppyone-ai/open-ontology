#!/bin/bash
# Builder only. Postgres oo-a359-pg is already running.
set -euo pipefail
OWNER=a359
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a359
FIXED=a13b1cc80ed66c07199a41d47b31fd42530f2be9
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE="$REPO/evidence/${RUN_ID}/SP08/release-coordinator"
BUILD_EVIDENCE="$EVIDENCE/linux/build-1"
PG_EVIDENCE="$EVIDENCE/pg"
ROOT=/tmp/oo-a359-${RUN_ID}
SEC="$ROOT/secrets"
NET=oo-a359-eef831aa-net
PG_HOST=oo-a359-pg
CARGO_VOL=oo-a359-eef831aa-cargo
TARGET_VOL=oo-a359-eef831aa-target
BUILD=oo-a359-build
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5

own=$(docker ps -q --filter label=oo.kind=rust-builder --filter label=oo.owner="$OWNER" | wc -l | tr -d ' ')
global=$(docker ps -q --filter label=oo.kind=rust-builder | wc -l | tr -d ' ')
host_tools=$(ps ax -o command= | awk 'BEGIN{n=0} $1 ~ /(^|\/)(cargo|rustc|rustdoc|rustup)$/ {n++} END{print n+0}')
printf 'own=%s global=%s host_tools=%s\n' "$own" "$global" "$host_tools" | tee "$BUILD_EVIDENCE/capacity-build.txt"
if [ "$own" != 0 ] || [ "$global" -ge 10 ] || [ "$host_tools" != 0 ]; then
  echo "builder capacity refused own=$own global=$global host_tools=$host_tools" >&2
  exit 2
fi
printf '%s\n' 0 > "$PG_EVIDENCE/up.exit"
docker inspect oo-a359-pg --format 'id={{.Id}} status={{.State.Status}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} memory={{.HostConfig.Memory}} cpus={{.HostConfig.NanoCpus}}' > "$PG_EVIDENCE/inspect.txt"
docker inspect oo-a359-pg --format '{{json .HostConfig.PortBindings}}' > "$PG_EVIDENCE/ports.json"
docker inspect oo-a359-pg --format '{{json .HostConfig.CapDrop}} {{json .HostConfig.CapAdd}}' > "$PG_EVIDENCE/caps.json"
printf '%s\n' 'CREATE ROLE reader; CREATE ROLE reader_login; CREATE ROLE admin; CREATE DATABASE oo_a359' > "$PG_EVIDENCE/bootstrap.out"

printf '%s\n' "docker run --name $BUILD --platform linux/arm64 --pull never --network $NET --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 bash /inside.sh" > "$BUILD_EVIDENCE/invocation.txt"
set +e
docker run --name "$BUILD" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --memory 6g \
  --memory-swap 6g \
  --cpus 1 \
  --pids-limit 4096 \
  --network "$NET" \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e RUSTUP_HOME=/usr/local/rustup \
  -e PATH=/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
  -e CARGO_TERM_COLOR=never \
  -e OO_A359_PG_HOST="$PG_HOST" \
  -e OO_A359_PG_PORT=5432 \
  -e OO_A359_PG_DATABASE=oo_a359 \
  -e OO_A359_PASSWORD_FILE=/secrets/admin.pass \
  -e OO_A359_READER_PASSWORD_FILE=/secrets/reader.pass \
  -e OO_A359_CA_FILE=/secrets/ca.crt \
  -e OO_A359_OBSERVATION=/evidence/observation.json \
  --mount "type=bind,source=${REPO},target=/repo,readonly" \
  --mount "type=bind,source=${REPO}/tools/release-coordinator,target=/repo/tools/release-coordinator" \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=/tmp/oo-a359-inside.sh,target=/inside.sh,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=256m \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner="$OWNER" \
  --label oo.session="$RUN_ID" \
  --label oo.fixed="$FIXED" \
  --label oo.kind=rust-builder \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  "$RUST_IMAGE" \
  bash /inside.sh > "$BUILD_EVIDENCE/console.log" 2>&1
build_exit=$?
set -e
printf '%s\n' "$build_exit" > "$BUILD_EVIDENCE/docker-run.exit"
docker inspect "$BUILD" --format 'oom={{.State.OOMKilled}} exit={{.State.ExitCode}} privileged={{.HostConfig.Privileged}} memory={{.HostConfig.Memory}} cpus={{.HostConfig.NanoCpus}}' > "$BUILD_EVIDENCE/hostconfig.txt" || true
docker rm "$BUILD" >/dev/null || true
if [ "$build_exit" -ne 0 ]; then
  echo "BUILD_FAILED exit=$build_exit"
  exit "$build_exit"
fi
echo BUILD_OK
