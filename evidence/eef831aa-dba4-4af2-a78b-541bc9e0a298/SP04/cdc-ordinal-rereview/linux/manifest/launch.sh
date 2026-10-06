#!/bin/bash
# Hash this review directory. The inventory rewrite is the last hash.
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a284/SP04/cdc-transaction-review/linux/manifest/launch.sh
# sha256 fadee5941f07add3818032e95dee045165e99aacc661e5f1a96fe310ca8a4913
# Tool origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/tools/sha256_manifest.rs
# sha256 d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3
# Does not prune. Starts no PostgreSQL. No cargo volume. Does not pull.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a298/SP04/cdc-ordinal-rereview
EVIDENCE=${REVIEW}/linux/manifest
OUT=/tmp/oo-a298-eef831aa-dba4-4af2-a78b-541bc9e0a298-manifest
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a298-manifest
REFRESH=oo-a298-manifest-refresh
INVENTORY=oo-a298-manifest-inventory

chmod 755 "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite manifest console.log" >&2
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
mkdir -p "$OUT"
own=$(docker ps -q --filter label=oo.owner=a298 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1 postgres=0\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1 || docker container inspect "$REFRESH" >/dev/null 2>&1 || docker container inspect "$INVENTORY" >/dev/null 2>&1; then
  echo "named manifest container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network none --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 rustc --edition 2024 -D warnings tools/sha256_manifest.rs" > "$EVIDENCE/invocation.txt"
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
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=256m \
  --label oo.project=open-ontology \
  --label oo.owner=a298 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a298 \
  --label oo.task=cdc-ordinal-rereview-manifest \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash -c 'export PATH=/usr/local/cargo/bin:/usr/local/bin:$PATH; rustc --edition 2024 -D warnings /evidence/tools/sha256_manifest.rs -o /usr/local/bin/sha256_manifest && /usr/local/bin/sha256_manifest /evidence /evidence/MANIFEST.sha256' \
  > "$OUT/first-console.txt" 2>&1
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
cp "$OUT/first-console.txt" "$EVIDENCE/console.log"
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
if [ "$run_exit" -ne 0 ]; then
  exit "$run_exit"
fi
own=$(docker ps -q --filter label=oo.owner=a298 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'refresh own=%s global=%s host_cargo=%s cap_global=10 cap_own=1 postgres=0\n' "$own" "$global_builders" "$host_cargo" >> "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: refresh builder capacity" >&2
  exit 3
fi
printf '%s\n' "docker run --name ${REFRESH} --platform linux/arm64 --pull never --network none --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 rustc --edition 2024 -D warnings tools/sha256_manifest.rs" >> "$EVIDENCE/invocation.txt"
set +e
docker run --name "$REFRESH" \
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
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=256m \
  --label oo.project=open-ontology \
  --label oo.owner=a298 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a298 \
  --label oo.task=cdc-ordinal-rereview-manifest-refresh \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash -c 'export PATH=/usr/local/cargo/bin:/usr/local/bin:$PATH; rustc --edition 2024 -D warnings /evidence/tools/sha256_manifest.rs -o /usr/local/bin/sha256_manifest && /usr/local/bin/sha256_manifest /evidence /evidence/MANIFEST.sha256' \
  > "$OUT/refresh-console.txt" 2>&1
refresh_exit=$?
set +e
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$REFRESH")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$REFRESH")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$REFRESH")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$REFRESH")"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$REFRESH")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$REFRESH")"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$REFRESH")"
  echo "ReadonlyRootfs=$(docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' "$REFRESH")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$REFRESH")"
} > "$EVIDENCE/refresh-hostconfig.txt"
printf '%s\n' "$refresh_exit" > "$EVIDENCE/refresh.exit"
cp "$OUT/refresh-console.txt" "$EVIDENCE/refresh-console.log"
docker rm "$REFRESH" > "$EVIDENCE/refresh-removed.txt"
if [ "$refresh_exit" -ne 0 ]; then
  exit "$refresh_exit"
fi
rm -rf "$OUT"
outside_rm=$?
if [ -e "$OUT" ]; then
  outside_present=yes
else
  outside_present=no
fi
{
  echo "manifest_outside=${OUT}"
  echo "manifest_outside_rm_exit=${outside_rm}"
  echo "manifest_outside_present_after=${outside_present}"
} >> "$REVIEW/CLEANUP.txt"
if [ "$outside_rm" -ne 0 ] || [ "$outside_present" != no ]; then
  echo "manifest outside directory was not removed" >&2
  exit 1
fi
own=$(docker ps -q --filter label=oo.owner=a298 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'inventory own=%s global=%s host_cargo=%s cap_global=10 cap_own=1 postgres=0\n' "$own" "$global_builders" "$host_cargo" >> "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: inventory builder capacity" >&2
  exit 3
fi
mkdir -p "$OUT"
printf '%s\n' "docker run --name ${INVENTORY} --platform linux/arm64 --pull never --network none --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 rustc --edition 2024 -D warnings tools/sha256_manifest.rs" >> "$EVIDENCE/invocation.txt"
set +e
docker run --name "$INVENTORY" \
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
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=256m \
  --label oo.project=open-ontology \
  --label oo.owner=a298 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a298 \
  --label oo.task=cdc-ordinal-rereview-manifest-inventory \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash -c 'export PATH=/usr/local/cargo/bin:/usr/local/bin:$PATH; rustc --edition 2024 -D warnings /evidence/tools/sha256_manifest.rs -o /usr/local/bin/sha256_manifest && /usr/local/bin/sha256_manifest /evidence /evidence/MANIFEST.sha256' \
  > "$OUT/inventory-console.txt" 2>&1
inventory_exit=$?
set +e
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$INVENTORY")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$INVENTORY")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$INVENTORY")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$INVENTORY")"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$INVENTORY")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$INVENTORY")"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$INVENTORY")"
  echo "ReadonlyRootfs=$(docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' "$INVENTORY")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$INVENTORY")"
} > "$OUT/inventory-hostconfig.txt"
printf '%s\n' "$inventory_exit" > "$OUT/inventory.exit"
docker rm "$INVENTORY" > "$OUT/inventory-removed.txt"
echo "inventory_exit=${inventory_exit}"
cat "$OUT/inventory-console.txt"
cat "$OUT/inventory-hostconfig.txt"
if [ "$inventory_exit" -ne 0 ]; then
  exit "$inventory_exit"
fi
rm -rf "$OUT"
final_rm=$?
if [ -e "$OUT" ]; then
  echo "inventory outside directory remained" >&2
  exit 1
fi
exit "$final_rm"
