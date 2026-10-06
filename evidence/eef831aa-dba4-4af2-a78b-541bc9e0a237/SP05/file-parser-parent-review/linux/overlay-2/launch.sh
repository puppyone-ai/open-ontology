#!/bin/bash
# Overlay the independent review harness on a fresh archive of bf30b717029d0715e0a1abb7fc8ec02ab7630534.
# Does not overwrite an existing console.log. Does not prune. Does not pull.
set -u
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a237/SP05/file-parser-parent-review
EVIDENCE=${REVIEW}/linux/overlay-2
ROOT=/tmp/oo-a237-eef831aa-dba4-4af2-a78b-541bc9e0a237/overlay-2
FIXED=bf30b717029d0715e0a1abb7fc8ec02ab7630534
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a237-overlay-2
NET=oo-a237-eef831aa-net
CARGO_VOL=oo-a237-eef831aa-cargo
TARGET_VOL=oo-a237-eef831aa-target
A204=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review
A199=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review
A200=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review
A224=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a224/SP05/file-parser-ipc-review

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite overlay-2 console.log" >&2
  exit 2
fi
mkdir -p "$EVIDENCE/oracle"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named overlay-2 container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
own=$(docker ps -q --filter label=oo.owner=a237 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 memory=6g cpus=1\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/containers-before.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
if ! grep -q 'arch=arm64' "$EVIDENCE/image.txt"; then
  echo "pinned image is not arm64" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
rm -rf "$ROOT/archive"
mkdir -p "$ROOT/archive"
git -C "$SRC" archive --format=tar "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates contracts \
  | tar -x -C "$ROOT/archive"
cp "$REVIEW/overlay/file_worker_parent_a237_review.rs" \
  "$ROOT/archive/crates/oo-connectors/tests/file_worker_parent_a237_review.rs"
cp "$REVIEW/overlay/file_worker_parent_a237_review.rs" "$EVIDENCE/expect-review.rs"
if [ ! -f "$ROOT/archive/crates/oo-connectors/src/file_worker_parent.rs" ]; then
  echo "archive missing the parent" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if ! cmp -s "$ROOT/archive/crates/oo-connectors/src/file_worker_parent.rs" "$REVIEW/source/blobs/file_worker_parent.rs" \
  || ! cmp -s "$ROOT/archive/crates/oo-connectors/tests/file_worker_parent.rs" "$REVIEW/source/blobs/file_worker_parent_test.rs" \
  || ! cmp -s "$ROOT/archive/crates/oo-connectors/src/lib.rs" "$REVIEW/source/blobs/lib.rs" \
  || ! cmp -s "$ROOT/archive/contracts/connectors/file-worker-parent-v1.md" "$REVIEW/source/blobs/file-worker-parent-v1.md" \
  || ! cmp -s "$ROOT/archive/crates/oo-connectors/src/bin/oo-file-parser-worker.rs" "$REVIEW/source/blobs/oo-file-parser-worker.rs" \
  || ! cmp -s "$ROOT/archive/crates/oo-connectors/Cargo.toml" "$REVIEW/source/blobs/oo-connectors-Cargo.toml" \
  || ! cmp -s "$ROOT/archive/Cargo.lock" "$REVIEW/source/blobs/Cargo.lock" \
  || ! cmp -s "$ROOT/archive/crates/oo-connectors/src/delimited.rs" "$REVIEW/source/blobs/delimited.rs" \
  || ! cmp -s "$ROOT/archive/crates/oo-connectors/src/jsonl.rs" "$REVIEW/source/blobs/jsonl.rs" \
  || ! cmp -s "$ROOT/archive/crates/oo-connectors/tests/file_worker_parent_a237_review.rs" "$EVIDENCE/expect-review.rs"
then
  echo "archive drifted from the saved blobs" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/fixed-tree.txt"
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a237 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a237 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a237 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a237 \
  --label oo.kind=cargo-home \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a237 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a237 \
  --label oo.kind=cargo-target \
  "$TARGET_VOL" >/dev/null
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
  -e OO_A237_ORACLE_DIR=/evidence/oracle \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${REVIEW}/source/blobs,target=/blobs,readonly" \
  --mount "type=bind,source=${REVIEW}/tools,target=/tools,readonly" \
  --mount "type=bind,source=${REVIEW}/recipes,target=/recipes,readonly" \
  --mount "type=bind,source=${A204},target=/origin/a204,readonly" \
  --mount "type=bind,source=${A199},target=/origin/a199,readonly" \
  --mount "type=bind,source=${A200},target=/origin/a200,readonly" \
  --mount "type=bind,source=${A224},target=/origin/a224,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a237 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a237 \
  --label oo.task=file-parser-parent-review-overlay-2 \
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
