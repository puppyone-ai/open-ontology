#!/bin/bash
# CLI boundary: unchanged description-candidate binary consumes the repaired library.
# The binary source is commit 827e768b, which is not in fixed fbc10b04.
# Origin: a204 preoverlay launch. Memory 6g, one CPU. No PostgreSQL, no pull, no prune.
set -u
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a262/SP07/description-selection-order
EVIDENCE=${REVIEW}/linux/cli
ROOT=/tmp/oo-a262-eef831aa-dba4-4af2-a78b-541bc9e0a262
FIXED=fbc10b04017bd7e0eb922c3c60732bbdf9a7f46a
CLI_COMMIT=827e768b9f94af6134a5734bd16e066ab0d47528
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a262-cli
NET=oo-a262-eef831aa-net
CARGO_VOL=oo-a262-eef831aa-cargo
TARGET_VOL=oo-a262-eef831aa-target

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite cli console.log" >&2
  exit 2
fi
mkdir -p "$REVIEW/cli-src" "$REVIEW/source/blobs"
git -C "$SRC" show "${CLI_COMMIT}:crates/oo-builder/src/bin/description-candidate.rs" > "$REVIEW/source/blobs/description-candidate-bin.rs"
cp "$REVIEW/source/blobs/description-candidate-bin.rs" "$REVIEW/cli-src/description-candidate.rs"
if ! cmp -s "$REVIEW/cli-src/description-candidate.rs" "$SRC/crates/oo-builder/src/bin/description-candidate.rs"; then
  echo "workspace CLI differs from commit ${CLI_COMMIT}" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if cmp -s "$REVIEW/patch/description_candidate.rs" "$REVIEW/source/blobs/description_candidate.rs"; then
  echo "refusing cli: producer still matches the unfixed blob" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/fixed-commit.txt"
printf '%s\n' "$CLI_COMMIT" > "$EVIDENCE/cli-commit.txt"
if [ ! -f "$ROOT/archive/crates/oo-builder/src/description_candidate.rs" ]; then
  echo "fixed archive is missing" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ -f "$ROOT/archive/crates/oo-builder/src/bin/description-candidate.rs" ]; then
  echo "fixed archive already contains the CLI" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
own=$(docker ps -q --filter label=oo.owner=a262 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
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
grep -q 'arch=arm64' "$EVIDENCE/image.txt" || {
  echo "image is not arm64" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
}
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named cli container already exists" >&2
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
  --mount "type=bind,source=${REVIEW}/source/blobs,target=/blobs,readonly" \
  --mount "type=bind,source=${REVIEW}/overlay,target=/overlay,readonly" \
  --mount "type=bind,source=${REVIEW}/patch,target=/patch,readonly" \
  --mount "type=bind,source=${REVIEW}/cli-src,target=/cli,readonly" \
  --mount "type=bind,source=${REVIEW}/tools,target=/tools,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a262 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a262 \
  --label oo.task=description-selection-order-cli \
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
