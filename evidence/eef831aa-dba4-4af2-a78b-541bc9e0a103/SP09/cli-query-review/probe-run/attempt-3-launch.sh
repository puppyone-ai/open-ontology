#!/bin/bash
# Second probe attempt. Reuses the smoke cargo and target volumes. No fetch.
set -u
ROOT="/Users/redux/puppy-one/open-ontology"
EVID="$ROOT/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a103/SP09/cli-query-review"
WORK="/tmp/oo-a103-445980e9-64e0-402a-a0a1-d8a8d6bd6f6b"
PROBE_SNAP="$WORK/probe"
SMOKE_SNAP="$WORK/smoke"
VOL_CARGO="oo-a103-445980e9-64e0-402a-a0a1-d8a8d6bd6f6b-cargo"
VOL_TARGET="oo-a103-445980e9-64e0-402a-a0a1-d8a8d6bd6f6b-target"
IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
EXPECT_LOCK_SHA="40664b41001d01db59d272e0d9fab3977842becc88998bd52eefaa0c1b7a7147"
UUID="$(uuidgen | tr '[:upper:]' '[:lower:]')"
CNAME="oo-a103-probe-$UUID"
OUT="$EVID/probe-run/attempt-3"
mkdir -p "$OUT/phases"
printf '%s\n' "$CNAME" > "$OUT/container-name.txt"

builders=$(docker ps --filter label=oo.kind=rust-builder --format '{{.Names}}' | wc -l | tr -d ' ')
own=$(docker ps --filter label=oo.owner=a103 --filter label=oo.kind=rust-builder --format '{{.Names}}' | wc -l | tr -d ' ')
host_n=$(ps -ax -o comm= | awk 'tolower($0) ~ /^(cargo|rustc|rustdoc|rustup)$/' | wc -l | tr -d ' ')
global=$((builders + host_n))
{
  echo "labeled_rust_builders=$builders"
  echo "own_rust_builders=$own"
  echo "host_toolchain_comms=$host_n"
  echo "global=$global"
} > "$OUT/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global" -ge 2 ]; then
  echo "capacity_full_before_start" >> "$OUT/capacity.txt"
  printf 'not_started\n' > "$OUT/docker-run.exit"
  exit 2
fi

lock_sha="$(shasum -a 256 "$PROBE_SNAP/Cargo.lock" | awk '{print $1}')"
if [ "$lock_sha" != "$EXPECT_LOCK_SHA" ]; then
  echo "lock_mismatch=$lock_sha" > "$OUT/identity.txt"
  printf 'lock_mismatch\n' > "$OUT/docker-run.exit"
  exit 4
fi
if ! cmp -s "$SMOKE_SNAP/crates/oo-cli/Cargo.toml" "$PROBE_SNAP/crates/oo-cli/Cargo.toml"; then
  printf 'cargo_toml_diverged\n' > "$OUT/docker-run.exit"
  exit 5
fi
if ! cmp -s "$SMOKE_SNAP/Cargo.lock" "$PROBE_SNAP/Cargo.lock"; then
  printf 'lock_diverged\n' > "$OUT/docker-run.exit"
  exit 5
fi

cp "$EVID/probe/a103_probe.rs" "$PROBE_SNAP/crates/oo-cli/tests/a103_probe.rs"
shasum -a 256 "$EVID/probe/a103_probe.rs" > "$EVID/probe/a103_probe.attempt-2.sha256"
{
  echo "lock_sha256=$lock_sha"
  echo "cargo_toml_unchanged=yes"
  echo "archive_unchanged=yes"
  echo "added_file=crates/oo-cli/tests/a103_probe.rs"
  echo "fetch=not_run"
  echo "network=none"
} > "$OUT/identity.txt"

set +e
docker run \
  --name "$CNAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --cap-drop ALL \
  --security-opt no-new-privileges:true \
  --memory 8g \
  --memory-swap 8g \
  --cpus 4 \
  --pids-limit 4096 \
  --network none \
  --label oo.owner=a103 \
  --label oo.kind=rust-builder \
  --label oo.run="$UUID" \
  -e HOME=/tmp \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_HOME=/cargo \
  -e CARGO_TARGET_DIR=/target \
  -e A103_REPORT=/out/probe-report.json \
  -e A103_CASES=/out/cases \
  --mount "type=bind,source=${PROBE_SNAP},target=/src,readonly" \
  --mount "type=bind,source=${EVID}/probe/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${OUT},target=/out" \
  --mount "type=volume,source=${VOL_CARGO},target=/cargo" \
  --mount "type=volume,source=${VOL_TARGET},target=/target" \
  "$IMAGE" \
  bash /inside.sh
probe_ec=$?
set -u
printf '%s\n' "$probe_ec" > "$OUT/docker-run.exit"
docker inspect "$CNAME" --format 'ExitCode={{.State.ExitCode}} OOMKilled={{.State.OOMKilled}} Error={{.State.Error}} Privileged={{.HostConfig.Privileged}} NetworkMode={{.HostConfig.NetworkMode}} CapDrop={{json .HostConfig.CapDrop}} CapAdd={{json .HostConfig.CapAdd}} SecurityOpt={{json .HostConfig.SecurityOpt}} Memory={{.HostConfig.Memory}} MemorySwap={{.HostConfig.MemorySwap}} NanoCpus={{.HostConfig.NanoCpus}} PidsLimit={{.HostConfig.PidsLimit}} Binds={{json .HostConfig.Binds}}' > "$OUT/inspect.txt"
docker inspect "$CNAME" --format '{{json .HostConfig.Tmpfs}}' > "$OUT/tmpfs.txt"
docker inspect "$CNAME" --format '{{range .Mounts}}{{.Type}} dst={{.Destination}} rw={{.RW}} name={{.Name}}{{println}}{{end}}' > "$OUT/mounts.txt"
docker rm -f "$CNAME" > "$OUT/container-rm.txt" 2>&1
printf '%s\n' "$?" >> "$OUT/container-rm.txt"
exit "$probe_ec"
