#!/bin/bash
# Compile and run the std-only manifest tool. Stdout goes to the path in $1,
# which must sit outside this evidence directory. Does not prune.
# Origin: a199 linux/manifest/launch.sh (docker-run.exit 0).
# Not linux/manifest-4: that run exited 126, /build was noexec.
set -u
if [ "$#" -ne 1 ]; then
  echo "usage: launch.sh <console-outside-evidence-dir>" >&2
  exit 2
fi
CONSOLE=$1
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a304/SP06/gateway-token-store
EVIDENCE=${REVIEW}/linux/inventory
CRATE=${SRC}/tools/gateway-token-store
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a304-inventory
ORIGIN=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/linux/manifest/launch.sh
case "$CONSOLE" in
  "$REVIEW"/*)
    echo "console path is inside the evidence directory" >&2
    exit 2
    ;;
esac
if [ -e "$EVIDENCE/docker-run.exit" ]; then
  echo "docker-run.exit exists; not overwriting" >&2
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  exit 1
fi
own=$(docker ps -q --filter label=oo.owner=a304 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 memory=6g cpus=1\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named inventory container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network none --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 rustc -o /usr/local/bin/sha256_manifest tools/sha256_manifest.rs" > "$EVIDENCE/invocation.txt"
{
  echo "origin_a199_manifest_launch evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/linux/manifest/launch.sh"
  shasum -a 256 "$ORIGIN"
  echo "changes memory_8g_to_6g cpus_4_to_1 cap_global_6_to_10 owner_a304 crate_recheck_before_walk binary_on_rootfs"
} > "$EVIDENCE/recipe-origin.txt"
mkdir -p "$(dirname "$CONSOLE")"
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
  --network none \
  -e HOME=/tmp \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  --mount "type=bind,source=${REVIEW},target=/evidence" \
  --mount "type=bind,source=${CRATE},target=/crate,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --label oo.project=open-ontology \
  --label oo.owner=a304 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a304 \
  --label oo.task=gateway-token-store-inventory \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash -c 'set -eu
export PATH=/usr/local/cargo/bin:/usr/local/bin:$PATH
rustc --edition 2024 -D warnings /evidence/tools/sha256_manifest.rs -o /usr/local/bin/sha256_manifest
/usr/local/bin/sha256_manifest /crate /tmp/crate-now.sha256
if ! cmp -s /tmp/crate-now.sha256 /evidence/linux/proof/crate-manifest.sha256; then
  cp /tmp/crate-now.sha256 /evidence/linux/inventory/crate-manifest-recheck.sha256
  echo PHASE crate_manifest_mismatch
  exit 5
fi
cp /tmp/crate-now.sha256 /evidence/linux/inventory/crate-manifest-recheck.sha256
/usr/local/bin/sha256_manifest /evidence /evidence/MANIFEST.sha256
' \
  > "$CONSOLE" 2>&1
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
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
cp "$CONSOLE" "$EVIDENCE/console.log"
rm -f "$CONSOLE"
rmdir "$(dirname "$CONSOLE")" 2>/dev/null || true
if [ -e "$CONSOLE" ]; then
  printf '%s\n' "present" > "$EVIDENCE/tmp-removed.txt"
else
  printf '%s\n' "absent" > "$EVIDENCE/tmp-removed.txt"
fi
exit "$run_exit"
