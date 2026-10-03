#!/bin/bash
# One owned builder for scoped clippy. Does not repeat the green sqlx-core tests.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a133/SP03/init-tail
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a133-init-tail-clippy
NET=oo-a133-eef831aa-net
CARGO_VOL=oo-a133-eef831aa-cargo
TARGET_VOL=oo-a133-eef831aa-target
SNAP=/tmp/oo-a133-eef831aa/proof-snapshot

mkdir -p "$EVIDENCE/proof/attempt6"
if [ -f "$EVIDENCE/proof/embed-console.log" ]; then
  cp "$EVIDENCE/proof/embed-console.log" "$EVIDENCE/proof/attempt6/embed-console.log"
fi
cp "$EVIDENCE/proof"/embed-*.exit "$EVIDENCE/proof/attempt6/" 2>/dev/null || true
cp "$EVIDENCE/proof"/embed-bytes-lock.txt "$EVIDENCE/proof"/embed-bytes-pin.txt "$EVIDENCE/proof"/embed-product-source.sha256 "$EVIDENCE/proof/attempt6/" 2>/dev/null || true
printf '%s\n' 'isolated sqlx-core tests passed (4 passed, bytes 1.12.1). clippy then exited 1 because cargo-clippy is not installed in the fresh image. This run installs the 1.94.0 clippy component and repeats only clippy.' > "$EVIDENCE/proof/attempt6/reason.txt"

running_rust=$(docker ps --no-trunc --format '{{.Image}} {{.Label "oo.kind"}}' | awk '$1 ~ /rust/ || $1 ~ /0f24958fa693/ || $1 ~ /f747a9f2b58b/ || $1 ~ /218765d808d3/ || $1 ~ /f31fa9eaac4e/ || $2=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a133 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'running_rust_image_or_label=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s\n' "$running_rust" "$own" "$host_cargo" > "$EVIDENCE/proof/clippy-capacity.txt"
if [ "$own" -ge 1 ] || [ "$((running_rust + host_cargo))" -ge 2 ]; then
  echo "deferred: rust builder capacity is full" >&2
  printf '%s\n' 3 > "$EVIDENCE/proof/clippy-docker-run.exit"
  exit 3
fi

test -f "$SNAP/crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs"
grep -q 'fn committed_init_len' "$SNAP/crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs"
if [ -f "$SNAP/crates/oo-connectors/src/stream.rs" ]; then
  echo "snapshot contains stream.rs; refusing to compile it" >&2
  printf '%s\n' 4 > "$EVIDENCE/proof/clippy-docker-run.exit"
  exit 4
fi

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/proof/clippy-image.txt"
grep -q 'arch=arm64' "$EVIDENCE/proof/clippy-image.txt"

if docker container inspect "$NAME" >/dev/null 2>&1; then
  prior=$(docker inspect -f '{{.State.Status}}' "$NAME")
  if [ "$prior" = "running" ]; then
    echo "deferred: named container is already running" >&2
    printf '%s\n' 3 > "$EVIDENCE/proof/clippy-docker-run.exit"
    exit 3
  fi
  docker rm "$NAME" >/dev/null
fi

rm -f "$EVIDENCE/proof/clippy-docker-run.exit" "$EVIDENCE/proof/clippy-worst.exit"

cat > "$EVIDENCE/proof/clippy-invocation.txt" <<EOF
continuation_of=embed sqlx-core tests exit 0, clippy component missing
phases=rustup-component-add-clippy,scoped-clippy
not_repeated=smoke,lib,bounds,accounting,portal,view,tail,sqlx-core
image=${IMAGE}
network=${NET}
container=${NAME}
privileged=no
cap_drop=ALL
postgres=no
stream_rs=absent
EOF

set +e
docker run --name "$NAME" \
  --pull never \
  --platform linux/arm64 \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory 8g \
  --memory-swap 8g \
  --cpus 4 \
  --pids-limit 4096 \
  --network "$NET" \
  --workdir /src \
  --mount "type=bind,src=${SNAP},dst=/src,readonly" \
  --mount "type=bind,src=${EVIDENCE},dst=/evidence" \
  --mount "type=volume,src=${CARGO_VOL},dst=/cargo-home" \
  --mount "type=volume,src=${TARGET_VOL},dst=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a133 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a133 \
  --label oo.kind=rust-builder \
  --label oo.task=init-tail-clippy \
  "$IMAGE" \
  bash /evidence/proof/inside-clippy.sh \
  > "$EVIDENCE/proof/clippy-console.log" 2>&1
run_exit=$?
set -e
printf '%s\n' "$run_exit" > "$EVIDENCE/proof/clippy-docker-run.exit"
docker inspect "$NAME" --format 'ExitCode={{.State.ExitCode}}
Status={{.State.Status}}
OOMKilled={{.State.OOMKilled}}
Privileged={{.HostConfig.Privileged}}
NetworkMode={{.HostConfig.NetworkMode}}
Memory={{.HostConfig.Memory}}
MemorySwap={{.HostConfig.MemorySwap}}
NanoCpus={{.HostConfig.NanoCpus}}
PidsLimit={{.HostConfig.PidsLimit}}
CapDrop={{json .HostConfig.CapDrop}}
CapAdd={{json .HostConfig.CapAdd}}
SecurityOpt={{json .HostConfig.SecurityOpt}}
{{range .Mounts}}mount dest={{.Destination}} type={{.Type}} rw={{.RW}}
{{end}}' > "$EVIDENCE/proof/clippy-hostconfig.txt" || true
docker rm "$NAME" > "$EVIDENCE/proof/clippy-container-rm.txt"
echo "HOST_DONE clippy_docker_run_exit=${run_exit}"
exit "$run_exit"
