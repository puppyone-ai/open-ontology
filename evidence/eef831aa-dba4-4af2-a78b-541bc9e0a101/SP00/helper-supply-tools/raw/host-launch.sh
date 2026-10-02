#!/bin/bash
set -eu
RAW="/Users/redux/puppy-one/open-ontology/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw"
REPO="/Users/redux/puppy-one/open-ontology"
COMMIT=f1a6005971e3dc3075094c87ed670fc9d25e9ae0
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
UUID=$(uuidgen | tr '[:upper:]' '[:lower:]')
BASE="/tmp/oo-a101-${UUID}"
printf '%s\n' "$UUID" > "$RAW/run-id.txt"
printf '%s\n' "$BASE" > "$RAW/tmp-base.txt"
mkdir -p "$BASE/snapshot"
{
  echo "uuid=$UUID"
  echo "base=$BASE"
  echo "commit=$COMMIT"
  echo "time=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
} > "$RAW/launch-identity.txt"

cd "$REPO"
{
  echo "HEAD=$(git rev-parse HEAD)"
  echo "HEAD_TREE=$(git rev-parse 'HEAD^{tree}')"
  echo "HEAD_PARENT=$(git rev-parse 'HEAD^')"
  echo "FIXED=$(git rev-parse "$COMMIT")"
  echo "FIXED_TREE=$(git rev-parse "${COMMIT}^{tree}")"
  echo "FIXED_PARENT=$(git rev-parse "${COMMIT}^")"
} > "$RAW/git-identity.txt"

git archive --format=tar "$COMMIT" \
  testing/harness/containment-supervisor \
  ci/deny.toml \
  rust-toolchain.toml \
  supply-chain/policy.md \
  supply-chain/licenses/harness \
  > "$BASE/snapshot.tar"
shasum -a 256 "$BASE/snapshot.tar" > "$RAW/snapshot.tar.sha256"
tar -xf "$BASE/snapshot.tar" -C "$BASE/snapshot"
( cd "$BASE/snapshot" && find . -type f | sort | while IFS= read -r p; do shasum -a 256 "$p"; done ) > "$RAW/snapshot-files.sha256"
shasum -a 256 "$BASE/snapshot/testing/harness/containment-supervisor/Cargo.lock" > "$RAW/snapshot-lock.sha256"
shasum -a 256 "$BASE/snapshot/ci/deny.toml" > "$RAW/snapshot-deny.sha256"

ps -ax -o pid=,ppid=,etime=,comm= > "$RAW/processes-before.txt"
docker ps --format '{{.ID}} {{.Image}} {{.Names}} {{.Status}}' > "$RAW/docker-ps-before.txt"

host_rust=$(awk '$4=="cargo" || $4=="rustc" || $4=="rustup" { c++ } END { print c+0 }' "$RAW/processes-before.txt")
rust_containers=$(awk 'BEGIN{c=0} /rust/ || /0f24958fa693/ { c++ } END{ print c+0 }' "$RAW/docker-ps-before.txt")
total=$((host_rust + rust_containers))
{
  echo "host_rust_comm=$host_rust"
  echo "rust_image_containers=$rust_containers"
  echo "total=$total"
} > "$RAW/builder-count-before.txt"
if [ "$total" -ge 2 ]; then
  echo "BUILDER_CAP total=$total" > "$RAW/unverified.txt"
  exit 6
fi

docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a101 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a101 \
  --label oo.task=helper-supply-tools \
  --label oo.run="$UUID" \
  "oo-a101-${UUID}-net" > "$RAW/network-id.txt"
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a101 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a101 \
  --label oo.task=helper-supply-tools \
  --label oo.run="$UUID" \
  "oo-a101-${UUID}-cargo" > "$RAW/volume-cargo.txt"
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a101 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a101 \
  --label oo.task=helper-supply-tools \
  --label oo.run="$UUID" \
  "oo-a101-${UUID}-target" > "$RAW/volume-target.txt"

NAME="oo-a101-helper-supply-${UUID}"
printf '%s\n' "$NAME" > "$RAW/container-name.txt"

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
  --network "oo-a101-${UUID}-net" \
  --workdir /src \
  --tmpfs /tmp:rw,nosuid,nodev,size=2g \
  -e CARGO_TERM_COLOR=never \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=2 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/work/home \
  -e TMPDIR=/tmp \
  -e GIT_TERMINAL_PROMPT=0 \
  --mount "type=bind,source=${BASE}/snapshot,target=/src,readonly" \
  --mount "type=bind,source=${RAW},target=/work" \
  --mount "type=volume,source=oo-a101-${UUID}-cargo,target=/cargo-home" \
  --mount "type=volume,source=oo-a101-${UUID}-target,target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a101 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a101 \
  --label oo.task=helper-supply-tools \
  --label oo.run="$UUID" \
  "$IMAGE" \
  bash /work/inside.sh > "$RAW/container.id"

docker inspect --format 'Id={{.Id}} Image={{.Image}} Platform={{.Platform}} Privileged={{.HostConfig.Privileged}} NetworkMode={{.HostConfig.NetworkMode}} CapDrop={{json .HostConfig.CapDrop}} SecurityOpt={{json .HostConfig.SecurityOpt}} Memory={{.HostConfig.Memory}} MemorySwap={{.HostConfig.MemorySwap}} NanoCpus={{.HostConfig.NanoCpus}} PidsLimit={{.HostConfig.PidsLimit}} ReadonlyRootfs={{.HostConfig.ReadonlyRootfs}}' "$NAME" > "$RAW/hostconfig.txt"

echo "LAUNCHED $NAME"

ready=0
for _ in $(seq 1 24); do
  if [ -f "$RAW/fetch-done" ] || [ -f "$RAW/scan-done" ]; then
    ready=1
    break
  fi
  sleep 10
done
if [ ! -f "$RAW/fetch-done" ]; then
  echo "FETCH_NOT_READY_YET" > "$RAW/launch-wait.txt"
  exit 0
fi
ps -ax -o pid=,ppid=,etime=,comm= > "$RAW/processes-before-install.txt"
docker ps --format '{{.ID}} {{.Image}} {{.Names}} {{.Status}}' > "$RAW/docker-ps-before-install.txt"
host_rust=$(awk '$4=="cargo" || $4=="rustc" || $4=="rustup" { c++ } END { print c+0 }' "$RAW/processes-before-install.txt")
rust_containers=$(awk 'BEGIN{c=0} /rust/ || /0f24958fa693/ { c++ } END{ print c+0 }' "$RAW/docker-ps-before-install.txt")
total=$((host_rust + rust_containers))
{
  echo "host_rust_comm=$host_rust"
  echo "rust_image_containers=$rust_containers"
  echo "total=$total"
  echo "time=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
} > "$RAW/builder-count-before-install.txt"
if [ "$total" -gt 2 ]; then
  echo "BUILDER_CAP_BEFORE_INSTALL total=$total" > "$RAW/launch-wait.txt"
  exit 0
fi
{
  echo "time=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "builders_total=$total"
  echo "host_rust_comm=$host_rust"
  echo "rust_image_containers=$rust_containers"
} > "$RAW/build-go"
echo "BUILD_GO total=$total" 
