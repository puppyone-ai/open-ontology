#!/bin/bash
# Host launch for the single a100 arm64 Rust 1.94 builder. No pull, no prune.
set -u
ROOT=/Users/redux/puppy-one/open-ontology
EV=$ROOT/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a100/SP01/P5-identity-review
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a100-msrv-5dd178d8
NET=oo-a100-5dd178d8-net
CARGO_VOL=oo-a100-5dd178d8-cargo
TARGET_VOL=oo-a100-5dd178d8-target
UUID=5dd178d8-06bf-4ab2-aed9-66575a76475b

mkdir -p "$EV/raw"
inventory() {
  {
    echo "when=$1"
    echo "---- ps ----"
    docker ps -a --format '{{.ID}} {{.Names}} {{.Image}} {{.Status}} {{.Labels}}'
    echo "---- volumes ----"
    docker volume ls --format '{{.Name}}'
    echo "---- networks ----"
    docker network ls --format '{{.ID}} {{.Name}}'
    echo "---- rust-builders ----"
    docker ps --filter label=oo.kind=rust-builder --filter status=running --format '{{.Names}} {{.Status}}'
    echo "---- host tools ----"
    ps -ax -o pid=,ppid=,etime=,comm= | awk '$4=="cargo" || $4=="rustc" || $4=="clippy-driver" || $4=="rustup" { print }'
  } > "$EV/raw/inventory-$1.txt" 2>&1
}

{
  echo "head=$(git -C "$ROOT" rev-parse HEAD)"
  echo "commit=$(git -C "$ROOT" rev-parse 6831d6e862303a0ce1c883925068078cbcb78e23)"
  echo "tree=$(git -C "$ROOT" rev-parse '6831d6e862303a0ce1c883925068078cbcb78e23^{tree}')"
  echo "parent=$(git -C "$ROOT" rev-parse '6831d6e862303a0ce1c883925068078cbcb78e23^')"
  echo "uuid=$UUID"
} > "$EV/raw/identity.txt"

lock=$(shasum -a 256 "$EV/archive/spike/Cargo.lock" | awk '{print $1}')
corpus=$(shasum -a 256 "$EV/archive/spike/corpus/expected.json" | awk '{print $1}')
blob=$(git -C "$ROOT" hash-object "$EV/archive/spike/corpus/expected.json")
tarsha=$(shasum -a 256 "$EV/archive/spike-6831d6e.tar" | awk '{print $1}')
keep=$(shasum -a 256 "$ROOT/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a091/SP01/P5-ipc-review/raw/probe-keep.out" | awk '{print $1}')
printf 'lock=%s\ncorpus=%s\ncorpus_blob=%s\ntar=%s\na091_probe_keep=%s\n' \
  "$lock" "$corpus" "$blob" "$tarsha" "$keep" > "$EV/raw/preflight-hashes.txt"
ok=1
[ "$lock" = "c46ce7f6a90e7f2983eaa4bb1471fb45e275e38a220716175801d56981ce53b7" ] || ok=0
[ "$corpus" = "6f35c87189ea79adc41399977237e7f6c29f5c001cc8d86e06632678db519931" ] || ok=0
[ "$blob" = "2ead1211bc596463550d4741862364fd45c589e4" ] || ok=0
[ "$keep" = "fb6984490f9b1fa89945e21665e6194475d239ebba4f7e86ead3953f4a651a46" ] || ok=0
if [ "$ok" -ne 1 ]; then
  echo "preflight mismatch" >&2
  exit 2
fi

docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$EV/raw/image-inspect.txt"
arch=$(sed -n 's/.* arch=//p' "$EV/raw/image-inspect.txt")
if [ "$arch" != "arm64" ]; then
  echo "image arch $arch" >&2
  exit 2
fi

inventory before
builders=$(docker ps -q --filter label=oo.kind=rust-builder --filter status=running | wc -l | tr -d ' ')
host=$(ps -ax -o pid=,ppid=,etime=,comm= | awk '$4=="cargo" || $4=="rustc" || $4=="clippy-driver" || $4=="rustup" { c++ } END { print c+0 }')
printf 'running_rust_builders=%s\nhost_cargo_rustc_clippy_rustup=%s\n' "$builders" "$host" > "$EV/raw/capacity-before.txt"
ps -ax -o pid=,ppid=,etime=,comm= | awk '$4=="cargo" || $4=="rustc" || $4=="clippy-driver" || $4=="rustup" { print }' >> "$EV/raw/capacity-before.txt"
if [ "$((builders + host))" -ge 2 ]; then
  echo "builder slot full" >&2
  exit 3
fi

docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a100 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a100 \
  --label oo.uuid="$UUID" \
  "$NET" > "$EV/raw/network-create.txt"
net_rc=$?
printf '%s\n' "$net_rc" > "$EV/raw/network-create.exit"
if [ "$net_rc" -ne 0 ]; then
  inventory after
  exit "$net_rc"
fi

docker volume create --label oo.owner=a100 --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a100 --label oo.uuid="$UUID" "$CARGO_VOL" > "$EV/raw/volume-cargo.txt"
docker volume create --label oo.owner=a100 --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a100 --label oo.uuid="$UUID" "$TARGET_VOL" > "$EV/raw/volume-target.txt"

docker run --name "$NAME" \
  --pull never \
  --init \
  --user 0:0 \
  --cap-drop ALL \
  --security-opt no-new-privileges \
  --memory 6g \
  --cpus 4 \
  --pids-limit 2048 \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --network "$NET" \
  --label oo.project=open-ontology \
  --label oo.owner=a100 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a100 \
  --label oo.uuid="$UUID" \
  --label oo.kind=rust-builder \
  --label oo.platform=linux/arm64 \
  --label oo.toolchain=1.94.0 \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=4 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -v "$EV/archive/spike:/src:ro" \
  -v "$EV/archive/debs:/debs:ro" \
  -v "$EV/raw/probes:/probe:ro" \
  -v "$EV:/evidence" \
  -v "$CARGO_VOL:/cargo-home" \
  -v "$TARGET_VOL:/target" \
  "$IMAGE" \
  bash /probe/inside.sh
run_rc=$?
printf '%s\n' "$run_rc" > "$EV/raw/docker-run.exit"

docker inspect "$NAME" --format 'Status={{.State.Status}} ExitCode={{.State.ExitCode}} OOMKilled={{.State.OOMKilled}} Error={{.State.Error}}' > "$EV/raw/docker-state.txt" 2>&1 || true
docker inspect "$NAME" --format 'CapDrop={{json .HostConfig.CapDrop}} Privileged={{.HostConfig.Privileged}} PortBindings={{json .HostConfig.PortBindings}} NetworkMode={{.HostConfig.NetworkMode}} Tmpfs={{json .HostConfig.Tmpfs}} SecurityOpt={{json .HostConfig.SecurityOpt}} Memory={{.HostConfig.Memory}} NanoCpus={{.HostConfig.NanoCpus}} PidsLimit={{.HostConfig.PidsLimit}}' > "$EV/raw/docker-hostconfig.txt" 2>&1 || true

docker rm "$NAME" > "$EV/raw/cleanup-container.txt" 2>&1
printf '%s\n' "$?" > "$EV/raw/cleanup-container.exit"
docker network rm "$NET" > "$EV/raw/cleanup-network.txt" 2>&1
printf '%s\n' "$?" > "$EV/raw/cleanup-network.exit"
docker volume rm "$CARGO_VOL" > "$EV/raw/cleanup-cargo.txt" 2>&1
printf '%s\n' "$?" > "$EV/raw/cleanup-cargo.exit"
docker volume rm "$TARGET_VOL" > "$EV/raw/cleanup-target.txt" 2>&1
printf '%s\n' "$?" > "$EV/raw/cleanup-target.exit"
inventory after
exit "$run_rc"
