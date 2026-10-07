#!/bin/bash
# Post-edit build of the owned crate copied off the fixed archive's package.
# Origin: a331 smoke launch.sh
#   sha256 9e25ff357ac4006df8555930b736055faf460417dfc3a8c9fd493a2d46c7fa8e
# and a325 postedit launch.sh
#   sha256 a14242f8676708c8b309b960db61a6e43e3a3eb88a8027023e2e989e4dda7865.
# Changes: owner a333, this session, the source mount is only
# tools/query-result-export, tests and Clippy run, no PostgreSQL.
# 6g, 1 CPU. Reuses the smoke cargo and target volumes. No pull or prune.
set -u
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a333/SP05/query-result-export-decimal
EVIDENCE=${REVIEW}/linux/postedit
ROOT=/tmp/oo-a333-eef831aa-dba4-4af2-a78b-541bc9e0a333
FIXED=63566b60aabdf29a29bfe55c4866edf3c4081cfd
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a333-post
NET=oo-a333-eef831aa-net
CARGO_VOL=oo-a333-eef831aa-cargo
TARGET_VOL=oo-a333-eef831aa-target

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite postedit console.log" >&2
  exit 2
fi
mkdir -p "$EVIDENCE/phases"
if [ ! -d "$ROOT/archive/tools/query-result-export" ]; then
  echo "smoke archive is missing" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
rm -rf "$ROOT/post"
mkdir -p "$ROOT/post"
cp -a "$SRC/tools/query-result-export/." "$ROOT/post/"
find "$ROOT/post" -name target -type d -prune -exec rm -rf {} +
git -C "$SRC" diff --name-only "$FIXED" -- tools/query-result-export > "$EVIDENCE/overlay-names.txt"
{
  echo "fixed=$FIXED"
  while IFS= read -r path; do
    [ -n "$path" ] || continue
    base=$(git -C "$SRC" rev-parse "$FIXED:$path")
    now=$(git -C "$SRC" hash-object "$SRC/$path")
    printf '%s base=%s now=%s\n' "$path" "$base" "$now"
  done < "$EVIDENCE/overlay-names.txt"
  for keep in Cargo.toml Cargo.lock; do
    base=$(git -C "$SRC" rev-parse "$FIXED:tools/query-result-export/$keep")
    now=$(git -C "$SRC" hash-object "$ROOT/post/$keep")
    printf 'unchanged %s base=%s now=%s\n' "$keep" "$base" "$now"
    if [ "$base" != "$now" ]; then
      echo "dependency file changed: $keep" >&2
      printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
      exit 2
    fi
  done
} > "$EVIDENCE/overlay-git.txt"
own=$(docker ps -q --filter label=oo.owner=a333 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
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
  echo "named postedit container already exists" >&2
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
  --mount "type=bind,source=${ROOT}/post,target=/src,readonly" \
  --mount "type=bind,source=${REVIEW}/tools,target=/tools,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${REVIEW},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a333 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a333 \
  --label oo.task=query-result-export-decimal-postedit \
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
