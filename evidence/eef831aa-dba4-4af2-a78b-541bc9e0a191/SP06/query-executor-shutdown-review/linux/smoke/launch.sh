#!/bin/bash
# One Linux arm64 Rust 1.94 builder. Immutable archive of 15d23fe9. No PostgreSQL.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a191/SP06/query-executor-shutdown-review
SMOKE=${EVIDENCE}/linux/smoke
ROOT=/tmp/oo-a191-eef831aa-dba4-4af2-a78b-541bc9e0a191
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
FIXED=15d23fe9f2cbc74e3f3278d70d0f56390568126e
NAME=oo-a191-smoke
NET=oo-a191-net
CARGO_VOL=oo-a191-cargo
TARGET_VOL=oo-a191-target

chmod 755 "$SMOKE/inside.sh" "$SMOKE/launch.sh" "$SMOKE/extract-libclang.sh"
if [ -f "$SMOKE/console.log" ]; then
  echo "refusing to overwrite smoke console.log" >&2
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  exit 1
fi
mkdir -p "$ROOT/archive" "$SMOKE/phases"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$SMOKE/archive-tree.txt"
git -C "$SRC" rev-parse "$FIXED" > "$SMOKE/fixed-commit.txt"
if [ ! -f "$ROOT/archive/crates/oo-gateway/src/query_executor.rs" ]; then
  git -C "$SRC" archive --format=tar "$FIXED" | tar -xf - -C "$ROOT/archive"
fi
if [ -f "$ROOT/archive/crates/oo-gateway/src/parser_worker.rs" ]; then
  echo "immutable archive contains parser_worker.rs" >&2
  exit 2
fi
if [ -f "$ROOT/archive/crates/oo-gateway/tests/query_executor_a191_review.rs" ]; then
  echo "review overlay present before the pre-overlay smoke" >&2
  exit 2
fi
git -C "$SRC" ls-tree -r "$FIXED" > "$SMOKE/ls-tree.txt"
awk -F'\t' '$1 ~ / blob / {print $2}' "$SMOKE/ls-tree.txt" > "$SMOKE/archive-paths.txt"
(
  cd "$ROOT/archive"
  git --git-dir="$SRC/.git" hash-object --stdin-paths < "$SMOKE/archive-paths.txt" > "$SMOKE/archive-hashes.txt"
)
python3 - "$SMOKE/ls-tree.txt" "$SMOKE/archive-hashes.txt" "$SMOKE/archive-verify.txt" <<'PY'
import sys
from pathlib import Path
listed, hashed, out = sys.argv[1:]
expect = []
for line in Path(listed).read_text().splitlines():
    meta, _path = line.split("\t", 1)
    parts = meta.split()
    if len(parts) < 3 or parts[1] != "blob":
        continue
    expect.append(parts[2])
got = [line.strip() for line in Path(hashed).read_text().splitlines() if line.strip()]
missing = 0
mismatch = 0
if len(got) != len(expect):
    missing = abs(len(expect) - len(got))
else:
    mismatch = sum(1 for left, right in zip(expect, got) if left != right)
text = f"blobs={len(expect)} hashed={len(got)} missing={missing} mismatch={mismatch}\n"
Path(out).write_text(text)
print(text, end="")
if missing or mismatch:
    raise SystemExit(1)
PY
printf '%s\n' "lib.rs=fixed-archive overlay=none" > "$SMOKE/overlay-absent.txt"

docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a191 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a191 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} {{.Driver}} internal={{.Internal}}' "$NET" > "$SMOKE/network.txt"
ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$SMOKE/host-cargo-procs.txt" || true
host_n=$(grep -c . "$SMOKE/host-cargo-procs.txt" || true)
docker ps --filter label=oo.kind=rust-builder --filter status=running --format '{{.Names}}' > "$SMOKE/builders-before.txt" || true
builder_n=$(grep -c . "$SMOKE/builders-before.txt" || true)
global_n=$((host_n + builder_n))
printf 'host_cargo=%s rust_builders=%s global=%s cap=3\n' "$host_n" "$builder_n" "$global_n" > "$SMOKE/capacity.txt"
if [ "$global_n" -ge 3 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$SMOKE/docker-run.exit"
  exit 3
fi
own=$(docker ps -q --filter label=oo.owner=a191 --filter label=oo.kind=rust-builder --filter status=running)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$SMOKE/docker-run.exit"
  exit 3
fi
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a191 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a191 \
  --label oo.kind=cargo \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a191 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a191 \
  --label oo.kind=target \
  "$TARGET_VOL" >/dev/null
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named smoke container already exists" >&2
  printf '%s\n' 2 > "$SMOKE/docker-run.exit"
  exit 2
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$SMOKE/image.txt"
grep -q 'arch=arm64' "$SMOKE/image.txt"
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL image=pinned-rust-1.94 bash /inside.sh" > "$SMOKE/invocation.txt"
set +e
docker run --name "$NAME" \
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
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=4 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${SMOKE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${SMOKE}/extract-libclang.sh,target=/extract-libclang.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE}/tools/sha256.rs,target=/tools/sha256.rs,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a191 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a191 \
  --label oo.task=query-executor-shutdown-review-smoke \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$SMOKE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$SMOKE/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME" 2>/dev/null || echo missing)"
  echo "CapAdd=$(docker inspect -f '{{json .HostConfig.CapAdd}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME" 2>/dev/null || echo missing)"
  echo "PortBindings=$(docker inspect -f '{{json .HostConfig.PortBindings}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME" 2>/dev/null || echo missing)"
} > "$SMOKE/hostconfig.txt"
docker inspect -f '{{json .Mounts}}' "$NAME" > "$SMOKE/mounts.txt" 2>/dev/null || true
docker rm "$NAME" > "$SMOKE/container-removed.txt" 2>/dev/null || true
printf '%s\n' "$run_exit" > "$SMOKE/smoke.exit"
exit "$run_exit"
