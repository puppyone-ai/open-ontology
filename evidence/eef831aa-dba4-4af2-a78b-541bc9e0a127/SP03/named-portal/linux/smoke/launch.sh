#!/bin/bash
# One owned linux/arm64 Rust 1.94 smoke builder. Immutable git archive of the fixed SHA.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a127/SP03/named-portal/linux/smoke
ROOT=/tmp/oo-a127-eef831aa
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a127-msrv-smoke
NET=oo-a127-eef831aa-net
CARGO_VOL=oo-a127-eef831aa-cargo
TARGET_VOL=oo-a127-eef831aa-target
FIXED=b8e7bf1b1c25fbc0f8213898eb7de84b248f8211

mkdir -p "$EVIDENCE"
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
rm -f "$EVIDENCE"/*.exit "$EVIDENCE"/console.log "$EVIDENCE"/hostconfig.txt
# vendor-match.exit is produced before launch; recreate it if the previous rm removed it.
if [ ! -f "$EVIDENCE/vendor-match.txt" ]; then
  echo "vendor-match.txt missing" >&2
  exit 2
fi

docker ps --filter label=oo.project=open-ontology --format 'name={{.Names}} kind={{.Label "oo.kind"}} owner={{.Label "oo.owner"}} status={{.Status}}' > "$EVIDENCE/containers-before.txt"
ps -axo pid=,ppid=,etime=,comm= > "$EVIDENCE/processes-comm-only.txt"
running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a127 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'running_rust_image_or_label=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s\n' "$running_rust" "$own" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ]; then
  echo "deferred: this task already has a running rust builder" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if [ "$((running_rust + host_cargo))" -ge 2 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi

test "$(cat "$EVIDENCE/source-head.txt")" = "$FIXED"
test "$(cat "$EVIDENCE/source-tree.txt")" = "e8f408d6f66ef5361dc7c6d6185ca7942eaf1819"
test "$(cat "$EVIDENCE/source-parent.txt")" = "578a55231e4b4373db014ff1450f8e1bb17ba639"
test -d "$ROOT/smoke-snapshot"
test ! -f "$ROOT/smoke-snapshot/crates/oo-connectors/vendor/sqlx-postgres/src/connection/portal.rs"

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"

if docker network inspect "$NET" >/dev/null 2>&1; then
  echo "network_exists=$NET" > "$EVIDENCE/network.txt"
else
  docker network create \
    --label oo.project=open-ontology \
    --label oo.owner=a127 \
    --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a127 \
    "$NET" > "$EVIDENCE/network.txt"
fi
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create --label oo.owner=a127 --label oo.project=open-ontology --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a127 "$CARGO_VOL" > "$EVIDENCE/cargo-volume.txt"
if [ ! -s "$EVIDENCE/cargo-volume.txt" ]; then
  docker volume inspect --format '{{.Name}}' "$CARGO_VOL" > "$EVIDENCE/cargo-volume.txt"
fi
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create --label oo.owner=a127 --label oo.project=open-ontology --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a127 "$TARGET_VOL" > "$EVIDENCE/target-volume.txt"
if [ ! -s "$EVIDENCE/target-volume.txt" ]; then
  docker volume inspect --format '{{.Name}}' "$TARGET_VOL" > "$EVIDENCE/target-volume.txt"
fi

if docker container inspect "$NAME" >/dev/null 2>&1; then
  prior=$(docker inspect -f '{{.State.Status}}' "$NAME")
  printf 'prior_status=%s\n' "$prior" > "$EVIDENCE/prior-container.txt"
  if [ "$prior" = "running" ]; then
    echo "deferred: named container is already running" >&2
    printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
    exit 3
  fi
  docker rm "$NAME" >> "$EVIDENCE/prior-container.txt"
fi

cat > "$EVIDENCE/invocation.txt" <<EOF
source_commit=${FIXED}
source_parent=578a55231e4b4373db014ff1450f8e1bb17ba639
source_tree=e8f408d6f66ef5361dc7c6d6185ca7942eaf1819
accepted_driver=ff783ac933b86e0ff068181afe0258764c5d2e5d
overlay=none
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
msrv_override=RUSTUP_TOOLCHAIN=1.94.0
network=${NET}
network_mode=bridge_not_host
container=${NAME}
cargo_volume=${CARGO_VOL}
target_volume=${TARGET_VOL}
snapshot=${ROOT}/smoke-snapshot
snapshot_method=complete git archive of the fixed SHA
prior_attempt=1
prior_attempt_exit=101
prior_attempt_cause=partial archive omitted contracts/source/pgsql/empty-catalog.json
privileged=no
home_mount=no
socket_mount=no
host_port=none
shared_cargo_cache=no
pull=never
cap_drop=ALL
cap_add=none
init=yes
no_new_privileges=yes
memory=8g
memory_swap=8g
cpus=4
pids_limit=4096
user=0:0
fetch=cargo fetch --manifest-path Cargo.toml --locked
lib=cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --lib
clippy=cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings
airgap_claim=no
EOF

set +e
docker run \
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
  -e CARGO_TERM_COLOR=never \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=4 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/tmp \
  --mount "type=bind,source=${ROOT}/smoke-snapshot,target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a127 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a127 \
  --label oo.task=named-portal-smoke \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
  echo "Status=$(docker inspect -f '{{.State.Status}}' "$NAME")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
  echo "MemorySwap=$(docker inspect -f '{{.HostConfig.MemorySwap}}' "$NAME")"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME")"
  echo "PidsLimit=$(docker inspect -f '{{.HostConfig.PidsLimit}}' "$NAME")"
  echo "CapDrop=$(docker inspect -f '{{.HostConfig.CapDrop}}' "$NAME")"
  echo "CapAdd=$(docker inspect -f '{{.HostConfig.CapAdd}}' "$NAME")"
  echo "SecurityOpt=$(docker inspect -f '{{.HostConfig.SecurityOpt}}' "$NAME")"
  docker inspect -f '{{range .Mounts}}mount dest={{.Destination}} type={{.Type}} rw={{.RW}}{{println}}{{end}}' "$NAME"
} > "$EVIDENCE/hostconfig.txt"
printf '%s\n' 0 > "$EVIDENCE/inspect.exit"
exit "$run_exit"
