#!/bin/bash
# One linux/arm64 Rust 1.94 builder. Compiles the pinned pack and runs dialogue identity tests.
# Does not start PostgreSQL, does not call the model, and does not reload the 100k chain.
set -u
SRC=/Users/redux/puppy-one/open-ontology
UUID=eef831aa-dba4-4af2-a78b-541bc9e0a389
REVIEW=${SRC}/evidence/${UUID}/SP12/product-benchmark
EVIDENCE=${REVIEW}/linux/ready
A399=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a399/SP09/pack-dialogue-compat/live/pack-command.json
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a389-ready
NET=oo-a389-eef831aa-net
CARGO_VOL=oo-a389-eef831aa-cargo
TARGET_VOL=oo-a389-eef831aa-target
SRC_VOL=oo-a389-eef831aa-src
DIGEST=e7490f751339374bf5d7ef3647cb55190a639d603b4f9186ecff65c0335cdd2e

mkdir -p "$EVIDENCE/phases"
chmod 755 "$SRC/testing/product-benchmark/linux/ready/inside.sh" "$SRC/testing/product-benchmark/linux/ready/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite ready console.log" >&2
  exit 2
fi
if [ ! -f "$A399" ]; then
  echo "a399 pack command is absent" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
python3 - "$REVIEW/live/run/steps/role-pack-wide.stdin" "$EVIDENCE/shop-command.json" "$DIGEST" << 'PY'
import json, sys
src, dest, digest = sys.argv[1:]
doc = json.loads(open(src).read())
for key in ("effective_oos", "visibility"):
    part = doc[key]
    if part["policy"]["id"] != "pol-a317" or part["knowledge"]["id"] != "know-a317":
        raise SystemExit("chain command identity is not the saved pair")
    part["policy"]["id"] = digest
    part["knowledge"]["id"] = "know_a317"
open(dest, "w").write(json.dumps(doc, separators=(",", ":")) + "\n")
PY

docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a389 \
  --label oo.session="$UUID" \
  --label oo.kind=network \
  "$NET" >/dev/null
own=$(docker ps -q --filter label=oo.owner=a389 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt" || { echo "image is not arm64" >&2; printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"; exit 1; }
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named ready container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create --label oo.project=open-ontology --label oo.owner=a389 --label oo.session="$UUID" --label oo.kind=cargo "$CARGO_VOL" > "$EVIDENCE/volume-cargo.txt"
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create --label oo.project=open-ontology --label oo.owner=a389 --label oo.session="$UUID" --label oo.kind=target "$TARGET_VOL" > "$EVIDENCE/volume-target.txt"
docker volume inspect "$SRC_VOL" >/dev/null 2>&1 || docker volume create --label oo.project=open-ontology --label oo.owner=a389 --label oo.session="$UUID" --label oo.kind=src "$SRC_VOL" > "$EVIDENCE/volume-src.txt"
if ! docker run --rm --pull never --network none --user 0:0 --security-opt no-new-privileges --cap-drop ALL --entrypoint test --mount "type=volume,source=${SRC_VOL},target=/src,readonly" "$IMAGE" -f /src/Cargo.toml >/dev/null 2>&1; then
  git -C "$SRC" archive HEAD | docker run --rm -i \
    --pull never \
    --network none \
    --user 0:0 \
    --security-opt no-new-privileges \
    --cap-drop ALL \
    --entrypoint tar \
    --mount "type=volume,source=${SRC_VOL},target=/src" \
    "$IMAGE" \
    -x -C /src
fi
printf '%s\n' "docker run -d --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 bash /inside.sh" > "$EVIDENCE/invocation.txt"
cid=$(docker run -d --name "$NAME" \
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
  --label oo.project=open-ontology \
  --label oo.owner=a389 \
  --label oo.session="$UUID" \
  --label oo.kind=rust-builder \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e TMPDIR=/cargo-home/tmp \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --mount "type=volume,source=${SRC_VOL},target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${A399},target=/a399/pack-command.json,readonly" \
  --mount "type=bind,source=${SRC}/testing/product-benchmark/linux/ready/inside.sh,target=/inside.sh,readonly" \
  "$IMAGE" \
  bash /inside.sh)
if [ -z "$cid" ]; then
  echo "ready container did not start" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
printf '%s\n' "$cid" > "$EVIDENCE/container.id"
printf '%s\n' 0 > "$EVIDENCE/docker-run.exit"
echo "ready_started id=$cid"
