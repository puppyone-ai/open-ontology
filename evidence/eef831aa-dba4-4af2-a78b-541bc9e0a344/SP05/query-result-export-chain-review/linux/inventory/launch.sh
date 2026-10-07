#!/bin/bash
# Inventory of this review directory. Origin: a333 chain inventory launch.sh
# SHA-256 23763f36f5429302919efa90c3ac8306b7bc0257e1df75d1275a9cc377de300a
# Changes: owner a344. Hashes this review only. Console stays in /tmp until
# after the manifest. 6GiB, 1 CPU, network none. No pull or prune.
set -u
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a344/SP05/query-result-export-chain-review
EVIDENCE=${REVIEW}/linux/inventory
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a344-chain-inventory
CARGO_VOL=oo-a344-eef831aa-hashcargo
CONSOLE=/tmp/oo-a344-chain-inventory-console.log

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
mkdir -p "$EVIDENCE"
if [ -f "$REVIEW/SHA256SUMS" ]; then
  echo "refusing to overwrite SHA256SUMS" >&2
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a344 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a344 \
  --label oo.kind=cargo-home \
  "$CARGO_VOL" >/dev/null
own=$(docker ps -q --filter status=running --filter label=oo.owner=a344 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global=$(docker ps -q --filter status=running --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own=%s global=%s cap_own=1 cap_global=10\n' "$own" "$global" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global" -ge 10 ]; then
  echo "deferred: rust builder capacity own=$own global=$global" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "inventory container name already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
printf '%s\n' "image=pinned-rust-1.94 memory=6g cpus=1 network=none" > "$EVIDENCE/invocation.txt"
sync
sleep 2
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
  --pids-limit 256 \
  --network none \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  --mount "type=bind,source=${REVIEW}/tools/sha256.rs,target=/tools/sha256.rs,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${REVIEW},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=256m \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --label oo.project=open-ontology \
  --label oo.owner=a344 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a344 \
  --label oo.task=query-result-export-chain-review-inventory \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$CONSOLE" 2>&1
run_exit=$?
set +e
cp "$CONSOLE" "$EVIDENCE/console.log"
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME" 2>/dev/null || echo missing)"
} > "$EVIDENCE/hostconfig.txt"
docker inspect -f '{{json .HostConfig.Tmpfs}}' "$NAME" > "$EVIDENCE/tmpfs.txt" 2>/dev/null || true
docker inspect -f '{{range .Mounts}}{{.Type}} {{.Destination}} rw={{.RW}}{{println}}{{end}}' "$NAME" > "$EVIDENCE/mounts.txt" 2>/dev/null || true
docker rm "$NAME" > "$EVIDENCE/container-removed.txt" 2>&1 || true
docker volume rm "$CARGO_VOL" > "$EVIDENCE/volume-removed.txt" 2>&1 || true
rm -f "$CONSOLE"
exit "$run_exit"
