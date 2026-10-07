#!/bin/bash
# One post-edit driver test. Not the product network, volumes, or PostgreSQL.
# Removes its own container and network before exit.
set -eu
set -o pipefail
REPO=/Users/redux/puppy-one/open-ontology
DEMO=$REPO/deploy/trusted-demo
EV=$REPO/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a312/SP11/trusted-demo-identity/linux/post-edit-drive-attempt-2
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a312-postedit
NET=oo-a312-postedit-net
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
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EV/image.txt"
if ! grep -q 'arch=arm64' "$EV/image.txt"; then
  echo "rust image is not arm64" >&2
  printf '%s\n' 1 > "$EV/docker-run.exit"
  exit 1
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named builder $NAME already exists" >&2
  printf '%s\n' 2 > "$EV/docker-run.exit"
  exit 2
fi
if docker network inspect "$NET" >/dev/null 2>&1; then
  echo "postedit network already exists" >&2
  printf '%s\n' 2 > "$EV/docker-run.exit"
  exit 2
fi
docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a312 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a312 \
  --label oo.task=trusted-demo-postedit \
  "$NET" > "$EV/network-id.txt"
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6442450944 --cpus 1 image=pinned-rust-1.94 bash -c" > "$EV/invocation.txt"
set +e
docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory 6442450944 \
  --memory-swap 6442450944 \
  --cpus 1 \
  --pids-limit 4096 \
  --network "$NET" \
  -e HOME=/tmp \
  -e CARGO_TERM_COLOR=never \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  --mount "type=bind,source=${DEMO},target=/deploy,readonly" \
  --mount "type=bind,source=${EV},target=/work" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=256m \
  --label oo.project=open-ontology \
  --label oo.owner=a312 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a312 \
  --label oo.fixed=25dfc9022780c3deff12f37b7ad7ea4ed1fb6480 \
  --label oo.kind=rust-builder \
  --label oo.task=trusted-demo-postedit \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  "$IMAGE" \
  bash -c 'set -eu; export PATH="/usr/local/cargo/bin:${PATH}"; rustc --version | tee /work/rustc-version.txt; rustup component add clippy --toolchain 1.94.0; rustup run 1.94.0 rustc --edition 2024 --test -D warnings /deploy/src/drive.rs -o /work/drive-test; /work/drive-test --test-threads=1; rustup run 1.94.0 clippy-driver --edition 2024 -D warnings /deploy/src/drive.rs -o /work/drive-clippy; echo POST_EDIT_DRIVE_OK' \
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
  echo "PidsLimit=$(docker inspect -f '{{.HostConfig.PidsLimit}}' "$NAME" 2>/dev/null || echo missing)"
} > "$EV/hostconfig.txt"
docker inspect -f '{{json .HostConfig.Tmpfs}}' "$NAME" > "$EV/tmpfs.txt" 2>/dev/null || true
docker inspect -f '{{range .Mounts}}{{.Type}} {{.Source}} {{.Destination}} rw={{.RW}}{{println}}{{end}}' "$NAME" > "$EV/mounts.txt" 2>/dev/null || true
docker rm "$NAME" > "$EV/container-removed.txt" || true
docker network rm "$NET" > "$EV/network-removed.txt" || true
printf '%s\n' "$run_exit" > "$EV/docker-run.exit"
exit "$run_exit"
