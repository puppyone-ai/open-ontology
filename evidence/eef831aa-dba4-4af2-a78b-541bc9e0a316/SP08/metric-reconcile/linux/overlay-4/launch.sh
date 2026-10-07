#!/bin/bash
# Overlay build for metric-reconcile. Fixed archive 99ac19af stays read-only.
# Launch shape from this task's preoverlay launch, which records
# a204 overlay-3 launch 78c8ef8c179c59317850c1b6a9ae53d3f883b8a1b24c2567fa0d3d26f1997eee.
# Lock generation shape from a212 impl-5 inside (offline, then one online retry).
# No prune. No host cargo. No root lock write.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a316/SP08/metric-reconcile/linux/overlay-4
ROOT=/tmp/oo-a316-eef831aa-dba4-4af2-a78b-541bc9e0a316/preoverlay
FIXED=99ac19af42059a308ba9c418815619e5a081f407
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a316-overlay-4
NET=oo-a316-eef831aa-net
CARGO_VOL=oo-a316-eef831aa-cargo
TARGET_VOL=oo-a316-eef831aa-target
SEC=/tmp/oo-a316-eef831aa-dba4-4af2-a78b-541bc9e0a316/secrets
CRATE=${SRC}/tools/metric-reconcile
TOOLS=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a316/SP08/metric-reconcile/tools
PRE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a316/SP08/metric-reconcile/linux/preoverlay

mkdir -p "$EVIDENCE"
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite overlay console.log" >&2
  exit 2
fi
if [ ! -f "$ROOT/archive/crates/oo-gateway/src/sql_guard.rs" ]; then
  echo "fixed archive is missing; preoverlay must remain" >&2
  exit 2
fi
if [ -e "$ROOT/archive/tools/metric-reconcile/src" ]; then
  echo "archive tools tree must stay an empty mount parent" >&2
  exit 2
fi
mkdir -p "$ROOT/archive/tools/metric-reconcile" "$ROOT/archive/examples/schema"
git -C "$SRC" show "${FIXED}:examples/schema/metrics-finance.yaml" > "$ROOT/archive/examples/schema/metrics-finance.yaml"
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/fixed-tree.txt"
shasum -a 256 "$ROOT/archive/Cargo.lock" | awk '{print $1}' > "$EVIDENCE/lock-before.sha256"
shasum -a 256 "$ROOT/archive/examples/schema/metrics-finance.yaml" > "$EVIDENCE/finance-yaml.sha256"
if [ "$(cat "$EVIDENCE/lock-before.sha256")" != "16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a" ]; then
  echo "archive lock hash drifted from the preoverlay baseline" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ ! -d "$SEC" ] || [ ! -f "$SEC/login.pass" ] || [ ! -f "$SEC/ca.crt" ]; then
  echo "private postgres secrets are not present" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  echo "task network is missing" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
if ! docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || ! docker volume inspect "$TARGET_VOL" >/dev/null 2>&1; then
  echo "preoverlay cargo volumes are missing" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
own=$(docker ps -q --filter label=oo.owner=a316 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own=%s global=%s cap_own=1 cap_global=10 memory=6g cpus=1\n' "$own" "$global_builders" > "$EVIDENCE/capacity.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/containers-before.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named overlay container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
{
  echo "preoverlay-launch $(shasum -a 256 "$PRE/launch.sh" | awk '{print $1}')"
  echo "preoverlay-inside $(shasum -a 256 "$PRE/inside.sh" | awk '{print $1}')"
  echo "a204-recorded 78c8ef8c179c59317850c1b6a9ae53d3f883b8a1b24c2567fa0d3d26f1997eee"
  echo "a200-up 96613a159f6c05b4a8d8d1fb79bb2613ae9b3e9a00a7b942196548a00964da45"
  echo "a200-pg-wrap 25712207f986b11e122bd992d5a695881e1e86625e0edf2d0b683b023ebfdd2f"
  echo "overlay-launch $(shasum -a 256 "$EVIDENCE/launch.sh" | awk '{print $1}')"
  echo "overlay-inside $(shasum -a 256 "$EVIDENCE/inside.sh" | awk '{print $1}')"
} > "$EVIDENCE/origin-hashes.txt"
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
  -e CARGO_TARGET_DIR=/target/a316-metric \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${CRATE},target=/src/tools/metric-reconcile" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${PRE}/extract-libclang.sh,target=/extract-libclang.sh,readonly" \
  --mount "type=bind,source=${TOOLS},target=/tools,readonly" \
  --mount "type=bind,source=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a316/SP08/metric-reconcile,target=/evidence" \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a316 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a316 \
  --label oo.task=metric-reconcile-overlay \
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
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME")"
} > "$EVIDENCE/hostconfig.txt"
docker rm "$NAME" >/dev/null
printf '%s\n' "$NAME" > "$EVIDENCE/container-removed.txt"
shasum -a 256 "$ROOT/archive/Cargo.lock" | awk '{print $1}' > "$EVIDENCE/lock-after.sha256"
exit "$run_exit"
