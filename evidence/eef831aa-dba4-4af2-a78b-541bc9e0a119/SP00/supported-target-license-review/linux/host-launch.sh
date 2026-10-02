#!/bin/bash
# Host driver for one a119 container. Does not stop, remove, or prune any other run.
set -eu
REPO="/Users/redux/puppy-one/open-ontology"
EVID="$REPO/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a119/SP00/supported-target-license-review"
COMMIT="341ece0ea25ae50ff054a4581e8a3389ccb2dd66"
TREE="b9b8d6119ab167aeba4d6feafe031c491ae3f727"
PARENT="3963fc2b0ece54cabe62071153918b2df815b1c7"
IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
UUID=$(uuidgen | tr '[:upper:]' '[:lower:]')
BASE="/tmp/oo-a119-${UUID}"
NAME="eef831aa-dba4-4af2-a78b-541bc9e0a119-rust"
NET="oo-a119-${UUID}-net"
VOLC="oo-a119-${UUID}-cargo"
VOLT="oo-a119-${UUID}-target"
LOG="$EVID/linux/host-launch.log"
mkdir -p "$EVID/linux" "$EVID/raw" "$EVID/identity"
printf '%s\n' "$UUID" >"$EVID/linux/run-id.txt"
printf '%s\n' "$NAME" >"$EVID/linux/container-name.txt"
printf '%s\n' "$$" >"$EVID/linux/driver.pid"
date -u +%Y-%m-%dT%H:%M:%SZ >"$EVID/linux/registered-at.txt"

say() { printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" | tee -a "$LOG" >/dev/null; }

cleanup() {
  local status=$?
  trap - EXIT
  if [ -n "${NAME:-}" ]; then
    docker stop "$NAME" >"$EVID/linux/docker-stop.txt" 2>&1 || true
    docker rm "$NAME" >"$EVID/linux/docker-rm.txt" 2>&1 || true
  fi
  if [ -n "${NET:-}" ]; then
    docker network rm "$NET" >"$EVID/linux/network-rm.txt" 2>&1 || true
  fi
  if [ -n "${VOLC:-}" ]; then
    docker volume rm "$VOLC" "$VOLT" >"$EVID/linux/volume-rm.txt" 2>&1 || true
  fi
  docker ps -a --filter "name=eef831aa-dba4-4af2-a78b-541bc9e0a119-rust" --format '{{.ID}} {{.Names}} {{.Status}}' >"$EVID/linux/docker-ps-owned-after.txt" || true
  docker ps --format '{{.Names}} {{.Status}}' >"$EVID/linux/docker-ps-after.txt" || true
  if [ -n "${BASE:-}" ] && [ -d "$BASE" ]; then
    rm -rf "$BASE"
    echo "removed" >"$EVID/linux/tmp-removed.txt"
  fi
  echo "$status" >"$EVID/linux/driver.exit"
  if [ "$status" -eq 0 ]; then
    echo "DONE" >>"$LOG"
  else
    echo "FAILED exit=$status" >>"$LOG"
  fi
  exit "$status"
}
trap cleanup EXIT

cd "$REPO"
{
  echo "HEAD=$(git rev-parse HEAD)"
  echo "HEAD_TREE=$(git rev-parse 'HEAD^{tree}')"
  echo "INPUT=$COMMIT"
  echo "INPUT_TREE=$(git rev-parse "$COMMIT^{tree}")"
  echo "INPUT_PARENT=$(git rev-parse "$COMMIT^")"
} >"$EVID/identity/git.txt"
got_tree=$(git rev-parse "$COMMIT^{tree}")
got_parent=$(git rev-parse "$COMMIT^")
if [ "$got_tree" != "$TREE" ] || [ "$got_parent" != "$PARENT" ]; then
  say "FAILED input identity"
  exit 4
fi

say "register expected and probe bytes"
sha256sum "$EVID/expected/outcomes.txt" "$EVID/probe/graph_oracle.rs" "$EVID/controls/"*.toml >"$EVID/expected/registered.sha256"

say "capacity before"
host_rust=$(ps -ax -o comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustup" || $1=="rustdoc" || $1=="rust-lld" { c++ } END { print c+0 }')
rust_up=$(docker ps --format '{{.Image}} {{.Names}}' | awk 'BEGIN{c=0} tolower($0) ~ /rust/ && tolower($0) !~ /-pg/ { c++ } END { print c+0 }')
a116_pg=$(docker ps --filter name=eef831aa-dba4-4af2-a78b-541bc9e0a116-pg --format '{{.Names}} {{.Status}}')
{
  echo "host_rust_comm=$host_rust"
  echo "running_rust_containers=$rust_up"
  echo "a116_pg=${a116_pg:-absent}"
} >"$EVID/linux/builder-count-before.txt"
if [ "$host_rust" != "0" ] || [ "$rust_up" -ge 2 ]; then
  say "FAILED builder cap host=$host_rust rust_up=$rust_up"
  exit 6
fi

say "extract archive"
mkdir -p "$BASE/src"
git archive "$COMMIT" | tar -x -C "$BASE/src"
(
  cd "$BASE/src"
  shasum -a 256 Cargo.lock ci/deny.toml crates/oo-sdk/Cargo.toml crates/oo-connectors/Cargo.toml rust-toolchain.toml
) >"$EVID/identity/archive-sha256.txt"
lock_sha=$(awk '$2=="Cargo.lock"{print $1}' "$EVID/identity/archive-sha256.txt")
deny_sha=$(awk '$2=="ci/deny.toml"{print $1}' "$EVID/identity/archive-sha256.txt")
if [ "$lock_sha" != "d3cf96bc32f829cc9a1e7818c064a4447b1bb5b06cffe4501e82d5617108b06f" ] || [ "$deny_sha" != "6483e22c78bedfbaa55d1558470a70c79552bac50377a4da802bad0a2df1f3e3" ]; then
  say "FAILED archive hashes"
  exit 4
fi

say "create network and volumes"
docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a119 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a119 \
  --label oo.kind=rust-builder \
  "$NET" >"$EVID/linux/network-id.txt"
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a119 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a119 \
  --label oo.kind=cargo-home \
  "$VOLC" >"$EVID/linux/volume-cargo.txt"
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a119 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a119 \
  --label oo.kind=target \
  "$VOLT" >"$EVID/linux/volume-target.txt"

say "capacity launch"
host_rust=$(ps -ax -o comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustup" || $1=="rustdoc" || $1=="rust-lld" { c++ } END { print c+0 }')
rust_up=$(docker ps --format '{{.Image}} {{.Names}}' | awk 'BEGIN{c=0} tolower($0) ~ /rust/ && tolower($0) !~ /-pg/ { c++ } END { print c+0 }')
{
  echo "host_rust_comm=$host_rust"
  echo "running_rust_containers=$rust_up"
} >"$EVID/linux/builder-count-launch.txt"
if [ "$host_rust" != "0" ] || [ "$rust_up" -ge 2 ]; then
  say "FAILED builder cap at launch host=$host_rust rust_up=$rust_up"
  exit 6
fi

say "start container"
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
  --tmpfs /work:rw,nosuid,nodev,size=1g \
  -e CARGO_TERM_COLOR=never \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target/a119 \
  -e CARGO_BUILD_JOBS=2 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/work/home \
  -e TMPDIR=/tmp \
  -e GIT_TERMINAL_PROMPT=0 \
  --mount "type=bind,source=${BASE}/src,target=/src,readonly" \
  --mount "type=bind,source=${EVID},target=/evidence" \
  --mount "type=volume,source=${VOLC},target=/cargo-home" \
  --mount "type=volume,source=${VOLT},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a119 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a119 \
  --label oo.kind=rust-builder \
  --label oo.slot=B \
  "$IMAGE" \
  sleep 21600 >"$EVID/linux/container.id"

docker inspect --format 'Privileged={{.HostConfig.Privileged}} CapDrop={{json .HostConfig.CapDrop}} SecurityOpt={{json .HostConfig.SecurityOpt}} NetworkMode={{.HostConfig.NetworkMode}} Memory={{.HostConfig.Memory}} MemorySwap={{.HostConfig.MemorySwap}} NanoCpus={{.HostConfig.NanoCpus}} PidsLimit={{.HostConfig.PidsLimit}}' "$NAME" >"$EVID/linux/hostconfig.txt"
docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' >"$EVID/linux/image-id.txt"

say "exec inside"
set +e
docker exec "$NAME" bash /evidence/linux/inside.sh
inside_code=$?
set -e
printf '%s\n' "$inside_code" >"$EVID/linux/inside.exit"
if [ "$inside_code" -ne 0 ]; then
  say "FAILED inside exit=$inside_code"
  exit "$inside_code"
fi
say "inside finished"
exit 0
