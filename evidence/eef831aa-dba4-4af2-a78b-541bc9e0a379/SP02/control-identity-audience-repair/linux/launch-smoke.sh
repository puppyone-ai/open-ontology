#!/bin/bash
# One short arm64 Rust 1.94 smoke. No PostgreSQL. No host network, no socket, no host home.
set -eu
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a379
REPO=/Users/redux/puppy-one/open-ontology
ROOT=${REPO}/evidence/${RUN_ID}/SP02/control-identity-audience-repair
RAW=${ROOT}/linux/raw
NET=oo-a379-eef831aa-net
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
mkdir -p "$RAW"
chmod 755 "$ROOT/linux/smoke-inner.sh"

own_rust=$(docker ps -q --filter label=oo.owner=a379 --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
global_rust=$(docker ps -q --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
own_pg=$(docker ps -q --filter label=oo.owner=a379 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own_rust=%s global_rust=%s host_cargo=%s own_pg=%s\n' "$own_rust" "$global_rust" "$host_cargo" "$own_pg" > "$RAW/capacity-before-smoke.txt"
if [ "$own_rust" -ge 1 ] || [ "$global_rust" -ge 10 ] || [ "$host_cargo" -ge 1 ] || [ "$own_pg" -ge 1 ]; then
  echo "deferred: capacity own_rust=$own_rust global_rust=$global_rust host_cargo=$host_cargo own_pg=$own_pg" >&2
  printf '%s\n' 3 > "$RAW/launch-smoke.exit"
  exit 3
fi

if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create \
    --label oo.project=open-ontology \
    --label oo.owner=a379 \
    --label oo.session="${RUN_ID}" \
    --label oo.kind=network \
    "$NET" > "$RAW/network.id"
fi

docker run -d --name oo-a379-smoke \
  --platform linux/arm64 \
  --pull never \
  --init \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --memory 6g \
  --memory-swap 6g \
  --cpus 1 \
  --pids-limit 256 \
  --network "$NET" \
  -e PATH=/usr/local/cargo/bin:/usr/bin:/bin \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  --mount "type=bind,source=${ROOT}/linux,target=/evidence/linux" \
  --label oo.project=open-ontology \
  --label oo.owner=a379 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=rust-builder \
  --label oo.task=control-identity-audience-repair \
  --entrypoint /evidence/linux/smoke-inner.sh \
  "$RUST_IMAGE" > "$RAW/smoke-builder.id"

docker inspect oo-a379-smoke --format 'Status={{.State.Status}} Memory={{.HostConfig.Memory}} Cpu={{.HostConfig.NanoCpus}} Priv={{.HostConfig.Privileged}} Net={{.HostConfig.NetworkMode}} CapDrop={{json .HostConfig.CapDrop}}' > "$RAW/smoke-hostconfig.txt"
code=0
docker wait oo-a379-smoke > "$RAW/smoke-wait.txt" || code=$?
docker inspect oo-a379-smoke --format 'Exit={{.State.ExitCode}} OOM={{.State.OOMKilled}}' >> "$RAW/smoke-hostconfig.txt"
printf '%s\n' "$code" > "$RAW/launch-smoke.exit"
exit "$code"
