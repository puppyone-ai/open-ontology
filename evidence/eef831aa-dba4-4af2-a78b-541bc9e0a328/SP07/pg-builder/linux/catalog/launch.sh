#!/bin/bash
# One Linux arm64 Rust 1.94 builder. Catalog and profile against the private PostgreSQL.
# Origin: evidence/.../linux/focused-4/launch.sh, which records a200, a204, a199, and a263.
# Change: build oo-pipeline catalog-pg, read oo-a328-pg, then wait for one comment replacement.
# The model endpoint is not called from this container.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a328/SP07/pg-builder
DIR=${EVIDENCE}/linux/catalog
ROOT=/tmp/oo-a328-eef831aa-dba4-4af2-a78b-541bc9e0a328/catalog
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
FIXED=8c2005d5d3cc48eecfb314aa278f7b1d4f00bef8
NAME=oo-a328-catalog
NET=oo-a328-eef831aa-net
CARGO_VOL=oo-a328-eef831aa-cargo
TARGET_VOL=oo-a328-eef831aa-target
SEC=/tmp/oo-a328-eef831aa-dba4-4af2-a78b-541bc9e0a328/secrets

chmod 755 "$DIR/inside.sh" "$DIR/launch.sh"
mkdir -p "$DIR/phases" "$EVIDENCE/live/flags"
if [ -f "$DIR/console.log" ]; then
  echo "refusing to overwrite catalog console.log" >&2
  printf '%s\n' 2 > "$DIR/docker-run.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$DIR/docker-run.exit"
  exit 1
fi
if [ ! -f "$SEC/reader.pass" ] || [ ! -f "$SEC/ca.crt" ]; then
  echo "private postgres secrets are absent" >&2
  printf '%s\n' 2 > "$DIR/docker-run.exit"
  exit 2
fi

git -C "$SRC" rev-parse "$FIXED" > "$DIR/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$DIR/archive-tree.txt"
git -C "$SRC" rev-parse HEAD > "$DIR/repo-head.txt"
rm -rf "$ROOT/archive"
mkdir -p "$ROOT/archive"
git -C "$SRC" archive --format=tar "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates \
  tools/description-pack tools/description-review \
  | tar -x -C "$ROOT/archive"
rm -rf "$ROOT/archive/tools/pg-builder" "$ROOT/archive/testing/real-db/pg-builder"
mkdir -p "$ROOT/archive/tools" "$ROOT/archive/testing/real-db"
cp -a "$SRC/tools/pg-builder" "$ROOT/archive/tools/pg-builder"
cp -a "$SRC/testing/real-db/pg-builder" "$ROOT/archive/testing/real-db/pg-builder"
rm -rf "$ROOT/archive/tools/pg-builder/target" "$ROOT/archive/testing/real-db/pg-builder/target"

ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$DIR/host-cargo-procs.txt" || true
host_n=$(grep -c . "$DIR/host-cargo-procs.txt" || true)
{
  docker ps --filter status=running --filter label=oo.kind=rust-builder --format '{{.Names}}'
  docker ps --filter status=running --filter ancestor=sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5 --format '{{.Names}}'
} | awk 'NF && !seen[$0]++' > "$DIR/builders-before.txt" || true
builder_n=$(grep -c . "$DIR/builders-before.txt" || true)
global_n=$((host_n + builder_n))
printf 'host_cargo=%s rust_builders=%s global=%s cap=10\n' "$host_n" "$builder_n" "$global_n" > "$DIR/capacity.txt"
if [ "$global_n" -ge 10 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$DIR/docker-run.exit"
  exit 3
fi
own=$(docker ps -aq --filter status=running --filter label=oo.owner=a328 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$DIR/docker-run.exit"
  exit 3
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$DIR/image.txt"
grep -q 'arch=arm64' "$DIR/image.txt"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named catalog container already exists" >&2
  printf '%s\n' 2 > "$DIR/docker-run.exit"
  exit 2
fi

cat > "$DIR/invocation.txt" <<EOF
run_id=eef831aa-dba4-4af2-a78b-541bc9e0a328
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
pull=never
phase=catalog
memory=6g
cpus=1
fixed=${FIXED}
postgres=oo-a328-pg
llm=not_called
EOF

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
  --pids-limit 256 \
  --network "$NET" \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target/pipeline \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${DIR}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=256m \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a328 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a328 \
  --label oo.task=pg-builder-catalog \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$DIR/console.log" 2>&1 &
run_pid=$!
commented=0
while kill -0 "$run_pid" 2>/dev/null; do
  if [ "$commented" -eq 0 ] && [ -f "$EVIDENCE/live/flags/pass1.ready" ]; then
    docker cp "$EVIDENCE/pg/inside-comment.sh" oo-a328-pg:/tmp/a328-inside-comment.sh
    docker exec oo-a328-pg bash /tmp/a328-inside-comment.sh > "$EVIDENCE/pg/comment-replace.txt" 2>&1
    comment_exit=$?
    printf '%s\n' "$comment_exit" > "$EVIDENCE/pg/comment-replace.exit"
    if [ "$comment_exit" -eq 0 ]; then
      date -u +%Y-%m-%dT%H:%M:%SZ > "$EVIDENCE/live/flags/comment.ready"
      commented=1
    fi
  fi
  sleep 5
done
wait "$run_pid"
run_status=$?
set -e
printf '%s\n' "$run_status" > "$DIR/docker-run.exit"
docker inspect --format 'oom={{.State.OOMKilled}} status={{.State.Status}} exit={{.State.ExitCode}} mem={{.HostConfig.Memory}} cpu={{.HostConfig.NanoCpus}} priv={{.HostConfig.Privileged}} net={{.HostConfig.NetworkMode}} pidslimit={{.HostConfig.PidsLimit}}' "$NAME" > "$DIR/hostconfig.txt" || true
docker rm "$NAME" >/dev/null || true
printf '%s\n' removed > "$DIR/container-removed.txt"
exit "$run_status"
