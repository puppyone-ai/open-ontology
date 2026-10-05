#!/bin/bash
# One Linux arm64 Rust 1.94 builder. Immutable archive of 8a289f0c. No PostgreSQL.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a184/SP06/query-executor-review
SMOKE=${EVIDENCE}/linux/smoke
ROOT=/tmp/oo-a184-eef831aa-dba4-4af2-a78b-541bc9e0a184
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
FIXED=8a289f0c699abb8c707ca45ad45cb036abc3fc05
NAME=oo-a184-smoke
NET=oo-a184-net
CARGO_VOL=oo-a184-cargo
TARGET_VOL=oo-a184-target

chmod 755 "$SMOKE/inside.sh" "$SMOKE/launch.sh" "$SMOKE/extract-libclang.sh"
if [ -f "$SMOKE/console.log" ]; then
  echo "refusing to overwrite smoke console.log" >&2
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  exit 1
fi
mkdir -p "$ROOT/archive" "$EVIDENCE/source/blobs/contracts/gateway" \
  "$EVIDENCE/source/blobs/crates/oo-gateway/src" \
  "$EVIDENCE/source/blobs/crates/oo-gateway/tests"
if [ ! -f "$ROOT/archive/crates/oo-gateway/src/query_executor.rs" ]; then
  git -C "$SRC" archive --format=tar "$FIXED" | tar -xf - -C "$ROOT/archive"
fi
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$SMOKE/archive-tree.txt"
python3 - "$SRC" "$FIXED" "$ROOT/archive" "$SMOKE/archive-verify.txt" <<'PY'
import hashlib, subprocess, sys
repo, commit, archive, out = sys.argv[1:]
listed = subprocess.check_output(["git", "-C", repo, "ls-tree", "-r", commit], text=True)
missing = mismatch = blobs = 0
for line in listed.splitlines():
    meta, path = line.split("\t", 1)
    kind, oid = meta.split()[1], meta.split()[2]
    blobs += 1
    target = __import__("pathlib").Path(archive, path)
    if kind != "blob":
        continue
    if not target.is_file():
        missing += 1
        continue
    digest = subprocess.check_output(["git", "-C", repo, "hash-object", str(target)], text=True).strip()
    if digest != oid:
        mismatch += 1
text = f"blobs={blobs} missing={missing} mismatch={mismatch}\n"
open(out, "w").write(text)
print(text, end="")
if missing or mismatch:
    raise SystemExit(1)
PY
if [ -f "$ROOT/archive/crates/oo-gateway/tests/query_executor_a184_review.rs" ]; then
  echo "immutable archive contains the review overlay" >&2
  exit 2
fi
git -C "$SRC" rev-parse "$FIXED" > "$EVIDENCE/source/COMMIT"
git -C "$SRC" rev-parse "${FIXED}^" > "$EVIDENCE/source/PARENT"
printf '%s\n' 03512f91324ac33e772e9702fe806e8e04037ec4 > "$EVIDENCE/source/FIXED_BASE"
blob() {
  git -C "$SRC" show "${FIXED}:$1" > "$EVIDENCE/source/blobs/$1"
}
blob contracts/gateway/query-executor-v1.md
blob crates/oo-gateway/src/lib.rs
blob crates/oo-gateway/src/query_executor.rs
blob crates/oo-gateway/tests/query_executor_pg_live.rs
(
  cd "$EVIDENCE/source/blobs"
  find . -type f | sort | while read -r file; do
    shasum -a 256 "$file"
  done
) > "$EVIDENCE/source/blobs.sha256"
python3 - "$SRC" "$FIXED" "$EVIDENCE/source/blobs" <<'PY'
import hashlib, subprocess, sys
from pathlib import Path
repo, commit, root = sys.argv[1:]
paths = [
    "contracts/gateway/query-executor-v1.md",
    "crates/oo-gateway/src/lib.rs",
    "crates/oo-gateway/src/query_executor.rs",
    "crates/oo-gateway/tests/query_executor_pg_live.rs",
]
for path in paths:
    expected = subprocess.check_output(["git", "-C", repo, "show", f"{commit}:{path}"])
    got = Path(root, path).read_bytes()
    if got != expected:
        raise SystemExit(f"blob mismatch {path}")
print("source_blobs=4 match=4")
PY

docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a184 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a184 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} {{.Driver}} internal={{.Internal}}' "$NET" > "$SMOKE/network.txt"
ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$SMOKE/host-cargo-procs.txt" || true
host_n=$(grep -c . "$SMOKE/host-cargo-procs.txt" || true)
docker ps --filter label=oo.kind=rust-builder --format '{{.Names}}' > "$SMOKE/builders-before.txt" || true
builder_n=$(grep -c . "$SMOKE/builders-before.txt" || true)
global_n=$((host_n + builder_n))
printf 'host_cargo=%s rust_builders=%s global=%s cap=3\n' "$host_n" "$builder_n" "$global_n" > "$SMOKE/capacity.txt"
if [ "$global_n" -ge 3 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$SMOKE/docker-run.exit"
  exit 3
fi
own=$(docker ps -q --filter label=oo.owner=a184 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$SMOKE/docker-run.exit"
  exit 3
fi
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a184 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a184 \
  --label oo.kind=cargo \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a184 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a184 \
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
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a184 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a184 \
  --label oo.task=query-executor-review-smoke \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$SMOKE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$SMOKE/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME")"
  echo "CapAdd=$(docker inspect -f '{{json .HostConfig.CapAdd}}' "$NAME")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
  echo "PortBindings=$(docker inspect -f '{{json .HostConfig.PortBindings}}' "$NAME")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
} > "$SMOKE/hostconfig.txt"
docker inspect -f '{{json .Mounts}}' "$NAME" > "$SMOKE/mounts.txt"
docker rm "$NAME" > "$SMOKE/container-removed.txt"
printf '%s\n' "$run_exit" > "$SMOKE/smoke.exit"
exit "$run_exit"
