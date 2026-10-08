#!/bin/bash
# One 6 GiB builder and one PostgreSQL 17.10. No host port, no host network,
# no docker socket, no host home, no shared cargo cache.
set -eu
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a379
REPO=/Users/redux/puppy-one/open-ontology
ROOT=${REPO}/evidence/${RUN_ID}/SP02/control-identity-audience-repair
RAW=${ROOT}/linux/raw
PG=${ROOT}/pg
SEC=/tmp/oo-a379-${RUN_ID}/secrets
NET=oo-a379-eef831aa-net
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
mkdir -p "$RAW"
chmod 755 "$ROOT/linux/run-proof.sh" "$PG/pg-wrap.sh" "$PG/up.sh" "$PG/init-roles.sh"

own_rust=$(docker ps -q --filter label=oo.owner=a379 --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
global_rust=$(docker ps -q --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
own_pg=$(docker ps -q --filter label=oo.owner=a379 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own_rust=%s global_rust=%s host_cargo=%s own_pg=%s\n' "$own_rust" "$global_rust" "$host_cargo" "$own_pg" > "$RAW/capacity-before-proof.txt"
if [ "$own_rust" -ge 1 ] || [ "$global_rust" -ge 10 ] || [ "$host_cargo" -ge 1 ] || [ "$own_pg" -ge 1 ]; then
  echo "deferred: capacity own_rust=$own_rust global_rust=$global_rust host_cargo=$host_cargo own_pg=$own_pg" >&2
  printf '%s\n' 3 > "$RAW/launch-proof.exit"
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

bash "$PG/up.sh"
docker exec -u root oo-a379-pg /fixture/init-roles.sh > "$RAW/init-roles.txt"
grep -q '^ok$' "$RAW/init-roles.txt"

docker volume create --label oo.owner=a379 --label oo.session="${RUN_ID}" --label oo.kind=cargo oo-a379-cargo > "$RAW/volume-cargo.txt"
docker volume create --label oo.owner=a379 --label oo.session="${RUN_ID}" --label oo.kind=work oo-a379-work > "$RAW/volume-work.txt"

docker run -d --name oo-a379-proof \
  --platform linux/arm64 \
  --pull never \
  --init \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --memory 6g \
  --memory-swap 6g \
  --cpus 1 \
  --pids-limit 1024 \
  --network "$NET" \
  --network-alias oo-a379-proof \
  -e PATH=/usr/local/cargo/bin:/usr/bin:/bin \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_HOME=/cargo \
  -e CARGO_TARGET_DIR=/work/target \
  --mount "type=bind,source=${REPO}/tools/control-identity,target=/src,readonly" \
  --mount "type=bind,source=${ROOT}/linux,target=/evidence/linux" \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=volume,source=oo-a379-cargo,target=/cargo" \
  --mount "type=volume,source=oo-a379-work,target=/work" \
  --label oo.project=open-ontology \
  --label oo.owner=a379 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=rust-builder \
  --label oo.task=control-identity-audience-repair \
  --entrypoint /evidence/linux/run-proof.sh \
  "$RUST_IMAGE" > "$RAW/builder.id"

docker inspect oo-a379-proof --format 'Memory={{.HostConfig.Memory}} Cpu={{.HostConfig.NanoCpus}} Priv={{.HostConfig.Privileged}} Net={{.HostConfig.NetworkMode}} CapDrop={{json .HostConfig.CapDrop}} Binds={{json .HostConfig.Binds}}' > "$RAW/builder-hostconfig.txt"
docker wait oo-a379-proof > "$RAW/proof-wait.txt"
docker inspect oo-a379-proof --format 'Exit={{.State.ExitCode}} OOM={{.State.OOMKilled}}' >> "$RAW/builder-hostconfig.txt"
code=$(cat "$RAW/proof-wait.txt")
printf '%s\n' "$code" > "$RAW/launch-proof.exit"
exit "$code"
