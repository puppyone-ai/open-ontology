#!/bin/bash
# Focused overlay. Starts only after the preoverlay container is gone and its exit is 0.
# Patches a private archive copy. Does not modify the git product tree.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a314/SP06/gateway-token-store-review
PROOF=${EVIDENCE}/linux/overlay
ROOT=/tmp/oo-a314-eef831aa-dba4-4af2-a78b-541bc9e0a314/overlay
FIXTURE=/tmp/oo-a314-eef831aa-dba4-4af2-a78b-541bc9e0a314/builder-fixture
FIXED=99ac19af42059a308ba9c418815619e5a081f407
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a314-overlay
NET=oo-a314-eef831aa-net
CARGO_VOL=oo-a314-eef831aa-cargo
TARGET_VOL=oo-a314-eef831aa-target
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a314
PHASE=${PROOF}/phase

mkdir -p "$PROOF" "$PHASE"
chmod 755 "$PROOF/inside.sh" "$PROOF/launch.sh" "$EVIDENCE/linux/watch-pg.sh"
if [ -f "$PROOF/console.log" ]; then
  echo "refusing to overwrite overlay console.log" >&2
  exit 2
fi
pre_exit=$(cat "$EVIDENCE/linux/preoverlay/docker-run.exit" 2>/dev/null || echo missing)
printf '%s\n' "$pre_exit" > "$PROOF/preoverlay-exit.txt"
if [ "$pre_exit" != "0" ]; then
  echo "preoverlay did not pass" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
if docker container inspect oo-a314-preoverlay >/dev/null 2>&1; then
  echo "preoverlay container still exists" >&2
  printf '%s\n' 3 > "$PROOF/docker-run.exit"
  exit 3
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$PROOF/docker-run.exit"
  exit 1
fi
own=$(docker ps -q --filter label=oo.owner=a314 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 memory=6g cpus=1\n' "$own" "$global_builders" "$host_cargo" > "$PROOF/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$PROOF/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named overlay container already exists" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
if [ "$(docker inspect -f '{{.State.Running}}' oo-a314-pg 2>/dev/null || true)" != "true" ]; then
  echo "postgres is not running" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
test -f "$FIXTURE/ca.crt"

rm -rf "$ROOT/archive"
mkdir -p "$ROOT/archive"
git -C "$SRC" archive --format=tar "$FIXED" Cargo.lock rust-toolchain.toml tools/gateway-token-store | tar -x -C "$ROOT/archive"
cp "$EVIDENCE/overlay/a314_review.rs" "$ROOT/archive/tools/gateway-token-store/src/a314_review.rs"
awk '
  /^mod live;/ { print; print ""; print "#[cfg(test)]"; print "mod a314_review;"; next }
  { print }
' "$ROOT/archive/tools/gateway-token-store/src/lib.rs" > "$ROOT/archive/tools/gateway-token-store/src/lib.rs.a314"
mv "$ROOT/archive/tools/gateway-token-store/src/lib.rs.a314" "$ROOT/archive/tools/gateway-token-store/src/lib.rs"
grep -q 'mod a314_review;' "$ROOT/archive/tools/gateway-token-store/src/lib.rs"
git -C "$SRC" diff -- tools/gateway-token-store/src/lib.rs > "$PROOF/product-lib-diff.txt" || true
diff -u "$SRC/tools/gateway-token-store/src/lib.rs" "$ROOT/archive/tools/gateway-token-store/src/lib.rs" > "$PROOF/archive-lib.diff" || true
git -C "$SRC" rev-parse HEAD > "$PROOF/repo-head.txt"
printf '%s\n' "$FIXED" > "$PROOF/fixed-commit.txt"
{
  echo "origin_a204_launch evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh"
  shasum -a 256 "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh"
  echo "origin_a304_proof_launch evidence/eef831aa-dba4-4af2-a78b-541bc9e0a304/SP06/gateway-token-store/linux/proof/launch.sh"
  shasum -a 256 "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a304/SP06/gateway-token-store/linux/proof/launch.sh"
  echo "changes archive_copy_only one_mod_line focused_a314_review_filter same_volumes"
} > "$PROOF/recipe-origin.txt"
cat > "$PROOF/invocation.txt" <<EOF
run_id=${RUN_ID}
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
network=${NET}
container=${NAME}
privileged=no
network_mode=bridge_not_host
home_mount=no
socket_mount=no
host_port=none
shared_cargo_cache=no
pull=never
phase=overlay
memory=6g
cpus=1
global_cap=10
fixed=${FIXED}
EOF

bash "$EVIDENCE/linux/watch-pg.sh" "$PHASE" > "$PROOF/watcher.log" 2>&1 &
echo $! > "$PROOF/watcher.pid"

set +e
docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --memory 6g \
  --memory-swap 6g \
  --cpus 1 \
  --pids-limit 4096 \
  --network "$NET" \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  -e OO_A314_LIVE=1 \
  -e OO_A314_HOST=oo-a314-pg \
  -e OO_A314_DB=oo_a314 \
  -e OO_A314_FIXTURE=/run/oo-a314/fixture \
  -e OO_A314_EVIDENCE=/evidence/linux/overlay \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${PROOF}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${FIXTURE},target=/run/oo-a314/fixture,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a314 \
  --label oo.session="${RUN_ID}" \
  --label oo.task=gateway-token-store-review-overlay \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$PROOF/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$PROOF/docker-run.exit"
printf '%s\n' "$run_exit" > "$PHASE/builder.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME" 2>/dev/null || echo missing)"
  echo "ReadonlyRootfs=$(docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME" 2>/dev/null || echo missing)"
} > "$PROOF/hostconfig.txt"
docker inspect -f '{{json .HostConfig.Tmpfs}}' "$NAME" > "$PROOF/tmpfs.txt" 2>/dev/null || true
docker inspect -f '{{range .Mounts}}{{.Type}} {{.Destination}} rw={{.RW}}{{println}}{{end}}' "$NAME" > "$PROOF/mounts.txt" 2>/dev/null || true
docker rm "$NAME" > "$PROOF/container-removed.txt" 2>&1 || true
if [ -f "$PROOF/watcher.pid" ]; then
  wait "$(cat "$PROOF/watcher.pid")" || true
fi
exit "$run_exit"
