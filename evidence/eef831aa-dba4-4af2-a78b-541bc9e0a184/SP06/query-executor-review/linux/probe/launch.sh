#!/bin/bash
# One Linux arm64 Rust 1.94 builder. Archive copy plus the review test. Reuses smoke volumes.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a184/SP06/query-executor-review
PROBE=${EVIDENCE}/linux/probe
ROOT=/tmp/oo-a184-eef831aa-dba4-4af2-a78b-541bc9e0a184
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
FIXED=8a289f0c699abb8c707ca45ad45cb036abc3fc05
NAME=oo-a184-probe
NET=oo-a184-net
CARGO_VOL=oo-a184-cargo
TARGET_VOL=oo-a184-target
SEC=${ROOT}/secrets

chmod 755 "$PROBE/inside.sh" "$PROBE/launch.sh"
if [ -f "$PROBE/console.log" ]; then
  echo "refusing to overwrite probe console.log" >&2
  exit 2
fi
if [ ! -f "$EVIDENCE/linux/smoke/smoke.exit" ] || [ "$(cat "$EVIDENCE/linux/smoke/smoke.exit")" != "0" ]; then
  echo "pre-overlay smoke is not green" >&2
  exit 2
fi
if [ ! -f "$ROOT/archive/crates/oo-gateway/src/query_executor.rs" ]; then
  echo "immutable archive is absent" >&2
  exit 2
fi
if [ -f "$ROOT/archive/crates/oo-gateway/tests/query_executor_a184_review.rs" ]; then
  echo "immutable archive contains the review overlay" >&2
  exit 2
fi
if ! docker ps --filter name=^/oo-a184-pg$ --filter status=running --format '{{.Names}}' | grep -q '^oo-a184-pg$'; then
  echo "private postgres is not running" >&2
  exit 2
fi
if [ ! -f "$SEC/ca.crt" ] || [ ! -f "$SEC/admin.pass" ] || [ ! -f "$SEC/login.pass" ]; then
  echo "fixture secrets are absent" >&2
  exit 2
fi
rm -rf "$ROOT/probe-src"
mkdir -p "$ROOT/probe-src"
cp -a "$ROOT/archive/." "$ROOT/probe-src/"
cp "$EVIDENCE/overlay/query_executor_a184_review.rs" "$ROOT/probe-src/crates/oo-gateway/tests/query_executor_a184_review.rs"
python3 - "$SRC" "$FIXED" "$ROOT/probe-src" "$PROBE/overlay-delta.txt" <<'PY'
import os, subprocess, sys
repo, commit, probe, out = sys.argv[1:]
lines = subprocess.check_output(["git", "-C", repo, "ls-tree", "-r", commit], text=True).splitlines()
bad = []
for line in lines:
    meta, path = line.split("\t", 1)
    kind, oid = meta.split()[1], meta.split()[2]
    if kind != "blob":
        continue
    full = os.path.join(probe, path)
    got = subprocess.check_output(["git", "hash-object", full], text=True).strip()
    if got != oid:
        bad.append(path)
extra = "crates/oo-gateway/tests/query_executor_a184_review.rs"
if not os.path.isfile(os.path.join(probe, extra)):
    raise SystemExit("overlay missing from probe copy")
text = f"compared={len(lines)} changed={len(bad)} overlay=present\n"
open(out, "w").write(text + "only the review test was added to the probe copy\n")
print(text, end="")
if bad:
    print("\n".join(bad[:20]))
    raise SystemExit(1)
PY
ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$PROBE/host-cargo-procs.txt" || true
host_n=$(grep -c . "$PROBE/host-cargo-procs.txt" || true)
docker ps --filter label=oo.kind=rust-builder --format '{{.Names}}' > "$PROBE/builders-before.txt" || true
builder_n=$(grep -c . "$PROBE/builders-before.txt" || true)
global_n=$((host_n + builder_n))
printf 'host_cargo=%s rust_builders=%s global=%s cap=3\n' "$host_n" "$builder_n" "$global_n" > "$PROBE/capacity.txt"
if [ "$global_n" -ge 3 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$PROBE/docker-run.exit"
  exit 3
fi
own=$(docker ps -q --filter label=oo.owner=a184 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$PROBE/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named probe container already exists" >&2
  exit 2
fi
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL image=pinned-rust-1.94 bash /inside.sh" > "$PROBE/invocation.txt"
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
  -e OO_A184_LIVE=1 \
  -e OO_A184_ORACLE=/evidence/live/oracle.txt \
  --mount "type=bind,source=${ROOT}/probe-src,target=/src,readonly" \
  --mount "type=bind,source=${PROBE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${SEC},target=/run/oo-a184/fixture,readonly" \
  --tmpfs /tmp:rw,nosuid,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a184 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a184 \
  --label oo.task=query-executor-review-probe \
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
if [ ! -f "$PROBE/probe.exit" ]; then
  printf '%s\n' "$run_exit" > "$PROBE/probe.exit"
fi
exit "$run_exit"
