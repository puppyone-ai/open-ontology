#!/bin/bash
# Pre-overlay smoke of the repaired pg-onboarding tree. No review test is added.
# Immutable archive 9951cda7. 6g, 1 CPU. Does not pull or prune. No PostgreSQL.
set -u
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a274/SP11/pg-onboarding-review
EVIDENCE=${REVIEW}/linux/preoverlay-attempt-2
ROOT=/tmp/oo-a274-eef831aa-dba4-4af2-a78b-541bc9e0a274/preoverlay
FIXED=9951cda7f128e30f1f6ce02e409d542d20a9e4fb
TOOL=50d4e705a2bb4ba014919ef1e071da065cf8e9b7
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a274-preoverlay-2
NET=oo-a274-eef831aa-net
CARGO_VOL=oo-a274-eef831aa-cargo
TARGET_VOL=oo-a274-eef831aa-target

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite preoverlay console.log" >&2
  exit 2
fi
mkdir -p "$EVIDENCE/phases" "$ROOT"
rm -rf "$ROOT/archive"
mkdir -p "$ROOT/archive"
git -C "$SRC" archive --format=tar "$FIXED" tools/pg-onboarding | tar -x -C "$ROOT/archive"
if [ ! -f "$ROOT/archive/tools/pg-onboarding/src/outcome.rs" ]; then
  echo "archive missing outcome.rs" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ -f "$ROOT/archive/tools/pg-onboarding/tests/pg_onboarding_a274_review.rs" ]; then
  echo "archive already contains a review overlay" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
blob_ok=0
for pair in \
  src/outcome.rs:outcome.rs \
  src/json_doc.rs:json_doc.rs \
  src/spawn.rs:spawn.rs \
  src/apply.rs:apply.rs \
  src/inspect.rs:inspect.rs \
  tests/failure_truth.rs:failure_truth.rs \
  tests/orchestration.rs:orchestration.rs \
  tests/live_chain.rs:live_chain.rs \
  Cargo.toml:Cargo.toml \
  Cargo.lock:Cargo.lock \
  CONTRACT.md:CONTRACT.md
do
  rel=${pair%%:*}
  name=${pair##*:}
  if ! cmp -s "$ROOT/archive/tools/pg-onboarding/$rel" "$REVIEW/source/blobs/$name"; then
    echo "archive drifted from blob $rel" >&2
    blob_ok=1
  fi
done
if [ "$blob_ok" -ne 0 ]; then
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if ! grep -q 'changes_may_have_been_applied' "$ROOT/archive/tools/pg-onboarding/src/outcome.rs"; then
  echo "archive is missing the repaired failure state" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
got_tree=$(git -C "$SRC" rev-parse "$FIXED:tools/pg-onboarding")
want_tree=$(git -C "$SRC" rev-parse "$TOOL:tools/pg-onboarding")
printf 'archive_tool=%s tool_commit_tree=%s\n' "$got_tree" "$want_tree" > "$EVIDENCE/tool-tree.txt"
if [ "$got_tree" != "$want_tree" ]; then
  echo "archive tool tree is not the repaired commit" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/fixed-commit.txt"
printf '%s\n' "$TOOL" > "$EVIDENCE/tool-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/fixed-tree.txt"
printf '%s\n' "overlay=absent" > "$EVIDENCE/overlay-absent.txt"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a274 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a274 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format 'id={{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a274 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a274 \
  --label oo.kind=cargo-home \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a274 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a274 \
  --label oo.kind=cargo-target \
  "$TARGET_VOL" >/dev/null
docker volume inspect --format '{{.Name}} {{.Mountpoint}}' "$CARGO_VOL" > "$EVIDENCE/volume-cargo.txt"
docker volume inspect --format '{{.Name}} {{.Mountpoint}}' "$TARGET_VOL" > "$EVIDENCE/volume-target.txt"
docker ps --filter label=oo.kind=rust-builder --format '{{.ID}} {{.Names}} {{.Label "oo.owner"}}' > "$EVIDENCE/builders-before.txt"
own=$(docker ps -q --filter label=oo.owner=a274 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt" || { echo "image is not arm64" >&2; printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"; exit 1; }
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named preoverlay container already exists" >&2
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
  -e CARGO_TARGET_DIR=/target/onboarding \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${ROOT}/archive/tools/pg-onboarding,target=/onboarding,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${REVIEW}/tools/sha256_manifest.rs,target=/tools/sha256_manifest.rs,readonly" \
  --mount "type=bind,source=${REVIEW},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a274 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a274 \
  --label oo.task=pg-onboarding-review-preoverlay-2 \
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
