#!/bin/bash
# a103 independent review launcher.
# Archives only the bound commit. One builder at a time. Volumes stay for a probe retry.
set -u
ROOT="/Users/redux/puppy-one/open-ontology"
EVID="$ROOT/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a103/SP09/cli-query-review"
COMMIT="3347f5a280da52c70525879b4608170b6b92b523"
EXPECT_TREE="0e9f38551ea6f88c78ff980efbf2eec40c0da4ab"
EXPECT_PARENT="1b9110265aae92bdd7eb02a71dcbeacf648375a5"
EXPECT_LOCK_SHA="40664b41001d01db59d272e0d9fab3977842becc88998bd52eefaa0c1b7a7147"
EXPECT_LOCK_BLOB="887cef930ba2fdadf3541aded755dc594e5bd1d7"
IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
UUID="$(uuidgen | tr '[:upper:]' '[:lower:]')"
WORK="/tmp/oo-a103-$UUID"
SMOKE_SNAP="$WORK/smoke"
PROBE_SNAP="$WORK/probe"
SMOKE_OUT="$EVID/linux-smoke/out"
PROBE_OUT="$EVID/probe-run/attempt-1"
NET="oo-a103-$UUID-net"
C_SMOKE="oo-a103-msrv-$UUID"
C_PROBE="oo-a103-probe-$UUID"
VOL_CARGO="oo-a103-$UUID-cargo"
VOL_TARGET="oo-a103-$UUID-target"

mkdir -p "$SMOKE_OUT/phases" "$PROBE_OUT/phases" "$WORK"
printf '%s\n' "$UUID" > "$EVID/run-id.txt"
printf '%s\n' "$WORK" > "$EVID/work-path.txt"
printf '%s\n' "$C_SMOKE" > "$EVID/smoke-container.txt"
printf '%s\n' "$C_PROBE" > "$EVID/probe-container.txt"

finish() {
  printf '%s\n' "$1" > "$EVID/launch.exit"
  exit "$1"
}

{
  printf 'run_id=%s\n' "$UUID"
  printf 'commit=%s\n' "$COMMIT"
  printf 'image=%s\n' "$IMAGE"
  printf 'platform=linux/arm64\n'
  printf 'toolchain=1.94.0\n'
  printf 'msrv_override=RUSTUP_TOOLCHAIN=1.94.0\n'
  printf 'pull=never\n'
  printf 'cap_drop=ALL\n'
  printf 'cap_add=none\n'
  printf 'init=yes\n'
  printf 'no_new_privileges=yes\n'
  printf 'memory=8g\n'
  printf 'cpus=4\n'
  printf 'pids_limit=4096\n'
  printf 'home_mount=no\n'
  printf 'socket_mount=no\n'
  printf 'privileged=no\n'
  printf 'smoke_network=bridge_not_host\n'
  printf 'probe_network=none\n'
  printf 'snapshot_method=git archive of bound commit only\n'
  printf 'smoke_command=cargo fetch --manifest-path Cargo.toml --locked; cargo test --manifest-path Cargo.toml --locked --offline -p oo-cli; cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-cli --all-targets -- -D warnings\n'
  printf 'probe_command=cargo test --manifest-path Cargo.toml --locked --offline -p oo-cli --test a103_probe -- --test-threads=1 --nocapture\n'
} > "$EVID/invocation.txt"

{
  echo "host_uname=$(uname -m)"
  echo "running_containers_before:"
  docker ps --format 'name={{.Names}} status={{.Status}} owner={{.Label "oo.owner"}} kind={{.Label "oo.kind"}}'
  echo "host_toolchain_comms:"
  ps -ax -o pid=,ppid=,etime=,comm= | awk 'tolower($4) ~ /^(cargo|rustc|rustdoc|rustup)$/ { print }'
  echo "note=capacity counted by container label and comm only"
} > "$EVID/capacity.txt"

builders=$(docker ps --filter label=oo.kind=rust-builder --format '{{.Names}}' | wc -l | tr -d ' ')
own=$(docker ps --filter label=oo.owner=a103 --filter label=oo.kind=rust-builder --format '{{.Names}}' | wc -l | tr -d ' ')
host_n=$(ps -ax -o comm= | awk 'tolower($0) ~ /^(cargo|rustc|rustdoc|rustup)$/' | wc -l | tr -d ' ')
global=$((builders + host_n))
{
  echo "labeled_rust_builders=$builders"
  echo "own_rust_builders=$own"
  echo "host_toolchain_comms=$host_n"
  echo "global=$global"
} >> "$EVID/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global" -ge 2 ]; then
  echo "capacity_full_before_start" >> "$EVID/capacity.txt"
  printf 'not_started\n' > "$EVID/linux-smoke/docker-run.exit"
  finish 2
fi

actual_commit="$(git -C "$ROOT" rev-parse "$COMMIT")"
actual_tree="$(git -C "$ROOT" rev-parse "${COMMIT}^{tree}")"
actual_parent="$(git -C "$ROOT" rev-parse "${COMMIT}^")"
actual_blob="$(git -C "$ROOT" rev-parse "${COMMIT}:Cargo.lock")"
if [ "$actual_commit" != "$COMMIT" ] || [ "$actual_tree" != "$EXPECT_TREE" ] || [ "$actual_parent" != "$EXPECT_PARENT" ] || [ "$actual_blob" != "$EXPECT_LOCK_BLOB" ]; then
  {
    echo "identity_mismatch"
    echo "commit=$actual_commit"
    echo "tree=$actual_tree"
    echo "parent=$actual_parent"
    echo "lock_blob=$actual_blob"
  } > "$EVID/input-refs.txt"
  printf 'identity_mismatch\n' > "$EVID/linux-smoke/docker-run.exit"
  finish 3
fi

git -C "$ROOT" archive --format=tar -o "$WORK/delivery.tar" "$COMMIT"
shasum -a 256 "$WORK/delivery.tar" > "$EVID/archive.tar.sha256"
mkdir -p "$SMOKE_SNAP" "$PROBE_SNAP"
tar -xf "$WORK/delivery.tar" -C "$SMOKE_SNAP"
tar -xf "$WORK/delivery.tar" -C "$PROBE_SNAP"
lock_sha="$(shasum -a 256 "$SMOKE_SNAP/Cargo.lock" | awk '{print $1}')"
if [ "$lock_sha" != "$EXPECT_LOCK_SHA" ]; then
  echo "lock_sha=$lock_sha" > "$EVID/input-refs.txt"
  printf 'lock_mismatch\n' > "$EVID/linux-smoke/docker-run.exit"
  finish 4
fi

cp "$EVID/probe/a103_probe.rs" "$PROBE_SNAP/crates/oo-cli/tests/a103_probe.rs"
if ! cmp -s "$SMOKE_SNAP/crates/oo-cli/Cargo.toml" "$PROBE_SNAP/crates/oo-cli/Cargo.toml"; then
  printf 'cargo_toml_diverged\n' > "$EVID/linux-smoke/docker-run.exit"
  finish 5
fi
if ! cmp -s "$SMOKE_SNAP/Cargo.lock" "$PROBE_SNAP/Cargo.lock"; then
  printf 'lock_diverged\n' > "$EVID/linux-smoke/docker-run.exit"
  finish 5
fi

{
  echo "commit=$actual_commit"
  echo "tree=$actual_tree"
  echo "parent=$actual_parent"
  echo "lock_blob=$actual_blob"
  echo "lock_sha256=$lock_sha"
  echo "archive_method=git archive of bound commit only"
  echo "worktree_not_copied=yes"
  echo "cargo_toml_probe_matches_smoke=yes"
  echo "subject=$(git -C "$ROOT" log -1 --format=%s "$COMMIT")"
  echo "author_date=$(git -C "$ROOT" log -1 --format=%aI "$COMMIT")"
} > "$EVID/input-refs.txt"
(
  cd "$SMOKE_SNAP"
  find crates/oo-cli contracts/cli Cargo.lock -type f -print0 | sort -z | xargs -0 shasum -a 256
) > "$EVID/linux-smoke/overlay-files.sha256"
shasum -a 256 "$EVID/probe/a103_probe.rs" > "$EVID/probe/a103_probe.rs.sha256"
shasum -a 256 "$EVID/linux-smoke/inside.sh" > "$EVID/linux-smoke/inside.sha256"
shasum -a 256 "$EVID/probe/inside.sh" > "$EVID/probe/inside.sha256"

docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$EVID/linux-smoke/image.txt"
if ! docker network create --label oo.owner=a103 --label oo.run="$UUID" --label oo.kind=rust-builder "$NET" > "$EVID/linux-smoke/network-create.txt"; then
  printf 'network_create_failed\n' > "$EVID/linux-smoke/docker-run.exit"
  finish 1
fi
if ! docker volume create --label oo.owner=a103 --label oo.run="$UUID" --label oo.kind=rust-builder "$VOL_CARGO" > "$EVID/linux-smoke/volume-cargo.txt"; then
  printf 'volume_cargo_failed\n' > "$EVID/linux-smoke/docker-run.exit"
  docker network rm "$NET" >/dev/null 2>&1 || true
  finish 1
fi
if ! docker volume create --label oo.owner=a103 --label oo.run="$UUID" --label oo.kind=rust-builder "$VOL_TARGET" > "$EVID/linux-smoke/volume-target.txt"; then
  printf 'volume_target_failed\n' > "$EVID/linux-smoke/docker-run.exit"
  docker volume rm "$VOL_CARGO" >/dev/null 2>&1 || true
  docker network rm "$NET" >/dev/null 2>&1 || true
  finish 1
fi

inspect_container() {
  local name="$1"
  local dest="$2"
  docker inspect "$name" --format 'ExitCode={{.State.ExitCode}} OOMKilled={{.State.OOMKilled}} Error={{.State.Error}} Privileged={{.HostConfig.Privileged}} NetworkMode={{.HostConfig.NetworkMode}} CapDrop={{json .HostConfig.CapDrop}} CapAdd={{json .HostConfig.CapAdd}} SecurityOpt={{json .HostConfig.SecurityOpt}} Memory={{.HostConfig.Memory}} MemorySwap={{.HostConfig.MemorySwap}} NanoCpus={{.HostConfig.NanoCpus}} PidsLimit={{.HostConfig.PidsLimit}} Binds={{json .HostConfig.Binds}}' > "$dest/inspect.txt"
  docker inspect "$name" --format '{{json .HostConfig.Tmpfs}}' > "$dest/tmpfs.txt"
  docker inspect "$name" --format '{{range .Mounts}}{{.Type}} dst={{.Destination}} rw={{.RW}} name={{.Name}}{{println}}{{end}}' > "$dest/mounts.txt"
}

set +e
docker run \
  --name "$C_SMOKE" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --cap-drop ALL \
  --security-opt no-new-privileges:true \
  --memory 8g \
  --memory-swap 8g \
  --cpus 4 \
  --pids-limit 4096 \
  --network "$NET" \
  --label oo.owner=a103 \
  --label oo.kind=rust-builder \
  --label oo.run="$UUID" \
  -e HOME=/tmp \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_HOME=/cargo \
  -e CARGO_TARGET_DIR=/target \
  --mount "type=bind,source=${SMOKE_SNAP},target=/src,readonly" \
  --mount "type=bind,source=${EVID}/linux-smoke/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${SMOKE_OUT},target=/out" \
  --mount "type=volume,source=${VOL_CARGO},target=/cargo" \
  --mount "type=volume,source=${VOL_TARGET},target=/target" \
  "$IMAGE" \
  bash /inside.sh
smoke_ec=$?
set -u
printf '%s\n' "$smoke_ec" > "$EVID/linux-smoke/docker-run.exit"
inspect_container "$C_SMOKE" "$EVID/linux-smoke"
docker rm -f "$C_SMOKE" > "$EVID/linux-smoke/container-rm.txt" 2>&1
smoke_rm=$?
printf '%s\n' "$smoke_rm" >> "$EVID/linux-smoke/container-rm.txt"
docker network rm "$NET" > "$EVID/linux-smoke/network-rm.txt" 2>&1
printf '%s\n' "$?" >> "$EVID/linux-smoke/network-rm.txt"

fetch_ec=1
if [ -f "$SMOKE_OUT/phases/fetch.exit" ]; then
  fetch_ec="$(cat "$SMOKE_OUT/phases/fetch.exit")"
fi
if [ "$fetch_ec" != "0" ]; then
  printf 'probe_not_run fetch_exit=%s\n' "$fetch_ec" > "$PROBE_OUT/phases/probe.exit"
  {
    echo "volumes_kept=$VOL_CARGO $VOL_TARGET"
    echo "work_kept=$WORK"
    echo "smoke_container_rm=$smoke_rm"
    echo "probe=not_started"
    echo "reason=fetch_not_zero"
  } > "$EVID/remaining.txt"
  finish "$smoke_ec"
fi

set +e
docker run \
  --name "$C_PROBE" \
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
  --mount "type=bind,source=${PROBE_OUT},target=/out" \
  --mount "type=volume,source=${VOL_CARGO},target=/cargo" \
  --mount "type=volume,source=${VOL_TARGET},target=/target" \
  "$IMAGE" \
  bash /inside.sh
probe_ec=$?
set -u
printf '%s\n' "$probe_ec" > "$PROBE_OUT/docker-run.exit"
inspect_container "$C_PROBE" "$PROBE_OUT"
docker rm -f "$C_PROBE" > "$PROBE_OUT/container-rm.txt" 2>&1
printf '%s\n' "$?" >> "$PROBE_OUT/container-rm.txt"

{
  echo "volumes_kept=$VOL_CARGO $VOL_TARGET"
  echo "work_kept=$WORK"
  echo "smoke_docker_exit=$smoke_ec"
  echo "probe_docker_exit=$probe_ec"
  echo "cleanup=deferred"
} > "$EVID/remaining.txt"

if [ "$smoke_ec" -ne 0 ]; then
  finish "$smoke_ec"
fi
finish "$probe_ec"
