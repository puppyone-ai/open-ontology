#!/bin/bash
# Seal integrated commit e213ce37 and run the session-store review.
# Smoke is the author's 23 tests. The overlay is copied only after that smoke.
# Does not generate or update a lock. Does not prune. Does not write the product tree.
set -u
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a281/SP11/session-store-review
EVIDENCE=${REVIEW}/linux/proof
ROOT=/tmp/oo-a281-eef831aa-dba4-4af2-a78b-541bc9e0a281
FIX=${ROOT}/fixture
FIXED=e213ce37140090e843ba98c188009f920962937d
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a281-proof1
NET=oo-a281-eef831aa-net
CARGO_VOL=oo-a281-eef831aa-cargo
TARGET_VOL=oo-a281-eef831aa-target

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite proof console.log" >&2
  exit 2
fi
if [ ! -d "$FIX" ] || [ ! -f "$FIX/endpoint.txt" ]; then
  echo "fresh fixture is absent" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
rm -rf "$ROOT/archive" "$ROOT/work"
mkdir -p "$ROOT/archive" "$ROOT/work"
git -C "$SRC" archive --format=tar "$FIXED" \
  Cargo.lock tools/session-store tools/session-transition \
  | tar -x -C "$ROOT/archive"
if [ ! -f "$ROOT/archive/tools/session-store/src/store.rs" ]; then
  echo "archive missing store.rs" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ ! -f "$ROOT/archive/tools/session-store/Cargo.lock" ]; then
  echo "archive missing the committed session-store lock" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ -f "$ROOT/archive/tools/session-store/tests/a281_session.rs" ]; then
  echo "archive already contains the review overlay" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
for rel in CONTRACT.md Cargo.toml Cargo.lock migrations/oo_session_control_v1.sql src/error.rs src/lib.rs src/main.rs src/sha256.rs src/sql.rs src/store.rs src/tls.rs tests/live.rs; do
  if ! cmp -s "$ROOT/archive/tools/session-store/$rel" "$REVIEW/source/blobs/$rel"; then
    echo "archive drifted from e213ce37 blob $rel" >&2
    printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
    exit 2
  fi
done
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/fixed-tree.txt"
git -C "$SRC" rev-parse "${FIXED}:tools/session-transition" > "$EVIDENCE/reducer-tree.txt"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  echo "private network is absent" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a281 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a281 \
  --label oo.kind=cargo-home \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a281 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a281 \
  --label oo.kind=cargo-target \
  "$TARGET_VOL" >/dev/null
own=$(docker ps -q --filter label=oo.owner=a281 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
own_pg=$(docker ps -q --filter label=oo.owner=a281 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s own_postgres=%s cap_global=10 cap_own=1 postgres=1\n' "$own" "$global_builders" "$host_cargo" "$own_pg" > "$EVIDENCE/capacity.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/containers-before.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ] || [ "$own_pg" -ne 1 ]; then
  echo "deferred: rust builder or postgres capacity" >&2
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
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${ROOT}/work,target=/work" \
  --mount "type=bind,source=${REVIEW}/source/blobs,target=/blobs,readonly" \
  --mount "type=bind,source=${REVIEW}/overlay,target=/overlay,readonly" \
  --mount "type=bind,source=${REVIEW}/oracle,target=/oracle,readonly" \
  --mount "type=bind,source=${REVIEW}/tools/lock_subset.rs,target=/tools/lock_subset.rs,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${FIX},target=/run/oo-a275/fixture,readonly" \
  --mount "type=bind,source=${FIX},target=/run/oo-a281/fixture,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=512m \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a281 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a281 \
  --label oo.task=session-store-review-proof \
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
