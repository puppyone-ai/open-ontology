#!/bin/bash
# Build the standalone trial-report-adapter on the sealed 8f192847 archive.
# sql-runner and trial-statistics come from that archive. The adapter is overlaid.
# Does not overwrite an existing console.log. Does not prune.
# Origin: a225 impl-4 launch 934c8098703b6462c4f774878d1a4241bd84905d
# evidence/eef831aa-dba4-4af2-a78b-541bc9e0a225/SP12/trial-statistics/linux/impl-4/launch.sh
# sha256 6c1e508ab64eb04408189f12f3617e30cea6e5c69453bc936550b10e6d28817a
# Delta: owner a250, fixed 8f192847, crate trial-report-adapter, archive also
# contains benchmarks/sql-runner and benchmarks/trial-statistics, 6 GiB, one CPU.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a250/SP12/trial-report-adapter/linux/impl
ROOT=/tmp/oo-a250-eef831aa-dba4-4af2-a78b-541bc9e0a250/impl
CRATE=${SRC}/benchmarks/trial-report-adapter
FIXED=8f192847ea7c4906c13f8419b5de2669875f7ef9
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a250-impl
NET=oo-a250-eef831aa-net
CARGO_VOL=oo-a250-eef831aa-cargo
TARGET_VOL=oo-a250-eef831aa-target

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite impl console.log" >&2
  exit 2
fi
rm -rf "$ROOT/archive"
mkdir -p "$ROOT/archive"
git -C "$SRC" archive --format=tar "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates \
  benchmarks/sql-runner benchmarks/trial-statistics \
  | tar -x -C "$ROOT/archive"
if [ ! -f "$ROOT/archive/crates/oo-core/src/json_scan.rs" ]; then
  echo "archive missing oo-core json_scan.rs" >&2
  exit 2
fi
if [ ! -f "$ROOT/archive/benchmarks/sql-runner/Cargo.lock" ]; then
  echo "archive missing sql-runner lock" >&2
  exit 2
fi
if [ ! -f "$ROOT/archive/benchmarks/trial-statistics/Cargo.lock" ]; then
  echo "archive missing trial-statistics lock" >&2
  exit 2
fi
if [ -e "$ROOT/archive/benchmarks/trial-report-adapter" ]; then
  echo "fixed archive already contains trial-report-adapter" >&2
  exit 2
fi
mkdir -p "$ROOT/archive/benchmarks"
cp -R "$CRATE" "$ROOT/archive/benchmarks/trial-report-adapter"
rm -rf "$ROOT/archive/benchmarks/trial-report-adapter/target"
find "$ROOT/archive/benchmarks/trial-report-adapter" -name '.DS_Store' -delete
if [ -f "$ROOT/archive/benchmarks/trial-report-adapter/Cargo.lock" ]; then
  echo "adapter lock must be generated inside the fixed archive" >&2
  exit 2
fi
shasum -a 256 "$ROOT/archive/rust-toolchain.toml" > "$EVIDENCE/toolchain-file.sha256"
mv "$ROOT/archive/rust-toolchain.toml" "$ROOT/archive/rust-toolchain.toml.repo"
(
  cd "$ROOT/archive/benchmarks/trial-report-adapter"
  find . -type f ! -path './target/*' -print | sort | while IFS= read -r file; do
    shasum -a 256 "$file"
  done
) > "$EVIDENCE/source-sha256.txt"
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/fixed-tree.txt"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a250 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a250 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a250 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a250 \
  --label oo.kind=cargo-home \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a250 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a250 \
  --label oo.kind=cargo-target \
  "$TARGET_VOL" >/dev/null
own=$(docker ps -q --filter label=oo.owner=a250 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/containers-before.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named impl container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 bash /inside.sh" > "$EVIDENCE/invocation.txt"
set +e
docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory 6g \
  --memory-swap 6g \
  --cpus 1 \
  --pids-limit 2048 \
  --network "$NET" \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${ROOT}/archive,target=/src" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a250 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a250 \
  --label oo.task=trial-report-adapter-impl \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME")"
  echo "ReadonlyRootfs=$(docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' "$NAME")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
} > "$EVIDENCE/hostconfig.txt"
docker inspect -f '{{json .HostConfig.Tmpfs}}' "$NAME" > "$EVIDENCE/tmpfs.txt"
docker inspect -f '{{range .Mounts}}{{.Type}} {{.Source}} {{.Destination}} rw={{.RW}}{{println}}{{end}}' "$NAME" > "$EVIDENCE/mounts.txt"
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
if [ -f "$ROOT/archive/benchmarks/trial-report-adapter/Cargo.lock" ]; then
  cp "$ROOT/archive/benchmarks/trial-report-adapter/Cargo.lock" "$EVIDENCE/Cargo.lock"
fi
exit "$run_exit"
