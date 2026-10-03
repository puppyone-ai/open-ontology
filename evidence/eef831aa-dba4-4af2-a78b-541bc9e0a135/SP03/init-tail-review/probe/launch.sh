#!/bin/bash
# One probe builder. Starts only after smoke has exited and Postgres is ready.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a135/SP03/init-tail-review
ROOT=/tmp/oo-a135-eef831aa
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a135-init-tail-probe
SMOKE=oo-a135-init-tail-smoke
CARGO_VOL=oo-a135-eef831aa-cargo
TARGET_VOL=oo-a135-eef831aa-target
LOGVOL=oo-a135-eef831aa-pglog
NET=oo-a135-eef831aa-net
SNAP=${ROOT}/snapshot
SEC=${ROOT}/secrets
ROWS=12000

mkdir -p "$EVIDENCE/probe"
chmod 755 "$EVIDENCE/probe/inside.sh" "$EVIDENCE/probe/launch.sh"

if [ ! -f "$EVIDENCE/smoke/worst.exit" ] || [ "$(cat "$EVIDENCE/smoke/worst.exit")" != "0" ]; then
  echo "smoke is not green; probes stay shut" >&2
  printf '%s\n' 2 > "$EVIDENCE/probe/docker-run.exit"
  exit 2
fi
if docker inspect -f '{{.State.Running}}' "$SMOKE" 2>/dev/null | grep -q true; then
  echo "deferred: smoke builder is still running" >&2
  printf '%s\n' 3 > "$EVIDENCE/probe/docker-run.exit"
  exit 3
fi
if [ ! -f "$EVIDENCE/pg/up.exit" ] || [ "$(cat "$EVIDENCE/pg/up.exit")" != "0" ]; then
  echo "postgres is not ready" >&2
  printf '%s\n' 2 > "$EVIDENCE/probe/docker-run.exit"
  exit 2
fi

running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a135 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'running_rust_image_or_label=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s\n' "$running_rust" "$own" "$host_cargo" > "$EVIDENCE/probe/capacity.txt"
if [ "$own" -ge 1 ]; then
  echo "deferred: this task already has a running rust builder" >&2
  printf '%s\n' 3 > "$EVIDENCE/probe/docker-run.exit"
  exit 3
fi
if [ "$((running_rust + host_cargo))" -ge 2 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$EVIDENCE/probe/docker-run.exit"
  exit 3
fi

python3 - <<PY > "$EVIDENCE/probe/oracle.txt"
import hashlib
n = $ROWS
digest = hashlib.sha256()
bits = 0
for i in range(1, n + 1):
    digest.update(i.to_bytes(4, "big"))
    bits ^= i
print(f"rows={n}")
print(f"sum={n * (n + 1) // 2}")
print(f"bit_xor={bits}")
print(f"row_sha256={digest.hexdigest()}")
print("row_sha256_input=big-endian i32 id for 1..=rows")
print("oracle_producer=host python3 hashlib, not the rust test")
PY

cp "$EVIDENCE/overlay/a135_tail_probe.rs" "$SNAP/crates/oo-connectors/tests/a135_tail_probe.rs"
want=$(awk '{print $1}' "$EVIDENCE/smoke/expect-buffered.sha256")
got=$(shasum -a 256 "$SNAP/crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs" | awk '{print $1}')
test "$got" = "$want"
shasum -a 256 "$SNAP/crates/oo-connectors/src/stream.rs" > "$EVIDENCE/probe/stream-before.sha256"

if docker container inspect "$NAME" >/dev/null 2>&1; then
  prior=$(docker inspect -f '{{.State.Status}}' "$NAME")
  if [ "$prior" = "running" ]; then
    echo "deferred: probe container is already running" >&2
    printf '%s\n' 3 > "$EVIDENCE/probe/docker-run.exit"
    exit 3
  fi
  docker rm "$NAME" >/dev/null
fi

cat > "$EVIDENCE/probe/invocation.txt" <<EOF
image=${IMAGE}
toolchain=1.94.0
container=${NAME}
network=${NET}
pull=never
privileged=no
home_mount=no
socket_mount=no
host_network=no
shared_cargo_cache=no
overlay=crates/oo-connectors/tests/a135_tail_probe.rs
product_edits=none
rows=${ROWS}
phases=probe,author-tail,tls,stream-live,stream-bounds
EOF

set +e
docker run \
  --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
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
  -e CARGO_TERM_COLOR=never \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=4 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/tmp \
  -e OO_A135_TLS_FIXTURE=/secrets/tls-pointer.json \
  -e OO_A135_ORACLE=/evidence/probe/oracle.txt \
  -e OO_PG_STREAM_FIXTURE=/secrets/stream-pointer.json \
  --mount "type=bind,source=${SNAP},target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/probe/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=volume,source=${LOGVOL},target=/pglog,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a135 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a135 \
  --label oo.task=init-tail-review-probe \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/probe/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EVIDENCE/probe/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
  echo "Status=$(docker inspect -f '{{.State.Status}}' "$NAME")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME")"
  echo "CapAdd=$(docker inspect -f '{{json .HostConfig.CapAdd}}' "$NAME")"
  echo "PortBindings=$(docker inspect -f '{{json .HostConfig.PortBindings}}' "$NAME")"
} > "$EVIDENCE/probe/hostconfig.txt"
exit "$run_exit"
