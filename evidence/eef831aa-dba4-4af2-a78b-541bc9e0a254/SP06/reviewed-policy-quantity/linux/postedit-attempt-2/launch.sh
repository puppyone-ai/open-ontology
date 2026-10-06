#!/bin/bash
# Post-edit focused tests of the integer quantity arm.
# The pre-edit smoke must already have exited 0. This run reuses its
# private network, cargo volume, and target volume. It does not pull,
# does not edit the host manifest, and does not start PostgreSQL.
set -u
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a254/SP06/reviewed-policy-quantity
EVIDENCE=${REVIEW}/linux/postedit
ROOT=/tmp/oo-a254-eef831aa-dba4-4af2-a78b-541bc9e0a254
FIXED=c828e2781fc37eb62249fab54fa0b7fff86f0249
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a254-postedit
NET=oo-a254-eef831aa-net
CARGO_VOL=oo-a254-eef831aa-cargo
TARGET_VOL=oo-a254-eef831aa-target

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite postedit console.log" >&2
  exit 2
fi
if [ ! -f "$REVIEW/linux/smoke/smoke.exit" ] || [ "$(tr -d '[:space:]' < "$REVIEW/linux/smoke/smoke.exit")" != "0" ]; then
  echo "pre-edit smoke has not passed" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
mkdir -p "$EVIDENCE/phases" "$REVIEW/overlay"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
cp "$SRC/crates/oo-gateway/src/reviewed_policy.rs" "$REVIEW/overlay/reviewed_policy.rs"
cp "$SRC/crates/oo-gateway/tests/reviewed_policy_quantity.rs" "$REVIEW/overlay/reviewed_policy_quantity.rs"
git -C "$SRC" hash-object "$REVIEW/overlay/reviewed_policy.rs" "$REVIEW/overlay/reviewed_policy_quantity.rs" \
  > "$EVIDENCE/overlay-blobs.txt"
awk '
  $0 ~ /"int2" \| "int4" \| "int8" => matches!/ {on=1}
  on {print}
  on && $0 ~ /"text" =>/ {exit}
' "$REVIEW/overlay/reviewed_policy.rs" > "$EVIDENCE/overlay-integer-arm.txt"
if ! grep -q 'SemanticType::Quantity' "$EVIDENCE/overlay-integer-arm.txt"; then
  echo "overlay integer arm does not accept quantity" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
rm -rf "$ROOT/work"
mkdir -p "$ROOT/work"
cp -a "$ROOT/archive/." "$ROOT/work/"
if [ ! -f "$ROOT/work/.fixed-base-sha" ]; then
  echo "fixed archive copy is missing" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
cp "$REVIEW/overlay/reviewed_policy.rs" "$ROOT/work/crates/oo-gateway/src/reviewed_policy.rs"
cp "$REVIEW/overlay/reviewed_policy_quantity.rs" "$ROOT/work/crates/oo-gateway/tests/reviewed_policy_quantity.rs"
if [ -f "$ROOT/work/rust-toolchain.toml" ]; then
  mv "$ROOT/work/rust-toolchain.toml" "$ROOT/work/rust-toolchain.toml.aside"
fi
own=$(docker ps -q --filter label=oo.owner=a254 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named postedit container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt" || { echo "image is not arm64" >&2; printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"; exit 1; }
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
  --mount "type=bind,source=${ROOT}/work,target=/work" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${REVIEW},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a254 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a254 \
  --label oo.task=reviewed-policy-quantity-postedit \
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
