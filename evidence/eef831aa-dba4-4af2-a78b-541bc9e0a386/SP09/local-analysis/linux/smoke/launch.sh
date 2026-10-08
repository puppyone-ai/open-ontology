#!/bin/bash
# Pre-edit toolchain smoke. No product tree is mounted.
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a272/SP11/session-transition-review/linux/manifest/launch.sh
# commit 7b5f1599bafb385a8c31a2b6fe38d8f71830fd1e
# sha256 df95de366350a6a90b829ea5ffb6a43d0640d49c357f77f49347ebc9e4fd8d1f
# Changes: owner a386, rustc --version plus one edition-2024 probe, network none.
set -u
SRC=/Users/redux/puppy-one/open-ontology
LIVE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a386/SP09/local-analysis/linux/smoke
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a386-smoke
chmod 755 "$LIVE/launch.sh"
if [ -f "$LIVE/console.log" ]; then
  echo "refusing to overwrite smoke console.log" >&2
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
  echo "named smoke container already exists" >&2
  printf '%s\n' 2 > "$LIVE/docker-run.exit"
  exit 2
fi
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network none --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 rustc --version and edition 2024 probe" > "$LIVE/invocation.txt"
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
  --network none \
  -e HOME=/tmp \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  --mount "type=bind,source=${LIVE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,size=64m \
  --label oo.project=open-ontology \
  --label oo.owner=a386 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a386 \
  --label oo.task=local-analysis-pre-edit-smoke \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash -c 'set -o pipefail; export PATH=/usr/local/cargo/bin:/usr/local/bin:$PATH; rustc --version | tee /evidence/rustc-version.txt; cargo --version | tee /evidence/cargo-version.txt; rustc --edition 2024 -D warnings /evidence/probe.rs -o /evidence/probe-bin; probe_rc=$?; if [ "$probe_rc" -ne 0 ]; then exit "$probe_rc"; fi; /evidence/probe-bin > /evidence/probe.txt; run_rc=$?; rm -f /evidence/probe-bin; exit "$run_rc"'
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
docker logs "$NAME" > "$LIVE/console.log" 2>&1
docker rm "$NAME" > "$LIVE/container-removed.txt"
printf '%s\n' "$run_exit" > "$LIVE/smoke.exit"
exit "$run_exit"
