#!/bin/bash
# One owned Linux arm64 Rust 1.94 builder for the baseline archive.
# No pull, prune, host network, socket, or foreign cleanup.
set -eu
RUN_ID="$(uuidgen | tr '[:upper:]' '[:lower:]')"
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
COMMIT=e84efd436dbc105b6a80df62464e1af7534e5af7
ROOT=/tmp/oo-a104-eef831aa-dba4-4af2-a78b-541bc9e0a104
EVIDENCE=/Users/redux/puppy-one/open-ontology/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a104/SP04/segment-preflight/linux/baseline
REPO=/Users/redux/puppy-one/open-ontology
NAME=oo-a104-msrv-${RUN_ID}
NET=oo-a104-${RUN_ID}-net
CARGO_VOL=oo-a104-eef831aa-dba4-4af2-a78b-541bc9e0a104-cargo
TARGET_VOL=oo-a104-eef831aa-dba4-4af2-a78b-541bc9e0a104-target
SESSION=eef831aa-dba4-4af2-a78b-541bc9e0a104

mkdir -p "$EVIDENCE" "$ROOT"
chmod +x "$EVIDENCE/inside.sh"
printf '%s\n' "$NAME" > "$EVIDENCE/container-name.txt"
printf '%s\n' "$NET" > "$EVIDENCE/network-name.txt"
printf '%s\n' "$CARGO_VOL" > "$EVIDENCE/cargo-volume.txt"
printf '%s\n' "$TARGET_VOL" > "$EVIDENCE/target-volume.txt"
printf '%s\n' "$RUN_ID" > "$EVIDENCE/run-id.txt"

{
  echo "observed_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "host_toolchain_comm:"
  ps -ax -o pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|clippy-driver)$/ {print}' || true
  echo "rust_builder_labels:"
  docker ps --filter label=oo.kind=rust-builder --format '{{.Names}} {{.Status}} {{.Label "oo.owner"}}' || true
} > "$EVIDENCE/builders-before.txt"

running=$(docker ps --filter label=oo.project=open-ontology --filter label=oo.kind=rust-builder --format '{{.Names}}')
count=$(printf '%s\n' "$running" | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -ax -o comm= | awk '$1 ~ /^(cargo|rustc|rustdoc|clippy-driver)$/' | wc -l | tr -d ' ')
printf 'builder_containers=%s host_cargo=%s\n' "$count" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$((count + host_cargo))" -ge 2 ]; then
  echo "deferred: rust builders already at global max" >&2
  exit 3
fi
own=$(docker ps --filter label=oo.owner=a104 --filter label=oo.kind=rust-builder --format '{{.Names}}')
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  exit 3
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  exit 1
fi

rm -rf "$ROOT/baseline-snapshot" "$ROOT/baseline-archive.tar"
mkdir -p "$ROOT/baseline-snapshot"
git -C "$REPO" archive --format=tar "$COMMIT" \
  Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates \
  contracts/pipeline/target-fence-receipts \
  > "$ROOT/baseline-archive.tar"
tar -x -f "$ROOT/baseline-archive.tar" -C "$ROOT/baseline-snapshot"
shasum -a 256 "$ROOT/baseline-archive.tar" | awk '{print $1}' > "$EVIDENCE/archive.tar.sha256"
git -C "$REPO" rev-parse "$COMMIT" > "$EVIDENCE/source-commit.txt"
git -C "$REPO" rev-parse "$COMMIT^{tree}" > "$EVIDENCE/source-tree.txt"
git -C "$REPO" rev-parse "$COMMIT^" > "$EVIDENCE/source-parent.txt"
git -C "$REPO" rev-parse HEAD > "$EVIDENCE/workspace-head-at-archive.txt"

files=0
links=0
mismatch=0
: > "$EVIDENCE/archive-mismatch.txt"
while IFS= read -r path; do
  [ -n "$path" ] || continue
  links=$((links + 1))
  echo "symlink $path" >> "$EVIDENCE/archive-mismatch.txt"
done < <(find "$ROOT/baseline-snapshot" -type l | sort)
while IFS= read -r path; do
  [ -n "$path" ] || continue
  rel=${path#"$ROOT/baseline-snapshot/"}
  files=$((files + 1))
  got=$(git hash-object "$path")
  exp=$(git -C "$REPO" rev-parse "$COMMIT:$rel")
  if [ "$got" != "$exp" ]; then
    mismatch=$((mismatch + 1))
    echo "mismatch $rel" >> "$EVIDENCE/archive-mismatch.txt"
  fi
done < <(find "$ROOT/baseline-snapshot" -type f | sort)
printf 'files=%s\nlinks=%s\nmismatched=%s\n' "$files" "$links" "$mismatch" > "$EVIDENCE/archive-verify.txt"
if [ "$mismatch" -ne 0 ] || [ "$links" -ne 0 ]; then
  echo "archive verification failed" >&2
  exit 2
fi

cat > "$EVIDENCE/invocation.txt" <<EOF
run_id=${RUN_ID}
source_commit=${COMMIT}
source_tree=$(cat "$EVIDENCE/source-tree.txt")
source_parent=$(cat "$EVIDENCE/source-parent.txt")
workspace_head=$(cat "$EVIDENCE/workspace-head-at-archive.txt")
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
msrv_override=RUSTUP_TOOLCHAIN=1.94.0
network=${NET}
container=${NAME}
cargo_volume=${CARGO_VOL}
target_volume=${TARGET_VOL}
snapshot=${ROOT}/baseline-snapshot
archive_tar=${ROOT}/baseline-archive.tar
archive_sha256=$(cat "$EVIDENCE/archive.tar.sha256")
snapshot_method=git archive of ${COMMIT} paths only
privileged=no
network_mode=bridge_not_host
home_mount=no
socket_mount=no
host_port=none
shared_cargo_cache=no
pull=never
cap_drop=ALL
cap_add=none
init=yes
no_new_privileges=yes
command=cargo fetch --manifest-path Cargo.toml --locked; cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline; cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-pipeline --all-targets -- -D warnings
EOF

docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a104 \
  --label oo.session="${SESSION}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=rust-builder \
  "$NET"
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a104 \
  --label oo.session="${SESSION}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=rust-builder \
  "$CARGO_VOL" >/dev/null
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a104 \
  --label oo.session="${SESSION}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=rust-builder \
  "$TARGET_VOL" >/dev/null

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
  --cpus 4 \
  --pids-limit 4096 \
  --network "$NET" \
  --workdir /src \
  --tmpfs /tmp:rw,nosuid,nodev,size=1g \
  -e CARGO_TERM_COLOR=never \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=4 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/tmp \
  --mount "type=bind,source=${ROOT}/baseline-snapshot,target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --label oo.project=open-ontology \
  --label oo.owner=a104 \
  --label oo.session="${SESSION}" \
  --label oo.task=segment-preflight-baseline \
  --label oo.run_id="${RUN_ID}" \
  --label oo.scope=sp04-segment-preflight \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set -e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
docker inspect "$NAME" --format 'ExitCode={{.State.ExitCode}} Status={{.State.Status}} OOMKilled={{.State.OOMKilled}} Error={{.State.Error}}' > "$EVIDENCE/inspect.txt"
docker inspect "$NAME" --format 'privileged={{.HostConfig.Privileged}} capdrop={{json .HostConfig.CapDrop}} capadd={{json .HostConfig.CapAdd}} ports={{json .HostConfig.PortBindings}} net={{.HostConfig.NetworkMode}} tmpfs={{json .HostConfig.Tmpfs}} security={{json .HostConfig.SecurityOpt}} readonly_root={{.HostConfig.ReadonlyRootfs}}' > "$EVIDENCE/hostconfig.txt"
docker inspect "$NAME" --format '{{json .HostConfig.Binds}}{{json .HostConfig.Mounts}}' > "$EVIDENCE/mounts.txt"

set +e
docker rm "$NAME" > "$EVIDENCE/cleanup-rm.txt" 2>&1
rm_exit=$?
docker network rm "$NET" > "$EVIDENCE/cleanup-network.txt" 2>&1
net_exit=$?
set -e
printf 'retained_for_final_smoke\n' > "$EVIDENCE/cleanup-cargo-volume.txt"
printf 'retained_for_final_smoke\n' > "$EVIDENCE/cleanup-target-volume.txt"
cargo_exit=0
target_exit=0
printf 'rm_exit=%s\nnetwork_exit=%s\ncargo_volume_exit=%s\ntarget_volume_exit=%s\ncargo_volume_retained=%s\ntarget_volume_retained=%s\n' "$rm_exit" "$net_exit" "$cargo_exit" "$target_exit" "$CARGO_VOL" "$TARGET_VOL" > "$EVIDENCE/cleanup.txt"
echo "docker_run_exit=${run_exit}"
exit "$run_exit"
