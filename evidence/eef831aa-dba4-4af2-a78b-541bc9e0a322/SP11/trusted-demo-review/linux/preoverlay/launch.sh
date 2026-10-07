#!/bin/bash
# Pre-overlay smoke. Origin: a204 linux/preoverlay/launch.sh.
# 6 GiB, 1 CPU, cap 10. Does not pull, prune, or mount the Docker socket or host home.
# Removes its own network and volumes only after the smoke exits 0.
set -eu
set -o pipefail
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a322/SP11/trusted-demo-review
EVIDENCE=${REVIEW}/linux/preoverlay
ROOT=/tmp/oo-a322-eef831aa-dba4-4af2-a78b-541bc9e0a322-pre
CLI_FIXED=25dfc9022780c3deff12f37b7ad7ea4ed1fb6480
REVIEW_FIXED=895864a5bf64e50cef84075d20fd7b0c9e41c264
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a322-preoverlay
NET=oo-a322-eef831aa-pre-net
CARGO_VOL=oo-a322-eef831aa-precargo
TARGET_VOL=oo-a322-eef831aa-pretarget
DEPLOY=${REVIEW}/source/deploy/trusted-demo
OWNER=a322
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a322

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite preoverlay console.log" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ ! -f "$DEPLOY/src/drive.rs" ] || [ ! -d "$REVIEW/recipes" ]; then
  echo "frozen product or recipes are absent" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
rm -rf "$ROOT/archive"
mkdir -p "$ROOT/archive" "$EVIDENCE/phases"
git -C "$SRC" archive --format=tar "$CLI_FIXED" | tar -x -C "$ROOT/archive"
printf '%s\n' "$CLI_FIXED" > "$ROOT/archive/.fixed-base-sha"
if [ ! -f "$ROOT/archive/tools/pg-onboarding/src/outcome.rs" ]; then
  echo "archive missing pg-onboarding" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ -e "$ROOT/archive/deploy/trusted-demo/src/drive.rs" ]; then
  echo "cli archive unexpectedly contains the demo driver" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
printf '%s\n' "$CLI_FIXED" > "$EVIDENCE/cli-commit.txt"
printf '%s\n' "$REVIEW_FIXED" > "$EVIDENCE/review-commit.txt"
git -C "$SRC" rev-parse "${CLI_FIXED}^{tree}" > "$EVIDENCE/cli-tree.txt"
git -C "$SRC" rev-parse "${REVIEW_FIXED}^{tree}" > "$EVIDENCE/review-tree.txt"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner="$OWNER" \
  --label oo.session="$RUN_ID" \
  --label oo.fixed="$REVIEW_FIXED" \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner="$OWNER" \
  --label oo.session="$RUN_ID" \
  --label oo.fixed="$REVIEW_FIXED" \
  --label oo.kind=cargo-home \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner="$OWNER" \
  --label oo.session="$RUN_ID" \
  --label oo.fixed="$REVIEW_FIXED" \
  --label oo.kind=cargo-target \
  "$TARGET_VOL" >/dev/null
own=$(docker ps -q --filter label=oo.owner="$OWNER" --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity own=$own global=$global_builders host_cargo=$host_cargo" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
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
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${DEPLOY},target=/deploy,readonly" \
  --mount "type=bind,source=${REVIEW}/recipes,target=/recipes,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner="$OWNER" \
  --label oo.session="$RUN_ID" \
  --label oo.fixed="$REVIEW_FIXED" \
  --label oo.task=trusted-demo-review-preoverlay \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set +e
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME" 2>/dev/null || echo missing)"
  echo "PidsLimit=$(docker inspect -f '{{.HostConfig.PidsLimit}}' "$NAME" 2>/dev/null || echo missing)"
  echo "ReadonlyRootfs=$(docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME" 2>/dev/null || echo missing)"
} > "$EVIDENCE/hostconfig.txt"
docker inspect -f '{{json .HostConfig.Tmpfs}}' "$NAME" > "$EVIDENCE/tmpfs.txt" 2>/dev/null || true
docker inspect -f '{{range .Mounts}}{{.Type}} {{.Source}} {{.Destination}} rw={{.RW}}{{println}}{{end}}' "$NAME" > "$EVIDENCE/mounts.txt" 2>/dev/null || true
docker rm "$NAME" > "$EVIDENCE/container-removed.txt" 2>/dev/null || true
if grep -F 'docker.sock' "$EVIDENCE/mounts.txt" >/dev/null 2>&1; then
  echo "docker socket is mounted" >&2
  run_exit=1
fi
mem=$(awk -F= '/^Memory=/{print $2}' "$EVIDENCE/hostconfig.txt")
cpus=$(awk -F= '/^NanoCpus=/{print $2}' "$EVIDENCE/hostconfig.txt")
priv=$(awk -F= '/^Privileged=/{print $2}' "$EVIDENCE/hostconfig.txt")
if [ "$mem" != "6442450944" ] || [ "$cpus" != "1000000000" ] || [ "$priv" != "false" ]; then
  echo "builder limits were not 6GiB, 1 CPU, and unprivileged" >&2
  run_exit=1
fi
if [ "$run_exit" -eq 0 ]; then
  docker volume rm "$CARGO_VOL" > "$EVIDENCE/volume-cargo-removed.txt" || run_exit=1
  docker volume rm "$TARGET_VOL" > "$EVIDENCE/volume-target-removed.txt" || run_exit=1
  docker network rm "$NET" > "$EVIDENCE/network-removed.txt" || run_exit=1
  rm -rf "$ROOT/archive"
  if [ "$run_exit" -eq 0 ]; then
    printf '%s\n' "removed_preoverlay_volumes_and_network" > "$EVIDENCE/cleanup-note.txt"
  else
    printf '%s\n' "preoverlay_cleanup_failed" > "$EVIDENCE/cleanup-note.txt"
  fi
else
  printf '%s\n' "kept_preoverlay_volumes_after_failure" > "$EVIDENCE/cleanup-note.txt"
fi
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
exit "$run_exit"
