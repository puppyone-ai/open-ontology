#!/bin/bash
set -eu
EVIDENCE=/Users/redux/puppy-one/open-ontology/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a070/SP00/escape-containment/linux-arm64
ROOT=/tmp/oo-a070-fixed-7b753993-c087-4c9b-9aa7-5ba409e11708
RUN_ID=$(uuidgen | tr 'A-Z' 'a-z')
printf '%s\n' "$RUN_ID" >"$EVIDENCE/run-id.txt"
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a070-fixed-${RUN_ID}
CARGO_VOL=oo-a070-${RUN_ID}-cargo
TARGET_VOL=oo-a070-${RUN_ID}-target

docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.kind"}} {{.Label "oo.owner"}}' >"$EVIDENCE/builders-observed.txt"
rust_builders=$(docker ps --filter label=oo.kind=rust-builder --format '{{.ID}} {{.Names}} {{.Label "oo.owner"}}')
image_builders=$(docker ps --format '{{.ID}} {{.Names}} {{.Image}}' | awk '/0f24958fa693|rust@sha256:0f24958fa693/ {print}')
host_cargo=$(ps -ax -o pid=,command= | awk '/[c]argo |[r]ustc / && $0 !~ /docker run/ {print}')
{
  printf 'label_rust_builders:\n%s\n' "$rust_builders"
  printf 'image_builders:\n%s\n' "$image_builders"
  printf 'host_cargo:\n%s\n' "$host_cargo"
} >"$EVIDENCE/builder-count.txt"
label_n=$(printf '%s\n' "$rust_builders" | awk 'NF{n++} END{print n+0}')
image_n=$(printf '%s\n' "$image_builders" | awk 'NF{n++} END{print n+0}')
host_n=$(printf '%s\n' "$host_cargo" | awk 'NF{n++} END{print n+0}')
running_n=$image_n
if [ "$label_n" -gt "$running_n" ]; then running_n=$label_n; fi
global_n=$((running_n + host_n))
if [ "$global_n" -ge 2 ]; then
  echo "deferred: $global_n rust builders already running" >&2
  printf '%s\n' 3 >"$EVIDENCE/launch.exit"
  exit 3
fi

docker volume create "$CARGO_VOL" >"$EVIDENCE/volume-cargo.txt"
docker volume create "$TARGET_VOL" >"$EVIDENCE/volume-target.txt"

cat >"$EVIDENCE/invocation.txt" <<EOF
run_id=${RUN_ID}
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
msrv_override=RUSTUP_TOOLCHAIN=1.94.0
container=${NAME}
cargo_volume=${CARGO_VOL}
target_volume=${TARGET_VOL}
snapshot=${ROOT}/snapshot
snapshot_tar_sha256=$(cat "$ROOT/snapshot.tar.sha256")
archive_commit=$(cat "$EVIDENCE/archive-commit.txt")
archive_tree=$(cat "$EVIDENCE/archive-tree.txt")
archive_parent=$(cat "$EVIDENCE/archive-parent.txt")
privileged=no
network_mode=bridge
home_mount=no
socket_mount=no
shared_cargo_cache=no
pull=never
init=yes
user=0:0
cap_drop=ALL
security_opt=no-new-privileges
memory=8g
memory_swap=8g
cpus=4
pids_limit=4096
tmpfs=/tmp:rw,nosuid,nodev,noexec,size=2g
src_readonly=yes
command=cargo fetch --manifest-path Cargo.toml --locked; cargo test --locked --offline -p xtask; cargo clippy --locked --offline -p xtask --all-targets -- -D warnings; independent escape probe
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
  --network bridge \
  --workdir /src \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=2g \
  -e CARGO_TERM_COLOR=never \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=4 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/tmp \
  --mount "type=bind,source=${ROOT}/snapshot,target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/out" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a070 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a070 \
  --label oo.task=escape-containment-fixed \
  --label oo.run_id="${RUN_ID}" \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh
run_exit=$?
set -e
printf '%s\n' "$run_exit" >"$EVIDENCE/docker-run.exit"
docker inspect "$NAME" --format 'ExitCode={{.State.ExitCode}} Status={{.State.Status}} OOMKilled={{.State.OOMKilled}} Error={{.State.Error}}' >"$EVIDENCE/inspect.txt"
docker inspect "$NAME" --format '{{json .HostConfig.Tmpfs}}' >"$EVIDENCE/tmpfs.json"
docker inspect "$NAME" --format '{{json .HostConfig.CapDrop}}' >"$EVIDENCE/cap-drop.json"
docker inspect "$NAME" --format 'Privileged={{.HostConfig.Privileged}} NetworkMode={{.HostConfig.NetworkMode}} PidMode={{.HostConfig.PidMode}} ReadonlyRootfs={{.HostConfig.ReadonlyRootfs}}' >"$EVIDENCE/hostconfig.txt"
docker inspect "$NAME" --format '{{json .HostConfig.Binds}}' >"$EVIDENCE/binds.json"
docker inspect "$NAME" --format '{{json .Mounts}}' >"$EVIDENCE/mounts.json"
echo "docker_run_exit=${run_exit}"
exit "$run_exit"
