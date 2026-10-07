#!/bin/bash
# Author tests, Clippy, and the independent oracle on the immutable archive.
# Origin: a325 postedit launch.sh
#   evidence/eef831aa-dba4-4af2-a78b-541bc9e0a325/SP05/query-result-export/linux/postedit/launch.sh
#   sha256 a14242f8676708c8b309b960db61a6e43e3a3eb88a8027023e2e989e4dda7865
# Changes: owner a331, fixed 15558917, no product overlay, archive mounted
# readonly, review oracle instead of a bash live cmp. 6g, 1 CPU. No PostgreSQL.
# No pull, prune, host cargo, or shared cache.
set -u
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a331/SP05/query-result-export-review
EVIDENCE=${REVIEW}/linux/proof
ROOT=/tmp/oo-a331-eef831aa-dba4-4af2-a78b-541bc9e0a331
FIXED=15558917e2c5be12b74dd1d64fb542b7d05a03af
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a331-proof
NET=oo-a331-eef831aa-net
CARGO_VOL=oo-a331-eef831aa-cargo
TARGET_VOL=oo-a331-eef831aa-target

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite proof console.log" >&2
  exit 2
fi
mkdir -p "$EVIDENCE/phases"
if [ "$(cat "$REVIEW/linux/smoke/smoke.exit" 2>/dev/null || true)" != "0" ]; then
  echo "smoke did not exit 0" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ ! -d "$ROOT/archive" ] || [ ! -f "$ROOT/archive/.fixed-base-sha" ]; then
  echo "fixed archive is absent" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ "$(cat "$ROOT/archive/.fixed-base-sha")" != "$FIXED" ]; then
  echo "fixed archive base mismatch" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ ! -d "$ROOT/archive/tools/query-result-export" ]; then
  echo "archive lost query-result-export" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a331 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a331 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a331 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a331 \
  --label oo.kind=cargo-home \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a331 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a331 \
  --label oo.kind=cargo-target \
  "$TARGET_VOL" >/dev/null
docker volume inspect --format '{{.Name}} {{.Mountpoint}}' "$CARGO_VOL" > "$EVIDENCE/volume-cargo.txt"
docker volume inspect --format '{{.Name}} {{.Mountpoint}}' "$TARGET_VOL" > "$EVIDENCE/volume-target.txt"
own=$(docker ps -q --filter label=oo.owner=a331 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/containers-before.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt" || { echo "image is not arm64" >&2; printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"; exit 1; }
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
  --mount "type=bind,source=${REVIEW}/tools,target=/tools,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${REVIEW},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a331 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a331 \
  --label oo.task=query-result-export-review-proof \
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
