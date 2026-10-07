#!/bin/bash
# Independent review. Fixed archive plus one review test. No product copy-back.
# Reuses the preoverlay network and volumes. One 6GiB/1CPU builder. No PostgreSQL.
set -eu
set -o pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a342/SP05/filehost-retention-complete-review/linux/review
PRE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a342/SP05/filehost-retention-complete-review/linux/preoverlay
ROOT=/tmp/oo-a342-eef831aa-dba4-4af2-a78b-541bc9e0a342
FIXED=cd8f754c1b4db22fc962370c4b29bb846b0fccc3
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a342-review
NET=oo-a342-eef831aa-net
CARGO_VOL=oo-a342-eef831aa-cargo
TARGET_VOL=oo-a342-eef831aa-target
SESSION=eef831aa-dba4-4af2-a78b-541bc9e0a342
BASE=${ROOT}/fixed-archive
SNAP=${ROOT}/review-snap

mkdir -p "$EVIDENCE/phases" "$ROOT"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite review console.log" >&2
  exit 2
fi
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ ! -f "$BASE/Cargo.lock" ] || [ -f "$BASE/rust-toolchain.toml" ]; then
  echo "fixed archive is missing; not creating a second archive" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi

{
  echo "a342 review launch"
  echo "fixed ${FIXED}"
  echo "product def1f7eea53794449d3975b1987d700727cb7d60"
  echo "change copy review_independent.rs into the snap only; no product copy-back; reuse preoverlay network and volumes"
  echo "a342 preoverlay launch"
  echo "path evidence/eef831aa-dba4-4af2-a78b-541bc9e0a342/SP05/filehost-retention-complete-review/linux/preoverlay/launch.sh"
  shasum -a 256 "$PRE/launch.sh"
  echo "a336 review launch"
  echo "path evidence/eef831aa-dba4-4af2-a78b-541bc9e0a336/SP05/filehost-retention-review/linux/review/launch.sh"
  shasum -a 256 "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a336/SP05/filehost-retention-review/linux/review/launch.sh"
  echo "change uuid, fixed commit cd8f754, product def1f7ee, fresh review_independent.rs"
  echo "a323 proof-3 launch"
  echo "path evidence/eef831aa-dba4-4af2-a78b-541bc9e0a323/SP05/filehost-retention-delete/linux/proof-3/launch.sh"
  shasum -a 256 "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a323/SP05/filehost-retention-delete/linux/proof-3/launch.sh"
  echo "a204 launch"
  echo "commit b2b3adabd05de94723d8ed13b33be0144a0de1f9"
  echo "path evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh"
  shasum -a 256 "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh"
  echo "a200 launch"
  echo "commit a587031c719a484830d78e493605f48e2056cae4"
  echo "path evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/proof/launch.sh"
  shasum -a 256 "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/proof/launch.sh"
  echo "a199 launch"
  echo "commit 5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9"
  echo "path evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/linux/overlay/launch.sh"
  shasum -a 256 "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/linux/overlay/launch.sh"
  echo "lock oracle"
  shasum -a 256 "$EVIDENCE/lock_from_baseline.rs"
  echo "change copied byte-for-byte; no edit"
} > "$EVIDENCE/recipe-origin.txt"

rm -rf "$SNAP"
mkdir -p "$SNAP"
cp -a "$BASE/." "$SNAP/"
cp "$EVIDENCE/review_independent.rs" "$SNAP/tools/artifact-retention/tests/review_independent.rs"
printf '%s\n' "$FIXED" > "$EVIDENCE/fixed-commit.txt"
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
(
  cd "$SNAP"
  shasum -a 256 \
    Cargo.lock \
    crates/oo-control/src/file_hosting.rs \
    tools/artifact-retention/Cargo.lock \
    tools/artifact-retention/tests/review_independent.rs \
    > "$EVIDENCE/archive-sha256.txt"
)
shasum -a 256 "$EVIDENCE/review_independent.rs" > "$EVIDENCE/overlay-sha256-before.txt"

if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
docker network inspect "$NET" >/dev/null
docker network inspect --format 'name={{.Name}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
docker volume inspect "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null
docker volume inspect "$CARGO_VOL" --format 'name={{.Name}}' > "$EVIDENCE/volume-cargo.txt"
docker volume inspect "$TARGET_VOL" --format 'name={{.Name}}' > "$EVIDENCE/volume-target.txt"

own=$(docker ps -q --filter label=oo.owner=a342 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" || $1=="clippy-driver" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1 postgres=0 memory=6g cpus=1\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/containers-before.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named review container already exists" >&2
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
  --mount "type=bind,source=${SNAP},target=/src" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=512m \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a342 \
  --label oo.session="$SESSION" \
  --label oo.task=filehost-retention-complete-review \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME" 2>/dev/null || echo missing)"
  echo "ReadonlyRootfs=$(docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME" 2>/dev/null || echo missing)"
} > "$EVIDENCE/hostconfig.txt"
docker inspect -f '{{json .HostConfig.Tmpfs}}' "$NAME" > "$EVIDENCE/tmpfs.txt" 2>/dev/null || true
docker inspect -f '{{range .Mounts}}{{.Type}} {{.Source}} {{.Destination}} rw={{.RW}}{{println}}{{end}}' "$NAME" > "$EVIDENCE/mounts.txt" 2>/dev/null || true
docker rm "$NAME" > "$EVIDENCE/container-removed.txt" 2>/dev/null || true
exit "$run_exit"
