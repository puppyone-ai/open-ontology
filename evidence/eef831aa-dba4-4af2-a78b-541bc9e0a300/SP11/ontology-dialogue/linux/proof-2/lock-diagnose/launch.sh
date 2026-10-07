#!/bin/bash
# One short builder. Reuses the private cargo volume. Does not start PostgreSQL.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a300/SP11/ontology-dialogue
DIAG=${EVIDENCE}/linux/proof-2/lock-diagnose
ROOT=/tmp/oo-a300-eef831aa-dba4-4af2-a78b-541bc9e0a300
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a300-lockdiag
CARGO_VOL=oo-a300-eef831aa-cargo
chmod 755 "$DIAG/inside.sh" "$DIAG/launch.sh"
if [ -f "$DIAG/console.log" ]; then
  echo "refusing to overwrite lockdiag console" >&2
  exit 2
fi
if docker ps -q --filter status=running --filter label=oo.owner=a300 --filter label=oo.kind=rust-builder | grep -q .; then
  echo "this task already has a rust builder" >&2
  exit 3
fi
ps -axo comm= | awk '$1 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/' > "$DIAG/host-cargo-procs.txt" || true
host_n=$(grep -c . "$DIAG/host-cargo-procs.txt" || true)
{
  docker ps --filter status=running --filter label=oo.kind=rust-builder --format '{{.Names}}'
  docker ps --filter status=running --filter ancestor=sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5 --format '{{.Names}}'
} | awk 'NF && !seen[$0]++' > "$DIAG/builders-before.txt" || true
builder_n=$(grep -c . "$DIAG/builders-before.txt" || true)
global_n=$((host_n + builder_n))
printf 'host_cargo=%s rust_builders=%s global=%s cap=10\n' "$host_n" "$builder_n" "$global_n" > "$DIAG/capacity.txt"
if [ "$global_n" -ge 10 ]; then
  echo "deferred: capacity" >&2
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "lockdiag container already exists" >&2
  exit 2
fi
docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --network none \
  --cap-drop ALL \
  --security-opt no-new-privileges:true \
  --memory 6g \
  --memory-swap 6g \
  --cpus 1 \
  --pids-limit 256 \
  --label oo.project=open-ontology \
  --label oo.owner=a300 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a300 \
  --label oo.kind=rust-builder \
  --label oo.task=ontology-dialogue-lockdiag \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  --mount "type=bind,source=${ROOT}/proof-src,target=/src,readonly" \
  --mount "type=bind,source=${DIAG},target=/out" \
  --mount "type=bind,source=${DIAG}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  "$IMAGE" \
  bash /inside.sh > "$DIAG/console.log" 2>&1
status=$?
printf '%s\n' "$status" > "$DIAG/docker-run.exit"
docker inspect "$NAME" --format 'exit={{.State.ExitCode}} oom={{.State.OOMKilled}} memory={{.HostConfig.Memory}}' > "$DIAG/inspect.txt" || true
docker rm "$NAME" > "$DIAG/container-removed.txt" 2>&1 || true
exit "$status"
