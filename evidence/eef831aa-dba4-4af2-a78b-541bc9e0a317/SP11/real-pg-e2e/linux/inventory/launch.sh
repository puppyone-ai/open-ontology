#!/bin/bash
# One 6 GiB inventory builder. Reuses the a317 cargo volume and private network.
# Origin shape: evidence/.../linux/chain/launch.sh docker flags.
# Change: no archive compile, no secrets mount, walker only.
set -u
OWNER=a317
SESSION=eef831aa-dba4-4af2-a78b-541bc9e0a317
REPO=/Users/redux/puppy-one/open-ontology
REVIEW=${REPO}/evidence/${SESSION}/SP11/real-pg-e2e
EVIDENCE=${REVIEW}/linux/inventory
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NET=oo-a317-eef831aa-net
CARGO_VOL=oo-a317-eef831aa-cargo
NAME=oo-a317-inventory
OUT=/tmp/oo-a317-${SESSION}/inventory-console.log

mkdir -p "$(dirname "$OUT")"
own=$(docker ps -q --filter label=oo.owner=${OWNER} --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
global=$(docker ps -q --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own=%s global=%s cap_global=10\n' "$own" "$global" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global" -ge 10 ]; then
  echo "deferred: rust-builder capacity own=$own global=$global" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if docker ps -a --filter "name=^/${NAME}$" --format '{{.Status}}' | grep -q .; then
  echo "inventory container name already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$EVIDENCE/image.txt"

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
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${REPO}/testing/real-db/pg-e2e,target=/crate,readonly" \
  --mount "type=bind,source=${REVIEW},target=/evidence" \
  --label oo.project=open-ontology \
  --label oo.owner=a317 \
  --label oo.session=${SESSION} \
  --label oo.task=real-pg-e2e-inventory \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$OUT" 2>&1
code=$?
set +e
printf '%s\n' "$code" > "$EVIDENCE/docker-run.exit"
docker inspect "$NAME" --format 'ExitCode={{.State.ExitCode}} OOMKilled={{.State.OOMKilled}} Privileged={{.HostConfig.Privileged}} NetworkMode={{.HostConfig.NetworkMode}} CapDrop={{json .HostConfig.CapDrop}} Memory={{.HostConfig.Memory}} NanoCpus={{.HostConfig.NanoCpus}} ReadonlyRootfs={{.HostConfig.ReadonlyRootfs}} Id={{.Id}}' > "$EVIDENCE/hostconfig.txt"
docker inspect "$NAME" --format '{{json .Mounts}}' > "$EVIDENCE/mounts.txt"
cp "$OUT" "$EVIDENCE/console.log"
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
exit "$code"
