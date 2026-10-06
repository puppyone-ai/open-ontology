#!/bin/bash
# Two-pass Rust manifest. The console path must sit outside this evidence tree.
# Does not prune. Does not modify a279. One builder, 6 GiB, 1 CPU, network none.
set -u
if [ "$#" -ne 2 ]; then
  echo "usage: launch.sh pass1|pass2 <console-outside-evidence>" >&2
  exit 2
fi
MODE=$1
CONSOLE=$2
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a282/SP10/pi-rpc-verification-fix
EVIDENCE=${REVIEW}/linux/manifest
ORIGIN=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/manifest
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
case "$MODE" in
  pass1) NAME=oo-a282-manifest ;;
  pass2) NAME=oo-a282-manifest-rewrite ;;
  *)
    echo "mode must be pass1 or pass2" >&2
    exit 2
    ;;
esac
case "$CONSOLE" in
  "$REVIEW"/*)
    echo "console path is inside the evidence directory" >&2
    exit 2
    ;;
esac
if [ "$CONSOLE" = "$REVIEW" ]; then
  echo "console path is the evidence directory" >&2
  exit 2
fi
if [ "$MODE" = pass1 ]; then
  if [ -f "$EVIDENCE/console.log" ]; then
    echo "refusing to overwrite manifest console.log" >&2
    exit 2
  fi
else
  if [ ! -f "$EVIDENCE/console.log" ] || [ ! -f "$EVIDENCE/hostconfig.txt" ] || [ ! -f "$EVIDENCE/docker-run.exit" ]; then
    echo "pass1 records are missing" >&2
    exit 2
  fi
  if [ "$(cat "$EVIDENCE/docker-run.exit")" != 0 ]; then
    echo "pass1 docker-run.exit is not 0" >&2
    exit 2
  fi
  if [ -f "$EVIDENCE/rewrite-stdout.txt" ]; then
    echo "refusing to overwrite rewrite-stdout.txt" >&2
    exit 2
  fi
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  exit 1
fi
mkdir -p "$(dirname "$CONSOLE")"
own=$(docker ps -q --filter label=oo.owner=a282 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
if [ "$MODE" = pass1 ]; then
  printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_task=1 mode=pass1\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
  docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/containers-before.txt"
else
  printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_task=1 mode=pass2\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/pass2-capacity.txt"
  docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/pass2-containers-before.txt"
fi
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  if [ "$MODE" = pass1 ]; then
    printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  fi
  exit 3
fi
if [ "$MODE" = pass1 ]; then
  docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
  if ! grep -q 'arch=arm64' "$EVIDENCE/image.txt"; then
    echo "pinned image is not arm64" >&2
    exit 1
  fi
  printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network none --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 bash /evidence/linux/manifest/inside.sh" > "$EVIDENCE/invocation.txt"
else
  docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/pass2-image.txt"
  if ! grep -q 'arch=arm64' "$EVIDENCE/pass2-image.txt"; then
    echo "pinned image is not arm64" >&2
    exit 1
  fi
  printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network none --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 bash /evidence/linux/manifest/inside.sh" > "$EVIDENCE/pass2-invocation.txt"
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named manifest container already exists" >&2
  exit 2
fi
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
  --mount "type=bind,source=${ORIGIN},target=/origin-manifest,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=256m \
  --label oo.project=open-ontology \
  --label oo.owner=a282 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a282 \
  --label oo.task=pi-rpc-verification-fix-manifest \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /evidence/linux/manifest/inside.sh > "$CONSOLE" 2>&1
run_exit=$?
set +e
if [ "$MODE" = pass1 ]; then
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
  docker inspect -f '{{range .Mounts}}{{.Type}} {{.Destination}} rw={{.RW}}{{println}}{{end}}' "$NAME" > "$EVIDENCE/mounts.txt"
  cp "$CONSOLE" "$EVIDENCE/console.log"
  docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
else
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
    echo "docker_run_exit=${run_exit}"
  } >> "$CONSOLE"
  docker rm "$NAME" >> "$CONSOLE"
  cp "$CONSOLE" "$EVIDENCE/rewrite-stdout.txt"
  out_dir=$(dirname "$CONSOLE")
  rm -f "$CONSOLE" "$out_dir/pass1.stdout"
  rmdir "$out_dir"
  dir_exit=$?
  printf 'outside_stdout_dir_removed=%s\n' "$dir_exit" >> "$EVIDENCE/rewrite-stdout.txt"
fi
exit "$run_exit"
