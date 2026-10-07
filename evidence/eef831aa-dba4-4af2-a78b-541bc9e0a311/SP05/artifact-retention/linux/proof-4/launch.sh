#!/bin/bash
# Proof-1. Fixed archive plus the artifact-retention overlay. One 6GiB/1CPU builder.
# Reuses the pre-edit volumes and network. Does not pull, prune, or start PostgreSQL.
# Does not overwrite an existing console.log.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a311/SP05/artifact-retention/linux/proof-4
PRE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a311/SP05/artifact-retention/linux/pre-edit
ROOT=/tmp/oo-a311-eef831aa-dba4-4af2-a78b-541bc9e0a311
FIXED=cab5e8e219d1924070df7336f5ab5d4ce7cf40dc
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a311-proof-4
NET=oo-a311-eef831aa-net
CARGO_VOL=oo-a311-eef831aa-cargo
TARGET_VOL=oo-a311-eef831aa-target
SESSION=eef831aa-dba4-4af2-a78b-541bc9e0a311
BASE=${ROOT}/pre-edit-archive
SNAP=${ROOT}/proof-archive-4

mkdir -p "$EVIDENCE/phases" "$ROOT"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite proof-1 console.log" >&2
  exit 2
fi
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"

{
  echo "pre-edit launch"
  echo "path evidence/eef831aa-dba4-4af2-a78b-541bc9e0a311/SP05/artifact-retention/linux/pre-edit/launch.sh"
  shasum -a 256 "$PRE/launch.sh"
  echo "change container pre-edit->proof-4; overlay tools/artifact-retention; same 6g 1cpu network volumes; no second archive fetch"
  echo "proof-2 preserved"
  echo "path evidence/eef831aa-dba4-4af2-a78b-541bc9e0a311/SP05/artifact-retention/linux/proof-2/console.log"
  echo "result docker-run.exit 101; lock checksum gate passed; bin could not call pub(crate) control_dir; digest as_slice is deprecated"
  echo "change keep the gated child lock; publish control_dir; compare digests with AsRef"
  echo "proof-3 preserved"
  echo "path evidence/eef831aa-dba4-4af2-a78b-541bc9e0a311/SP05/artifact-retention/linux/proof-3/console.log"
  echo "result docker-run.exit 101; prebuilt lock stayed locked; tests failed to compile"
  echo "change bin path accepts hyphen or underscore; Deployment is Debug; scratch clones the parent path"
  echo "proof-1 preserved"
  echo "path evidence/eef831aa-dba4-4af2-a78b-541bc9e0a311/SP05/artifact-retention/linux/proof-1/console.log"
  echo "result docker-run.exit 126 because /tmp is noexec and the emitter binary was placed there"
  echo "change emitter binary path /tmp/lock_from_baseline -> /target/lock_from_baseline"
  echo "a204 launch"
  echo "commit b2b3adabd05de94723d8ed13b33be0144a0de1f9"
  echo "path evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh"
  echo "sha256 08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841"
  echo "a200 launch"
  echo "commit a587031c719a484830d78e493605f48e2056cae4"
  echo "path evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/proof/launch.sh"
  echo "sha256 c57fcc9ee166c216b0d4786a2fefb1a0c3bab8dbedc9e2cba632100e516b7541"
  echo "a199 launch"
  echo "commit 5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9"
  echo "path evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/linux/overlay/launch.sh"
  echo "sha256 1212609e380ecea5b72033fa4eb936b61edcf33430f7fe8aa84d3549017c0537"
  echo "a209 smoke shape"
  echo "path evidence/eef831aa-dba4-4af2-a78b-541bc9e0a209/SP05/download-eof-review/pre-edit/inside.sh"
  echo "sha256 b5cf6e162266a3365bd7512378cc43de36b005b16c557b1a29d7d65f00245aa9"
} > "$EVIDENCE/recipe-origin.txt"

if [ ! -f "$BASE/Cargo.lock" ] || [ ! -f "$BASE/crates/oo-control/src/file_hosting.rs" ]; then
  echo "pre-edit archive is missing; not creating a second archive" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ -f "$BASE/rust-toolchain.toml" ]; then
  echo "pre-edit archive still has rust-toolchain.toml" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
rm -rf "$SNAP"
mkdir -p "$SNAP"
cp -a "$BASE/." "$SNAP/"
mkdir -p "$SNAP/tools"
rm -rf "$SNAP/tools/artifact-retention"
mkdir -p "$SNAP/tools/artifact-retention"
cp -a "$SRC/tools/artifact-retention/." "$SNAP/tools/artifact-retention/"
if [ ! -f "$SNAP/tools/artifact-retention/Cargo.lock" ]; then
  echo "proof-3 requires the gated child lock from proof-2" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
printf '%s\n' "$FIXED" > "$EVIDENCE/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/fixed-tree.txt"
git -C "$SRC" merge-base --is-ancestor "$FIXED" HEAD && echo ancestor=yes >> "$EVIDENCE/fixed-commit.txt"
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
(
  cd "$SNAP"
  shasum -a 256 Cargo.lock crates/oo-control/src/file_hosting.rs > "$EVIDENCE/archive-sha256.txt"
)
shasum -a 256 "$SRC/tools/artifact-retention/Cargo.toml" "$SRC/tools/artifact-retention/CONTRACT.md" > "$EVIDENCE/overlay-sha256-before.txt"

if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
grep -q 'os=linux' "$EVIDENCE/image.txt"

docker network inspect "$NET" >/dev/null 2>&1 || {
  echo "owned network is missing; not creating a second network in proof" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
}
docker network inspect --format 'name={{.Name}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || {
  echo "owned cargo volume is missing" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
}
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || {
  echo "owned target volume is missing" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
}
docker volume inspect "$CARGO_VOL" --format 'name={{.Name}}' > "$EVIDENCE/volume-cargo.txt"
docker volume inspect "$TARGET_VOL" --format 'name={{.Name}}' > "$EVIDENCE/volume-target.txt"

own=$(docker ps -q --filter label=oo.owner=a311 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" || $1=="clippy-driver" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1 postgres=0 memory=6g cpus=1\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/containers-before.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named proof container already exists" >&2
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
  --pids-limit 4096 \
  --network "$NET" \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${SNAP},target=/src" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=512m \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a311 \
  --label oo.session="$SESSION" \
  --label oo.task=artifact-retention-proof-4 \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME" 2>/dev/null || echo missing)"
  echo "ReadonlyRootfs=$(docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME" 2>/dev/null || echo missing)"
} > "$EVIDENCE/hostconfig.txt"
docker inspect -f '{{json .HostConfig.Tmpfs}}' "$NAME" > "$EVIDENCE/tmpfs.txt" 2>/dev/null || true
docker inspect -f '{{range .Mounts}}{{.Type}} {{.Source}} {{.Destination}} rw={{.RW}}{{println}}{{end}}' "$NAME" > "$EVIDENCE/mounts.txt" 2>/dev/null || true
docker rm "$NAME" > "$EVIDENCE/container-removed.txt" 2>/dev/null || true
if [ -d "$SNAP/tools/artifact-retention" ]; then
  cp -a "$SNAP/tools/artifact-retention/." "$SRC/tools/artifact-retention/"
fi
exit "$run_exit"
