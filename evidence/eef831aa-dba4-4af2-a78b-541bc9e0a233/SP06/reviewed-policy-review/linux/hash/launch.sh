#!/bin/bash
# Compile tools/sha256.rs in the pinned image and hash this evidence tree.
# Writes hostconfig before the digest walk. Removes only oo-a233-hash. Does not prune.
set -u
EVIDENCE=/Users/redux/puppy-one/open-ontology/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a233/SP06/reviewed-policy-review
HASH=${EVIDENCE}/linux/hash
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a233-hash

mkdir -p "$HASH"
if [ -f "$HASH/console.log" ]; then
  echo "refusing to overwrite hash console.log" >&2
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  exit 1
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named hash container already exists" >&2
  exit 2
fi

ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$HASH/host-cargo-procs.txt" || true
host_n=$(grep -c . "$HASH/host-cargo-procs.txt" || true)
{
  docker ps --filter status=running --filter label=oo.kind=rust-builder --format '{{.Names}}'
  docker ps --filter status=running --filter ancestor=sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5 --format '{{.Names}}'
} | awk 'NF && !seen[$0]++' > "$HASH/builders-before.txt" || true
builder_n=$(grep -c . "$HASH/builders-before.txt" || true)
global_n=$((host_n + builder_n))
printf 'host_cargo=%s rust_builders=%s global=%s cap=10\n' "$host_n" "$builder_n" "$global_n" > "$HASH/capacity.txt"
if [ "$global_n" -ge 10 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  exit 3
fi

cat > "$HASH/invocation.txt" <<EOF
image=${IMAGE}
container=${NAME}
network=none
memory=6g
cpus=1
pull=never
privileged=no
purpose=sha256-inventory
EOF

docker run -d --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --memory 6g \
  --memory-swap 6g \
  --cpus 1 \
  --pids-limit 512 \
  --network none \
  -e HOME=/tmp \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,size=256m \
  --label oo.project=open-ontology \
  --label oo.owner=a233 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a233 \
  --label oo.task=reviewed-policy-review-hash \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  sleep 180 >/dev/null

{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME")"
  echo "CapAdd=$(docker inspect -f '{{json .HostConfig.CapAdd}}' "$NAME")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
} > "$HASH/hostconfig.txt"

set +e
docker exec "$NAME" bash -c 'rustc --edition 2021 -D warnings /evidence/tools/sha256.rs -o /usr/local/bin/sha256 && /usr/local/bin/sha256 dir /evidence/SHA256SUMS /evidence' > "$HASH/console.log" 2>&1
exec_exit=$?
set +e
printf '%s\n' "$exec_exit" > "$HASH/docker-exec.exit"
if [ "$exec_exit" -ne 0 ]; then
  docker rm -f "$NAME" > "$HASH/container-removed.txt"
  exit "$exec_exit"
fi
printf '%s\n' "$NAME" > "$HASH/container-removed.txt"
docker exec "$NAME" /usr/local/bin/sha256 dir /evidence/SHA256SUMS /evidence > /tmp/oo-a233-hash-rewrite.log 2>&1
rewrite_exit=$?
printf '%s\n' "$rewrite_exit" > /tmp/oo-a233-hash-rewrite.exit
if [ "$rewrite_exit" -ne 0 ]; then
  docker rm -f "$NAME" > /tmp/oo-a233-hash-rm.txt
  exit "$rewrite_exit"
fi
docker rm -f "$NAME" > /tmp/oo-a233-hash-rm.txt
cmp -s "$HASH/container-removed.txt" /tmp/oo-a233-hash-rm.txt
exit $?
