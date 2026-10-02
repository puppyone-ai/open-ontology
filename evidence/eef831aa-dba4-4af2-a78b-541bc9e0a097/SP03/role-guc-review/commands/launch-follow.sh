#!/bin/bash
# One follow-up Rust builder. Reuses the archive, network, cargo, and target volumes.
set -eu
RUN=eef831aa-dba4-4af2-a78b-541bc9e0a097
ROOT=/Users/redux/puppy-one/open-ontology
BASE=/tmp/oo-$RUN
ARCH=$BASE/isolated-archive
PROBES=$BASE/probes
SEC=$BASE/secrets
EVID=$ROOT/evidence/$RUN/SP03/role-guc-review
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NET=oo-a097-net-$RUN
RUST_NAME=oo-a097-follow-$RUN
PG_NAME=oo-a097-pg-$RUN
CARGO_VOL=oo-a097-cargo-$RUN
TARGET_VOL=oo-a097-target-$RUN
PG_VOL=oo-a097-pglog-$RUN

mkdir -p "$EVID/followup/phases" "$PROBES"
cp "$EVID/followup/a097_role_guc.rs" "$PROBES/a097_role_guc.rs"
cp "$EVID/followup/inside.sh" "$PROBES/follow-inside.sh"
chmod 755 "$EVID/followup/inside.sh" "$PROBES/follow-inside.sh"

{
  echo "when=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "host_rust"
  ps -ax -o pid=,ppid=,etime=,comm= | awk '$4 ~ /(^|\/)(cargo|rustc|rustdoc|clippy-driver)$/ {print}'
  echo "docker_running"
  docker ps --filter status=running --format '{{.Names}} {{.Image}} {{.Status}}'
} | tee "$EVID/followup/capacity.txt"

rust_running=$(docker ps --filter status=running --format '{{.Image}}' | awk '/0f24958fa693/ {n++} END{print n+0}')
host_running=$(ps -ax -o comm= | awk '$0 ~ /(^|\/)(cargo|rustc|rustdoc|clippy-driver)$/ {n++} END{print n+0}')
echo "rust_running_count=$rust_running host_rust_count=$host_running" | tee -a "$EVID/followup/capacity.txt"
if [ "$((rust_running + host_running))" -ge 2 ]; then
  echo "builder capacity full: rust=$rust_running host=$host_running" >&2
  exit 2
fi
if ! docker exec "$PG_NAME" pg_isready -U postgres -d source_lab >/dev/null 2>&1; then
  echo "postgres fixture is not ready" >&2
  exit 1
fi

docker rm -f "$RUST_NAME" >/dev/null 2>&1 || true
docker run -d --name "$RUST_NAME" \
  --platform linux/arm64 \
  --pull never \
  --cap-drop ALL \
  --security-opt no-new-privileges:true \
  --init \
  --memory 8g --memory-swap 8g --cpus 4 --pids-limit 4096 \
  --network "$NET" \
  --label project=open-ontology --label owner=a097 --label scope=role-guc-review --label run="$RUN" \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_TERM_COLOR=never \
  -e OO_PG_FIXTURE=/secrets/pointer.json \
  -v "$ARCH":/src:ro \
  -v "$SEC":/secrets:ro \
  -v "$PG_VOL":/pglog:ro \
  -v "$EVID/followup/phases":/phases \
  -v "$PROBES":/opt/a097:ro \
  -v "$EVID/followup/inside.sh":/inside.sh:ro \
  -v "$CARGO_VOL":/cargo \
  -v "$TARGET_VOL":/target \
  -w /src \
  "$RUST_IMAGE" \
  bash /inside.sh >/dev/null

echo "launch_ok name=$RUST_NAME"
