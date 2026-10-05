#!/bin/bash
# Reproduce the pre-fix stall against the immutable archive. One builder, one PG.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a196/SP06/cancel-transport-bounds
RED=${EVIDENCE}/linux/red
ROOT=/tmp/oo-a196-eef831aa-dba4-4af2-a78b-541bc9e0a196
SEC=/tmp/oo-a196-eef831aa-dba4-4af2-a78b-541bc9e0a196/secrets/client
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a196-red
NET=oo-a196-net
CARGO_VOL=oo-a196-cargo
TARGET_VOL=oo-a196-target

chmod 755 "$RED/inside.sh" "$RED/launch.sh"
if [ -f "$RED/console.log" ]; then
  echo "refusing to overwrite red console.log" >&2
  exit 2
fi
if [ ! -f "$ROOT/archive/crates/oo-gateway/src/query_executor.rs" ]; then
  echo "immutable archive is missing" >&2
  exit 2
fi
if [ ! -d "$SEC" ]; then
  echo "postgres client secrets are missing" >&2
  exit 2
fi
python3 - "$ROOT/archive/crates/oo-gateway/tests/query_executor_pg_live.rs" "$RED/stall_append.rs" "$ROOT/red-test.rs" <<'PY'
import pathlib, sys
src, append, out = map(pathlib.Path, sys.argv[1:])
text = src.read_text()
needle = "    direct_privileges(&mut env).await;\n"
insert = """    if std::env::var(\"OO_A196_RED\").ok().as_deref() == Some(\"1\") {
        prove_baseline_stall(&mut env).await;
        env.oracle.fact(\"red_result\", \"reproduced\");
        return;
    }
    direct_privileges(&mut env).await;
"""
if needle not in text:
    raise SystemExit("red insert point missing")
text = text.replace(needle, insert, 1)
out.write_text(text + "\n" + append.read_text())
print(f"red_test_bytes={out.stat().st_size}")
PY
ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$RED/host-cargo-procs.txt" || true
host_n=$(grep -c . "$RED/host-cargo-procs.txt" || true)
docker ps --filter label=oo.kind=rust-builder --format '{{.Names}}' > "$RED/builders-before.txt" || true
builder_n=$(grep -c . "$RED/builders-before.txt" || true)
global_n=$((host_n + builder_n))
printf 'host_cargo=%s rust_builders=%s global=%s cap=6\n' "$host_n" "$builder_n" "$global_n" > "$RED/capacity.txt"
if [ "$global_n" -ge 6 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$RED/docker-run.exit"
  exit 3
fi
own=$(docker ps -q --filter label=oo.owner=a196 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$RED/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named red container already exists" >&2
  exit 2
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$RED/image.txt"
grep -q 'arch=arm64' "$RED/image.txt"
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL image=pinned-rust-1.94 bash /inside.sh" > "$RED/invocation.txt"
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
  -e OO_A189_LIVE=1 \
  -e OO_A196_RED=1 \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${ROOT}/red-test.rs,target=/src/crates/oo-gateway/tests/query_executor_pg_live.rs,readonly" \
  --mount "type=bind,source=${RED}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${SEC},target=/run/oo-a189/fixture,readonly" \
  --tmpfs /tmp:rw,nosuid,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a196 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a196 \
  --label oo.task=cancel-transport-bounds-red \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$RED/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$RED/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME" 2>/dev/null || echo missing)"
  echo "PortBindings=$(docker inspect -f '{{json .HostConfig.PortBindings}}' "$NAME" 2>/dev/null || echo missing)"
} > "$RED/hostconfig.txt"
docker inspect -f '{{json .Mounts}}' "$NAME" > "$RED/mounts.txt" 2>/dev/null || true
docker rm "$NAME" > "$RED/container-removed.txt" 2>/dev/null || true
printf '%s\n' "$run_exit" > "$RED/red.exit"
exit "$run_exit"
