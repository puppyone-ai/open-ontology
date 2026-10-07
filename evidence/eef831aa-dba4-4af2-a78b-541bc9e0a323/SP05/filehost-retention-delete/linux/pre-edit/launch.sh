#!/bin/bash
# Pre-edit smoke. Fixed archive only. One 6GiB/1CPU builder. No PostgreSQL.
# Does not overwrite an existing console.log. Does not prune. Does not pull.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a323/SP05/filehost-retention-delete/linux/pre-edit
ROOT=/tmp/oo-a323-eef831aa-dba4-4af2-a78b-541bc9e0a323
FIXED=895864a5bf64e50cef84075d20fd7b0c9e41c264
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a323-pre-edit
NET=oo-a323-eef831aa-net
CARGO_VOL=oo-a323-eef831aa-cargo
TARGET_VOL=oo-a323-eef831aa-target
SESSION=eef831aa-dba4-4af2-a78b-541bc9e0a323
SNAP=${ROOT}/pre-edit-archive

mkdir -p "$EVIDENCE/phases" "$ROOT"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite pre-edit console.log" >&2
  exit 2
fi
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"

{
  echo "a204 launch"
  echo "commit b2b3adabd05de94723d8ed13b33be0144a0de1f9"
  echo "path evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh"
  echo "sha256 08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841"
  echo "change memory 8g->6g cpus 4->1 global cap 6->10 owner a204->a323 package file_hosting smoke no review overlay"
  echo "a200 launch"
  echo "commit a587031c719a484830d78e493605f48e2056cae4"
  echo "path evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/proof/launch.sh"
  echo "sha256 c57fcc9ee166c216b0d4786a2fefb1a0c3bab8dbedc9e2cba632100e516b7541"
  echo "change proof launch not used for this smoke; private network and volume labels kept, postgres omitted"
  echo "a199 launch"
  echo "commit 5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9"
  echo "path evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/linux/overlay/launch.sh"
  echo "sha256 1212609e380ecea5b72033fa4eb936b61edcf33430f7fe8aa84d3549017c0537"
  echo "change no python hashlib; smoke hash is shasum/git; product oracles are Rust sha2"
  echo "a209 smoke shape"
  echo "path evidence/eef831aa-dba4-4af2-a78b-541bc9e0a209/SP05/download-eof-review/pre-edit/inside.sh"
  echo "sha256 b5cf6e162266a3365bd7512378cc43de36b005b16c557b1a29d7d65f00245aa9"
  echo "change same file_hosting phases on baseline 895864a5; 6g 1cpu; toolchain file moved aside"
  echo "a311 pre-edit launch"
  echo "commit 726606d2382e99977d4e843bf36c27110505dee7"
  echo "path evidence/eef831aa-dba4-4af2-a78b-541bc9e0a311/SP05/artifact-retention/linux/pre-edit/launch.sh"
  echo "sha256 eb535b1a11e16aea9d120d5cfe8039d1b7b6354d796ba274a30c8744e822b662"
  echo "change owner a311->a323 fixed cab5e8e2->895864a5 evidence path filehost-retention-delete"
  echo "a311 pre-edit inside"
  echo "path evidence/eef831aa-dba4-4af2-a78b-541bc9e0a311/SP05/artifact-retention/linux/pre-edit/inside.sh"
  echo "sha256 2b2255f5a0f050b4ec232f60d04bc9f8d292c9c9ed8c62bda2b2c51358a8d81f"
  echo "change task name only; same file_hosting phases and overlay-absent gate"
} > "$EVIDENCE/recipe-origin.txt"

rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$SRC" archive --format=tar "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates contracts \
  | tar -x -C "$SNAP"
if [ ! -f "$SNAP/crates/oo-control/src/file_hosting.rs" ]; then
  echo "archive missing file_hosting.rs" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ -e "$SNAP/tools/artifact-retention" ]; then
  echo "archive already contains the overlay" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ -f "$SNAP/rust-toolchain.toml" ]; then
  mv "$SNAP/rust-toolchain.toml" "$SNAP/rust-toolchain.toml.aside"
fi
printf '%s\n' "$FIXED" > "$EVIDENCE/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/fixed-tree.txt"
(
  cd "$SNAP"
  shasum -a 256 \
    Cargo.toml Cargo.lock \
    crates/oo-control/src/file_hosting.rs \
    crates/oo-control/src/lib.rs \
    crates/oo-control/tests/file_hosting.rs \
    crates/oo-control/Cargo.toml \
    contracts/control/local-file-hosting-v1.md \
    > "$EVIDENCE/archive-sha256.txt"
)
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"

if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
grep -q 'os=linux' "$EVIDENCE/image.txt"

docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a323 \
  --label oo.session="$SESSION" \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format 'name={{.Name}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a323 \
  --label oo.session="$SESSION" \
  --label oo.kind=cargo-home \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a323 \
  --label oo.session="$SESSION" \
  --label oo.kind=cargo-target \
  "$TARGET_VOL" >/dev/null
docker volume inspect "$CARGO_VOL" --format 'name={{.Name}}' > "$EVIDENCE/volume-cargo.txt"
docker volume inspect "$TARGET_VOL" --format 'name={{.Name}}' > "$EVIDENCE/volume-target.txt"

own=$(docker ps -q --filter label=oo.owner=a323 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
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
  echo "named pre-edit container already exists" >&2
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
  --label oo.owner=a323 \
  --label oo.session="$SESSION" \
  --label oo.task=filehost-retention-delete-pre-edit \
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
exit "$run_exit"
