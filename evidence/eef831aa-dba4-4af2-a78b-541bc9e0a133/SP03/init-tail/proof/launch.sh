#!/bin/bash
# One owned linux/arm64 Rust 1.94 builder. Archive of 79515fcf plus the tail overlay only.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a133/SP03/init-tail
ROOT=/tmp/oo-a133-eef831aa
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a133-init-tail-proof
NET=oo-a133-eef831aa-net
CARGO_VOL=oo-a133-eef831aa-cargo
TARGET_VOL=oo-a133-eef831aa-target
SOURCE=79515fcf7e7032c8c212540eaae4b48a5957e3a2
SNAP=${ROOT}/proof-snapshot
FIXTURE_HASH=044022fcabb7587632133cb8f81c3665292c32acc7af3432b5bfe4d066679a3c
OVERLAY_BUF=${SRC}/crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs
OVERLAY_PATCH=${SRC}/crates/oo-connectors/vendor/PATCH.md
OVERLAY_TEST=${SRC}/crates/oo-connectors/tests/pg_wire_tls_tail.rs

mkdir -p "$EVIDENCE/proof" "$ROOT"
chmod 755 "$EVIDENCE/proof/inside.sh" "$EVIDENCE/proof/launch.sh"

running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a133 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'running_rust_image_or_label=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s\n' "$running_rust" "$own" "$host_cargo" > "$EVIDENCE/proof/capacity.txt"
if [ "$own" -ge 1 ] || [ "$((running_rust + host_cargo))" -ge 2 ]; then
  echo "deferred: rust builder capacity is full" >&2
  printf '%s\n' 3 > "$EVIDENCE/proof/docker-run.exit"
  exit 3
fi

git -C "$SRC" rev-parse "$SOURCE" > "$EVIDENCE/proof/source-commit.txt"
git -C "$SRC" rev-parse "${SOURCE}^{tree}" > "$EVIDENCE/proof/source-tree.txt"

rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$SRC" archive --format=tar "$SOURCE" \
  Cargo.toml Cargo.lock rust-toolchain.toml crates .cargo contracts/source/pgsql/empty-catalog.json \
  | tar -C "$SNAP" -xf -
test -f "$SNAP/contracts/source/pgsql/empty-catalog.json"
got=$(shasum -a 256 "$SNAP/contracts/source/pgsql/empty-catalog.json" | awk '{print $1}')
if [ "$got" != "$FIXTURE_HASH" ]; then
  echo "fixture hash ${got} != ${FIXTURE_HASH}" >&2
  printf '%s\n' 4 > "$EVIDENCE/proof/docker-run.exit"
  exit 4
fi

session_commit=$(git -C "$SRC" rev-parse "${SOURCE}:crates/oo-connectors/src/session.rs")
session_snap=$(git -C "$SRC" hash-object "$SNAP/crates/oo-connectors/src/session.rs")
if [ "$session_commit" != "$session_snap" ]; then
  echo "snapshot session.rs is not the fixed archive" >&2
  printf '%s\n' 4 > "$EVIDENCE/proof/docker-run.exit"
  exit 4
fi
printf 'session.rs %s\n' "$session_snap" > "$EVIDENCE/proof/archive-identity.txt"

install -m 644 "$OVERLAY_BUF" "$SNAP/crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs"
install -m 644 "$OVERLAY_PATCH" "$SNAP/crates/oo-connectors/vendor/PATCH.md"
install -m 644 "$OVERLAY_TEST" "$SNAP/crates/oo-connectors/tests/pg_wire_tls_tail.rs"
grep -q 'fn committed_init_len' "$SNAP/crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs"
if [ -f "$SNAP/crates/oo-connectors/src/stream.rs" ]; then
  if ! git -C "$SRC" cat-file -e "${SOURCE}:crates/oo-connectors/src/stream.rs" 2>/dev/null; then
    echo "snapshot picked up untracked stream.rs" >&2
    printf '%s\n' 4 > "$EVIDENCE/proof/docker-run.exit"
    exit 4
  fi
fi
printf '%s\n' 0 > "$EVIDENCE/proof/snapshot-prep.exit"

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/proof/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/proof/image.txt"

if docker container inspect "$NAME" >/dev/null 2>&1; then
  prior=$(docker inspect -f '{{.State.Status}}' "$NAME")
  if [ "$prior" = "running" ]; then
    echo "deferred: named container is already running" >&2
    printf '%s\n' 3 > "$EVIDENCE/proof/docker-run.exit"
    exit 3
  fi
  docker rm "$NAME" >/dev/null
fi

if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create \
    --label oo.project=open-ontology \
    --label oo.owner=a133 \
    --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a133 \
    "$NET" >/dev/null
fi
docker volume create --label oo.project=open-ontology --label oo.owner=a133 --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a133 "$CARGO_VOL" >/dev/null
docker volume create --label oo.project=open-ontology --label oo.owner=a133 --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a133 "$TARGET_VOL" >/dev/null

cat > "$EVIDENCE/proof/invocation.txt" <<EOF
source_commit=${SOURCE}
source_tree=$(cat "$EVIDENCE/proof/source-tree.txt")
snapshot_method=git archive of ${SOURCE} plus buffered.rs PATCH.md pg_wire_tls_tail.rs overlay
fixture_sha256=${FIXTURE_HASH}
session_blob=${session_snap}
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
msrv_override=RUSTUP_TOOLCHAIN=1.94.0
network=${NET}
network_mode=bridge_not_host
container=${NAME}
cargo_volume=${CARGO_VOL}
target_volume=${TARGET_VOL}
snapshot=${SNAP}
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
postgres=no
phases=fetch,lib,bounds,accounting,portal,view,tail,sqlx-core,clippy
EOF

set +e
docker run --name "$NAME" \
  --pull never \
  --platform linux/arm64 \
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
  --mount "type=bind,src=${SNAP},dst=/src,readonly" \
  --mount "type=bind,src=${EVIDENCE},dst=/evidence" \
  --mount "type=volume,src=${CARGO_VOL},dst=/cargo-home" \
  --mount "type=volume,src=${TARGET_VOL},dst=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a133 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a133 \
  --label oo.kind=rust-builder \
  --label oo.task=init-tail-proof \
  "$IMAGE" \
  bash /evidence/proof/inside.sh \
  > "$EVIDENCE/proof/console.log" 2>&1
run_exit=$?
set -e
printf '%s\n' "$run_exit" > "$EVIDENCE/proof/docker-run.exit"
docker inspect "$NAME" --format 'ExitCode={{.State.ExitCode}}
Status={{.State.Status}}
OOMKilled={{.State.OOMKilled}}
Privileged={{.HostConfig.Privileged}}
NetworkMode={{.HostConfig.NetworkMode}}
Memory={{.HostConfig.Memory}}
MemorySwap={{.HostConfig.MemorySwap}}
NanoCpus={{.HostConfig.NanoCpus}}
PidsLimit={{.HostConfig.PidsLimit}}
CapDrop={{json .HostConfig.CapDrop}}
CapAdd={{json .HostConfig.CapAdd}}
SecurityOpt={{json .HostConfig.SecurityOpt}}
{{range .Mounts}}mount dest={{.Destination}} type={{.Type}} rw={{.RW}}
{{end}}' > "$EVIDENCE/proof/hostconfig.txt" || true
docker rm "$NAME" > "$EVIDENCE/proof/container-rm.txt"
echo "HOST_DONE docker_run_exit=${run_exit}"
exit "$run_exit"
