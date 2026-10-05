#!/bin/bash
# One Linux arm64 Rust 1.94 builder for the fresh oracle. Reuses the smoke cargo and target volumes.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a179/SP06/sql-guard-review
PROBE=${EVIDENCE}/linux/probe
ROOT=/tmp/oo-a179-eef831aa-dba4-4af2-a78b-541bc9e0a179
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a179-probe
NET=oo-a179-net
CARGO_VOL=oo-a179-cargo
TARGET_VOL=oo-a179-target
SEC=${ROOT}/secrets

chmod 755 "$PROBE/inside.sh" "$PROBE/launch.sh"
if [ -f "$PROBE/console.log" ]; then
  echo "refusing to overwrite probe console.log" >&2
  exit 2
fi
if [ ! -f "$EVIDENCE/linux/smoke/smoke.exit" ] || [ "$(cat "$EVIDENCE/linux/smoke/smoke.exit")" != 0 ]; then
  echo "pre-overlay smoke is not green" >&2
  exit 2
fi
if [ ! -f "$ROOT/archive/crates/oo-gateway/src/sql_guard.rs" ]; then
  echo "immutable archive is absent" >&2
  exit 2
fi
if [ -f "$ROOT/archive/crates/oo-gateway/tests/sql_guard_a179_review.rs" ]; then
  echo "immutable archive already contains the overlay" >&2
  exit 2
fi
rm -rf "$ROOT/probe-src"
mkdir -p "$ROOT/probe-src"
cp -a "$ROOT/archive/." "$ROOT/probe-src/"
cp "$EVIDENCE/overlay/sql_guard_a179_review.rs" "$ROOT/probe-src/crates/oo-gateway/tests/sql_guard_a179_review.rs"
python3 - <<'PY'
import os, subprocess
repo = "/Users/redux/puppy-one/open-ontology"
archive = "/tmp/oo-a179-eef831aa-dba4-4af2-a78b-541bc9e0a179/archive"
probe = "/tmp/oo-a179-eef831aa-dba4-4af2-a78b-541bc9e0a179/probe-src"
sha = "03512f91324ac33e772e9702fe806e8e04037ec4"
extra = "crates/oo-gateway/tests/sql_guard_a179_review.rs"
lines = subprocess.check_output(["git","-C",repo,"ls-tree","-r",sha], text=True).splitlines()
bad = []
for line in lines:
    meta, path = line.split("\t", 1)
    blob = meta.split()[2]
    full = os.path.join(probe, path)
    got = subprocess.check_output(["git","hash-object", full], text=True).strip()
    if got != blob:
        bad.append(path)
overlay = os.path.join(probe, extra)
if not os.path.isfile(overlay):
    raise SystemExit("overlay missing from probe copy")
print(f"compared={len(lines)} changed={len(bad)} overlay=present")
if bad:
    print("\n".join(bad[:20]))
    raise SystemExit(1)
PY
printf '%s\n' "only crates/oo-gateway/tests/sql_guard_a179_review.rs was added to the probe copy" > "$PROBE/overlay-delta.txt"

own=$(docker ps -q --filter label=oo.owner=a179 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s\n' "$own" "$global_builders" "$host_cargo" > "$PROBE/capacity.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$PROBE/containers-before.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 2 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$PROBE/docker-run.exit"
  exit 3
fi
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  echo "task network is missing" >&2
  exit 2
fi
if [ ! -d "$SEC" ]; then
  echo "fixture secrets are absent" >&2
  exit 2
fi
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL image=pinned-rust-1.94 bash /inside.sh" > "$PROBE/invocation.txt"
mkdir -p "$EVIDENCE/live"
set +e
docker run --name "$NAME" \
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
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=4 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  -e OO_A179_LIVE=1 \
  --mount "type=bind,source=${ROOT}/probe-src,target=/src,readonly" \
  --mount "type=bind,source=${PROBE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${SEC},target=/run/oo-a179/fixture,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a179 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a179 \
  --label oo.task=sql-guard-review-probe \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$PROBE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$PROBE/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME")"
  echo "CapAdd=$(docker inspect -f '{{json .HostConfig.CapAdd}}' "$NAME")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
  echo "PortBindings=$(docker inspect -f '{{json .HostConfig.PortBindings}}' "$NAME")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
} > "$PROBE/hostconfig.txt"
docker inspect -f '{{json .Mounts}}' "$NAME" > "$PROBE/mounts.txt"
docker rm "$NAME" > "$PROBE/container-removed.txt"
exit "$run_exit"
