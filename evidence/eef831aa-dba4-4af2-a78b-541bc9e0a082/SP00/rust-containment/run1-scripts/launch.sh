#!/bin/bash
# One linux/arm64 Rust 1.94 builder. Archive 5ef3d6d, overlay owned paths, fetch, then offline tests.
# Does not prune, does not mount the user home, and does not touch another task's container.
set -euo pipefail
EVIDENCE=/Users/redux/puppy-one/open-ontology/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a082/SP00/rust-containment
REPO=/Users/redux/puppy-one/open-ontology
COMMIT=5ef3d6d819fd72fdd5e86f79eb9e719910f7e6e0
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
RUN_ID=$(uuidgen | tr 'A-Z' 'a-z')
ROOT=/tmp/oo-a082-${RUN_ID}
SHORT=${RUN_ID%%-*}
NAME=oo-a082-rust194-${SHORT}
CARGO_VOL=oo-a082-cargo-${SHORT}
TARGET_VOL=oo-a082-target-${SHORT}
progress="$EVIDENCE/launch-progress.log"

say() { printf '%s %s\n' "$(date -u +%H:%M:%S)" "$*" | tee -a "$progress"; }

cleanup() {
  if [ -n "${NAME:-}" ]; then
    docker rm -f "$NAME" >>"$progress" 2>&1 || true
  fi
  if [ -n "${CARGO_VOL:-}" ]; then
    docker volume rm "$CARGO_VOL" >>"$progress" 2>&1 || true
  fi
  if [ -n "${TARGET_VOL:-}" ]; then
    docker volume rm "$TARGET_VOL" >>"$progress" 2>&1 || true
  fi
}
finish() {
  local code=$?
  if [ ! -f "$EVIDENCE/launch.exit" ]; then
    printf '%s\n' "$code" >"$EVIDENCE/launch.exit" || true
  fi
  cleanup
}
trap finish EXIT

mkdir -p "$EVIDENCE"
printf '%s\n' "$RUN_ID" >"$EVIDENCE/run-id.txt"
printf '%s\n' "$NAME" >"$EVIDENCE/container-name.txt"
printf '%s\n' "$ROOT" >"$EVIDENCE/snapshot-root.txt"
printf '%s\n' "$COMMIT" >"$EVIDENCE/archive-commit.txt"
: >"$progress"
say "snapshot start $ROOT"

rm -rf "$ROOT"
mkdir -p "$ROOT/snapshot" "$ROOT/export"
git -C "$REPO" archive --format=tar "$COMMIT" | tar -C "$ROOT/snapshot" -xf -
copy_tree() {
  local rel=$1
  rm -rf "$ROOT/snapshot/$rel"
  mkdir -p "$ROOT/snapshot/$rel"
  tar -C "$REPO/$rel" --exclude target -cf - . | tar -C "$ROOT/snapshot/$rel" -xf -
}
copy_tree testing/harness/containment-supervisor
cp "$REPO/testing/harness/README.md" "$ROOT/snapshot/testing/harness/README.md"
cp "$REPO/crates/xtask/src/containment.rs" "$ROOT/snapshot/crates/xtask/src/containment.rs"
cp "$REPO/crates/xtask/src/deny_toml.rs" "$ROOT/snapshot/crates/xtask/src/deny_toml.rs"
cp "$REPO/crates/xtask/src/runner_capture.rs" "$ROOT/snapshot/crates/xtask/src/runner_capture.rs"
cp "$REPO/crates/xtask/src/suites.rs" "$ROOT/snapshot/crates/xtask/src/suites.rs"

git -C "$ROOT/snapshot" init >>"$progress" 2>&1
git -C "$ROOT/snapshot" add -A >>"$progress" 2>&1
git -C "$ROOT/snapshot" -c user.email=a082@localhost -c user.name=a082 -c commit.gpgsign=false \
  commit -m "archive ${COMMIT} plus a082 owned overlay" >>"$progress" 2>&1
git -C "$ROOT/snapshot" rev-parse HEAD >"$EVIDENCE/snapshot-head.txt"
say "snapshot head $(cat "$EVIDENCE/snapshot-head.txt")"

# Cap-dropped root can write this bind only when the directory is writable by
# whoever the mount maps to. The helper directory created later is mode 0755.
chmod -R a+rwX "$ROOT/snapshot"

(
  cd "$ROOT/snapshot"
  find testing/harness/containment-supervisor crates/xtask/src/containment.rs \
    crates/xtask/src/deny_toml.rs crates/xtask/src/runner_capture.rs \
    crates/xtask/src/suites.rs testing/harness/README.md -type f -print \
    | sort | while IFS= read -r file; do shasum -a 256 "$file"; done
) >"$EVIDENCE/overlay.sha256"

say "builder check"
docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.kind"}} {{.Label "oo.owner"}}' >"$EVIDENCE/builders-observed.txt"
rust_builders=$(docker ps --filter label=oo.kind=rust-builder --format '{{.ID}} {{.Names}} {{.Label "oo.owner"}}')
image_builders=$(docker ps --format '{{.ID}} {{.Names}} {{.Image}}' | awk '/0f24958fa693|rust:1.94|library\/rust/ {print}')
host_cargo=$(ps -ax -o pid=,command= | awk '/[c]argo |[r]ustc / && $0 !~ /docker/ {print}')
{
  printf 'label_rust_builders:\n%s\n' "$rust_builders"
  printf 'image_builders:\n%s\n' "$image_builders"
  printf 'host_cargo:\n%s\n' "$host_cargo"
} >"$EVIDENCE/builder-count.txt"
label_n=$(printf '%s\n' "$rust_builders" | awk 'NF{n++} END{print n+0}')
image_n=$(printf '%s\n' "$image_builders" | awk 'NF{n++} END{print n+0}')
host_n=$(printf '%s\n' "$host_cargo" | awk 'NF{n++} END{print n+0}')
running_n=$image_n
if [ "$label_n" -gt "$running_n" ]; then running_n=$label_n; fi
global_n=$((running_n + host_n))
say "global_rust=$global_n image=$image_n label=$label_n host=$host_n"
if [ "$global_n" -ge 2 ]; then
  say "deferred: global rust builders already at $global_n"
  printf '%s\n' 3 >"$EVIDENCE/launch.exit"
  exit 3
fi

docker volume create "$CARGO_VOL" >>"$progress" 2>&1
docker volume create "$TARGET_VOL" >>"$progress" 2>&1
printf 'cargo=%s\ntarget=%s\n' "$CARGO_VOL" "$TARGET_VOL" >"$EVIDENCE/volumes.txt"

cat >"$EVIDENCE/invocation.txt" <<EOF
run_id=${RUN_ID}
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
container=${NAME}
cargo_volume=${CARGO_VOL}
target_volume=${TARGET_VOL}
snapshot=${ROOT}/snapshot
archive_commit=${COMMIT}
privileged=no
network_mode=bridge-then-disconnect
home_mount=no
socket_mount=no
shared_cargo_cache=no
pull=never
init=yes
user=0:0
cap_drop=ALL
security_opt=no-new-privileges
memory=8g
memory_swap=8g
cpus=4
pids_limit=4096
tmpfs=/tmp:rw,nosuid,nodev,noexec,size=2g
src_readonly=no
EOF

say "docker run"
set +e
docker run -d \
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
  --network bridge \
  --workdir /src \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=2g \
  -e CARGO_TERM_COLOR=never \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=4 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/tmp \
  --mount "type=bind,source=${ROOT}/snapshot,target=/src" \
  --mount "type=bind,source=${EVIDENCE},target=/out" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a082 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a082 \
  --label oo.task=rust-containment \
  --label oo.run_id="${RUN_ID}" \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  sleep 28800 >>"$progress" 2>&1
run_rc=$?
set -e
printf '%s\n' "$run_rc" >"$EVIDENCE/docker-run.exit"
if [ "$run_rc" -ne 0 ]; then
  say "docker run failed $run_rc"
  printf '%s\n' "$run_rc" >"$EVIDENCE/launch.exit"
  exit "$run_rc"
fi
printf 'started\n' >"$EVIDENCE/container-started.txt"
say "fetch"
set +e
docker exec "$NAME" bash /out/fetch.sh
fetch_rc=$?
set -e
printf '%s\n' "$fetch_rc" >"$EVIDENCE/fetch-exec.exit"
say "fetch exec $fetch_rc"

say "disconnect network"
set +e
nets=$(docker inspect -f '{{range $k, $v := .NetworkSettings.Networks}}{{$k}} {{end}}' "$NAME")
for net in $nets; do
  docker network disconnect "$net" "$NAME"
  say "disconnected $net $?"
done
set -e
docker inspect "$NAME" --format '{{json .NetworkSettings.Networks}}' >"$EVIDENCE/networks-after-disconnect.json"

say "test"
set +e
docker exec "$NAME" bash /out/test.sh
test_rc=$?
set -e
printf '%s\n' "$test_rc" >"$EVIDENCE/test-exec.exit"
say "test exec $test_rc"

docker inspect "$NAME" --format 'ExitCode={{.State.ExitCode}} Status={{.State.Status}} OOMKilled={{.State.OOMKilled}} Error={{.State.Error}}' >"$EVIDENCE/inspect.txt" || true
docker inspect "$NAME" --format '{{json .HostConfig.CapDrop}}' >"$EVIDENCE/cap-drop.json" || true
docker inspect "$NAME" --format 'Privileged={{.HostConfig.Privileged}} NetworkMode={{.HostConfig.NetworkMode}} ReadonlyRootfs={{.HostConfig.ReadonlyRootfs}}' >"$EVIDENCE/hostconfig.txt" || true

say "export artifacts"
set +e
docker cp "$NAME:/src/testing/harness/containment-supervisor" "$ROOT/export/containment-supervisor"
docker cp "$NAME:/src/crates/xtask/src/containment.rs" "$ROOT/export/containment.rs"
docker cp "$NAME:/src/crates/xtask/src/deny_toml.rs" "$ROOT/export/deny_toml.rs"
docker cp "$NAME:/src/crates/xtask/src/runner_capture.rs" "$ROOT/export/runner_capture.rs"
docker cp "$NAME:/src/crates/xtask/src/suites.rs" "$ROOT/export/suites.rs"
docker cp "$NAME:/src/testing/harness/README.md" "$ROOT/export/README.md"
set -e
say "export done"

# Critical phase exits. 0 is success. A missing file is recorded as missing.
fail=0
for phase in supply-check helper-test helper-clippy helper-build pin xtask-lib xtask-cli xtask-clippy probe; do
  if [ -f "$EVIDENCE/${phase}.exit" ]; then
    code=$(tr -d '[:space:]' <"$EVIDENCE/${phase}.exit")
  else
    code=missing
    fail=1
  fi
  say "phase $phase $code"
  if [ "$code" != 0 ]; then fail=1; fi
done
printf '%s\n' "$fail" >"$EVIDENCE/launch.exit"
say "launch exit $fail"
exit "$fail"
