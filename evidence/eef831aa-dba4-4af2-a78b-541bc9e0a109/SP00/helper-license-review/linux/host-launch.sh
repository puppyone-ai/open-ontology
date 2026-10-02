#!/bin/bash
# One a109 Rust builder. Slot B. Does not touch a107 resources.
set -eu
REPO="/Users/redux/puppy-one/open-ontology"
REVIEW="$REPO/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a109/SP00/helper-license-review"
COMMIT=abd074af5d59d413e12b8f615c50343e1655047f
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
UUID=$(uuidgen | tr '[:upper:]' '[:lower:]')
BASE="/tmp/oo-a109-${UUID}"
NAME="eef831aa-dba4-4af2-a78b-541bc9e0a109-rust"
NET="oo-a109-${UUID}-net"
VOLC="oo-a109-${UUID}-cargo"
VOLT="oo-a109-${UUID}-target"
mkdir -p "$BASE/archive" "$REVIEW/linux"
printf '%s\n' "$UUID" >"$REVIEW/linux/run-id.txt"
printf '%s\n' "$BASE" >"$REVIEW/linux/tmp-base.txt"
printf '%s\n' "$NAME" >"$REVIEW/linux/container-name.txt"
printf '%s\n' "$$" >"$REVIEW/linux/driver.pid"

cleanup() {
  local status=$?
  if [ -n "${NAME:-}" ]; then
    docker stop "$NAME" >"$REVIEW/linux/docker-stop.txt" 2>&1 || true
    docker rm "$NAME" >"$REVIEW/linux/docker-rm.txt" 2>&1 || true
  fi
  if [ -n "${NET:-}" ]; then
    docker network rm "$NET" >"$REVIEW/linux/network-rm.txt" 2>&1 || true
  fi
  if [ -n "${VOLC:-}" ]; then
    docker volume rm "$VOLC" "$VOLT" >"$REVIEW/linux/volume-rm.txt" 2>&1 || true
  fi
  docker ps -a --filter "name=eef831aa-dba4-4af2-a78b-541bc9e0a109" --format '{{.ID}} {{.Names}} {{.Status}}' >"$REVIEW/linux/docker-ps-owned-after.txt" || true
  docker ps --format '{{.ID}} {{.Image}} {{.Names}} {{.Status}}' >"$REVIEW/linux/docker-ps-after.txt" || true
  if [ -n "${BASE:-}" ] && [ -d "$BASE" ]; then
    rm -rf "$BASE"
    echo "removed $BASE" >"$REVIEW/linux/tmp-removed.txt"
  fi
  echo "$status" >"$REVIEW/linux/driver.exit"
}
trap cleanup EXIT

cd "$REPO"
{
  echo "HEAD=$(git rev-parse HEAD)"
  echo "HEAD_TREE=$(git rev-parse 'HEAD^{tree}')"
  echo "BINDING=$COMMIT"
  echo "BINDING_TREE=$(git rev-parse "${COMMIT}^{tree}")"
  echo "BINDING_PARENT=$(git rev-parse "${COMMIT}^")"
  echo "time=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
} >"$REVIEW/linux/git-identity.txt"

git archive --format=tar "$COMMIT" \
  testing/harness/helper-evidence-check \
  testing/harness/containment-supervisor/Cargo.toml \
  testing/harness/containment-supervisor/Cargo.lock \
  testing/harness/containment-supervisor/provenance \
  ci/deny.toml \
  evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw/helper-metadata.out \
  evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw/age.txt \
  evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw/scan-audit.out \
  evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw/scan-deny-advisories.err \
  evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw/advisory-rev.txt \
  evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw/meta \
  evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw/upstream-licenses \
  >"$BASE/snapshot.tar"
shasum -a 256 "$BASE/snapshot.tar" >"$REVIEW/linux/snapshot.tar.sha256"
tar -xf "$BASE/snapshot.tar" -C "$BASE/archive"
shasum -a 256 "$BASE/archive/testing/harness/containment-supervisor/Cargo.lock" >"$REVIEW/linux/archive-lock.sha256"
lock_hash=$(awk '{print $1}' "$REVIEW/linux/archive-lock.sha256")
if [ "$lock_hash" != "14a0a0c66d604bae81f59b770e66a2d7cf00b38e427c3afbc8344bb4864189c1" ]; then
  echo "FAILED archive lock $lock_hash" | tee "$REVIEW/linux/status.txt"
  exit 3
fi

ps -ax -o pid=,comm= >"$REVIEW/linux/processes-before.txt"
docker ps --format '{{.ID}} {{.Image}} {{.Names}} {{.Status}}' >"$REVIEW/linux/docker-ps-before.txt"
host_rust=$(awk '$2=="cargo" || $2=="rustc" || $2=="rustup" { c++ } END { print c+0 }' "$REVIEW/linux/processes-before.txt")
rust_containers=$(awk 'BEGIN{c=0} $2 ~ /0f24958fa693/ || $2 ~ /rust/ { if ($3 !~ /a107-pg/ && $0 !~ /Exited/) c++ } END{ print c+0 }' "$REVIEW/linux/docker-ps-before.txt")
total=$((host_rust + rust_containers))
{
  echo "host_rust_comm=$host_rust"
  echo "rust_image_containers=$rust_containers"
  echo "total=$total"
} >"$REVIEW/linux/builder-count-before.txt"
if [ "$total" -ge 2 ]; then
  echo "FAILED builder cap total=$total" | tee "$REVIEW/linux/status.txt"
  exit 6
fi

docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a109 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a109 \
  --label oo.task=helper-license-review \
  --label oo.kind=rust-builder \
  --label oo.run="$UUID" \
  "$NET" >"$REVIEW/linux/network-id.txt"
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a109 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a109 \
  --label oo.task=helper-license-review \
  --label oo.kind=rust-builder \
  --label oo.run="$UUID" \
  "$VOLC" >"$REVIEW/linux/volume-cargo.txt"
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a109 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a109 \
  --label oo.task=helper-license-review \
  --label oo.kind=rust-builder \
  --label oo.run="$UUID" \
  "$VOLT" >"$REVIEW/linux/volume-target.txt"

docker run -d \
  --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory 8g \
  --memory-swap 8g \
  --cpus 4 \
  --pids-limit 4096 \
  --network "$NET" \
  --workdir /archive \
  --tmpfs /tmp:rw,nosuid,nodev,size=2g \
  -e CARGO_TERM_COLOR=never \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target/checker \
  -e CARGO_BUILD_JOBS=2 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/work/home \
  -e TMPDIR=/tmp \
  -e GIT_TERMINAL_PROMPT=0 \
  --mount "type=bind,source=${BASE}/archive,target=/archive,readonly" \
  --mount "type=bind,source=${REVIEW},target=/review" \
  --mount "type=volume,source=${VOLC},target=/cargo-home" \
  --mount "type=volume,source=${VOLT},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a109 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a109 \
  --label oo.task=helper-license-review \
  --label oo.kind=rust-builder \
  --label oo.slot=B \
  --label oo.run="$UUID" \
  "$IMAGE" \
  sleep 7200 >"$REVIEW/linux/container.id"

docker inspect --format 'Id={{.Id}} Image={{.Image}} Platform={{.Platform}} Privileged={{.HostConfig.Privileged}} NetworkMode={{.HostConfig.NetworkMode}} CapDrop={{json .HostConfig.CapDrop}} SecurityOpt={{json .HostConfig.SecurityOpt}} Memory={{.HostConfig.Memory}} MemorySwap={{.HostConfig.MemorySwap}} NanoCpus={{.HostConfig.NanoCpus}} PidsLimit={{.HostConfig.PidsLimit}}' "$NAME" >"$REVIEW/linux/hostconfig.txt"

set +e
docker exec "$NAME" bash /review/linux/inside.sh
exec_code=$?
set -e
echo "$exec_code" >"$REVIEW/linux/docker-exec.exit"
if [ -f "$REVIEW/linux/phases/smoke.exit" ]; then
  smoke=$(tr -d '[:space:]' <"$REVIEW/linux/phases/smoke.exit")
else
  smoke=$exec_code
fi
echo "smoke=$smoke exec=$exec_code" >"$REVIEW/linux/status.txt"
exit "$smoke"
