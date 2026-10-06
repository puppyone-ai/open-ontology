#!/bin/bash
# Independent review of cdc-transaction. One 6GiB builder, no PostgreSQL.
# Smoke is the author 27 tests plus the three oo-core position tests.
# The review overlay is copied only after that smoke, inside the container.
# Does not overwrite an existing console.log. Does not prune. Does not pull.
set -u
set -o pipefail
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a284/SP04/cdc-transaction-review
EVIDENCE=${REVIEW}/linux/proof
ROOT=/tmp/oo-a284-eef831aa-dba4-4af2-a78b-541bc9e0a284/proof
FIXED=c551afea55d5a9164760878a5d6a76835b869875
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a284-proof
NET=oo-a284-eef831aa-net
CARGO_VOL=oo-a284-eef831aa-cargo
TARGET_VOL=oo-a284-eef831aa-target

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite proof console.log" >&2
  exit 2
fi
rm -rf "$ROOT/archive" "$ROOT/core" "$ROOT/work"
mkdir -p "$ROOT/archive" "$ROOT/core" "$ROOT/work"
git -C "$SRC" archive --format=tar "$FIXED" tools/cdc-transaction | tar -x -C "$ROOT/archive"
git -C "$SRC" archive --format=tar "$FIXED" Cargo.toml Cargo.lock crates | tar -x -C "$ROOT/core"
find "$ROOT/archive" "$ROOT/core" -name '._*' -delete
find "$ROOT/archive" "$ROOT/core" -name '.DS_Store' -delete
if [ -f "$ROOT/core/rust-toolchain.toml" ]; then
  echo "archive unexpectedly contains rust-toolchain.toml" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ ! -f "$ROOT/archive/tools/cdc-transaction/src/lib.rs" ]; then
  echo "archive missing cdc-transaction" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ ! -f "$ROOT/core/crates/oo-core/src/lib.rs" ] || [ ! -f "$ROOT/core/Cargo.lock" ]; then
  echo "archive missing oo-core" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ -e "$ROOT/core/crates/oo-core/tests/cdc_d01_position.rs" ]; then
  echo "fixed archive already has the position overlay" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ -f "$ROOT/archive/tools/cdc-transaction/tests/a284_independent.rs" ]; then
  echo "archive already contains the review overlay" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
while IFS= read -r rel; do
  if ! cmp -s "$ROOT/archive/tools/cdc-transaction/$rel" "$REVIEW/source/blobs/$rel"; then
    echo "archive drifted from sealed blob $rel" >&2
    printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
    exit 2
  fi
done < <(cd "$REVIEW/source/blobs" && find . -type f | sed 's|^\./||' | sort)
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/fixed-tree.txt"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a284 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a284 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a284 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a284 \
  --label oo.kind=cargo-home \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a284 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a284 \
  --label oo.kind=cargo-target \
  "$TARGET_VOL" >/dev/null
own=$(docker ps -q --filter label=oo.owner=a284 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1 postgres=0\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/containers-before.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named proof container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 bash /inside.sh" > "$EVIDENCE/invocation.txt"
set +e
docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory 6g \
  --memory-swap 6g \
  --cpus 1 \
  --pids-limit 4096 \
  --network "$NET" \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target/tool \
  -e CARGO_BUILD_JOBS=1 \
  -e CARGO_INCREMENTAL=0 \
  -e CARGO_PROFILE_DEV_DEBUG=0 \
  -e CARGO_PROFILE_TEST_DEBUG=0 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${ROOT}/archive/tools/cdc-transaction,target=/tool,readonly" \
  --mount "type=bind,source=${ROOT}/core,target=/core,readonly" \
  --mount "type=bind,source=${ROOT}/work,target=/work" \
  --mount "type=bind,source=${REVIEW}/source/blobs,target=/blobs,readonly" \
  --mount "type=bind,source=${REVIEW}/overlay,target=/overlay,readonly" \
  --mount "type=bind,source=${REVIEW}/oracle,target=/oracle,readonly" \
  --mount "type=bind,source=${REVIEW}/tools/lock_subset.rs,target=/tools/lock_subset.rs,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=256m \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a284 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a284 \
  --label oo.task=cdc-transaction-review-proof \
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
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME")"
  echo "ReadonlyRootfs=$(docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' "$NAME")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
} > "$EVIDENCE/hostconfig.txt"
docker inspect -f '{{json .HostConfig.Tmpfs}}' "$NAME" > "$EVIDENCE/tmpfs.txt"
docker inspect -f '{{range .Mounts}}{{.Type}} {{.Source}} {{.Destination}} rw={{.RW}}{{println}}{{end}}' "$NAME" > "$EVIDENCE/mounts.txt"
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
exit "$run_exit"
