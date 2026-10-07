#!/bin/bash
# Hash deploy/trusted-demo with the Safe Rust walker. Network none. No product resources.
set -eu
set -o pipefail
REPO=/Users/redux/puppy-one/open-ontology
DEMO=$REPO/deploy/trusted-demo
EV=$REPO/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a312/SP11/trusted-demo-identity/linux/hash-source
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a312-hashsrc
mkdir -p "$EV"
own=$(docker ps -q --filter label=oo.owner=a312 --filter label=oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a312 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10\n' "$own" "$global" "$host_cargo" > "$EV/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: builder capacity own=$own global=$global host_cargo=$host_cargo" >&2
  printf '%s\n' 3 > "$EV/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "hash container already exists" >&2
  printf '%s\n' 2 > "$EV/docker-run.exit"
  exit 2
fi
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network none --cap-drop ALL --memory 6442450944 --cpus 1 image=pinned-rust-1.94 bash -c" > "$EV/invocation.txt"
set +e
docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory 6442450944 \
  --memory-swap 6442450944 \
  --cpus 1 \
  --pids-limit 4096 \
  --network none \
  -e HOME=/tmp \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${DEMO},target=/deploy,readonly" \
  --mount "type=bind,source=${EV},target=/work" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=256m \
  --label oo.project=open-ontology \
  --label oo.owner=a312 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a312 \
  --label oo.fixed=25dfc9022780c3deff12f37b7ad7ea4ed1fb6480 \
  --label oo.kind=rust-builder \
  --label oo.task=trusted-demo-hash-source \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  "$IMAGE" \
  bash -c 'set -eu; export PATH="/usr/local/cargo/bin:${PATH}"; rustc --version | tee /work/rustc-version.txt; rustup run 1.94.0 rustc --edition 2021 /deploy/tools/sha256_manifest.rs -o /usr/local/cargo/bin/sha256-manifest; /usr/local/cargo/bin/sha256-manifest /deploy /work/source-sha256.txt' \
  > "$EV/console.log" 2>&1
run_exit=$?
set -e
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME" 2>/dev/null || echo missing)"
} > "$EV/hostconfig.txt"
docker rm "$NAME" > "$EV/container-removed.txt" || true
printf '%s\n' "$run_exit" > "$EV/docker-run.exit"
exit "$run_exit"
