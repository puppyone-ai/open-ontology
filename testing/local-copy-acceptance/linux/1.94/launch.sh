#!/bin/bash
# One private Linux arm64 Rust 1.94 builder for local-copy-acceptance.
# No pull. No prune. Does not stop other containers. Does not start PostgreSQL.
# Origin shape: testing/minimal-loop-acceptance/linux/focused/launch.sh
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a409/SP03/local-copy-acceptance/linux/1.94
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a409-linux194
NET=oo-a409-eef831aa-net
CARGO_VOL=oo-a409-eef831aa-cargo
TARGET_VOL=oo-a409-eef831aa-target
mkdir -p "$EVIDENCE/phases"
chmod 755 "$SRC/testing/local-copy-acceptance/linux/1.94/inside.sh" "$SRC/testing/local-copy-acceptance/linux/1.94/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite linux 1.94 console.log" >&2
  exit 2
fi
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
{
  echo "host_rustc_raw_kept=evidence/eef831aa-dba4-4af2-a78b-541bc9e0a409/SP03/local-copy-acceptance/raw/toolchain.txt"
  echo "host_toolchain=rustc 1.98.1 Homebrew"
  echo "host_fullreport_before_linux=6a4696f5b9a8891462893e2e3fea6e1c8d617c59b8edea5732ffcdf3e5298ee4"
  echo "host_measurement=in_memory_judgment_not_linux_1.94"
} > "$EVIDENCE/host-deviation.txt"
{
  echo "copy_rs_sha256=$(shasum -a 256 "$SRC/tools/local-pg-mirror/src/copy.rs" | awk '{print $1}')"
  echo "contract_sha256=$(shasum -a 256 "$SRC/tools/local-pg-mirror/CONTRACT.md" | awk '{print $1}')"
  echo "lib_rs_modules=$(grep -E '^mod ' "$SRC/tools/local-pg-mirror/src/lib.rs" | tr '\n' ' ')"
  echo "untracked=$(git -C "$SRC" status --short -- tools/local-pg-mirror | tr '\n' '; ')"
  echo "decision=producer_unfixed_not_executed"
  echo "postgres_started_by_a409=0"
  echo "enterprise_stop_by_a409=0"
} > "$EVIDENCE/producer-observation-host.txt"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker image inspect -f 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a409 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a409 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a409 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a409 \
  --label oo.kind=cargo \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a409 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a409 \
  --label oo.kind=target \
  "$TARGET_VOL" >/dev/null
docker ps -a --format '{{.Names}} {{.Status}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/containers-before.txt"
own=$(docker ps -q --filter label=oo.owner=a409 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
cleanup_unused() {
  docker volume rm "$CARGO_VOL" > "$EVIDENCE/cargo-volume-removed.txt" 2>&1 || true
  docker volume rm "$TARGET_VOL" > "$EVIDENCE/target-volume-removed.txt" 2>&1 || true
  docker network rm "$NET" > "$EVIDENCE/network-removed.txt" 2>&1 || true
}
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  cleanup_unused
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  cleanup_unused
  exit 2
fi
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
  --pids-limit 256 \
  --network "$NET" \
  -e HOME=/tmp \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  --mount "type=bind,source=${SRC},target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${SRC}/testing/local-copy-acceptance/linux/1.94/inside.sh,target=/inside.sh,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=256m \
  --label oo.project=open-ontology \
  --label oo.owner=a409 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a409 \
  --label oo.task=local-copy-acceptance-linux194 \
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
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
} > "$EVIDENCE/hostconfig.txt"
docker inspect -f '{{json .HostConfig.Tmpfs}}' "$NAME" > "$EVIDENCE/tmpfs.txt"
docker inspect -f '{{range .Mounts}}{{.Type}} {{.Source}} {{.Destination}} rw={{.RW}}{{println}}{{end}}' "$NAME" > "$EVIDENCE/mounts.txt"
docker inspect -f '{{json .NetworkSettings.Networks}}' "$NAME" > "$EVIDENCE/network.txt"
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
docker volume rm "$CARGO_VOL" > "$EVIDENCE/cargo-volume-removed.txt"
docker volume rm "$TARGET_VOL" > "$EVIDENCE/target-volume-removed.txt"
docker network rm "$NET" > "$EVIDENCE/network-removed.txt"
exit "$run_exit"
