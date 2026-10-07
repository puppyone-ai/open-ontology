#!/bin/bash
# One builder: compile the release-coordinator binary. Tests already passed.
set -euo pipefail
OWNER=a359
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a359
FIXED=a13b1cc80ed66c07199a41d47b31fd42530f2be9
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE="$REPO/evidence/${RUN_ID}/SP08/release-coordinator/linux/build-bin"
NET=oo-a359-eef831aa-net
CARGO_VOL=oo-a359-eef831aa-cargo
TARGET_VOL=oo-a359-eef831aa-target
BUILD=oo-a359-build
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
mkdir -p "$EVIDENCE"
own=$(docker ps -q --filter label=oo.kind=rust-builder --filter label=oo.owner="$OWNER" | wc -l | tr -d ' ')
global=$(docker ps -q --filter label=oo.kind=rust-builder | wc -l | tr -d ' ')
host_tools=$(ps ax -o command= | awk 'BEGIN{n=0} $1 ~ /(^|\/)(cargo|rustc|rustdoc|rustup)$/ {n++} END{print n+0}')
printf 'own=%s global=%s host_tools=%s\n' "$own" "$global" "$host_tools" | tee "$EVIDENCE/capacity.txt"
if [ "$own" != 0 ] || [ "$global" -ge 10 ] || [ "$host_tools" != 0 ]; then
  echo capacity_refused >&2
  exit 2
fi
set +e
docker run --name "$BUILD" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --memory 6g \
  --memory-swap 6g \
  --cpus 1 \
  --pids-limit 4096 \
  --network "$NET" \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e RUSTUP_HOME=/usr/local/rustup \
  -e PATH=/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${REPO},target=/repo,readonly" \
  --mount "type=bind,source=${REPO}/tools/release-coordinator,target=/repo/tools/release-coordinator" \
  --mount "type=bind,source=/tmp/oo-a359-inside-bin.sh,target=/inside-bin.sh,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=256m \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner="$OWNER" \
  --label oo.session="$RUN_ID" \
  --label oo.fixed="$FIXED" \
  --label oo.kind=rust-builder \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  "$RUST_IMAGE" \
  bash /inside-bin.sh > "$EVIDENCE/console.log" 2>&1
build_exit=$?
set -e
printf '%s\n' "$build_exit" > "$EVIDENCE/docker-run.exit"
docker inspect "$BUILD" --format 'oom={{.State.OOMKilled}} exit={{.State.ExitCode}} privileged={{.HostConfig.Privileged}} memory={{.HostConfig.Memory}} cpus={{.HostConfig.NanoCpus}}' > "$EVIDENCE/hostconfig.txt" || true
docker rm "$BUILD" >/dev/null || true
echo "BIN_EXIT=$build_exit"
exit "$build_exit"
