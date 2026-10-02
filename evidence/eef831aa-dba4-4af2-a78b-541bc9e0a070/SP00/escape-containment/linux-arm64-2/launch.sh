#!/bin/bash
# Replacement Linux run after attempt 1 rejected the multiarch gcc basename.
# Reuses this task's cargo and target volumes from attempt 1. Does not prune.
set -eu
EVIDENCE=/Users/redux/puppy-one/open-ontology/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a070/SP00/escape-containment/linux-arm64-2
ROOT=/tmp/oo-a070-fixed-7b753993-c087-4c9b-9aa7-5ba409e11708
PRIOR=/Users/redux/puppy-one/open-ontology/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a070/SP00/escape-containment/linux-arm64
RUN_ID=$(uuidgen | tr 'A-Z' 'a-z')
printf '%s\n' "$RUN_ID" >"$EVIDENCE/run-id.txt"
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a070-fixed2-${RUN_ID}
CARGO_VOL=$(tr -d '[:space:]' <"$PRIOR/volume-cargo.txt")
TARGET_VOL=$(tr -d '[:space:]' <"$PRIOR/volume-target.txt")
printf 'reused_from=%s\ncargo_volume=%s\ntarget_volume=%s\n' "$(cat "$PRIOR/run-id.txt")" "$CARGO_VOL" "$TARGET_VOL" >"$EVIDENCE/volumes-reused.txt"

rm -f "$ROOT/snapshot.tar"
tar -cf "$ROOT/snapshot.tar" -C "$ROOT/snapshot" .
shasum -a 256 "$ROOT/snapshot.tar" | tee "$ROOT/snapshot.tar.sha256" | tee "$EVIDENCE/snapshot.tar.sha256"
wc -c "$ROOT/snapshot.tar" | awk '{print $1}' | tee "$ROOT/snapshot.tar.bytes" | tee "$EVIDENCE/snapshot.tar.bytes"
( cd "$ROOT/snapshot" && find . -type f -print | sort | while IFS= read -r f; do shasum -a 256 "$f"; done ) >"$ROOT/snapshot.files.sha256"
cp "$ROOT/snapshot.files.sha256" "$EVIDENCE/snapshot.files.sha256"
shasum -a 256 "$ROOT/snapshot.files.sha256" | awk '{print "files_list",$1}' | tee "$EVIDENCE/snapshot.files.sha256.sha"

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

cat >"$EVIDENCE/invocation.txt" <<EOF
run_id=${RUN_ID}
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
msrv_override=RUSTUP_TOOLCHAIN=1.94.0
container=${NAME}
cargo_volume=${CARGO_VOL}
target_volume=${TARGET_VOL}
volumes=reused-from-attempt-1
snapshot=${ROOT}/snapshot
snapshot_tar_sha256=$(awk '{print $1}' "$EVIDENCE/snapshot.tar.sha256")
archive_commit=$(cat "$PRIOR/../linux-arm64/archive-commit.txt")
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
  --label oo.task=escape-containment-fixed-2 \
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
