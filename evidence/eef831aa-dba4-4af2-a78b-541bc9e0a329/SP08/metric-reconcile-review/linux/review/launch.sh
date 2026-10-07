#!/bin/bash
# One review builder. Launch shape from a204 overlay-3
# 78c8ef8c179c59317850c1b6a9ae53d3f883b8a1b24c2567fa0d3d26f1997eee
# via a316 preoverlay launch 1d985581ec415312613a7af91d0530e894e757d79294d497fa8580e74b0d5e56.
# Archive is fixed commit 15558917. rust-toolchain.toml is set aside so the
# pinned 1.94.0 image is the compiler. The product tree is not mounted.
set -u
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a329/SP08/metric-reconcile-review
EVIDENCE=${REVIEW}/linux/review
ROOT=/tmp/oo-a329-eef831aa-dba4-4af2-a78b-541bc9e0a329/review
FIXED=15558917e2c5be12b74dd1d64fb542b7d05a03af
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a329-review
NET=oo-a329-eef831aa-net
CARGO_VOL=oo-a329-eef831aa-cargo
TARGET_VOL=oo-a329-eef831aa-target
SEC=/tmp/oo-a329-eef831aa-dba4-4af2-a78b-541bc9e0a329/secrets

mkdir -p "$EVIDENCE"
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh" "$EVIDENCE/extract-libclang.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite review console.log" >&2
  exit 2
fi
rm -rf "$ROOT/archive"
mkdir -p "$ROOT/archive"
git -C "$SRC" archive --format=tar "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates contracts tools/metric-reconcile \
  | tar -x -C "$ROOT/archive"
if [ ! -f "$ROOT/archive/crates/oo-gateway/tests/sql_guard.rs" ]; then
  echo "archive missing sql_guard.rs" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ ! -f "$ROOT/archive/tools/metric-reconcile/src/lib.rs" ]; then
  echo "archive missing metric-reconcile" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if ! diff -rq "$ROOT/archive/tools/metric-reconcile" "$REVIEW/source/blobs/tools/metric-reconcile" > "$EVIDENCE/source-diff.txt"; then
  echo "archive drifted from the saved source blobs" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
mv "$ROOT/archive/rust-toolchain.toml" "$EVIDENCE/rust-toolchain.aside.toml"
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/fixed-tree.txt"
shasum -a 256 "$ROOT/archive/Cargo.lock" | awk '{print $1}' > "$EVIDENCE/lock-before.sha256"
if [ "$(cat "$EVIDENCE/lock-before.sha256")" != "16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a" ]; then
  echo "archive lock hash drifted" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ ! -f "$SEC/login.pass" ] || [ ! -f "$SEC/ca.crt" ]; then
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
  echo "cargo volumes are missing" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
own=$(docker ps -q --filter label=oo.owner=a329 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own=%s global=%s cap_own=1 cap_global=10 memory=6g cpus=1\n' "$own" "$global_builders" > "$EVIDENCE/capacity.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/containers-before.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt" || exit 2
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named review container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
{
  echo "a204-overlay3-launch 78c8ef8c179c59317850c1b6a9ae53d3f883b8a1b24c2567fa0d3d26f1997eee"
  echo "a200-up 96613a159f6c05b4a8d8d1fb79bb2613ae9b3e9a00a7b942196548a00964da45"
  echo "a200-pg-wrap 25712207f986b11e122bd992d5a695881e1e86625e0edf2d0b683b023ebfdd2f"
  echo "a200-extract dc0d980692091bec55d4fbc3eb12a0eda9c5aaa4b9f7d5c6eab995b2a726690d"
  echo "a199-preoverlay-launch bf2a422d751db818cdd4d852d14be00436d4177cb584b85131a4774453413f55"
  echo "a316-preoverlay-launch 1d985581ec415312613a7af91d0530e894e757d79294d497fa8580e74b0d5e56"
  echo "a316-extract 025174865bb7d489b7994ffa8301c0dc56283d872e2b366bef933b69af0aa1de"
  echo "a316-pg-wrap c6726b50d06c73d3eb419affa426623dadfeb6943d242eed94a0e94a45d092fd"
  echo "review-launch $(shasum -a 256 "$EVIDENCE/launch.sh" | awk '{print $1}')"
  echo "review-inside $(shasum -a 256 "$EVIDENCE/inside.sh" | awk '{print $1}')"
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
  -e CARGO_TARGET_DIR=/target/a329-smoke \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE}/extract-libclang.sh,target=/extract-libclang.sh,readonly" \
  --mount "type=bind,source=${REVIEW}/tools,target=/tools,readonly" \
  --mount "type=bind,source=${REVIEW}/oracle,target=/oracle,readonly" \
  --mount "type=bind,source=${REVIEW},target=/evidence" \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a329 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a329 \
  --label oo.task=metric-reconcile-review \
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
