#!/bin/bash
# Pre-overlay of the frozen author suite. One 6GiB/1CPU builder. One private Postgres.
# Does not overwrite an existing console.log. Does not pull. Does not prune.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a314/SP06/gateway-token-store-review
PROOF=${EVIDENCE}/linux/preoverlay
ROOT=/tmp/oo-a314-eef831aa-dba4-4af2-a78b-541bc9e0a314/preoverlay
FIXTURE=/tmp/oo-a314-eef831aa-dba4-4af2-a78b-541bc9e0a314/builder-fixture
FIXED=99ac19af42059a308ba9c418815619e5a081f407
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a314-preoverlay
NET=oo-a314-eef831aa-net
CARGO_VOL=oo-a314-eef831aa-cargo
TARGET_VOL=oo-a314-eef831aa-target
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a314
PHASE=${EVIDENCE}/linux/proof/phase

mkdir -p "$PROOF" "$EVIDENCE/linux/proof"
chmod 755 "$PROOF/inside.sh" "$PROOF/launch.sh" "$EVIDENCE/linux/watch-pg.sh" "$EVIDENCE/pg/up.sh"
if [ -f "$PROOF/console.log" ]; then
  echo "refusing to overwrite preoverlay console.log" >&2
  exit 2
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
docker ps --filter label=oo.kind=rust-builder --format '{{.Names}} {{.Label "oo.owner"}} {{.Status}}' > "$PROOF/containers-before.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$PROOF/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named preoverlay container already exists" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi

if [ "$(docker inspect -f '{{.State.Running}}' oo-a314-pg 2>/dev/null || true)" = "true" ] \
  && docker exec -u postgres oo-a314-pg pg_isready -U oo_fixture_admin -d oo_a304 >/dev/null 2>&1 \
  && test -f "$FIXTURE/ca.crt"; then
  echo "reused running oo-a314-pg" > "$PROOF/pg-up.exit"
  printf '%s\n' 0 >> "$PROOF/pg-up.exit"
else
  bash "$EVIDENCE/pg/up.sh"
  pg_exit=$?
  printf '%s\n' "$pg_exit" > "$PROOF/pg-up.exit"
  if [ "$pg_exit" -ne 0 ]; then
    echo "postgres did not start" >&2
    printf '%s\n' "$pg_exit" > "$PROOF/docker-run.exit"
    exit "$pg_exit"
  fi
fi
test -f "$FIXTURE/ca.crt"
test -f "$FIXTURE/wrong.crt"
test -f "$FIXTURE/admin.pass"
test -f "$FIXTURE/token-admin.pass"
test -f "$FIXTURE/token-runtime.pass"

rm -rf "$ROOT/archive"
mkdir -p "$ROOT/archive" "$PHASE"
git -C "$SRC" archive --format=tar "$FIXED" Cargo.lock rust-toolchain.toml tools/gateway-token-store | tar -x -C "$ROOT/archive"
test ! -e "$ROOT/archive/tools/gateway-token-store/src/a314_review.rs"
git -C "$SRC" rev-parse HEAD > "$PROOF/repo-head.txt"
printf '%s\n' "$FIXED" > "$PROOF/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$PROOF/fixed-tree.txt"
{
  echo "origin_a204_launch evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh"
  shasum -a 256 "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh"
  echo "origin_a200_launch evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/proof/launch.sh"
  shasum -a 256 "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/proof/launch.sh"
  echo "origin_a200_pg evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/pg/up.sh"
  shasum -a 256 "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/pg/up.sh"
  echo "origin_a304_proof_launch evidence/eef831aa-dba4-4af2-a78b-541bc9e0a304/SP06/gateway-token-store/linux/proof/launch.sh"
  shasum -a 256 "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a304/SP06/gateway-token-store/linux/proof/launch.sh"
  echo "origin_a199_manifest evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/tools/sha256_manifest.rs"
  shasum -a 256 "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/tools/sha256_manifest.rs"
  echo "changes owner_a314 fixed_99ac19af archive_includes_crate_no_worktree_copy overlay_absent subset_must_be_125 old_root_lock_only memory_6g cpus_1"
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
phase=preoverlay
memory=6g
cpus=1
global_cap=10
fixed=${FIXED}
archive=${ROOT}/archive
fixture=builder-fixture-without-private-keys
EOF

bash "$EVIDENCE/linux/watch-pg.sh" "$PHASE" > "$PROOF/watcher.log" 2>&1 &
echo $! > "$PROOF/watcher.pid"

docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a314 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=cargo-home \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a314 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=target \
  "$TARGET_VOL" >/dev/null

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
  -e OO_A304_LIVE=1 \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${PROOF}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${FIXTURE},target=/run/oo-a304/fixture,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a314 \
  --label oo.session="${RUN_ID}" \
  --label oo.task=gateway-token-store-review-preoverlay \
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
