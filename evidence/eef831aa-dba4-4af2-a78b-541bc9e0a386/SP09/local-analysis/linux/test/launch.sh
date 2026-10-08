#!/bin/bash
# Locked linux/arm64 test of tools/local-analysis.
# Smoke shape origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a272/SP11/session-transition-review/linux/manifest/launch.sh
# commit 7b5f1599bafb385a8c31a2b6fe38d8f71830fd1e
# sha256 df95de366350a6a90b829ea5ffb6a43d0640d49c357f77f49347ebc9e4fd8d1f
# pids-limit 4096 and CARGO_BUILD_JOBS=1 origin:
# evidence/eef831aa-dba4-4af2-a78b-541bc9e0a344/SP05/query-result-export-chain-review/linux/chain/launch.sh
# Changes: owner a386, network none, private target and cargo-home volumes, no database, no auth env.
set -u
SRC=/Users/redux/puppy-one/open-ontology
LIVE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a386/SP09/local-analysis/linux/test
PKG=${SRC}/tools/local-analysis
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a386-analysis
TARGET_VOL=oo-a386-eef831aa-target
CARGO_VOL=oo-a386-eef831aa-cargo
chmod 755 "$LIVE/launch.sh" "$LIVE/inside.sh"
if [ -f "$LIVE/console.log" ]; then
  echo "refusing to overwrite test console.log" >&2
  printf '%s\n' 2 > "$LIVE/docker-run.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$LIVE/docker-run.exit"
  exit 1
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$LIVE/image.txt"
ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}' > "$LIVE/host-cargo-count.txt"
docker ps --filter status=running --filter label=oo.kind=rust-builder --format '{{.Names}}' > "$LIVE/builders-before.txt" || true
host_n=$(cat "$LIVE/host-cargo-count.txt")
builder_n=$(grep -c . "$LIVE/builders-before.txt" || true)
own=$(docker ps -q --filter status=running --filter label=oo.owner=a386 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own=%s host_cargo=%s rust_builders=%s cap_global=10 cap_own=1\n' "$own" "$host_n" "$builder_n" > "$LIVE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$builder_n" -ge 10 ] || [ "$host_n" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$LIVE/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named test container already exists" >&2
  printf '%s\n' 2 > "$LIVE/docker-run.exit"
  exit 2
fi
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create "$TARGET_VOL" >/dev/null
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create "$CARGO_VOL" >/dev/null
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network none --cap-drop ALL --memory 6g --cpus 1 cargo test --locked --offline and cargo clippy --locked --offline" > "$LIVE/invocation.txt"
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
  --network none \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${PKG},target=/src/local-analysis" \
  --mount "type=bind,source=${LIVE},target=/evidence" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --tmpfs /tmp:rw,nosuid,nodev,size=512m \
  --label oo.project=open-ontology \
  --label oo.owner=a386 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a386 \
  --label oo.task=local-analysis-test \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /evidence/inside.sh > "$LIVE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$LIVE/docker-run.exit"
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
} > "$LIVE/hostconfig.txt"
docker rm "$NAME" >/dev/null
exit "$run_exit"
