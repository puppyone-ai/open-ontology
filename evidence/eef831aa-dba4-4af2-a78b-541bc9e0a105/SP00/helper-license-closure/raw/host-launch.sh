#!/bin/bash
# One Linux arm64 Rust 1.94 container. Helper smoke on a readonly f1a6005 archive, then the verifier.
set -eu
REPO="/Users/redux/puppy-one/open-ontology"
RAW="$REPO/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a105/SP00/helper-license-closure/raw"
COMMIT=f1a6005971e3dc3075094c87ed670fc9d25e9ae0
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
UUID=$(uuidgen | tr '[:upper:]' '[:lower:]')
BASE="/tmp/oo-a105-${UUID}"
NAME="oo-a105-helper-evidence-${UUID}"
NET="oo-a105-${UUID}-net"
VOLC="oo-a105-${UUID}-cargo"
VOLT="oo-a105-${UUID}-target"
mkdir -p "$BASE/archive" "$BASE/repo" "$RAW"
printf '%s\n' "$UUID" > "$RAW/run-id.txt"
printf '%s\n' "$BASE" > "$RAW/tmp-base.txt"
printf '%s\n' "$NAME" > "$RAW/container-name.txt"

cd "$REPO"
{
  echo "named_baseline=e33f108b8226a7e5a80fa184a9a531751d491aa2"
  echo "HEAD=$(git rev-parse HEAD)"
  echo "HEAD_TREE=$(git rev-parse 'HEAD^{tree}')"
  echo "HEAD_PARENT=$(git rev-parse 'HEAD^')"
  echo "FIXED=$(git rev-parse "$COMMIT")"
  echo "FIXED_TREE=$(git rev-parse "${COMMIT}^{tree}")"
  echo "FIXED_PARENT=$(git rev-parse "${COMMIT}^")"
  echo "time=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
} > "$RAW/git-identity.txt"

git archive --format=tar "$COMMIT" testing/harness/containment-supervisor > "$BASE/snapshot.tar"
shasum -a 256 "$BASE/snapshot.tar" > "$RAW/snapshot.tar.sha256"
tar -xf "$BASE/snapshot.tar" -C "$BASE/archive"
shasum -a 256 "$BASE/archive/testing/harness/containment-supervisor/Cargo.lock" > "$RAW/archive-lock.sha256"
lock_hash=$(awk '{print $1}' "$RAW/archive-lock.sha256")
if [ "$lock_hash" != "14a0a0c66d604bae81f59b770e66a2d7cf00b38e427c3afbc8344bb4864189c1" ]; then
  echo "FAILED archive lock hash $lock_hash" | tee "$RAW/status.txt"
  exit 3
fi

STAGE="$BASE/repo"
mkdir -p \
  "$STAGE/testing/harness/containment-supervisor/provenance" \
  "$STAGE/testing/harness/helper-evidence-check" \
  "$STAGE/ci" \
  "$STAGE/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw/meta" \
  "$STAGE/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw/upstream-licenses"
rsync -a "$REPO/testing/harness/helper-evidence-check/" "$STAGE/testing/harness/helper-evidence-check/"
cp "$REPO/testing/harness/containment-supervisor/Cargo.toml" "$REPO/testing/harness/containment-supervisor/Cargo.lock" \
  "$STAGE/testing/harness/containment-supervisor/"
rsync -a "$REPO/testing/harness/containment-supervisor/provenance/" \
  "$STAGE/testing/harness/containment-supervisor/provenance/"
cp "$REPO/ci/deny.toml" "$STAGE/ci/deny.toml"
A101="$REPO/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw"
DST="$STAGE/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw"
cp "$A101/helper-metadata.out" "$A101/age.txt" "$A101/scan-audit.out" \
  "$A101/scan-deny-advisories.err" "$A101/advisory-rev.txt" "$DST/"
cp "$A101/meta/version-bitflags.json" "$A101/meta/version-cfg-if.json" \
  "$A101/meta/version-cfg_aliases.json" "$A101/meta/version-libc.json" \
  "$A101/meta/version-nix.json" "$DST/meta/"
rsync -a "$A101/upstream-licenses/" "$DST/upstream-licenses/"

set +e
python3 "$REPO/governance/check_governance.py" --self-test > "$RAW/governance-self-test.out" 2> "$RAW/governance-self-test.err"
echo $? > "$RAW/governance-self-test.exit"
set -e

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
  echo "FAILED builder cap total=$total" | tee "$RAW/status.txt"
  exit 6
fi

cleanup() {
  if [ -n "${NAME:-}" ]; then
    docker stop "$NAME" > "$RAW/docker-stop.txt" 2>&1 || true
    docker rm "$NAME" > "$RAW/docker-rm.txt" 2>&1 || true
  fi
  if [ -n "${NET:-}" ]; then
    docker network rm "$NET" > "$RAW/network-rm.txt" 2>&1 || true
  fi
  if [ -n "${VOLC:-}" ]; then
    docker volume rm "$VOLC" "$VOLT" > "$RAW/volume-rm.txt" 2>&1 || true
  fi
  docker ps --format '{{.ID}} {{.Image}} {{.Names}} {{.Status}}' > "$RAW/docker-ps-after.txt" || true
}
trap cleanup EXIT

docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a105 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a105 \
  --label oo.task=helper-license-closure \
  --label oo.run="$UUID" \
  "$NET" > "$RAW/network-id.txt"
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a105 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a105 \
  --label oo.task=helper-license-closure \
  --label oo.run="$UUID" \
  "$VOLC" > "$RAW/volume-cargo.txt"
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a105 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a105 \
  --label oo.task=helper-license-closure \
  --label oo.run="$UUID" \
  "$VOLT" > "$RAW/volume-target.txt"

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
  --mount "type=bind,source=${BASE}/archive,target=/archive,readonly" \
  --mount "type=bind,source=${BASE}/repo,target=/repo,readonly" \
  --mount "type=bind,source=${RAW},target=/work" \
  --mount "type=volume,source=${VOLC},target=/cargo-home" \
  --mount "type=volume,source=${VOLT},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a105 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a105 \
  --label oo.task=helper-license-closure \
  --label oo.run="$UUID" \
  "$IMAGE" \
  sleep 7200 > "$RAW/container.id"

docker inspect --format 'Id={{.Id}} Image={{.Image}} Platform={{.Platform}} Privileged={{.HostConfig.Privileged}} NetworkMode={{.HostConfig.NetworkMode}} CapDrop={{json .HostConfig.CapDrop}} SecurityOpt={{json .HostConfig.SecurityOpt}} Memory={{.HostConfig.Memory}} MemorySwap={{.HostConfig.MemorySwap}} NanoCpus={{.HostConfig.NanoCpus}} PidsLimit={{.HostConfig.PidsLimit}} ReadonlyRootfs={{.HostConfig.ReadonlyRootfs}}' "$NAME" > "$RAW/hostconfig.txt"

set +e
docker exec "$NAME" bash /work/inside.sh > "$RAW/inside-console.out" 2> "$RAW/inside-console.err"
exec_rc=$?
set -e
echo "$exec_rc" > "$RAW/docker-exec.exit"
if [ "$exec_rc" -ne 0 ]; then
  if [ ! -f "$RAW/status.txt" ]; then
    echo "FAILED docker-exec $exec_rc" > "$RAW/status.txt"
  fi
  echo "FAILED exec_rc=$exec_rc"
  exit "$exec_rc"
fi
if ! grep -q '^DONE$' "$RAW/status.txt"; then
  echo "FAILED missing DONE" | tee "$RAW/status.txt"
  exit 1
fi
echo "DONE $NAME"
