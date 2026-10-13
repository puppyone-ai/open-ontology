#!/bin/bash
# One linux/arm64 Rust 1.94 builder for the a395 product stack.
# Reuses the a387 launch shape. Does not prune. Does not touch foreign containers.
set -u
ROOT=/Users/redux/puppy-one/open-ontology
UUID=eef831aa-dba4-4af2-a78b-541bc9e0a395
# Docker Desktop does not share /tmp. Runtime files stay in this gitignored directory, not $HOME.
STATE=${ROOT}/deploy/product-stack/.run
EVIDENCE=${ROOT}/evidence/${UUID}/SP14/product-stack/linux/build
PARENT=${ROOT}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a387/SP11/session-control-api/linux/impl/launch.sh
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a395-build
NET=oo-a395-eef831aa-net
CARGO_VOL=oo-a395-eef831aa-cargo
TARGET_VOL=oo-a395-eef831aa-target
HERE=${ROOT}/deploy/product-stack/linux

mkdir -p "$EVIDENCE" "$STATE/bin" "$STATE/src"
chmod 755 "$HERE/inside.sh" "$HERE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite build console.log" >&2
  exit 2
fi
{
  echo "product-stack builder recipe origin"
  echo "direct_parent_path=${PARENT}"
  echo "direct_parent_sha256=$(shasum -a 256 "$PARENT" | awk '{print $1}')"
  echo "minimal_changes=uuid a395; private net and cargo volumes; oracle tests only; release closure from git archive HEAD committed blobs including path deps, not the dirty identity worktree; selfbuild from git archive HEAD; no source mount; no generate-lockfile; state under deploy/product-stack/.run"
  echo "image=${IMAGE}"
} > "$EVIDENCE/recipe-origin.txt"

clean_tree() {
  local path="$1"
  if [ -n "$(git -C "$ROOT" status --porcelain -- "$path")" ]; then
    echo "dirty ${path}"
    return 1
  fi
  return 0
}
{
  echo "identity_pin=a6f92da8a0bf872f8a30efb116c2c7b14463505b"
  git -C "$ROOT" diff --stat a6f92da8a0bf872f8a30efb116c2c7b14463505b -- tools/control-identity
  echo "audit_pin=1078d69e8c3cf8dacb145198f1e4123c767a196b"
  git -C "$ROOT" diff --stat 1078d69e8c3cf8dacb145198f1e4123c767a196b -- tools/audit-ingest
  echo "session_pin=53017ed29"
  git -C "$ROOT" diff --stat 53017ed29 -- tools/session-control-api tools/session-store tools/session-transition tools/session-release tools/gateway-token-store
} > "$EVIDENCE/pin-diff.txt"

OO_BUILD_IDENTITY=0
OO_BUILD_AUDIT=0
OO_BUILD_SESSION=0
OO_BUILD_SELFBUILD=0
OO_BUILD_RELEASE=0
if [ ! -s "$EVIDENCE/pin-diff.txt" ] || ! grep -q '^ tools/' "$EVIDENCE/pin-diff.txt"; then
  clean_tree tools/control-identity && OO_BUILD_IDENTITY=1
  clean_tree tools/audit-ingest && OO_BUILD_AUDIT=1
  if clean_tree tools/session-control-api && clean_tree tools/session-store && clean_tree tools/session-transition && clean_tree tools/session-release && clean_tree tools/gateway-token-store; then
    OO_BUILD_SESSION=1
  fi
fi
# Archives use committed blobs. A dirty worktree is not mounted.
# control-identity here is only the release lock's dev-dependency. The identity
# service image stays the a6f92 pin and is not rebuilt.
# oo-builder inherits edition from the real workspace root, so the archive
# includes that Cargo.toml and the workspace crates. It does not edit them.
if git -C "$ROOT" cat-file -e HEAD:tools/selfbuild-control-api/Cargo.lock 2>/dev/null; then
  OO_BUILD_SELFBUILD=1
fi
if [ -x "$STATE/bin/selfbuild-control-api" ]; then
  OO_BUILD_SELFBUILD=0
fi
if clean_tree tools/release-control-api && clean_tree tools/release-coordinator && git -C "$ROOT" cat-file -e HEAD:tools/release-control-api/Cargo.lock 2>/dev/null; then
  OO_BUILD_RELEASE=1
fi
if [ "${OO_ORACLE_ONLY:-0}" = 1 ]; then
  OO_BUILD_IDENTITY=0
  OO_BUILD_AUDIT=0
  OO_BUILD_SESSION=0
  OO_BUILD_SELFBUILD=0
  OO_BUILD_RELEASE=0
fi
rm -rf "$STATE/src"
mkdir -p "$STATE/src"
if [ "$OO_BUILD_SELFBUILD" = 1 ]; then
  git -C "$ROOT" archive HEAD tools/selfbuild-control-api | tar -x -C "$STATE/src"
fi
if [ "$OO_BUILD_RELEASE" = 1 ]; then
  git -C "$ROOT" archive HEAD \
    Cargo.toml \
    crates \
    tools/release-control-api \
    tools/release-coordinator \
    tools/control-identity | tar -x -C "$STATE/src"
fi
{
  echo "OO_BUILD_IDENTITY=${OO_BUILD_IDENTITY}"
  echo "OO_BUILD_AUDIT=${OO_BUILD_AUDIT}"
  echo "OO_BUILD_SESSION=${OO_BUILD_SESSION}"
  echo "OO_BUILD_SELFBUILD=${OO_BUILD_SELFBUILD}"
  echo "OO_BUILD_RELEASE=${OO_BUILD_RELEASE}"
  echo "source=recipe_only"
  if [ "$OO_BUILD_RELEASE" = 1 ]; then
    echo "release=$(git -C "$ROOT" log -1 --format=%H -- tools/release-control-api)"
    echo "release_closure=HEAD committed blobs; workspace Cargo.toml and crates; path deps release-coordinator control-identity oo-builder oo-core oo-jobs; identity service image is not rebuilt"
  else
    echo "release=not_built"
  fi
  if [ -x "$STATE/bin/selfbuild-control-api" ]; then
    echo "selfbuild_binary=present"
  fi
} > "$EVIDENCE/build-selection.txt"

if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a395 \
  --label oo.session="${UUID}" \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a395 \
  --label oo.session="${UUID}" \
  --label oo.kind=cargo-home \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a395 \
  --label oo.session="${UUID}" \
  --label oo.kind=cargo-target \
  "$TARGET_VOL" >/dev/null
own=$(docker ps -q --filter label=oo.owner=a395 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named builder container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ ! -d "$STATE/src" ] || [ ! -d "$STATE/bin" ]; then
  echo "state directories are absent" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
ls "$STATE/src" >/dev/null
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1" > "$EVIDENCE/invocation.txt"

mounts=(
  --mount "type=bind,source=${ROOT}/deploy/product-stack,target=/src/deploy/product-stack,readonly"
  --mount "type=bind,source=${HERE}/inside.sh,target=/inside.sh,readonly"
  --mount "type=bind,source=${EVIDENCE},target=/evidence"
  --mount "type=bind,source=${STATE}/bin,target=/out"
)
if [ "$OO_BUILD_IDENTITY" = 1 ]; then
  mounts+=(--mount "type=bind,source=${ROOT}/tools/control-identity,target=/src/tools/control-identity,readonly")
fi
if [ "$OO_BUILD_AUDIT" = 1 ]; then
  mounts+=(--mount "type=bind,source=${ROOT}/tools/audit-ingest,target=/src/tools/audit-ingest,readonly")
fi
if [ "$OO_BUILD_SESSION" = 1 ]; then
  mounts+=(
    --mount "type=bind,source=${ROOT}/tools/session-control-api,target=/src/tools/session-control-api,readonly"
    --mount "type=bind,source=${ROOT}/tools/session-store,target=/src/tools/session-store,readonly"
    --mount "type=bind,source=${ROOT}/tools/session-transition,target=/src/tools/session-transition,readonly"
    --mount "type=bind,source=${ROOT}/tools/session-release,target=/src/tools/session-release,readonly"
    --mount "type=bind,source=${ROOT}/tools/gateway-token-store,target=/src/tools/gateway-token-store,readonly"
  )
fi
if [ "$OO_BUILD_SELFBUILD" = 1 ] || [ "$OO_BUILD_RELEASE" = 1 ]; then
  mounts+=(--mount "type=bind,source=${STATE}/src,target=/archive,readonly")
fi

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
  -e CARGO_INCREMENTAL=0 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  -e OO_BUILD_IDENTITY="$OO_BUILD_IDENTITY" \
  -e OO_BUILD_AUDIT="$OO_BUILD_AUDIT" \
  -e OO_BUILD_SESSION="$OO_BUILD_SESSION" \
  -e OO_BUILD_SELFBUILD="$OO_BUILD_SELFBUILD" \
  -e OO_BUILD_RELEASE="$OO_BUILD_RELEASE" \
  "${mounts[@]}" \
  --tmpfs /tmp:rw,noexec,nosuid,nodev,size=512m \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a395 \
  --label oo.session="${UUID}" \
  --label oo.task=product-stack \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.host" 2>&1
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
} > "$EVIDENCE/hostconfig.txt"
docker inspect -f '{{json .HostConfig.Tmpfs}}' "$NAME" > "$EVIDENCE/tmpfs.txt" 2>/dev/null || true
docker inspect -f '{{json .Mounts}}' "$NAME" > "$EVIDENCE/mounts.txt" 2>/dev/null || true
docker rm "$NAME" >/dev/null 2>&1 || true
printf '%s\n' removed > "$EVIDENCE/container-removed.txt"
exit "$run_exit"
